<?php

declare(strict_types=1);

final readonly class HeavyWorkloadSummary
{
    public function __construct(
        public int $batches,
        public int $affectedRows,
        public float $elapsedSeconds,
    ) {
    }
}

final class HeavyProcess
{
    private int $completedBatches = 0;
    private ?HeavyWorkloadSummary $summary = null;
    private ?string $workerError = null;

    /** @param resource $socket */
    public function __construct(
        private readonly int $pid,
        private $socket,
    ) {
    }

    public function awaitReady(int $timeoutSeconds): void
    {
        stream_set_blocking($this->socket, true);
        stream_set_timeout($this->socket, $timeoutSeconds);

        while (true) {
            $message = $this->readMessage();
            if ($message === null) {
                $metadata = stream_get_meta_data($this->socket);
                $reason = $metadata['timed_out'] ? 'timed out' : 'closed';
                throw new RuntimeException("Heavy worker signal channel {$reason} before READY");
            }

            $this->handleMessage($message);
            if (($message['type'] ?? null) === 'ready') {
                stream_set_blocking($this->socket, false);
                return;
            }

            if ($this->workerError !== null) {
                throw new RuntimeException("Heavy worker failed: {$this->workerError}");
            }
        }
    }

    public function refreshProgress(): int
    {
        stream_set_blocking($this->socket, false);
        while (($message = $this->readMessage()) !== null) {
            $this->handleMessage($message);
        }

        if ($this->workerError !== null) {
            throw new RuntimeException("Heavy worker failed: {$this->workerError}");
        }

        return $this->completedBatches;
    }

    public function wait(): HeavyWorkloadSummary
    {
        $waitedPid = pcntl_waitpid($this->pid, $status);
        if ($waitedPid !== $this->pid) {
            throw new RuntimeException('Failed to wait for heavy worker');
        }

        $this->refreshProgress();
        fclose($this->socket);

        if (!pcntl_wifexited($status) || pcntl_wexitstatus($status) !== 0) {
            $detail = $this->workerError ?? 'unknown worker error';
            throw new RuntimeException("Heavy worker exited unsuccessfully: {$detail}");
        }

        if ($this->summary === null) {
            throw new RuntimeException('Heavy worker exited without a summary');
        }

        return $this->summary;
    }

    /** @return array<string, mixed>|null */
    private function readMessage(): ?array
    {
        $line = fgets($this->socket);
        if ($line === false) {
            return null;
        }

        $message = json_decode($line, true, flags: JSON_THROW_ON_ERROR);
        if (!is_array($message)) {
            throw new RuntimeException('Invalid heavy worker message');
        }

        return $message;
    }

    /** @param array<string, mixed> $message */
    private function handleMessage(array $message): void
    {
        $type = $message['type'] ?? null;
        if ($type === 'batch' || $type === 'ready') {
            $this->completedBatches = max(
                $this->completedBatches,
                (int) ($message['batch'] ?? 0),
            );
        } elseif ($type === 'done') {
            $this->summary = new HeavyWorkloadSummary(
                batches: (int) $message['batches'],
                affectedRows: (int) $message['affected_rows'],
                elapsedSeconds: (float) $message['elapsed_seconds'],
            );
        } elseif ($type === 'error') {
            $this->workerError = (string) ($message['message'] ?? 'unknown error');
        }
    }
}

final class Workload
{
    public static function forkHeavyWorker(Config $config): HeavyProcess
    {
        $sockets = stream_socket_pair(
            STREAM_PF_UNIX,
            STREAM_SOCK_STREAM,
            STREAM_IPPROTO_IP,
        );
        if ($sockets === false) {
            throw new RuntimeException('Failed to create heavy worker signal channel');
        }

        [$parentSocket, $childSocket] = $sockets;
        $pid = pcntl_fork();
        if ($pid === -1) {
            fclose($parentSocket);
            fclose($childSocket);
            throw new RuntimeException('Failed to fork heavy worker');
        }

        if ($pid === 0) {
            fclose($parentSocket);
            $exitCode = self::runHeavyWorker($config, $childSocket);
            fclose($childSocket);
            exit($exitCode);
        }

        fclose($childSocket);
        return new HeavyProcess($pid, $parentSocket);
    }

    public static function updateMarker(PDO $source): int
    {
        $source->exec(<<<'SQL'
            UPDATE lag_demo.marker
            SET value = value + 1, updated_at = CURRENT_TIMESTAMP(6)
            WHERE id = 1
            SQL);

        return Database::markerValue($source);
    }

    /** @param resource $socket */
    private static function runHeavyWorker(Config $config, $socket): int
    {
        $readySent = false;

        try {
            $source = Database::connect($config);
            printf(
                "Starting %d heavy UPDATE batches on Source...\n",
                $config->totalHeavyBatches(),
            );
            $workloadStarted = hrtime(true);
            $firstId = 1;
            $batchNumber = 0;
            $affectedRows = 0;
            $statement = $source->prepare(<<<'SQL'
                UPDATE lag_demo.items
                SET
                    version = version + 1,
                    payload = CONCAT(
                        SHA2(CONCAT(payload, ':', id, ':', version), 256),
                        REPEAT('z', 448)
                    )
                WHERE id BETWEEN ? AND ?
                SQL);

            while ($firstId <= $config->demoRows) {
                $lastId = min($firstId + $config->heavyBatchSize - 1, $config->demoRows);
                $batchStarted = hrtime(true);
                $statement->execute([$firstId, $lastId]);
                ++$batchNumber;
                $affectedRows += $statement->rowCount();

                printf(
                    "  heavy batch %d/%d: rows %d..=%d, affected=%d, %.2fs\n",
                    $batchNumber,
                    $config->totalHeavyBatches(),
                    $firstId,
                    $lastId,
                    $statement->rowCount(),
                    self::elapsedSeconds($batchStarted),
                );
                self::send($socket, ['type' => 'batch', 'batch' => $batchNumber]);

                if ($batchNumber === $config->markerAfterBatches) {
                    self::send($socket, ['type' => 'ready', 'batch' => $batchNumber]);
                    $readySent = true;
                }

                $firstId = $lastId + 1;
            }

            self::send($socket, [
                'type' => 'done',
                'batches' => $batchNumber,
                'affected_rows' => $affectedRows,
                'elapsed_seconds' => self::elapsedSeconds($workloadStarted),
            ]);
            return 0;
        } catch (Throwable $throwable) {
            self::send($socket, [
                'type' => 'error',
                'before_ready' => !$readySent,
                'message' => $throwable->getMessage(),
            ]);
            fprintf(STDERR, "Heavy worker error: %s\n", $throwable->getMessage());
            return 1;
        }
    }

    /**
     * @param resource $socket
     * @param array<string, mixed> $message
     */
    private static function send($socket, array $message): void
    {
        $encoded = json_encode($message, JSON_THROW_ON_ERROR) . "\n";
        if (fwrite($socket, $encoded) === false) {
            throw new RuntimeException('Failed to send heavy worker progress');
        }
        fflush($socket);
    }

    private static function elapsedSeconds(int $started): float
    {
        return (hrtime(true) - $started) / 1_000_000_000;
    }
}

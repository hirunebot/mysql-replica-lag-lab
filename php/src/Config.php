<?php

declare(strict_types=1);

final readonly class Config
{
    public function __construct(
        public string $sourceDsn,
        public string $replicaDsn,
        public string $databaseUser,
        public string $databasePassword,
        public int $demoRows,
        public int $seedBatchSize,
        public int $heavyBatchSize,
        public int $markerAfterBatches,
        public int $pollIntervalMs,
        public int $initialSyncTimeoutSecs,
        public int $finalSyncTimeoutSecs,
    ) {
        $this->validate();
    }

    public static function fromEnvironment(string $projectRoot): self
    {
        self::loadEnvFile($projectRoot . '/.env');

        return new self(
            sourceDsn: self::envString(
                'PHP_SOURCE_DSN',
                'mysql:host=127.0.0.1;port=3307;charset=utf8mb4',
            ),
            replicaDsn: self::envString(
                'PHP_REPLICA_DSN',
                'mysql:host=127.0.0.1;port=3308;charset=utf8mb4',
            ),
            databaseUser: self::envString('PHP_DATABASE_USER', 'root'),
            databasePassword: self::envString('PHP_DATABASE_PASSWORD', 'root'),
            demoRows: self::envInt('DEMO_ROWS', 1_000_000),
            seedBatchSize: self::envInt('SEED_BATCH_SIZE', 1_000),
            heavyBatchSize: self::envInt('HEAVY_BATCH_SIZE', 50_000),
            markerAfterBatches: self::envInt('MARKER_AFTER_BATCHES', 5),
            pollIntervalMs: self::envInt('POLL_INTERVAL_MS', 500),
            initialSyncTimeoutSecs: self::envInt('INITIAL_SYNC_TIMEOUT_SECS', 600),
            finalSyncTimeoutSecs: self::envInt('FINAL_SYNC_TIMEOUT_SECS', 600),
        );
    }

    public function totalHeavyBatches(): int
    {
        return intdiv($this->demoRows + $this->heavyBatchSize - 1, $this->heavyBatchSize);
    }

    private function validate(): void
    {
        if ($this->sourceDsn === $this->replicaDsn) {
            throw new InvalidArgumentException(
                'PHP_SOURCE_DSN and PHP_REPLICA_DSN must be different',
            );
        }

        foreach ([
            'DEMO_ROWS' => $this->demoRows,
            'SEED_BATCH_SIZE' => $this->seedBatchSize,
            'HEAVY_BATCH_SIZE' => $this->heavyBatchSize,
            'MARKER_AFTER_BATCHES' => $this->markerAfterBatches,
            'POLL_INTERVAL_MS' => $this->pollIntervalMs,
            'INITIAL_SYNC_TIMEOUT_SECS' => $this->initialSyncTimeoutSecs,
            'FINAL_SYNC_TIMEOUT_SECS' => $this->finalSyncTimeoutSecs,
        ] as $name => $value) {
            if ($value <= 0) {
                throw new InvalidArgumentException("{$name} must be greater than zero");
            }
        }

        if ($this->markerAfterBatches > $this->totalHeavyBatches()) {
            throw new InvalidArgumentException(sprintf(
                'MARKER_AFTER_BATCHES (%d) exceeds the number of heavy batches (%d)',
                $this->markerAfterBatches,
                $this->totalHeavyBatches(),
            ));
        }
    }

    private static function envString(string $name, string $default): string
    {
        $value = getenv($name);

        return $value === false || $value === '' ? $default : $value;
    }

    private static function envInt(string $name, int $default): int
    {
        $value = getenv($name);
        if ($value === false || $value === '') {
            return $default;
        }

        if (filter_var($value, FILTER_VALIDATE_INT) === false) {
            throw new InvalidArgumentException("{$name} must be an integer: {$value}");
        }

        return (int) $value;
    }

    private static function loadEnvFile(string $path): void
    {
        if (!is_file($path)) {
            return;
        }

        $lines = file($path, FILE_IGNORE_NEW_LINES);
        if ($lines === false) {
            throw new RuntimeException("Failed to read {$path}");
        }

        foreach ($lines as $line) {
            $line = trim($line);
            if ($line === '' || str_starts_with($line, '#') || !str_contains($line, '=')) {
                continue;
            }

            [$name, $value] = array_map('trim', explode('=', $line, 2));
            if ($name === '' || getenv($name) !== false) {
                continue;
            }

            if (
                strlen($value) >= 2
                && (($value[0] === '"' && str_ends_with($value, '"'))
                    || ($value[0] === "'" && str_ends_with($value, "'")))
            ) {
                $value = substr($value, 1, -1);
            }

            putenv("{$name}={$value}");
            $_ENV[$name] = $value;
        }
    }
}

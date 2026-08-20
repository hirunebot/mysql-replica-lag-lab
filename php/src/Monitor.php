<?php

declare(strict_types=1);

final readonly class MarkerObservation
{
    public function __construct(
        public float $elapsedSeconds,
        public bool $staleReadObserved,
    ) {
    }
}

final class Monitor
{
    public static function waitForMarker(
        PDO $source,
        PDO $replica,
        int $expectedValue,
        HeavyProcess $heavyProcess,
        Config $config,
    ): MarkerObservation {
        $started = hrtime(true);
        $staleReadObserved = false;

        echo "\n";
        echo "elapsed | Source marker | Replica marker | lag(s) | relay(bytes) | read-pos | exec-pos | heavy\n";

        while (true) {
            $sourceMarker = Database::markerValue($source);
            $replicaMarker = Database::markerValue($replica);
            $status = Database::replicaStatus($replica);
            if ($status === null) {
                throw new RuntimeException('Replication status disappeared while monitoring');
            }

            $completedBatches = $heavyProcess->refreshProgress();
            $lag = $status->secondsBehindSource === null
                ? 'NULL'
                : (string) $status->secondsBehindSource;

            printf(
                "%6.2fs | %13d | %14d | %6s | %12d | %8d | %8d | %d/%d\n",
                self::elapsedSeconds($started),
                $sourceMarker,
                $replicaMarker,
                $lag,
                $status->relayLogSpace,
                $status->readSourceLogPos,
                $status->execSourceLogPos,
                $completedBatches,
                $config->totalHeavyBatches(),
            );

            if ($sourceMarker === $expectedValue && $replicaMarker !== $expectedValue) {
                $staleReadObserved = true;
            }

            if ($replicaMarker === $expectedValue) {
                return new MarkerObservation(
                    elapsedSeconds: self::elapsedSeconds($started),
                    staleReadObserved: $staleReadObserved,
                );
            }

            if (self::elapsedSeconds($started) >= $config->finalSyncTimeoutSecs) {
                throw new RuntimeException(sprintf(
                    'Marker synchronization timed out after %.2fs',
                    self::elapsedSeconds($started),
                ));
            }

            usleep($config->pollIntervalMs * 1_000);
        }
    }

    private static function elapsedSeconds(int $started): float
    {
        return (hrtime(true) - $started) / 1_000_000_000;
    }
}

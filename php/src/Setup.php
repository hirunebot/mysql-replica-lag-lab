<?php

declare(strict_types=1);

final class Setup
{
    public static function recreateSchema(PDO $source): void
    {
        echo "Recreating the lag_demo schema on Source...\n";

        $source->exec('DROP DATABASE IF EXISTS lag_demo');
        $source->exec('CREATE DATABASE lag_demo');
        $source->exec(<<<'SQL'
            CREATE TABLE lag_demo.items (
                id BIGINT UNSIGNED NOT NULL PRIMARY KEY,
                version BIGINT UNSIGNED NOT NULL,
                payload VARCHAR(1024) NOT NULL,
                updated_at TIMESTAMP(6) NOT NULL
                    DEFAULT CURRENT_TIMESTAMP(6)
                    ON UPDATE CURRENT_TIMESTAMP(6),
                KEY idx_version (version)
            ) ENGINE=InnoDB
            SQL);
        $source->exec(<<<'SQL'
            CREATE TABLE lag_demo.marker (
                id BIGINT UNSIGNED NOT NULL PRIMARY KEY,
                value BIGINT UNSIGNED NOT NULL,
                updated_at TIMESTAMP(6) NOT NULL
                    DEFAULT CURRENT_TIMESTAMP(6)
                    ON UPDATE CURRENT_TIMESTAMP(6)
            ) ENGINE=InnoDB
            SQL);
        $source->exec('INSERT INTO lag_demo.marker (id, value) VALUES (1, 0)');
    }

    public static function seed(PDO $source, Config $config): void
    {
        printf("Seeding %d rows on Source...\n", $config->demoRows);
        $started = hrtime(true);
        $firstId = 1;
        $progressStep = max(intdiv($config->demoRows, 20), $config->seedBatchSize);
        $nextProgress = $progressStep;

        while ($firstId <= $config->demoRows) {
            $lastId = min($firstId + $config->seedBatchSize - 1, $config->demoRows);
            $values = [];

            for ($id = $firstId; $id <= $lastId; ++$id) {
                $values[] = "({$id}, 1, REPEAT('x', 512))";
            }

            $source->exec(
                'INSERT INTO lag_demo.items (id, version, payload) VALUES '
                . implode(',', $values),
            );

            if ($lastId >= $nextProgress || $lastId === $config->demoRows) {
                printf(
                    "  seed: %d/%d (%.1f%%)\n",
                    $lastId,
                    $config->demoRows,
                    $lastId * 100 / $config->demoRows,
                );
                $nextProgress += $progressStep;
            }

            $firstId = $lastId + 1;
        }

        printf("Seed completed in %.2fs.\n", self::elapsedSeconds($started));
    }

    public static function waitForInitialSync(PDO $replica, Config $config): void
    {
        echo "Waiting for Replica to receive all initial rows...\n";
        $started = hrtime(true);
        $progressStep = max(intdiv($config->demoRows, 20), 1);
        $nextProgress = 0;
        $schemaWaitReported = false;

        while (true) {
            try {
                $count = Database::itemCount($replica);
                if ($count >= $nextProgress || $count === $config->demoRows) {
                    printf("  Replica seed rows: %d/%d\n", $count, $config->demoRows);
                    $nextProgress = $count + $progressStep;
                }

                if ($count === $config->demoRows && Database::markerValue($replica) === 0) {
                    printf(
                        "Initial synchronization completed in %.2fs.\n",
                        self::elapsedSeconds($started),
                    );
                    return;
                }
            } catch (PDOException $exception) {
                if (!$schemaWaitReported) {
                    printf("  Replica schema is not visible yet: %s\n", $exception->getMessage());
                    $schemaWaitReported = true;
                }
            }

            self::ensureNotTimedOut($started, $config->initialSyncTimeoutSecs, 'Initial sync');
            usleep($config->pollIntervalMs * 1_000);
        }
    }

    public static function waitForFinalSync(PDO $replica, Config $config): void
    {
        echo "Waiting for Replica to apply all remaining heavy updates...\n";
        $started = hrtime(true);
        $lastReported = null;

        while (true) {
            $count = Database::updatedItemCount($replica);
            if ($count !== $lastReported) {
                printf("  Replica updated rows: %d/%d\n", $count, $config->demoRows);
                $lastReported = $count;
            }

            if ($count === $config->demoRows) {
                printf(
                    "Final synchronization completed in %.2fs.\n",
                    self::elapsedSeconds($started),
                );
                return;
            }

            self::ensureNotTimedOut($started, $config->finalSyncTimeoutSecs, 'Final sync');
            usleep($config->pollIntervalMs * 1_000);
        }
    }

    private static function ensureNotTimedOut(
        int $started,
        int $timeoutSeconds,
        string $operation,
    ): void {
        $elapsed = self::elapsedSeconds($started);
        if ($elapsed >= $timeoutSeconds) {
            throw new RuntimeException(sprintf(
                '%s timed out after %.2fs',
                $operation,
                $elapsed,
            ));
        }
    }

    private static function elapsedSeconds(int $started): float
    {
        return (hrtime(true) - $started) / 1_000_000_000;
    }
}

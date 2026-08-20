#!/usr/bin/env php
<?php

declare(strict_types=1);

$projectRoot = dirname(__DIR__, 2);

require_once __DIR__ . '/../src/Config.php';
require_once __DIR__ . '/../src/Database.php';
require_once __DIR__ . '/../src/Setup.php';
require_once __DIR__ . '/../src/Workload.php';
require_once __DIR__ . '/../src/Monitor.php';

try {
    assertRuntimeRequirements();
    $config = Config::fromEnvironment($projectRoot);
    printBanner($config);

    $source = Database::connect($config);
    $replica = Database::connect($config, replica: true);
    Database::ensureReplicationRunning($replica);

    Setup::recreateSchema($source);
    Setup::seed($source, $config);

    $sourceRows = Database::itemCount($source);
    if ($sourceRows !== $config->demoRows) {
        throw new RuntimeException(sprintf(
            'Source seed count mismatch: expected %d, got %d',
            $config->demoRows,
            $sourceRows,
        ));
    }
    Setup::waitForInitialSync($replica, $config);

    echo "\nInitial state is synchronized. Starting the lag scenario.\n";

    // Do not share PDO connections across fork. Parent and child reconnect below.
    $source = null;
    $replica = null;
    gc_collect_cycles();

    $heavyProcess = Workload::forkHeavyWorker($config);
    $source = Database::connect($config);
    $replica = Database::connect($config, replica: true);
    $heavyProcess->awaitReady($config->finalSyncTimeoutSecs);

    echo "\n";
    printf(
        "%d heavy batches committed. Updating the independent marker now...\n",
        $config->markerAfterBatches,
    );
    $markerStarted = hrtime(true);
    $expectedMarker = Workload::updateMarker($source);
    printf(
        "Source marker committed as value %d in %.3fs.\n",
        $expectedMarker,
        elapsedSeconds($markerStarted),
    );

    $observation = Monitor::waitForMarker(
        $source,
        $replica,
        $expectedMarker,
        $heavyProcess,
        $config,
    );
    $heavySummary = $heavyProcess->wait();

    echo "\n";
    if ($observation->staleReadObserved) {
        echo "SUCCESS: Replica returned the old marker after Source committed the new value.\n";
        printf(
            "The lightweight marker took %.2fs to become visible on Replica.\n",
            $observation->elapsedSeconds,
        );
    } else {
        echo "WARNING: Replica had already applied the marker at the first observation.\n";
        echo "Increase DEMO_ROWS or MARKER_AFTER_BATCHES, or lower REPLICA_CPUS, then retry.\n";
    }

    printf(
        "Heavy workload: %d batches, %d affected rows, %.2fs on Source.\n",
        $heavySummary->batches,
        $heavySummary->affectedRows,
        $heavySummary->elapsedSeconds,
    );

    Setup::waitForFinalSync($replica, $config);
    Database::ensureReplicationRunning($replica);
    $finalMarker = Database::markerValue($replica);
    if ($finalMarker !== $expectedMarker) {
        throw new RuntimeException(sprintf(
            'Final marker mismatch: Source=%d, Replica=%d',
            $expectedMarker,
            $finalMarker,
        ));
    }

    echo "Demo completed with Source and Replica in sync.\n";
    exit(0);
} catch (Throwable $throwable) {
    fprintf(STDERR, "Error: %s\n", $throwable->getMessage());
    exit(1);
}

function assertRuntimeRequirements(): void
{
    if (PHP_SAPI !== 'cli') {
        throw new RuntimeException('This demo must run with PHP CLI');
    }

    if (PHP_VERSION_ID < 80200) {
        throw new RuntimeException('PHP 8.2 or later is required');
    }

    foreach (['pdo', 'pdo_mysql', 'pcntl'] as $extension) {
        if (!extension_loaded($extension)) {
            throw new RuntimeException("Required PHP extension is missing: {$extension}");
        }
    }

    foreach (['pcntl_fork', 'pcntl_waitpid', 'stream_socket_pair'] as $function) {
        if (!function_exists($function)) {
            throw new RuntimeException("Required PHP function is missing: {$function}");
        }
    }
}

function printBanner(Config $config): void
{
    echo "mysql-replica-lag-lab (PHP)\n";
    printf("  rows                 : %d\n", $config->demoRows);
    printf("  seed batch size      : %d\n", $config->seedBatchSize);
    printf("  heavy batch size     : %d\n", $config->heavyBatchSize);
    printf(
        "  marker after batches : %d / %d\n",
        $config->markerAfterBatches,
        $config->totalHeavyBatches(),
    );
    printf("  poll interval        : %dms\n", $config->pollIntervalMs);
}

function elapsedSeconds(int $started): float
{
    return (hrtime(true) - $started) / 1_000_000_000;
}

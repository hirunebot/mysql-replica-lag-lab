#!/usr/bin/env php
<?php

declare(strict_types=1);

require_once __DIR__ . '/../src/Config.php';
require_once __DIR__ . '/../src/Workload.php';
require_once __DIR__ . '/../src/Monitor.php';

$tests = [
    'CFG-01 calculates a partial final batch' => function (): void {
        assertSameValue(6, validConfig()->totalHeavyBatches());
    },
    'CFG-02 rejects identical Source and Replica DSNs' => function (): void {
        assertThrows(
            fn () => validConfig(['replicaDsn' => 'mysql:source']),
            InvalidArgumentException::class,
        );
    },
    'CFG-03 rejects zero workload values' => function (): void {
        foreach ([
            'demoRows',
            'seedBatchSize',
            'heavyBatchSize',
            'markerAfterBatches',
            'pollIntervalMs',
            'initialSyncTimeoutSecs',
            'finalSyncTimeoutSecs',
        ] as $field) {
            assertThrows(
                fn () => validConfig([$field => 0]),
                InvalidArgumentException::class,
            );
        }
    },
    'CFG-04 rejects marker position after the final batch' => function (): void {
        assertThrows(
            fn () => validConfig(['markerAfterBatches' => 7]),
            InvalidArgumentException::class,
        );
    },
    'BAT-01 and BAT-02 cover all rows without overlap' => function (): void {
        $ranges = [];
        for ($batch = 1; $batch <= 3; ++$batch) {
            $ranges[] = Workload::batchRange($batch, 41, 20);
        }

        assertSameValue([[1, 20], [21, 40], [41, 41]], $ranges);
    },
    'BAT-03 rejects invalid and excess batch numbers' => function (): void {
        assertSameValue(null, Workload::batchRange(0, 41, 20));
        assertSameValue(null, Workload::batchRange(4, 41, 20));
        assertSameValue(null, Workload::batchRange(1, 0, 20));
        assertSameValue(null, Workload::batchRange(1, 41, 0));
    },
    'MRK-01 and MRK-02 retain stale state after catch-up' => function (): void {
        $tracker = new MarkerTracker();

        assertSameValue(false, $tracker->observe(1, 0, 1));
        assertSameValue(true, $tracker->staleReadObserved());
        assertSameValue(true, $tracker->observe(1, 1, 1));
        assertSameValue(true, $tracker->staleReadObserved());
    },
    'MRK-03 does not report stale when Replica is immediately current' => function (): void {
        $tracker = new MarkerTracker();

        assertSameValue(true, $tracker->observe(1, 1, 1));
        assertSameValue(false, $tracker->staleReadObserved());
    },
];

$failures = 0;
foreach ($tests as $name => $test) {
    try {
        $test();
        printf("ok - %s\n", $name);
    } catch (Throwable $throwable) {
        ++$failures;
        fprintf(STDERR, "not ok - %s: %s\n", $name, $throwable->getMessage());
    }
}

printf("\n%d tests, %d failures\n", count($tests), $failures);
exit($failures === 0 ? 0 : 1);

/** @param array<string, mixed> $overrides */
function validConfig(array $overrides = []): Config
{
    $values = array_replace([
        'sourceDsn' => 'mysql:source',
        'replicaDsn' => 'mysql:replica',
        'databaseUser' => 'root',
        'databasePassword' => 'root',
        'demoRows' => 101,
        'seedBatchSize' => 10,
        'heavyBatchSize' => 20,
        'markerAfterBatches' => 2,
        'pollIntervalMs' => 100,
        'initialSyncTimeoutSecs' => 1,
        'finalSyncTimeoutSecs' => 1,
    ], $overrides);

    return new Config(...$values);
}

function assertSameValue(mixed $expected, mixed $actual): void
{
    if ($expected !== $actual) {
        throw new RuntimeException(sprintf(
            'expected %s, got %s',
            var_export($expected, true),
            var_export($actual, true),
        ));
    }
}

/** @param class-string<Throwable> $expectedClass */
function assertThrows(callable $callback, string $expectedClass): void
{
    try {
        $callback();
    } catch (Throwable $throwable) {
        if ($throwable instanceof $expectedClass) {
            return;
        }

        throw new RuntimeException(sprintf(
            'expected %s, got %s',
            $expectedClass,
            $throwable::class,
        ));
    }

    throw new RuntimeException("expected {$expectedClass} to be thrown");
}

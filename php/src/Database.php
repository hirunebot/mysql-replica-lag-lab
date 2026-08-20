<?php

declare(strict_types=1);

final readonly class ReplicaStatus
{
    public function __construct(
        public string $ioRunning,
        public string $sqlRunning,
        public ?int $secondsBehindSource,
        public int $relayLogSpace,
        public int $readSourceLogPos,
        public int $execSourceLogPos,
    ) {
    }
}

final class Database
{
    public static function connect(Config $config, bool $replica = false): PDO
    {
        $label = $replica ? 'Replica' : 'Source';
        $dsn = $replica ? $config->replicaDsn : $config->sourceDsn;

        try {
            return new PDO(
                $dsn,
                $config->databaseUser,
                $config->databasePassword,
                [
                    PDO::ATTR_ERRMODE => PDO::ERRMODE_EXCEPTION,
                    PDO::ATTR_DEFAULT_FETCH_MODE => PDO::FETCH_ASSOC,
                    PDO::ATTR_EMULATE_PREPARES => false,
                    PDO::ATTR_TIMEOUT => 10,
                ],
            );
        } catch (PDOException $exception) {
            throw new RuntimeException("Failed to connect to {$label}", 0, $exception);
        }
    }

    public static function ensureReplicationRunning(PDO $replica): void
    {
        $status = self::replicaStatus($replica);
        if ($status === null) {
            throw new RuntimeException(
                'Replication is not configured; run ./scripts/configure-replication.sh first',
            );
        }

        if ($status->ioRunning !== 'Yes' || $status->sqlRunning !== 'Yes') {
            throw new RuntimeException(sprintf(
                'Replication is not running: IO=%s, SQL=%s',
                $status->ioRunning,
                $status->sqlRunning,
            ));
        }
    }

    public static function replicaStatus(PDO $replica): ?ReplicaStatus
    {
        $statement = $replica->query('SHOW REPLICA STATUS');
        $row = $statement->fetch();
        if ($row === false) {
            return null;
        }

        return new ReplicaStatus(
            ioRunning: (string) $row['Replica_IO_Running'],
            sqlRunning: (string) $row['Replica_SQL_Running'],
            secondsBehindSource: $row['Seconds_Behind_Source'] === null
                ? null
                : (int) $row['Seconds_Behind_Source'],
            relayLogSpace: (int) $row['Relay_Log_Space'],
            readSourceLogPos: (int) $row['Read_Source_Log_Pos'],
            execSourceLogPos: (int) $row['Exec_Source_Log_Pos'],
        );
    }

    public static function markerValue(PDO $database): int
    {
        return (int) $database
            ->query('SELECT value FROM lag_demo.marker WHERE id = 1')
            ->fetchColumn();
    }

    public static function itemCount(PDO $database): int
    {
        return (int) $database
            ->query('SELECT COUNT(*) FROM lag_demo.items')
            ->fetchColumn();
    }

    public static function updatedItemCount(PDO $database): int
    {
        return (int) $database
            ->query('SELECT COUNT(*) FROM lag_demo.items WHERE version = 2')
            ->fetchColumn();
    }
}

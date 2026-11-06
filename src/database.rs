use std::time::Duration;

use crate::queries;
use anyhow::{Context, Result, bail};
use sqlx::{MySqlConnection, MySqlExecutor, MySqlPool, Row, mysql::MySqlPoolOptions};

const READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) struct SchemaPending;

impl std::fmt::Display for SchemaPending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the current demo schema is not ready on Replica")
    }
}

impl std::error::Error for SchemaPending {}

#[derive(Debug)]
pub struct ReplicaStatus {
    pub io_running: String,
    pub sql_running: String,
    pub seconds_behind_source: Option<u64>,
    pub relay_log_space: u64,
    pub read_source_log_pos: u64,
    pub exec_source_log_pos: u64,
    pub source_log_file: String,
    pub relay_source_log_file: String,
}

pub(crate) async fn connect(url: &str, label: &str, max: u32) -> Result<MySqlPool> {
    MySqlPoolOptions::new()
        .max_connections(max)
        .acquire_timeout(Duration::from_secs(10))
        .connect(url)
        .await
        .with_context(|| format!("failed to connect to {label}"))
}

pub(crate) fn read_pool(url: &str, max: u32) -> Result<MySqlPool> {
    Ok(MySqlPoolOptions::new()
        .max_connections(max)
        .acquire_timeout(READ_TIMEOUT)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET SESSION max_execution_time = 4500, lock_wait_timeout = 4, transaction_read_only = ON")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_lazy(url)?)
}

pub(crate) async fn bounded_read<T>(
    pool: &MySqlPool,
    read: impl AsyncFnOnce(&mut MySqlConnection) -> Result<T>,
) -> Result<T> {
    let deadline = tokio::time::Instant::now() + READ_TIMEOUT;
    let mut connection = tokio::time::timeout_at(deadline, pool.acquire())
        .await
        .map_err(|_| sqlx::Error::PoolTimedOut)??;
    match tokio::time::timeout_at(deadline, read(&mut connection)).await {
        Ok(result) => result,
        Err(error) => {
            // キャンセルした結果セットをflushする処理にプールの枠を占有させない。
            connection.close_on_drop();
            Err(error).context("database read timed out")
        }
    }
}

pub(crate) async fn executed_gtids<'a>(connection: impl MySqlExecutor<'a>) -> Result<String> {
    Ok(sqlx::query_scalar("SELECT @@GLOBAL.gtid_executed")
        .fetch_one(connection)
        .await?)
}

pub(crate) async fn has_executed<'a>(
    connection: impl MySqlExecutor<'a>,
    gtids: &str,
) -> Result<bool> {
    let value: i64 = sqlx::query_scalar("SELECT GTID_SUBSET(?, @@GLOBAL.gtid_executed)")
        .bind(gtids)
        .fetch_one(connection)
        .await?;
    Ok(value == 1)
}

pub async fn ensure_replication_running(replica: &MySqlPool) -> Result<()> {
    let Some(status) = replica_status(replica).await? else {
        bail!("replication is not configured; run ./scripts/configure-replication.sh first");
    };

    if status.io_running != "Yes" || status.sql_running != "Yes" {
        bail!(
            "replication is not running: IO={}, SQL={}; inspect SHOW REPLICA STATUS",
            status.io_running,
            status.sql_running
        );
    }

    Ok(())
}

pub async fn replica_status<'a>(replica: impl MySqlExecutor<'a>) -> Result<Option<ReplicaStatus>> {
    let Some(row) = sqlx::query("SHOW REPLICA STATUS")
        .fetch_optional(replica)
        .await
        .context("failed to read replica status")?
    else {
        return Ok(None);
    };

    // SQLx does not expose names for SHOW REPLICA STATUS columns consistently,
    // so use the stable MySQL 8.4 result-set positions instead.
    Ok(Some(ReplicaStatus {
        io_running: row.try_get(10)?,
        sql_running: row.try_get(11)?,
        seconds_behind_source: row.try_get(32)?,
        relay_log_space: row.try_get(22)?,
        read_source_log_pos: row.try_get(6)?,
        exec_source_log_pos: row.try_get(21)?,
        source_log_file: row.try_get(5)?,
        relay_source_log_file: row.try_get(9)?,
    }))
}

pub async fn marker_value<'a>(connection: impl MySqlExecutor<'a>) -> Result<u64> {
    let value: u64 = sqlx::query_scalar(queries::MARKER_VALUE)
        .fetch_one(connection)
        .await
        .context("failed to read marker")?;
    Ok(value)
}

pub async fn item_count<'a>(connection: impl MySqlExecutor<'a>) -> Result<u64> {
    let count: i64 = sqlx::query_scalar(queries::ITEM_COUNT)
        .fetch_one(connection)
        .await
        .context("failed to count items")?;
    Ok(count as u64)
}

pub async fn updated_item_count<'a>(connection: impl MySqlExecutor<'a>) -> Result<u64> {
    let count: i64 = sqlx::query_scalar(queries::UPDATED_COUNT)
        .fetch_one(connection)
        .await
        .context("failed to count updated items")?;
    Ok(count as u64)
}

pub(crate) async fn updated_batch_count<'a>(
    connection: impl MySqlExecutor<'a>,
    first: u64,
    last: u64,
) -> Result<u64> {
    let count: i64 = sqlx::query_scalar(queries::UPDATED_BATCH_COUNT)
        .bind(first)
        .bind(last)
        .fetch_one(connection)
        .await?;
    Ok(count as u64)
}

pub(crate) async fn marker_sum(connection: &mut MySqlConnection) -> Result<Option<String>> {
    // DECIMALのSUMを精度を落とさず文字列化し、SQL NULLは保持する。
    Ok(
        sqlx::query_scalar("SELECT CAST(SUM(value) AS CHAR) FROM lag_demo.marker")
            .fetch_one(connection)
            .await?,
    )
}

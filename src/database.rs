use std::time::Duration;

use anyhow::{Context, Result, bail};
use sqlx::{MySqlPool, Row, mysql::MySqlPoolOptions};

#[derive(Debug)]
pub struct ReplicaStatus {
    pub io_running: String,
    pub sql_running: String,
    pub seconds_behind_source: Option<u64>,
    pub relay_log_space: u64,
    pub read_source_log_pos: u64,
    pub exec_source_log_pos: u64,
}

pub async fn connect(url: &str, label: &str) -> Result<MySqlPool> {
    MySqlPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(10))
        .connect(url)
        .await
        .with_context(|| format!("failed to connect to {label}"))
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

pub async fn replica_status(replica: &MySqlPool) -> Result<Option<ReplicaStatus>> {
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
    }))
}

pub async fn marker_value(pool: &MySqlPool) -> Result<u64> {
    let value: u64 = sqlx::query_scalar("SELECT value FROM lag_demo.marker WHERE id = 1")
        .fetch_one(pool)
        .await
        .context("failed to read marker")?;
    Ok(value)
}

pub async fn item_count(pool: &MySqlPool) -> Result<u64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lag_demo.items")
        .fetch_one(pool)
        .await
        .context("failed to count items")?;
    Ok(count as u64)
}

pub async fn updated_item_count(pool: &MySqlPool) -> Result<u64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lag_demo.items WHERE version = 2")
        .fetch_one(pool)
        .await
        .context("failed to count updated items")?;
    Ok(count as u64)
}

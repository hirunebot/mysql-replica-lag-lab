use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use anyhow::{Context, Result};
use sqlx::MySqlPool;
use tokio::{sync::oneshot, task::JoinHandle};

use crate::config::Config;

#[derive(Debug)]
pub struct HeavyWorkloadSummary {
    pub batches: u64,
    pub affected_rows: u64,
    pub elapsed_seconds: f64,
}

pub struct HeavyWorkload {
    pub ready: oneshot::Receiver<()>,
    pub progress: Arc<AtomicU64>,
    pub handle: JoinHandle<Result<HeavyWorkloadSummary>>,
}

pub fn spawn_heavy_workload(source: MySqlPool, config: Config) -> HeavyWorkload {
    let (ready_tx, ready_rx) = oneshot::channel();
    let progress = Arc::new(AtomicU64::new(0));
    let task_progress = Arc::clone(&progress);

    let handle =
        tokio::spawn(
            async move { run_heavy_workload(source, config, ready_tx, task_progress).await },
        );

    HeavyWorkload {
        ready: ready_rx,
        progress,
        handle,
    }
}

async fn run_heavy_workload(
    source: MySqlPool,
    config: Config,
    ready_tx: oneshot::Sender<()>,
    progress: Arc<AtomicU64>,
) -> Result<HeavyWorkloadSummary> {
    println!(
        "Starting {} heavy UPDATE batches on Source...",
        config.total_heavy_batches()
    );
    let workload_started = Instant::now();
    let mut first_id = 1;
    let mut batch_number = 0;
    let mut affected_rows = 0;
    let mut ready_tx = Some(ready_tx);

    while first_id <= config.demo_rows {
        let last_id = (first_id + config.heavy_batch_size - 1).min(config.demo_rows);
        let batch_started = Instant::now();
        let result = sqlx::query(
            r#"
            UPDATE lag_demo.items
            SET
                version = version + 1,
                payload = CONCAT(
                    SHA2(CONCAT(payload, ':', id, ':', version), 256),
                    REPEAT('z', 448)
                )
            WHERE id BETWEEN ? AND ?
            "#,
        )
        .bind(first_id)
        .bind(last_id)
        .execute(&source)
        .await
        .with_context(|| format!("heavy batch failed for rows {first_id}..={last_id}"))?;

        batch_number += 1;
        affected_rows += result.rows_affected();
        progress.store(batch_number, Ordering::Relaxed);

        println!(
            "  heavy batch {batch_number}/{}: rows {first_id}..={last_id}, affected={}, {:.2}s",
            config.total_heavy_batches(),
            result.rows_affected(),
            batch_started.elapsed().as_secs_f64()
        );

        if batch_number == config.marker_after_batches
            && let Some(sender) = ready_tx.take()
        {
            let _ = sender.send(());
        }

        first_id = last_id + 1;
    }

    Ok(HeavyWorkloadSummary {
        batches: batch_number,
        affected_rows,
        elapsed_seconds: workload_started.elapsed().as_secs_f64(),
    })
}

pub async fn update_marker(source: &MySqlPool) -> Result<u64> {
    sqlx::query(
        "UPDATE lag_demo.marker SET value = value + 1, updated_at = CURRENT_TIMESTAMP(6) WHERE id = 1",
    )
    .execute(source)
    .await
    .context("failed to update marker on Source")?;

    crate::database::marker_value(source).await
}

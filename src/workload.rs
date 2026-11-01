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
    let mut affected_rows = 0;
    let mut ready_tx = Some(ready_tx);

    for batch_number in 1..=config.total_heavy_batches() {
        let (first_id, last_id) =
            batch_range(batch_number, config.demo_rows, config.heavy_batch_size)
                .context("failed to calculate heavy batch range")?;
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
    }

    Ok(HeavyWorkloadSummary {
        batches: config.total_heavy_batches(),
        affected_rows,
        elapsed_seconds: workload_started.elapsed().as_secs_f64(),
    })
}

pub(crate) fn batch_range(
    batch_number: u64,
    total_rows: u64,
    batch_size: u64,
) -> Option<(u64, u64)> {
    if batch_number == 0 || total_rows == 0 || batch_size == 0 {
        return None;
    }

    let first_id = batch_number
        .checked_sub(1)?
        .checked_mul(batch_size)?
        .checked_add(1)?;
    if first_id > total_rows {
        return None;
    }

    let last_id = first_id
        .saturating_add(batch_size.saturating_sub(1))
        .min(total_rows);
    Some((first_id, last_id))
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

#[cfg(test)]
mod tests {
    use super::batch_range;

    #[test]
    fn batch_ranges_cover_all_rows_without_overlap() {
        let ranges: Vec<_> = (1..=3)
            .map(|batch| batch_range(batch, 41, 20).unwrap())
            .collect();

        assert_eq!(ranges, vec![(1, 20), (21, 40), (41, 41)]);
    }

    #[test]
    fn batch_range_rejects_invalid_or_excess_batch() {
        assert_eq!(batch_range(0, 41, 20), None);
        assert_eq!(batch_range(4, 41, 20), None);
        assert_eq!(batch_range(1, 0, 20), None);
        assert_eq!(batch_range(1, 41, 0), None);
    }
}

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use anyhow::{Context, Result, ensure};
use sqlx::MySqlPool;
use tokio::sync::{oneshot, watch};

use crate::{
    config::Config,
    queries,
    runner::{Journal, batch_range},
};

pub(crate) struct HeavyWorkloadSummary {
    pub batches: u64,
    pub affected_rows: u64,
    pub elapsed_seconds: f64,
}

pub(crate) async fn run_heavy_workload(
    source: &MySqlPool,
    config: &Config,
    j: &Journal,
    ready: oneshot::Sender<()>,
    progress: &AtomicU64,
    stop: &watch::Sender<bool>,
) -> Result<HeavyWorkloadSummary> {
    let total = config.total_heavy_batches();
    j.line(format_args!(
        "Starting {total} heavy UPDATE batches on Source..."
    ));
    let started = Instant::now();
    let mut affected_rows = 0;
    let mut ready = Some(ready);
    for n in 1..=total {
        ensure!(!*stop.borrow(), "heavy workload stopped");
        let (first, last) = batch_range(n, config.demo_rows, config.heavy_batch_size)
            .context("invalid heavy batch range")?;
        let batch_started = Instant::now();
        let id = format!("batch-{n}");
        let rows = j
            .operation(&id, async {
                let statement = queries::heavy_update(first, last);
                j.statement(&id, statement.clone(), Some((first, last)));
                queries::execute(source, &statement)
                    .await
                    .with_context(|| format!("heavy batch failed for rows {first}..={last}"))
            })
            .await?;
        affected_rows += rows;
        progress.store(n, Ordering::Relaxed);
        j.line(format_args!(
            "  heavy batch {n}/{total}: rows {first}..={last}, affected={rows}, {:.2}s",
            batch_started.elapsed().as_secs_f64()
        ));
        if n == config.marker_after_batches
            && let Some(ready) = ready.take()
        {
            let _ = ready.send(());
        }
    }
    Ok(HeavyWorkloadSummary {
        batches: total,
        affected_rows,
        elapsed_seconds: started.elapsed().as_secs_f64(),
    })
}

pub(crate) async fn update_marker(source: &MySqlPool, j: &Journal) -> Result<u64> {
    let statement = queries::marker_update();
    j.statement("marker", statement.clone(), Some((1, 1)));
    queries::execute(source, &statement)
        .await
        .context("failed to update marker on Source")
}

#[cfg(test)]
mod tests {
    use crate::runner::batch_range;

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

use std::time::Instant;

use anyhow::{Context, Result, bail};
use sqlx::MySqlPool;

use crate::{
    config::Config,
    contracts::{
        DatabaseTarget::Replica,
        ObservationMetric::{Marker, TotalRows, UpdatedRows},
    },
    database, queries,
    runner::{Journal, batch_range},
};

pub async fn recreate_schema(source: &MySqlPool, j: &Journal) -> Result<u64> {
    j.line(format_args!("Recreating the lag_demo schema on Source..."));
    let mut affected = 0;
    for sql in queries::SETUP {
        j.statement("setup", queries::statement(sql), None);
        affected += sqlx::query(sql)
            .execute(source)
            .await
            .context("failed to initialize lag_demo")?
            .rows_affected();
    }
    Ok(affected)
}

pub async fn seed(source: &MySqlPool, config: &Config, j: &Journal) -> Result<u64> {
    j.line(format_args!(
        "Seeding {} rows on Source...",
        config.demo_rows
    ));
    let started = Instant::now();
    let progress_step = (config.demo_rows / 20).max(config.seed_batch_size);
    let mut next_progress = progress_step;
    let mut affected = 0;
    for batch in 1..=config.demo_rows.div_ceil(config.seed_batch_size) {
        let (first_id, last_id) = batch_range(batch, config.demo_rows, config.seed_batch_size)
            .context("invalid seed batch")?;
        let statement = queries::seed_insert(first_id, last_id);
        j.statement("seed", statement.clone(), Some((first_id, last_id)));
        affected += sqlx::query(&statement.template)
            .execute(source)
            .await
            .with_context(|| format!("failed to seed rows {first_id}..={last_id}"))?
            .rows_affected();
        j.seed_progress(affected);
        if last_id >= next_progress || last_id == config.demo_rows {
            j.line(format_args!(
                "  seed: {last_id}/{} ({:.1}%)",
                config.demo_rows,
                last_id as f64 * 100.0 / config.demo_rows as f64
            ));
            next_progress = next_progress.saturating_add(progress_step);
        }
    }
    j.line(format_args!(
        "Seed completed in {:.2}s.",
        started.elapsed().as_secs_f64()
    ));
    Ok(affected)
}

pub async fn wait_for_initial_sync(
    replica: &MySqlPool,
    config: &Config,
    j: &Journal,
) -> Result<()> {
    j.line(format_args!(
        "Waiting for Replica to receive all initial rows..."
    ));
    let started = Instant::now();
    let progress_step = (config.demo_rows / 20).max(1);
    let mut next_progress = 0;
    let mut schema_wait_reported = false;
    loop {
        match j
            .decimal(Replica, TotalRows, database::item_count(replica))
            .await
        {
            Ok(count) => {
                if count >= next_progress || count == config.demo_rows {
                    j.line(format_args!(
                        "  Replica seed rows: {count}/{}",
                        config.demo_rows
                    ));
                    next_progress = count.saturating_add(progress_step);
                }
                if count == config.demo_rows
                    && j.decimal(Replica, Marker, database::marker_value(replica))
                        .await?
                        == 0
                {
                    j.line(format_args!(
                        "Initial synchronization completed in {:.2}s.",
                        started.elapsed().as_secs_f64()
                    ));
                    return Ok(());
                }
            }
            Err(error) if !schema_wait_reported => {
                j.line(format_args!(
                    "  Replica schema is not visible yet: {error:#}"
                ));
                schema_wait_reported = true;
            }
            Err(_) => {}
        }
        if started.elapsed() >= config.initial_sync_timeout {
            bail!(
                "initial synchronization timed out after {:.2}s",
                started.elapsed().as_secs_f64()
            );
        }
        tokio::time::sleep(config.poll_interval).await;
    }
}

pub async fn wait_for_final_sync(replica: &MySqlPool, config: &Config, j: &Journal) -> Result<()> {
    j.line(format_args!(
        "Waiting for Replica to apply all remaining heavy updates..."
    ));
    let started = Instant::now();
    let mut last_reported = None;
    loop {
        let count = j
            .decimal(Replica, UpdatedRows, database::updated_item_count(replica))
            .await?;
        if last_reported != Some(count) {
            j.line(format_args!(
                "  Replica updated rows: {count}/{}",
                config.demo_rows
            ));
            last_reported = Some(count);
        }
        if count == config.demo_rows {
            j.line(format_args!(
                "Final synchronization completed in {:.2}s.",
                started.elapsed().as_secs_f64()
            ));
            return Ok(());
        }
        if started.elapsed() >= config.final_sync_timeout {
            bail!(
                "final synchronization timed out after {:.2}s",
                started.elapsed().as_secs_f64()
            );
        }
        tokio::time::sleep(config.poll_interval).await;
    }
}

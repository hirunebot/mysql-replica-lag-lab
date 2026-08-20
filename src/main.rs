mod config;
mod database;
mod monitor;
mod setup;
mod workload;

use std::time::Instant;

use anyhow::{Context, Result};

use crate::config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let config = Config::from_env()?;

    print_banner(&config);

    let (source, replica) = tokio::try_join!(
        database::connect(&config.source_database_url, "Source"),
        database::connect(&config.replica_database_url, "Replica"),
    )?;

    database::ensure_replication_running(&replica).await?;
    setup::recreate_schema(&source).await?;
    setup::seed(&source, &config).await?;

    let source_rows = database::item_count(&source).await?;
    anyhow::ensure!(
        source_rows == config.demo_rows,
        "Source seed count mismatch: expected {}, got {source_rows}",
        config.demo_rows
    );
    setup::wait_for_initial_sync(&replica, &config).await?;

    println!();
    println!("Initial state is synchronized. Starting the lag scenario.");

    let heavy = workload::spawn_heavy_workload(source.clone(), config.clone());
    heavy
        .ready
        .await
        .context("heavy workload stopped before the marker insertion point")?;

    println!();
    println!(
        "{} heavy batches committed. Updating the independent marker now...",
        config.marker_after_batches
    );
    let marker_started = Instant::now();
    let expected_marker = workload::update_marker(&source).await?;
    println!(
        "Source marker committed as value {expected_marker} in {:.3}s.",
        marker_started.elapsed().as_secs_f64()
    );

    let observation =
        monitor::wait_for_marker(&source, &replica, expected_marker, heavy.progress, &config)
            .await?;

    let heavy_summary = heavy
        .handle
        .await
        .context("heavy workload task panicked")??;

    println!();
    if observation.stale_read_observed {
        println!("SUCCESS: Replica returned the old marker after Source committed the new value.");
        println!(
            "The lightweight marker took {:.2}s to become visible on Replica.",
            observation.elapsed.as_secs_f64()
        );
    } else {
        println!("WARNING: Replica had already applied the marker at the first observation.");
        println!("Increase DEMO_ROWS or MARKER_AFTER_BATCHES, or lower REPLICA_CPUS, then retry.");
    }

    println!(
        "Heavy workload: {} batches, {} affected rows, {:.2}s on Source.",
        heavy_summary.batches, heavy_summary.affected_rows, heavy_summary.elapsed_seconds
    );

    setup::wait_for_final_sync(&replica, &config).await?;
    database::ensure_replication_running(&replica).await?;
    let final_marker = database::marker_value(&replica).await?;
    anyhow::ensure!(
        final_marker == expected_marker,
        "final marker mismatch: Source={expected_marker}, Replica={final_marker}"
    );

    println!("Demo completed with Source and Replica in sync.");
    Ok(())
}

fn print_banner(config: &Config) {
    println!("mysql-replica-lag-lab");
    println!("  rows                 : {}", config.demo_rows);
    println!("  seed batch size      : {}", config.seed_batch_size);
    println!("  heavy batch size     : {}", config.heavy_batch_size);
    println!(
        "  marker after batches : {} / {}",
        config.marker_after_batches,
        config.total_heavy_batches()
    );
    println!("  poll interval        : {:?}", config.poll_interval);
}

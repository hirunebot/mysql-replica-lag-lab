use std::{fmt, sync::Arc};

use anyhow::Result;
use mysql_replica_lag_lab::{
    config::Config,
    contracts::{EventPayload, RunConfig},
    events::Reporter,
    runner::{self, RunnerConfig},
};

struct CliReporter;

impl Reporter for CliReporter {
    fn report(&self, _event: EventPayload) {}
    fn line(&self, message: fmt::Arguments<'_>) {
        println!("{message}");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let config = Config::from_env()?;
    print_banner(&config);
    let id = format!("cli-{}", std::process::id());
    runner::run(
        RunnerConfig {
            server_instance_id: id.clone(),
            run_id: id,
            source_database_url: config.source_database_url,
            replica_database_url: config.replica_database_url,
            workload: RunConfig {
                demo_rows: config.demo_rows.to_string(),
                seed_batch_size: config.seed_batch_size.to_string(),
                heavy_batch_size: config.heavy_batch_size.to_string(),
                marker_after_batches: config.marker_after_batches.to_string(),
            },
            poll_interval: config.poll_interval,
            initial_sync_timeout: config.initial_sync_timeout,
            final_sync_timeout: config.final_sync_timeout,
        },
        Arc::new(CliReporter),
    )
    .await?;
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

use std::{fmt::Write as _, time::Instant};

use anyhow::{Context, Result, bail};
use sqlx::MySqlPool;

use crate::{config::Config, database};

pub async fn recreate_schema(source: &MySqlPool) -> Result<()> {
    println!("Recreating the lag_demo schema on Source...");

    sqlx::query("DROP DATABASE IF EXISTS lag_demo")
        .execute(source)
        .await
        .context("failed to drop lag_demo")?;
    sqlx::query("CREATE DATABASE lag_demo")
        .execute(source)
        .await
        .context("failed to create lag_demo")?;
    sqlx::query(
        r#"
        CREATE TABLE lag_demo.items (
            id BIGINT UNSIGNED NOT NULL PRIMARY KEY,
            version BIGINT UNSIGNED NOT NULL,
            payload VARCHAR(1024) NOT NULL,
            updated_at TIMESTAMP(6) NOT NULL
                DEFAULT CURRENT_TIMESTAMP(6)
                ON UPDATE CURRENT_TIMESTAMP(6),
            KEY idx_version (version)
        ) ENGINE=InnoDB
        "#,
    )
    .execute(source)
    .await
    .context("failed to create items table")?;
    sqlx::query(
        r#"
        CREATE TABLE lag_demo.marker (
            id BIGINT UNSIGNED NOT NULL PRIMARY KEY,
            value BIGINT UNSIGNED NOT NULL,
            updated_at TIMESTAMP(6) NOT NULL
                DEFAULT CURRENT_TIMESTAMP(6)
                ON UPDATE CURRENT_TIMESTAMP(6)
        ) ENGINE=InnoDB
        "#,
    )
    .execute(source)
    .await
    .context("failed to create marker table")?;
    sqlx::query("INSERT INTO lag_demo.marker (id, value) VALUES (1, 0)")
        .execute(source)
        .await
        .context("failed to initialize marker")?;

    Ok(())
}

pub async fn seed(source: &MySqlPool, config: &Config) -> Result<()> {
    println!("Seeding {} rows on Source...", config.demo_rows);
    let started = Instant::now();
    let mut first_id = 1;
    let progress_step = (config.demo_rows / 20).max(config.seed_batch_size);
    let mut next_progress = progress_step;

    while first_id <= config.demo_rows {
        let last_id = (first_id + config.seed_batch_size - 1).min(config.demo_rows);
        let mut statement =
            String::from("INSERT INTO lag_demo.items (id, version, payload) VALUES ");

        for id in first_id..=last_id {
            if id != first_id {
                statement.push(',');
            }
            write!(statement, "({id}, 1, REPEAT('x', 512))")?;
        }

        sqlx::query(&statement)
            .execute(source)
            .await
            .with_context(|| format!("failed to seed rows {first_id}..={last_id}"))?;

        if last_id >= next_progress || last_id == config.demo_rows {
            println!(
                "  seed: {last_id}/{} ({:.1}%)",
                config.demo_rows,
                last_id as f64 * 100.0 / config.demo_rows as f64
            );
            next_progress = next_progress.saturating_add(progress_step);
        }

        first_id = last_id + 1;
    }

    println!("Seed completed in {:.2}s.", started.elapsed().as_secs_f64());
    Ok(())
}

pub async fn wait_for_initial_sync(replica: &MySqlPool, config: &Config) -> Result<()> {
    println!("Waiting for Replica to receive all initial rows...");
    let started = Instant::now();
    let progress_step = (config.demo_rows / 20).max(1);
    let mut next_progress = 0;
    let mut schema_wait_reported = false;

    loop {
        match database::item_count(replica).await {
            Ok(count) => {
                if count >= next_progress || count == config.demo_rows {
                    println!("  Replica seed rows: {count}/{}", config.demo_rows);
                    next_progress = count.saturating_add(progress_step);
                }

                if count == config.demo_rows {
                    let marker = database::marker_value(replica).await?;
                    if marker == 0 {
                        println!(
                            "Initial synchronization completed in {:.2}s.",
                            started.elapsed().as_secs_f64()
                        );
                        return Ok(());
                    }
                }
            }
            Err(error) => {
                if !schema_wait_reported {
                    println!("  Replica schema is not visible yet: {error:#}");
                    schema_wait_reported = true;
                }
            }
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

pub async fn wait_for_final_sync(replica: &MySqlPool, config: &Config) -> Result<()> {
    println!("Waiting for Replica to apply all remaining heavy updates...");
    let started = Instant::now();
    let mut last_reported = None;

    loop {
        let count = database::updated_item_count(replica).await?;
        if last_reported != Some(count) {
            println!("  Replica updated rows: {count}/{}", config.demo_rows);
            last_reported = Some(count);
        }

        if count == config.demo_rows {
            println!(
                "Final synchronization completed in {:.2}s.",
                started.elapsed().as_secs_f64()
            );
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

use std::{env, str::FromStr, time::Duration};

use anyhow::{Context, Result, bail};

#[derive(Clone, Debug)]
pub struct Config {
    pub source_database_url: String,
    pub replica_database_url: String,
    pub demo_rows: u64,
    pub seed_batch_size: u64,
    pub heavy_batch_size: u64,
    pub marker_after_batches: u64,
    pub poll_interval: Duration,
    pub initial_sync_timeout: Duration,
    pub final_sync_timeout: Duration,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let config = Self {
            source_database_url: env_or(
                "SOURCE_DATABASE_URL",
                "mysql://root:root@127.0.0.1:3307/mysql",
            ),
            replica_database_url: env_or(
                "REPLICA_DATABASE_URL",
                "mysql://root:root@127.0.0.1:3308/mysql",
            ),
            demo_rows: parse_env("DEMO_ROWS", 1_000_000)?,
            seed_batch_size: parse_env("SEED_BATCH_SIZE", 1_000)?,
            heavy_batch_size: parse_env("HEAVY_BATCH_SIZE", 50_000)?,
            marker_after_batches: parse_env("MARKER_AFTER_BATCHES", 5)?,
            poll_interval: Duration::from_millis(parse_env("POLL_INTERVAL_MS", 500)?),
            initial_sync_timeout: Duration::from_secs(parse_env("INITIAL_SYNC_TIMEOUT_SECS", 600)?),
            final_sync_timeout: Duration::from_secs(parse_env("FINAL_SYNC_TIMEOUT_SECS", 600)?),
        };

        config.validate()?;
        Ok(config)
    }

    pub fn total_heavy_batches(&self) -> u64 {
        self.demo_rows.div_ceil(self.heavy_batch_size)
    }

    fn validate(&self) -> Result<()> {
        if self.source_database_url == self.replica_database_url {
            bail!("SOURCE_DATABASE_URL and REPLICA_DATABASE_URL must be different");
        }

        for (name, value) in [
            ("DEMO_ROWS", self.demo_rows),
            ("SEED_BATCH_SIZE", self.seed_batch_size),
            ("HEAVY_BATCH_SIZE", self.heavy_batch_size),
            ("MARKER_AFTER_BATCHES", self.marker_after_batches),
        ] {
            if value == 0 {
                bail!("{name} must be greater than zero");
            }
        }

        if self.poll_interval.is_zero()
            || self.initial_sync_timeout.is_zero()
            || self.final_sync_timeout.is_zero()
        {
            bail!("poll interval and sync timeouts must be greater than zero");
        }

        if self.marker_after_batches > self.total_heavy_batches() {
            bail!(
                "MARKER_AFTER_BATCHES ({}) exceeds the number of heavy batches ({})",
                self.marker_after_batches,
                self.total_heavy_batches()
            );
        }

        Ok(())
    }
}

fn env_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn parse_env<T>(name: &str, default: T) -> Result<T>
where
    T: FromStr + Copy,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match env::var(name) {
        Ok(value) => value
            .parse()
            .with_context(|| format!("failed to parse {name}={value}")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error).with_context(|| format!("failed to read {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> Config {
        Config {
            source_database_url: "mysql://source".into(),
            replica_database_url: "mysql://replica".into(),
            demo_rows: 101,
            seed_batch_size: 10,
            heavy_batch_size: 20,
            marker_after_batches: 2,
            poll_interval: Duration::from_millis(100),
            initial_sync_timeout: Duration::from_secs(1),
            final_sync_timeout: Duration::from_secs(1),
        }
    }

    #[test]
    fn calculates_partial_final_batch() {
        assert_eq!(valid_config().total_heavy_batches(), 6);
    }

    #[test]
    fn rejects_marker_after_last_batch() {
        let mut config = valid_config();
        config.marker_after_batches = 7;

        assert!(config.validate().is_err());
    }
}

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Result, bail};
use sqlx::MySqlPool;

use crate::{
    config::Config,
    contracts::{
        DatabaseTarget::{Replica, Source},
        ObservationMetric::Marker,
    },
    database,
    runner::{Journal, replica_status},
};

#[derive(Debug)]
pub struct MarkerObservation {
    pub elapsed: Duration,
    pub stale_read_observed: bool,
}

#[derive(Debug, Default)]
struct MarkerTracker {
    stale_read_observed: bool,
}

impl MarkerTracker {
    fn observe(&mut self, source_value: u64, replica_value: u64, expected_value: u64) -> bool {
        if source_value == expected_value && replica_value != expected_value {
            self.stale_read_observed = true;
        }

        replica_value == expected_value
    }
}

pub async fn wait_for_marker(
    source: &MySqlPool,
    replica: &MySqlPool,
    expected_value: u64,
    heavy_progress: &AtomicU64,
    config: &Config,
    j: &Journal,
) -> Result<MarkerObservation> {
    let started = Instant::now();
    let mut tracker = MarkerTracker::default();

    j.line(format_args!(""));
    j.line(format_args!(
        "elapsed | Source marker | Replica marker | lag(s) | relay(bytes) | read-pos | exec-pos | heavy"
    ));

    loop {
        let (source_marker, replica_marker, status) = tokio::join!(
            j.decimal(Source, Marker, database::marker_value(source)),
            j.decimal(Replica, Marker, database::marker_value(replica)),
            j.replication(replica_status(replica)),
        );
        let (source_marker, replica_marker, status) = (source_marker?, replica_marker?, status?);
        let completed_batches = heavy_progress.load(Ordering::Relaxed);
        let lag = status
            .seconds_behind_source
            .map_or_else(|| "NULL".to_owned(), |value| value.to_string());

        j.line(format_args!(
            "{:>6.2}s | {:>13} | {:>14} | {:>6} | {:>12} | {:>8} | {:>8} | {}/{}",
            started.elapsed().as_secs_f64(),
            source_marker,
            replica_marker,
            lag,
            status.relay_log_space,
            status.read_source_log_pos,
            status.exec_source_log_pos,
            completed_batches,
            config.total_heavy_batches(),
        ));

        if source_marker == expected_value && replica_marker != expected_value {
            j.stale(source_marker, replica_marker);
        }

        if tracker.observe(source_marker, replica_marker, expected_value) {
            return Ok(MarkerObservation {
                elapsed: started.elapsed(),
                stale_read_observed: tracker.stale_read_observed,
            });
        }

        if started.elapsed() >= config.final_sync_timeout {
            bail!(
                "marker synchronization timed out after {:.2}s",
                started.elapsed().as_secs_f64()
            );
        }

        tokio::time::sleep(config.poll_interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::MarkerTracker;

    #[test]
    fn records_stale_read_until_replica_catches_up() {
        let mut tracker = MarkerTracker::default();

        assert!(!tracker.observe(1, 0, 1));
        assert!(tracker.stale_read_observed);
        assert!(tracker.observe(1, 1, 1));
        assert!(tracker.stale_read_observed);
    }

    #[test]
    fn immediate_replica_visibility_is_not_a_stale_read() {
        let mut tracker = MarkerTracker::default();

        assert!(tracker.observe(1, 1, 1));
        assert!(!tracker.stale_read_observed);
    }
}

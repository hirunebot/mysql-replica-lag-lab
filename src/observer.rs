//! Web実行のDB観測。表示と同期判定は同じ観測結果を使う。
use std::{
    sync::atomic::{AtomicU8, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use sqlx::MySqlPool;
use tokio::{sync::watch, time::MissedTickBehavior};

use crate::{
    config::Config,
    contracts::{
        DatabaseTarget,
        DatabaseTarget::{Replica, Source},
        Observation, ObservationMetric,
        ObservationMetric::{Marker, Replication, TotalRows, UpdatedRows},
        ObservationStatus, ObservationValue, ReplicaApplicationStatus,
    },
    database::{self, SchemaPending, bounded_read},
    monitor::MarkerObservation,
    runner::{Journal, replica_status},
};

const COUNTS_INTERVAL: Duration = Duration::from_secs(2);
const WAITING_SCHEMA: u8 = 0;
const CHECK_FAILED: u8 = 1;
const READY_SCHEMA: u8 = 2;

pub(crate) fn initial_observations() -> Vec<Observation> {
    [
        (Source, TotalRows),
        (Source, UpdatedRows),
        (Source, Marker),
        (Replica, TotalRows),
        (Replica, UpdatedRows),
        (Replica, Marker),
        (Replica, Replication),
    ]
    .map(|(target, metric)| Observation {
        target,
        metric,
        status: ObservationStatus::NotObserved,
        value: None,
        started_at: None,
        observed_at: None,
        elapsed_ms: None,
        error: None,
    })
    .into()
}

pub(crate) struct Observer {
    source: MySqlPool,
    replica: MySqlPool,
    setup_gtids: String,
    schema: AtomicU8,
    changed: watch::Sender<()>,
    marker: watch::Sender<Option<(u64, u64)>>,
    stop: watch::Sender<bool>,
    stopped: watch::Sender<bool>,
}

impl Observer {
    pub(crate) fn new(config: &Config, setup_gtids: String) -> Result<Self> {
        ensure!(
            !setup_gtids.is_empty(),
            "dashboard observations require GTID replication"
        );
        Ok(Self {
            source: database::read_pool(&config.source_database_url, 2)?,
            replica: database::read_pool(&config.replica_database_url, 2)?,
            setup_gtids,
            schema: AtomicU8::new(WAITING_SCHEMA),
            changed: watch::Sender::new(()),
            marker: watch::Sender::new(None),
            stop: watch::Sender::new(false),
            stopped: watch::Sender::new(false),
        })
    }

    pub(crate) async fn during<T>(
        &self,
        work: impl Future<Output = Result<T>>,
        j: &Journal,
        interval: Duration,
    ) -> Result<T> {
        let (result, ()) = tokio::join!(
            async {
                let result = work.await;
                self.stop.send_replace(true);
                result
            },
            async {
                tokio::join!(
                    tick(interval, self.stop.subscribe(), || self.fast(j)),
                    tick(COUNTS_INTERVAL, self.stop.subscribe(), || self.counts(j)),
                );
                self.stopped.send_replace(true);
            }
        );
        tokio::join!(self.source.close(), self.replica.close());
        result
    }

    async fn fast(&self, j: &Journal) {
        let (source, _) = tokio::join!(
            j.decimal(
                Source,
                Marker,
                bounded_read(&self.source, async |connection| database::marker_value(
                    connection
                )
                .await)
            ),
            j.replication(bounded_read(&self.replica, async |connection| {
                replica_status(connection).await
            })),
        );
        // Sourceを先に読み、Replicaの古い値をその後に観測した場合だけ記録する。
        let replica = j
            .decimal(
                Replica,
                Marker,
                bounded_read(&self.replica, async |connection| {
                    if self.schema.load(Ordering::Relaxed) != READY_SCHEMA {
                        let ready =
                            database::has_executed(&mut *connection, &self.setup_gtids).await;
                        self.schema.store(
                            match &ready {
                                Ok(true) => READY_SCHEMA,
                                Ok(false) => WAITING_SCHEMA,
                                Err(_) => CHECK_FAILED,
                            },
                            Ordering::Relaxed,
                        );
                        ready?;
                    }
                    self.ensure_schema()?;
                    database::marker_value(connection).await
                }),
            )
            .await;
        let pair = source.ok().zip(replica.ok());
        if let Some((source @ 1, replica)) = pair
            && source != replica
        {
            j.stale(source, replica);
        }
        self.marker.send_replace(pair);
        self.changed.send_replace(());
    }

    fn ensure_schema(&self) -> Result<()> {
        match self.schema.load(Ordering::Relaxed) {
            READY_SCHEMA => Ok(()),
            WAITING_SCHEMA => Err(SchemaPending.into()),
            _ => anyhow::bail!("could not check the current schema on Replica"),
        }
    }

    async fn counts(&self, j: &Journal) {
        tokio::join!(
            async {
                let _ = j
                    .decimal(
                        Source,
                        TotalRows,
                        bounded_read(&self.source, async |connection| {
                            database::item_count(connection).await
                        }),
                    )
                    .await;
                let _ = j
                    .decimal(
                        Source,
                        UpdatedRows,
                        bounded_read(&self.source, async |connection| {
                            database::updated_item_count(connection).await
                        }),
                    )
                    .await;
            },
            async {
                let _ = j
                    .decimal(
                        Replica,
                        TotalRows,
                        bounded_read(&self.replica, async |connection| {
                            self.ensure_schema()?;
                            database::item_count(connection).await
                        }),
                    )
                    .await;
                let _ = j
                    .decimal(
                        Replica,
                        UpdatedRows,
                        bounded_read(&self.replica, async |connection| {
                            self.ensure_schema()?;
                            database::updated_item_count(connection).await
                        }),
                    )
                    .await;
            }
        );
        self.changed.send_replace(());
        if self.ensure_schema().is_ok()
            && !*self.stop.borrow()
            && let Some(plan) = j.oldest_unconfirmed_batch()
        {
            let first: u64 = plan.first_id.as_deref().unwrap().parse().unwrap();
            let last: u64 = plan.last_id.as_deref().unwrap().parse().unwrap();
            let status = match bounded_read(&self.replica, async |connection| {
                database::updated_batch_count(connection, first, last).await
            })
            .await
            {
                Ok(count) if count == last - first + 1 => ReplicaApplicationStatus::ObservedApplied,
                Ok(_) => ReplicaApplicationStatus::Pending,
                Err(_) => ReplicaApplicationStatus::Unavailable,
            };
            j.replica_batch(&plan.operation_id, status);
        }
    }

    async fn wait(&self, timeout: Duration, mut ready: impl FnMut() -> bool) -> Result<()> {
        let mut changed = self.changed.subscribe();
        tokio::time::timeout(timeout, async {
            while !ready() {
                changed.changed().await.expect("Observer owns the sender");
            }
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn wait_initial(&self, j: &Journal, config: &Config) -> Result<()> {
        let since = Instant::now();
        let rows = config.demo_rows.to_string();
        self.wait(config.initial_sync_timeout, || {
            let observations = j.observations_since(since);
            decimal_is(&observations, Source, TotalRows, &rows)
                && decimal_is(&observations, Replica, TotalRows, &rows)
                && decimal_is(&observations, Replica, Marker, "0")
        })
        .await
        .context("initial synchronization timed out")
    }

    pub(crate) async fn wait_marker(
        &self,
        j: &Journal,
        timeout: Duration,
        started: Instant,
    ) -> Result<MarkerObservation> {
        let mut marker = self.marker.subscribe();
        tokio::time::timeout(timeout, marker.wait_for(|values| *values == Some((1, 1))))
            .await
            .context("marker synchronization timed out")??;
        Ok(MarkerObservation {
            elapsed: started.elapsed(),
            stale_read_observed: j.has_stale_read(),
        })
    }

    pub(crate) async fn wait_final(&self, j: &Journal, config: &Config) -> Result<()> {
        let since = Instant::now();
        let expected = config.demo_rows.to_string();
        let complete = || final_counts_match(&j.observations_since(since), &expected);
        self.wait(config.final_sync_timeout, complete)
            .await
            .context("final synchronization timed out")?;
        self.stop.send_replace(true);
        self.stopped
            .subscribe()
            .wait_for(|stopped| *stopped)
            .await?;
        ensure!(
            complete(),
            "final database observations could not be confirmed"
        );
        j.confirm_all_batches();
        Ok(())
    }
}

async fn tick<F: Future<Output = ()>>(
    interval: Duration,
    mut stop: watch::Receiver<bool>,
    mut sample: impl FnMut() -> F,
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            biased;
            _ = stop.wait_for(|stop| *stop) => return,
            _ = ticker.tick() => {},
        }
        sample().await;
    }
}

fn decimal_is(
    observations: &[Observation],
    target: DatabaseTarget,
    metric: ObservationMetric,
    expected: &str,
) -> bool {
    observations.iter().any(|o| {
        o.target == target
            && o.metric == metric
            && matches!(&o.value, Some(ObservationValue::Decimal(value)) if value == expected)
    })
}

fn final_counts_match(observations: &[Observation], rows: &str) -> bool {
    [Source, Replica].into_iter().all(|target| {
        decimal_is(observations, target, TotalRows, rows)
            && decimal_is(observations, target, UpdatedRows, rows)
            && decimal_is(observations, target, Marker, "1")
    }) && observations.iter().any(|o| o.target == Replica && o.metric == Replication
        && matches!(&o.value, Some(ObservationValue::Replication(r)) if r.receiver_running && r.applier_running))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::Notify;

    /// 照会が間隔を超えても重複せず、停止時は発行済みの照会を回収する。
    #[tokio::test]
    async fn slow_sample_does_not_overlap_or_get_dropped_on_stop() {
        let (stop, stopped) = watch::channel(false);
        let started = Notify::new();
        let finished = Notify::new();
        let count = std::cell::Cell::new(0);
        tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(
                tick(Duration::from_millis(1), stopped, || async {
                    count.set(count.get() + 1);
                    started.notify_one();
                    finished.notified().await;
                }),
                async {
                    started.notified().await;
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    assert_eq!(count.get(), 1);
                    stop.send_replace(true);
                    finished.notify_one();
                }
            );
        })
        .await
        .unwrap();
        assert_eq!(count.get(), 1);
    }
}

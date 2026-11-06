use std::{
    collections::HashMap,
    fmt,
    future::Future,
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::Result;
use chrono::{SecondsFormat, Utc};

use super::RunnerConfig;
use crate::{
    contracts::*,
    database::{ReplicaStatus, SchemaPending},
    events::Reporter,
};

pub(super) fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn public_error(code: &str, message: &str) -> ApiError {
    ApiError {
        code: code.into(),
        message: message.into(),
        retryable: false,
    }
}

fn write_status(error: &anyhow::Error) -> OperationStatus {
    match error.downcast_ref::<sqlx::Error>() {
        Some(
            sqlx::Error::Io(_)
            | sqlx::Error::Protocol(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::WorkerCrashed,
        ) => OperationStatus::OutcomeUnknown,
        _ => OperationStatus::Failed,
    }
}

pub(crate) struct Journal {
    reporter: Arc<dyn Reporter>,
    state: Mutex<JournalState>,
    indices: HashMap<String, usize>,
    dashboard: bool,
}

struct JournalState {
    snapshot: RunSnapshot,
    read_started: HashMap<(DatabaseTarget, ObservationMetric), Instant>,
}

impl Journal {
    pub(super) fn new(
        config: &RunnerConfig,
        plan: Vec<OperationPlan>,
        reporter: Arc<dyn Reporter>,
        dashboard: bool,
    ) -> Self {
        let indices = plan
            .iter()
            .enumerate()
            .map(|(i, p)| (p.operation_id.clone(), i))
            .collect();
        let operations = plan
            .iter()
            .map(|p| OperationState {
                operation_id: p.operation_id.clone(),
                status: OperationStatus::Planned,
                replica_status: ReplicaApplicationStatus::Unknown,
                replica_observed_at: None,
                started_at: None,
                completed_at: None,
                elapsed_ms: None,
                affected_rows: None,
                error: None,
            })
            .collect();
        reporter.plan(&plan);
        Self {
            reporter,
            indices,
            dashboard,
            state: Mutex::new(JournalState {
                read_started: HashMap::new(),
                snapshot: RunSnapshot {
                    schema_version: 1,
                    server_instance_id: config.server_instance_id.clone(),
                    run_id: config.run_id.clone(),
                    sequence: "0".into(),
                    phase: RunPhase::Idle,
                    config: config.workload.clone(),
                    plan,
                    operations,
                    observations: if dashboard {
                        crate::observer::initial_observations()
                    } else {
                        vec![]
                    },
                    manual_reads: vec![],
                    stale_read_observed: false,
                    error: None,
                },
            }),
        }
    }

    pub(crate) fn line(&self, message: fmt::Arguments<'_>) {
        self.reporter.line(message);
    }

    pub(crate) fn statement(&self, id: &str, sql: SqlStatement, range: Option<(u64, u64)>) {
        let mut state = self.state.lock().unwrap();
        let s = &mut state.snapshot;
        let stored = &mut s.plan[self.indices[id]];
        let mut p = stored.clone();
        p.sql = sql;
        if let Some((first, last)) = range {
            p.first_id = Some(first.to_string());
            p.last_id = Some(last.to_string());
            p.planned_rows = Some((last - first + 1).to_string());
        }
        self.reporter.statement(&p);
        if self.dashboard && matches!(stored.kind, OperationKind::Setup | OperationKind::Seed) {
            stored.sql = p.sql;
            stored.first_id = p.first_id;
            stored.last_id = p.last_id;
            let plan = stored.clone();
            self.emit(s, EventPayload::QueryStatement { plan });
        }
    }

    fn next(s: &mut RunSnapshot) {
        s.sequence = (s.sequence.parse::<u64>().unwrap() + 1).to_string();
    }

    fn emit(&self, s: &mut RunSnapshot, event: EventPayload) {
        Self::next(s);
        self.reporter.report(event);
    }

    pub(super) fn phase(&self, phase: RunPhase) {
        let mut state = self.state.lock().unwrap();
        let s = &mut state.snapshot;
        if phase == RunPhase::Failing && s.phase == phase {
            return;
        }
        debug_assert!(can_transition(s.phase, phase));
        s.phase = phase;
        self.emit(s, EventPayload::RunPhaseChanged { phase });
    }

    pub(super) fn finish(&self, success: bool) {
        let mut state = self.state.lock().unwrap();
        let s = &mut state.snapshot;
        let phase = if success {
            RunPhase::Completed
        } else {
            RunPhase::Failed
        };
        debug_assert!(can_transition(s.phase, phase));
        s.phase = phase;
        if !success {
            s.error = Some(public_error(
                "run_failed",
                "Demo execution failed; see the runner log.",
            ));
        }
        Self::next(s);
        let snapshot = Box::new(s.clone());
        self.reporter.report(if success {
            EventPayload::RunCompleted { snapshot }
        } else {
            EventPayload::RunFailed { snapshot }
        });
    }

    pub(crate) async fn operation(
        &self,
        id: &str,
        work: impl Future<Output = Result<u64>>,
    ) -> Result<u64> {
        let started = Instant::now();
        let i = self.indices[id];
        {
            let mut state = self.state.lock().unwrap();
            let s = &mut state.snapshot;
            let o = &mut s.operations[i];
            o.status = OperationStatus::Running;
            o.started_at = Some(now());
            let operation = o.clone();
            self.emit(s, EventPayload::QueryStarted { operation });
        }
        let result = work.await;
        let mut state = self.state.lock().unwrap();
        let s = &mut state.snapshot;
        let batch = self.dashboard && s.plan[i].kind == OperationKind::HeavyUpdate;
        let o = &mut s.operations[i];
        o.completed_at = Some(now());
        o.elapsed_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
        let event = match &result {
            Ok(count) => {
                o.status = OperationStatus::Committed;
                if batch {
                    o.replica_status = ReplicaApplicationStatus::Pending;
                }
                o.affected_rows = Some(count.to_string());
                EventPayload::QueryCompleted {
                    operation: o.clone(),
                }
            }
            Err(error) => {
                o.status = if id == "final-sync" {
                    OperationStatus::Failed
                } else {
                    write_status(error)
                };
                o.error = Some(if o.status == OperationStatus::OutcomeUnknown {
                    public_error(
                        "outcome_unknown",
                        "The connection failed; the SQL outcome is unknown.",
                    )
                } else {
                    public_error("query_failed", "The operation failed; see the runner log.")
                });
                EventPayload::QueryFailed {
                    operation: o.clone(),
                }
            }
        };
        self.emit(s, event);
        result
    }

    pub(crate) fn seed_progress(&self, rows: u64) {
        let mut state = self.state.lock().unwrap();
        let o = &mut state.snapshot.operations[self.indices["seed"]];
        o.affected_rows = Some(rows.to_string());
        self.reporter.seed_progress(o);
    }

    pub(crate) async fn decimal(
        &self,
        target: DatabaseTarget,
        metric: ObservationMetric,
        read: impl Future<Output = Result<u64>>,
    ) -> Result<u64> {
        self.observe(target, metric, read, |v| {
            ObservationValue::Decimal(v.to_string())
        })
        .await
    }

    pub(crate) async fn replication(
        &self,
        read: impl Future<Output = Result<ReplicaStatus>>,
    ) -> Result<ReplicaStatus> {
        self.observe(
            DatabaseTarget::Replica,
            ObservationMetric::Replication,
            read,
            |r| {
                ObservationValue::Replication(ReplicationState {
                    receiver_running: r.io_running == "Yes",
                    applier_running: r.sql_running == "Yes",
                    seconds_behind_source: r.seconds_behind_source.map(|v| v as f64),
                    relay_log_bytes: r.relay_log_space.to_string(),
                    source_log_file: r.source_log_file.clone(),
                    read_source_log_pos: r.read_source_log_pos.to_string(),
                    relay_source_log_file: r.relay_source_log_file.clone(),
                    exec_source_log_pos: r.exec_source_log_pos.to_string(),
                })
            },
        )
        .await
    }

    async fn observe<T>(
        &self,
        target: DatabaseTarget,
        metric: ObservationMetric,
        read: impl Future<Output = Result<T>>,
        value: impl FnOnce(&T) -> ObservationValue,
    ) -> Result<T> {
        let started_at = now();
        let start = Instant::now();
        let result = read.await;
        let mut state = self.state.lock().unwrap();
        state.read_started.insert((target, metric), start);
        let s = &mut state.snapshot;
        let old = s
            .observations
            .iter()
            .position(|o| o.target == target && o.metric == metric);
        let mut observation = Observation {
            target,
            metric,
            status: ObservationStatus::Available,
            value: None,
            started_at: Some(started_at),
            observed_at: None,
            elapsed_ms: Some(start.elapsed().as_secs_f64() * 1000.0),
            error: None,
        };
        match &result {
            Ok(v) => {
                observation.value = Some(value(v));
                observation.observed_at = Some(now());
            }
            Err(e) => {
                let missing = e
                    .downcast_ref::<sqlx::Error>()
                    .and_then(sqlx::Error::as_database_error)
                    .and_then(|db| db.code())
                    .is_some_and(|code| code == "42S02" || code == "1049" || code == "1146");
                observation.status = if e.is::<SchemaPending>()
                    || missing
                        && matches!(s.phase, RunPhase::Preparing | RunPhase::WaitingInitialSync)
                {
                    ObservationStatus::NotReady
                } else {
                    ObservationStatus::Unavailable
                };
                observation.error = Some(public_error(
                    "observation_failed",
                    "Database observation failed.",
                ));
                if let Some(i) = old
                    && observation.status == ObservationStatus::Unavailable
                {
                    observation.value = s.observations[i].value.clone();
                    observation.observed_at = s.observations[i].observed_at.clone();
                }
            }
        }
        if let Some(i) = old {
            s.observations[i] = observation.clone();
        } else {
            s.observations.push(observation.clone());
        }
        self.emit(s, EventPayload::DbObserved { observation });
        result
    }

    pub(crate) fn observations_since(&self, since: Instant) -> Vec<Observation> {
        let state = self.state.lock().unwrap();
        state
            .snapshot
            .observations
            .iter()
            .filter(|o| {
                o.status == ObservationStatus::Available
                    && state
                        .read_started
                        .get(&(o.target, o.metric))
                        .is_some_and(|started| *started >= since)
            })
            .cloned()
            .collect()
    }

    pub(crate) fn oldest_unconfirmed_batch(&self) -> Option<OperationPlan> {
        let state = self.state.lock().unwrap();
        let snapshot = &state.snapshot;
        snapshot
            .plan
            .iter()
            .zip(&snapshot.operations)
            .find(|(plan, operation)| {
                plan.kind == OperationKind::HeavyUpdate
                    && operation.status == OperationStatus::Committed
                    && operation.replica_status != ReplicaApplicationStatus::ObservedApplied
            })
            .map(|(plan, _)| plan.clone())
    }

    pub(crate) fn replica_batch(&self, index: &str, status: ReplicaApplicationStatus) {
        let mut state = self.state.lock().unwrap();
        let snapshot = &mut state.snapshot;
        let operation = &mut snapshot.operations[self.indices[index]];
        if operation.replica_status == ReplicaApplicationStatus::ObservedApplied {
            return;
        }
        operation.replica_status = status;
        operation.replica_observed_at = Some(now());
        let operation = operation.clone();
        self.emit(
            snapshot,
            EventPayload::ReplicaOperationObserved { operation },
        );
    }

    pub(crate) fn confirm_all_batches(&self) {
        let mut state = self.state.lock().unwrap();
        let snapshot = &mut state.snapshot;
        let observed_at = snapshot
            .observations
            .iter()
            .find(|o| {
                o.target == DatabaseTarget::Replica && o.metric == ObservationMetric::UpdatedRows
            })
            .and_then(|o| o.observed_at.clone())
            .expect("validated replica COUNT");
        // 全件COUNTで確認済みなら、最終スナップショットからまとめて通知する。
        for (plan, operation) in snapshot.plan.iter().zip(&mut snapshot.operations) {
            if plan.kind == OperationKind::HeavyUpdate
                && operation.replica_status != ReplicaApplicationStatus::ObservedApplied
            {
                operation.replica_status = ReplicaApplicationStatus::ObservedApplied;
                operation.replica_observed_at = Some(observed_at.clone());
            }
        }
    }

    pub(crate) fn has_stale_read(&self) -> bool {
        self.state.lock().unwrap().snapshot.stale_read_observed
    }

    pub(crate) fn stale(&self, source: u64, replica: u64) {
        let mut state = self.state.lock().unwrap();
        let s = &mut state.snapshot;
        if !s.stale_read_observed {
            s.stale_read_observed = true;
            self.emit(
                s,
                EventPayload::StaleReadObserved {
                    source_value: source.to_string(),
                    replica_value: replica.to_string(),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct DiscardEvents;

    impl Reporter for DiscardEvents {
        fn report(&self, _: EventPayload) {}
    }

    /// 未作成と取得失敗を区別し、前回の値を表示用に保持しても同期判定には使わない。
    #[tokio::test]
    async fn only_fresh_successful_observations_can_confirm_sync() {
        let config = RunnerConfig {
            server_instance_id: "observer-test".into(),
            run_id: "run".into(),
            source_database_url: String::new(),
            replica_database_url: String::new(),
            workload: RunConfig {
                demo_rows: "20".into(),
                seed_batch_size: "10".into(),
                heavy_batch_size: "10".into(),
                marker_after_batches: "1".into(),
            },
            poll_interval: Duration::from_millis(500),
            initial_sync_timeout: Duration::from_secs(10),
            final_sync_timeout: Duration::from_secs(10),
        };
        let j = Journal::new(&config, vec![], Arc::new(DiscardEvents), true);
        let target = DatabaseTarget::Replica;
        let metric = ObservationMetric::Marker;
        let sample = || {
            j.state
                .lock()
                .unwrap()
                .snapshot
                .observations
                .iter()
                .find(|o| o.target == target && o.metric == metric)
                .unwrap()
                .clone()
        };

        assert_eq!(sample().status, ObservationStatus::NotObserved);
        let _ = j
            .decimal(target, metric, async { Err(SchemaPending.into()) })
            .await;
        assert_eq!(sample().status, ObservationStatus::NotReady);
        assert_eq!(sample().value, None);
        let before = Instant::now();
        j.decimal(target, metric, async { Ok(0) }).await.unwrap();
        let successful = sample();
        assert_eq!(
            j.observations_since(before),
            std::slice::from_ref(&successful)
        );
        assert!(j.observations_since(Instant::now()).is_empty());
        let _ = j
            .decimal(target, metric, async {
                anyhow::bail!("connection unavailable")
            })
            .await;
        let failed = sample();
        assert_eq!(failed.status, ObservationStatus::Unavailable);
        assert_eq!(failed.value, successful.value);
        assert_eq!(failed.observed_at, successful.observed_at);
        assert!(j.observations_since(before).is_empty());
    }

    #[test]
    fn connection_loss_has_unknown_write_outcome() {
        let error = anyhow::Error::new(sqlx::Error::Io(std::io::Error::from(
            std::io::ErrorKind::ConnectionReset,
        )))
        .context("failed to update marker on Source");
        assert_eq!(write_status(&error), OperationStatus::OutcomeUnknown);
        assert_eq!(
            write_status(&sqlx::Error::PoolTimedOut.into()),
            OperationStatus::Failed
        );
    }
}

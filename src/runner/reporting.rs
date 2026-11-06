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
use crate::{contracts::*, database::ReplicaStatus, events::Reporter};

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
    snapshot: Mutex<RunSnapshot>,
    indices: HashMap<String, usize>,
}

impl Journal {
    pub(super) fn new(
        config: &RunnerConfig,
        plan: Vec<OperationPlan>,
        reporter: Arc<dyn Reporter>,
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
            snapshot: Mutex::new(RunSnapshot {
                schema_version: 1,
                server_instance_id: config.server_instance_id.clone(),
                run_id: config.run_id.clone(),
                sequence: "0".into(),
                phase: RunPhase::Idle,
                config: config.workload.clone(),
                plan,
                operations,
                observations: vec![],
                manual_reads: vec![],
                stale_read_observed: false,
                error: None,
            }),
        }
    }

    pub(crate) fn line(&self, message: fmt::Arguments<'_>) {
        self.reporter.line(message);
    }

    pub(crate) fn statement(&self, id: &str, sql: SqlStatement, range: Option<(u64, u64)>) {
        let mut p = self.snapshot.lock().unwrap().plan[self.indices[id]].clone();
        p.sql = sql;
        if let Some((first, last)) = range {
            p.first_id = Some(first.to_string());
            p.last_id = Some(last.to_string());
            p.planned_rows = Some((last - first + 1).to_string());
        }
        self.reporter.statement(&p);
    }

    fn next(s: &mut RunSnapshot) {
        s.sequence = (s.sequence.parse::<u64>().unwrap() + 1).to_string();
    }

    fn emit(&self, s: &mut RunSnapshot, event: EventPayload) {
        Self::next(s);
        self.reporter.report(event);
    }

    pub(super) fn phase(&self, phase: RunPhase) {
        let mut s = self.snapshot.lock().unwrap();
        if phase == RunPhase::Failing && s.phase == phase {
            return;
        }
        debug_assert!(can_transition(s.phase, phase));
        s.phase = phase;
        self.emit(&mut s, EventPayload::RunPhaseChanged { phase });
    }

    pub(super) fn finish(&self, success: bool) {
        let mut s = self.snapshot.lock().unwrap();
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
        Self::next(&mut s);
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
            let mut s = self.snapshot.lock().unwrap();
            let o = &mut s.operations[i];
            o.status = OperationStatus::Running;
            o.started_at = Some(now());
            let operation = o.clone();
            self.emit(&mut s, EventPayload::QueryStarted { operation });
        }
        let result = work.await;
        let mut s = self.snapshot.lock().unwrap();
        let o = &mut s.operations[i];
        o.completed_at = Some(now());
        o.elapsed_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
        let event = match &result {
            Ok(count) => {
                o.status = OperationStatus::Committed;
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
        self.emit(&mut s, event);
        result
    }

    pub(crate) fn seed_progress(&self, rows: u64) {
        let mut s = self.snapshot.lock().unwrap();
        let o = &mut s.operations[self.indices["seed"]];
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
        let mut s = self.snapshot.lock().unwrap();
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
                observation.status = if missing && s.phase == RunPhase::WaitingInitialSync {
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
        self.emit(&mut s, EventPayload::DbObserved { observation });
        result
    }

    pub(crate) fn stale(&self, source: u64, replica: u64) {
        let mut s = self.snapshot.lock().unwrap();
        if !s.stale_read_observed {
            s.stale_read_observed = true;
            self.emit(
                &mut s,
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

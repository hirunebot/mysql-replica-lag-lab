//! 最新の1実行を保持し、HTTP要求と実行タスクの寿命を分離する。
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, ensure};
use chrono::{SecondsFormat, Utc};
use sqlx::{Connection, MySqlConnection, MySqlPool, mysql::MySqlConnectOptions};
use tokio::task::JoinError;
use uuid::Uuid;

use crate::{
    config::Config,
    contracts::*,
    database,
    events::Reporter,
    runner::{self, PreflightFailure, RunOutcome, RunnerConfig},
};

mod reads;
mod stream;
pub(crate) use reads::ReadError;

type RunFuture = Pin<Box<dyn Future<Output = anyhow::Result<RunOutcome>> + Send>>;
type Engine = dyn Fn(RunnerConfig, Arc<dyn Reporter>) -> RunFuture + Send + Sync;

pub(crate) enum StartError {
    Invalid(String),
    Busy,
}

pub(crate) struct RunManager {
    config: Config,
    default_config: RunConfig,
    instance: String,
    source: MySqlConnectOptions,
    replica: MySqlConnectOptions,
    manual_pool: MySqlPool,
    engine: Arc<Engine>,
    current: Mutex<Option<CurrentRun>>,
}

struct CurrentRun {
    snapshot: RunSnapshot,
    // 終了通知を受けただけでは解放しない。タスクの終了を確認してから解放する。
    reserved: bool,
    generation: u64,
    schema_observed: bool,
    terminal: Option<RunSnapshot>,
    sequence: u64,
    history: stream::History,
}

impl CurrentRun {
    fn read_pending(&self) -> bool {
        self.snapshot
            .manual_reads
            .first()
            .is_some_and(|read| read.status == ManualReadStatus::Pending)
    }

    fn busy(&self) -> bool {
        self.reserved || self.read_pending()
    }

    fn advance(&mut self) {
        self.sequence += 1;
        self.snapshot.sequence = self.sequence.to_string();
    }

    fn publish(&mut self, event: EventPayload) {
        self.advance();
        self.record(event);
    }
}

impl RunManager {
    pub(crate) fn new(config: Config) -> anyhow::Result<Arc<Self>> {
        Self::with_engine(
            config,
            Arc::new(|config, reporter| Box::pin(runner::run_dashboard(config, reporter))),
        )
    }

    pub(crate) fn with_engine(config: Config, engine: Arc<Engine>) -> anyhow::Result<Arc<Self>> {
        config.validate()?;
        // SQLxの接続文字列エラーには認証情報が含まれ得るので引き継がない。
        let source: MySqlConnectOptions = config
            .source_database_url
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid SOURCE_DATABASE_URL"))?;
        let replica: MySqlConnectOptions = config
            .replica_database_url
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid REPLICA_DATABASE_URL"))?;
        let same_target = match (source.get_socket(), replica.get_socket()) {
            (Some(a), Some(b)) => a == b,
            (None, None) => {
                source.get_port() == replica.get_port()
                    && source.get_host().eq_ignore_ascii_case(replica.get_host())
            }
            _ => false,
        };
        ensure!(
            !same_target,
            "Source and Replica must use different endpoints"
        );
        let default_config = RunConfig {
            demo_rows: config.demo_rows.to_string(),
            seed_batch_size: config.seed_batch_size.to_string(),
            heavy_batch_size: config.heavy_batch_size.to_string(),
            marker_after_batches: config.marker_after_batches.to_string(),
        };
        validate_web(&default_config).context("invalid dashboard default workload")?;
        let manual_pool = database::read_pool(&config.replica_database_url, 1)
            .map_err(|_| anyhow::anyhow!("invalid Replica read configuration"))?;
        Ok(Arc::new(Self {
            config,
            default_config,
            instance: Uuid::new_v4().to_string(),
            source,
            replica,
            manual_pool,
            engine,
            current: Mutex::new(None),
        }))
    }

    pub(crate) async fn state(&self) -> DashboardState {
        let (source, replica) = tokio::join!(
            probe(DatabaseTarget::Source, &self.source),
            probe(DatabaseTarget::Replica, &self.replica)
        );
        DashboardState {
            schema_version: 1,
            server_instance_id: self.instance.clone(),
            default_config: self.default_config.clone(),
            current_run: self
                .current
                .lock()
                .unwrap()
                .as_ref()
                .map(|r| r.snapshot.clone()),
            connections: vec![source, replica],
        }
    }

    pub(crate) fn run(&self, id: &str) -> Option<RunSnapshot> {
        self.current
            .lock()
            .unwrap()
            .as_ref()
            .filter(|r| r.snapshot.run_id == id)
            .map(|r| r.snapshot.clone())
    }

    pub(crate) fn start_run(
        self: &Arc<Self>,
        workload: RunConfig,
    ) -> Result<RunSnapshot, StartError> {
        validate_web(&workload).map_err(|e| StartError::Invalid(e.to_string()))?;
        // 先に空きを確認して、進行中のrunに対し不要なSQL計画を作らない。
        if self
            .current
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(CurrentRun::busy)
        {
            return Err(StartError::Busy);
        }
        let plan = runner::build_plan(&workload)
            .map_err(|_| StartError::Invalid("Could not prepare the workload plan.".into()))?;
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
        let mut snapshot = RunSnapshot {
            schema_version: 1,
            server_instance_id: self.instance.clone(),
            run_id: String::new(),
            sequence: "0".into(),
            phase: RunPhase::Preparing,
            config: workload.clone(),
            plan,
            operations,
            observations: crate::observer::initial_observations(),
            manual_reads: vec![],
            stale_read_observed: false,
            error: None,
        };
        {
            let mut current = self.current.lock().unwrap();
            if current.as_ref().is_some_and(CurrentRun::busy) {
                return Err(StartError::Busy);
            }
            let generation = current
                .as_ref()
                .map_or(1, |previous| previous.generation + 1);
            // 起動インスタンスと採番を組にし、履歴を保持せずに古いrunを判別する。
            snapshot.run_id = format!("{}.{}", self.instance, generation);
            *current = Some(CurrentRun {
                snapshot: snapshot.clone(),
                reserved: true,
                generation,
                schema_observed: false,
                terminal: None,
                sequence: 0,
                history: stream::History::default(),
            });
        }
        let id = snapshot.run_id.clone();
        let config = RunnerConfig {
            server_instance_id: self.instance.clone(),
            run_id: id.clone(),
            source_database_url: self.config.source_database_url.clone(),
            replica_database_url: self.config.replica_database_url.clone(),
            workload,
            poll_interval: self.config.poll_interval,
            initial_sync_timeout: self.config.initial_sync_timeout,
            final_sync_timeout: self.config.final_sync_timeout,
        };
        let manager = self.clone();
        let reporter = Arc::new(RunReporter {
            manager: manager.clone(),
            id: id.clone(),
        });
        let engine = self.engine.clone();
        let worker = tokio::spawn(async move { engine(config, reporter).await });
        tokio::spawn(async move {
            manager.finish(&id, worker.await);
        });
        Ok(snapshot)
    }

    fn finish(&self, id: &str, result: Result<anyhow::Result<RunOutcome>, JoinError>) {
        let mut current = self.current.lock().unwrap();
        let Some(run) = current.as_mut().filter(|r| r.snapshot.run_id == id) else {
            return;
        };
        let terminal = run.terminal.take().filter(|s| {
            s.run_id == id
                && s.server_instance_id == self.instance
                && s.config == run.snapshot.config
                && validate_snapshot(s).is_ok()
        });
        let expected = match &result {
            Ok(Ok(_)) => Some(RunPhase::Completed),
            Ok(Err(_)) => Some(RunPhase::Failed),
            Err(_) => None,
        };
        let released = terminal.as_ref().is_some_and(|s| {
            Some(s.phase) == expected
                && s.operations.iter().all(|o| {
                    !matches!(
                        o.status,
                        OperationStatus::Running | OperationStatus::OutcomeUnknown
                    )
                })
        });
        if let Some(mut s) = terminal {
            s.manual_reads = std::mem::take(&mut run.snapshot.manual_reads);
            run.snapshot = s;
        }
        if released {
            run.reserved = false;
            if let Ok(Err(error)) = &result {
                run.snapshot.error = Some(run_error(error));
            }
        } else {
            run.snapshot.phase = RunPhase::Failing;
            run.snapshot.error = Some(error(
                "completion_unconfirmed",
                "Database work could not be confirmed stopped; the run remains reserved.",
                false,
            ));
        }
        run.advance();
        let snapshot = Box::new(run.snapshot.clone());
        run.record(match run.snapshot.phase {
            RunPhase::Completed => EventPayload::RunCompleted { snapshot },
            RunPhase::Failed => EventPayload::RunFailed { snapshot },
            _ => EventPayload::Snapshot { snapshot },
        });
    }
}

struct RunReporter {
    manager: Arc<RunManager>,
    id: String,
}
impl RunReporter {
    fn update(&self, f: impl FnOnce(&mut CurrentRun)) {
        let mut current = self.manager.current.lock().unwrap();
        if let Some(r) = current
            .as_mut()
            .filter(|r| r.snapshot.run_id == self.id && r.reserved)
        {
            f(r);
        }
    }
}
impl Reporter for RunReporter {
    fn report(&self, event: EventPayload) {
        self.update(|r| {
            match &event {
                EventPayload::RunPhaseChanged { phase }
                    if can_transition(r.snapshot.phase, *phase) =>
                {
                    r.snapshot.phase = *phase;
                    r.schema_observed |= *phase == RunPhase::Running;
                }
                EventPayload::QueryStarted { operation }
                | EventPayload::QueryCompleted { operation }
                | EventPayload::QueryFailed { operation }
                | EventPayload::ReplicaOperationObserved { operation } => {
                    if let Some(slot) = r
                        .snapshot
                        .operations
                        .iter_mut()
                        .find(|o| o.operation_id == operation.operation_id)
                    {
                        *slot = operation.clone();
                    }
                }
                EventPayload::DbObserved { observation } => {
                    r.schema_observed |= observation.target == DatabaseTarget::Replica
                        && observation.metric == ObservationMetric::Marker
                        && observation.status == ObservationStatus::Available;
                    if let Some(slot) =
                        r.snapshot.observations.iter_mut().find(|o| {
                            o.target == observation.target && o.metric == observation.metric
                        })
                    {
                        *slot = observation.clone();
                    } else {
                        r.snapshot.observations.push(observation.clone());
                    }
                }
                EventPayload::StaleReadObserved { .. } => r.snapshot.stale_read_observed = true,
                EventPayload::RunCompleted { snapshot } | EventPayload::RunFailed { snapshot } => {
                    r.terminal = Some((**snapshot).clone());
                    return;
                }
                _ => return,
            }
            r.publish(event);
        });
    }

    fn seed_progress(&self, operation: &OperationState) {
        self.update(|r| {
            if let Some(slot) = r
                .snapshot
                .operations
                .iter_mut()
                .find(|o| o.operation_id == operation.operation_id)
            {
                *slot = operation.clone();
                r.publish(EventPayload::QueryProgress {
                    operation: operation.clone(),
                });
            }
        });
    }
}

fn validate_web(config: &RunConfig) -> anyhow::Result<()> {
    let [rows, seed, heavy, marker] = runner::parse_workload(config)?;
    let batches = rows.div_ceil(heavy);
    ensure!(rows <= 10_000_000, "demo_rows must be at most 10000000");
    ensure!(
        seed.min(rows) <= 5_000,
        "a seed INSERT may contain at most 5000 rows"
    );
    ensure!(batches <= 1_000, "at most 1000 heavy batches are allowed");
    ensure!(
        marker < batches,
        "at least one heavy batch must follow the marker"
    );
    Ok(())
}

fn error(code: &str, message: &str, retryable: bool) -> ApiError {
    ApiError {
        code: code.into(),
        message: message.into(),
        retryable,
    }
}
fn run_error(cause: &anyhow::Error) -> ApiError {
    match cause.downcast_ref::<PreflightFailure>() {
        Some(PreflightFailure::DatabaseUnavailable) => error(
            "db_unavailable",
            "Source or Replica could not be reached.",
            true,
        ),
        Some(PreflightFailure::ReplicationNotReady) => error(
            "replication_not_ready",
            "Configure and start replication before running the demo.",
            true,
        ),
        None => error("run_failed", "Demo execution failed.", false),
    }
}

async fn probe(target: DatabaseTarget, options: &MySqlConnectOptions) -> DatabaseConnection {
    let host = options.get_host().trim_matches(['[', ']']);
    let display_address = if host.contains(':') {
        format!("[{host}]:{}", options.get_port())
    } else {
        format!("{host}:{}", options.get_port())
    };
    let available = matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            MySqlConnection::connect_with(options)
        )
        .await,
        Ok(Ok(_))
    );
    DatabaseConnection {
        target,
        display_address,
        checked_at: Some(Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)),
        status: if available {
            ConnectionStatus::Available
        } else {
            ConnectionStatus::Unavailable
        },
        error: (!available)
            .then(|| error("db_unavailable", "Database could not be reached.", true)),
    }
}

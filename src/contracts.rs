//! Step 1 review scaffold: DTOs and signatures only.
//! Validation and state transitions deliberately remain unimplemented until approval.
use serde::{Deserialize, Serialize};

pub type DecimalString = String;
pub type UtcTimestamp = String;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    pub demo_rows: DecimalString,
    pub seed_batch_size: DecimalString,
    pub heavy_batch_size: DecimalString,
    pub marker_after_batches: DecimalString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunPhase {
    Idle,
    Preparing,
    WaitingInitialSync,
    Running,
    WaitingFinalSync,
    Failing,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseTarget {
    Source,
    Replica,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Setup,
    Seed,
    HeavyUpdate,
    MarkerUpdate,
    Observe,
    FinalSync,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlStatement {
    pub template: String,
    pub parameters: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationPlan {
    pub operation_id: String,
    pub kind: OperationKind,
    pub target: DatabaseTarget,
    pub batch_number: Option<DecimalString>,
    pub first_id: Option<DecimalString>,
    pub last_id: Option<DecimalString>,
    pub planned_rows: Option<DecimalString>,
    pub sql: SqlStatement,
    /// Required commits; this is not a total execution order.
    pub after_committed: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Planned,
    Running,
    Committed,
    Failed,
    OutcomeUnknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplicaApplicationStatus {
    Unknown,
    Pending,
    ObservedApplied,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationState {
    pub operation_id: String,
    pub status: OperationStatus,
    pub replica_status: ReplicaApplicationStatus,
    pub replica_observed_at: Option<UtcTimestamp>,
    pub started_at: Option<UtcTimestamp>,
    pub completed_at: Option<UtcTimestamp>,
    pub elapsed_ms: Option<f64>,
    pub affected_rows: Option<DecimalString>,
    pub error: Option<ApiError>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationMetric {
    TotalRows,
    UpdatedRows,
    Marker,
    Replication,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStatus {
    NotObserved,
    NotReady,
    Observing,
    Available,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicationState {
    pub receiver_running: bool,
    pub applier_running: bool,
    pub seconds_behind_source: Option<f64>,
    pub relay_log_bytes: DecimalString,
    pub source_log_file: String,
    pub read_source_log_pos: DecimalString,
    pub relay_source_log_file: String,
    pub exec_source_log_pos: DecimalString,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservationValue {
    Decimal(DecimalString),
    Replication(ReplicationState),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub target: DatabaseTarget,
    pub metric: ObservationMetric,
    pub status: ObservationStatus,
    pub value: Option<ObservationValue>,
    pub started_at: Option<UtcTimestamp>,
    /// Timestamp of the retained successful value, even after a later failure.
    pub observed_at: Option<UtcTimestamp>,
    pub elapsed_ms: Option<f64>,
    pub error: Option<ApiError>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadPreset {
    MarkerSum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualReadStatus {
    Pending,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualRead {
    pub read_id: String,
    pub run_id: String,
    pub target: DatabaseTarget,
    pub preset: ReadPreset,
    pub sql: String,
    pub status: ManualReadStatus,
    /// Succeeded + null means SQL NULL; status distinguishes pending/failure.
    pub value: Option<DecimalString>,
    pub started_at: UtcTimestamp,
    pub completed_at: Option<UtcTimestamp>,
    pub elapsed_ms: Option<f64>,
    pub error: Option<ApiError>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSnapshot {
    pub schema_version: u32,
    pub server_instance_id: String,
    pub run_id: String,
    /// A decimal string, so sequence precision does not depend on JavaScript numbers.
    pub sequence: DecimalString,
    pub phase: RunPhase,
    pub config: RunConfig,
    pub plan: Vec<OperationPlan>,
    pub operations: Vec<OperationState>,
    pub observations: Vec<Observation>,
    pub manual_reads: Vec<ManualRead>,
    pub stale_read_observed: bool,
    pub error: Option<ApiError>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    NotChecked,
    Checking,
    Available,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConnection {
    pub target: DatabaseTarget,
    /// Host and port only; never a connection URL containing credentials.
    pub display_address: String,
    pub status: ConnectionStatus,
    pub checked_at: Option<UtcTimestamp>,
    pub error: Option<ApiError>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DashboardState {
    pub schema_version: u32,
    pub server_instance_id: String,
    pub default_config: RunConfig,
    /// No current run means idle; it does not mean a previous run completed.
    pub current_run: Option<RunSnapshot>,
    pub connections: Vec<DatabaseConnection>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub schema_version: u32,
    pub run_id: String,
    pub sequence: DecimalString,
    pub occurred_at: UtcTimestamp,
    #[serde(flatten)]
    pub event: EventPayload,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum EventPayload {
    RunPhaseChanged { phase: RunPhase },
    QueryStarted { operation: OperationState },
    QueryCompleted { operation: OperationState },
    QueryFailed { operation: OperationState },
    DbObserved { observation: Observation },
    StaleReadObserved { source_value: DecimalString, replica_value: DecimalString },
    ManualReadStarted { read: ManualRead },
    ManualReadCompleted { read: ManualRead },
    ManualReadFailed { read: ManualRead },
    RunCompleted { snapshot: Box<RunSnapshot> },
    RunFailed { snapshot: Box<RunSnapshot> },
    Snapshot { snapshot: Box<RunSnapshot> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContractErrorCode {
    UnsupportedSchemaVersion,
    InvalidDecimal,
    InvalidTimestamp,
    InvalidDuration,
    InconsistentObservation,
    InconsistentRead,
    InconsistentOperation,
    InconsistentRun,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractError {
    pub code: ContractErrorCode,
    pub path: String,
}

/// Validate public fields and cross-field invariants, without issuing SQL.
/// Failure identifies a machine-readable code and the invalid field path.
pub fn validate_snapshot(_snapshot: &RunSnapshot) -> Result<(), ContractError> {
    todo!("Step 1: implement after contract/test approval")
}

/// Validate the event version, identifiers, timestamp, and payload consistency.
pub fn validate_event(_event: &EventEnvelope) -> Result<(), ContractError> {
    todo!("Step 1: implement after contract/test approval")
}

/// Test whether a requested phase change is permitted; no mutation or DB access.
/// Completion prerequisites are checked by the runner, not inferred from this graph.
pub fn can_transition(_from: RunPhase, _to: RunPhase) -> bool {
    todo!("Step 1: implement after contract/test approval")
}

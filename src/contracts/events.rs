//! 継続通知（SSE）の形式。画面の中心データを理解してから読む。
//! 通知の中身には親モジュールの型をそのまま使う。
use super::{
    DecimalString, ManualRead, Observation, OperationPlan, OperationState, RunPhase, RunSnapshot,
    UtcTimestamp,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "WireEnvelope")]
pub struct EventEnvelope {
    pub schema_version: u32,
    pub run_id: String,
    pub sequence: DecimalString,
    pub occurred_at: UtcTimestamp,
    #[serde(flatten)]
    pub event: EventPayload,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum EventPayload {
    RunPhaseChanged {
        phase: RunPhase,
    },
    QueryStarted {
        operation: OperationState,
    },
    QueryProgress {
        operation: OperationState,
    },
    QueryStatement {
        plan: OperationPlan,
    },
    QueryCompleted {
        operation: OperationState,
    },
    QueryFailed {
        operation: OperationState,
    },
    ReplicaOperationObserved {
        operation: OperationState,
    },
    DbObserved {
        observation: Observation,
    },
    StaleReadObserved {
        source_value: DecimalString,
        replica_value: DecimalString,
    },
    ManualReadStarted {
        read: ManualRead,
    },
    ManualReadCompleted {
        read: ManualRead,
    },
    ManualReadFailed {
        read: ManualRead,
    },
    RunCompleted {
        snapshot: Box<RunSnapshot>,
    },
    RunFailed {
        snapshot: Box<RunSnapshot>,
    },
    Snapshot {
        snapshot: Box<RunSnapshot>,
    },
}

impl EventPayload {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::RunPhaseChanged { .. } => "run_phase_changed",
            Self::QueryStarted { .. } => "query_started",
            Self::QueryProgress { .. } => "query_progress",
            Self::QueryStatement { .. } => "query_statement",
            Self::QueryCompleted { .. } => "query_completed",
            Self::QueryFailed { .. } => "query_failed",
            Self::ReplicaOperationObserved { .. } => "replica_operation_observed",
            Self::DbObserved { .. } => "db_observed",
            Self::StaleReadObserved { .. } => "stale_read_observed",
            Self::ManualReadStarted { .. } => "manual_read_started",
            Self::ManualReadCompleted { .. } => "manual_read_completed",
            Self::ManualReadFailed { .. } => "manual_read_failed",
            Self::RunCompleted { .. } => "run_completed",
            Self::RunFailed { .. } => "run_failed",
            Self::Snapshot { .. } => "snapshot",
        }
    }
}

// flattenとdeny_unknown_fieldsを併用せず、受信時だけ外側の項目を厳密に確認する。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireEnvelope {
    schema_version: u32,
    run_id: String,
    sequence: DecimalString,
    occurred_at: UtcTimestamp,
    r#type: String,
    payload: serde_json::Value,
}

impl TryFrom<WireEnvelope> for EventEnvelope {
    type Error = serde_json::Error;
    fn try_from(w: WireEnvelope) -> Result<Self, Self::Error> {
        let event =
            serde_json::from_value(serde_json::json!({"type": w.r#type, "payload": w.payload}))?;
        Ok(Self {
            schema_version: w.schema_version,
            run_id: w.run_id,
            sequence: w.sequence,
            occurred_at: w.occurred_at,
            event,
        })
    }
}

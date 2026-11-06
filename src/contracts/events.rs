//! 継続通知（SSE）の形式。画面の中心データを理解してから読む。
//! 通知の中身には親モジュールの型をそのまま使う。
use super::{
    DecimalString, ManualRead, Observation, OperationState, RunPhase, RunSnapshot, UtcTimestamp,
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
    QueryCompleted {
        operation: OperationState,
    },
    QueryFailed {
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

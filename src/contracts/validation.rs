//! 通信データの形式と整合性だけを検証する。DB操作や状態変更は行わない。
use super::*;
use std::collections::HashSet;

type Result = std::result::Result<(), ContractError>;
fn require(ok: bool, code: ContractErrorCode, path: &str) -> Result {
    if ok {
        Ok(())
    } else {
        Err(ContractError {
            code,
            path: path.into(),
        })
    }
}
fn decimal(value: &str, path: &str) -> Result {
    require(
        value == "0"
            || (!value.starts_with('0')
                && !value.is_empty()
                && value.bytes().all(|c| c.is_ascii_digit())),
        ContractErrorCode::InvalidDecimal,
        path,
    )
}
fn duration(value: Option<f64>, path: &str) -> Result {
    require(
        value.is_none_or(|v| v.is_finite() && v >= 0.0),
        ContractErrorCode::InvalidDuration,
        path,
    )
}
fn timestamp(value: Option<&str>, path: &str) -> Result {
    let Some(v) = value else { return Ok(()) };
    let valid = || -> Option<bool> {
        if !v.is_ascii() || v.len() < 20 || !v.ends_with('Z') {
            return None;
        }
        for (i, c) in [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')] {
            if v.as_bytes()[i] != c {
                return None;
            }
        }
        let n = |a, b| {
            let part = &v[a..b];
            if !part.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            part.parse::<u32>().ok()
        };
        let (y, m, d, h, min, s) = (
            n(0, 4)?,
            n(5, 7)?,
            n(8, 10)?,
            n(11, 13)?,
            n(14, 16)?,
            n(17, 19)?,
        );
        let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let days = match m {
            2 => {
                if leap {
                    29
                } else {
                    28
                }
            }
            4 | 6 | 9 | 11 => 30,
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            _ => 0,
        };
        let tail = &v[19..v.len() - 1];
        Some(
            d > 0
                && d <= days
                && h < 24
                && min < 60
                && s < 60
                && (tail.is_empty()
                    || (tail.starts_with('.')
                        && tail.len() > 1
                        && tail[1..].bytes().all(|c| c.is_ascii_digit()))),
        )
    };
    require(
        valid() == Some(true),
        ContractErrorCode::InvalidTimestamp,
        path,
    )
}
fn identifier(value: &str, path: &str) -> Result {
    require(
        !value.trim().is_empty(),
        ContractErrorCode::InconsistentRun,
        path,
    )
}
fn config(c: &RunConfig) -> Result {
    for (name, v) in [
        ("demo_rows", &c.demo_rows),
        ("seed_batch_size", &c.seed_batch_size),
        ("heavy_batch_size", &c.heavy_batch_size),
        ("marker_after_batches", &c.marker_after_batches),
    ] {
        decimal(v, &format!("config.{name}"))?;
    }
    Ok(())
}
fn operation(o: &OperationState) -> Result {
    identifier(&o.operation_id, "operation.operation_id")?;
    timestamp(o.started_at.as_deref(), "operation.started_at")?;
    timestamp(o.completed_at.as_deref(), "operation.completed_at")?;
    timestamp(
        o.replica_observed_at.as_deref(),
        "operation.replica_observed_at",
    )?;
    duration(o.elapsed_ms, "operation.elapsed_ms")?;
    if let Some(v) = &o.affected_rows {
        decimal(v, "operation.affected_rows")?;
    }
    require(
        o.status != OperationStatus::Committed
            || (o.started_at.is_some()
                && o.completed_at.is_some()
                && o.elapsed_ms.is_some()
                && o.affected_rows.is_some()
                && o.error.is_none()),
        ContractErrorCode::InconsistentOperation,
        "operation.committed",
    )?;
    require(
        o.replica_status != ReplicaApplicationStatus::ObservedApplied
            || o.replica_observed_at.is_some(),
        ContractErrorCode::InconsistentOperation,
        "operation.replica_observed_at",
    )
}
fn observation(o: &Observation) -> Result {
    timestamp(o.started_at.as_deref(), "observation.started_at")?;
    timestamp(o.observed_at.as_deref(), "observation.observed_at")?;
    duration(o.elapsed_ms, "observation.elapsed_ms")?;
    if let Some(v) = &o.value {
        match v {
            ObservationValue::Decimal(v) => {
                require(
                    o.metric != ObservationMetric::Replication,
                    ContractErrorCode::InconsistentObservation,
                    "observation.metric",
                )?;
                decimal(v, "observation.value")?;
            }
            ObservationValue::Replication(r) => {
                require(
                    o.metric == ObservationMetric::Replication,
                    ContractErrorCode::InconsistentObservation,
                    "observation.metric",
                )?;
                duration(r.seconds_behind_source, "observation.seconds_behind_source")?;
                for v in [
                    &r.relay_log_bytes,
                    &r.read_source_log_pos,
                    &r.exec_source_log_pos,
                ] {
                    decimal(v, "observation.log_position")?;
                }
            }
        }
    }
    use ObservationStatus::*;
    let valid = match o.status {
        NotObserved => {
            o.value.is_none()
                && o.observed_at.is_none()
                && o.started_at.is_none()
                && o.elapsed_ms.is_none()
                && o.error.is_none()
        }
        NotReady => o.value.is_none() && o.observed_at.is_none() && o.error.is_some(),
        Available => {
            o.value.is_some()
                && o.observed_at.is_some()
                && o.started_at.is_some()
                && o.elapsed_ms.is_some()
                && o.error.is_none()
        }
        Observing => {
            o.value.is_some() == o.observed_at.is_some()
                && o.started_at.is_some()
                && o.error.is_none()
        }
        Unavailable => o.value.is_some() == o.observed_at.is_some() && o.error.is_some(),
    };
    require(
        valid,
        ContractErrorCode::InconsistentObservation,
        "observation.status",
    )
}
fn manual_read(r: &ManualRead, run: &str) -> Result {
    identifier(&r.read_id, "read.read_id")?;
    timestamp(Some(&r.started_at), "read.started_at")?;
    timestamp(r.completed_at.as_deref(), "read.completed_at")?;
    duration(r.elapsed_ms, "read.elapsed_ms")?;
    if let Some(v) = &r.value {
        decimal(v, "read.value")?;
    }
    let valid = r.run_id == run
        && r.target == DatabaseTarget::Replica
        && r.sql == "SELECT SUM(value) FROM lag_demo.marker;"
        && match r.status {
            ManualReadStatus::Pending => {
                r.value.is_none()
                    && r.completed_at.is_none()
                    && r.elapsed_ms.is_none()
                    && r.error.is_none()
            }
            ManualReadStatus::Succeeded => {
                r.completed_at.is_some() && r.elapsed_ms.is_some() && r.error.is_none()
            }
            ManualReadStatus::Failed => {
                r.value.is_none()
                    && r.completed_at.is_some()
                    && r.elapsed_ms.is_some()
                    && r.error.is_some()
            }
        };
    require(valid, ContractErrorCode::InconsistentRead, "read.status")
}

pub fn validate_snapshot(s: &RunSnapshot) -> Result {
    require(
        s.schema_version == 1,
        ContractErrorCode::UnsupportedSchemaVersion,
        "schema_version",
    )?;
    identifier(&s.server_instance_id, "server_instance_id")?;
    identifier(&s.run_id, "run_id")?;
    decimal(&s.sequence, "sequence")?;
    config(&s.config)?;
    let mut ids = HashSet::new();
    for p in &s.plan {
        identifier(&p.operation_id, "plan.operation_id")?;
        require(
            ids.insert(&p.operation_id),
            ContractErrorCode::InconsistentOperation,
            "plan.operation_id",
        )?;
        for v in [&p.batch_number, &p.first_id, &p.last_id, &p.planned_rows]
            .into_iter()
            .flatten()
        {
            decimal(v, "plan.range")?;
        }
    }
    for p in &s.plan {
        require(
            p.after_committed
                .iter()
                .all(|id| id != &p.operation_id && ids.contains(id)),
            ContractErrorCode::InconsistentOperation,
            "plan.after_committed",
        )?;
    }
    let mut seen = HashSet::new();
    for o in &s.operations {
        operation(o)?;
        require(
            ids.contains(&o.operation_id) && seen.insert(&o.operation_id),
            ContractErrorCode::InconsistentOperation,
            "operations.operation_id",
        )?;
    }
    for o in &s.observations {
        observation(o)?;
    }
    let mut reads = HashSet::new();
    for r in &s.manual_reads {
        manual_read(r, &s.run_id)?;
        require(
            reads.insert(&r.read_id),
            ContractErrorCode::InconsistentRead,
            "manual_reads.read_id",
        )?;
    }
    if s.phase == RunPhase::Completed {
        completed(s)?;
    }
    Ok(())
}
fn completed(s: &RunSnapshot) -> Result {
    let value = |target, metric| -> Option<&ObservationValue> {
        let mut rows = s
            .observations
            .iter()
            .filter(|o| o.target == target && o.metric == metric);
        let o = rows.next()?;
        if rows.next().is_some() || o.status != ObservationStatus::Available {
            return None;
        }
        o.value.as_ref()
    };
    let mut valid = s.error.is_none()
        && !s.plan.is_empty()
        && s.operations.len() == s.plan.len()
        && s.operations
            .iter()
            .all(|o| o.status == OperationStatus::Committed);
    for target in [DatabaseTarget::Source, DatabaseTarget::Replica] {
        for metric in [ObservationMetric::TotalRows, ObservationMetric::UpdatedRows] {
            valid &= matches!(value(target,metric),Some(ObservationValue::Decimal(v)) if v == &s.config.demo_rows);
        }
        valid &= matches!(value(target,ObservationMetric::Marker),Some(ObservationValue::Decimal(v)) if v == "1");
    }
    valid &= matches!(value(DatabaseTarget::Replica,ObservationMetric::Replication),Some(ObservationValue::Replication(r)) if r.receiver_running && r.applier_running);
    require(valid, ContractErrorCode::InconsistentRun, "completed")
}

pub fn validate_event(e: &EventEnvelope) -> Result {
    require(
        e.schema_version == 1,
        ContractErrorCode::UnsupportedSchemaVersion,
        "schema_version",
    )?;
    identifier(&e.run_id, "run_id")?;
    decimal(&e.sequence, "sequence")?;
    timestamp(Some(&e.occurred_at), "occurred_at")?;
    use EventPayload::*;
    match &e.event {
        RunPhaseChanged { .. } => Ok(()),
        QueryStarted { operation: o }
        | QueryProgress { operation: o }
        | QueryCompleted { operation: o }
        | QueryFailed { operation: o } => {
            operation(o)?;
            let valid = match &e.event {
                QueryStarted { .. } => o.status == OperationStatus::Running,
                QueryProgress { .. } => {
                    o.status == OperationStatus::Running && o.affected_rows.is_some()
                }
                QueryCompleted { .. } => o.status == OperationStatus::Committed,
                _ => matches!(
                    o.status,
                    OperationStatus::Failed | OperationStatus::OutcomeUnknown
                ),
            };
            require(
                valid,
                ContractErrorCode::InconsistentOperation,
                "payload.operation.status",
            )
        }
        ReplicaOperationObserved { operation: o } => {
            operation(o)?;
            require(
                o.status == OperationStatus::Committed
                    && o.replica_status != ReplicaApplicationStatus::Unknown
                    && o.replica_observed_at.is_some(),
                ContractErrorCode::InconsistentOperation,
                "payload.operation.replica_status",
            )
        }
        DbObserved { observation: o } => observation(o),
        StaleReadObserved {
            source_value,
            replica_value,
        } => {
            decimal(source_value, "payload.source_value")?;
            decimal(replica_value, "payload.replica_value")?;
            require(
                source_value == "1" && replica_value != source_value,
                ContractErrorCode::InconsistentObservation,
                "payload.stale",
            )
        }
        ManualReadStarted { read } | ManualReadCompleted { read } | ManualReadFailed { read } => {
            manual_read(read, &e.run_id)?;
            let status = match &e.event {
                ManualReadStarted { .. } => ManualReadStatus::Pending,
                ManualReadCompleted { .. } => ManualReadStatus::Succeeded,
                _ => ManualReadStatus::Failed,
            };
            require(
                read.status == status,
                ContractErrorCode::InconsistentRead,
                "payload.read.status",
            )
        }
        RunCompleted { snapshot } | RunFailed { snapshot } | Snapshot { snapshot } => {
            validate_snapshot(snapshot)?;
            let valid = snapshot.run_id == e.run_id
                && snapshot.sequence == e.sequence
                && match &e.event {
                    RunCompleted { .. } => snapshot.phase == RunPhase::Completed,
                    RunFailed { .. } => snapshot.phase == RunPhase::Failed,
                    _ => true,
                };
            require(
                valid,
                ContractErrorCode::InconsistentRun,
                "payload.snapshot",
            )
        }
    }
}

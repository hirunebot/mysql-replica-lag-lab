//! 通信契約の正常系と、不正な形式・状態の拒否を検証する。
use mysql_replica_lag_lab::contracts::*;
use serde_json::{Value, json};

fn snapshot() -> RunSnapshot {
    serde_json::from_str(include_str!("fixtures/dashboard/snapshot.json")).unwrap()
}

fn observation_cases() -> Value {
    serde_json::from_str(include_str!("fixtures/dashboard/observations.json")).unwrap()
}

fn read_cases() -> Value {
    serde_json::from_str(include_str!("fixtures/dashboard/manual-reads.json")).unwrap()
}

/// 共有JSONとRustのシリアライズ結果が一致し、認証情報の項目を受け付けない。
#[test]
fn shared_json_matches_rust_serialization_without_credential_fields() {
    let wire: Value =
        serde_json::from_str(include_str!("fixtures/dashboard/snapshot.json")).unwrap();
    let dto: RunSnapshot = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(dto).unwrap(), wire);
    let mut with_secret = wire;
    with_secret["source_database_url"] = json!("mysql://private");
    assert!(serde_json::from_value::<RunSnapshot>(with_secret).is_err());
}

/// 正常なスナップショットを受け入れ、大きな整数文字列を精度を失わず保持する。
#[test]
fn accepts_normal_snapshot_and_lossless_large_integer_strings() {
    let mut dto = snapshot();
    assert_eq!(validate_snapshot(&dto), Ok(()));
    dto.sequence = "9007199254740993".into();
    dto.observations[0].value = Some(ObservationValue::Decimal("18446744073709551615".into()));
    assert_eq!(validate_snapshot(&dto), Ok(()));
    assert_eq!(
        serde_json::to_value(dto).unwrap()["sequence"],
        "9007199254740993"
    );
}

/// 未取得・値0・テーブル未作成・取得失敗を区別する。
#[test]
fn distinguishes_missing_zero_and_failed_observations() {
    for case in observation_cases().as_object().unwrap().values() {
        let observation: Observation = serde_json::from_value(case.clone()).unwrap();
        let mut dto = snapshot();
        dto.observations = vec![observation];
        assert_eq!(validate_snapshot(&dto), Ok(()));
        assert_eq!(serde_json::to_value(&dto.observations[0]).unwrap(), *case);
    }
}

/// 手動SELECTのSQL NULL・取得中・失敗を区別する。
#[test]
fn distinguishes_sql_null_from_pending_and_failed_manual_reads() {
    for case in read_cases().as_object().unwrap().values() {
        let mut dto = snapshot();
        dto.manual_reads = vec![serde_json::from_value(case.clone()).unwrap()];
        assert_eq!(validate_snapshot(&dto), Ok(()));
        assert_eq!(serde_json::to_value(&dto.manual_reads[0]).unwrap(), *case);
    }
}

/// 負数・小数・先頭ゼロ・指数表記・空文字の10進文字列を拒否する。
#[test]
fn rejects_noncanonical_decimal_strings() {
    for invalid in ["-1", "1.5", "01", "1e3", ""] {
        let mut dto = snapshot();
        dto.config.demo_rows = invalid.into();
        assert_eq!(
            validate_snapshot(&dto).unwrap_err().code,
            ContractErrorCode::InvalidDecimal
        );
    }
}

/// 実在しない日時とUTC以外の時刻を拒否する。
#[test]
fn rejects_invalid_calendar_dates_and_non_utc_timestamps() {
    for invalid in [
        "invalid",
        "+026-11-01T09:00:00Z",
        "2026-02-30T09:00:00Z",
        "2026-11-01T18:00:00+09:00",
    ] {
        let mut dto = snapshot();
        dto.observations[0].observed_at = Some(invalid.into());
        assert_eq!(
            validate_snapshot(&dto).unwrap_err().code,
            ContractErrorCode::InvalidTimestamp
        );
    }
}

/// 負の値・無限大・NaNの所要時間を拒否する。
#[test]
fn rejects_negative_or_nonfinite_duration() {
    for invalid in [-1.0, f64::INFINITY, f64::NAN] {
        let mut dto = snapshot();
        dto.observations[0].elapsed_ms = Some(invalid);
        assert_eq!(
            validate_snapshot(&dto).unwrap_err().code,
            ContractErrorCode::InvalidDuration
        );
    }
}

/// 取得成功なのに値がない観測を拒否する。
#[test]
fn rejects_available_observation_without_value() {
    let mut dto = snapshot();
    dto.observations[0].value = None;
    assert_eq!(
        validate_snapshot(&dto).unwrap_err().code,
        ContractErrorCode::InconsistentObservation
    );
}

/// 別の実行に紐づく手動SELECT結果を拒否する。
#[test]
fn rejects_manual_reads_from_another_run() {
    let mut dto = snapshot();
    let mut read: ManualRead = serde_json::from_value(read_cases()["zero"].clone()).unwrap();
    read.run_id = "other-run".into();
    dto.manual_reads.push(read);
    assert_eq!(
        validate_snapshot(&dto).unwrap_err().code,
        ContractErrorCode::InconsistentRead
    );
}

/// コミット済みなのに完了記録がない処理を拒否する。
#[test]
fn rejects_committed_operation_without_completion_record() {
    let mut dto = snapshot();
    dto.operations[0].completed_at = None;
    assert_eq!(
        validate_snapshot(&dto).unwrap_err().code,
        ContractErrorCode::InconsistentOperation
    );
}

/// 未対応のスナップショットのバージョンを拒否する。
#[test]
fn rejects_unsupported_snapshot_version() {
    let mut dto = snapshot();
    dto.schema_version = 2;
    assert_eq!(
        validate_snapshot(&dto).unwrap_err().code,
        ContractErrorCode::UnsupportedSchemaVersion
    );
}

/// 共有SSEイベントを検証し、シリアライズしても内容を失わない。
#[test]
fn shared_events_roundtrip_and_validate() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/dashboard/events.json")).unwrap();
    for case in cases {
        let event: EventEnvelope = serde_json::from_value(case.clone()).unwrap();
        assert_eq!(validate_event(&event), Ok(()));
        assert_eq!(serde_json::to_value(event).unwrap(), case);
    }
}

/// 通常の進行と後始末を許可し、早すぎる完了や不正な再開を拒否する。
#[test]
fn phase_graph_allows_progress_and_cleanup_but_not_early_completion() {
    use RunPhase::*;
    for (from, to) in [
        (Idle, Preparing),
        (Preparing, WaitingInitialSync),
        (WaitingInitialSync, Running),
        (Running, WaitingFinalSync),
        (WaitingFinalSync, Completed),
        (Running, Failing),
        (Failing, Failed),
    ] {
        assert!(can_transition(from, to), "{from:?} -> {to:?}");
    }
    for (from, to) in [
        (Running, Completed),
        (Running, Failed),
        (Failing, Preparing),
        (Failed, Preparing),
        (Completed, Running),
        (Running, Running),
    ] {
        assert!(!can_transition(from, to), "{from:?} -> {to:?}");
    }
}

/// markerの一致だけで全行更新の確認がない完了状態を拒否する。
#[test]
fn rejects_completed_run_with_only_matching_markers() {
    let mut dto = snapshot();
    dto.phase = RunPhase::Completed;
    dto.observations[1].value = Some(ObservationValue::Decimal("1".into()));
    assert_eq!(
        validate_snapshot(&dto).unwrap_err().code,
        ContractErrorCode::InconsistentRun
    );
}

/// 観測項目と値の種類が一致しない場合は拒否する。
#[test]
fn rejects_metric_value_kind_mismatch() {
    let mut dto = snapshot();
    dto.observations[0].metric = ObservationMetric::Replication;
    assert_eq!(
        validate_snapshot(&dto).unwrap_err().code,
        ContractErrorCode::InconsistentObservation
    );
}

/// 実行前のDB接続失敗を、実行を捏造せず表現できる。
#[test]
fn idle_state_can_report_unreachable_database_without_a_run() {
    let wire: Value =
        serde_json::from_str(include_str!("fixtures/dashboard/idle-state.json")).unwrap();
    let state: DashboardState = serde_json::from_value(wire.clone()).unwrap();
    assert!(state.current_run.is_none());
    assert_eq!(state.connections[1].status, ConnectionStatus::Unavailable);
    assert_eq!(serde_json::to_value(state).unwrap(), wire);
}

fn completed_snapshot() -> RunSnapshot {
    let mut s = snapshot();
    s.phase = RunPhase::Completed;
    s.stale_read_observed = false;
    let committed = s.operations[0].clone();
    for o in &mut s.operations {
        let id = o.operation_id.clone();
        *o = committed.clone();
        o.operation_id = id;
    }
    let template = s.observations[0].clone();
    s.observations.clear();
    for target in [DatabaseTarget::Source, DatabaseTarget::Replica] {
        for metric in [
            ObservationMetric::TotalRows,
            ObservationMetric::UpdatedRows,
            ObservationMetric::Marker,
        ] {
            let mut o = template.clone();
            o.target = target;
            o.metric = metric;
            o.value = Some(ObservationValue::Decimal(
                if metric == ObservationMetric::Marker {
                    "1".into()
                } else {
                    s.config.demo_rows.clone()
                },
            ));
            s.observations.push(o);
        }
    }
    s.observations.push(
        serde_json::from_value(observation_cases()["unknown_replication_lag"].clone()).unwrap(),
    );
    s
}

/// stale read未観測でも、全更新が同期済みなら完了を受け入れる。
#[test]
fn accepts_completed_run_without_stale_read() {
    assert_eq!(validate_snapshot(&completed_snapshot()), Ok(()));
}

/// 正常な完了例の各根拠を一つずつ壊して拒否を確認する。
#[test]
fn rejects_incomplete_or_unhealthy_completion() {
    for case in 0..4 {
        let mut s = completed_snapshot();
        match case {
            0 => s.observations[4].value = Some(ObservationValue::Decimal("0".into())),
            1 => s.observations[5].status = ObservationStatus::Unavailable,
            2 => {
                let Some(ObservationValue::Replication(r)) = &mut s.observations[6].value else {
                    unreachable!()
                };
                r.applier_running = false;
            }
            _ => s.operations[0].status = OperationStatus::Running,
        }
        assert!(validate_snapshot(&s).is_err(), "case={case}");
    }
}

/// nullを明示する項目は、省略と同一視しない。
#[test]
fn rejects_missing_nullable_fields() {
    let mut wire = serde_json::to_value(snapshot()).unwrap();
    wire["observations"][0]
        .as_object_mut()
        .unwrap()
        .remove("error");
    assert!(serde_json::from_value::<RunSnapshot>(wire).is_err());
    let mut idle: Value =
        serde_json::from_str(include_str!("fixtures/dashboard/idle-state.json")).unwrap();
    idle.as_object_mut().unwrap().remove("current_run");
    assert!(serde_json::from_value::<DashboardState>(idle).is_err());
}

/// SSEも外側・payload内の未知項目を黙って捨てず拒否する。
#[test]
fn rejects_unknown_event_fields_and_inconsistent_status() {
    let events: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/dashboard/events.json")).unwrap();
    let mut extra = events[0].clone();
    extra["source_database_url"] = json!("mysql://private");
    assert!(serde_json::from_value::<EventEnvelope>(extra).is_err());
    let mut extra = events[0].clone();
    extra["payload"]["extra"] = json!(true);
    assert!(serde_json::from_value::<EventEnvelope>(extra).is_err());
    let mut event: EventEnvelope = serde_json::from_value(events[2].clone()).unwrap();
    let EventPayload::ManualReadCompleted { read } = &mut event.event else {
        unreachable!()
    };
    read.status = ManualReadStatus::Pending;
    assert!(validate_event(&event).is_err());
}

/// 表で定義した全64組の遷移を確認する。
#[test]
fn phase_graph_matches_entire_contract() {
    use RunPhase::*;
    let phases = [
        Idle,
        Preparing,
        WaitingInitialSync,
        Running,
        WaitingFinalSync,
        Failing,
        Completed,
        Failed,
    ];
    let allowed = [
        (Idle, Preparing),
        (Preparing, WaitingInitialSync),
        (Preparing, Failing),
        (WaitingInitialSync, Running),
        (WaitingInitialSync, Failing),
        (Running, WaitingFinalSync),
        (Running, Failing),
        (WaitingFinalSync, Completed),
        (WaitingFinalSync, Failing),
        (Failing, Failed),
    ];
    for from in phases {
        for to in phases {
            assert_eq!(
                can_transition(from, to),
                allowed.contains(&(from, to)),
                "{from:?}->{to:?}"
            );
        }
    }
}

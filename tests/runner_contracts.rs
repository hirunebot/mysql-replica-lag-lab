//! 共通runnerの計画・SQL・事前検証をDBに接続せず確認する。
use mysql_replica_lag_lab::{
    contracts::{DatabaseTarget, OperationKind, RunConfig},
    queries, runner,
};

fn config() -> RunConfig {
    RunConfig {
        demo_rows: "41".into(),
        seed_batch_size: "10".into(),
        heavy_batch_size: "20".into(),
        marker_after_batches: "2".into(),
    }
}

/// 各行を重複・欠落なく覆い、端数で最終IDを超えない。
#[test]
fn batch_ranges_cover_partial_final_batch() {
    let ranges: Vec<_> = (1..=3).map(|i| runner::batch_range(i, 41, 20)).collect();
    assert_eq!(ranges, vec![Some((1, 20)), Some((21, 40)), Some((41, 41))]);
}

/// 0・範囲外・乗算オーバーフローでpanicや誤った範囲を返さない。
#[test]
fn rejects_invalid_batch_ranges() {
    for (batch, rows, size) in [
        (0, 41, 20),
        (1, 0, 20),
        (1, 41, 0),
        (4, 41, 20),
        (u64::MAX, u64::MAX, 2),
    ] {
        assert_eq!(runner::batch_range(batch, rows, size), None);
    }
}

/// 予定には端数バッチを含み、markerと後続バッチは同じ先行コミットを待つ。
#[test]
fn plan_tracks_ranges_and_marker_dependencies_without_total_order() {
    let plan = runner::build_plan(&config()).unwrap();
    let heavy: Vec<_> = plan
        .iter()
        .filter(|p| p.kind == OperationKind::HeavyUpdate)
        .collect();
    assert_eq!(heavy.len(), 3);
    assert_eq!(heavy[2].first_id.as_deref(), Some("41"));
    assert_eq!(heavy[2].last_id.as_deref(), Some("41"));
    assert_eq!(heavy[2].planned_rows.as_deref(), Some("1"));
    assert_eq!(
        heavy[2].after_committed,
        vec![heavy[1].operation_id.clone()]
    );
    let marker = plan
        .iter()
        .find(|p| p.kind == OperationKind::MarkerUpdate)
        .unwrap();
    assert_eq!(marker.after_committed, vec![heavy[1].operation_id.clone()]);
    assert!(!heavy[2].after_committed.contains(&marker.operation_id));
    assert_eq!(marker.target, DatabaseTarget::Source);
    assert_eq!(marker.sql, queries::marker_update());
    assert_eq!(heavy[2].sql, queries::heavy_update(41, 41));
    assert_eq!(
        plan.iter()
            .filter(|p| p.kind == OperationKind::Seed)
            .count(),
        1
    );
    assert_eq!(
        plan.iter()
            .filter(|p| p.kind == OperationKind::Setup)
            .count(),
        1
    );
    assert_eq!(
        plan.iter()
            .filter(|p| p.kind == OperationKind::FinalSync)
            .count(),
        1
    );
}

/// 既存CLIはmarkerを最終バッチ後に投入する設定も許可する。
#[test]
fn accepts_marker_after_final_batch_for_cli_compatibility() {
    let mut c = config();
    c.marker_after_batches = "3".into();
    assert!(runner::build_plan(&c).is_ok());
}

/// 不正な数値・0・バッチ数を超えるmarker位置・u64範囲外を実行前に拒否する。
#[test]
fn rejects_invalid_execution_settings() {
    for field in ["rows", "seed", "batch", "marker"] {
        for value in ["0", "-1", "01", "1.5", "18446744073709551616"] {
            let mut c = config();
            match field {
                "rows" => c.demo_rows = value.into(),
                "seed" => c.seed_batch_size = value.into(),
                "batch" => c.heavy_batch_size = value.into(),
                _ => c.marker_after_batches = value.into(),
            }
            assert!(runner::build_plan(&c).is_err(), "{field}={value}");
        }
    }
    let mut c = config();
    c.marker_after_batches = "4".into();
    assert!(runner::build_plan(&c).is_err());
}

/// 表示予定と実行で共有するSQLは、現行の更新内容を省略しない。
#[test]
fn shared_sql_preserves_payload_and_marker_updates() {
    let update = queries::heavy_update(21, 40);
    let normalized = update
        .template
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(
        normalized,
        "UPDATE lag_demo.items SET version = version + 1, payload = CONCAT( SHA2(CONCAT(payload, ':', id, ':', version), 256), REPEAT('z', 448) ) WHERE id BETWEEN ? AND ?"
    );
    assert_eq!(update.parameters, vec!["21", "40"]);
    let marker = queries::marker_update();
    assert_eq!(
        marker.template,
        "UPDATE lag_demo.marker SET value = value + 1, updated_at = CURRENT_TIMESTAMP(6) WHERE id = 1"
    );
    assert!(marker.parameters.is_empty());
}

/// 不正設定では計画の公開も接続も始めない。
#[tokio::test]
async fn invalid_workload_is_rejected_before_start() {
    use mysql_replica_lag_lab::{
        contracts::{EventPayload, OperationPlan},
        events::Reporter,
    };
    use std::{sync::Arc, time::Duration};
    struct NoEvents;
    impl Reporter for NoEvents {
        fn plan(&self, _: &[OperationPlan]) {
            panic!("invalid config produced a plan")
        }
        fn report(&self, _: EventPayload) {
            panic!("invalid config started the run")
        }
    }
    let mut workload = config();
    workload.heavy_batch_size = "0".into();
    let result = runner::run(
        runner::RunnerConfig {
            run_id: "invalid-config".into(),
            server_instance_id: "test".into(),
            source_database_url: "not-a-url".into(),
            replica_database_url: "also-not-a-url".into(),
            workload,
            poll_interval: Duration::from_millis(1),
            initial_sync_timeout: Duration::from_secs(1),
            final_sync_timeout: Duration::from_secs(1),
        },
        Arc::new(NoEvents),
    )
    .await;
    assert!(result.unwrap_err().to_string().contains("HEAVY_BATCH_SIZE"));
}

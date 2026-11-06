//! ローカルデモ専用。明示実行するとlag_demoを作り直す。
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use mysql_replica_lag_lab::{
    contracts::*,
    events::Reporter,
    runner::{self, RunnerConfig},
};
use sqlx::MySqlPool;

const SOURCE: &str = "mysql://root:root@127.0.0.1:3307/mysql";
const REPLICA: &str = "mysql://root:root@127.0.0.1:3308/mysql";
const ROWS: u64 = 30_001;
const BATCH: u64 = 1_000;

#[derive(Default)]
struct Recorded {
    events: Mutex<Vec<EventPayload>>,
    statements: Mutex<Vec<OperationPlan>>,
    seed: Mutex<Vec<u64>>,
    fail_marker_on: Option<MySqlPool>,
}
impl Reporter for Recorded {
    fn report(&self, event: EventPayload) {
        if let Some(pool) = &self.fail_marker_on
            && matches!(
                &event,
                EventPayload::RunPhaseChanged {
                    phase: RunPhase::Running
                }
            )
        {
            // このテストのSourceだけに故障を注入。初期同期の後、UPDATEの前に作る。
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                sqlx::raw_sql("CREATE TRIGGER lag_demo.reject_marker BEFORE UPDATE ON lag_demo.marker FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT = 'runner test marker failure'")
                    .execute(pool).await.expect("install marker fault on local demo Source");
            })
            });
        }
        self.events.lock().unwrap().push(event);
    }
    fn statement(&self, statement: &OperationPlan) {
        self.statements.lock().unwrap().push(statement.clone());
    }
    fn seed_progress(&self, operation: &OperationState) {
        self.seed
            .lock()
            .unwrap()
            .push(operation.affected_rows.as_ref().unwrap().parse().unwrap());
    }
}

fn config(id: &str) -> RunnerConfig {
    RunnerConfig {
        run_id: id.into(),
        server_instance_id: "runner-test".into(),
        source_database_url: SOURCE.into(),
        replica_database_url: REPLICA.into(),
        workload: RunConfig {
            demo_rows: ROWS.to_string(),
            seed_batch_size: "1000".into(),
            heavy_batch_size: BATCH.to_string(),
            marker_after_batches: "2".into(),
        },
        poll_interval: Duration::from_millis(50),
        initial_sync_timeout: Duration::from_secs(60),
        final_sync_timeout: Duration::from_secs(60),
    }
}

fn verify_events(events: &[EventPayload], id: &str) {
    for (i, event) in events.iter().enumerate() {
        validate_event(&EventEnvelope {
            schema_version: 1,
            run_id: id.into(),
            sequence: (i + 1).to_string(),
            occurred_at: "2026-11-06T00:00:00Z".into(),
            event: event.clone(),
        })
        .unwrap();
    }
}

/// markerの実SQLを失敗させた後、ワーカーの終了を確認。続けて正常系の通知も検証する。
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires local Docker MySQL and recreates lag_demo"]
async fn runner_reports_real_sql_and_drains_after_marker_failure() {
    let source = MySqlPool::connect(SOURCE).await.unwrap();
    let failing = Arc::new(Recorded {
        fail_marker_on: Some(source.clone()),
        ..Default::default()
    });
    let result = runner::run(config("marker-failure"), failing.clone()).await;
    sqlx::raw_sql("DROP TRIGGER IF EXISTS lag_demo.reject_marker")
        .execute(&source)
        .await
        .unwrap();
    let error = result.unwrap_err();
    assert!(format!("{error:#}").contains("runner test marker failure"));
    {
        let events = failing.events.lock().unwrap();
        verify_events(&events, "marker-failure");
        let failed = events.iter().position(|e| matches!(e,EventPayload::QueryFailed {operation:o} if o.operation_id=="marker" && o.status==OperationStatus::Failed)).unwrap();
        assert!(events[failed + 1..].iter().any(|e| matches!(
            e,
            EventPayload::RunPhaseChanged {
                phase: RunPhase::Failing
            }
        )));
        assert!(!events[failed+1..].iter().any(|e| matches!(e,EventPayload::QueryStarted {operation:o} if o.operation_id.starts_with("batch-"))));
        let Some(EventPayload::RunFailed { snapshot }) = events.last() else {
            panic!("missing terminal failure")
        };
        assert!(
            snapshot
                .operations
                .iter()
                .all(|o| o.status != OperationStatus::Running)
        );
        assert!(
            snapshot
                .operations
                .iter()
                .any(|o| o.operation_id.starts_with("batch-")
                    && o.status == OperationStatus::Planned)
        );
    }
    let updated: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lag_demo.items WHERE version=2")
        .fetch_one(&source)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lag_demo.items WHERE version=2")
        .fetch_one(&source)
        .await
        .unwrap();
    assert_eq!(after, updated, "writes continued after the runner returned");
    assert!((2 * BATCH..ROWS).contains(&(after as u64)));

    let ok = Arc::new(Recorded::default());
    let outcome = runner::run(config("normal-run"), ok.clone()).await.unwrap();
    assert_eq!(outcome.affected_rows, ROWS);
    assert_eq!(outcome.batches, ROWS.div_ceil(BATCH));
    assert_eq!(ok.seed.lock().unwrap().last(), Some(&ROWS));
    let events = ok.events.lock().unwrap();
    verify_events(&events, "normal-run");
    let committed = events
        .iter()
        .position(
            |e| matches!(e,EventPayload::QueryCompleted {operation:o} if o.operation_id=="batch-2"),
        )
        .unwrap();
    let marker = events
        .iter()
        .position(
            |e| matches!(e,EventPayload::QueryStarted {operation:o} if o.operation_id=="marker"),
        )
        .unwrap();
    assert!(committed < marker);
    let Some(EventPayload::RunCompleted { snapshot }) = events.last() else {
        panic!("missing terminal completion")
    };
    validate_snapshot(snapshot).unwrap();
    let statements = ok.statements.lock().unwrap();
    let tail = statements
        .iter()
        .find(|p| p.operation_id == "batch-31")
        .unwrap();
    assert_eq!(tail.sql.parameters, [ROWS.to_string(), ROWS.to_string()]);
    let seed: Vec<_> = statements
        .iter()
        .filter(|p| p.operation_id == "seed")
        .collect();
    assert_eq!(seed.len() as u64, ROWS.div_ceil(1_000));
    assert_eq!(seed.last().unwrap().last_id.as_deref(), Some("30001"));
}

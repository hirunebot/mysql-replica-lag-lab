use std::{
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    body::{Body, BodyDataStream, to_bytes},
    http::Request,
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::sync::Notify;
use tower::ServiceExt;

use super::*;
use crate::{
    contracts::{
        ConnectionStatus, EventEnvelope, EventPayload, OperationStatus, RunPhase, validate_event,
        validate_snapshot,
    },
    events::Reporter,
    runner::{PreflightFailure, RunnerConfig},
};

fn config() -> Config {
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let replica = TcpListener::bind("127.0.0.1:0").unwrap();
    Config {
        source_database_url: format!("mysql://root:secret@{}/mysql", source.local_addr().unwrap()),
        replica_database_url: format!(
            "mysql://root:secret@{}/mysql",
            replica.local_addr().unwrap()
        ),
        demo_rows: 41,
        seed_batch_size: 10,
        heavy_batch_size: 20,
        marker_after_batches: 2,
        poll_interval: Duration::from_millis(10),
        initial_sync_timeout: Duration::from_secs(2),
        final_sync_timeout: Duration::from_secs(2),
    }
}

/// 初回snapshotからの境界と、切断後に同じsequenceを送り直さない復帰を検証する。
#[tokio::test]
async fn event_stream_replays_changes_after_the_snapshot_and_resume_cursor() {
    let proceed = Arc::new(Notify::new());
    let gate = proceed.clone();
    let app = router_with(
        RunManager::with_engine(
            config(),
            Arc::new(move |_, reporter| {
                let gate = gate.clone();
                Box::pin(async move {
                    gate.notified().await;
                    for phase in [RunPhase::WaitingInitialSync, RunPhase::Running] {
                        reporter.report(EventPayload::RunPhaseChanged { phase });
                    }
                    std::future::pending().await
                })
            }),
        )
        .unwrap(),
    );
    let (_, run) = post_run(&app, workload()).await;
    let id = run["run_id"].as_str().unwrap();
    let path = format!("/api/runs/{id}/events");
    let (status, mut initial) = event_stream(&app, &path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        matches!(read_event(&mut initial).await.event, EventPayload::Snapshot { snapshot } if snapshot.sequence == "0")
    );
    proceed.notify_one();
    let first = read_event(&mut initial).await;
    assert_eq!(first.sequence, "1");
    assert!(matches!(
        first.event,
        EventPayload::RunPhaseChanged {
            phase: RunPhase::WaitingInitialSync
        }
    ));
    drop(initial);
    let (status, mut replay) = event_stream(
        &app,
        &format!("{path}?after=wrong"),
        Some(&format!("{id}:1")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let second = read_event(&mut replay).await;
    assert_eq!(second.sequence, "2");
    assert!(matches!(
        second.event,
        EventPayload::RunPhaseChanged {
            phase: RunPhase::Running
        }
    ));

    let (_, mut reset) = event_stream(&app, &format!("{path}?after=9999"), None).await;
    assert!(
        matches!(read_event(&mut reset).await.event, EventPayload::Snapshot { snapshot } if snapshot.sequence == "2")
    );
    assert_eq!(
        event_stream(&app, &format!("{path}?after=01"), None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        event_stream(&app, &path, Some("wrong-run:1")).await.0,
        StatusCode::BAD_REQUEST
    );
    let restarted = router(config()).unwrap();
    assert_eq!(
        event_stream(&restarted, &path, None).await.0,
        StatusCode::NOT_FOUND
    );
}

async fn event_stream(
    app: &Router,
    path: &str,
    last_event: Option<&str>,
) -> (StatusCode, BodyDataStream) {
    let mut request = Request::builder().uri(path);
    if let Some(last) = last_event {
        request = request.header("last-event-id", last);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    if response.status() == StatusCode::OK {
        assert_eq!(response.headers()["content-type"], "text/event-stream");
    }
    (response.status(), response.into_body().into_data_stream())
}

async fn read_event(stream: &mut BodyDataStream) -> EventEnvelope {
    let bytes = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let bytes = stream.next().await.unwrap().unwrap();
            if !bytes.starts_with(b":") {
                break bytes;
            }
        }
    })
    .await
    .unwrap();
    let frame = std::str::from_utf8(&bytes).unwrap();
    let data = frame
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .unwrap();
    let event: EventEnvelope = serde_json::from_str(data).unwrap();
    assert!(
        frame
            .lines()
            .any(|line| line == format!("id: {}:{}", event.run_id, event.sequence))
    );
    assert!(
        frame
            .lines()
            .any(|line| line == format!("event: {}", event.event.name()))
    );
    validate_event(&event).unwrap();
    event
}

fn workload() -> Value {
    json!({"demo_rows":"41","seed_batch_size":"10","heavy_batch_size":"20","marker_after_batches":"2"})
}
async fn request(app: &Router, method: &str, uri: &str, body: Option<&str>) -> (StatusCode, Value) {
    let mut r = Request::builder().method(method).uri(uri);
    if body.is_some() {
        r = r.header("content-type", "application/json");
    }
    let response = app
        .clone()
        .oneshot(
            r.body(Body::from(body.unwrap_or_default().to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}
async fn post_run(app: &Router, body: Value) -> (StatusCode, Value) {
    request(app, "POST", "/api/runs", Some(&body.to_string())).await
}

/// 固定のpreset以外を拒否し、存在しないrunをDBへ問い合わせない。
#[tokio::test]
async fn manual_read_rejects_unknown_run_and_custom_sql() {
    let app = router(config()).unwrap();
    let uri = "/api/runs/unknown/reads";
    for invalid in [
        r#"{"preset":"other"}"#,
        r#"{"preset":"marker_sum","sql":"SELECT 1"}"#,
    ] {
        let (status, body) = request(&app, "POST", uri, Some(invalid)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "invalid_read");
    }
    let (status, body) = request(&app, "POST", uri, Some(r#"{"preset":"marker_sum"}"#)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "run_not_found");
}

/// 画面未ビルドでもAPIは起動し、未知APIにHTMLを返さない。
#[tokio::test]
async fn static_dashboard_serves_assets_without_taking_over_api_errors() {
    let dist = std::env::temp_dir().join(format!("lag-lab-static-{}", uuid::Uuid::new_v4()));
    let app = dashboard_router(config(), dist.clone()).unwrap();
    let get = |uri: &str| {
        app.clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
    };
    let missing = get("/").await.unwrap();
    assert_eq!(missing.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        to_bytes(missing.into_body(), 1024)
            .await
            .unwrap()
            .ends_with(b"in frontend/.")
    );
    let (status, body) = request(&app, "GET", "/api/unknown", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "run_not_found");

    std::fs::create_dir_all(dist.join("assets")).unwrap();
    std::fs::write(dist.join("index.html"), "<title>dashboard</title>").unwrap();
    std::fs::write(dist.join("assets/app.js"), "console.log('dashboard')").unwrap();
    let page = get("/").await.unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert!(
        page.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    let asset = get("/assets/app.js").await.unwrap();
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(asset.into_body(), 1024).await.unwrap(),
        "console.log('dashboard')"
    );
    assert_eq!(
        get("/assets/missing.js").await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    std::fs::remove_dir_all(dist).unwrap();
}
async fn until(app: &Router, id: &str, ready: impl Fn(&RunSnapshot) -> bool) -> RunSnapshot {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let (code, body) = request(app, "GET", &format!("/api/runs/{id}"), None).await;
            assert_eq!(code, StatusCode::OK);
            let snapshot = serde_json::from_value(body).unwrap();
            if ready(&snapshot) {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("run did not reach expected state")
}

/// POSTのレスポンスを読まずTCP接続を閉じても、登録済みrunnerは完了する。
#[tokio::test]
async fn disconnect_does_not_cancel_started_run() {
    use tokio::io::AsyncWriteExt;

    let started = Arc::new(Notify::new());
    let finish = Arc::new(Notify::new());
    let engine = Arc::new({
        let started = started.clone();
        let finish = finish.clone();
        move |config: RunnerConfig, reporter: Arc<dyn Reporter>| {
            let started = started.clone();
            let finish = finish.clone();
            Box::pin(async move {
                started.notify_one();
                finish.notified().await;
                failed(&config, reporter.as_ref(), false);
                Err(PreflightFailure::ReplicationNotReady.into())
            }) as _
        }
    });
    let app = router_with(RunManager::with_engine(config(), engine).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn({
        let app = app.clone();
        async move { axum::serve(listener, app).await.unwrap() }
    });
    let mut connection = tokio::net::TcpStream::connect(addr).await.unwrap();
    let body = workload().to_string();
    connection.write_all(format!("POST /api/runs HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), started.notified())
        .await
        .unwrap();
    drop(connection);
    let (_, state) = request(&app, "GET", "/api/state", None).await;
    let id = state["current_run"]["run_id"].as_str().unwrap();
    finish.notify_one();
    assert_eq!(
        until(&app, id, |s| s.phase == RunPhase::Failed)
            .await
            .error
            .unwrap()
            .code,
        "replication_not_ready"
    );
    server.abort();
}
fn failed(c: &RunnerConfig, reporter: &dyn Reporter, unknown: bool) {
    reporter.report(EventPayload::RunPhaseChanged {
        phase: RunPhase::Failing,
    });
    let mut snapshot: RunSnapshot =
        serde_json::from_str(include_str!("../../tests/fixtures/dashboard/snapshot.json")).unwrap();
    snapshot.run_id = c.run_id.clone();
    snapshot.server_instance_id = c.server_instance_id.clone();
    snapshot.config = c.workload.clone();
    snapshot.phase = RunPhase::Failed;
    if unknown {
        snapshot.operations[0].status = OperationStatus::OutcomeUnknown;
    }
    reporter.report(EventPayload::RunFailed {
        snapshot: Box::new(snapshot),
    });
}

/// DB停止中も200で接続不可を返し、開始後の失敗はrunに残す。
#[tokio::test]
async fn unavailable_databases_do_not_stop_http() {
    let app = router(config()).unwrap();
    let (code, state) = request(&app, "GET", "/api/state", None).await;
    assert_eq!(code, StatusCode::OK);
    assert!(!state.to_string().contains("secret"));
    let state: DashboardState = serde_json::from_value(state).unwrap();
    assert!(state.current_run.is_none());
    assert_eq!(state.connections.len(), 2);
    assert!(
        state
            .connections
            .iter()
            .all(|c| c.status == ConnectionStatus::Unavailable)
    );
    let (code, body) = post_run(&app, workload()).await;
    assert_eq!(code, StatusCode::ACCEPTED);
    let initial: RunSnapshot = serde_json::from_value(body).unwrap();
    assert_eq!(initial.phase, RunPhase::Preparing);
    validate_snapshot(&initial).unwrap();
    let failed = until(&app, &initial.run_id, |s| s.phase == RunPhase::Failed).await;
    assert_eq!(failed.error.unwrap().code, "db_unavailable");
    assert!(
        failed
            .operations
            .iter()
            .all(|o| o.status == OperationStatus::Planned)
    );
}

/// 不正な本文、接続先やSQLの混入、Web用上限を拒否して実行を起動しない。
#[tokio::test]
async fn invalid_requests_never_start_the_engine() {
    let started = Arc::new(AtomicUsize::new(0));
    let count = started.clone();
    let app = router_with(
        RunManager::with_engine(
            config(),
            Arc::new(move |_, _| {
                count.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { panic!("invalid config reached runner") })
            }),
        )
        .unwrap(),
    );
    for (key, value) in [
        ("source_database_url", json!("mysql://private")),
        ("sql", json!("DROP DATABASE lag_demo")),
        ("command", json!("echo unexpected")),
        ("demo_rows", json!("10000001")),
        ("demo_rows", json!("0")),
        ("demo_rows", json!(41)),
        ("marker_after_batches", json!("3")),
        ("demo_rows", json!("20001")),
        ("seed_batch_size", json!("18446744073709551616")),
    ] {
        let mut body = workload();
        body[key] = value;
        let (status, body) = post_run(&app, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{key}");
        assert_eq!(
            serde_json::from_value::<ApiError>(body).unwrap().code,
            "invalid_config"
        );
    }
    assert_eq!(
        request(&app, "POST", "/api/runs", Some("{")).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&app, "POST", "/api/runs", None).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(started.load(Ordering::SeqCst), 0);
    assert!(request(&app, "GET", "/api/state", None).await.1["current_run"].is_null());
}

/// 同時開始は一方だけ202。終端通知が来てもタスクが終わるまでは枠を解放しない。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_starts_reserve_until_background_task_returns() {
    let finish = Arc::new(Notify::new());
    let gate = finish.clone();
    let engine = Arc::new(move |c: RunnerConfig, reporter: Arc<dyn Reporter>| {
        let gate = gate.clone();
        Box::pin(async move {
            failed(&c, reporter.as_ref(), false);
            gate.notified().await;
            Err(PreflightFailure::ReplicationNotReady.into())
        }) as _
    });
    let app = router_with(RunManager::with_engine(config(), engine).unwrap());
    // 別タスクから同時に要求し、両レスポンスを受け取った後もrunnerは待機する。
    let a = tokio::spawn({
        let app = app.clone();
        async move { post_run(&app, workload()).await }
    });
    let b = tokio::spawn({
        let app = app.clone();
        async move { post_run(&app, workload()).await }
    });
    let mut responses = [a.await.unwrap(), b.await.unwrap()];
    responses.sort_by_key(|r| r.0);
    assert_eq!(responses[0].0, StatusCode::ACCEPTED);
    assert_eq!(responses[1].0, StatusCode::CONFLICT);
    assert_eq!(responses[1].1["code"], "run_in_progress");
    let id = responses[0].1["run_id"].as_str().unwrap().to_owned();
    drop(responses);
    let draining = until(&app, &id, |s| s.phase == RunPhase::Failing).await;
    assert_eq!(post_run(&app, workload()).await.0, StatusCode::CONFLICT);
    finish.notify_one();
    let terminal = until(&app, &id, |s| s.phase == RunPhase::Failed).await;
    assert!(terminal.sequence.parse::<u64>().unwrap() > draining.sequence.parse::<u64>().unwrap());
    assert_eq!(terminal.error.unwrap().code, "replication_not_ready");
    let (status, next) = post_run(&app, workload()).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_ne!(next["run_id"], id);
    let (status, body) = request(&app, "GET", &format!("/api/runs/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "run_not_found");
    finish.notify_one();
}

/// 結果不明のSQLやタスクpanicでは、次のデモを開始できない。
#[tokio::test]
async fn unconfirmed_completion_keeps_reservation() {
    for panic in [false, true] {
        let engine = Arc::new(move |c: RunnerConfig, reporter: Arc<dyn Reporter>| {
            Box::pin(async move {
                assert!(!panic, "simulated worker panic");
                failed(&c, reporter.as_ref(), true);
                Err(anyhow::anyhow!("secret driver details"))
            }) as _
        });
        let app = router_with(RunManager::with_engine(config(), engine).unwrap());
        let (code, body) = post_run(&app, workload()).await;
        assert_eq!(code, StatusCode::ACCEPTED);
        let held = until(&app, body["run_id"].as_str().unwrap(), |s| {
            s.error
                .as_ref()
                .is_some_and(|e| e.code == "completion_unconfirmed")
        })
        .await;
        assert_eq!(held.phase, RunPhase::Failing);
        assert!(!serde_json::to_string(&held).unwrap().contains("secret"));
        assert_eq!(post_run(&app, workload()).await.0, StatusCode::CONFLICT);
    }
}

/// 認証情報だけ異なる同一接続先は、Source/Replicaとして許可しない。
#[test]
fn duplicate_database_endpoint_is_rejected() {
    let mut config = config();
    config.replica_database_url = config
        .source_database_url
        .replace("root:secret", "other:password");
    assert!(router(config).is_err());
}

use std::sync::Mutex;

use super::*;
use crate::contracts::{EventPayload, ObservationStatus, ObservationValue, validate_snapshot};

#[derive(Default)]
struct Timings {
    heavy_started: Option<Instant>,
    heavy_completed: Option<Instant>,
    marker_committed: Option<Instant>,
    marker_visible: Option<Instant>,
    final_started: Option<Instant>,
    final_completed: Option<Instant>,
    count_samples_ms: Vec<f64>,
    failed_counts: usize,
}

#[derive(Default)]
struct Measurements(Mutex<Timings>);

impl Reporter for Measurements {
    fn report(&self, event: EventPayload) {
        let now = Instant::now();
        let mut t = self.0.lock().unwrap();
        match event {
            EventPayload::QueryStarted { operation }
                if operation.operation_id.starts_with("batch-") =>
            {
                t.heavy_started.get_or_insert(now);
            }
            EventPayload::QueryCompleted { operation }
                if operation.operation_id.starts_with("batch-") =>
            {
                t.heavy_completed = Some(now);
            }
            EventPayload::QueryCompleted { operation } if operation.operation_id == "marker" => {
                t.marker_committed = Some(now);
            }
            EventPayload::DbObserved { observation }
                if observation.target == DatabaseTarget::Replica
                    && observation.metric == ObservationMetric::Marker
                    && observation.status == ObservationStatus::Available
                    && t.marker_committed.is_some()
                    && matches!(&observation.value, Some(ObservationValue::Decimal(value)) if value == "1") =>
            {
                t.marker_visible.get_or_insert(now);
            }
            EventPayload::DbObserved { observation }
                if matches!(
                    observation.metric,
                    ObservationMetric::TotalRows | ObservationMetric::UpdatedRows
                ) =>
            {
                if observation.status == ObservationStatus::Available {
                    t.count_samples_ms.push(observation.elapsed_ms.unwrap());
                } else {
                    t.failed_counts += 1;
                }
            }
            EventPayload::RunPhaseChanged {
                phase: RunPhase::WaitingFinalSync,
            } => t.final_started = Some(now),
            EventPayload::RunCompleted { snapshot } => {
                validate_snapshot(&snapshot).unwrap();
                t.final_completed = Some(now);
            }
            _ => {}
        }
    }
}

fn config(mode: &str, repetition: usize) -> RunnerConfig {
    RunnerConfig {
        server_instance_id: "observer-benchmark".into(),
        run_id: format!("{mode}-{repetition}"),
        source_database_url: "mysql://root:root@127.0.0.1:3307/mysql".into(),
        replica_database_url: "mysql://root:root@127.0.0.1:3308/mysql".into(),
        workload: RunConfig {
            demo_rows: "40000".into(),
            seed_batch_size: "1000".into(),
            heavy_batch_size: "5000".into(),
            marker_after_batches: "2".into(),
        },
        poll_interval: Duration::from_millis(500),
        initial_sync_timeout: Duration::from_secs(600),
        final_sync_timeout: Duration::from_secs(600),
    }
}

/// 同じローカルDBと負荷設定でCLIとWeb観測を交互に3回ずつ測る。
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires local Docker MySQL and recreates lag_demo six times"]
async fn compare_cli_and_dashboard_observation() {
    println!(
        "mode,repetition,heavy_s,marker_s,final_sync_s,count_n,count_median_ms,count_max_ms,count_failed,stale"
    );
    for repetition in 1..=3 {
        for web in if repetition == 2 {
            [true, false]
        } else {
            [false, true]
        } {
            let mode = if web { "web" } else { "cli" };
            let recorder = Arc::new(Measurements::default());
            let settings = config(mode, repetition);
            let outcome = if web {
                run_dashboard(settings, recorder.clone()).await
            } else {
                run(settings, recorder.clone()).await
            }
            .unwrap();
            assert_eq!(outcome.affected_rows, 40_000);
            let mut times = recorder.0.lock().unwrap();
            let elapsed = |metric, start: Option<Instant>, end: Option<Instant>| {
                let (Some(start), Some(end)) = (start, end) else {
                    panic!("{mode} run {repetition} did not emit both {metric} timings");
                };
                end.duration_since(start).as_secs_f64()
            };
            times.count_samples_ms.sort_unstable_by(f64::total_cmp);
            let n = times.count_samples_ms.len();
            assert!(
                n > 0,
                "{mode} run {repetition} recorded no successful COUNTs"
            );
            let median =
                (times.count_samples_ms[(n - 1) / 2] + times.count_samples_ms[n / 2]) / 2.0;
            println!(
                "{mode},{repetition},{:.3},{:.3},{:.3},{n},{median:.3},{:.3},{},{}",
                elapsed("UPDATE", times.heavy_started, times.heavy_completed),
                elapsed("marker", times.marker_committed, times.marker_visible),
                elapsed("final sync", times.final_started, times.final_completed),
                times.count_samples_ms[n - 1],
                times.failed_counts,
                outcome.stale_read_observed,
            );
        }
    }
}

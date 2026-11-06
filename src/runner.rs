//! CLIと将来のHTTPサーバーが共有する実行手順。
use std::{
    future::Future,
    sync::{Arc, atomic::AtomicU64},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use sqlx::{MySqlExecutor, MySqlPool};
use tokio::sync::{oneshot, watch};

use crate::{
    config::Config,
    contracts::{
        DatabaseTarget, ObservationMetric, OperationKind, OperationPlan, RunConfig, RunPhase,
        SqlStatement,
    },
    database,
    events::Reporter,
    monitor,
    observer::Observer,
    queries, setup, workload,
};
mod reporting;
pub(crate) use reporting::Journal;

/// workloadだけが公開の負荷設定。接続情報と待ち時間はサーバー/CLI側で保持する。
/// 認証情報の意図しない出力を避けるためDebug/Serializeは実装しない。
pub struct RunnerConfig {
    pub server_instance_id: String,
    pub run_id: String,
    pub source_database_url: String,
    pub replica_database_url: String,
    pub workload: RunConfig,
    pub poll_interval: Duration,
    pub initial_sync_timeout: Duration,
    pub final_sync_timeout: Duration,
}

/// DBの同期完了と、stale readを観測できたかは別の結果。
#[derive(Debug)]
pub struct RunOutcome {
    pub batches: u64,
    pub affected_rows: u64,
    pub stale_read_observed: bool,
    pub marker_visibility: Duration,
}

/// 0・範囲外・算術オーバーフローはNone。端数の最終バッチも含む。
pub fn batch_range(batch: u64, rows: u64, batch_size: u64) -> Option<(u64, u64)> {
    if batch_size == 0 {
        return None;
    }
    let first = batch
        .checked_sub(1)?
        .checked_mul(batch_size)?
        .checked_add(1)?;
    if first > rows {
        return None;
    }
    Some((first, first.saturating_add(batch_size - 1).min(rows)))
}

/// 設定を検証して予定を生成する。DB接続・スキーマ変更・通知は行わない。
/// CLI互換のため、markerが最終バッチの後になる既存設定も許可する。
pub fn build_plan(config: &RunConfig) -> Result<Vec<OperationPlan>> {
    let [rows, seed, heavy, marker] = parse_workload(config)?;
    plan(rows, seed, heavy, marker)
}

pub(crate) fn parse_workload(c: &RunConfig) -> Result<[u64; 4]> {
    fn number(value: &str, name: &str) -> Result<u64> {
        let n: u64 = value.parse().with_context(|| format!("invalid {name}"))?;
        ensure!(n.to_string() == value, "invalid {name}");
        Ok(n)
    }
    let rows = number(&c.demo_rows, "demo_rows")?;
    let seed = number(&c.seed_batch_size, "seed_batch_size")?;
    let heavy = number(&c.heavy_batch_size, "heavy_batch_size")?;
    let marker = number(&c.marker_after_batches, "marker_after_batches")?;
    Config::validate_workload(rows, seed, heavy, marker)?;
    Ok([rows, seed, heavy, marker])
}

#[derive(Debug)]
pub(crate) enum PreflightFailure {
    DatabaseUnavailable,
    ReplicationNotReady,
}

impl std::fmt::Display for PreflightFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::DatabaseUnavailable => "failed to reach the demo databases",
            Self::ReplicationNotReady => "replication is not ready",
        })
    }
}
impl std::error::Error for PreflightFailure {}

fn plan(rows: u64, seed: u64, heavy: u64, marker: u64) -> Result<Vec<OperationPlan>> {
    use DatabaseTarget::{Replica, Source};
    use OperationKind::*;
    let total = rows.div_ceil(heavy);
    let mut plan = Vec::new();
    plan.try_reserve_exact(
        usize::try_from(total)?
            .checked_add(4)
            .context("too many batches")?,
    )?;
    let operation = |id: String, kind, target, sql: SqlStatement, after_committed| OperationPlan {
        operation_id: id,
        kind,
        target,
        sql,
        after_committed,
        batch_number: None,
        first_id: None,
        last_id: None,
        planned_rows: None,
    };
    plan.push(operation(
        "setup".into(),
        Setup,
        Source,
        queries::statement(queries::SETUP[0]),
        vec![],
    ));
    let mut seed_plan = operation(
        "seed".into(),
        Seed,
        Source,
        queries::seed_insert(1, seed.min(rows)),
        vec!["setup".into()],
    );
    seed_plan.planned_rows = Some(rows.to_string());
    plan.push(seed_plan);
    for n in 1..=total {
        let (first, last) = batch_range(n, rows, heavy).context("invalid heavy batch range")?;
        let after = if n == 1 {
            "seed".into()
        } else {
            format!("batch-{}", n - 1)
        };
        let mut p = operation(
            format!("batch-{n}"),
            HeavyUpdate,
            Source,
            queries::heavy_update(first, last),
            vec![after],
        );
        p.batch_number = Some(n.to_string());
        p.first_id = Some(first.to_string());
        p.last_id = Some(last.to_string());
        p.planned_rows = Some((last - first + 1).to_string());
        plan.push(p);
        if n == marker {
            let mut p = operation(
                "marker".into(),
                MarkerUpdate,
                Source,
                queries::marker_update(),
                vec![format!("batch-{n}")],
            );
            p.first_id = Some("1".into());
            p.last_id = Some("1".into());
            p.planned_rows = Some("1".into());
            plan.push(p);
        }
    }
    plan.push(operation(
        "final-sync".into(),
        FinalSync,
        Replica,
        queries::statement(queries::UPDATED_COUNT),
        vec![format!("batch-{total}"), "marker".into()],
    ));
    Ok(plan)
}

/// 実行開始から最終同期までを所有する。事前検証失敗ならSQLを発行しない。
/// エラー時は新しいバッチ投入を止め、実行中のSQL/子タスクの終了確認後に返る。
/// 接続情報を含む生エラーは公開イベントへ渡さず、CLI側で詳細を扱う。
pub async fn run(config: RunnerConfig, reporter: Arc<dyn Reporter>) -> Result<RunOutcome> {
    run_with_observations(config, reporter, false).await
}

pub(crate) async fn run_dashboard(
    config: RunnerConfig,
    reporter: Arc<dyn Reporter>,
) -> Result<RunOutcome> {
    run_with_observations(config, reporter, true).await
}

async fn run_with_observations(
    config: RunnerConfig,
    reporter: Arc<dyn Reporter>,
    dashboard: bool,
) -> Result<RunOutcome> {
    ensure!(
        !config.run_id.trim().is_empty() && !config.server_instance_id.trim().is_empty(),
        "run identifiers must not be empty"
    );
    let [rows, seed, heavy, marker] = parse_workload(&config.workload)?;
    let settings = Config {
        source_database_url: config.source_database_url.clone(),
        replica_database_url: config.replica_database_url.clone(),
        demo_rows: rows,
        seed_batch_size: seed,
        heavy_batch_size: heavy,
        marker_after_batches: marker,
        poll_interval: config.poll_interval,
        initial_sync_timeout: config.initial_sync_timeout,
        final_sync_timeout: config.final_sync_timeout,
    };
    settings.validate()?;
    let journal = Journal::new(
        &config,
        plan(rows, seed, heavy, marker)?,
        reporter,
        dashboard,
    );
    journal.phase(RunPhase::Preparing);
    let result = execute(&settings, &journal, dashboard).await;
    if result.is_err() {
        journal.phase(RunPhase::Failing);
    }
    journal.finish(result.is_ok());
    result
}

async fn execute(c: &Config, j: &Journal, dashboard: bool) -> Result<RunOutcome> {
    let (source, replica) = tokio::try_join!(
        database::connect(
            &c.source_database_url,
            "Source",
            if dashboard { 2 } else { 10 }
        ),
        database::connect(
            &c.replica_database_url,
            "Replica",
            if dashboard { 1 } else { 10 }
        )
    )
    .context(PreflightFailure::DatabaseUnavailable)?;
    database::ensure_replication_running(&replica)
        .await
        .context(PreflightFailure::ReplicationNotReady)?;
    j.operation("setup", setup::recreate_schema(&source, j))
        .await?;
    if dashboard {
        let gtids = database::bounded_read(&source, async |connection| {
            database::executed_gtids(connection).await
        })
        .await?;
        let observer = Observer::new(c, gtids)?;
        // Webのpreflight専用接続を観測開始前に返す。
        replica.close().await;
        observer
            .during(
                execute_work(c, j, &source, &replica, Some(&observer)),
                j,
                c.poll_interval,
            )
            .await
    } else {
        execute_work(c, j, &source, &replica, None).await
    }
}

async fn execute_work(
    c: &Config,
    j: &Journal,
    source: &MySqlPool,
    replica: &MySqlPool,
    observer: Option<&Observer>,
) -> Result<RunOutcome> {
    use DatabaseTarget::{Replica, Source};
    use ObservationMetric::{Marker, TotalRows, UpdatedRows};
    j.operation("seed", setup::seed(source, c, j)).await?;
    if observer.is_none() {
        let rows = j
            .decimal(Source, TotalRows, database::item_count(source))
            .await?;
        ensure!(
            rows == c.demo_rows,
            "Source seed count mismatch: expected {}, got {rows}",
            c.demo_rows
        );
    }
    j.phase(RunPhase::WaitingInitialSync);
    if let Some(observer) = observer {
        observer.wait_initial(j, c).await?;
    } else {
        setup::wait_for_initial_sync(replica, c, j).await?;
    }
    j.phase(RunPhase::Running);
    j.line(format_args!(
        "\nInitial state is synchronized. Starting the lag scenario."
    ));
    let (ready, ready_rx) = oneshot::channel();
    let (stop, _) = watch::channel(false);
    let progress = AtomicU64::new(0);
    let (heavy, (expected, observation)) = supervise(
        &stop,
        || j.phase(RunPhase::Failing),
        workload::run_heavy_workload(source, c, j, ready, &progress, &stop),
        async {
            ready_rx
                .await
                .context("heavy workload stopped before the marker insertion point")?;
            ensure!(!*stop.borrow(), "heavy workload stopped");
            j.line(format_args!(
                "\n{} heavy batches committed. Updating the independent marker now...",
                c.marker_after_batches
            ));
            let started = Instant::now();
            let affected = j
                .operation("marker", workload::update_marker(source, j))
                .await?;
            let mut stopped = stop.subscribe();
            if let Some(observer) = observer {
                ensure!(affected == 1, "marker UPDATE did not affect one row");
                let observation = tokio::select! {
                    biased;
                    _ = stopped.wait_for(|stop| *stop) => anyhow::bail!("heavy workload stopped"),
                    result = observer.wait_marker(j, c.final_sync_timeout, started) => result?,
                };
                return Ok((1, observation));
            }
            let marker = j
                .decimal(Source, Marker, database::marker_value(source))
                .await?;
            j.line(format_args!(
                "Source marker committed as value {marker} in {:.3}s.",
                started.elapsed().as_secs_f64()
            ));
            let observation = tokio::select! {
                biased;
                _ = stopped.wait_for(|stop| *stop) => anyhow::bail!("heavy workload stopped"),
                result = monitor::wait_for_marker(source,replica,marker,&progress,c,j) => result?,
            };
            Ok((marker, observation))
        },
    )
    .await?;
    if observation.stale_read_observed {
        j.line(format_args!(
            "\nSUCCESS: Replica returned the old marker after Source committed the new value."
        ));
        j.line(format_args!(
            "The lightweight marker took {:.2}s to become visible on Replica.",
            observation.elapsed.as_secs_f64()
        ));
    } else {
        j.line(format_args!(
            "\nWARNING: Replica had already applied the marker at the first observation."
        ));
        j.line(format_args!(
            "Increase DEMO_ROWS or MARKER_AFTER_BATCHES, or lower REPLICA_CPUS, then retry."
        ));
    }
    j.line(format_args!(
        "Heavy workload: {} batches, {} affected rows, {:.2}s on Source.",
        heavy.batches, heavy.affected_rows, heavy.elapsed_seconds
    ));
    j.phase(RunPhase::WaitingFinalSync);
    j.operation("final-sync", async {
        if let Some(observer) = observer {
            observer.wait_final(j, c).await?;
            return Ok(c.demo_rows);
        }
        setup::wait_for_final_sync(replica, c, j).await?;
        let (source_total, replica_total, source_updated, source_marker, replica_marker, status) =
            tokio::try_join!(
                j.decimal(Source, TotalRows, database::item_count(source)),
                j.decimal(Replica, TotalRows, database::item_count(replica)),
                j.decimal(Source, UpdatedRows, database::updated_item_count(source)),
                j.decimal(Source, Marker, database::marker_value(source)),
                j.decimal(Replica, Marker, database::marker_value(replica)),
                j.replication(replica_status(replica)),
            )?;
        ensure!(
            [source_total, replica_total, source_updated]
                .into_iter()
                .all(|n| n == c.demo_rows),
            "final row count mismatch"
        );
        ensure!(
            source_marker == expected && replica_marker == expected && expected == 1,
            "final marker mismatch: Source={source_marker}, Replica={replica_marker}"
        );
        ensure!(
            status.io_running == "Yes" && status.sql_running == "Yes",
            "replication is not running at final sync"
        );
        Ok(c.demo_rows)
    })
    .await?;
    j.line(format_args!(
        "Demo completed with Source and Replica in sync."
    ));
    Ok(RunOutcome {
        batches: heavy.batches,
        affected_rows: heavy.affected_rows,
        stale_read_observed: observation.stale_read_observed,
        marker_visibility: observation.elapsed,
    })
}

pub(crate) async fn replica_status<'a>(
    replica: impl MySqlExecutor<'a>,
) -> Result<database::ReplicaStatus> {
    database::replica_status(replica)
        .await?
        .context("replication status disappeared while monitoring")
}

// 片方が失敗しても、もう片方の書き込みをdrop/abortしない。
// stopは次の書き込みを止めるためだけに使い、発行済みSQLの応答を待つ。
async fn supervise<A, B>(
    stop: &watch::Sender<bool>,
    on_failure: impl FnOnce(),
    a: impl Future<Output = Result<A>>,
    b: impl Future<Output = Result<B>>,
) -> Result<(A, B)> {
    tokio::pin!(a, b);
    tokio::select! {
        result = &mut a => match result {
            Ok(a) => Ok((a,b.await?)),
            Err(e) => { stop.send_replace(true); on_failure(); let _ = b.await; Err(e) }
        },
        result = &mut b => match result {
            Ok(b) => Ok((a.await?,b)),
            Err(e) => { stop.send_replace(true); on_failure(); let _ = a.await; Err(e) }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[tokio::test]
    async fn marker_failure_waits_for_active_write_without_starting_another() {
        let (stop, mut cancelled) = watch::channel(false);
        let (started, active) = oneshot::channel();
        let finished_write = Cell::new(false);
        let cleanup_started = Cell::new(false);
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            supervise(
                &stop,
                || {
                    assert!(!finished_write.get());
                    cleanup_started.set(true);
                },
                async {
                    started.send(()).unwrap();
                    cancelled.wait_for(|stop| *stop).await.unwrap();
                    assert!(cleanup_started.get());
                    finished_write.set(true);
                    ensure!(!*stop.borrow(), "next batch cancelled");
                    panic!("unexpected next batch");
                    #[allow(unreachable_code)]
                    Ok(())
                },
                async {
                    active.await.unwrap();
                    Err::<(), _>(anyhow::anyhow!("marker failed"))
                },
            ),
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap_err().to_string(), "marker failed");
        assert!(finished_write.get());
    }
}

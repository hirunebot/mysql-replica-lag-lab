use std::{sync::Arc, time::Instant};

use super::{RunManager, error};
use crate::{contracts::*, database, queries::MARKER_SUM};
use chrono::{SecondsFormat, Utc};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadError {
    Missing,
    Stale,
    NotReady,
    Busy,
    DatabaseUnavailable,
    Timeout,
}

impl ReadError {
    pub(crate) fn api_error(self) -> ApiError {
        match self {
            Self::Missing => error("run_not_found", "No matching run was found.", false),
            Self::Stale => error(
                "stale_run",
                "A newer run replaced the requested run.",
                false,
            ),
            Self::NotReady => error(
                "not_ready",
                "The Replica marker table is not ready for this run.",
                true,
            ),
            Self::Busy => error(
                "read_in_progress",
                "A manual read is already in progress.",
                true,
            ),
            Self::DatabaseUnavailable => {
                error("db_unavailable", "Replica could not be read.", true)
            }
            Self::Timeout => error("read_timeout", "The manual read timed out.", true),
        }
    }

    fn from_query(cause: anyhow::Error) -> Self {
        if cause.is::<tokio::time::error::Elapsed>() {
            return Self::Timeout;
        }
        match cause.downcast_ref::<sqlx::Error>() {
            Some(sqlx::Error::Database(error)) => {
                match error
                    .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
                    .map(sqlx::mysql::MySqlDatabaseError::number)
                {
                    Some(1049 | 1146) => Self::NotReady,
                    Some(1205 | 3024) => Self::Timeout,
                    _ => Self::DatabaseUnavailable,
                }
            }
            _ => Self::DatabaseUnavailable,
        }
    }
}

impl RunManager {
    pub(crate) async fn read_marker_sum(
        self: &Arc<Self>,
        id: &str,
    ) -> Result<ManualRead, ReadError> {
        let started = Instant::now();
        let read = self.reserve_read(id)?;
        let manager = self.clone();
        let pool = self.manual_pool.clone();
        // HTTPが切断されても照会と履歴更新を最後まで行い、新規runとの排他を解放する。
        tokio::spawn(async move {
            let result = tokio::spawn(async move {
                database::bounded_read(&pool, database::marker_sum)
                    .await
                    .map_err(ReadError::from_query)
            })
            .await
            .unwrap_or(Err(ReadError::DatabaseUnavailable));
            manager.complete_read(read, started, result)
        })
        .await
        .map_err(|_| ReadError::DatabaseUnavailable)?
    }

    pub(super) fn reserve_read(&self, id: &str) -> Result<ManualRead, ReadError> {
        let mut current = self.current.lock().unwrap();
        let run = current.as_mut().ok_or(ReadError::Missing)?;
        if run.snapshot.run_id != id {
            let issued = id.split_once('.').is_some_and(|(instance, generation)| {
                instance == self.instance
                    && generation
                        .parse::<u64>()
                        .is_ok_and(|n| n > 0 && n < run.generation && n.to_string() == generation)
            });
            return Err(if issued {
                ReadError::Stale
            } else {
                ReadError::Missing
            });
        }
        match run.snapshot.phase {
            RunPhase::Running | RunPhase::WaitingFinalSync | RunPhase::Completed => {}
            RunPhase::Failed if !run.reserved && run.schema_observed => {}
            _ => return Err(ReadError::NotReady),
        }
        if run.read_pending() {
            return Err(ReadError::Busy);
        }
        let read = ManualRead {
            read_id: Uuid::new_v4().to_string(),
            run_id: id.to_owned(),
            target: DatabaseTarget::Replica,
            preset: ReadPreset::MarkerSum,
            sql: MARKER_SUM.into(),
            status: ManualReadStatus::Pending,
            value: None,
            started_at: now(),
            completed_at: None,
            elapsed_ms: None,
            error: None,
        };
        run.snapshot.manual_reads.insert(0, read.clone());
        run.snapshot.manual_reads.truncate(5);
        run.publish(EventPayload::ManualReadStarted { read: read.clone() });
        Ok(read)
    }

    pub(super) fn complete_read(
        &self,
        mut read: ManualRead,
        started: Instant,
        result: Result<Option<String>, ReadError>,
    ) -> Result<ManualRead, ReadError> {
        read.completed_at = Some(now());
        read.elapsed_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
        match &result {
            Ok(value) => {
                read.status = ManualReadStatus::Succeeded;
                read.value.clone_from(value);
            }
            Err(error) => {
                read.status = ManualReadStatus::Failed;
                read.error = Some(error.api_error());
            }
        }
        let mut current = self.current.lock().unwrap();
        let run = current
            .as_mut()
            .filter(|run| run.snapshot.run_id == read.run_id)
            .ok_or(ReadError::Stale)?;
        let pending = run
            .snapshot
            .manual_reads
            .first_mut()
            .filter(|pending| {
                pending.read_id == read.read_id && pending.status == ManualReadStatus::Pending
            })
            .ok_or(ReadError::Stale)?;
        *pending = read.clone();
        run.publish(if result.is_ok() {
            EventPayload::ManualReadCompleted { read: read.clone() }
        } else {
            EventPayload::ManualReadFailed { read: read.clone() }
        });
        result.map(|_| read)
    }
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    /// 準備中の照会を拒否し、pendingを終端更新で失わず、新規runと排他にする。
    #[tokio::test]
    async fn read_reservation_survives_runner_completion_and_blocks_next_run() {
        let manager = manager();
        let config = manager.default_config.clone();
        let run = manager.start_run(config.clone()).ok().unwrap();
        assert_eq!(manager.reserve_read(&run.run_id), Err(ReadError::NotReady));
        {
            let mut current = manager.current.lock().unwrap();
            let current = current.as_mut().unwrap();
            current.snapshot.phase = RunPhase::Running;
            current.schema_observed = true;
        }
        let read = manager.reserve_read(&run.run_id).unwrap();
        assert_eq!(manager.reserve_read(&run.run_id), Err(ReadError::Busy));
        let mut terminal = run.clone();
        terminal.phase = RunPhase::Failed;
        manager.current.lock().unwrap().as_mut().unwrap().terminal = Some(terminal);
        manager.finish(&run.run_id, Ok(Err(anyhow::anyhow!("test failure"))));
        assert_eq!(
            manager.run(&run.run_id).unwrap().manual_reads,
            std::slice::from_ref(&read)
        );
        assert!(matches!(
            manager.start_run(config.clone()),
            Err(super::super::StartError::Busy)
        ));
        manager
            .complete_read(read, Instant::now(), Ok(Some("0".into())))
            .unwrap();
        let next = manager.start_run(config).ok().unwrap();
        assert_eq!(manager.reserve_read(&run.run_id), Err(ReadError::Stale));
        assert!(next.manual_reads.is_empty());
    }

    /// NULL・成功値・失敗を区別し、古い結果を更新せず最新5件を保存する。
    #[tokio::test]
    async fn manual_reads_keep_the_last_five_values_and_errors() {
        let manager = manager();
        let run = manager
            .start_run(manager.default_config.clone())
            .ok()
            .unwrap();
        manager
            .current
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .snapshot
            .phase = RunPhase::Completed;
        let inputs = [
            Ok(Some("0".into())),
            Ok(None),
            Ok(Some("1".into())),
            Err(ReadError::Timeout),
            Err(ReadError::DatabaseUnavailable),
            Ok(Some("18446744073709551616".into())),
        ];
        for value in inputs {
            let read = manager.reserve_read(&run.run_id).unwrap();
            let expected = value.clone();
            let result = manager.complete_read(read, Instant::now(), value);
            assert_eq!(result.map(|read| read.value), expected);
        }
        let state = manager.run(&run.run_id).unwrap();
        assert_eq!(state.manual_reads.len(), 5);
        assert_eq!(
            state.manual_reads[0].value.as_deref(),
            Some("18446744073709551616")
        );
        assert_eq!(
            state.manual_reads[1].error.as_ref().unwrap().code,
            "db_unavailable"
        );
        assert_eq!(
            state.manual_reads[2].error.as_ref().unwrap().code,
            "read_timeout"
        );
        assert_eq!(state.manual_reads[3].value.as_deref(), Some("1"));
        assert_eq!(state.manual_reads[4].status, ManualReadStatus::Succeeded);
        assert!(state.manual_reads[4].value.is_none());
    }

    fn manager() -> Arc<RunManager> {
        RunManager::with_engine(
            Config {
                source_database_url: "mysql://root@127.0.0.1:3307/mysql".into(),
                replica_database_url: "mysql://root@127.0.0.1:3308/mysql".into(),
                demo_rows: 30,
                seed_batch_size: 5,
                heavy_batch_size: 10,
                marker_after_batches: 1,
                poll_interval: std::time::Duration::from_millis(500),
                initial_sync_timeout: std::time::Duration::from_secs(10),
                final_sync_timeout: std::time::Duration::from_secs(10),
            },
            Arc::new(|_, _| Box::pin(std::future::pending())),
        )
        .unwrap()
    }
}

use std::{collections::VecDeque, sync::Arc};

use chrono::{SecondsFormat, Utc};
use tokio::sync::watch;

use super::{CurrentRun, RunManager};
use crate::contracts::{EventEnvelope, EventPayload, RunSnapshot};

const MAX_EVENTS: usize = 2_000;
const MAX_BYTES: usize = 4 * 1024 * 1024;

pub(crate) struct Frame {
    pub(crate) id: String,
    pub(crate) name: &'static str,
    pub(crate) json: String,
    sequence: u64,
}

impl Frame {
    fn new(run_id: &str, sequence: u64, event: EventPayload) -> Self {
        let name = event.name();
        let json = serde_json::to_string(&EventEnvelope {
            schema_version: 1,
            run_id: run_id.into(),
            sequence: sequence.to_string(),
            occurred_at: Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true),
            event,
        })
        .expect("dashboard events are JSON serializable");
        Self {
            id: format!("{run_id}:{sequence}"),
            name,
            json,
            sequence,
        }
    }

    fn bytes(&self) -> usize {
        self.id.len() + self.name.len() + self.json.len()
    }
}

#[derive(Default)]
pub(super) struct History {
    events: VecDeque<Arc<Frame>>,
    bytes: usize,
    changed: watch::Sender<u64>,
}

impl History {
    fn append(&mut self, event: Frame) {
        let sequence = event.sequence;
        self.bytes += event.bytes();
        self.events.push_back(Arc::new(event));
        while self.events.len() > MAX_EVENTS || self.bytes > MAX_BYTES {
            self.bytes -= self.events.pop_front().expect("nonempty history").bytes();
        }
        self.changed.send_replace(sequence);
    }

    fn next_after(&self, after: u64) -> Option<Arc<Frame>> {
        let start = self.events.front()?.sequence;
        let index = usize::try_from(after.checked_add(1)?.checked_sub(start)?).ok()?;
        self.events.get(index).cloned()
    }
}

impl CurrentRun {
    pub(super) fn record(&mut self, event: EventPayload) {
        self.history
            .append(Frame::new(&self.snapshot.run_id, self.sequence, event));
    }
}

pub(crate) struct Subscription {
    manager: Arc<RunManager>,
    run_id: String,
    after: Option<u64>,
    changed: watch::Receiver<u64>,
}

enum Next {
    Replay(Arc<Frame>),
    Snapshot(Box<RunSnapshot>, u64),
    Wait,
}

impl RunManager {
    pub(crate) fn subscribe(
        self: &Arc<Self>,
        run_id: String,
        after: Option<u64>,
    ) -> Option<Subscription> {
        let current = self.current.lock().unwrap();
        let run = current
            .as_ref()
            .filter(|run| run.snapshot.run_id == run_id)?;
        Some(Subscription {
            manager: self.clone(),
            changed: run.history.changed.subscribe(),
            run_id,
            after,
        })
    }
}

impl Subscription {
    fn next_frame(&self) -> Option<Next> {
        let current = self.manager.current.lock().unwrap();
        let run = current
            .as_ref()
            .filter(|run| run.snapshot.run_id == self.run_id)?;
        if let Some(after) = self.after {
            if after == run.sequence {
                return Some(Next::Wait);
            }
            if let Some(frame) = run.history.next_after(after) {
                return Some(Next::Replay(frame));
            }
        }
        Some(Next::Snapshot(Box::new(run.snapshot.clone()), run.sequence))
    }

    pub(crate) async fn next(&mut self) -> Option<Arc<Frame>> {
        loop {
            let frame = match self.next_frame()? {
                Next::Replay(frame) => frame,
                Next::Snapshot(snapshot, sequence) => Arc::new(Frame::new(
                    &self.run_id,
                    sequence,
                    EventPayload::Snapshot { snapshot },
                )),
                Next::Wait => {
                    self.changed.changed().await.ok()?;
                    continue;
                }
            };
            self.after = Some(frame.sequence);
            return Some(frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// イベント件数とバイト数のどちらを超えても古い通知を破棄する。
    #[test]
    fn history_is_bounded_and_exposes_only_contiguous_replay() {
        let event = |sequence, json: String| Frame {
            id: format!("run:{sequence}"),
            name: "db_observed",
            json,
            sequence,
        };
        let mut history = History::default();
        for sequence in 1..=MAX_EVENTS as u64 + 2 {
            history.append(event(sequence, "{}".into()));
        }
        assert_eq!(history.events.len(), MAX_EVENTS);
        assert!(history.next_after(1).is_none());
        assert_eq!(history.next_after(2).unwrap().sequence, 3);

        let first = MAX_EVENTS as u64 + 3;
        history.append(event(first, "x".repeat(MAX_BYTES / 2)));
        history.append(event(first + 1, "x".repeat(MAX_BYTES / 2)));
        assert!(history.bytes <= MAX_BYTES);
        assert!(history.next_after(first - 1).is_none());
        assert_eq!(history.next_after(first).unwrap().sequence, first + 1);
    }
}

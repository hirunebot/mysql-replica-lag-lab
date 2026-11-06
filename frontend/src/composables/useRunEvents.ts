import type { ManualRead, RunSnapshot } from '../types/api.ts';
import { decodeEventEnvelope, type EventEnvelope, type EventPayload } from '../types/events.ts';

const names: Record<EventPayload['type'], true> = {
  snapshot: true, run_phase_changed: true, query_started: true, query_progress: true,
  query_statement: true,
  query_completed: true, query_failed: true, replica_operation_observed: true, db_observed: true,
  stale_read_observed: true, manual_read_started: true, manual_read_completed: true,
  manual_read_failed: true, run_completed: true, run_failed: true,
};

export function openRunEvents(
  run: RunSnapshot, callbacks: { open(): void; event(event: EventEnvelope): void; interrupted(): void },
): EventSource {
  const source = new EventSource(`/api/runs/${encodeURIComponent(run.run_id)}/events?after=${run.sequence}`);
  source.onopen = callbacks.open;
  source.onerror = callbacks.interrupted;
  for (const name of Object.keys(names)) {
    source.addEventListener(name, (message: MessageEvent<string>) => {
      let event: EventEnvelope;
      try {
        event = decodeEventEnvelope(JSON.parse(message.data));
        if (event.type !== name || message.lastEventId !== `${event.run_id}:${event.sequence}`) {
          throw new Error('Inconsistent SSE event');
        }
      } catch {
        callbacks.interrupted();
        return;
      }
      callbacks.event(event);
    });
  }
  return source;
}

export function mergeRead(reads: readonly ManualRead[], read: ManualRead): ManualRead[] {
  const existing = reads.find(item => item.read_id === read.read_id);
  if (existing && existing.status !== 'pending' && read.status === 'pending') return [...reads];
  return [read, ...reads.filter(item => item.read_id !== read.read_id)]
    .sort((a, b) => b.started_at.localeCompare(a.started_at)).slice(0, 5);
}

/** undefined は連番の欠落や別インスタンスのスナップショットを表す。 */
export function applyRunEvent(run: RunSnapshot, event: EventEnvelope): RunSnapshot | undefined {
  if (event.run_id !== run.run_id || BigInt(event.sequence) <= BigInt(run.sequence)) return run;
  if (event.type !== 'snapshot' && BigInt(event.sequence) !== BigInt(run.sequence) + 1n) return undefined;
  const next = { ...run, sequence: event.sequence };
  switch (event.type) {
    case 'snapshot':
    case 'run_completed':
    case 'run_failed':
      return event.payload.snapshot.server_instance_id === run.server_instance_id
        ? event.payload.snapshot : undefined;
    case 'run_phase_changed':
      return { ...next, phase: event.payload.phase };
    case 'query_statement': {
      const plan = event.payload.plan;
      if (!run.plan.some(item => item.operation_id === plan.operation_id && item.kind === plan.kind)) return undefined;
      return { ...next, plan: run.plan.map(item => item.operation_id === plan.operation_id ? plan : item) };
    }
    case 'query_started':
    case 'query_progress':
    case 'query_completed':
    case 'query_failed':
    case 'replica_operation_observed':
      if (!run.plan.some(item => item.operation_id === event.payload.operation.operation_id)) return undefined;
      return { ...next, operations: run.operations.map(item =>
        item.operation_id === event.payload.operation.operation_id ? event.payload.operation : item) };
    case 'db_observed': {
      const sample = event.payload.observation;
      return { ...next, observations: [...run.observations.filter(item =>
        item.target !== sample.target || item.metric !== sample.metric), sample] };
    }
    case 'stale_read_observed':
      return { ...next, stale_read_observed: true };
    case 'manual_read_started':
    case 'manual_read_completed':
    case 'manual_read_failed':
      return { ...next, manual_reads: mergeRead(run.manual_reads, event.payload.read) };
  }
}

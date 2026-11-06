/** SSE通知の形式。api.tsの中心データを理解してから読む。 */
import type { DecimalString, UtcTimestamp, RunPhase, OperationState, Observation, ManualRead, RunSnapshot } from './api.ts';
import { readEventEnvelope } from './validation.ts';

export type EventPayload =
  | { type: 'run_phase_changed'; payload: { phase: RunPhase } }
  | { type: 'query_started' | 'query_completed' | 'query_failed'; payload: { operation: OperationState } }
  | { type: 'db_observed'; payload: { observation: Observation } }
  | { type: 'stale_read_observed'; payload: { source_value: DecimalString; replica_value: DecimalString } }
  | { type: 'manual_read_started' | 'manual_read_completed' | 'manual_read_failed'; payload: { read: ManualRead } }
  | { type: 'run_completed' | 'run_failed' | 'snapshot'; payload: { snapshot: RunSnapshot } };
export type EventEnvelope = {
  schema_version: 1;
  run_id: string;
  sequence: DecimalString;
  occurred_at: UtcTimestamp;
} & EventPayload;

/** 不正な形式と、通知種別に矛盾する結果を拒否する。 */
export function decodeEventEnvelope(input: unknown): EventEnvelope {
  return readEventEnvelope(input);
}

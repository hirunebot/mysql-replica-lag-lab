/**
 * 最初に読む型: RunSnapshot（実行全体）、OperationPlan / OperationState（予定・実績）、
 * Observation（DBの実測値）、ManualRead（手動SELECT結果）。
 * DashboardStateは実行前も含む外側の箱、RunConfigは負荷設定。
 * 小さな状態型はフィールドから必要なときだけ読む。SSEの詳細はevents.tsへ分離。
 * 読み方: docs/12-contracts-reading-guide.md。検証処理の詳細はvalidation.ts。
 */
export { decodeEventEnvelope } from './events.ts';
export type { EventEnvelope, EventPayload } from './events.ts';
import { readDashboardState, readRunSnapshot } from './validation.ts';
export type DecimalString = string;
export type UtcTimestamp = string;
export type DatabaseTarget = 'source' | 'replica';
export type RunPhase = 'idle' | 'preparing' | 'waiting_initial_sync' | 'running'
  | 'waiting_final_sync' | 'failing' | 'completed' | 'failed';
export interface RunConfig {
  demo_rows: DecimalString;
  seed_batch_size: DecimalString;
  heavy_batch_size: DecimalString;
  marker_after_batches: DecimalString;
}
export interface ApiError { code: string; message: string; retryable: boolean }
export type OperationKind = 'setup' | 'seed' | 'heavy_update' | 'marker_update' | 'observe' | 'final_sync';
export interface SqlStatement { template: string; parameters: string[] }
export interface OperationPlan {
  operation_id: string;
  kind: OperationKind;
  target: DatabaseTarget;
  batch_number: DecimalString | null;
  first_id: DecimalString | null;
  last_id: DecimalString | null;
  planned_rows: DecimalString | null;
  sql: SqlStatement;
  after_committed: string[];
}
export type OperationStatus = 'planned' | 'running' | 'committed' | 'failed' | 'outcome_unknown';
export type ReplicaApplicationStatus = 'unknown' | 'pending' | 'observed_applied' | 'unavailable';
export interface OperationState {
  operation_id: string;
  status: OperationStatus;
  replica_status: ReplicaApplicationStatus;
  replica_observed_at: UtcTimestamp | null;
  started_at: UtcTimestamp | null;
  completed_at: UtcTimestamp | null;
  elapsed_ms: number | null;
  affected_rows: DecimalString | null;
  error: ApiError | null;
}
export type ObservationMetric = 'total_rows' | 'updated_rows' | 'marker' | 'replication';
export type ObservationStatus = 'not_observed' | 'not_ready' | 'observing' | 'available' | 'unavailable';
export interface ReplicationState {
  receiver_running: boolean;
  applier_running: boolean;
  seconds_behind_source: number | null;
  relay_log_bytes: DecimalString;
  source_log_file: string;
  read_source_log_pos: DecimalString;
  relay_source_log_file: string;
  exec_source_log_pos: DecimalString;
}
export type ObservationValue =
  | { kind: 'decimal'; value: DecimalString }
  | { kind: 'replication'; value: ReplicationState };
export interface Observation {
  target: DatabaseTarget;
  metric: ObservationMetric;
  status: ObservationStatus;
  value: ObservationValue | null;
  started_at: UtcTimestamp | null;
  observed_at: UtcTimestamp | null;
  elapsed_ms: number | null;
  error: ApiError | null;
}
export interface ManualRead {
  read_id: string;
  run_id: string;
  target: DatabaseTarget;
  preset: 'marker_sum';
  sql: string;
  status: 'pending' | 'succeeded' | 'failed';
  value: DecimalString | null;
  started_at: UtcTimestamp;
  completed_at: UtcTimestamp | null;
  elapsed_ms: number | null;
  error: ApiError | null;
}
export interface RunSnapshot {
  schema_version: 1;
  server_instance_id: string;
  run_id: string;
  sequence: DecimalString;
  phase: RunPhase;
  config: RunConfig;
  plan: OperationPlan[];
  operations: OperationState[];
  observations: Observation[];
  manual_reads: ManualRead[];
  stale_read_observed: boolean;
  error: ApiError | null;
}
/** Validate untrusted JSON and return its DTO; throw on malformed/unsupported data. */
export function decodeRunSnapshot(input: unknown): RunSnapshot {
  return readRunSnapshot(input);
}
export interface DatabaseConnection {
  target: DatabaseTarget;
  display_address: string;
  status: 'not_checked' | 'checking' | 'available' | 'unavailable';
  checked_at: UtcTimestamp | null;
  error: ApiError | null;
}
export interface DashboardState {
  schema_version: 1;
  server_instance_id: string;
  default_config: RunConfig;
  current_run: RunSnapshot | null;
  connections: DatabaseConnection[];
}

/** Decode initial state, including idle/unreachable DB states, without inventing a run. */
export function decodeDashboardState(input: unknown): DashboardState {
  return readDashboardState(input);
}

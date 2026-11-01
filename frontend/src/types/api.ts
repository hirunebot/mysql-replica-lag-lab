/** Step 1 contract proposal. This file does not implement an API client or UI. */
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

/** Validate untrusted JSON and return its DTO; throw on malformed/unsupported data. */
export function decodeRunSnapshot(_input: unknown): RunSnapshot {
  throw new Error('Not implemented: awaiting Step 1 contract/test approval');
}
/** Validate an SSE envelope and its payload; never silently accept an unknown type. */
export function decodeEventEnvelope(_input: unknown): EventEnvelope {
  throw new Error('Not implemented: awaiting Step 1 contract/test approval');
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
export function decodeDashboardState(_input: unknown): DashboardState {
  throw new Error('Not implemented: awaiting Step 1 contract/test approval');
}

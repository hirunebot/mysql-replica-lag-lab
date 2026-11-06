/** 型定義を読むときは不要。受信JSONの形と、状態間の整合を確認する。 */
import type { DashboardState, RunSnapshot, OperationState, Observation, ManualRead } from './api.ts';
import type { EventEnvelope } from './events.ts';

type Check = (value: unknown, path: string) => void;
function demand(ok: boolean, path: string): asserts ok {
  if (!ok) throw new Error(`Invalid contract: ${path}`);
}
const str: Check = (v,p) => demand(typeof v === 'string',p);
const id: Check = (v,p) => demand(typeof v === 'string' && v.trim().length > 0,p);
const bool: Check = (v,p) => demand(typeof v === 'boolean',p);
const decimal: Check = (v,p) => demand(typeof v === 'string' && /^(0|[1-9][0-9]*)$/.test(v),p);
const duration: Check = (v,p) => demand(typeof v === 'number' && Number.isFinite(v) && v >= 0,p);
const choice = (...values: unknown[]): Check => (v,p) => demand(values.includes(v),p);
const nullable = (c: Check): Check => (v,p) => { if (v !== null) c(v,p); };
const array = (c: Check): Check => (v,p) => {
  demand(Array.isArray(v),p); v.forEach((x,i) => c(x,`${p}[${i}]`));
};
const object = (fields: Record<string, Check>): Check => (v,p) => {
  demand(typeof v === 'object' && v !== null && !Array.isArray(v),p);
  const record = v as Record<string,unknown>;
  demand(Object.keys(record).length === Object.keys(fields).length,p);
  for (const [key,check] of Object.entries(fields)) {
    demand(Object.hasOwn(record,key),`${p}.${key}`); check(record[key],`${p}.${key}`);
  }
};
const timestamp: Check = (v,p) => {
  demand(typeof v === 'string',p);
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d+)?Z$/.exec(v);
  demand(m !== null,p);
  const [y,mo,d,h,mi,s] = m.slice(1,7).map(Number);
  const days = mo === 2 ? (y%4 === 0 && (y%100 !== 0 || y%400 === 0) ? 29 : 28)
    : [4,6,9,11].includes(mo) ? 30 : 31;
  demand(mo >= 1 && mo <= 12 && d >= 1 && d <= days && h < 24 && mi < 60 && s < 60,p);
};
const target = choice('source','replica');
const phase = choice('idle','preparing','waiting_initial_sync','running','waiting_final_sync','failing','completed','failed');
const error = nullable(object({code:str,message:str,retryable:bool}));
const config = object({demo_rows:decimal,seed_batch_size:decimal,heavy_batch_size:decimal,marker_after_batches:decimal});
const replication = object({receiver_running:bool,applier_running:bool,seconds_behind_source:nullable(duration),
  relay_log_bytes:decimal,source_log_file:str,read_source_log_pos:decimal,relay_source_log_file:str,exec_source_log_pos:decimal});
const observationValue: Check = (v,p) => {
  demand(typeof v === 'object' && v !== null,p);
  const kind = (v as {kind?:unknown}).kind;
  if (kind === 'decimal') object({kind:choice('decimal'),value:decimal})(v,p);
  else object({kind:choice('replication'),value:replication})(v,p);
};
const observationShape = object({target,metric:choice('total_rows','updated_rows','marker','replication'),
  status:choice('not_observed','not_ready','observing','available','unavailable'),value:nullable(observationValue),
  started_at:nullable(timestamp),observed_at:nullable(timestamp),elapsed_ms:nullable(duration),error});
const operationShape = object({operation_id:id,status:choice('planned','running','committed','failed','outcome_unknown'),
  replica_status:choice('unknown','pending','observed_applied','unavailable'),replica_observed_at:nullable(timestamp),
  started_at:nullable(timestamp),completed_at:nullable(timestamp),elapsed_ms:nullable(duration),affected_rows:nullable(decimal),error});
const plan = object({operation_id:id,kind:choice('setup','seed','heavy_update','marker_update','observe','final_sync'),target,
  batch_number:nullable(decimal),first_id:nullable(decimal),last_id:nullable(decimal),planned_rows:nullable(decimal),
  sql:object({template:str,parameters:array(str)}),after_committed:array(id)});
const readShape = object({read_id:id,run_id:id,target,preset:choice('marker_sum'),sql:str,
  status:choice('pending','succeeded','failed'),value:nullable(decimal),started_at:timestamp,
  completed_at:nullable(timestamp),elapsed_ms:nullable(duration),error});
const snapshotShape = object({schema_version:choice(1),server_instance_id:id,run_id:id,sequence:decimal,phase,config,
  plan:array(plan),operations:array(operationShape),observations:array(observationShape),manual_reads:array(readShape),
  stale_read_observed:bool,error});

function operation(o: OperationState): void {
  demand(o.status !== 'committed' || (o.started_at !== null && o.completed_at !== null
    && o.elapsed_ms !== null && o.affected_rows !== null && o.error === null),'operation.committed');
  demand(o.replica_status !== 'observed_applied' || o.replica_observed_at !== null,'operation.replica_observed_at');
}
function observation(o: Observation): void {
  demand(o.value === null || (o.metric === 'replication') === (o.value.kind === 'replication'),'observation.metric');
  let valid: boolean;
  switch (o.status) {
    case 'not_observed': valid = o.value === null && o.observed_at === null && o.started_at === null && o.elapsed_ms === null && o.error === null; break;
    case 'not_ready': valid = o.value === null && o.observed_at === null && o.error !== null; break;
    case 'available': valid = o.value !== null && o.observed_at !== null && o.started_at !== null && o.elapsed_ms !== null && o.error === null; break;
    case 'observing': valid = (o.value !== null) === (o.observed_at !== null) && o.started_at !== null && o.error === null; break;
    case 'unavailable': valid = (o.value !== null) === (o.observed_at !== null) && o.error !== null; break;
  }
  demand(valid,'observation.status');
}
function manualRead(r: ManualRead,run: string): void {
  demand(r.run_id === run && r.target === 'replica' && r.sql === 'SELECT SUM(value) FROM lag_demo.marker;','read.target');
  const valid = r.status === 'pending'
    ? r.value === null && r.completed_at === null && r.elapsed_ms === null && r.error === null
    : r.completed_at !== null && r.elapsed_ms !== null && (r.status === 'succeeded'
      ? r.error === null : r.value === null && r.error !== null);
  demand(valid,'read.status');
}
function completed(s: RunSnapshot): void {
  const value = (target: string,metric: string) => {
    const rows = s.observations.filter(o => o.target === target && o.metric === metric);
    return rows.length === 1 && rows[0].status === 'available' ? rows[0].value : null;
  };
  demand(s.error === null && s.plan.length > 0 && s.operations.length === s.plan.length
    && s.operations.every(o => o.status === 'committed'),'completed.operations');
  for (const target of ['source','replica']) {
    for (const metric of ['total_rows','updated_rows','marker']) {
      const v = value(target,metric);
      demand(v?.kind === 'decimal' && v.value === (metric === 'marker' ? '1' : s.config.demo_rows),'completed.observations');
    }
  }
  const r = value('replica','replication');
  demand(r?.kind === 'replication' && r.value.receiver_running && r.value.applier_running,'completed.replication');
}
export function readRunSnapshot(input: unknown): RunSnapshot {
  snapshotShape(input,'snapshot');
  const s = input as RunSnapshot;
  const ids = new Set(s.plan.map(p => p.operation_id));
  demand(ids.size === s.plan.length,'plan.operation_id');
  for (const p of s.plan) demand(p.after_committed.every(x => x !== p.operation_id && ids.has(x)),'plan.after_committed');
  const seen = new Set<string>();
  for (const o of s.operations) {
    operation(o); demand(ids.has(o.operation_id) && !seen.has(o.operation_id),'operations.operation_id'); seen.add(o.operation_id);
  }
  s.observations.forEach(observation);
  const reads = new Set<string>();
  for (const r of s.manual_reads) {
    manualRead(r,s.run_id); demand(!reads.has(r.read_id),'manual_reads.read_id'); reads.add(r.read_id);
  }
  if (s.phase === 'completed') completed(s);
  return s;
}
const dashboardShape = object({schema_version:choice(1),server_instance_id:id,default_config:config,
  current_run:nullable(snapshotShape),connections:array(object({target,display_address:str,
    status:choice('not_checked','checking','available','unavailable'),checked_at:nullable(timestamp),error}))});
export function readDashboardState(input: unknown): DashboardState {
  dashboardShape(input,'dashboard');
  const s = input as DashboardState;
  if (s.current_run !== null) {
    readRunSnapshot(s.current_run);
    demand(s.current_run.server_instance_id === s.server_instance_id,'current_run.server_instance_id');
  }
  for (const c of s.connections) demand(!/[\/@\s]/.test(c.display_address) && /:\d+$/.test(c.display_address),'connection.display_address');
  return s;
}
export function readEventEnvelope(input: unknown): EventEnvelope {
  // 種別ごとにpayloadの項目を限定し、未知項目も拒否する。
  demand(typeof input === 'object' && input !== null,'event');
  const type = (input as {type?:unknown}).type;
  const payloads: Record<string,Check> = {
    run_phase_changed:object({phase}),query_started:object({operation:operationShape}),
    query_completed:object({operation:operationShape}),query_failed:object({operation:operationShape}),
    db_observed:object({observation:observationShape}),
    stale_read_observed:object({source_value:decimal,replica_value:decimal}),
    manual_read_started:object({read:readShape}),manual_read_completed:object({read:readShape}),manual_read_failed:object({read:readShape}),
    run_completed:object({snapshot:snapshotShape}),run_failed:object({snapshot:snapshotShape}),snapshot:object({snapshot:snapshotShape}),
  };
  demand(typeof type === 'string' && Object.hasOwn(payloads,type),'event.type');
  object({schema_version:choice(1),run_id:id,sequence:decimal,occurred_at:timestamp,type:choice(type),payload:payloads[type]})(input,'event');
  const e = input as EventEnvelope;
  if ('operation' in e.payload) {
    const o = e.payload.operation; operation(o);
    demand(type === 'query_started' ? o.status === 'running' : type === 'query_completed'
      ? o.status === 'committed' : ['failed','outcome_unknown'].includes(o.status),'event.operation.status');
  } else if ('observation' in e.payload) observation(e.payload.observation);
  else if ('source_value' in e.payload) demand(e.payload.source_value === '1' && e.payload.replica_value !== '1','event.stale');
  else if ('read' in e.payload) {
    const r = e.payload.read; manualRead(r,e.run_id);
    demand(r.status === (type === 'manual_read_started' ? 'pending' : type === 'manual_read_completed' ? 'succeeded' : 'failed'),'event.read.status');
  } else if ('snapshot' in e.payload) {
    const s = readRunSnapshot(e.payload.snapshot);
    demand(s.run_id === e.run_id && s.sequence === e.sequence,'event.snapshot');
    demand(type !== 'run_completed' || s.phase === 'completed','event.completed');
    demand(type !== 'run_failed' || s.phase === 'failed','event.failed');
  }
  return e;
}

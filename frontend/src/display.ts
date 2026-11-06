import type { OperationPlan, OperationStatus, ReplicaApplicationStatus, RunPhase } from './types/api.ts';

const numbers = new Intl.NumberFormat('ja-JP');
const times = new Intl.DateTimeFormat('ja-JP', {
  year: 'numeric', month: '2-digit', day: '2-digit',
  hour: '2-digit', minute: '2-digit', second: '2-digit', timeZoneName: 'short',
});

export const phaseLabel: Record<RunPhase, string> = {
  idle: '実行前', preparing: '準備中', waiting_initial_sync: '初期同期中', running: '実行中',
  waiting_final_sync: '最終同期中', completed: '同期完了', failing: '終了確認中', failed: '実行失敗',
};

export function formatTime(value: string): string {
  return times.format(new Date(value));
}

export function formatDecimal(value?: string): string {
  return value === undefined ? '—' : numbers.format(BigInt(value));
}

export function operationLabel(operation: OperationPlan): string {
  switch (operation.kind) {
    case 'setup': return 'スキーマ準備';
    case 'seed': return '初期データ投入';
    case 'heavy_update': return `UPDATE ${operation.batch_number ?? ''}`;
    case 'marker_update': return 'marker更新';
    case 'final_sync': return '最終同期確認';
    case 'observe': return 'DB観測';
  }
}

export const operationStatus: Record<OperationStatus, string> = {
  planned: '予定', running: '実行中', committed: '完了', failed: '失敗', outcome_unknown: '結果不明',
};

export const replicaStatus: Record<ReplicaApplicationStatus, string> = {
  unknown: '未確認', pending: '反映待ち', observed_applied: '反映確認済み', unavailable: '確認失敗',
};

export const operationPurpose: Record<OperationPlan['kind'], string> = {
  setup: 'デモ用のスキーマとテーブルを作り直す。',
  seed: '初期データを分割して投入する。SQLは直近のINSERTを表示する。',
  heavy_update: '指定のID範囲を更新し、Replicaに適用される負荷を作る。',
  marker_update: '指定バッチのコミット後にmarkerを更新する。後続のUPDATEと並行できる。',
  observe: 'DBに読み取りを発行し、実値を観測する。',
  final_sync: 'marker一致後も、全行の更新と複製の稼働を確認する。',
};

export function operationRange(operation: OperationPlan): string | undefined {
  if (operation.first_id === null || operation.last_id === null) return;
  const first = formatDecimal(operation.first_id);
  return `ID ${first}${operation.first_id === operation.last_id ? '' : `–${formatDecimal(operation.last_id)}`}`;
}

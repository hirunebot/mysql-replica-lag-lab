import type { OperationPlan, RunPhase } from './types/api.ts';

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

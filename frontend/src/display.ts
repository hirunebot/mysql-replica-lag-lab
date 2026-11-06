import type { OperationPlan } from './types/api.ts';

const numbers = new Intl.NumberFormat('ja-JP');

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

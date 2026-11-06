<script setup lang="ts">
import { formatDecimal, formatTime } from '../display.ts';
import type { ManualRead } from '../types/api.ts';

defineProps<{ enabled?: boolean; reading?: boolean; reads?: ManualRead[]; error?: string }>();
defineEmits<{ read: []; clear: [] }>();

function result(read: ManualRead): string {
  if (read.status === 'failed') return '失敗';
  if (read.status === 'pending') return '取得中';
  return read.value === null ? 'NULL' : formatDecimal(read.value);
}
</script>

<template>
  <div class="manual-read">
    <div class="read-action">
      <code>SELECT SUM(value) FROM lag_demo.marker;</code>
      <button type="button" :disabled="!enabled" @click="$emit('read')">{{ reading ? '取得中' : 'Slaveで実行' }}</button>
    </div>
    <p v-if="error" class="warning" role="alert">{{ error }}</p>
    <div v-if="reads?.length" class="read-history">
      <div class="section-top">
        <span>実行結果（直近5件）</span>
        <button type="button" @click="$emit('clear')">表示をクリア</button>
      </div>
      <div class="table-wrap">
        <table aria-label="手動SELECTの履歴">
          <thead><tr><th scope="col">SUM(value)</th><th scope="col">実行時刻</th></tr></thead>
          <tbody>
            <tr v-for="read in reads" :key="read.read_id">
              <td><strong>{{ result(read) }}</strong><small v-if="read.error" class="row-note">{{ read.error.message }}</small></td>
              <td><time :datetime="read.started_at">{{ formatTime(read.started_at) }}</time></td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</template>

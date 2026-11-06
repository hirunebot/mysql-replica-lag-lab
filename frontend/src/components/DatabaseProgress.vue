<script setup lang="ts">
import { computed } from 'vue';
import type { DatabaseConnection, Observation } from '../types/api.ts';
import { formatDecimal, formatTime } from '../display.ts';

const props = defineProps<{
  label: string; address?: string; connection?: DatabaseConnection['status'];
  count?: Observation; marker?: Observation; total?: string; stale?: boolean;
}>();

function decimal(observation?: Observation) {
  return observation?.value?.kind === 'decimal' ? observation.value.value : undefined;
}

const percent = computed(() => {
  const count = decimal(props.count);
  if (count === undefined || !props.total || props.total === '0') return undefined;
  return Math.min(100, Number(BigInt(count) * 1000n / BigInt(props.total)) / 10);
});

function freshness(observation?: Observation): string {
  const time = observation?.observed_at ? formatTime(observation.observed_at) : '';
  if (observation?.status === 'unavailable') return time ? `取得失敗 · 前回値 ${time}` : '取得失敗';
  if (props.stale && time) return `前回値 ${time}`;
  if (observation?.status === 'observing') return time ? `取得中 · 前回値 ${time}` : '取得中';
  if (observation?.status === 'not_ready') return '準備中';
  return time || '未取得';
}
</script>

<template>
  <article class="db">
    <div class="db-header">
      <h2>{{ label }}</h2>
      <span v-if="address" class="server">{{ address }}</span>
      <span v-if="connection === 'unavailable'" class="muted">DB接続不可</span>
    </div>
    <div class="big"><b>{{ formatDecimal(decimal(count)) }}</b> <span>/ {{ formatDecimal(total) }} 件</span></div>
    <div class="meter" role="progressbar" :aria-label="`${label} UPDATE反映済み`" :aria-valuenow="percent">
      <div :style="{ width: `${percent ?? 0}%` }" />
    </div>
    <div class="stats">
      <span>UPDATE反映済み <template v-if="percent !== undefined">{{ percent }}%</template></span>
      <span>{{ freshness(count) }}</span>
    </div>
    <div class="marker">
      <code>marker.value</code><strong>{{ formatDecimal(decimal(marker)) }}</strong>
      <small class="muted">{{ freshness(marker) }}</small>
    </div>
    <slot />
  </article>
</template>

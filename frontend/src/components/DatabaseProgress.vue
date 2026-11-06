<script setup lang="ts">
import { computed } from 'vue';
import type { Observation } from '../types/api.ts';
import { formatDecimal } from '../display.ts';

const props = defineProps<{ label: string; address?: string; count?: Observation; marker?: Observation; total?: string }>();

function decimal(observation?: Observation) {
  return observation?.value?.kind === 'decimal' ? observation.value.value : undefined;
}

const percent = computed(() => {
  const count = decimal(props.count);
  if (count === undefined || !props.total || props.total === '0') return undefined;
  return Math.min(100, Number(BigInt(count) * 1000n / BigInt(props.total)) / 10);
});
</script>

<template>
  <article class="db">
    <div class="db-header">
      <h2>{{ label }}</h2>
      <span v-if="address" class="server">{{ address }}</span>
    </div>
    <div class="big"><b>{{ formatDecimal(decimal(count)) }}</b> <span>/ {{ formatDecimal(total) }} 件</span></div>
    <div class="meter" role="progressbar" :aria-label="`${label} UPDATE反映済み`" :aria-valuenow="percent">
      <div :style="{ width: `${percent ?? 0}%` }" />
    </div>
    <div class="stats"><span>UPDATE反映済み</span><span>{{ percent === undefined ? '未取得' : `${percent}%` }}</span></div>
    <div class="marker"><code>marker.value</code><strong>{{ formatDecimal(decimal(marker)) }}</strong></div>
    <slot />
  </article>
</template>

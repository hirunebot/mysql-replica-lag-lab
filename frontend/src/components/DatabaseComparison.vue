<script setup lang="ts">
import type { DatabaseConnection, DatabaseTarget, ObservationMetric, RunSnapshot } from '../types/api.ts';
import DatabaseProgress from './DatabaseProgress.vue';
import ManualReadPanel from './ManualReadPanel.vue';

const props = defineProps<{ snapshot: RunSnapshot | null; connections?: DatabaseConnection[] }>();
const targets = [{ key: 'source', name: 'Master' }, { key: 'replica', name: 'Slave' }] as const;

function observation(target: DatabaseTarget, metric: ObservationMetric) {
  return props.snapshot?.observations.find(item => item.target === target && item.metric === metric);
}
</script>

<template>
  <section class="compare" aria-label="UPDATE反映件数">
    <DatabaseProgress
      v-for="target in targets"
      :key="target.key"
      :label="target.name"
      :address="connections?.find(connection => connection.target === target.key)?.display_address"
      :count="observation(target.key, 'updated_rows')"
      :marker="observation(target.key, 'marker')"
      :total="snapshot?.config.demo_rows"
    >
      <ManualReadPanel v-if="target.key === 'replica'" />
    </DatabaseProgress>
  </section>
</template>

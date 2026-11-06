<script setup lang="ts">
import type { DatabaseConnection, DatabaseTarget, ManualRead, ObservationMetric, RunSnapshot } from '../types/api.ts';
import DatabaseProgress from './DatabaseProgress.vue';
import ManualReadPanel from './ManualReadPanel.vue';

const props = defineProps<{
  snapshot: RunSnapshot | null; connections?: DatabaseConnection[]; stale?: boolean;
  canRead?: boolean; reading?: boolean; manualReads?: ManualRead[]; readError?: string;
}>();
defineEmits<{ read: []; clearReads: [] }>();
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
      :connection="connections?.find(connection => connection.target === target.key)?.status"
      :count="observation(target.key, 'updated_rows')"
      :marker="observation(target.key, 'marker')"
      :total="snapshot?.config.demo_rows"
      :stale="stale"
    >
      <ManualReadPanel
        v-if="target.key === 'replica'"
        :enabled="canRead" :reading="reading" :reads="manualReads" :error="readError"
        @read="$emit('read')" @clear="$emit('clearReads')"
      />
    </DatabaseProgress>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { decodeRunSnapshot, type RunSnapshot } from './types/api.ts';
import DatabaseComparison from './components/DatabaseComparison.vue';
import OperationTimeline from './components/OperationTimeline.vue';
import QueryDetail from './components/QueryDetail.vue';
import RunToolbar from './components/RunToolbar.vue';
import { useRun } from './composables/useRun.ts';
import { phaseLabel } from './display.ts';

const showFixture = import.meta.env.DEV && new URLSearchParams(location.search).get('fixture') === '1';
const fixtureRun = ref<RunSnapshot | null>(null);
const live = useRun(!showFixture);
const snapshot = computed(() => showFixture ? fixtureRun.value : live.snapshot.value);
const connection = live.connection;
const status = computed(() => {
  if (showFixture) return 'サンプル / DB未接続';
  switch (connection.value) {
    case 'loading': return '取得中';
    case 'disconnected': return '接続中断';
    case 'unavailable': return 'APIに接続できません';
    case 'connected': return snapshot.value ? phaseLabel[snapshot.value.phase] : '実行前';
  }
});
const selectedId = ref<string>();
const selected = computed(() => snapshot.value?.plan.find(operation => operation.operation_id === selectedId.value));
watch(() => snapshot.value?.run_id, () => {
  selectedId.value = snapshot.value?.plan[0]?.operation_id;
});

if (import.meta.env.DEV && showFixture) {
  void import('../../tests/fixtures/dashboard/snapshot.json').then(({ default: fixture }) => {
    fixtureRun.value = decodeRunSnapshot(fixture);
  });
}
</script>

<template>
  <main class="dashboard">
    <RunToolbar :status="status" :can-refresh="!showFixture" @refresh="live.refresh()" />
    <DatabaseComparison
      :snapshot="snapshot"
      :connections="live.dashboard.value?.connections"
      :stale="!showFixture && connection !== 'connected'"
    />
    <div class="lower">
      <OperationTimeline
        :snapshot="snapshot"
        :selected-id="selectedId"
        @select="selectedId = $event"
      />
      <QueryDetail :operation="selected" />
    </div>
  </main>
</template>

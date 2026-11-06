<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { decodeRunSnapshot, type RunConfig, type RunSnapshot } from './types/api.ts';
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
const settings = ref<RunConfig>();
const starting = live.starting;
const canStart = live.canStart;
const canConfigure = live.canConfigure;
const canRead = live.canRead;
const pendingRead = live.pendingRead;
const manualReads = live.manualReads;
const startError = live.startError;
const readError = live.readError;
watch(() => live.dashboard.value?.server_instance_id, () => {
  settings.value = live.dashboard.value?.current_run?.config ?? live.dashboard.value?.default_config;
});
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
const selectedState = computed(() => snapshot.value?.operations.find(operation => operation.operation_id === selectedId.value));
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
    <RunToolbar
      :status="status" :can-refresh="!showFixture" :can-start="canStart && !!settings" :starting="starting"
      :show-start-note="canConfigure" :error="startError || snapshot?.error?.message"
      @refresh="live.refresh()" @start="settings && live.start(settings)"
    />
    <DatabaseComparison
      :snapshot="snapshot"
      :connections="live.dashboard.value?.connections"
      :stale="!showFixture && connection !== 'connected'"
      :can-read="canRead" :reading="pendingRead" :manual-reads="manualReads" :read-error="readError"
      @read="live.read()" @clear-reads="live.clearReads()"
    />
    <div class="lower">
      <OperationTimeline
        :snapshot="snapshot"
        :selected-id="selectedId"
        :stale="!showFixture && connection !== 'connected'"
        :settings="showFixture ? snapshot?.config : settings" :can-configure="canConfigure"
        @select="selectedId = $event"
        @configure="settings = $event"
      />
      <QueryDetail :operation="selected" :state="selectedState" />
    </div>
  </main>
</template>

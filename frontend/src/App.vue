<script setup lang="ts">
import { computed, ref } from 'vue';
import { decodeRunSnapshot, type RunSnapshot } from './types/api.ts';
import DatabaseComparison from './components/DatabaseComparison.vue';
import OperationTimeline from './components/OperationTimeline.vue';
import QueryDetail from './components/QueryDetail.vue';
import RunToolbar from './components/RunToolbar.vue';

const showFixture = import.meta.env.DEV && new URLSearchParams(location.search).get('fixture') === '1';
const snapshot = ref<RunSnapshot | null>(null);
const selectedId = ref<string>();
const selected = computed(() => snapshot.value?.plan.find(operation => operation.operation_id === selectedId.value));

if (import.meta.env.DEV && showFixture) {
  void import('../../tests/fixtures/dashboard/snapshot.json').then(({ default: fixture }) => {
    snapshot.value = decodeRunSnapshot(fixture);
    selectedId.value = snapshot.value.plan[0]?.operation_id;
  });
}
</script>

<template>
  <main class="dashboard">
    <RunToolbar :status="showFixture ? 'サンプル / DB未接続' : '未取得'" />
    <DatabaseComparison :snapshot="snapshot" />
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

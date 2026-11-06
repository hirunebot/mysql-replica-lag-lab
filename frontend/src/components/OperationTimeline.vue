<script setup lang="ts">
import type { RunSnapshot } from '../types/api.ts';
import { formatDecimal, operationLabel } from '../display.ts';
import RunSettingsDialog from './RunSettingsDialog.vue';

defineProps<{ snapshot: RunSnapshot | null; selectedId?: string }>();
defineEmits<{ select: [id: string] }>();
</script>

<template>
  <section class="panel" aria-labelledby="flow-heading">
    <div class="section-top">
      <h2 id="flow-heading">処理の流れ</h2>
      <RunSettingsDialog :config="snapshot?.config" />
    </div>
    <p class="flow-config" v-if="snapshot">
      全 {{ formatDecimal(snapshot.config.demo_rows) }} 件 / UPDATE {{ formatDecimal(snapshot.config.heavy_batch_size) }} 件ずつ<br>
      marker更新 {{ formatDecimal(snapshot.config.marker_after_batches) }} バッチ完了後
    </p>
    <div class="table-wrap">
      <table>
        <thead><tr><th scope="col">処理・対象</th><th scope="col">Master</th><th scope="col">Slave</th></tr></thead>
        <tbody>
          <tr v-for="operation in snapshot?.plan" :key="operation.operation_id" :class="{ selected: selectedId === operation.operation_id }">
            <td><button type="button" class="batch-button" :aria-pressed="selectedId === operation.operation_id" @click="$emit('select', operation.operation_id)">{{ operationLabel(operation) }}</button></td>
            <td>—</td><td>—</td>
          </tr>
          <tr v-if="!snapshot?.plan.length"><td colspan="3" class="muted">処理の計画は未取得です。</td></tr>
        </tbody>
      </table>
    </div>
  </section>
</template>

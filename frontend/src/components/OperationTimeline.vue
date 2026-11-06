<script setup lang="ts">
import { computed } from 'vue';
import { TooltipContent, TooltipPortal, TooltipProvider, TooltipRoot, TooltipTrigger } from 'reka-ui';
import type { RunConfig, RunSnapshot } from '../types/api.ts';
import { formatDecimal, formatTime, operationLabel, operationPurpose, operationRange, operationStatus, replicaStatus } from '../display.ts';
import RunSettingsDialog from './RunSettingsDialog.vue';

const props = defineProps<{
  snapshot: RunSnapshot | null; selectedId?: string; stale?: boolean; settings?: RunConfig; canConfigure?: boolean;
}>();
defineEmits<{ select: [id: string]; configure: [config: RunConfig] }>();
const rows = computed(() => {
  const states = new Map(props.snapshot?.operations.map(state => [state.operation_id, state]));
  return props.snapshot?.plan.map(plan => ({ plan, state: states.get(plan.operation_id) })) ?? [];
});
</script>

<template>
  <section class="panel" aria-labelledby="flow-heading">
    <div class="section-top">
      <h2 id="flow-heading">処理の流れ</h2>
      <RunSettingsDialog :config="settings" :editable="canConfigure" @save="$emit('configure', $event)" />
    </div>
    <p class="flow-config" v-if="snapshot">
      全 {{ formatDecimal(snapshot.config.demo_rows) }} 件 / UPDATE {{ formatDecimal(snapshot.config.heavy_batch_size) }} 件ずつ<br>
      marker更新 {{ formatDecimal(snapshot.config.marker_after_batches) }} バッチ完了後
    </p>
    <TooltipProvider :delay-duration="550" :skip-delay-duration="0">
    <div class="table-wrap">
      <table>
        <thead><tr><th scope="col">処理・対象</th><th scope="col">Master</th><th scope="col">Slave</th></tr></thead>
        <tbody>
          <tr v-for="{ plan, state } in rows" :key="plan.operation_id" :class="{ selected: selectedId === plan.operation_id }">
            <td>
              <TooltipRoot>
                <TooltipTrigger as-child>
                  <button type="button" class="batch-button" :aria-pressed="selectedId === plan.operation_id" @click="$emit('select', plan.operation_id)">
                    {{ operationLabel(plan) }}
                  </button>
                </TooltipTrigger>
                <TooltipPortal>
                  <TooltipContent class="operation-tooltip" :side-offset="6">{{ operationPurpose[plan.kind] }}</TooltipContent>
                </TooltipPortal>
              </TooltipRoot>
              <small v-if="plan.planned_rows !== null" class="row-note">
                <template v-if="plan.kind !== 'seed' && operationRange(plan)">{{ operationRange(plan) }} / </template>
                {{ formatDecimal(plan.planned_rows) }} 件
              </small>
            </td>
            <td>
              <span v-if="plan.target === 'source'" class="badge" :data-status="state?.status">{{ operationStatus[state?.status ?? 'planned'] }}</span>
              <span v-else>—</span>
            </td>
            <td>
              <span v-if="plan.target === 'replica'" class="badge" :data-status="state?.status">{{ operationStatus[state?.status ?? 'planned'] }}</span>
              <template v-else>
                <span class="badge" :data-status="state?.replica_status">{{ replicaStatus[state?.replica_status ?? 'unknown'] }}</span>
                <small v-if="state?.replica_observed_at" class="row-note">
                  <template v-if="stale">前回 </template>{{ formatTime(state.replica_observed_at) }}
                </small>
              </template>
            </td>
          </tr>
          <tr v-if="!snapshot?.plan.length"><td colspan="3" class="muted">処理の計画は未取得です。</td></tr>
        </tbody>
      </table>
    </div>
    </TooltipProvider>
    <p v-if="snapshot" class="table-note muted">markerと後続のUPDATEは並行実行できます。</p>
  </section>
</template>

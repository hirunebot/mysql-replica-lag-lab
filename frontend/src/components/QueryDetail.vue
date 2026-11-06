<script setup lang="ts">
import { ref, watch } from 'vue';
import type { OperationPlan, OperationState } from '../types/api.ts';
import { formatDecimal, formatTime, operationLabel, operationPurpose, operationRange, operationStatus } from '../display.ts';

const props = defineProps<{ operation?: OperationPlan; state?: OperationState }>();
const feedback = ref('');
watch(() => props.operation, () => { feedback.value = ''; });

async function copySql() {
  if (!props.operation) return;
  try {
    await navigator.clipboard.writeText(props.operation.sql.template);
    feedback.value = 'SQLテンプレートをコピーしました。';
  } catch {
    feedback.value = 'コピーできません。下のSQLを選択してコピーしてください。';
  }
}
</script>

<template>
  <section class="panel" aria-labelledby="sql-heading">
    <div class="section-top"><h2 id="sql-heading">SQL</h2></div>
    <div class="details" v-if="operation">
      <h3>{{ operationLabel(operation) }}</h3>
      <p class="muted">{{ operationPurpose[operation.kind] }}</p>
      <dl class="query-meta">
        <div><dt>発行先</dt><dd>{{ operation.target === 'source' ? 'Master' : 'Slave' }}</dd></div>
        <div><dt>対象</dt><dd>{{ operationRange(operation) ?? '—' }}</dd></div>
        <div><dt>予定件数</dt><dd>{{ formatDecimal(operation.planned_rows ?? undefined) }}</dd></div>
        <div><dt>結果</dt><dd>
          {{ operationStatus[state?.status ?? 'planned'] }}
          <template v-if="state?.affected_rows != null"> · {{ formatDecimal(state.affected_rows) }} 件</template>
          <template v-if="state?.elapsed_ms != null"> · {{ state.elapsed_ms.toFixed(1) }} ms</template>
          <small v-if="state?.completed_at" class="row-note">{{ formatTime(state.completed_at) }}</small>
          <small v-if="state?.error" class="row-note">{{ state.error.message }}</small>
        </dd></div>
      </dl>
      <div class="section-top"><span>SQLテンプレート</span><button type="button" @click="copySql">コピー</button></div>
      <pre><code>{{ operation.sql.template }}</code></pre>
      <p v-if="operation.sql.parameters.length">バインド値 <code>{{ operation.sql.parameters.join(', ') }}</code></p>
      <p v-if="feedback" role="status">{{ feedback }}</p>
    </div>
    <p class="details muted" v-else>処理を選択するとSQLを表示します。</p>
  </section>
</template>

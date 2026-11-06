<script setup lang="ts">
import type { OperationPlan } from '../types/api.ts';
import { operationLabel } from '../display.ts';

defineProps<{ operation?: OperationPlan }>();
</script>

<template>
  <section class="panel" aria-labelledby="sql-heading">
    <div class="section-top"><h2 id="sql-heading">SQL</h2></div>
    <div class="details" v-if="operation">
      <h3>{{ operationLabel(operation) }}</h3>
      <pre><code>{{ operation.sql.template }}</code></pre>
    </div>
    <p class="details muted" v-else>処理を選択するとSQLを表示します。</p>
  </section>
</template>

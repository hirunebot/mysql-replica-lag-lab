<script setup lang="ts">
defineProps<{
  status: string; canStart?: boolean; canRefresh?: boolean; starting?: boolean;
  showStartNote?: boolean; error?: string;
}>();
defineEmits<{ start: []; refresh: [] }>();
</script>

<template>
  <header class="toolbar">
    <div class="title-group">
      <h1>mysql-replica-lag-lab</h1>
      <span class="muted" role="status">{{ status }}</span>
    </div>
    <div class="toolbar-actions">
      <button v-if="canRefresh" type="button" @click="$emit('refresh')">再取得</button>
      <button type="button" class="primary" :disabled="!canStart" @click="$emit('start')">{{ starting ? '開始要求中' : '開始' }}</button>
    </div>
  </header>
  <p v-if="showStartNote" class="toolbar-note muted">開始するとSourceの lag_demo を作り直します。</p>
  <p v-if="error" class="toolbar-note warning" role="alert">{{ error }}</p>
</template>

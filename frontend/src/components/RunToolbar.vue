<script setup lang="ts">
defineProps<{ status: string; canStart?: boolean; canRefresh?: boolean }>();
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
      <button type="button" class="primary" :disabled="!canStart" @click="$emit('start')">開始</button>
    </div>
  </header>
</template>

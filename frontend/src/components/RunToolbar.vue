<script setup lang="ts">
defineProps<{ status: string; canStart?: boolean }>();
defineEmits<{ start: [] }>();
</script>

<template>
  <header class="toolbar">
    <div class="title-group">
      <h1>mysql-replica-lag-lab</h1>
      <span class="muted" role="status">{{ status }}</span>
    </div>
    <button type="button" class="primary" :disabled="!canStart" @click="$emit('start')">開始</button>
  </header>
</template>

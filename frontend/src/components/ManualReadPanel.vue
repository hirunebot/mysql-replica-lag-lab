<script setup lang="ts">
defineProps<{ enabled?: boolean }>();
defineEmits<{ read: [] }>();
</script>

<template>
  <div class="manual-read">
    <div class="read-action">
      <code>SELECT SUM(value) FROM lag_demo.marker;</code>
      <button type="button" :disabled="!enabled" @click="$emit('read')">Slaveで実行</button>
    </div>
  </div>
</template>

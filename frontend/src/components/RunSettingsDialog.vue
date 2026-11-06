<script setup lang="ts">
import { DialogClose, DialogContent, DialogDescription, DialogOverlay, DialogPortal, DialogRoot, DialogTitle, DialogTrigger } from 'reka-ui';
import { ref, watch } from 'vue';
import type { RunConfig } from '../types/api.ts';
import { formatDecimal } from '../display.ts';

const props = defineProps<{ config?: RunConfig; editable?: boolean }>();
const emit = defineEmits<{ save: [config: RunConfig] }>();
const open = ref(false);
const draft = ref<RunConfig>();
const error = ref('');
const fields = {
  demo_rows: '対象行数', seed_batch_size: 'seedバッチサイズ',
  heavy_batch_size: 'UPDATEバッチサイズ', marker_after_batches: 'marker投入位置',
} as const;

watch(open, value => {
  if (value) {
    draft.value = props.config ? { ...props.config } : undefined;
    error.value = '';
  }
});

function save() {
  if (!props.editable || !draft.value) return;
  const config = draft.value;
  if (Object.values(config).some(value => !/^[1-9][0-9]{0,19}$/.test(value)
      || BigInt(value) > 18_446_744_073_709_551_615n)) {
    error.value = '各項目に先頭ゼロのない正の整数を入力してください。';
    return;
  }
  const rows = BigInt(config.demo_rows);
  const seed = BigInt(config.seed_batch_size);
  const heavy = BigInt(config.heavy_batch_size);
  const marker = BigInt(config.marker_after_batches);
  const batches = (rows + heavy - 1n) / heavy;
  if (rows > 10_000_000n || (seed > 5000n && rows > 5000n) || batches > 1000n || marker >= batches) {
    error.value = '対象は1,000万行、seedは1回5,000行、UPDATEは1,000回までです。markerの後にUPDATEを1回以上残してください。';
    return;
  }
  emit('save', { ...config });
  open.value = false;
}
</script>

<template>
  <DialogRoot v-model:open="open">
    <DialogTrigger as-child><button type="button">設定</button></DialogTrigger>
    <DialogPortal>
      <DialogOverlay class="dialog-overlay" />
      <DialogContent class="dialog-content">
        <DialogTitle as="h2">実行設定</DialogTitle>
        <DialogDescription class="muted">
          {{ !config ? '負荷設定は未取得です。' : editable ? '次の実行の負荷を設定します。' : '現在の負荷設定です。' }}
        </DialogDescription>
        <form v-if="editable && draft" @submit.prevent="save">
          <label v-for="(label, key) in fields" :key="key" class="setting-field">
            {{ label }}
            <input v-model.trim="draft[key]" inputmode="numeric" maxlength="20" required>
          </label>
          <p v-if="error" role="alert" class="warning">{{ error }}</p>
          <div class="dialog-actions">
            <DialogClose as-child><button type="button">閉じる</button></DialogClose>
            <button type="submit" class="primary">設定を保存</button>
          </div>
        </form>
        <dl v-else-if="config" class="settings-values">
          <div v-for="(label, key) in fields" :key="key"><dt>{{ label }}</dt><dd>{{ formatDecimal(config[key]) }}</dd></div>
        </dl>
        <div v-if="!editable || !draft" class="dialog-actions">
          <DialogClose as-child><button type="button">閉じる</button></DialogClose>
        </div>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>

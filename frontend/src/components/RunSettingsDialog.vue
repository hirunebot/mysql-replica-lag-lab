<script setup lang="ts">
import { DialogClose, DialogContent, DialogDescription, DialogOverlay, DialogPortal, DialogRoot, DialogTitle, DialogTrigger } from 'reka-ui';
import type { RunConfig } from '../types/api.ts';
import { formatDecimal } from '../display.ts';

defineProps<{ config?: RunConfig }>();
</script>

<template>
  <DialogRoot>
    <DialogTrigger as-child><button type="button">設定</button></DialogTrigger>
    <DialogPortal>
      <DialogOverlay class="dialog-overlay" />
      <DialogContent class="dialog-content">
        <DialogTitle as="h2">実行設定</DialogTitle>
        <DialogDescription class="muted">{{ config ? '現在表示している実行の負荷設定です。' : '負荷設定は未取得です。' }}</DialogDescription>
        <dl v-if="config" class="settings-values">
          <div><dt>対象行数</dt><dd>{{ formatDecimal(config.demo_rows) }}</dd></div>
          <div><dt>seedバッチサイズ</dt><dd>{{ formatDecimal(config.seed_batch_size) }}</dd></div>
          <div><dt>UPDATEバッチサイズ</dt><dd>{{ formatDecimal(config.heavy_batch_size) }}</dd></div>
          <div><dt>marker投入位置</dt><dd>{{ formatDecimal(config.marker_after_batches) }}</dd></div>
        </dl>
        <div class="dialog-actions"><DialogClose as-child><button type="button">閉じる</button></DialogClose></div>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>

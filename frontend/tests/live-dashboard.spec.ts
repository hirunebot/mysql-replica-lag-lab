import { flushPromises, mount } from '@vue/test-utils';
import { defineComponent } from 'vue';
import { expect, test, vi } from 'vitest';
import fixture from '../../tests/fixtures/dashboard/snapshot.json';
import idle from '../../tests/fixtures/dashboard/idle-state.json';
import type { DashboardApi } from '../src/api/client.contract.ts';
import { useRun } from '../src/composables/useRun.ts';
import { applyRunEvent, type openRunEvents } from '../src/composables/useRunEvents.ts';
import { decodeDashboardState, decodeRunSnapshot, type Observation } from '../src/types/api.ts';
import type { EventEnvelope } from '../src/types/events.ts';
import DatabaseComparison from '../src/components/DatabaseComparison.vue';

const phase = (sequence: string, run_id = fixture.run_id): EventEnvelope => ({
  schema_version: 1, run_id, sequence, occurred_at: '2026-01-01T00:00:00Z',
  type: 'run_phase_changed', payload: { phase: 'waiting_final_sync' },
});

test('古い実行と重複を無視し、欠落は最新snapshotで復帰する', () => {
  const run = decodeRunSnapshot(fixture);
  expect(applyRunEvent(run, phase('42'))).toBe(run);
  expect(applyRunEvent(run, phase('43', 'different'))).toBe(run);
  expect(applyRunEvent(run, phase('44'))).toBeUndefined();
  const snapshot = { ...run, sequence: '44' };
  expect(applyRunEvent(run, { ...phase('44'), type: 'snapshot', payload: { snapshot } })).toBe(snapshot);
});

test('接続中断を表示し、再取得後に新しい通知を購読し直す', async () => {
  const run = decodeRunSnapshot(fixture);
  const state = { ...decodeDashboardState(idle), server_instance_id: run.server_instance_id, current_run: run };
  const api: DashboardApi = {
    getState: vi.fn().mockResolvedValue(state),
    getRun: vi.fn().mockResolvedValueOnce(run).mockResolvedValue({ ...run, sequence: '44' }),
    startRun: vi.fn(), readMarkerSum: vi.fn(),
  };
  const close = vi.fn();
  let callbacks!: Parameters<typeof openRunEvents>[1];
  const subscribe: typeof openRunEvents = vi.fn((_, handlers) => {
    callbacks = handlers;
    return { close } as unknown as EventSource;
  });
  const Harness = defineComponent({ setup() { return useRun(true, api, subscribe); }, template: '<div />' });
  const wrapper = mount(Harness);
  await flushPromises();
  callbacks.open();
  expect(wrapper.vm.connection).toBe('connected');
  callbacks.event(phase('44'));
  expect(wrapper.vm.connection).toBe('disconnected');
  expect(wrapper.vm.snapshot?.sequence).toBe('42');
  expect(close).toHaveBeenCalledOnce();
  await wrapper.vm.refresh();
  callbacks.open();
  expect(wrapper.vm.connection).toBe('connected');
  expect(wrapper.vm.snapshot?.sequence).toBe('44');
  expect(api.startRun).not.toHaveBeenCalled();
  wrapper.unmount();
  expect(close).toHaveBeenCalledTimes(2);
});

test('片側だけの失敗と前回値の時刻を表示し、実測した0を保持する', () => {
  const run = decodeRunSnapshot(fixture);
  const sample: Observation = {
    target: 'replica', metric: 'updated_rows', status: 'unavailable',
    value: { kind: 'decimal', value: '0' }, started_at: '2026-01-01T00:00:01Z',
    observed_at: '2026-01-01T00:00:00Z', elapsed_ms: 10,
    error: { code: 'db_unavailable', message: 'Unavailable', retryable: true },
  };
  const wrapper = mount(DatabaseComparison, { props: { snapshot: {
    ...run, observations: [...run.observations.filter(o => o.target !== 'replica' || o.metric !== 'updated_rows'), sample],
  } } });
  const [master, replica] = wrapper.findAll('.db');
  expect(master.text()).not.toContain('取得失敗');
  expect(replica.text()).toContain('取得失敗 · 前回値');
  expect(replica.get('.big b').text()).toBe('0');
});

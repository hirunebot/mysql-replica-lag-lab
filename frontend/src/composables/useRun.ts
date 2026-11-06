import { onMounted, onScopeDispose, ref, shallowReadonly, shallowRef } from 'vue';
import { dashboardApi, DashboardHttpError } from '../api/client.ts';
import type { DashboardApi } from '../api/client.contract.ts';
import type { DashboardState, RunSnapshot } from '../types/api.ts';
import { applyRunEvent, openRunEvents } from './useRunEvents.ts';

type Connection = 'loading' | 'connected' | 'disconnected' | 'unavailable';
const retryDelay = 2_000;

export function useRun(enabled: boolean, api: DashboardApi = dashboardApi, subscribe = openRunEvents) {
  const dashboard = shallowRef<DashboardState | null>(null);
  const snapshot = shallowRef<RunSnapshot | null>(null);
  const connection = ref<Connection>('loading');
  let stream: EventSource | undefined;
  let request: AbortController | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;

  function stop() {
    stream?.close();
    stream = undefined;
    request?.abort();
    request = undefined;
    clearTimeout(timer);
  }

  function schedule() {
    clearTimeout(timer);
    if (!disposed) timer = setTimeout(() => void refresh(false), retryDelay);
  }

  function watchRun(run: RunSnapshot, controller: AbortController) {
    function interrupted() {
      if (controller.signal.aborted || stream !== source) return;
      source.close();
      stream = undefined;
      connection.value = 'disconnected';
      schedule();
    }
    const source = subscribe(run, {
      open() {
        if (stream !== source) return;
        clearTimeout(timer);
        connection.value = 'connected';
      },
      interrupted,
      event(event) {
        if (stream !== source || !snapshot.value) return;
        const next = applyRunEvent(snapshot.value, event);
        if (!next) interrupted();
        else snapshot.value = next;
      },
    });
    stream = source;
    timer = setTimeout(interrupted, 8_000);
  }

  async function refresh(manual = true, retryReplacement = true): Promise<void> {
    if (!enabled || disposed) return;
    stop();
    if (manual) connection.value = 'loading';
    const controller = new AbortController();
    request = controller;
    try {
      const state = await api.getState(controller.signal);
      if (controller.signal.aborted) return;
      if (snapshot.value?.server_instance_id !== state.server_instance_id
          || snapshot.value?.run_id !== state.current_run?.run_id) snapshot.value = null;
      dashboard.value = state;
      if (state.current_run) {
        const run = await api.getRun(state.current_run.run_id, controller.signal);
        if (controller.signal.aborted) return;
        if (run.run_id !== state.current_run.run_id || run.server_instance_id !== state.server_instance_id) {
          throw new Error('Inconsistent run response');
        }
        snapshot.value = run;
        watchRun(run, controller);
      } else {
        connection.value = 'connected';
        schedule();
      }
    } catch (error) {
      if (controller.signal.aborted) return;
      if (retryReplacement && error instanceof DashboardHttpError && error.status === 404) {
        await refresh(false, false);
        return;
      }
      connection.value = dashboard.value ? 'disconnected' : 'unavailable';
      schedule();
    }
  }

  if (enabled) onMounted(() => void refresh());
  onScopeDispose(() => { disposed = true; stop(); });
  return {
    dashboard: shallowReadonly(dashboard), snapshot: shallowReadonly(snapshot),
    connection: shallowReadonly(connection), refresh,
  };
}

import { computed, onMounted, onScopeDispose, ref, shallowReadonly, shallowRef } from 'vue';
import { dashboardApi, DashboardHttpError } from '../api/client.ts';
import type { DashboardApi } from '../api/client.contract.ts';
import type { DashboardState, ManualRead, RunConfig, RunSnapshot } from '../types/api.ts';
import { applyRunEvent, mergeRead, openRunEvents } from './useRunEvents.ts';

type Connection = 'loading' | 'connected' | 'disconnected' | 'unavailable';
const retryDelay = 2_000;

export function useRun(enabled: boolean, api: DashboardApi = dashboardApi, subscribe = openRunEvents) {
  const dashboard = shallowRef<DashboardState | null>(null);
  const snapshot = shallowRef<RunSnapshot | null>(null);
  const connection = ref<Connection>('loading');
  const starting = ref(false);
  const reading = ref(false);
  const startError = ref('');
  const readError = ref('');
  const responses = shallowRef<ManualRead[]>([]);
  const hidden = shallowRef(new Set<string>());
  const allReads = computed(() => responses.value.reduce(mergeRead, snapshot.value?.manual_reads ?? []));
  const manualReads = computed(() => allReads.value.filter(read => !hidden.value.has(read.read_id)));
  const active = computed(() => snapshot.value !== null && !['completed', 'failed'].includes(snapshot.value.phase));
  const pendingRead = computed(() => reading.value || allReads.value.some(read => read.status === 'pending'));
  const canConfigure = computed(() => !!dashboard.value && connection.value === 'connected'
    && !active.value && !starting.value && !pendingRead.value);
  const canStart = computed(() => canConfigure.value
    && ['source', 'replica'].every(target => dashboard.value?.connections
      .some(item => item.target === target && item.status === 'available')));
  const canRead = computed(() => {
    const run = snapshot.value;
    return !!run && !pendingRead.value && !starting.value && connection.value === 'connected'
      && (['running', 'waiting_final_sync', 'completed'].includes(run.phase)
        || (run.phase === 'failed' && run.observations.some(item =>
          item.target === 'replica' && item.metric === 'marker' && item.observed_at !== null)));
  });
  let stream: EventSource | undefined;
  let request: AbortController | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;
  let startRequest: AbortController | undefined;
  let readRequest: AbortController | undefined;

  function adopt(run: RunSnapshot | null) {
    if (snapshot.value?.run_id !== run?.run_id || snapshot.value?.server_instance_id !== run?.server_instance_id) {
      readRequest?.abort();
      reading.value = false;
      readError.value = '';
      responses.value = [];
      hidden.value = new Set();
    }
    snapshot.value = run;
  }

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
        else adopt(next);
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
          || snapshot.value?.run_id !== state.current_run?.run_id) adopt(null);
      dashboard.value = state;
      if (state.current_run) {
        const run = await api.getRun(state.current_run.run_id, controller.signal);
        if (controller.signal.aborted) return;
        if (run.run_id !== state.current_run.run_id || run.server_instance_id !== state.server_instance_id) {
          throw new Error('Inconsistent run response');
        }
        adopt(run);
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

  async function start(config: RunConfig) {
    if (!canStart.value) return;
    const previousId = snapshot.value?.run_id;
    const controller = new AbortController();
    startRequest = controller;
    starting.value = true;
    startError.value = '';
    try {
      const run = await api.startRun(config, controller.signal);
      if (controller.signal.aborted) return;
      if (dashboard.value?.server_instance_id !== run.server_instance_id
          || (snapshot.value?.run_id !== previousId && snapshot.value?.run_id !== run.run_id)) {
        await refresh();
      } else if (snapshot.value?.run_id !== run.run_id) {
        stop();
        adopt(run);
        connection.value = 'loading';
        request = new AbortController();
        watchRun(run, request);
      }
    } catch (error) {
      if (controller.signal.aborted) return;
      startError.value = error instanceof DashboardHttpError && error.status < 500
        ? error.message : '開始要求の結果を再取得しています。';
      await refresh();
    } finally {
      if (startRequest === controller) starting.value = false;
    }
  }

  async function read() {
    const id = snapshot.value?.run_id;
    if (!id || !canRead.value) return;
    const controller = new AbortController();
    readRequest = controller;
    reading.value = true;
    readError.value = '';
    try {
      const result = await api.readMarkerSum(id, controller.signal);
      if (!controller.signal.aborted && snapshot.value?.run_id === id && result.run_id === id) {
        responses.value = mergeRead(responses.value, result);
      }
    } catch (error) {
      if (controller.signal.aborted) return;
      readError.value = error instanceof DashboardHttpError ? error.message : '手動SELECTの結果を再取得しています。';
      if (!(error instanceof DashboardHttpError)) await refresh();
    } finally {
      if (readRequest === controller) reading.value = false;
    }
  }

  function clearReads() {
    hidden.value = new Set([...hidden.value, ...allReads.value.map(read => read.read_id)]);
  }

  if (enabled) onMounted(() => void refresh());
  onScopeDispose(() => { disposed = true; stop(); startRequest?.abort(); readRequest?.abort(); });
  return {
    dashboard: shallowReadonly(dashboard), snapshot: shallowReadonly(snapshot),
    connection: shallowReadonly(connection), refresh, start, read, clearReads,
    canStart, canConfigure, canRead, manualReads, pendingRead,
    starting: shallowReadonly(starting), startError: shallowReadonly(startError), readError: shallowReadonly(readError),
  };
}

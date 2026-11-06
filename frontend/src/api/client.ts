import type { DashboardApi } from './client.contract.ts';
import { decodeDashboardState, decodeRunSnapshot, type ApiError } from '../types/api.ts';
import { readApiError, readManualRead } from '../types/validation.ts';

export class DashboardHttpError extends Error {
  constructor(readonly status: number, readonly detail: ApiError | null) {
    super(detail?.message ?? `HTTP ${status}`);
  }
}

async function request<T>(
  path: string, status: number, decode: (json: unknown) => T, signal?: AbortSignal, body?: unknown,
): Promise<T> {
  const response = await fetch(path, {
    signal: AbortSignal.any(signal ? [signal, AbortSignal.timeout(8_000)] : [AbortSignal.timeout(8_000)]),
    cache: 'no-store',
    ...(body === undefined ? {} : {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body),
    }),
  });
  if (response.status !== status) {
    let detail: ApiError | null = null;
    try { detail = readApiError(await response.json()); } catch { /* JSON以外のHTTPエラーも保持する。 */ }
    throw new DashboardHttpError(response.status, detail);
  }
  return decode(await response.json());
}

const runPath = (id: string) => `/api/runs/${encodeURIComponent(id)}`;

export const dashboardApi: DashboardApi = {
  getState: signal => request('/api/state', 200, decodeDashboardState, signal),
  getRun: (id, signal) => request(runPath(id), 200, decodeRunSnapshot, signal),
  startRun: (config, signal) => request('/api/runs', 202, decodeRunSnapshot, signal, config),
  readMarkerSum: (id, signal) => request(
    `${runPath(id)}/reads`, 200, json => readManualRead(json, id), signal, { preset: 'marker_sum' },
  ),
};

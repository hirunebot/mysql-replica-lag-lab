/** ダッシュボードが使うHTTP API。 */
import type { DashboardState, ManualRead, RunConfig, RunSnapshot } from '../types/api.ts';

export interface DashboardApi {
  /** GET /api/state: current run, safe defaults, and per-DB connectivity. */
  getState(signal?: AbortSignal): Promise<DashboardState>;
  /** POST /api/runs: reserve execution and return its initial snapshot (202). */
  startRun(config: RunConfig, signal?: AbortSignal): Promise<RunSnapshot>;
  /** GET /api/runs/{id}: recover state without starting another run. */
  getRun(runId: string, signal?: AbortSignal): Promise<RunSnapshot>;
  /** POST /api/runs/{id}/reads: fresh fixed SELECT on Replica, not cached state. */
  readMarkerSum(runId: string, signal?: AbortSignal): Promise<ManualRead>;
}

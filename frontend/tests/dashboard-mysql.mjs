// 専用のローカルDBを停止・再起動し、lag_demoを作り直す受入テスト。
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { chromium, expect } from '@playwright/test';

if (process.env.DASHBOARD_MYSQL !== '1') {
  throw new Error('専用のローカルDBでのみ DASHBOARD_MYSQL=1 npm run test:mysql を実行してください。');
}
const url = new URL(process.env.DASHBOARD_URL ?? 'http://127.0.0.1:3000/');
assert.equal(url.protocol, 'http:');
assert.ok(['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname));
const root = fileURLToPath(new URL('../../', import.meta.url));
const runCommand = (command, args) => execFileSync(command, args, { cwd: root, encoding: 'utf8', timeout: 120_000 });
const compose = (...args) => runCommand('docker', ['compose', ...args]);
const mysql = (target, sql) => compose(
  'exec', '-T', '--env', 'MYSQL_PWD=root', target, 'mysql', '--user=root', '--batch', '--skip-column-names', '-e', sql,
).trim();
const startDb = () => compose('up', '-d', '--wait', 'source', 'replica');
const readDb = service => mysql(service, `SELECT CONCAT(
  (SELECT value FROM lag_demo.marker WHERE id=1), ':',
  (SELECT COUNT(*) FROM lag_demo.items), ':',
  (SELECT COUNT(*) FROM lag_demo.items WHERE version=2));`);

const browser = await chromium.launch();
const desktop = await browser.newPage();
const mobile = await browser.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true });
const observer = await mobile.newPage();
const status = page => page.locator('.toolbar [role=status]');
const runButton = page => page.getByRole('button', { name: '開始', exact: true });
const history = page => page.getByRole('table', { name: '手動SELECTの履歴' });
const state = async () => {
  const response = await desktop.request.get(new URL('/api/state', url).href);
  assert.equal(response.status(), 200);
  return response.json();
};
const snapshot = async id => {
  const response = await desktop.request.get(new URL(`/api/runs/${id}`, url).href);
  assert.equal(response.status(), 200);
  return response.json();
};
async function start() {
  const [response] = await Promise.all([
    desktop.waitForResponse(response => new URL(response.url()).pathname === '/api/runs'
      && response.request().method() === 'POST'),
    runButton(desktop).click(),
  ]);
  assert.equal(response.status(), 202);
  return response.json();
}
async function manualRead(page) {
  const [response] = await Promise.all([
    page.waitForResponse(response => new URL(response.url()).pathname.endsWith('/reads')
      && response.request().method() === 'POST'),
    page.getByRole('button', { name: 'Slaveで実行', exact: true }).click(),
  ]);
  assert.equal(response.status(), 200);
  return response.json();
}

let containersStopped = false;
let sqlThreadStopped = false;
try {
  const idle = await state();
  assert.equal(idle.current_run, null, '新しく起動したダッシュボードを使ってください');
  assert.deepEqual(idle.connections.map(c => c.display_address).sort(), ['127.0.0.1:3307', '127.0.0.1:3308']);
  containersStopped = true;
  compose('stop', 'replica', 'source');
  await desktop.goto(url.href);
  await expect(desktop.getByText('DB接続不可', { exact: true })).toHaveCount(2, { timeout: 15_000 });
  await expect(status(desktop)).toHaveText('実行前');
  assert.deepEqual(await desktop.locator('.big b').allTextContents(), ['—', '—']);
  assert.deepEqual(await desktop.locator('.marker strong').allTextContents(), ['—', '—']);
  await expect(runButton(desktop)).toBeDisabled();
  console.log('OK DB停止中は件数・marker未取得');
  const unavailable = await desktop.request.post(new URL('/api/runs', url).href, { data: idle.default_config });
  assert.equal(unavailable.status(), 202);
  const unavailableRun = await unavailable.json();
  await expect.poll(async () => (await snapshot(unavailableRun.run_id)).error?.code, { timeout: 20_000 })
    .toBe('db_unavailable');
  assert.ok((await snapshot(unavailableRun.run_id)).operations.every(operation => operation.status === 'planned'));
  console.log('OK DB停止中の直接開始は書き込み前に失敗');

  startDb();
  containersStopped = false;
  runCommand('./scripts/configure-replication.sh', []);
  await desktop.getByRole('button', { name: '再取得', exact: true }).click();
  const run = await start();
  assert.deepEqual(run.config, idle.default_config);
  const rows = run.config.demo_rows;
  console.log(`START サーバーの既定設定 ${rows}行`);

  await expect(status(desktop)).toHaveText('実行中', { timeout: 610_000 });
  sqlThreadStopped = true;
  mysql('replica', 'STOP REPLICA SQL_THREAD');
  await expect(desktop.locator('.db').first().locator('.marker strong')).toHaveText('1', { timeout: 610_000 });
  const paused = await snapshot(run.run_id);
  assert.equal(paused.observations.find(item => item.target === 'replica' && item.metric === 'marker').value.value, '0');
  const committedMarker = paused.operations.find(item => item.operation_id === 'marker');
  assert.equal(committedMarker.status, 'committed');
  const marker = paused.plan.find(item => item.operation_id === 'marker');
  const predecessor = paused.operations.find(item => item.operation_id === marker.after_committed[0]);
  assert.equal(predecessor.status, 'committed');
  assert.ok(predecessor.completed_at <= committedMarker.started_at);
  const seed = paused.plan.find(item => item.operation_id === 'seed');
  assert.equal(seed.last_id, rows);
  assert.ok(seed.sql.template.includes(`(${rows}, 1, REPEAT`));
  await desktop.getByRole('button', { name: 'UPDATE 1', exact: true }).click();
  await expect(desktop.locator('pre')).toContainText('WHERE id BETWEEN ? AND ?');
  await expect(desktop.getByText('バインド値')).toContainText(`1, ${run.config.heavy_batch_size}`);
  const early = await manualRead(desktop);
  assert.equal(early.value, '0');

  await observer.goto(url.href);
  await expect(status(observer)).toHaveText('実行中', { timeout: 15_000 });
  await observer.reload();
  await expect(status(observer)).toHaveText('実行中', { timeout: 15_000 });
  await expect(history(observer).getByText('0', { exact: true })).toBeVisible();
  await mobile.setOffline(true);
  await observer.getByRole('button', { name: '再取得', exact: true }).click();
  await expect(status(observer)).toHaveText('接続中断');
  await expect(observer.getByText(/前回値/).first()).toBeVisible();
  await mobile.setOffline(false);
  await expect(status(observer)).toHaveText('実行中', { timeout: 15_000 });
  const duplicate = await desktop.request.post(new URL('/api/runs', url).href, { data: run.config });
  assert.equal(duplicate.status(), 409);
  assert.equal((await state()).current_run.run_id, run.run_id);
  console.log('OK 2タブ・再読込・通信断復帰・二重開始拒否・手動SELECT=0');
  mysql('replica', 'START REPLICA SQL_THREAD');
  sqlThreadStopped = false;

  await expect(status(desktop)).toHaveText('同期完了', { timeout: 610_000 });
  await expect(status(observer)).toHaveText('同期完了', { timeout: 15_000 });
  await expect(desktop.getByRole('button', { name: 'UPDATE 1', exact: true })).toHaveAttribute('aria-pressed', 'true');
  const late = await manualRead(desktop);
  assert.equal(late.value, '1');
  assert.notEqual(early.read_id, late.read_id);
  await expect(history(desktop).locator('tbody strong')).toHaveText(['1', '0']);
  const historyRows = history(desktop).locator('tbody tr');
  await expect(historyRows.nth(0).locator('time')).toHaveAttribute('datetime', late.started_at);
  await expect(historyRows.nth(1).locator('time')).toHaveAttribute('datetime', early.started_at);
  for (const service of ['source', 'replica']) {
    assert.equal(readDb(service), `1:${rows}:${rows}`);
  }
  const formattedRows = new Intl.NumberFormat('ja-JP').format(BigInt(rows));
  await expect(desktop.locator('.big b')).toHaveText([formattedRows, formattedRows]);
  await expect(desktop.locator('.marker strong')).toHaveText(['1', '1']);
  await desktop.getByRole('button', { name: '表示をクリア' }).click();
  await expect(history(desktop)).toHaveCount(0);
  await desktop.reload();
  await expect(history(desktop).locator('tbody strong')).toHaveText(['1', '0']);
  console.log('OK 同期完了と両DBの直接照合・履歴保持と再読込');

  await expect(status(desktop)).toHaveText('同期完了');
  sqlThreadStopped = true;
  mysql('replica', 'STOP REPLICA SQL_THREAD');
  const rejected = await start();
  await expect(status(desktop)).toHaveText('実行失敗', { timeout: 15_000 });
  const failed = await snapshot(rejected.run_id);
  assert.equal(failed.error.code, 'replication_not_ready');
  assert.ok(failed.operations.every(operation => operation.status === 'planned'));
  assert.equal(readDb('source'), `1:${rows}:${rows}`);
  for (const service of ['source', 'replica']) {
    assert.equal(mysql(service, `SELECT COUNT(*) FROM performance_schema.processlist
      WHERE COMMAND='Query' AND ID<>CONNECTION_ID() AND INFO LIKE '%lag_demo%'`), '0');
  }
  mysql('replica', 'START REPLICA SQL_THREAD');
  sqlThreadStopped = false;
  await desktop.getByRole('button', { name: '設定', exact: true }).click();
  for (const [label, value] of [
    ['対象行数', '12000'], ['UPDATEバッチサイズ', '4000'], ['marker投入位置', '1'],
  ]) await desktop.getByLabel(label).fill(value);
  await desktop.getByRole('button', { name: '設定を保存' }).click();
  const next = await start();
  await expect(status(desktop)).toHaveText('同期完了', { timeout: 120_000 });
  assert.notEqual(next.run_id, run.run_id);
  assert.deepEqual((await snapshot(next.run_id)).manual_reads, []);
  await expect(history(desktop)).toHaveCount(0);
  assert.equal(readDb('replica'), '1:12000:12000');
  console.log('OK 適用停止時に書き込みなし・残存照会なし・次runに履歴が混入しない');
} finally {
  try {
    if (containersStopped) startDb();
    if (sqlThreadStopped) mysql('replica', 'START REPLICA SQL_THREAD');
  } finally {
    await browser.close();
  }
}

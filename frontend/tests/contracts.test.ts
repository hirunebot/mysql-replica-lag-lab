/** 承認用の先行テスト。承認前のため、検証関数の本体は未実装。
 * Node 24以上で実行：node --test frontend/tests/contracts.test.ts
 * ネイティブの型除去ではテストを実行できるが、TypeScriptの型チェックは行われない。
 */
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { decodeDashboardState, decodeEventEnvelope, decodeRunSnapshot } from '../src/types/api.ts';

const fixture = (name: string): any => JSON.parse(readFileSync(
  new URL(`../../tests/fixtures/dashboard/${name}.json`, import.meta.url), 'utf8',
));
const snapshot = (): any => fixture('snapshot');

function expectRejected(input: unknown): void {
  // 先に正常例を読み込ませ、常に例外を投げるスタブが合格しないようにする。
  decodeRunSnapshot(snapshot());
  assert.throws(() => decodeRunSnapshot(input));
}

test('Rustと共有する正常なスナップショットを読み込み、MasterとSlaveの値を区別する', () => {
  const input = snapshot();
  const result = decodeRunSnapshot(input);
  assert.deepEqual(result, input);
  assert.notEqual(result.observations[0].value, result.observations[1].value);
});

test('JavaScriptの安全な整数範囲を超える値とsequenceを精度を失わず保持する', () => {
  const input = snapshot();
  input.sequence = '9007199254740993';
  input.observations[0].value.value = '18446744073709551615';
  assert.equal(decodeRunSnapshot(input).sequence, input.sequence);
  assert.deepEqual(decodeRunSnapshot(input).observations[0].value, input.observations[0].value);
});

test('未取得・値0・テーブル未作成・一部の観測失敗を区別する', () => {
  const input = snapshot();
  for (const observation of Object.values(fixture('observations'))) {
    input.observations = [observation];
    assert.deepEqual(decodeRunSnapshot(input).observations[0], observation);
  }
});

test('次の読み取りが失敗しても、前回成功時の値と観測時刻を保持する', () => {
  const input = snapshot();
  input.observations = [fixture('observations').unavailable_with_previous_value];
  const result = decodeRunSnapshot(input).observations[0];
  assert.equal(result.status, 'unavailable');
  assert.deepEqual(result.value, { kind: 'decimal', value: '0' });
  assert.equal(result.observed_at, input.observations[0].observed_at);
});

test('手動SELECTのSQL NULL・取得中・失敗を区別する', () => {
  for (const read of Object.values(fixture('manual-reads'))) {
    const input = snapshot();
    input.manual_reads = [read];
    assert.deepEqual(decodeRunSnapshot(input).manual_reads[0], read);
  }
});

test('数値型や不正な10進文字列を暗黙変換せず拒否する', () => {
  for (const value of [200000, '-1', '1.5', '01', '1e3', '']) {
    const input = snapshot();
    input.config.demo_rows = value;
    expectRejected(input);
  }
});

test('未対応のスキーマバージョン・必須項目の欠落・認証情報の項目を拒否する', () => {
  const version = snapshot(); version.schema_version = 2; expectRejected(version);
  const missing = snapshot(); delete missing.run_id; expectRejected(missing);
  const secret = snapshot(); secret.source_database_url = 'mysql://private'; expectRejected(secret);
});

test('不正な日時・UTC以外の時刻・非有限または負の所要時間を拒否する', () => {
  for (const value of ['not-a-date', '2026-02-30T09:00:00Z', '2026-11-01T18:00:00+09:00']) {
    const input = snapshot(); input.observations[0].observed_at = value; expectRejected(input);
  }
  for (const value of [-1, Infinity, NaN]) {
    const input = snapshot(); input.observations[0].elapsed_ms = value; expectRejected(input);
  }
});

test('取得成功なのに値または成功時刻がない観測を拒否する', () => {
  const missingValue = snapshot(); missingValue.observations[0].value = null; expectRejected(missingValue);
  const missingTime = snapshot(); missingTime.observations[0].observed_at = null; expectRejected(missingTime);
});

test('別の実行やMasterに紐づく手動SELECT結果を拒否する', () => {
  for (const change of [{run_id:'different-run'}, {target:'source'}]) {
    const input = snapshot();
    input.manual_reads = [{...fixture('manual-reads').zero, ...change}];
    expectRejected(input);
  }
});

test('失敗した手動SELECTに成功値が付いている場合は拒否する', () => {
  const input = snapshot();
  input.manual_reads = [{...fixture('manual-reads').failure, value:'0'}];
  expectRejected(input);
});

test('コミット済みなのに完了記録がない処理を拒否する', () => {
  const input = snapshot(); input.operations[0].completed_at = null; expectRejected(input);
});

test('共有するSSEのJSON例をpayloadとsequenceを失わず読み込む', () => {
  for (const event of fixture('events')) assert.deepEqual(decodeEventEnvelope(event), event);
});

test('未対応のSSE種別と手動SELECTイベントの状態の不整合を拒否する', () => {
  const events = fixture('events');
  decodeEventEnvelope(events[0]); // 常に拒否するだけの実装が合格しないようにする。
  assert.throws(() => decodeEventEnvelope({...events[0], type:'invented_event'}));
  const wrongStatus = structuredClone(events[2]);
  wrongStatus.payload.read.status = 'pending';
  assert.throws(() => decodeEventEnvelope(wrongStatus));
});

test('markerの一致だけで全行更新の確認がない完了状態を拒否する', () => {
  const input = snapshot();
  input.phase = 'completed';
  input.observations[1].value.value = '1';
  expectRejected(input);
});

test('観測項目と値の種類が一致しない場合は拒否する', () => {
  const input = snapshot();
  input.observations[0].metric = 'replication';
  expectRejected(input);
});


test('実行前にDBが利用できなくても、存在しない実行を完了扱いにしない', () => {
  const input = fixture('idle-state');
  const result = decodeDashboardState(input);
  assert.equal(result.current_run, null);
  assert.equal(result.connections[1].status, 'unavailable');
  assert.deepEqual(result, input);
});

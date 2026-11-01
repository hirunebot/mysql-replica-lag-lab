# Dashboard API・状態モデル：ステップ1 承認用

## 状態と承認対象

**契約・テストを先に作成した段階。処理本体は未実装。**

[詳細実装手順](10-frontend-implementation-steps.md)の「契約・テスト → 承認 → 実装」に従う。今回確認していただくのはデータ形式、状態遷移、検証の期待値。承認後に実装する範囲は、DTOの検証と状態遷移の判定だけとする。HTTPサーバー、SQL実行、runner、SSE送信、Vueの画面は後続ステップで別途テスト・シグネチャを提示する。

| ファイル | 今回の役割 |
| --- | --- |
| `src/contracts.rs` | Rustの公開DTO、検証エラー、検証・状態遷移の未実装シグネチャ |
| `src/lib.rs` | DTOを結合テストから参照するための公開境界 |
| `tests/dashboard_contracts.rs` | Rust側の契約・シリアライズ・状態遷移テスト |
| `tests/fixtures/dashboard/` | RustとTypeScriptで共有するJSON例 |
| `frontend/src/types/api.ts` | TypeScript型と未実装decoder |
| `frontend/src/api/client.contract.ts` | HTTPクライアントの宣言のみ。通信処理は未実装 |
| `frontend/tests/contracts.test.ts` | JSONからの型検証・異常値拒否のテスト |
| `Cargo.toml` | DTO用Serde、テスト用serde_jsonの依存宣言 |

既存CLIのmain・SQL・デモの実行手順は変更していない。新しい関数は明示的な未実装エラーを返すため、呼び出しても仮の成功結果は得られない。

## 1. 画面とAPIの語彙

画面のMasterはAPIの `source`、Slaveは `replica`。SQLの更新対象と、読み取りで反映を確認した対象を混同しない。

- 行数・ID・marker・SUM・イベントsequence：符号なし10進文字列。`"0"` を許可し、`"01"`、`"1.5"`、`"1e3"`、負数、JSON数値を許可しない。設定の0禁止や実行上限の検証はステップ3で行う。
- 時刻：実在する日時を表すUTCのRFC 3339、末尾は `Z`。表示時だけユーザーのタイムゾーンへ変換する。
- 所要時間・遅延秒数：有限で0以上の数値。未取得・不明はnull。
- 接続先表示：ホストとポートのみ。認証情報を含むURLやSQLxの生エラーを送らない。
- `schema_version`：初期値1。対応していないバージョンは拒否し、表示を続けない。

## 2. HTTP契約

APIは以下の予定。現在のコードにはHTTPハンドラーを追加していない。

| エンドポイント | 入力 | 正常応答 |
| --- | --- | --- |
| `GET /api/state` | なし | 200 / DashboardState |
| `POST /api/runs` | RunConfigをJSON bodyで直接送信 | 202 / RunSnapshot |
| `GET /api/runs/{id}` | run_id | 200 / RunSnapshot |
| `POST /api/runs/{id}/reads` | `{"preset":"marker_sum"}` | 200 / 完了したManualRead |
| `GET /api/runs/{id}/events` | Last-Event-IDまたはafter | 200 / SSE |

エラー応答は `{ "code": "...", "message": "...", "retryable": false }`。不正要求400、未知run404、実行競合・not_ready409、DB利用不可503、手動照会タイムアウト504を基本とする。

DashboardStateはserver_instance_id、default_config、current_run、connectionsを返す。実行前はcurrent_run=null。DB停止や未確認を、存在しないrunの「完了」として返さない。現在runがある場合、そのsequenceとDB別の取得状態はRunSnapshotに含む。

RunSnapshotは設定、実行計画、各処理の実績、DB別の観測、手動SELECT履歴を含む。各処理はoperation_id、手動照会はread_idで対応付ける。

## 3. 予定と実績

OperationPlanは対象、SQLテンプレートとバインド値、ID範囲、予定件数、コミット済みであるべき依存operation_idを持つ。配列順だけから厳密な実行順を保証しない。例えばmarkerと3番目のバッチは、2番目のコミット後に並行実行できる。

OperationStateは `planned / running / committed / failed / outcome_unknown`。committedには完了時刻・実績件数・所要時間が必要。SQLを送った後に接続が失われた場合は、結果不明を未反映と扱わない。

Replicaの反映状態は別フィールド `unknown / pending / observed_applied / unavailable`。SourceがcommittedでもReplicaを自動的にobserved_appliedにしない。確認済みとする場合は観測時刻を持つ。

## 4. DB観測とNULLの区別

Observationはtarget、metric、statusと、型付きvalueを持つ。行数・markerは `{ "kind": "decimal", "value": "0" }`。レプリケーション状態はkind=replicationとし、receiver / applier、遅延秒数、ログのファイル名と位置をまとめる。

| status | value | 意味 |
| --- | --- | --- |
| not_observed | null | まだ照会していない |
| not_ready | null | 初期構築中でテーブルが利用できない |
| observing | nullまたは前回値 | 最新の照会を実行中 |
| available | 実測値 | 今回の取得が成功 |
| unavailable | nullまたは前回値 | 最新の取得が失敗 |

observed_atは「保持している値の取得に成功した時刻」。取得失敗後も値を保持するなら、その古い成功時刻を残す。started_atとelapsed_msは最新の照会を表すため、observed_atがstarted_atより古い場合がある。成功値がない失敗と、古い値を保持する失敗を区別する。

片側のDBだけ失敗しても、もう片側の成功値を消さない。Seconds_Behind_Sourceのnullは遅延0秒へ変換しない。複数のログ位置から行数を算出しない。

## 5. 手動SELECT

手動照会はReplica、preset=marker_sum、SQL=`SELECT SUM(value) FROM lag_demo.marker;` に固定する。現行markerは1行なのでSUMはそのvalueと一致する。

| status | value | completed_at / elapsed_ms | error |
| --- | --- | --- | --- |
| pending | null | null | null |
| succeeded | 10進文字列またはnull | 必須 | null |
| failed | null | 必須 | 必須 |

succeededのnullだけがSQL NULLを表す。pendingやfailedのnullをSQL NULLとして表示しない。run_idが現在のrunと異なる履歴、Sourceへの手動照会、失敗に成功値を付けた応答は拒否する。

APIとSSEの結果は同じread_idで統合する。直近5件の履歴は取得時の値を保持し、自動監視や後の照会で上書きしない。履歴保持そのものの実装・テストはステップ5・10で扱う。

## 6. 状態遷移

```text
idle → preparing → waiting_initial_sync → running → waiting_final_sync → completed
            └────────────────────────────┴───────────────┘
                                ↓
                              failing → failed
```

正確な許可遷移は次の表とする。異なるrunの作成は新しい状態機械として扱い、同一runをfailedからpreparingへ戻さない。

| 現在 | 次に許可する状態 |
| --- | --- |
| idle | preparing |
| preparing | waiting_initial_sync、failing |
| waiting_initial_sync | running、failing |
| running | waiting_final_sync、failing |
| waiting_final_sync | completed、failing |
| failing | failed |
| completed / failed | なし |

同じ状態のイベントは遷移判定ではfalseとし、イベントの重複排除は後続のSSE処理が担う。running→completedの飛び越しと、後始末前のrunning→failedを認めない。

遷移判定関数は遷移の形だけを確認する。実DBの最終確認はrunnerが担当する。RunSnapshotの検証でも、completedなのに更新が未完了、反映件数の根拠がない、両markerが不一致、レプリケーションが不健全、といった矛盾したデータは拒否する。

## 7. 承認後に実装する関数

```rust
pub fn validate_snapshot(snapshot: &RunSnapshot) -> Result<(), ContractError>;
pub fn validate_event(event: &EventEnvelope) -> Result<(), ContractError>;
pub fn can_transition(from: RunPhase, to: RunPhase) -> bool;
```

- validate_snapshot：公開フィールドの形式と相互関係を確認する。DBへは接続しない。
- validate_event：version、識別子、sequence、時刻、イベント種別とpayloadの整合を確認する。
- can_transition：上の表に従う純粋な判定。
- ContractError：codeとフィールドpathを持つ。内容はInvalidDecimal、InvalidTimestamp、InvalidDuration、InconsistentObservation、InconsistentRead、InconsistentOperation、InconsistentRun、UnsupportedSchemaVersion。

```typescript
export function decodeDashboardState(input: unknown): DashboardState;
export function decodeRunSnapshot(input: unknown): RunSnapshot;
export function decodeEventEnvelope(input: unknown): EventEnvelope;
```

JSONを検証して対応する型を返す。不正値は例外として拒否し、数値への変換や欠落フィールドへの仮値補完をしない。イベントの未対応typeを黙って受け入れない。HTTPクライアントのDashboardApiインターフェースは将来の呼び出し側の契約だけであり、今回実装する通信処理はない。

## 8. テスト・fixture

共有JSONは以下の5ファイル。サンプルID・時刻・SQLは契約テスト用で、実DBの観測結果ではない。fixtureのUPDATEは短縮したSQL例であり、後続のrunnerでは既存の実SQLを共有する。

| fixture | 内容 |
| --- | --- |
| snapshot.json | SourceとReplicaでmarkerが異なる実行中の状態、4バッチとmarkerの依存条件 |
| observations.json | 未取得、テーブル未作成、0、取得失敗、前回値保持、不明な遅延秒数 |
| manual-reads.json | 成功0・成功1・SQL NULL・取得中・失敗 |
| events.json | 状態変更、DB観測、手動SELECT完了のSSE envelope |
| idle-state.json | runなし、片側DBが利用不可の初期状態 |

先行テストでは、正常な共有fixture、整数精度、非正規な10進表現、日時・所要時間、version、欠落フィールド、認証情報フィールド、観測種別と値の不一致、成功記録の欠落、別runの照会、完了条件の欠落、未対応イベント種別を確認する。

TypeScript側の拒否テストは、先に有効な応答を読めることを確認する。すべての入力に例外を投げる未実装スタブが「不正値を拒否できた」として合格しないようにする。

### 検証コマンド

```bash
# Node 24以上。依存追加なしで実行できる承認段階のテスト
node --test frontend/tests/contracts.test.ts

# Rust toolchainがある環境で実行
cargo test --test dashboard_contracts
cargo test --bin mysql-replica-lag-lab
cargo fmt --check
```

NodeのネイティブTypeScript実行は型を除去するだけで、型チェックではない。Vue / Vitest / vue-tscの導入はステップ7に残す。承認後のDTO検証実装時にはTypeScriptコンパイラによる型チェックも実施する。

Rustの依存宣言にはSerdeとserde_jsonを追加しているが、この環境にはcargoがなく、Cargo.lockの再生成とRustテストは未実施。既存lockfileにも両パッケージは存在するが、直接依存への変更を含め、承認後の検証でCargoに整合させる。未実施を成功とは扱わない。

### 今回の実行結果

- TypeScript：17件実行、17件失敗。すべて承認待ちスタブの `Not implemented` による想定した失敗。合格・skipは0件。
- Rust：未実行。cargo / rustc / rustfmtが利用できないため、コンパイル・テスト・書式の確認は残っている。
- JSON fixture：5ファイルとも構文解析可能であることを確認。
- 処理本体・HTTP通信・DB操作・Vue画面：今回の差分では実装していない。

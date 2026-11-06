# フロントエンド 詳細実装手順

## 1. 位置付け

[基本計画](09-frontend-implementation-plan.md)を、実装・レビュー可能な単位に分解する。画面の基準は [HTMLワイヤーフレーム](wireframes/index.html)。本書のAPI、ファイル、コマンドは実装予定であり、動作確認済みの機能ではない。

基本計画と本書で詳細が異なる場合は本書を優先する。各ステップは、先にテストと関数シグネチャを提示し、ユーザーの明示的な承認後に処理本体を実装する。本文の上限・間隔は初期値とし、実機検証で変更した場合は理由を記録する。

### 全ステップ共通の進め方：契約・テスト → 承認 → 実装

ユーザーが作業内容を把握してから実装を判断できるよう、ステップごとに以下の順序で進める。計画全体の了承だけを、各ステップの処理本体の実装承認とは扱わない。

| 段階 | 作成・実施する内容 | 次へ進む条件 |
| --- | --- | --- |
| A. 仕様と変更範囲 | 対象ファイル、責務、入力・出力、エラー、既存機能への影響を整理 | テストと公開境界を記述できる |
| B. テストとシグネチャを先行作成 | 期待動作を検証するテスト、型、関数シグネチャ、API契約、Vueのprops / emitsをファイルに書く | レビュー可能な差分がある |
| C. 承認待ち | 差分へのリンク、テスト一覧、期待結果、未実装箇所、現在の検証結果を提示 | ユーザーが当該範囲を明示的に承認 |
| D. 処理本体の実装 | 承認された契約・テストを満たす処理を実装 | 対象テストと必要な回帰確認が成功 |
| E. 結果報告 | 変更・実行した検証・制限を報告 | 次ステップのAへ進める |

承認前に作成するのは、契約を表す型・シグネチャ・コメント、テストと必要なfixtureまでとする。DB操作、runner、イベント処理、UIの処理本体は書かない。実装が必要な箇所は明示的な未実装スタブにし、仮の成功値を返してテストを通さない。

未実装でテストが失敗する場合は、その失敗を想定どおりの状態として記録する。テスト基盤が未導入で実行できない場合も未実行と明記する。未実装を理由にテストを無条件に成功させたり、skipを合格として報告したりしない。

テストは外部から見える動作・境界・異常系を検証し、内部の実装手順をそのまま写すものにしない。型と関数だけでは表現できない画面配置・ホバー・キーボード操作は、コンポーネントテストやブラウザの受入ケースとして先に記述する。

テストのDescriptionと説明コメントは日本語で書く。TypeScriptのtest / describe等の表示名は日本語にし、Rustでは英語の関数名に日本語の説明コメントを付ける。この方針は後続ステップで追加するテストにも適用する。

承認依頼には少なくとも以下を含める。

- 今回実装するステップと範囲、対象ファイルへのリンク。
- 関数シグネチャと責務、引数・返り値・エラーの意味。
- 作成したテスト名、入力条件、期待結果。
- 実装後に行う検証と、現時点の未実装・未実行箇所。

承認待ちの間は、承認対象の処理本体や、それを前提とする後続実装に進まない。契約や期待動作の変更が必要になった場合はテストとシグネチャを修正し、変更範囲を再提示して承認を得る。承認済み範囲内での内部実装や通常の不具合修正には追加の承認工程を増やさない。

## 2. 確定する仕様

- Vue 3 + TypeScript + Vite。HTTPサーバーはAxum。実行エンジンは既存Rustコードを共有する。
- Master / Slaveの反映件数とmarkerを上段に、処理一覧と選択したSQLを下段に表示する。
- 対象行数・バッチサイズ・marker投入位置は「処理の流れ」に置く。
- キャッチコピー、サイドバー、説明用の経路図、独立したstale readの通知帯は置かない。
- バッチの目的・ID範囲・件数・SQL・実行結果はホバーと選択で確認できる。
- Slaveへ手動で `SELECT SUM(value) FROM lag_demo.marker;` を発行できる。結果は自動更新せず、時刻付きで直近5件残す。
- DB起動とレプリケーション設定は既存Shellの事前操作。画面の「開始」はRustのデモを起動し、ShellやE2Eスクリプトを呼ばない。
- 実画面には「開始」を置く。ワイヤーフレームの「再生」「場面選択」「一時停止」はサンプル専用であり移植しない。
- 「表示をクリア」は手動SELECTの表示履歴を消す操作とする。DBの削除・巻き戻しとは区別する。新規実行は既存デモ同様に `lag_demo` を再作成する。
- 実行中止、実行の一時停止、PHPエンジンの画面操作、自由入力SQL、過去実行の永続保存は初期版に含めない。

## 3. ステップと依存関係

| ステップ | 成果物 | 前提 |
| --- | --- | --- |
| 1 | 型・API・状態遷移の契約 | 本書 |
| 2 | 共通runnerとCLI互換 | 1 |
| 3 | 実行管理とHTTP API | 2 |
| 4 | 実DBの観測と最終同期判定 | 3 |
| 5 | Slaveの手動SELECT API | 3 |
| 6 | SSEと切断復帰 | 3〜5 |
| 7 | Vueプロジェクトと画面骨格 | 1 |
| 8 | API接続とDB比較表示 | 4、6、7 |
| 9 | 処理一覧・SQL詳細 | 2、6、7 |
| 10 | 手動SELECTと開始操作の接続 | 5、8、9 |
| 11 | ビルド・配信・起動手順 | 8〜10 |
| 12 | 実DBでの受入検証 | 11 |

各ステップの差分は「契約・テスト」と「承認後の実装」を区別してレビュー可能にする。コミットを作成する場合もこの境界で分ける。コミット・プッシュは実装時の依頼範囲に従う。

### 先行提示する契約の例

以下は説明用の候補であり、確定した実装ではない。実装着手時に型を含めた実ファイルとテストを作成し、承認対象とする。

```rust
// 入力検証と実行枠の予約。成功後のrun_idで進捗を追跡する。
pub async fn start_run(&self, config: RunConfig) -> Result<RunSnapshot, StartRunError>;

// キャッシュではなく、対象runのSlaveへ固定SELECTを発行する。
pub async fn read_marker_sum(&self, run_id: RunId) -> Result<ManualRead, ReadError>;

// queryの開始・結果と、実測値による状態遷移を通知する。
pub async fn run(config: Config, reporter: Reporter) -> Result<RunOutcome, RunError>;
```

```typescript
// 応答の検証とHTTPエラー処理をAPIクライアントへ集約する。
export function startRun(config: RunConfig): Promise<RunSnapshot>;
export function readMarkerSum(runId: string): Promise<ManualRead>;

// 古いrunや重複sequenceのイベントを適用しない。
export function applyRunEvent(
  snapshot: RunSnapshot,
  event: EventEnvelope,
): RunSnapshot;
```

例えば手動SELECTでは、0・1・NULL・テーブル未作成・タイムアウト・新規runとの競合のテストを先に作る。フロントエンドでは、取得中の二重送信防止、read_idによる重複排除、自動監視による過去結果の上書き防止を先にテスト化する。

## ステップ1：通信契約と状態モデルを固定する

契約・テストと検証結果は [Dashboard API・状態モデル](11-dashboard-api.md) を参照。ステップ1のDTO検証・状態遷移は実装済み。共通runner以降は未実装で、各ステップの契約・テストを提示してから進める。

### 作業

`src/contracts.rs` にSerialize可能な公開DTO、`frontend/src/types/api.ts` に対応するTypeScript型を定義する。Rust内部のConfigや接続URLをそのまま返さない。APIの具体的な型は `docs/11-dashboard-api.md` として実装時に記録する。

| モデル | 主な項目 |
| --- | --- |
| RunConfig | demo_rows、seed_batch_size、heavy_batch_size、marker_after_batches |
| RunSnapshot | server_instance_id、run_id、sequence、phase、config、plan、operations、observations、manual_reads、stale_read_observed、error |
| OperationPlan | operation_id、kind、target、batch_number、first_id、last_id、planned_rows、SQLテンプレート、バインド値、依存条件 |
| OperationState | planned / running / committed / failed / outcome_unknown、開始・完了時刻、elapsed_ms、affected_rows |
| Observation | target、metric、value、status、started_at、observed_at、elapsed_ms、error |
| ManualRead | read_id、run_id、target、SQL、status、value、started_at、completed_at、elapsed_ms、error |
| EventEnvelope | schema_version、run_id、sequence、occurred_at、type、payload |

全体状態は `idle → preparing → waiting_initial_sync → running → waiting_final_sync → completed`。途中の失敗では `failing` で後始末し、終了確認後に `failed` とする。marker一致だけでcompletedへ進まない。server起動直後のidleは以前のデモ完了を意味しない。

SQLの開始・結果、DBの観測、実行状態は別のモデルにする。Replicaのバッチ別状態は `unknown / pending / observed_applied / unavailable` とし、Sourceの成功だけで適用済みにしない。

API上の行数・ID・marker値・SUMは10進文字列に統一し、JavaScriptの整数精度の欠落を避ける。秒数・所要時間は有限のnumber、SUMのSQL NULLはJSON nullとする。時刻はUTCのRFC 3339で返し、UIではローカル時刻とタイムゾーンを付ける。

### 完了条件・検証

- API正常例、値未取得、SQL NULL、観測失敗、部分取得失敗のJSON例を用意する。
- Rustのシリアライズ結果をTypeScript側の契約検証で読み取れる。
- 公開モデルにDBのパスワードや接続URLが含まれない。

## ステップ2：Rustの実行処理を共有する

[共通runnerの契約と検証](13-runner-contract.md)を参照。CLIは共通runnerを呼び出し、従来の起動方法と出力を維持する。

### 変更対象

`src/main.rs`、`src/lib.rs`、`src/runner.rs`、`src/workload.rs`、`src/setup.rs`、`src/monitor.rs`、`src/events.rs`、`src/queries.rs`、`Cargo.toml`。

### 作業

1. mainのデモ手順を `runner::run(config, reporter)` に抽出する。CLIはConfigの読み込み、表示用reporter、終了コードを担当する。
2. `batch_range` と設定検証をライブラリから利用できる範囲で公開する。接続設定と画面から指定できる負荷設定を分離する。
3. SQLテンプレートとパラメータを実行側で定義し、計画と通知から同じ定義を参照する。画面表示用に別のUPDATEを手書きしない。
4. setup・seed・各UPDATE・markerの開始と結果を通知する。既存CLIのE2Eが参照するメッセージはCLI reporterで維持する。
5. markerは「指定バッチ数がコミット済み」という依存条件を持たせる。後続バッチとは並行するため、予定一覧を厳密な全順序として扱わず、実際の開始・完了時刻を記録する。
6. seedは一覧上1つの処理へ集約し、投入済み件数を更新する。詳細では実際のINSERTバッチのID範囲とSQLを確認できるようにする。payloadを全行分通知しない。
7. バックエンドがUPDATEの成功応答を受けた場合だけcommittedとする。通信断による結果不明は `outcome_unknown` とし、失敗＝未反映と解釈しない。
8. ワーカーの所有者をrunnerへ集約する。エラー後は新しいバッチの投入を止め、既に投入したSQLと子タスクを終了確認してから実行枠を解放する。ブラウザ切断ではrunnerを止めない。
9. CargoにCLIの `default-run` を設定し、HTTPバイナリ追加後も `cargo run --release` の意味を保つ。

### 完了条件・検証

- 既存の単体テストとRust CLIのE2Eが成功する。
- 端数バッチ、markerの投入条件、UPDATE失敗前後の通知が正しい。
- markerの処理失敗時にも後続ワーカーを放置しない。
- 既存SQLの意味・データ量・コミット単位を変更しない。

## ステップ3：Axumと実行管理を追加する

開始・状態取得の3つのAPIを実装済み。[起動と検証](14-dashboard-http.md)を参照。SSEと手動SELECTは後続ステップ。

### 変更対象

`src/bin/server.rs`、`src/api/`、`src/run_manager.rs`、`Cargo.toml`。

### 作業

- AxumとID生成の依存を追加し、既存のSerdeと時刻処理を使う。SSEの配信処理はステップ6で追加する。
- バインド先は既定 `127.0.0.1:3000`。DB設定は既存環境変数から取得する。
- HTTP受付はDB停止時にも起動し、stateに接続不可を返せるようにする。DBエラーでWebサーバー全体を終了させない。
- 最新runとスナップショットをメモリ保持する。run_idは再起動・再実行で衝突しない値とする。
- 開始要求の検証後、排他枠の予約を原子的に行い、preparingのrunを登録して202を返す。接続・レプリケーション確認はrunner側のpreflightで行い、失敗はrunの結果に残す。
- 不正設定は400、既に実行中は409、未知runは404。エラー本文は `{ code, message, retryable }` に揃える。
- 画面の初期上限は1,000万行・UPDATE計画1,000バッチ。既存の0禁止・接続先重複禁止に加え、Web実行ではmarker後のバッチを最低1つ要求する。CLIの既存仕様は維持する。
- クライアントにDB接続先や任意SQL、Shellコマンドを指定させない。
- 終了確認ができない実行は実行枠を保持し、再開始できる状態と表示しない。サーバー再起動前後のDB作業の停止確認を利用手順に記載する。

| API | 応答・動作 |
| --- | --- |
| `GET /api/state` | サーバーID、既定の公開設定、最新run、DB到達状態 |
| `POST /api/runs` | 負荷設定を検証して202、run_idと初期スナップショット |
| `GET /api/runs/{id}` | 最新スナップショットとsequence |

### 完了条件・検証

- 同時の開始要求のうち1つだけが202となる。
- DB停止・レプリケーション未設定がrunの失敗として表示可能。
- 不正設定ではDROP DATABASEやUPDATEを発行しない。
- HTTPリクエストの終了やブラウザ切断後もrunが継続する。

## ステップ4：実DBの観測を独立した処理へ整理する

Web実行の独立観測と同期判定を実装済み。[観測間隔・接続上限・検証](14-dashboard-http.md#web実行のdb観測)を参照。

### 変更対象

`src/observer.rs`、`src/database.rs`、`src/setup.rs`、`src/monitor.rs`、`src/run_manager.rs`。

### 作業

1. 1つのrunに1つの観測サービスを置き、クライアント数に関係なく同じ結果を配信する。既存の同期待ちと画面監視の重複照会を避ける。
2. 書き込み・自動監視・手動SELECTの接続プールを分離する。全体の接続数を明示し、実装時に各プールの上限を決める。
3. markerとレプリケーション状態は既定500ms、全体COUNTは既定2秒。前回の照会が未完了なら重ねて実行しない。
4. 各照会の初期タイムアウトを5秒とする。SELECTを実際に停止するDB側の制限と、SQLx側の待ち時間制限を組み合わせる。タイムアウトした処理をバックグラウンドで無制限に残さない。
5. 毎回新しい読み取りスナップショットを取得し、長いトランザクションを保持しない。
6. 観測成功時は実値と取得時刻を保存する。失敗時は前回値の時刻を残してunavailableにし、0へ置き換えない。開始前はnot_observed、初期構築中のテーブル未作成はnot_readyとする。
7. 全体COUNTは `version = 2`。Sourceのrows_affected累計は別項目で保持し、DB観測値の代わりに進捗バーへ代入しない。
8. バッチ別のSlave反映は、Sourceでコミット済みのID範囲に対して `WHERE id BETWEEN ? AND ? AND version = 2` のCOUNTで確認する。2秒ごとに最古の未確認バッチを最大1件照会する。全件更新を実測できた場合は、その観測時刻で全バッチを反映確認済みにできる。
9. marker一致後も観測を継続する。全行更新、両DBのmarker一致、Sourceワーカーの正常終了、receiver / applierの正常稼働を確認してcompletedとする。
10. 自動観測でstale readが見えなかった場合も正常同期を完了可能にする。`stale_read_observed=false` を保持し、成功した同期と観測できたかどうかを分離する。

最終判定で許可する観測値は当該runの最新の成功値とし、古いrunや取得失敗中の前回値で完了させない。終了時は観測タスクを回収する。

### 完了条件・検証

- 片側DBの取得失敗、markerの未作成、NULLの遅延値を区別できる。
- Sourceコミット済み／Slave未反映を実DBで確認できる。
- バッチ内の処理中件数やbinlog位置差を行数として表示しない。
- COUNTが5秒以上掛かっても照会が積み重ならない。

## ステップ5：手動SELECT APIを実装する

固定SELECTと最新5件のサーバー履歴を実装済み。[手動SELECTのAPI](14-dashboard-http.md#手動select)を参照。

### 変更対象

`src/api/reads.rs`、`src/database.rs`、`src/run_manager.rs`。

### 作業

- `POST /api/runs/{id}/reads` は固定クエリ `marker_sum` のみ受け付ける。
- 自動監視のキャッシュを返さず、Slaveの読み取り専用接続でその場の `SELECT SUM(value) FROM lag_demo.marker;` を実行する。
- SUMはMySQLではDECIMALになり得るため、SQLxのDECIMAL対応を利用するか明示的に文字列へ変換し、値を損失なく返す。空集合のNULLは保持する。
- run_idに加えread_idを付け、開始・完了・結果・所要時間を保存する。API応答とSSE通知に同じread_idを使う。
- runごとの手動照会は同時1件。処理中は409、対象runの初期構築中は409のnot_ready、DB利用不可は503、照会タイムアウトは504。未知runは404。
- runningだけでなく、初期同期後と最終同期待ち、completedでも利用可能にする。failedは終了処理が終わり対象テーブルが利用可能なら照会を許可する。
- 新規実行によるschema再作成と照会を競合させない。開始予約は進行中の手動照会と排他し、完了までは409を返す。古いrunへのSELECT要求は新しいDB状態を読ませず409とする。
- 成功・失敗を含む直近5件をサーバー側に保持し、ページ再読込でも現在runの履歴を取得できるようにする。
- 履歴の表示クリアはフロントエンドだけの操作とし、クリア時点までのread_idを非表示にする。ページ再読込でサーバー側の直近履歴が戻る仕様を明記する。

### 完了条件・検証

- 同期前の0、同期後の1、NULL、テーブル未作成、DB切断を区別できる。
- クリック時の実値を返し、自動監視の更新で過去の値が変わらない。
- 手動照会と次runの開始を並行させても結果のrunが混ざらない。

## ステップ6：SSEとスナップショット復帰を実装する

SSEの配信、履歴からの再送、最新スナップショットへの復帰を実装済み。[接続方法と上限](14-dashboard-http.md#sse)を参照。

### 変更対象

`src/events.rs`、`src/api/events.rs`、`src/run_manager.rs`。

### 作業

1. runごとに単調増加するsequenceを採番し、状態変更とイベント履歴追加を同じ排他区間で行う。ネットワーク送信はロック外で行う。
2. `GET /api/runs/{id}/events` でSSEを配信する。event IDはrun_idとsequenceを含め、15秒ごとにkeep-aliveを送る。
3. 初回はスナップショットとsequenceを返し、その後のイベントだけを送る。取得と購読の間のイベントも履歴から補う。
4. 自動再接続ではLast-Event-ID、新しいEventSourceを作る場合は検証済みのafterパラメータを受け付ける。両方がある場合の優先順位をAPI文書で固定する。
5. 履歴は1runあたり最大2,000イベント、合計4MiBを初期上限とする。両方の条件で古いものを削除し、SQL payloadもサイズを制限する。
6. 要求位置が履歴外なら `snapshot` イベントで最新状態へ置き換える。遅い受信側も同様に復帰させ、無制限にバッファを増やさない。
7. queryの開始・結果は同じoperation_idで対応付ける。自動監視SELECTはUIでは種別ごとに集約し、イベントによって無限に行を追加しない。
8. サーバー再起動はserver_instance_idの変更として識別し、以前のrunが完了したと推測しない。

### 完了条件・検証

- 同じsequenceの再受信で二重反映しない。
- スナップショット取得直後のイベントを取りこぼさない。
- 履歴外・通信断・サーバー再起動から状態を再取得できる。
- 複数タブでもDB監視回数が増えない。

## ステップ7：Vueの画面骨格を作る

`frontend/` で `npm ci` と `npm run dev` を実行する。通常画面は未取得状態を表示し、開発時のみ `/?fixture=1` でサンプルを選べる。ブラウザ検証は `npx playwright install chromium` の後に `npm run test:browser`。

### ファイル構成

```text
frontend/
  package.json / package-lock.json
  index.html / vite.config.ts / tsconfig*.json
  src/
    main.ts / App.vue
    styles.css
    types/api.ts
    api/client.ts
    composables/useRun.ts
    composables/useRunEvents.ts
    components/
      RunToolbar.vue
      DatabaseComparison.vue
      DatabaseProgress.vue
      OperationTimeline.vue
      QueryDetail.vue
      ManualReadPanel.vue
      RunSettingsDialog.vue
```

### 作業

- npmとlockfileに統一し、実装時点のNode LTSとViteの要求を満たすバージョンを `.node-version` に固定する。
- Vue、TypeScript、Vite、vue-tsc、Vitest、Vue Test Utils、Reka UIを追加する。ブラウザ検証にPlaywrightを使う。
- ワイヤーフレームの白地・罫線・控えめな色・情報配置を移植する。キャッチコピー等を再導入しない。
- 初期は明示的なfixtureで画面を組み、実API接続後は通常起動でfixtureへ自動フォールバックしない。
- PCでは比較を2列、処理とSQLを2列にする。狭い幅では縦に並べ、テーブルだけ必要に応じて横スクロールする。

### 完了条件・検証

- TypeScriptのstrictモード、`vue-tsc --noEmit`、`vite build` が通る。
- 390pxと1440pxで操作でき、ページ全体が横にはみ出さない。
- 数値未取得の初期画面を0件や同期済みとして表示しない。

## ステップ8：APIと画面状態を接続する

### 作業

- `api/client.ts` に型検証、HTTPエラーの読み取り、要求キャンセル処理を集約する。
- useRunを画面内で1回だけ作り、readonly stateと操作関数を各コンポーネントへ渡す。コンポーネントごとのSSE接続を作らない。
- 初回state取得→現在runのスナップショット→SSEの順に接続する。未取得や不明なイベント型は成功状態に変換しない。
- 重複sequenceを無視し、run_id違いを適用しない。イベント欠落時はスナップショットで復帰する。
- DB比較へ実測件数・marker・時刻を渡す。取得中、失敗、前回値を見分けられる表示にする。
- SSE切断中は小さな「接続中断」表示を出し、古い値をそのまま最新と見せない。再接続でrunを開始し直さない。
- 自動再接続と手動の再取得を区別し、画面破棄時にEventSource・タイマーを解放する。

### 完了条件・検証

- 片側だけ古い値、片側だけ失敗、SSEの重複・切断・復帰をfixtureイベントで検証する。
- API利用不可のときサンプル成功画面へ切り替わらない。

## ステップ9：処理一覧とSQL詳細を接続する

### 作業

- サーバーが生成した計画を一覧にする。ID範囲・件数はクライアントで別計算しない。
- Sourceの実行状態とSlaveの観測結果を別列で表示する。
- 未確認のSlave状態は「未確認」、新しい観測がない場合は取得時刻付きの前回状態とする。
- 行選択をoperation_idで管理し、SSE更新や再描画でも選択を維持する。
- 長時間ホバー（初期550ms）とフォーカスで目的を表示し、クリック・タッチでSQL詳細を固定する。
- SQLのテンプレートとバインド値を確認できるようにする。SQLはテキストとして描画し、HTMLとして挿入しない。
- クリップボード利用不可時はSQL選択によるコピーを案内する。
- tooltipはEscapeで閉じる。設定ダイアログは閉じたとき操作元へフォーカスを戻す。固定SQLパネルへ勝手にフォーカスを移さない。

### 完了条件・検証

- 実行と表示のSQL・ID範囲・更新件数が一致する。
- markerと後続UPDATEが並行しても状態を誤って直列表示しない。
- マウス、キーボード、タッチで同じ詳細へ到達できる。

## ステップ10：手動SELECT・開始操作を接続する

### 作業

1. Slave側に固定SELECTと「Slaveで実行」を置き、POST /readsへ接続する。
2. 取得中は同じボタンを無効にし、成功値・NULL・失敗を別々に表示する。
3. API応答とSSEイベントはread_idで統合する。二重登録せず、開始時刻の新しい順に直近5件を表示する。
4. 自動観測によるmarker変化では履歴のvalueを変更しない。新runの応答と古いSELECTの応答が前後しても混ぜない。
5. 開始操作は設定値を送信し、202が返るまでは開始済みとしない。要求結果が不明な通信断では自動でPOSTし直さず、stateを取得する。
6. 設定画面に行数・seedサイズ・UPDATEサイズ・marker位置を置く。開始前の画面内にDB再作成を明示し、追加の確認ダイアログは挟まない。
7. 実行中は設定変更と再開始を無効にする。手動SELECTはAPIが許可する段階で有効にし、初期構築中は無効にする。
8. 完了後の再開始、表示クリア、ページ再読込時の履歴復帰を接続する。

### 完了条件・検証

- 再生中相当のUPDATEを止めずに手動SELECTを実行できる。
- 0を読んだ履歴が、後で1を読んでも残る。
- 連打、APIとSSEの到着順逆転、run変更直後の応答を正しく扱う。

## ステップ11：静的配信と起動方法を整える

### 作業

- Viteの `/api` を `http://127.0.0.1:3000` へプロキシする。
- Axumから `frontend/dist` を配信する。未知の `/api` をindex.htmlへフォールバックさせない。
- ビルド成果物がなくてもAPIは起動でき、画面アクセスにはビルド手順を返す。
- 起動時に実際のURLを表示する。実機検証はWSL内のHTTPとWindowsブラウザからのlocalhost接続の両方で行う。
- `.gitignore` にnode_modules、dist、ブラウザのテスト成果物を追加する。
- README、利用方法、トラブルシューティングへ手順とNode要件、CLI / PHPとの同時実行禁止を追記する。

予定する開発手順：

```bash
docker compose up -d --wait
./scripts/configure-replication.sh
cargo run --release --bin server
# 別ターミナル
cd frontend
npm ci
npm run dev
```

予定する単一サーバーでの配信手順：

```bash
cd frontend
npm ci
npm run build
cd ..
cargo run --release --bin server
```

### 完了条件・検証

- 開発プロキシとAxumの静的配信の両方で、開始・SSE・手動SELECTが動く。
- ローカル専用バインドと既存のDB接続ポートを維持する。
- DB停止や画面ビルド未実施でも、対処に必要なエラーを取得できる。

## ステップ12：実DBで検証して記録する

### 自動検証

| 層 | 重点ケース |
| --- | --- |
| Rust | 計画境界、状態遷移、ワーカー終了、開始排他、SUMのNULL / DECIMAL |
| HTTP / SSE | 不正要求、二重開始、手動照会との競合、スナップショットとイベントの境界、履歴欠落 |
| Vue | run違い・重複イベント、前回値の鮮度、手動履歴保持、通信断後の復帰 |
| ブラウザ | 開始→バッチ選択→SQL→手動SELECT→完了、再読込、キーボード、モバイル幅 |
| 既存機能 | Rust / PHP単体・静的検証、既存E2E |

想定チェック：

```bash
./scripts/test-unit.sh
cd frontend
npm ci
npm run typecheck
npm run test -- --run
npm run build
cd ..
./scripts/test-e2e.sh all
```

ブラウザのfixture検証と実DB検証を分離する。fixtureで0→1を保証するテストと、実DBの観測成功を混同しない。

### 実DBの受入シナリオ

1. DB未起動でWebを開き、偽の件数や同期完了が出ないことを確認する。
2. DBを起動・構成し、標準設定で開始する。
3. seed、各UPDATE、markerの実行順とSQLを確認する。
4. Sourceのmarker更新後、Slaveが追いつく前に手動SELECTを試す。自動テストでは対象段階を待って呼び出す。
5. 追いついた後に再度SELECTし、過去の0と新しい1が時刻付きで残ることを確認する。
6. 全件同期後に両DBへ直接SQLを実行し、画面の件数・markerを照合する。
7. 実行中のページ再読込・複数タブ・一時的な通信断で、デモが二重起動しないことを確認する。
8. DB停止・レプリケーション停止で失敗を確認し、残存ワーカーと照会を点検する。
9. 次runで前runの結果が混入しないことを確認する。

stale readの発生は負荷と観測タイミングに依存する。未観測の場合は未観測として記録し、行数やReplicaのCPU制限を調整して再試行する。手動の照会タイミングだけに実DB検証の合否を依存させない。

### 観測負荷の確認

同じ設定でCLI基準とWeb監視ありをそれぞれ3回実行し、Sourceの大量更新時間、marker反映時間、全件同期時間、COUNT所要時間を記録する。バッチ別COUNTを含めた追加照会の影響を比較し、間隔変更が必要なら理由を残す。

結果は実装時に `docs/12-dashboard-validation.md` へ、環境・設定・実測値・未確認事項とともに保存する。

## 4. 最終的な完了条件

- [ ] 各ステップでテストとシグネチャを先に提示し、承認後に処理本体を実装している。
- [ ] 承認済みワイヤーフレームの情報量と配置を維持している。
- [ ] Master / Slaveの反映件数とmarkerが実DBの観測値である。
- [ ] 各バッチの目的・範囲・件数・発行先・SQL・結果を確認できる。
- [ ] 任意のタイミングでSlaveへ手動SELECTでき、結果履歴を保持できる。
- [ ] DB再作成は開始操作に対応し、Shellの起動・構成とは分離されている。
- [ ] 二重開始、古いrunの応答、切断復帰、取得失敗を扱える。
- [ ] marker一致後も全件同期まで観測が継続する。
- [ ] 架空のstale readや常設の説明帯を表示しない。
- [ ] CLI / PHPの既存テスト、Webの型チェック・ビルド・検証が通る。
- [ ] WSLでの起動・閲覧手順と実DB検証結果が記録されている。

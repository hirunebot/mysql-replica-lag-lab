# ステップ2：共通runnerの契約と検証

実装済み。`cargo run --release` から共通runnerを呼び出します。

## 実行の境界

- [runner](../src/runner.rs) に事前検証から最終同期までを移す。CLIは環境変数の読み込み・表示・終了コードを担当し、既存の出力と `cargo run --release` を維持する。
- [SQL定義](../src/queries.rs) を実行・予定・通知で共用する。更新内容とコミット単位は維持し、seedは表示上1操作にまとめる。
- [Reporter](../src/events.rs) は既存の `EventPayload` で開始・結果・DB観測を通知する。seedの実SQL・ID範囲と累積件数は同じ操作IDの通知にまとめる。成功応答後だけコミット済みとし、結果不明の通信断は `outcome_unknown` とする。
- markerは指定バッチのコミット後に投入し、後続バッチとは並行できる。失敗時は新規バッチを止め、実行中のSQLとワーカーの終了確認後にエラーを返す。通知先の切断だけでは中止しない。

HTTP・SSE配信・Vue・複数実行の排他は後続ステップ。

## テスト

[runner契約テスト7件](../tests/runner_contracts.rs)を含むRust 38件が成功。TypeScript契約20件、Rustの書式・Clippyも成功。

- `cargo test --locked --test runner_mysql -- --ignored`：ローカルMySQLでmarker更新を失敗させ、失敗後の新規バッチがなく、runner終了後に更新が残らないことを確認。正常時の通知と最終状態も契約検証に通した。
- `scripts/test-e2e.sh rust`：既定の20万行で成功。Sourceが1、Replicaが0のmarkerを観測し、監視開始から31.05秒でReplicaも1になった。両DBで全20万行が更新済みで、レプリケーションの正常稼働も確認した。stale read未観測でも同期できれば、通常のCLIは警告付きで完了する。

実DBテストはローカルデモ専用で、`lag_demo` を作り直す。他のデモ実行と並行して走らせない。

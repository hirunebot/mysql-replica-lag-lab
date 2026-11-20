# ダッシュボードの検証記録

## 環境

2026-11-21、Ubuntu 22.04 WSL2、MySQL 8.4.9、Rust 1.98.0、PHP 8.4.27、Node.js 24.22.2、Playwright Chromium。Sourceは3307、Replicaは3308。Composeの設定に合わせ、CPU上限をSource 2、Replica 0.25、Replicaの適用workerを1に設定した。

Docker Desktopのエンジンに接続できなかったため、実DB検証にはWSL上の専用MySQL 2プロセスを使用した。既存スクリプトの起動・停止・SQL実行は一時的なローカルラッパーで両プロセスに接続した。Composeの構成検証はDockerのCLIで実施した。

## 再現手順

```bash
./scripts/test-unit.sh
(cd frontend && npm ci && npm run typecheck && npm run test -- --run && npm run build)
(cd frontend && npx playwright install chromium && npm run test:browser)
```

以下は専用のローカルDBで順番に実行する。デモ用DBを作り直す。ブラウザの受入テストはMySQLコンテナとReplicaの適用処理を一時停止し、復帰してから次へ進む。標準設定を検証する場合は負荷の環境変数を既定に戻す。

```bash
# 別ターミナルで実行し、URLが表示されたことを確認する
cargo run --release --bin server

# 別ターミナルで、サーバー起動直後に実行
(cd frontend && DASHBOARD_MYSQL=1 npm run test:mysql)

# ブラウザの受入テスト終了後、サーバーを停止してから順番に実行
./scripts/test-e2e.sh all
cargo test --locked --test runner_mysql -- --ignored
cargo test --release --locked --lib compare_cli_and_dashboard_observation -- --ignored --nocapture
```

Viteプロキシを検証する場合は、Rustサーバーを新しく起動し、`frontend` で `npm run dev` を実行する。受入テストは `DASHBOARD_URL=http://127.0.0.1:5173/ DASHBOARD_MYSQL=1 npm run test:mysql` で起動する。

## 検証結果

| 検証 | 結果 |
| --- | --- |
| Rust / PHP / Shell / Compose | Rust 53件、PHP 8件が成功。フォーマット、Clippy、構文、Compose構成も確認 |
| Web | 型チェック、契約20件、Vue 7件、ビルドが成功。Chromiumでデスクトップとモバイル幅の計8件が成功 |
| Axum静的配信から実DBを操作 | 標準設定の100万行で受入テストが成功。両DBの直接SQLでmarker `1`、全体100万件、更新済み100万件を確認 |
| Vite開発プロキシから実DBを操作 | 12万行、UPDATE 6,000行、5バッチ後にmarker投入する設定で、同じ受入テストが成功 |
| Rust / PHP CLIのE2E | 各20万行、4バッチで成功。両方でstale read、後続UPDATE、SourceとReplicaの最終同期を確認 |
| Runnerの実DB結合テスト | markerのSQLを失敗させ、ワーカー終了後に書き込みが続かないことを確認。続く正常runでSQL、通知、最終同期を確認 |

実DBの受入テストでは、初期同期後にReplicaのSQL適用を一時停止し、手動SELECTで `0` を確認した。再開後は `1` を取得し、両方の値と時刻が履歴に残った。複数タブ、再読込、通信断からの復帰、409による二重開始拒否、DB停止・適用停止時の開始失敗、次runへの履歴の混入防止も確認した。

今回の未確認項目は、Docker Composeでのコンテナ起動とWindows版Chromeからの閲覧。

## 観測負荷の測り方

4万行、seed 1,000行、UPDATE 5,000行、2バッチ後にmarker投入。観測間隔は両モードとも500msで、WebのCOUNTは2秒。各モードを3回ずつ実行する。Web側は実際のWeb Runnerの観測処理を使う。比較対象はDB観測で、ブラウザやSSEの転送時間は計測に含めない。

大量更新はSourceの先頭UPDATE開始から最後の完了まで、markerはSourceのコミット通知からReplicaで1を初めて実測するまで、最終同期は最終同期フェーズへの移行から実行完了まで。COUNTの中央値・最大は両DBの成功した全体件数・更新済み件数の照会を集計し、失敗・準備中は別に数える。バッチ別COUNTはWebの実行負荷に含まれる。

| モード | 回 | 大量更新 (秒) | marker反映 (秒) | 最終同期 (秒) | COUNT成功 | COUNT中央値 (ms) | COUNT最大 (ms) | 失敗・準備中 | stale read |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| CLI | 1 | 10.861 | 2.009 | 0.519 | 9 | 3.016 | 7.567 | 0 | 観測 |
| Web | 1 | 0.587 | 0.367 | 1.417 | 10 | 3.752 | 6.980 | 2 | 未観測 |
| Web | 2 | 6.973 | 0.357 | 5.031 | 30 | 8.237 | 24.925 | 2 | 未観測 |
| CLI | 2 | 0.631 | 0.502 | 0.520 | 8 | 3.943 | 9.412 | 0 | 観測 |
| CLI | 3 | 0.613 | 0.503 | 0.524 | 8 | 2.766 | 16.981 | 0 | 観測 |
| Web | 3 | 0.517 | 0.327 | 1.485 | 78 | 0.667 | 4322.112 | 6 | 未観測 |

全6回で最終同期を確認した。最終同期フェーズの中央値はCLIで0.520秒、Webで1.485秒だった。大量更新時間とCOUNTの最大値には試行間のばらつきがあり、この3回ずつの結果から観測処理の影響量は確定できない。観測間隔は既定値のままとした。

4万行でのWeb計測では、3回ともstale readは未観測だった。受入テストではReplicaの適用を一時停止し、手動SELECTが古い値を返す場面を確認した。

# ステップ3：HTTP API

実DBを使う場合は先に [MySQLを起動・設定](02-usage.md) してから `cargo run --bin server` を実行する。DB停止中でもAPIは起動できる。既定は `127.0.0.1:3000`。変更する場合は `DASHBOARD_BIND_ADDR` にループバックのIP・ポートを指定する。

| 操作 | API | 応答 |
| --- | --- | --- |
| 現在の状態 | `GET /api/state` | 200 / `DashboardState`。DBに到達できなくても `unavailable` を返す |
| 開始 | `POST /api/runs` | 202 / `preparing` の `RunSnapshot`。不正設定は400、実行枠が使用中なら409 |
| 実行の取得 | `GET /api/runs/{id}` | 200 / 最新の `RunSnapshot`。知らないIDは404 |

```sh
curl -sS http://127.0.0.1:3000/api/runs -H 'Content-Type: application/json' \
  -d '{"demo_rows":"200000","seed_batch_size":"1000","heavy_batch_size":"50000","marker_after_batches":"2"}'
```

受け付けるのは4つの負荷設定だけ。最大1,000万行・1,000 UPDATEバッチで、markerの後に最低1バッチを残す。接続先はサーバーの既存環境変数から読み込む。開始後のDB接続失敗やレプリケーション未設定は、その実行の `error` に記録する。

最新の1実行だけをメモリに保持し、HTTP切断で止めない。実行終了が不明なら `failing` と `completion_unconfirmed` を返し、次の開始を拒否する。再起動で履歴と排他枠は失われるため、停止・再起動の前後にはローカルDBの `SHOW FULL PROCESSLIST` と `SHOW REPLICA STATUS` で前のデモ処理が残っていないことを確認する。再起動でSQLは巻き戻らない。

開始は `lag_demo` を作り直す。CLI版や別プロセスのサーバーとは同時に実行しない。手動SELECTとSSEは未実装。

検証：`cargo test --locked`。HTTPテストで同時開始・TCP切断・DB停止・終了不明を確認した。実MySQLではHTTPから4万行の同期完了を取得し、Replicaの適用停止時には書き込み前に `replication_not_ready` を返した。

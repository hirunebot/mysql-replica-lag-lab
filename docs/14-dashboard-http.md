# ステップ3：HTTP API

実DBを使う場合は先に [MySQLを起動・設定](02-usage.md) してから `cargo run --bin server` を実行する。DB停止中でもAPIは起動できる。既定は `127.0.0.1:3000`。変更する場合は `DASHBOARD_BIND_ADDR` にループバックのIP・ポートを指定する。

| 操作 | API | 応答 |
| --- | --- | --- |
| 現在の状態 | `GET /api/state` | 200 / `DashboardState`。DBに到達できなくても `unavailable` を返す |
| 開始 | `POST /api/runs` | 202 / `preparing` の `RunSnapshot`。不正設定は400、実行枠が使用中なら409 |
| 実行の取得 | `GET /api/runs/{id}` | 200 / 最新の `RunSnapshot`。知らないIDは404 |
| 手動SELECT | `POST /api/runs/{id}/reads` | 200 / 完了した `ManualRead` |

```sh
curl -sS http://127.0.0.1:3000/api/runs -H 'Content-Type: application/json' \
  -d '{"demo_rows":"200000","seed_batch_size":"1000","heavy_batch_size":"50000","marker_after_batches":"2"}'
```

受け付けるのは4つの負荷設定だけ。最大1,000万行・1,000 UPDATEバッチで、markerの後に最低1バッチを残す。接続先はサーバーの既存環境変数から読み込む。開始後のDB接続失敗やレプリケーション未設定は、その実行の `error` に記録する。

最新の1実行だけをメモリに保持し、HTTP切断で止めない。実行終了が不明なら `failing` と `completion_unconfirmed` を返し、次の開始を拒否する。再起動で履歴と排他枠は失われるため、停止・再起動の前後にはローカルDBの `SHOW FULL PROCESSLIST` と `SHOW REPLICA STATUS` で前のデモ処理が残っていないことを確認する。再起動でSQLは巻き戻らない。

開始は `lag_demo` を作り直す。CLI版や別プロセスのサーバーとは同時に実行しない。SSEは後続ステップで追加する。

## Web実行のDB観測

1実行につき観測サービスは1つ。markerとレプリケーション状態は既定500ms、行数と最古の未確認バッチは2秒間隔で取得する。Replicaの初期データは今回のスキーマ作成をGTIDで確認してから読む。取得に失敗したときは、前回値とその取得時刻を残す。

Webの書き込みはSourceに最大2接続、preflightはReplicaに1接続。preflight用接続を閉じてから、観測専用に各DB最大2接続を使う。SELECTのDB側制限は4.5秒、接続取得を含むクライアント側制限は5秒。期限切れの接続はプールから取り除く。

初期・最終同期の判定も同じ観測を使う。最終同期にはSourceワーカーの正常終了後の観測が必要で、両DBの件数・marker・Replicaの稼働状態が揃った時点で完了する。stale read未観測でも同期は完了できる。CLIは従来の照会間隔と出力を維持する。

検証は `cargo test --locked`。HTTPテストで同時開始・TCP切断・DB停止・終了不明を確認した。独立観測では、実MySQLのHTTP実行で両DBのmarkerが1になった後も、Replicaの更新が1万行から4万行まで増え、全8バッチを反映済みにして完了した。

## 手動SELECT

`POST /api/runs/{id}/reads` に `{"preset":"marker_sum"}` を送る。Replicaの専用プール（上限1接続）で照会し、SUMを10進文字列またはSQL NULLとして返す。初期同期後から実行でき、成功と失敗を含む直近5件は現在のrunに保存する。

準備中・照会中・置き換えられたrunは409、未知IDは404、DB接続不可は503、照会のタイムアウトは504。照会中は新規runの開始を409にし、ブラウザを閉じても結果を記録する。

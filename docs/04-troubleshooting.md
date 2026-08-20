# トラブルシューティング

## 1. RustアプリがMySQLへ接続できない

コンテナとポートを確認します。

```bash
docker compose ps
docker compose logs source
docker compose logs replica
lsof -nP -iTCP:3307 -sTCP:LISTEN
lsof -nP -iTCP:3308 -sTCP:LISTEN
```

`.env` の既定接続先は次のとおりです。

```text
SOURCE_DATABASE_URL=mysql://root:root@127.0.0.1:3307/mysql
REPLICA_DATABASE_URL=mysql://root:root@127.0.0.1:3308/mysql
```

## 2. replication is not configured と表示される

レプリケーション構成スクリプトを実行します。

```bash
./scripts/configure-replication.sh
```

このスクリプトは既存のReplica接続設定をリセットし、このCompose環境のSourceへ接続し直します。

## 3. receiverまたはapplierが停止している

詳細を確認します。

```bash
docker compose exec replica \
  mysql -uroot -proot -e "SHOW REPLICA STATUS\G"
```

特に次の項目を確認します。

- `Last_IO_Error`
- `Last_SQL_Error`
- `Replica_IO_Running`
- `Replica_SQL_Running`

初期状態からやり直せる場合は、デモ用ボリュームを削除して再構築します。

```bash
./scripts/reset-demo.sh
docker compose up -d --wait
./scripts/configure-replication.sh
```

## 4. stale readを観測できない

次の警告が表示される場合、Replicaが最初の監視より前にmarkerへ追いついています。

```text
WARNING: Replica had already applied the marker at the first observation.
```

`.env` を次の順で調整します。

1. `MARKER_AFTER_BATCHES` を `10` に増やす。
2. `DEMO_ROWS` を `2000000` に増やす。
3. `HEAVY_BATCH_SIZE` を `100000` に増やす。
4. `REPLICA_CPUS` を `0.10` に下げる。

CPU設定を変更した場合はコンテナを再作成します。

```bash
docker compose up -d --force-recreate --wait
./scripts/configure-replication.sh
cargo run --release
```

## 5. 初期同期または最終同期がtimeoutする

まずレプリケーションが動作中か確認します。

```bash
docker compose exec replica \
  mysql -uroot -proot -e "SHOW REPLICA STATUS\G"
```

正常に適用中で単純に時間が足りない場合は、`.env` の次の値を増やします。

```text
INITIAL_SYNC_TIMEOUT_SECS=1200
FINAL_SYNC_TIMEOUT_SECS=1200
```

デモを短くしたい場合は `DEMO_ROWS` を減らします。ただし減らしすぎるとstale readの観測時間も短くなります。

## 6. markerは古いのにSeconds_Behind_Sourceが0になる

異常とは限りません。`Seconds_Behind_Source` は現在処理しているイベントのtimestampなどから得られる近似値で、backlogやネットワークの状態を常に正確に表すものではありません。

このラボでは次を優先して判断します。

1. Source / Replica のmarker実データ
2. receiver位置とapplier位置の差
3. relay logサイズ
4. `Seconds_Behind_Source`

## 7. ポートが使用中

`.env` のURLだけでは、Composeの公開ポートは変わりません。`compose.yaml` の `3307:3306` または `3308:3306` を空いているポートへ変更し、対応するURLも同じ番号へ変更します。

## 8. ディスク使用量が増えた

100万行のSource / Replicaデータとbinary log、relay logを作るため、数GB程度使用する可能性があります。

不要になったデモデータを削除します。

```bash
./scripts/reset-demo.sh
```

この操作でComposeプロジェクトのMySQLボリュームが削除され、デモデータは復元できません。


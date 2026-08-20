# 利用方法

## 1. 前提

このプロジェクトはローカルのデモ専用です。Rustアプリケーションは起動のたびに `lag_demo` データベースを削除して再作成します。`.env` の接続先を共有環境や本番環境へ変更しないでください。

必要なコマンドを確認します。

```bash
docker --version
docker compose version
rustc --version
cargo --version
```

Source はホストの `3307`、Replica は `3308` を使用します。

## 2. 初回起動

プロジェクトルートで環境変数ファイルを作ります。

```bash
cp .env.example .env
```

MySQLを起動します。

```bash
docker compose up -d --wait
```

GTID auto-position を使用するレプリケーションを構成します。

```bash
./scripts/configure-replication.sh
```

正常なら次の項目が表示されます。

```text
Replica_IO_Running: Yes
Replica_SQL_Running: Yes
```

## 3. デモ実行

実際の時間差を見やすくするため、release buildで実行します。

```bash
cargo run --release
```

既定の100万行・Replica 0.25 CPU構成では、ホスト性能によって数分かかります。処理中は初期同期、大量更新、marker監視、最終同期の進捗が表示されます。

処理は次の順に進みます。

1. Source / Replica の接続とレプリケーション状態を確認する。
2. `lag_demo` を再作成する。
3. Sourceへ初期データを投入する。
4. Replicaの初期同期完了を待つ。
5. 大量更新を複数トランザクションで開始する。
6. 先行バッチのコミット後、別コネクションからmarkerを更新する。
7. Replicaが古いmarkerを返す時間を計測する。
8. 大量更新をすべて完了し、Replicaの最終同期を確認する。

## 4. PHP版の実行

Rust版とPHP版は同じ `lag_demo` データベースを再作成するため、同時に実行しないでください。

### ローカルPHP

必要な環境を確認します。

```bash
php --version
php -m | grep -E '^(PDO|pdo_mysql|pcntl)$'
```

PHP 8.2以上と3つのextensionが揃っていれば、プロジェクトルートから実行できます。

```bash
php php/bin/demo.php
```

PHP版はプロジェクトルートの `.env` を外部ライブラリなしで読み込みます。`PHP_SOURCE_DSN` などが未指定の場合は `127.0.0.1:3307` / `127.0.0.1:3308` を使用します。

### PHPコンテナ

ローカルPHPのextension構成に依存したくない場合は、専用イメージを使用します。

```bash
docker compose --profile php build php-demo
docker compose --profile php run --rm php-demo
```

profile付きserviceのため、通常の `docker compose up -d` ではPHPコンテナは起動しません。

## 5. 出力の読み方

```text
elapsed | Source marker | Replica marker | lag(s) | relay(bytes) | read-pos | exec-pos | heavy
```

| 項目 | 意味 |
| --- | --- |
| `elapsed` | Sourceでmarkerをコミットしてからの時間 |
| `Source marker` | Sourceで現在読めるmarker値 |
| `Replica marker` | Replicaで現在読めるmarker値 |
| `lag(s)` | `Seconds_Behind_Source`。補助指標として使用 |
| `relay(bytes)` | relay logの総サイズ |
| `read-pos` | receiverがSourceから読み取った位置 |
| `exec-pos` | applierが適用した位置 |
| `heavy` | Sourceでコミット済みの大量更新バッチ数 |

`Source marker=1` かつ `Replica marker=0` の行が、今回再現したい一時的なデータ差です。

`Seconds_Behind_Source` は0でも、Replicaでmarkerが古い場合があります。デモの成否はmarkerの実データ比較で判断します。

## 6. 負荷パラメーター

`.env` で以下を変更できます。

| 環境変数 | 既定値 | 説明 |
| --- | ---: | --- |
| `DEMO_ROWS` | `1000000` | 初期データ件数 |
| `SEED_BATCH_SIZE` | `1000` | INSERT 1回あたりの件数 |
| `HEAVY_BATCH_SIZE` | `50000` | 重いUPDATE 1トランザクションの件数 |
| `MARKER_AFTER_BATCHES` | `5` | marker投入前にコミットする先行バッチ数 |
| `POLL_INTERVAL_MS` | `500` | 監視間隔 |
| `INITIAL_SYNC_TIMEOUT_SECS` | `600` | 初期同期待ちの上限 |
| `FINAL_SYNC_TIMEOUT_SECS` | `600` | marker・最終同期待ちの上限 |
| `PHP_SOURCE_DSN` | `mysql:host=127.0.0.1;port=3307;charset=utf8mb4` | PHP版のSource接続先 |
| `PHP_REPLICA_DSN` | `mysql:host=127.0.0.1;port=3308;charset=utf8mb4` | PHP版のReplica接続先 |
| `PHP_DATABASE_USER` | `root` | PHP版のMySQLユーザー |
| `PHP_DATABASE_PASSWORD` | `root` | PHP版のMySQLパスワード |
| `SOURCE_CPUS` | `2.0` | SourceコンテナのCPU上限 |
| `REPLICA_CPUS` | `0.25` | ReplicaコンテナのCPU上限 |

ComposeのCPU設定を変更した場合はコンテナを作り直します。

```bash
docker compose up -d --force-recreate --wait
./scripts/configure-replication.sh
```

## 7. 状態の手動確認

レプリケーション状態:

```bash
docker compose exec replica mysql -uroot -proot -e "SHOW REPLICA STATUS\G"
```

marker比較:

```bash
docker compose exec source \
  mysql -uroot -proot -e "SELECT * FROM lag_demo.marker"

docker compose exec replica \
  mysql -uroot -proot -e "SELECT * FROM lag_demo.marker"
```

件数比較:

```bash
docker compose exec source \
  mysql -uroot -proot -e "SELECT version, COUNT(*) FROM lag_demo.items GROUP BY version"

docker compose exec replica \
  mysql -uroot -proot -e "SELECT version, COUNT(*) FROM lag_demo.items GROUP BY version"
```

## 8. 再実行と終了

MySQLを起動したまま `cargo run --release` を再実行できます。デモ用スキーマは自動的に作り直されます。

コンテナを停止してデータを残す場合:

```bash
docker compose down
```

MySQLボリュームも削除する場合:

```bash
./scripts/reset-demo.sh
```

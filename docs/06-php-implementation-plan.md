# PHP版 実装計画

## 1. 目的

既存のRust版と同じMySQL Source / Replica環境を使用し、先行する大量更新によって後続の軽いmarker更新がReplicaで待たされる現象をPHP CLIから再現する。

PHP版でも次の状態を実データで観測し、最終的にReplicaがSourceへ追いつくところまで確認する。

```text
Source  marker = 1
Replica marker = 0
```

## 2. Rust版との機能対応

PHP版はRust版の代替エントリーポイントとし、デモの意味が言語によって変わらないよう、次の機能を揃える。

- 同じ `lag_demo.items` / `lag_demo.marker` スキーマ
- 同じ初期データ件数とpayloadサイズ
- 同じ重いUPDATEとバッチ境界
- 指定バッチ数のコミット後にmarkerを更新
- Source / Replicaのmarker実値を比較
- `Seconds_Behind_Source`、relay logサイズ、読み取り・適用位置を表示
- stale readの成否を明示
- 大量更新の完了とReplicaの最終同期を確認

Rust版とPHP版は同じデモ用データベースを作り直すため、同時には実行しない。

## 3. 実行要件

- PHP 8.2以上
- PDO
- `pdo_mysql` extension
- `pcntl` extension
- Docker Composeで起動した既存のMySQL 8.4 Source / Replica

ローカルPHPに必要なextensionがない環境向けに、extensionを含むPHP CLI Dockerイメージも用意する。

## 4. 並行実行方式

PHP CLIの `pcntl_fork()` とUNIX socket pairを使用する。

```text
親プロセス
  ├── schema作成・seed・初期同期待ち
  ├── fork
  ├── 子からREADY通知を待つ
  ├── marker更新
  ├── Source / Replicaを監視
  └── 子の終了と最終同期を確認

子プロセス
  ├── Sourceへ新規PDO接続
  ├── 重いUPDATEをバッチ実行
  ├── 指定バッチ後にREADY通知
  └── 残りの重いUPDATEを継続
```

fork前に使用したPDO接続は明示的に破棄し、親子がそれぞれ新しい接続を作る。これにより、fork後に同じMySQLソケットを共有する問題を避ける。

## 5. 想定ファイル構成

```text
php/
  Dockerfile
  bin/
    demo.php
  src/
    Config.php
    Database.php
    Setup.php
    Workload.php
    Monitor.php
```

| ファイル | 責務 |
| --- | --- |
| `Config.php` | 環境変数、既定値、入力検証 |
| `Database.php` | PDO接続、marker・件数・レプリケーション状態取得 |
| `Setup.php` | schema再作成、seed、初期・最終同期待ち |
| `Workload.php` | fork、重いUPDATE、READY通知、marker更新 |
| `Monitor.php` | stale readとレプリケーション状態の時系列表示 |
| `demo.php` | 全処理のオーケストレーションと終了コード管理 |

外部パッケージは使用せず、PHP標準機能とPDOだけで実装する。

## 6. 設定

Rust版と負荷パラメーターを共有する。

| 環境変数 | 既定値 |
| --- | ---: |
| `DEMO_ROWS` | `1000000` |
| `SEED_BATCH_SIZE` | `1000` |
| `HEAVY_BATCH_SIZE` | `50000` |
| `MARKER_AFTER_BATCHES` | `5` |
| `POLL_INTERVAL_MS` | `500` |
| `INITIAL_SYNC_TIMEOUT_SECS` | `600` |
| `FINAL_SYNC_TIMEOUT_SECS` | `600` |

PDO接続にはPHP専用の次の環境変数を追加する。

```text
PHP_SOURCE_DSN=mysql:host=127.0.0.1;port=3307;charset=utf8mb4
PHP_REPLICA_DSN=mysql:host=127.0.0.1;port=3308;charset=utf8mb4
PHP_DATABASE_USER=root
PHP_DATABASE_PASSWORD=root
```

## 7. 処理手順

1. 必須extensionと設定値を検証する。
2. Source / ReplicaへPDO接続する。
3. receiver / applierが稼働中であることを確認する。
4. `lag_demo` を再作成する。
5. バッチINSERTで初期データを投入する。
6. Replicaが初期データへ追いつくまで待つ。
7. PDO接続を破棄してからforkする。
8. 子プロセスで大量UPDATEを開始する。
9. 子が指定バッチ数をコミットしたら親へREADYを通知する。
10. 親が別接続からmarkerを更新する。
11. Source / Replicaのmarkerとレプリケーション状態を定期表示する。
12. stale readを観測し、markerの反映時間を記録する。
13. 子プロセスの正常終了を確認する。
14. Replicaで全行が `version=2` になるまで待つ。
15. markerの最終一致とレプリケーションの正常稼働を確認する。

## 8. エラー処理

- 不正な環境変数はデータベース変更前に終了する。
- レプリケーション未構成・停止中は実行を拒否する。
- 子プロセスがREADY前に失敗した場合、親へエラー通知して終了する。
- 子プロセスの終了コードが0以外ならデモを失敗扱いにする。
- 初期同期、marker反映、最終同期にはtimeoutを設ける。
- 例外は標準エラーへ出力し、CLIの終了コードを1にする。

## 9. Docker実行

Composeにprofile付きの `php-demo` serviceを追加する。通常の `docker compose up` では起動せず、次のコマンドで必要なときだけ実行する。

```bash
docker compose --profile php run --rm php-demo
```

コンテナ内では `source:3306` / `replica:3306` を使用する。

## 10. ドキュメント更新

- READMEへRust版・PHP版の実行方法を併記する。
- `docs/02-usage.md` へPHPローカル実行とDocker実行を追加する。
- `docs/07-php-validation.md` にlint・実機検証結果を記録する。
- トラブルシューティングへPHP extensionとfork関連の項目を追加する。

## 11. 検証項目

### 静的検証

- すべてのPHPファイルで `php -l`
- Shell、Rust、Composeの既存検証が引き続き成功
- `git diff --check`

### 実機検証

- PHP版でSourceの新marker / Replicaの旧markerを同時観測
- markerがReplicaへ反映されるまでの時間を表示
- 子プロセスが大量更新を完了
- Replicaで全対象行が `version=2`
- Source / Replicaのmarkerが最終一致
- receiver / applierが最後まで正常稼働

## 12. 完了条件

- PHP版がRust版と同じシナリオを再現できる。
- ローカルPHPとDockerの両方に実行手順がある。
- 初見の利用者がREADMEと番号付きdocsから構成と操作を把握できる。
- lint・既存テスト・Compose検証が成功する。
- 実機検証結果が番号付きドキュメントへ記録される。

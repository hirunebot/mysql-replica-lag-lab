# mysql-replica-lag-lab 実装計画

## 1. 目的

MySQL の非同期レプリケーション構成をローカルに構築し、レプリカで重い更新の適用が続いている間、後続の軽い更新までレプリカへの反映を待たされる状況を再現する。

Rust 製のデモアプリケーションから負荷の投入と Source / Replica の監視を行い、次の状態を時系列で可視化する。

```text
Source  : 軽い更新がコミット済み
Replica : 先行する大量更新の適用中で、軽い更新は未反映
```

ここで観測するデータ差は恒久的な不整合ではなく、非同期レプリケーションによる一時的な stale read とする。

## 2. プロジェクト名

- リポジトリ名: `mysql-replica-lag-lab`
- 呼称: `Lag Lab`
- Rust パッケージ名: `mysql-replica-lag-lab`

## 3. デモ対象

### 対象に含めるもの

- Docker Compose による MySQL Source / Replica 構成
- GTID と ROW 形式の非同期レプリケーション
- Replica の applier worker を 1 に制限した逐次適用
- Replica の CPU 制限による再現性の向上
- Rust による初期データ生成
- 複数バッチの大量 `UPDATE` によるレプリケーション backlog の生成
- 大量更新中に別コネクションから投入する軽い marker 更新
- Source / Replica の marker 値とレプリケーション状態の定期監視
- 軽い更新が Replica に反映されるまでの時間計測

### 対象外

- MySQL Group Replication
- 半同期レプリケーション
- フェイルオーバーや昇格処理
- 永続的なデータ不整合の再現
- 本番環境向けの性能チューニング

## 4. 想定構成

```text
Rust demo application
  ├── Source : localhost:3307
  │     └── 大量更新と軽い marker 更新を実行
  │
  └── Replica: localhost:3308
        ├── replica_parallel_workers=1
        ├── read_only=ON
        ├── super_read_only=ON
        └── CPU を Source より低く制限

Source -- binary log --> Replica relay log -- applier --> Replica tables
```

Docker ネットワーク内では、Replica から Source へホスト名 `source:3306` で接続する。

## 5. デモシナリオ

1. Source と Replica を起動する。
2. GTID auto-position を使用してレプリケーションを開始する。
3. Source にデモ用テーブルを作成する。
4. Rust から `items` テーブルへ大量の初期データを投入する。
5. Replica の初期同期完了を待つ。
6. Rust のバックグラウンドタスクから `items` を複数バッチで更新する。
7. 一定数の重い更新バッチが Source でコミットされた時点で、別コネクションから `marker` を更新する。
8. Source では marker が更新済み、Replica では未更新の状態を監視して表示する。
9. Replica の marker が更新された時点で、反映までの経過時間を表示する。
10. 重い更新タスクとレプリケーションが最終的に完了したことを確認する。

軽い更新は、少なくとも複数の重いトランザクションが binlog 上で先行したあとにコミットさせる。重いトランザクションのコミット前に軽い更新が先行すると、軽い更新が先にレプリケートされ、意図した待ち状態にならないためである。

## 6. データモデル

### `items`

大量更新による backlog 生成に使用する。

| カラム | 用途 |
| --- | --- |
| `id` | 主キー、バッチ範囲の指定 |
| `version` | 更新回数の確認 |
| `payload` | binlog と適用処理に十分なデータ量を持たせる |
| `updated_at` | 更新日時の確認 |

初期値は 100 万行、payload は約 512 byte を想定する。実行環境に応じて行数を変更可能にする。

### `marker`

重い更新とは無関係な軽量 SQL の反映状況を確認する。

| カラム | 用途 |
| --- | --- |
| `id` | 固定値 `1` の主キー |
| `value` | Source / Replica の値を比較 |
| `updated_at` | 更新時刻の確認 |

## 7. Rust アプリケーション構成

非同期ランタイムには Tokio、MySQL 接続には SQLx を使用する。

```text
src/
  main.rs        エントリーポイントとデモ全体の制御
  config.rs      接続先、行数、バッチサイズなどの設定
  database.rs    Source / Replica 接続と共通クエリ
  setup.rs       スキーマ作成、seed、初期同期待ち
  workload.rs    大量更新と marker 更新
  monitor.rs     marker、lag、relay log 状態の監視
```

初期実装では必要以上に抽象化せず、責務が明確になった段階で `main.rs` から各モジュールへ分割する。

## 8. 監視項目

デモ中は一定間隔で次の情報を1行に表示する。

- marker 更新後の経過秒数
- Source の marker 値
- Replica の marker 値
- `Seconds_Behind_Source`
- `Relay_Log_Space`
- `Read_Source_Log_Pos`
- `Exec_Source_Log_Pos`
- 大量更新の完了バッチ数

`Seconds_Behind_Source` 単独では正確な backlog 量を表さない場合がある。そのため、marker の実データ比較をデモの主判定とし、レプリケーションステータスは補助情報として扱う。

## 9. 実装フェーズ

### Phase 1: MySQL 環境

- `compose.yaml` を作成する。
- Source 初期化 SQL でレプリケーションユーザーを作成する。
- Source / Replica に GTID、server ID、binary log を設定する。
- Replica に単一 applier worker と CPU 制限を設定する。
- レプリケーション開始用スクリプトを作成する。
- `SHOW REPLICA STATUS` で正常接続を確認する。

### Phase 2: Rust の初期化処理

- Cargo プロジェクトを作成する。
- Source / Replica の connection pool を作成する。
- デモ用スキーマを作成する。
- バッチ INSERT で初期データを生成する。
- Replica の初期同期完了を待つ処理を実装する。

### Phase 3: backlog 生成

- 大量更新を指定行数ごとのトランザクションへ分割する。
- バックグラウンドタスクから連続して更新する。
- 指定バッチ数のコミット後にメインタスクへ通知する。
- Source が Replica より速く更新を生成できることを確認する。

### Phase 4: 軽い更新と監視

- 別コネクションから marker 更新をコミットする。
- Source / Replica の marker を定期比較する。
- marker が Replica へ反映されるまでの時間を計測する。
- 大量更新の進行状況とレプリケーション状態を同時表示する。

### Phase 5: 再現性とドキュメント

- 複数回実行して stale read の時間窓が発生することを確認する。
- 行数、バッチサイズ、Replica CPU を環境変数で調整可能にする。
- 通常実行、負荷調整、初期化、トラブルシューティングを文書化する。

## 10. 想定ファイル構成

```text
mysql-replica-lag-lab/
  Cargo.toml
  Cargo.lock
  compose.yaml
  README.md
  .env.example
  mysql/
    source-init/
      01-create-replication-user.sql
  scripts/
    configure-replication.sh
    reset-demo.sh
  src/
    main.rs
    config.rs
    database.rs
    setup.rs
    workload.rs
    monitor.rs
  docs/
    01-plan.md
    02-usage.md
    03-demo-scenario.md
    04-troubleshooting.md
```

ドキュメントは参照順に2桁の番号を付ける。ファイル追加時も `05-...md` のように連番とする。

## 11. 調整可能なパラメーター

| パラメーター | 初期値 | 目的 |
| --- | ---: | --- |
| `DEMO_ROWS` | `1000000` | 初期データ件数 |
| `SEED_BATCH_SIZE` | `1000` | INSERT 1回あたりの件数 |
| `HEAVY_BATCH_SIZE` | `50000` | UPDATE 1トランザクションあたりの件数 |
| `MARKER_AFTER_BATCHES` | `5` | marker 投入前にコミットする重い更新数 |
| `POLL_INTERVAL_MS` | `500` | Source / Replica の監視間隔 |
| Replica CPU | `0.25` | applier を意図的に遅くする制限 |

ラグが短すぎる場合は、`DEMO_ROWS`、`HEAVY_BATCH_SIZE`、`MARKER_AFTER_BATCHES` の順に増やすか、Replica CPU を下げる。

## 12. 完了条件

- Source / Replica が Docker Compose で起動する。
- レプリケーションの receiver と applier が正常稼働する。
- 初期データが Replica へ同期される。
- 大量更新中に軽い marker 更新を別コネクションから実行できる。
- Source の marker 更新後、Replica が古い marker 値を返す時間窓を観測できる。
- marker が Replica へ反映されるまでの時間が表示される。
- 最終的に Source / Replica の marker 値が一致する。
- 初期化からデモ実行まで README の手順だけで再現できる。

## 13. 検証時の注意

- デモ専用環境で実行し、本番データベースへ接続しない。
- Replica へのアプリケーション書き込みは禁止する。
- 大量更新に主キー範囲を使用し、意図しないフルスキャンを避ける。
- Docker Desktop の CPU 制限の挙動はホスト環境に依存するため、必要に応じてデータ量も調整する。
- 初期同期中の backlog とデモ本体の backlog を混同しないよう、初期同期完了後に負荷を開始する。
- デモ終了時には Replica が最終状態へ追いついたことを確認する。

# mysql-replica-lag-lab

MySQL の非同期レプリケーションで、先行する大量更新の適用が後続の軽い更新を待たせ、Source と Replica で一時的に異なるデータが読める状況を再現するラボです。負荷の投入と観測は Rust アプリケーションから行います。

```text
Source:  大量更新 A1, A2, ... をコミット → 軽い marker 更新をコミット
Replica: A1 を適用 → A2 を適用 → ... → marker は relay log 内で待機
```

このラボが扱うのは恒久的なデータ不整合ではなく、非同期レプリケーションによる一時的な stale read です。

## 必要なもの

- Docker Desktop または Docker Engine + Compose v2
- Rust stable と Cargo
- Source 用の `3307`、Replica 用の `3308` ポート

## クイックスタート

```bash
cp .env.example .env
docker compose up -d --wait
./scripts/configure-replication.sh
cargo run --release
```

アプリケーションは実行のたびにデモ専用の `lag_demo` データベースを Source 上で作り直します。この接続設定をデモ環境以外へ向けないでください。

成功時は、次のような出力になります。

```text
elapsed | Source marker | Replica marker | lag(s) | relay(bytes) | ...
  0.01s |             1 |              0 |      2 |    145223140 | ...
  1.02s |             1 |              0 |      3 |    192881020 | ...
  3.53s |             1 |              1 |      0 |    390112340 | ...

SUCCESS: Replica returned the old marker after Source committed the new value.
The lightweight marker took 3.53s to become visible on Replica.
```

marker の更新自体は軽量ですが、Replica の applier worker を1つに制限しているため、先行する大量更新を追い越せません。

## 構成

```text
Rust application
  ├── mysql://127.0.0.1:3307  Source
  │       └── binary log (GTID / ROW)
  │
  └── mysql://127.0.0.1:3308  Replica
          ├── relay log
          ├── replica_parallel_workers=1
          └── CPU limit: 0.25
```

主なソースコードは次の責務に分かれています。

| ファイル | 責務 |
| --- | --- |
| `src/config.rs` | 環境変数、既定値、入力検証 |
| `src/database.rs` | MySQL接続、実データとレプリケーション状態の取得 |
| `src/setup.rs` | スキーマ再作成、seed、初期・最終同期待ち |
| `src/workload.rs` | 大量更新の生成、軽いmarker更新 |
| `src/monitor.rs` | Source / Replica の差とlagの時系列表示 |
| `src/main.rs` | デモ全体のオーケストレーション |

## ドキュメント

1. [実装計画](docs/01-plan.md)
2. [利用方法](docs/02-usage.md)
3. [デモシナリオと仕組み](docs/03-demo-scenario.md)
4. [トラブルシューティング](docs/04-troubleshooting.md)
5. [検証記録](docs/05-validation.md)

## 開発時の確認

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
docker compose config --quiet
```

## 停止と初期化

コンテナだけ停止し、MySQLデータを残す場合:

```bash
docker compose down
```

コンテナとデモ用MySQLボリュームを削除して最初からやり直す場合:

```bash
./scripts/reset-demo.sh
```

`reset-demo.sh` はこのComposeプロジェクトのMySQLボリュームを削除するため、保存されたデモデータは復元できません。

## ライセンス

MIT

# 検証記録

## 1. 検証目的

計画の完了条件に対し、静的検証だけでなく、実際のMySQL Source / Replica上でstale readの発生と最終同期を確認する。

## 2. 検証環境

検証日: 2026-08-20

| 項目 | バージョンまたは設定 |
| --- | --- |
| MySQL | `8.4.10` |
| Docker | `28.2.2` |
| Docker Compose | `v2.37.1-desktop.1` |
| Rust | `1.91.1` |
| Source CPU上限 | `2.0` |
| Replica CPU上限 | `0.25` |
| 初期データ | `1,000,000` 行 |
| 重いUPDATE | `50,000` 行 × 20バッチ |
| marker投入位置 | 5バッチのコミット後 |
| Replica applier worker | `1` |

## 3. 静的検証

実行コマンド:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
bash -n scripts/configure-replication.sh
bash -n scripts/reset-demo.sh
docker compose config --quiet
git diff --check
```

結果:

- rustfmt差分なし
- Clippy警告なし
- 単体テスト2件成功
- Shellスクリプト構文エラーなし
- Compose設定エラーなし
- whitespaceエラーなし

## 4. レプリケーション構成確認

`./scripts/configure-replication.sh` の結果:

```text
Replication is running.
  Source_Host: source
  Replica_IO_Running: Yes
  Replica_SQL_Running: Yes
  Approximate lag: 0s
```

## 5. 実動作結果

既定値で次を実行した。

```bash
cargo run --release
```

主要な実測値:

| 項目 | 結果 |
| --- | ---: |
| Sourceへのseed | 10.60秒 |
| Replicaの初期同期 | 189.28秒 |
| Sourceの大量更新全体 | 14.50秒 |
| 大量更新件数 | 1,000,000行 |
| marker更新のSourceコミット | 0.150秒 |
| markerがReplicaで見えるまで | 41.87秒 |
| Replicaの最終同期 | 273.69秒 |

marker監視では、次の状態を継続的に観測した。

```text
elapsed | Source marker | Replica marker | lag(s) | heavy
  0.06s |             1 |              0 |      3 | 5/20
  7.56s |             1 |              0 |     11 | 16/20
 26.85s |             1 |              0 |     28 | 20/20
 41.87s |             1 |              1 |     42 | 20/20
```

アプリケーションの判定:

```text
SUCCESS: Replica returned the old marker after Source committed the new value.
The lightweight marker took 41.87s to become visible on Replica.
Heavy workload: 20 batches, 1000000 affected rows, 14.50s on Source.
Final synchronization completed in 273.69s.
Demo completed with Source and Replica in sync.
```

## 6. 完了条件との対応

| 完了条件 | 証拠 | 結果 |
| --- | --- | --- |
| ComposeでSource / Replicaが起動 | 両コンテナのhealthcheck成功 | 達成 |
| receiverとapplierが正常稼働 | 両Running項目が`Yes` | 達成 |
| 初期データがReplicaへ同期 | Replicaで100万行を確認後に負荷開始 | 達成 |
| 大量更新中にmarkerを別コネクションで更新 | 5バッチ後にmarkerをコミットし、後続15バッチを継続 | 達成 |
| Sourceの新値とReplicaの旧値を同時観測 | 41.87秒間にわたり `1` / `0` を観測 | 達成 |
| marker反映時間を表示 | 41.87秒と計測 | 達成 |
| 最終的にmarkerが一致 | Source / Replicaとも`1` | 達成 |
| 最終的に全更新を適用 | Replicaで100万行が`version=2` | 達成 |

## 7. 備考

- `Seconds_Behind_Source` は補助指標とし、成否はmarkerの実値で判定した。
- ReplicaのCPU制限により初期同期と最終同期にも時間がかかる。これはデモの再現性とのトレードオフである。
- 実行時間はホスト性能によって変動するため、同じ秒数になることは要件としない。

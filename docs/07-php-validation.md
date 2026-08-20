# PHP版 検証記録

## 1. 検証目的

PHP版が実装計画どおりにRust版と同じstale readシナリオを再現し、親子プロセスの並行動作、最終同期、Docker実行環境まで正常に機能することを確認する。

## 2. 検証環境

検証日: 2026-08-20

| 項目 | バージョンまたは設定 |
| --- | --- |
| ローカルPHP | `8.5.9` |
| Docker PHP | `8.4.24` |
| MySQL | `8.4.10` |
| PDO driver | `pdo_mysql` |
| 並行処理 | `pcntl_fork()` |
| Source CPU上限 | `2.0` |
| Replica CPU上限 | `0.25` |
| 初期データ | `300,000` 行 |
| 重いUPDATE | `50,000` 行 × 6バッチ |
| marker投入位置 | 3バッチのコミット後 |
| Replica applier worker | `1` |

## 3. PHP構文検証

実行コマンド:

```bash
find php -name '*.php' -type f -print0 | xargs -0 -n1 php -l
```

対象となる6ファイルすべてで `No syntax errors detected` を確認した。

## 4. ローカルPHP実機デモ

検証時間を抑えつつ複数の先行・後続バッチを持たせるため、次の設定で実行した。

```bash
DEMO_ROWS=300000 \
HEAVY_BATCH_SIZE=50000 \
MARKER_AFTER_BATCHES=3 \
php php/bin/demo.php
```

主要な実測値:

| 項目 | 結果 |
| --- | ---: |
| Sourceへのseed | 3.68秒 |
| Replicaの初期同期 | 50.60秒 |
| Sourceの大量更新全体 | 4.88秒 |
| 大量更新件数 | 300,000行 |
| marker更新のSourceコミット | 0.027秒 |
| markerがReplicaで見えるまで | 24.20秒 |
| Replicaの最終同期 | 47.69秒 |

marker監視の抜粋:

```text
elapsed | Source marker | Replica marker | lag(s) | heavy
  0.00s |             1 |              0 |      2 | 3/6
  5.49s |             1 |              0 |      7 | 6/6
 14.88s |             1 |              0 |     16 | 6/6
 24.20s |             1 |              1 |     24 | 6/6
```

アプリケーションの最終結果:

```text
SUCCESS: Replica returned the old marker after Source committed the new value.
The lightweight marker took 24.20s to become visible on Replica.
Heavy workload: 6 batches, 300000 affected rows, 4.88s on Source.
Final synchronization completed in 47.69s.
Demo completed with Source and Replica in sync.
```

## 5. forkと進捗通知

実機ログから次を確認した。

- 子プロセスが3バッチをコミットした時点で親へREADYを通知した。
- 親はREADY後、別PDO接続からmarkerをコミットした。
- 親がmarkerを監視している間も、子は残り3バッチを実行した。
- UNIX socket経由の進捗が `3/6` から `6/6` へ更新された。
- 子の終了コードは0で、完了summaryを親が受信した。
- fork前のPDOは破棄され、親子が個別に再接続した。

## 6. Docker PHP検証

イメージをビルドした。

```bash
docker compose --profile php build php-demo
```

ビルド結果:

```text
php-demo Built
```

コンテナ内でextensionとMySQL接続を確認した。

```text
PHP=8.4.24 extensions=ok source=1 replica=1
```

これにより、Dockerイメージ内の `pdo_mysql` / `pcntl` と、Composeネットワーク上の `source:3306` / `replica:3306` への接続を確認した。

## 7. 完了条件との対応

| 完了条件 | 証拠 | 結果 |
| --- | --- | --- |
| Rust版と同じシナリオ | 同一schema・UPDATE・marker比較を実行 | 達成 |
| stale readを実データで観測 | 24.20秒間 `Source=1 / Replica=0` | 達成 |
| 大量更新とmarker監視を並行実行 | fork子が6バッチ、親がmarkerを監視 | 達成 |
| 子プロセスの正常終了 | 終了コード0とsummary受信 | 達成 |
| 最終データ一致 | 30万行が`version=2`、marker一致 | 達成 |
| レプリケーション正常稼働 | 最終確認でreceiver/applier稼働 | 達成 |
| ローカルPHP手順 | `docs/02-usage.md`へ記載 | 達成 |
| Docker PHP手順 | profile付きserviceをビルド・接続確認 | 達成 |
| PHP構文検証 | 全6ファイルでlint成功 | 達成 |

## 8. 備考

- Rust版とPHP版は同じ `lag_demo` を再作成するため、同時実行しない。
- PHP版でもデモ成否は `Seconds_Behind_Source` ではなくmarkerの実値で判定する。
- 既定の100万行構成は、30万行で行った今回の検証より長いstale read時間を作りやすい。
- 実行時間はホスト性能によって変化する。

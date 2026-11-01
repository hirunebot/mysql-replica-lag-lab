# テスト計画

## 1. 目的

Rust版・PHP版のデモが、単に正常終了するだけでなく、意図した順序で重い更新とmarker更新を実行し、Replicaのstale readを実際に観測したうえで最終同期へ到達することを自動検証する。

## 2. テストレベル

| レベル | 対象 | 外部依存 | 実行時間の目安 |
| --- | --- | --- | --- |
| Rust単体テスト | 設定、バッチ境界、marker状態遷移 | なし | 1秒未満 |
| PHP単体テスト | 設定、バッチ境界、marker状態遷移 | PHP CLI | 1秒未満 |
| 静的検証 | Rust fmt/Clippy、PHP lint、Shell、Compose | 各ランタイム | 数秒 |
| E2E | Source / Replica、Rust版・PHP版の全シナリオ | Docker、MySQL | 数分 |

## 3. 単体テストケース

### 設定

| ID | テスト | 期待結果 |
| --- | --- | --- |
| CFG-01 | 端数を含む行数から総バッチ数を計算する | 最終端数を1バッチとして数える |
| CFG-02 | SourceとReplicaに同じ接続先を指定する | 実行前に拒否する |
| CFG-03 | 行数・バッチサイズなどに0を指定する | 実行前に拒否する |
| CFG-04 | marker投入位置が総バッチ数を超える | 実行前に拒否する |

### バッチ境界

| ID | テスト | 期待結果 |
| --- | --- | --- |
| BAT-01 | 行数がバッチサイズで割り切れる | 重複・欠落なしで全行を覆う |
| BAT-02 | 最終バッチに端数がある | 最終IDを超えず全行を覆う |
| BAT-03 | 0番または総バッチ数を超える番号 | 範囲なしを返す |

### marker状態遷移

| ID | テスト | 期待結果 |
| --- | --- | --- |
| MRK-01 | Sourceが新値、Replicaが旧値 | stale readを記録し、未同期と判定する |
| MRK-02 | MRK-01の後にReplicaが新値 | stale read記録を保持したまま同期済みと判定する |
| MRK-03 | 最初の観測から両方が新値 | 同期済みだがstale read未観測と判定する |

## 4. E2Eテストケース

| ID | テスト | 判定条件 |
| --- | --- | --- |
| E2E-01 | MySQL起動とレプリケーション構成 | receiver / applierがともに稼働する |
| E2E-02 | 初期データ同期 | Source / Replicaの行数とmarker初期値が一致する |
| E2E-03 | 先行する重い更新 | marker投入前に指定バッチ数がコミットされる |
| E2E-04 | marker stale read | 出力中に `Source=1 / Replica=0` が存在し、SUCCESS判定になる |
| E2E-05 | 重い更新の継続 | marker投入後にも後続バッチが実行される |
| E2E-06 | marker最終一致 | Source / Replicaのmarkerがともに1になる |
| E2E-07 | 全更新の最終適用 | Source / Replicaで対象行がすべて `version=2` になる |
| E2E-08 | レプリケーション健全性 | エラーなしでreceiver / applierが稼働し続ける |
| E2E-09 | Replica書き込み保護 | `read_only` / `super_read_only` が有効である |

E2E-02からE2E-09をRust版とPHP版の双方で順番に実行する。

## 5. E2E用データ量

既定のE2E設定は、再現性と実行時間のバランスを取る。

| 項目 | 既定値 |
| --- | ---: |
| `E2E_ROWS` | `200000` |
| `E2E_SEED_BATCH_SIZE` | `1000` |
| `E2E_HEAVY_BATCH_SIZE` | `50000` |
| `E2E_MARKER_AFTER_BATCHES` | `2` |
| `E2E_POLL_INTERVAL_MS` | `250` |
| 同期timeout | `600`秒 |

4つの重いバッチのうち、2バッチをmarkerより前、2バッチをmarkerより後に配置する。これにより「先行更新がmarkerを待たせる」と「marker投入後も重い処理が継続する」の両方を検証する。

## 6. 実行コマンド

単体・静的テスト:

```bash
./scripts/test-unit.sh
```

Rust版E2E:

```bash
./scripts/test-e2e.sh rust
```

PHP版E2E:

```bash
./scripts/test-e2e.sh php
```

両方のE2E:

```bash
./scripts/test-e2e.sh all
```

## 7. E2Eの副作用

- `lag_demo` データベースを削除して作り直す。
- Rust版とPHP版を直列実行する。
- Source / ReplicaのDockerコンテナとvolumeは削除しない。
- テスト終了後は最終実行言語のテストデータが残る。
- 本番または共有MySQLへ接続して実行しない。

## 8. 完了条件

- RustとPHPで同等の単体テストケースが存在する。
- 失敗時にテストケース名と理由が表示され、非0で終了する。
- E2Eがログ文字列だけでなく、Source / Replicaの最終データをSQLで再検証する。
- E2Eがstale readの実観測を必須条件とする。
- READMEからテストの目的・コマンド・副作用を確認できる。
- 単体テストとRust/PHP双方のE2Eが実際に成功する。

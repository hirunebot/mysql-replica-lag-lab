# デモシナリオと仕組み

## 1. 示したいこと

アプリケーションがSourceへ書き込んだ直後にReplicaから読み取る構成では、Sourceでコミット済みの値がReplicaでまだ読めない場合があります。

このラボでは、重い更新と無関係な `marker` テーブルを用意し、軽い更新までレプリケーションbacklogの後ろで待たされることを示します。

## 2. なぜ重いSQLの「コミット後」にmarkerを投入するのか

MySQLのReplicaが適用できるのは、Sourceのbinary logへ記録されたトランザクションです。

長時間のUPDATEがSource上でまだ実行中で未コミットの場合、そのトランザクションはReplicaで適用開始できません。その間に別トランザクションのmarker更新が先にコミットすると、markerがbinary log上でも先行し、Replicaへ早く反映される可能性があります。

そこで、このラボでは次の順序を作ります。

```text
1. heavy batch 1 commit
2. heavy batch 2 commit
3. heavy batch 3 commit
4. heavy batch 4 commit
5. heavy batch 5 commit
6. marker update commit
7. heavy batch 6 commit
8. heavy batch 7 commit
...
```

Sourceでは後続の大量更新が続いているため「重い処理中に別SQLを実行」という状態です。一方、Replicaではmarkerより前の大量更新を順番に適用する必要があります。

## 3. トランザクションの流れ

```text
Source                           Replica
------                           -------
heavy batch 1 commit ────────▶ relay log ─▶ apply batch 1
heavy batch 2 commit ────────▶ relay log     wait
heavy batch 3 commit ────────▶ relay log     wait
heavy batch 4 commit ────────▶ relay log     wait
heavy batch 5 commit ────────▶ relay log     wait
marker=1 commit       ────────▶ relay log     wait

SELECT marker => 1                         SELECT marker => 0

                                  ...batch 5 applied...
                                  marker applied

SELECT marker => 1                         SELECT marker => 1
```

Replicaは `replica_parallel_workers=1` で構成されるため、トランザクションを追い越して適用しません。

## 4. データモデル上の工夫

### 大量更新用 `items`

- 既定で100万行を作成する。
- 1行あたり約512 byteのpayloadを持つ。
- 主キー範囲ごとに5万行を更新する。
- payload全体を更新し、ROW形式のbinary logに十分な行イベントを生成する。

### 観測用 `marker`

- 1行だけを保持する。
- 大量更新用テーブルとは独立している。
- `value=0` から `value=1` への更新だけを行う。
- Source / Replicaへ同じSELECTを発行し、結果を直接比較する。

markerを別テーブルにすることで、ロック待ちではなくレプリケーション適用待ちであることを説明しやすくしています。

## 5. 再現性を高める設定

| 設定 | 狙い |
| --- | --- |
| `binlog_format=ROW` | 更新された各行を行イベントとして転送する |
| `binlog_row_image=FULL` | 更新行の完全なイメージを記録する |
| `replica_parallel_workers=1` | Replicaの適用を逐次化する |
| `read_only=ON` / `super_read_only=ON` | 構成後のReplicaへの通常書き込みを防ぐ |
| `REPLICA_CPUS=0.25` | SourceよりReplicaの適用能力を低くする |
| `sync_binlog=1` | コミット・永続化境界を明確にする |
| `innodb_flush_log_at_trx_commit=1` | トランザクションの耐久性を保つ |

CPU制限だけに依存せず、データ件数と先行バッチ数も調整できるようにしています。

## 6. 成功条件

次のすべてを満たした場合に成功とします。

1. Sourceのmarker更新がコミットされる。
2. Sourceで新しいmarker、Replicaで古いmarkerを同時に観測する。
3. その後、ReplicaのmarkerがSourceと一致する。
4. 最終的に全 `items` が `version=2` となる。
5. receiverとapplierが停止せず、正常なレプリケーションで追いつく。

## 7. 実運用との対応

実システムでは、次のようなケースに相当します。

- APIが更新をSourceへ書き込む。
- 同じリクエストまたは直後のリクエストがReplicaへSELECTする。
- Replicaにbacklogがあり、更新前の値が返る。
- 「保存したはずの内容が一時的に見えない」という挙動になる。

対策の候補には、更新直後の読み取りをSourceへ向ける、GTID位置までの適用を待つ、用途に応じた整合性レベルを設計する、Replicaの適用能力を監視・改善する、といった方法があります。このプロジェクト自体は対策の実装ではなく、問題の再現と観察に範囲を限定しています。

## 8. 公式資料

- [MySQL 8.4: Replication Formats](https://dev.mysql.com/doc/refman/8.4/en/replication-formats.html)
- [MySQL 8.4: Replica Server Options and Variables](https://dev.mysql.com/doc/refman/8.4/en/replication-options-replica.html)
- [MySQL 8.4: SHOW REPLICA STATUS](https://dev.mysql.com/doc/refman/8.4/en/show-replica-status.html)

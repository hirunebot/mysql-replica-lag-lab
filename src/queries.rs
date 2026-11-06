//! SQLは実行と予定/通知で同じ定義を使う。画面用の短縮SQLは作らない。
use crate::contracts::SqlStatement;

/// 既存UPDATEのversion加算・SHA2と512byte payload・ID範囲を維持する。
pub fn heavy_update(first_id: u64, last_id: u64) -> SqlStatement {
    SqlStatement {
        template: r#"UPDATE lag_demo.items
            SET version = version + 1,
                payload = CONCAT(
                    SHA2(CONCAT(payload, ':', id, ':', version), 256),
                    REPEAT('z', 448)
                )
            WHERE id BETWEEN ? AND ?"#
            .into(),
        parameters: vec![first_id.to_string(), last_id.to_string()],
    }
}

/// 既存marker UPDATEのvalue加算・updated_at更新・id=1を維持する。
pub fn marker_update() -> SqlStatement {
    statement(
        "UPDATE lag_demo.marker SET value = value + 1, updated_at = CURRENT_TIMESTAMP(6) WHERE id = 1",
    )
}

pub(crate) const MARKER_VALUE: &str = "SELECT value FROM lag_demo.marker WHERE id = 1";
pub(crate) const ITEM_COUNT: &str = "SELECT COUNT(*) FROM lag_demo.items";
pub(crate) const UPDATED_COUNT: &str = "SELECT COUNT(*) FROM lag_demo.items WHERE version = 2";
pub(crate) const UPDATED_BATCH_COUNT: &str =
    "SELECT COUNT(*) FROM lag_demo.items WHERE id BETWEEN ? AND ? AND version = 2";

pub(crate) fn statement(sql: &str) -> SqlStatement {
    SqlStatement {
        template: sql.into(),
        parameters: Vec::new(),
    }
}

/// ステップ2のバインド値はIDのみ。通知した同じ値を整数として送る。
pub(crate) async fn execute(pool: &sqlx::MySqlPool, sql: &SqlStatement) -> anyhow::Result<u64> {
    let mut query = sqlx::query(&sql.template);
    for value in &sql.parameters {
        query = query.bind(value.parse::<u64>()?);
    }
    Ok(query.execute(pool).await?.rows_affected())
}

pub(crate) const SETUP: [&str; 5] = [
    "DROP DATABASE IF EXISTS lag_demo",
    "CREATE DATABASE lag_demo",
    r#"CREATE TABLE lag_demo.items (
            id BIGINT UNSIGNED NOT NULL PRIMARY KEY,
            version BIGINT UNSIGNED NOT NULL,
            payload VARCHAR(1024) NOT NULL,
            updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
            KEY idx_version (version)
        ) ENGINE=InnoDB"#,
    r#"CREATE TABLE lag_demo.marker (
            id BIGINT UNSIGNED NOT NULL PRIMARY KEY,
            value BIGINT UNSIGNED NOT NULL,
            updated_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6)
        ) ENGINE=InnoDB"#,
    "INSERT INTO lag_demo.marker (id, value) VALUES (1, 0)",
];

pub(crate) fn seed_insert(first: u64, last: u64) -> SqlStatement {
    use std::fmt::Write;
    let mut sql = String::from("INSERT INTO lag_demo.items (id, version, payload) VALUES ");
    for id in first..=last {
        if id != first {
            sql.push(',');
        }
        write!(sql, "({id}, 1, REPEAT('x', 512))").expect("write to String");
    }
    statement(&sql)
}

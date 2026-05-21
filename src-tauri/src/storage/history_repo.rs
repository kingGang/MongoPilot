use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRow {
    pub id: i64,
    pub connection_id: String,
    pub database_name: String,
    pub collection_name: Option<String>,
    pub query_text: String,
    pub query_type: String,
    pub execution_time_ms: Option<i64>,
    pub result_count: Option<i64>,
    pub error_message: Option<String>,
    pub created_at: String,
}

pub async fn insert_history(
    pool: &SqlitePool,
    connection_id: &str,
    database_name: &str,
    collection_name: Option<&str>,
    query_text: &str,
    query_type: &str,
    execution_time_ms: Option<i64>,
    result_count: Option<i64>,
    error_message: Option<&str>,
) -> Result<i64, AppError> {
    let result = sqlx::query(
        r#"INSERT INTO query_history (
            connection_id, database_name, collection_name,
            query_text, query_type, execution_time_ms,
            result_count, error_message
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(connection_id)
    .bind(database_name)
    .bind(collection_name)
    .bind(query_text)
    .bind(query_type)
    .bind(execution_time_ms)
    .bind(result_count)
    .bind(error_message)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;

    Ok(result.last_insert_rowid())
}

pub async fn list_history(
    pool: &SqlitePool,
    connection_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<HistoryRow>, AppError> {
    let rows = sqlx::query_as::<_, HistoryRow>(
        "SELECT * FROM query_history WHERE connection_id = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
    )
    .bind(connection_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(rows)
}

pub async fn search_history(
    pool: &SqlitePool,
    connection_id: &str,
    keyword: &str,
    limit: i64,
) -> Result<Vec<HistoryRow>, AppError> {
    let pattern = format!("%{keyword}%");
    let rows = sqlx::query_as::<_, HistoryRow>(
        "SELECT * FROM query_history WHERE connection_id = ? AND query_text LIKE ? ORDER BY created_at DESC LIMIT ?",
    )
    .bind(connection_id)
    .bind(&pattern)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(rows)
}

pub async fn clear_history(pool: &SqlitePool, connection_id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM query_history WHERE connection_id = ?")
        .bind(connection_id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}

/// 列出所有连接的执行记录 (按时间倒序). 上限 limit, 用于"执行记录" 面板跨连接展示.
pub async fn list_all_history(pool: &SqlitePool, limit: i64) -> Result<Vec<HistoryRow>, AppError> {
    let rows = sqlx::query_as::<_, HistoryRow>(
        "SELECT * FROM query_history ORDER BY created_at DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(rows)
}

/// 清空所有连接的执行记录.
pub async fn clear_all_history(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::query("DELETE FROM query_history")
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::database::init_test_db;

    #[tokio::test]
    async fn insert_and_list() {
        let pool = init_test_db().await;
        insert_history(
            &pool, "conn-1", "testdb", Some("users"),
            "db.users.find({})", "shell", Some(15), Some(10), None,
        ).await.unwrap();

        let rows = list_history(&pool, "conn-1", 50, 0).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].query_text, "db.users.find({})");
        assert_eq!(rows[0].result_count, Some(10));
    }

    #[tokio::test]
    async fn search_by_keyword() {
        let pool = init_test_db().await;
        insert_history(&pool, "conn-1", "testdb", None, "db.users.find({})", "shell", None, None, None).await.unwrap();
        insert_history(&pool, "conn-1", "testdb", None, "db.orders.find({})", "shell", None, None, None).await.unwrap();

        let rows = search_history(&pool, "conn-1", "users", 50).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].query_text.contains("users"));
    }

    #[tokio::test]
    async fn clear_removes_all() {
        let pool = init_test_db().await;
        insert_history(&pool, "conn-1", "testdb", None, "query1", "shell", None, None, None).await.unwrap();
        insert_history(&pool, "conn-1", "testdb", None, "query2", "shell", None, None, None).await.unwrap();

        clear_history(&pool, "conn-1").await.unwrap();
        let rows = list_history(&pool, "conn-1", 50, 0).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn list_respects_limit_offset() {
        let pool = init_test_db().await;
        for i in 0..5 {
            insert_history(&pool, "conn-1", "testdb", None, &format!("query{i}"), "shell", None, None, None).await.unwrap();
        }

        let rows = list_history(&pool, "conn-1", 2, 0).await.unwrap();
        assert_eq!(rows.len(), 2);

        let rows = list_history(&pool, "conn-1", 2, 3).await.unwrap();
        assert_eq!(rows.len(), 2);
    }
}

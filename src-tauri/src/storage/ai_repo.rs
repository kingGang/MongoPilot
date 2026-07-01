//! AI 会话 + 消息 + facts 持久化.
//!
//! 消息 payload 是**已序列化的 AgentMessage JSON**, repo 层不解析 —— 前端 store 直接把
//! `AgentMessage` 结构 JSON.stringify 传下来, 读取时前端 JSON.parse 回结构. 这样后端不需要
//! 跟踪 AgentMessage schema 的每一次演进, 消息字段变了也不用改 migration.

use serde::Serialize;
use sqlx::{Row, SqlitePool};

use crate::error::AppError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRow {
    pub id: String,
    pub title: String,
    pub connection_id: Option<String>,
    pub database: Option<String>,
    pub collection: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRow {
    pub id: i64,
    pub position: i64,
    /// 已序列化的 AgentMessage JSON, 前端 parse
    pub payload: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactRow {
    pub id: i64,
    pub scope: String,
    pub key: String,
    pub value: String,
    pub created_at: i64,
    pub updated_at: i64,
}

// ---- Conversations ----

pub async fn list_conversations(pool: &SqlitePool) -> Result<Vec<ConversationRow>, AppError> {
    let rows = sqlx::query(
        "SELECT id, title, connection_id, database_name, collection_name, created_at, updated_at
         FROM ai_conversations
         ORDER BY updated_at DESC
         LIMIT 200",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    Ok(rows
        .into_iter()
        .map(|r| ConversationRow {
            id: r.get("id"),
            title: r.get("title"),
            connection_id: r.get("connection_id"),
            database: r.get("database_name"),
            collection: r.get("collection_name"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        })
        .collect())
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_conversation(
    pool: &SqlitePool,
    id: &str,
    title: &str,
    connection_id: Option<&str>,
    database: Option<&str>,
    collection: Option<&str>,
    now_ms: i64,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO ai_conversations (id, title, connection_id, database_name, collection_name, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            title = excluded.title,
            connection_id = excluded.connection_id,
            database_name = excluded.database_name,
            collection_name = excluded.collection_name,
            updated_at = excluded.updated_at",
    )
    .bind(id)
    .bind(title)
    .bind(connection_id)
    .bind(database)
    .bind(collection)
    .bind(now_ms)
    .bind(now_ms)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(())
}

pub async fn update_conversation_title(
    pool: &SqlitePool,
    id: &str,
    title: &str,
    now_ms: i64,
) -> Result<(), AppError> {
    sqlx::query("UPDATE ai_conversations SET title = ?, updated_at = ? WHERE id = ?")
        .bind(title)
        .bind(now_ms)
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}

pub async fn touch_conversation(pool: &SqlitePool, id: &str, now_ms: i64) -> Result<(), AppError> {
    sqlx::query("UPDATE ai_conversations SET updated_at = ? WHERE id = ?")
        .bind(now_ms)
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}

pub async fn delete_conversation(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    // SQLite FK 需要显式开 pragma foreign_keys=ON; 保险起见手动删 messages
    sqlx::query("DELETE FROM ai_messages WHERE conversation_id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    sqlx::query("DELETE FROM ai_conversations WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}

pub async fn clear_conversation_messages(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM ai_messages WHERE conversation_id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}

// ---- Messages ----

pub async fn get_messages(
    pool: &SqlitePool,
    conversation_id: &str,
) -> Result<Vec<MessageRow>, AppError> {
    let rows = sqlx::query(
        "SELECT id, position, payload, created_at
         FROM ai_messages
         WHERE conversation_id = ?
         ORDER BY position ASC",
    )
    .bind(conversation_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    Ok(rows
        .into_iter()
        .map(|r| MessageRow {
            id: r.get("id"),
            position: r.get("position"),
            payload: r.get("payload"),
            created_at: r.get("created_at"),
        })
        .collect())
}

pub async fn append_message(
    pool: &SqlitePool,
    conversation_id: &str,
    position: i64,
    payload: &str,
    now_ms: i64,
) -> Result<i64, AppError> {
    let res = sqlx::query(
        "INSERT INTO ai_messages (conversation_id, position, payload, created_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind(conversation_id)
    .bind(position)
    .bind(payload)
    .bind(now_ms)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(res.last_insert_rowid())
}

// ---- Facts ----

pub async fn list_facts(pool: &SqlitePool, scopes: &[String]) -> Result<Vec<FactRow>, AppError> {
    if scopes.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = scopes.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, scope, fact_key, fact_value, created_at, updated_at
         FROM ai_facts
         WHERE scope IN ({placeholders})
         ORDER BY updated_at DESC"
    );
    let mut q = sqlx::query(&sql);
    for s in scopes {
        q = q.bind(s);
    }
    let rows = q.fetch_all(pool).await.map_err(AppError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| FactRow {
            id: r.get("id"),
            scope: r.get("scope"),
            key: r.get("fact_key"),
            value: r.get("fact_value"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        })
        .collect())
}

pub async fn upsert_fact(
    pool: &SqlitePool,
    scope: &str,
    key: &str,
    value: &str,
    now_ms: i64,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO ai_facts (scope, fact_key, fact_value, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(scope, fact_key) DO UPDATE SET
            fact_value = excluded.fact_value,
            updated_at = excluded.updated_at",
    )
    .bind(scope)
    .bind(key)
    .bind(value)
    .bind(now_ms)
    .bind(now_ms)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(())
}

pub async fn delete_fact(pool: &SqlitePool, scope: &str, key: &str) -> Result<bool, AppError> {
    let res = sqlx::query("DELETE FROM ai_facts WHERE scope = ? AND fact_key = ?")
        .bind(scope)
        .bind(key)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(res.rows_affected() > 0)
}

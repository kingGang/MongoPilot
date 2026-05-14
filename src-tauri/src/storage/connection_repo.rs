use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ConnectionRow {
    pub id: String,
    pub name: String,
    pub group_path: String,
    pub color: Option<String>,
    pub conn_type: String,
    pub host: String,
    pub port: i64,
    pub auth_type: String,
    pub username: Option<String>,
    pub password_encrypted: Option<String>,
    pub auth_db: Option<String>,
    pub replica_set: Option<String>,
    pub srv: i64,
    pub tls: i64,
    pub tls_ca_file: Option<String>,
    pub tls_cert_file: Option<String>,
    pub tls_key_file: Option<String>,
    pub tls_allow_invalid: i64,
    pub ssh_enabled: i64,
    pub ssh_host: Option<String>,
    pub ssh_port: Option<i64>,
    pub ssh_username: Option<String>,
    pub ssh_auth_type: Option<String>,
    pub ssh_password_encrypted: Option<String>,
    pub ssh_private_key: Option<String>,
    pub ssh_passphrase_encrypted: Option<String>,
    pub default_db: Option<String>,
    pub uri_options: Option<String>,
    pub read_only: Option<i64>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn list_connections(pool: &SqlitePool) -> Result<Vec<ConnectionRow>, AppError> {
    let rows = sqlx::query_as::<_, ConnectionRow>(
        "SELECT * FROM connections ORDER BY group_path, sort_order, name",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(rows)
}

pub async fn get_connection(pool: &SqlitePool, id: &str) -> Result<ConnectionRow, AppError> {
    let row = sqlx::query_as::<_, ConnectionRow>("SELECT * FROM connections WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound(format!("连接 {id} 不存在")))?;
    Ok(row)
}

pub async fn insert_connection(pool: &SqlitePool, row: &ConnectionRow) -> Result<(), AppError> {
    sqlx::query(
        r#"INSERT INTO connections (
            id, name, group_path, color, conn_type, host, port,
            auth_type, username, password_encrypted, auth_db,
            replica_set, srv, tls, tls_ca_file, tls_cert_file, tls_key_file, tls_allow_invalid,
            ssh_enabled, ssh_host, ssh_port, ssh_username, ssh_auth_type,
            ssh_password_encrypted, ssh_private_key, ssh_passphrase_encrypted,
            default_db, uri_options, read_only, sort_order
        ) VALUES (
            ?, ?, ?, ?, ?, ?, ?,
            ?, ?, ?, ?,
            ?, ?, ?, ?, ?, ?, ?,
            ?, ?, ?, ?, ?,
            ?, ?, ?,
            ?, ?, ?, ?
        )"#,
    )
    .bind(&row.id)
    .bind(&row.name)
    .bind(&row.group_path)
    .bind(&row.color)
    .bind(&row.conn_type)
    .bind(&row.host)
    .bind(row.port)
    .bind(&row.auth_type)
    .bind(&row.username)
    .bind(&row.password_encrypted)
    .bind(&row.auth_db)
    .bind(&row.replica_set)
    .bind(row.srv)
    .bind(row.tls)
    .bind(&row.tls_ca_file)
    .bind(&row.tls_cert_file)
    .bind(&row.tls_key_file)
    .bind(row.tls_allow_invalid)
    .bind(row.ssh_enabled)
    .bind(&row.ssh_host)
    .bind(&row.ssh_port)
    .bind(&row.ssh_username)
    .bind(&row.ssh_auth_type)
    .bind(&row.ssh_password_encrypted)
    .bind(&row.ssh_private_key)
    .bind(&row.ssh_passphrase_encrypted)
    .bind(&row.default_db)
    .bind(&row.uri_options)
    .bind(row.read_only.unwrap_or(0))
    .bind(row.sort_order)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(())
}

pub async fn update_connection(pool: &SqlitePool, row: &ConnectionRow) -> Result<(), AppError> {
    let result = sqlx::query(
        r#"UPDATE connections SET
            name = ?, group_path = ?, color = ?, conn_type = ?, host = ?, port = ?,
            auth_type = ?, username = ?, password_encrypted = ?, auth_db = ?,
            replica_set = ?, srv = ?, tls = ?, tls_ca_file = ?, tls_cert_file = ?,
            tls_key_file = ?, tls_allow_invalid = ?,
            ssh_enabled = ?, ssh_host = ?, ssh_port = ?, ssh_username = ?, ssh_auth_type = ?,
            ssh_password_encrypted = ?, ssh_private_key = ?, ssh_passphrase_encrypted = ?,
            default_db = ?, uri_options = ?, read_only = ?, sort_order = ?,
            updated_at = datetime('now')
        WHERE id = ?"#,
    )
    .bind(&row.name)
    .bind(&row.group_path)
    .bind(&row.color)
    .bind(&row.conn_type)
    .bind(&row.host)
    .bind(row.port)
    .bind(&row.auth_type)
    .bind(&row.username)
    .bind(&row.password_encrypted)
    .bind(&row.auth_db)
    .bind(&row.replica_set)
    .bind(row.srv)
    .bind(row.tls)
    .bind(&row.tls_ca_file)
    .bind(&row.tls_cert_file)
    .bind(&row.tls_key_file)
    .bind(row.tls_allow_invalid)
    .bind(row.ssh_enabled)
    .bind(&row.ssh_host)
    .bind(&row.ssh_port)
    .bind(&row.ssh_username)
    .bind(&row.ssh_auth_type)
    .bind(&row.ssh_password_encrypted)
    .bind(&row.ssh_private_key)
    .bind(&row.ssh_passphrase_encrypted)
    .bind(&row.default_db)
    .bind(&row.uri_options)
    .bind(row.read_only.unwrap_or(0))
    .bind(row.sort_order)
    .bind(&row.id)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("连接 {} 不存在", row.id)));
    }
    Ok(())
}

pub async fn delete_connection(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM connections WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("连接 {id} 不存在")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::database::init_test_db;

    fn make_test_row(id: &str, name: &str) -> ConnectionRow {
        ConnectionRow {
            id: id.to_string(),
            name: name.to_string(),
            group_path: "".to_string(),
            color: None,
            conn_type: "standalone".to_string(),
            host: "localhost".to_string(),
            port: 27017,
            auth_type: "none".to_string(),
            username: None,
            password_encrypted: None,
            auth_db: Some("admin".to_string()),
            replica_set: None,
            srv: 0,
            tls: 0,
            tls_ca_file: None,
            tls_cert_file: None,
            tls_key_file: None,
            tls_allow_invalid: 0,
            ssh_enabled: 0,
            ssh_host: None,
            ssh_port: Some(22),
            ssh_username: None,
            ssh_auth_type: Some("password".to_string()),
            ssh_password_encrypted: None,
            ssh_private_key: None,
            ssh_passphrase_encrypted: None,
            default_db: None,
            uri_options: None,
            sort_order: 0,
            read_only: Some(0),
            created_at: "".to_string(),
            updated_at: "".to_string(),
        }
    }

    #[tokio::test]
    async fn insert_and_get() {
        let pool = init_test_db().await;
        let row = make_test_row("conn-1", "测试服务器");
        insert_connection(&pool, &row).await.unwrap();

        let fetched = get_connection(&pool, "conn-1").await.unwrap();
        assert_eq!(fetched.name, "测试服务器");
        assert_eq!(fetched.host, "localhost");
        assert_eq!(fetched.port, 27017);
    }

    #[tokio::test]
    async fn list_empty() {
        let pool = init_test_db().await;
        let rows = list_connections(&pool).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn list_returns_sorted() {
        let pool = init_test_db().await;
        let mut row_b = make_test_row("b", "B 服务器");
        row_b.group_path = "prod".to_string();
        let row_a = make_test_row("a", "A 服务器");

        insert_connection(&pool, &row_b).await.unwrap();
        insert_connection(&pool, &row_a).await.unwrap();

        let rows = list_connections(&pool).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "a");
        assert_eq!(rows[1].id, "b");
    }

    #[tokio::test]
    async fn update_existing() {
        let pool = init_test_db().await;
        let mut row = make_test_row("conn-1", "旧名称");
        insert_connection(&pool, &row).await.unwrap();

        row.name = "新名称".to_string();
        row.host = "192.168.1.1".to_string();
        update_connection(&pool, &row).await.unwrap();

        let fetched = get_connection(&pool, "conn-1").await.unwrap();
        assert_eq!(fetched.name, "新名称");
        assert_eq!(fetched.host, "192.168.1.1");
    }

    #[tokio::test]
    async fn update_nonexistent_fails() {
        let pool = init_test_db().await;
        let row = make_test_row("no-such-id", "幽灵");
        let result = update_connection(&pool, &row).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_existing() {
        let pool = init_test_db().await;
        let row = make_test_row("conn-1", "待删除");
        insert_connection(&pool, &row).await.unwrap();

        delete_connection(&pool, "conn-1").await.unwrap();
        let result = get_connection(&pool, "conn-1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_nonexistent_fails() {
        let pool = init_test_db().await;
        let result = delete_connection(&pool, "no-such-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_nonexistent_fails() {
        let pool = init_test_db().await;
        let result = get_connection(&pool, "no-such-id").await;
        assert!(result.is_err());
    }
}

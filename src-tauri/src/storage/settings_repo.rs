use sqlx::SqlitePool;
use crate::error::AppError;

pub async fn get_setting(pool: &SqlitePool, key: &str) -> Result<Option<String>, AppError> {
    let row = sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(row)
}

pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO app_settings (key, value, updated_at) VALUES (?, ?, datetime('now')) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')"
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(())
}

pub async fn delete_setting(pool: &SqlitePool, key: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM app_settings WHERE key = ?")
        .bind(key)
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
    async fn set_and_get() {
        let pool = init_test_db().await;
        set_setting(&pool, "ai.provider", "claude").await.unwrap();
        let val = get_setting(&pool, "ai.provider").await.unwrap();
        assert_eq!(val, Some("claude".to_string()));
    }

    #[tokio::test]
    async fn get_nonexistent() {
        let pool = init_test_db().await;
        let val = get_setting(&pool, "nonexistent").await.unwrap();
        assert_eq!(val, None);
    }

    #[tokio::test]
    async fn upsert() {
        let pool = init_test_db().await;
        set_setting(&pool, "key1", "value1").await.unwrap();
        set_setting(&pool, "key1", "value2").await.unwrap();
        let val = get_setting(&pool, "key1").await.unwrap();
        assert_eq!(val, Some("value2".to_string()));
    }

    #[tokio::test]
    async fn delete_existing() {
        let pool = init_test_db().await;
        set_setting(&pool, "key1", "value1").await.unwrap();
        delete_setting(&pool, "key1").await.unwrap();
        let val = get_setting(&pool, "key1").await.unwrap();
        assert_eq!(val, None);
    }
}

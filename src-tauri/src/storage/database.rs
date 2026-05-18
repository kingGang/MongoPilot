use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::path::Path;
use std::str::FromStr;

use crate::error::AppError;

pub type DbPool = SqlitePool;

/// 初始化 SQLite 数据库：如需创建文件、执行迁移。
pub async fn init_db(app_data_dir: &Path) -> Result<DbPool, AppError> {
    let db_path = app_data_dir.join("mongopilot.db");
    std::fs::create_dir_all(app_data_dir)
        .map_err(|e| AppError::Database(sqlx::Error::Io(e)))?;

    let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let options = SqliteConnectOptions::from_str(&db_url)
        .map_err(AppError::Database)?
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .map_err(AppError::Database)?;

    run_migrations(&pool).await?;

    Ok(pool)
}

async fn run_migrations(pool: &SqlitePool) -> Result<(), AppError> {
    let migration_sql = include_str!("../../migrations/001_init.sql");
    sqlx::raw_sql(migration_sql)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    let migration_002 = include_str!("../../migrations/002_history.sql");
    sqlx::raw_sql(migration_002)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    let migration_003 = include_str!("../../migrations/003_settings.sql");
    sqlx::raw_sql(migration_003).execute(pool).await.map_err(AppError::Database)?;
    let migration_004 = include_str!("../../migrations/004_read_only.sql");
    sqlx::raw_sql(migration_004).execute(pool).await.ok(); // ok(): 列已存在时忽略
    let migration_005 = include_str!("../../migrations/005_scripts.sql");
    sqlx::raw_sql(migration_005)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}

#[cfg(test)]
pub async fn init_test_db() -> DbPool {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();

    run_migrations(&pool).await.unwrap();
    pool
}

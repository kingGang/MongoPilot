use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRow {
    pub id: String,
    pub name: String,
    pub folder_path: String,
    pub content: String,
    pub connection_id: Option<String>,
    pub database_name: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ScriptFolderRow {
    pub path: String,
    pub sort_order: i64,
    pub created_at: String,
}

pub async fn list_scripts(pool: &SqlitePool) -> Result<Vec<ScriptRow>, AppError> {
    sqlx::query_as::<_, ScriptRow>(
        r#"SELECT id, name, folder_path, content, connection_id, database_name,
                  sort_order, created_at, updated_at
           FROM scripts
           ORDER BY folder_path, sort_order, name"#,
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)
}

pub async fn get_script(pool: &SqlitePool, id: &str) -> Result<ScriptRow, AppError> {
    let row = sqlx::query_as::<_, ScriptRow>(
        r#"SELECT id, name, folder_path, content, connection_id, database_name,
                  sort_order, created_at, updated_at
           FROM scripts WHERE id = ?"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)?;
    row.ok_or_else(|| AppError::NotFound(format!("脚本 {id} 不存在")))
}

/// 插入新脚本 (id 由调用方生成, 通常 uuid)
pub async fn insert_script(pool: &SqlitePool, row: &ScriptRow) -> Result<(), AppError> {
    sqlx::query(
        r#"INSERT INTO scripts (id, name, folder_path, content, connection_id, database_name,
                                 sort_order, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, datetime('now'), datetime('now'))"#,
    )
    .bind(&row.id)
    .bind(&row.name)
    .bind(&row.folder_path)
    .bind(&row.content)
    .bind(&row.connection_id)
    .bind(&row.database_name)
    .bind(row.sort_order)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(())
}

/// 更新脚本的全部可编辑字段 (name/folder_path/content/connection_id/database_name/sort_order)
pub async fn update_script(pool: &SqlitePool, row: &ScriptRow) -> Result<(), AppError> {
    sqlx::query(
        r#"UPDATE scripts SET
              name = ?, folder_path = ?, content = ?,
              connection_id = ?, database_name = ?, sort_order = ?,
              updated_at = datetime('now')
           WHERE id = ?"#,
    )
    .bind(&row.name)
    .bind(&row.folder_path)
    .bind(&row.content)
    .bind(&row.connection_id)
    .bind(&row.database_name)
    .bind(row.sort_order)
    .bind(&row.id)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(())
}

pub async fn delete_script(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM scripts WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}

// ---- folders ----

pub async fn list_folders(pool: &SqlitePool) -> Result<Vec<ScriptFolderRow>, AppError> {
    sqlx::query_as::<_, ScriptFolderRow>(
        "SELECT path, sort_order, created_at FROM script_folders ORDER BY path",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)
}

pub async fn create_folder(pool: &SqlitePool, path: &str) -> Result<(), AppError> {
    if path.is_empty() {
        return Err(AppError::InvalidInput("文件夹路径不能为空".into()));
    }
    sqlx::query("INSERT OR IGNORE INTO script_folders (path) VALUES (?)")
        .bind(path)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}

/// 删除文件夹.
/// - cascade = true: 连同里面的脚本一起删
/// - cascade = false: 文件夹非空时返回错误
pub async fn delete_folder(pool: &SqlitePool, path: &str, cascade: bool) -> Result<(), AppError> {
    if path.is_empty() {
        return Err(AppError::InvalidInput("不能删除根目录".into()));
    }
    // 该文件夹及其所有子孙文件夹的路径前缀
    let prefix_like = format!("{path}/%");
    if !cascade {
        let count: (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*) FROM scripts WHERE folder_path = ? OR folder_path LIKE ?"#,
        )
        .bind(path)
        .bind(&prefix_like)
        .fetch_one(pool)
        .await
        .map_err(AppError::Database)?;
        if count.0 > 0 {
            return Err(AppError::InvalidInput(format!(
                "文件夹 {path} 内还有 {} 个脚本, 请先移走或使用强制删除",
                count.0
            )));
        }
    }
    // 删除该路径下/孙路径下的脚本
    sqlx::query("DELETE FROM scripts WHERE folder_path = ? OR folder_path LIKE ?")
        .bind(path)
        .bind(&prefix_like)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    // 删除该文件夹及其子文件夹
    sqlx::query("DELETE FROM script_folders WHERE path = ? OR path LIKE ?")
        .bind(path)
        .bind(&prefix_like)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}

/// 重命名文件夹 + 同步迁移该路径下/孙路径下所有脚本的 folder_path
pub async fn rename_folder(
    pool: &SqlitePool,
    old_path: &str,
    new_path: &str,
) -> Result<(), AppError> {
    if old_path.is_empty() || new_path.is_empty() {
        return Err(AppError::InvalidInput("路径不能为空".into()));
    }
    if old_path == new_path {
        return Ok(());
    }
    let old_prefix = format!("{old_path}/");
    let new_prefix = format!("{new_path}/");

    let mut tx = pool.begin().await.map_err(AppError::Database)?;

    // 重命名子文件夹
    sqlx::query(
        r#"UPDATE script_folders
           SET path = ? || substr(path, length(?) + 1)
           WHERE path LIKE ?"#,
    )
    .bind(&new_prefix)
    .bind(&old_prefix)
    .bind(format!("{old_prefix}%"))
    .execute(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    // 重命名自身
    sqlx::query("UPDATE script_folders SET path = ? WHERE path = ?")
        .bind(new_path)
        .bind(old_path)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    // 同步脚本: folder_path = old_path -> new_path
    sqlx::query("UPDATE scripts SET folder_path = ? WHERE folder_path = ?")
        .bind(new_path)
        .bind(old_path)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    // 子文件夹下的脚本
    sqlx::query(
        r#"UPDATE scripts
           SET folder_path = ? || substr(folder_path, length(?) + 1)
           WHERE folder_path LIKE ?"#,
    )
    .bind(&new_prefix)
    .bind(&old_prefix)
    .bind(format!("{old_prefix}%"))
    .execute(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;
    Ok(())
}

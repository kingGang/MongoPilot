use serde::Deserialize;
use tauri::State;

use crate::connection::manager::ConnectionManager;
use crate::error::AppError;
use crate::query::executor::{self, QueryResult};
use crate::storage::database::DbPool;
use crate::storage::history_repo::{self, HistoryRow};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunQueryRequest {
    pub connection_id: String,
    pub database: String,
    pub query_text: String,
    /// 分页 skip（第几条开始）
    pub skip: Option<u64>,
    /// 每页大小
    pub page_size: Option<i64>,
}

#[tauri::command]
pub async fn run_query(
    mgr: State<'_, ConnectionManager>,
    pool: State<'_, DbPool>,
    request: RunQueryRequest,
) -> Result<QueryResult, AppError> {
    // 只读检查：拦截写操作
    if mgr.is_read_only(&request.connection_id).await {
        let q = request.query_text.trim();
        let write_ops = ["insertOne(", "insertMany(", "updateOne(", "updateMany(",
            "deleteOne(", "deleteMany(", "replaceOne(", "dropUser(", "createUser(", "drop("];
        for op in &write_ops {
            if q.contains(op) {
                return Err(AppError::InvalidInput("只读连接：不允许执行写操作".into()));
            }
        }
    }

    let client = mgr.get_client(&request.connection_id).await?;

    let pagination = match (request.skip, request.page_size) {
        (Some(skip), Some(page_size)) => Some(executor::Pagination { skip, page_size }),
        _ => None,
    };

    let result = executor::execute_shell_query(
        &client,
        &request.database,
        &request.query_text,
        pagination,
    )
    .await;

    match &result {
        Ok(qr) => {
            let _ = history_repo::insert_history(
                &pool, &request.connection_id, &request.database, None,
                &request.query_text, "shell", Some(qr.execution_time_ms), Some(qr.count), None,
            ).await;
        }
        Err(e) => {
            let _ = history_repo::insert_history(
                &pool, &request.connection_id, &request.database, None,
                &request.query_text, "shell", None, None, Some(&e.to_string()),
            ).await;
        }
    }

    result
}

#[tauri::command]
pub async fn get_query_history(
    pool: State<'_, DbPool>,
    connection_id: String,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<HistoryRow>, AppError> {
    history_repo::list_history(&pool, &connection_id, limit.unwrap_or(50), offset.unwrap_or(0)).await
}

#[tauri::command]
pub async fn search_query_history(
    pool: State<'_, DbPool>,
    connection_id: String,
    keyword: String,
) -> Result<Vec<HistoryRow>, AppError> {
    history_repo::search_history(&pool, &connection_id, &keyword, 50).await
}

#[tauri::command]
pub async fn clear_query_history(
    pool: State<'_, DbPool>,
    connection_id: String,
) -> Result<(), AppError> {
    history_repo::clear_history(&pool, &connection_id).await
}

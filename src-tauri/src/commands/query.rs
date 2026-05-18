use mongodb::bson::Document;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

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
    /// 前端生成的 UUID，用于匹配异步 count 事件（可选，没传时后端不 emit）
    pub query_id: Option<String>,
}

/// `query:count-ready` 事件负载
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CountReadyPayload {
    query_id: String,
    total_count: i64,
}

#[tauri::command]
pub async fn run_query(
    app: AppHandle,
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

    // 非空 filter 的 find 会把 pending_count 传出, 这里 spawn 后台任务真正做
    // count_documents, 并通过 Tauri 事件把精确 total_count 推给前端.
    if let Ok(qr) = &result {
        if let (Some(pc), Some(qid)) = (qr.pending_count.clone(), request.query_id.clone()) {
            let client = client.clone();
            let db_name = request.database.clone();
            let app_handle = app.clone();
            tokio::spawn(async move {
                let coll = client
                    .database(&db_name)
                    .collection::<Document>(&pc.collection_name);
                let Ok(cnt) = coll.count_documents(pc.filter).await else {
                    let _ = app_handle.emit(
                        "query:count-ready",
                        CountReadyPayload { query_id: qid, total_count: -2 },
                    );
                    return;
                };
                let total = cnt as i64;
                let effective = match pc.user_limit {
                    Some(ul) => std::cmp::min(total, ul),
                    None => total,
                };
                let _ = app_handle.emit(
                    "query:count-ready",
                    CountReadyPayload { query_id: qid, total_count: effective },
                );
            });
        }
    }

    // QueryResult 序列化到前端时 pending_count 被 serde(skip) 掩掉, 不会外泄.
    result
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunScriptOpsRequest {
    pub connection_id: String,
    pub database: String,
    /// 前端脚本模式收集到的写操作, 每条已渲染成 `db.coll.method(JSON...)`
    pub statements: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRunSummary {
    pub total: usize,
    pub ok: usize,
    pub failed: usize,
    pub first_error: Option<String>,
    pub elapsed_ms: u64,
}

/// 脚本模式: 前端把整段命令式脚本在 webview 里跑一遍, 收集到的所有写操作
/// (每条已渲染成 `db.coll.method(JSON...)`) 通过这里顺序执行.
#[tauri::command]
pub async fn run_script_ops(
    mgr: State<'_, ConnectionManager>,
    pool: State<'_, DbPool>,
    request: RunScriptOpsRequest,
) -> Result<ScriptRunSummary, AppError> {
    if mgr.is_read_only(&request.connection_id).await {
        return Err(AppError::InvalidInput(
            "只读连接：不允许执行写操作".into(),
        ));
    }

    let client = mgr.get_client(&request.connection_id).await?;
    let start = std::time::Instant::now();
    let total = request.statements.len();
    let mut ok = 0usize;
    let mut failed = 0usize;
    let mut first_error: Option<String> = None;

    for stmt in &request.statements {
        match executor::execute_shell_query(&client, &request.database, stmt, None).await {
            Ok(_) => ok += 1,
            Err(e) => {
                failed += 1;
                if first_error.is_none() {
                    first_error = Some(e.to_string());
                }
            }
        }
    }

    let elapsed_ms = start.elapsed().as_millis() as u64;

    let _ = history_repo::insert_history(
        &pool,
        &request.connection_id,
        &request.database,
        None,
        &format!("[脚本] {total} 条写操作 (成功 {ok} / 失败 {failed})"),
        "script",
        Some(elapsed_ms as i64),
        Some(ok as i64),
        first_error.as_deref(),
    )
    .await;

    Ok(ScriptRunSummary {
        total,
        ok,
        failed,
        first_error,
        elapsed_ms,
    })
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

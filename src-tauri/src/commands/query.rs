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
    // 只读检查：拦截写操作 (shell 语法层 + aggregate 写入阶段)
    if mgr.is_read_only(&request.connection_id).await {
        if has_write_op(request.query_text.trim()) {
            return Err(AppError::InvalidInput("只读连接：不允许执行写操作".into()));
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

/// 拉取所有连接的执行记录, 由前端按 connection_id 分组展示.
#[tauri::command]
pub async fn list_all_query_history(
    pool: State<'_, DbPool>,
    limit: Option<i64>,
) -> Result<Vec<HistoryRow>, AppError> {
    history_repo::list_all_history(&pool, limit.unwrap_or(500)).await
}

/// 清空所有连接的执行记录.
#[tauri::command]
pub async fn clear_all_query_history(pool: State<'_, DbPool>) -> Result<(), AppError> {
    history_repo::clear_all_history(&pool).await
}

/// 判断 shell 查询是否含写操作 —— 用于只读连接的拦截.
///
/// 覆盖三类:
///   1) executor 已支持的集合方法 (insertOne/Many, updateOne/Many, deleteOne/Many, replaceOne)
///   2) 数据库级写命令 (createUser/dropUser, drop = db.coll.drop / db.dropDatabase 等)
///   3) aggregate 写入阶段 ($out, $merge) —— 这一类绕过 (1) 的方法名黑名单
///
/// 另外, 把 executor 当前还没分发但 mongosh 用户常用的写法也提前 deny:
///   findAndModify / findOneAndUpdate / findOneAndReplace / findOneAndDelete /
///   bulkWrite / createIndex / dropIndex / createCollection / renameCollection /
///   grantRoles / revokeRoles / createRole / dropRole / dropDatabase / mapReduce
/// —— 即使现在跑会因 "不支持的操作" 退出, 也直接给只读语义更明确的报错.
fn has_write_op(q: &str) -> bool {
    const METHOD_PREFIXES: &[&str] = &[
        // 已实现的集合写
        "insertOne(", "insertMany(", "updateOne(", "updateMany(",
        "deleteOne(", "deleteMany(", "replaceOne(",
        // 集合 / DB 级写
        "drop(", "dropDatabase(", "createCollection(", "renameCollection(",
        "createIndex(", "dropIndex(", "createIndexes(", "dropIndexes(", "reIndex(",
        // 用户 / 角色
        "createUser(", "dropUser(", "updateUser(",
        "createRole(", "dropRole(", "updateRole(",
        "grantRolesToUser(", "revokeRolesFromUser(",
        "grantRolesToRole(", "revokeRolesFromRole(",
        "grantPrivilegesToRole(", "revokePrivilegesFromRole(",
        // 这些 mongosh 调用即使 executor 不分发, 在只读语义下也直接 deny
        "findAndModify(", "findOneAndUpdate(", "findOneAndReplace(", "findOneAndDelete(",
        "bulkWrite(", "mapReduce(",
    ];
    for m in METHOD_PREFIXES {
        if q.contains(m) {
            return true;
        }
    }
    // aggregate 写入阶段: $out / $merge.
    // 必须排除 $mergeObjects (是 read 端聚合表达式) —— 通过 "$ident 后必须紧跟非字母"
    // 来区分 ($merge -> 写, $mergeObjects -> 读).
    contains_pipeline_stage(q, "$out") || contains_pipeline_stage(q, "$merge")
}

/// 检查 `q` 是否包含一个 pipeline stage 名 `stage`. 要求:
///   - 命中的 stage 后续字符不是 ASCII 字母 (否则会把 `$mergeObjects` 误判成 `$merge`)
///   - 不区分 stage 前面的字符 (常见前导是 `{`, `,`, ` ` 或 `"`)
fn contains_pipeline_stage(q: &str, stage: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = q[start..].find(stage) {
        let abs = start + pos;
        let after = q[abs + stage.len()..].chars().next();
        match after {
            Some(c) if c.is_ascii_alphabetic() => {
                start = abs + 1; // $mergeObjects 之类, 继续往后找
                continue;
            }
            _ => return true,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::has_write_op;

    #[test]
    fn detects_basic_collection_writes() {
        assert!(has_write_op("db.users.insertOne({name:'a'})"));
        assert!(has_write_op("db.users.deleteMany({})"));
        assert!(has_write_op("db.users.replaceOne({_id:1},{x:2})"));
    }

    #[test]
    fn detects_aggregate_out_and_merge_stages() {
        assert!(has_write_op(r#"db.users.aggregate([{ $out: "snapshot" }])"#));
        assert!(has_write_op(r#"db.users.aggregate([{$merge:{into:"snap"}}])"#));
    }

    #[test]
    fn does_not_flag_merge_objects_or_other_read_stages() {
        // $mergeObjects 是读端表达式, 不应误伤
        assert!(!has_write_op(r#"db.users.aggregate([{$group:{_id:"$g", merged:{$mergeObjects:"$$ROOT"}}}])"#));
        assert!(!has_write_op("db.users.find({})"));
        assert!(!has_write_op("db.users.countDocuments({})"));
        assert!(!has_write_op("show dbs"));
    }

    #[test]
    fn detects_user_and_index_admin_writes() {
        assert!(has_write_op(r#"db.createUser({user:"u",pwd:"p",roles:[]})"#));
        assert!(has_write_op(r#"db.dropUser("u")"#));
        assert!(has_write_op(r#"db.users.createIndex({k:1})"#));
        assert!(has_write_op(r#"db.users.dropIndex("k_1")"#));
        assert!(has_write_op(r#"db.users.findAndModify({query:{},update:{$set:{x:1}}})"#));
        assert!(has_write_op(r#"db.users.bulkWrite([{updateOne:{filter:{},update:{}}}])"#));
    }
}

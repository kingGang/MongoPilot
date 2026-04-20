use mongodb::bson::{doc, Document};
use serde::Serialize;
use tauri::State;

use crate::connection::manager::ConnectionManager;
use crate::error::AppError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub host: String,
    pub version: String,
    pub uptime: i64,
    pub connections: ConnectionStats,
    pub opcounters: OpCounters,
    pub memory: MemoryStats,
    pub storage_engine: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStats {
    pub current: i64,
    pub available: i64,
    pub total_created: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpCounters {
    pub insert: i64,
    pub query: i64,
    pub update: i64,
    pub delete: i64,
    pub getmore: i64,
    pub command: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStats {
    pub resident: i64,
    pub virtual_mem: i64,
    pub mapped: i64,
}

#[tauri::command]
pub async fn get_server_status(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
) -> Result<ServerStatus, AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let db = client.database("admin");

    let result = db
        .run_command(doc! { "serverStatus": 1 })
        .await
        .map_err(AppError::Mongo)?;

    let connections = result.get_document("connections").ok();
    let opcounters = result.get_document("opcounters").ok();
    let mem = result.get_document("mem").ok();
    let storage = result.get_document("storageEngine").ok();

    Ok(ServerStatus {
        host: result.get_str("host").unwrap_or("unknown").to_string(),
        version: result.get_str("version").unwrap_or("unknown").to_string(),
        uptime: result.get_i64("uptime").or_else(|_| result.get_i32("uptime").map(|v| v as i64)).unwrap_or(0),
        connections: ConnectionStats {
            current: get_i64_field(connections, "current"),
            available: get_i64_field(connections, "available"),
            total_created: get_i64_field(connections, "totalCreated"),
        },
        opcounters: OpCounters {
            insert: get_i64_field(opcounters, "insert"),
            query: get_i64_field(opcounters, "query"),
            update: get_i64_field(opcounters, "update"),
            delete: get_i64_field(opcounters, "delete"),
            getmore: get_i64_field(opcounters, "getmore"),
            command: get_i64_field(opcounters, "command"),
        },
        memory: MemoryStats {
            resident: get_i64_field(mem, "resident"),
            virtual_mem: get_i64_field(mem, "virtual"),
            mapped: get_i64_field(mem, "mapped"),
        },
        storage_engine: storage
            .and_then(|s| s.get_str("name").ok())
            .unwrap_or("unknown")
            .to_string(),
    })
}

fn get_i64_field(doc: Option<&Document>, key: &str) -> i64 {
    doc.and_then(|d| d.get_i64(key).ok().or_else(|| d.get_i32(key).ok().map(|v| v as i64)))
        .unwrap_or(0)
}

#[tauri::command]
pub async fn explain_query(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection: String,
    filter: serde_json::Value,
) -> Result<serde_json::Value, AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let db = client.database(&database);

    let filter_doc: Document = serde_json::from_value(filter)
        .map_err(|e| AppError::InvalidInput(format!("无效的过滤条件: {e}")))?;

    let cmd = doc! {
        "explain": {
            "find": &collection,
            "filter": filter_doc,
        },
        "verbosity": "executionStats"
    };

    let result = db.run_command(cmd).await.map_err(AppError::Mongo)?;

    serde_json::to_value(&result)
        .map_err(|e| AppError::InvalidInput(format!("序列化失败: {e}")))
}

/// 从 shell 语法 `db.coll.find({...}).sort({...}).limit(N)` 中提取集合/过滤/排序/limit,
/// 并在同一数据库下跑 `explain` 命令 (verbosity: executionStats).
#[tauri::command]
pub async fn explain_shell_query(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    query_text: String,
) -> Result<serde_json::Value, AppError> {
    use crate::query::executor::{extract_parens, parse_chained_arg, parse_json_arg};

    let query = query_text.trim();
    let query = query.strip_prefix("db.").ok_or_else(|| {
        AppError::InvalidInput("查询必须以 db. 开头，例如 db.collection.find({})".into())
    })?;

    // 解析 `getCollection("x")` 或直接的 `collName`
    let (collection, rest) = if let Some(rest) = query.strip_prefix("getCollection(") {
        let gc_end = rest.find(')').ok_or_else(|| {
            AppError::InvalidInput("getCollection() 括号不匹配".into())
        })?;
        let inner = &rest[..gc_end];
        let coll = inner.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
        let after = &rest[gc_end + 1..];
        let after = after.strip_prefix('.').unwrap_or(after);
        (coll, after)
    } else {
        let dot_pos = query.find('.').ok_or_else(|| {
            AppError::InvalidInput("语法错误：缺少 .find()".into())
        })?;
        let coll = query[..dot_pos].to_string();
        (coll, &query[dot_pos + 1..])
    };

    if !rest.starts_with("find(") {
        return Err(AppError::InvalidInput(
            "目前 Explain 仅支持 find 查询 (find / findOne 暂不支持其他)".into(),
        ));
    }

    let filter_str = extract_parens(rest, "find")?;
    let filter: Document = parse_json_arg(&filter_str)?;

    let after_find = &rest[rest.find(')').unwrap_or(rest.len()) + 1..];
    let projection = parse_chained_arg(after_find, ".projection(");
    let sort = parse_chained_arg(after_find, ".sort(");
    let skip = parse_chained_arg(after_find, ".skip(");
    let limit = parse_chained_arg(after_find, ".limit(");

    let mut explain_args = doc! {
        "find": &collection,
        "filter": filter,
    };
    if let Some(proj_str) = projection {
        let d: Document = parse_json_arg(&proj_str)?;
        if !d.is_empty() { explain_args.insert("projection", d); }
    }
    if let Some(sort_str) = sort {
        let d: Document = parse_json_arg(&sort_str)?;
        if !d.is_empty() { explain_args.insert("sort", d); }
    }
    if let Some(s) = skip.and_then(|v| v.trim().parse::<i64>().ok()) {
        explain_args.insert("skip", s);
    }
    if let Some(l) = limit.and_then(|v| v.trim().parse::<i64>().ok()) {
        explain_args.insert("limit", l);
    }

    let cmd = doc! {
        "explain": explain_args,
        "verbosity": "executionStats",
    };

    let client = mgr.get_client(&connection_id).await?;
    let db = client.database(&database);
    let result = db.run_command(cmd).await.map_err(AppError::Mongo)?;

    serde_json::to_value(&result)
        .map_err(|e| AppError::InvalidInput(format!("序列化失败: {e}")))
}

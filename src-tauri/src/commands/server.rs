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

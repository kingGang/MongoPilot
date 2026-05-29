use futures::future::join_all;
use mongodb::bson::{doc, Bson, Document};
use serde::Serialize;
use tauri::State;

use crate::connection::manager::ConnectionManager;
use crate::error::AppError;

/// 从 BSON Document 中取数值，兼容 i32/i64/f64
fn get_num(doc: &Document, key: &str) -> i64 {
    match doc.get(key) {
        Some(Bson::Int64(v)) => *v,
        Some(Bson::Int32(v)) => *v as i64,
        Some(Bson::Double(v)) => *v as i64,
        _ => 0,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseInfo {
    pub name: String,
    pub size_on_disk: i64,
    pub empty: bool,
    pub collection_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionInfo {
    pub name: String,
    pub collection_type: String,
    pub count: i64,
    pub size: i64,
}

#[tauri::command]
pub async fn list_databases(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
) -> Result<Vec<DatabaseInfo>, AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let dbs = client
        .list_databases()
        .await
        .map_err(AppError::Mongo)?;

    // 每个 DB 取 collection_count 是独立的 RTT, 并行发起避免 N 次串行等待.
    let count_futs = dbs.iter().map(|db| {
        let client = client.clone();
        let name = db.name.clone();
        async move {
            client
                .database(&name)
                .list_collection_names()
                .await
                .map(|c| c.len() as i64)
                .unwrap_or(0)
        }
    });
    let counts = join_all(count_futs).await;

    let result = dbs
        .into_iter()
        .zip(counts)
        .map(|(db, coll_count)| DatabaseInfo {
            name: db.name,
            size_on_disk: db.size_on_disk as i64,
            empty: db.size_on_disk == 0,
            collection_count: coll_count,
        })
        .collect();

    Ok(result)
}

#[tauri::command]
pub async fn list_collections(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
) -> Result<Vec<CollectionInfo>, AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let db = client.database(&database);
    let collection_names = db
        .list_collection_names()
        .await
        .map_err(AppError::Mongo)?;

    // 每个集合一个 collStats 命令 (count + size 都在返回里, 无需再单独 count).
    // 并行 fan-out 避免大库 50+ 集合时几秒钟的串行等待.
    let info_futs = collection_names.into_iter().map(|name| {
        let db = db.clone();
        async move {
            let stats = db
                .run_command(doc! { "collStats": &name })
                .await
                .ok();
            let (count, size) = match stats {
                Some(d) => (get_num(&d, "count"), get_num(&d, "size")),
                None => (0, 0),
            };
            CollectionInfo {
                name,
                collection_type: "collection".to_string(),
                count,
                size,
            }
        }
    });
    let result = join_all(info_futs).await;

    Ok(result)
}

#[tauri::command]
pub async fn drop_database(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
) -> Result<(), AppError> {
    if mgr.is_read_only(&connection_id).await {
        return Err(AppError::InvalidInput("只读连接: 不允许删除数据库".into()));
    }
    let client = mgr.get_client(&connection_id).await?;
    client.database(&database).drop().await.map_err(AppError::Mongo)?;
    Ok(())
}

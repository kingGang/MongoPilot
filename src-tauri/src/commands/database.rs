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

    let mut result = Vec::new();
    for db in dbs {
        let coll_count = client
            .database(&db.name)
            .list_collection_names()
            .await
            .map(|c| c.len() as i64)
            .unwrap_or(0);

        result.push(DatabaseInfo {
            name: db.name,
            size_on_disk: db.size_on_disk as i64,
            empty: db.size_on_disk == 0,
            collection_count: coll_count,
        });
    }

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

    let mut result = Vec::new();
    for name in collection_names {
        let coll = db.collection::<Document>(&name);
        let count = coll
            .estimated_document_count()
            .await
            .unwrap_or(0) as i64;

        let size = db
            .run_command(doc! { "collStats": &name })
            .await
            .ok()
            .map(|d| get_num(&d, "size"))
            .unwrap_or(0);

        result.push(CollectionInfo {
            name,
            collection_type: "collection".to_string(),
            count,
            size,
        });
    }

    Ok(result)
}

#[tauri::command]
pub async fn drop_database(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
) -> Result<(), AppError> {
    let client = mgr.get_client(&connection_id).await?;
    client.database(&database).drop().await.map_err(AppError::Mongo)?;
    Ok(())
}

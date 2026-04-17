use mongodb::bson::{doc, Bson, Document};
use mongodb::IndexModel;
use serde::{Deserialize, Serialize};
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
pub struct CollectionStats {
    pub document_count: i64,
    pub total_size: i64,
    pub avg_document_size: i64,
    pub index_count: i64,
    pub total_index_size: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexInfo {
    pub name: String,
    pub keys: serde_json::Value,
    pub unique: bool,
    pub sparse: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIndexOptions {
    pub unique: Option<bool>,
    pub sparse: Option<bool>,
    pub name: Option<String>,
    pub expire_after_seconds: Option<i64>,
}

#[tauri::command]
pub async fn create_collection(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection_name: String,
) -> Result<(), AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let db = client.database(&database);
    db.create_collection(&collection_name)
        .await
        .map_err(AppError::Mongo)?;
    Ok(())
}

#[tauri::command]
pub async fn drop_collection(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection_name: String,
) -> Result<(), AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let coll = client
        .database(&database)
        .collection::<Document>(&collection_name);
    coll.drop().await.map_err(AppError::Mongo)?;
    Ok(())
}

#[tauri::command]
pub async fn get_collection_stats(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection_name: String,
) -> Result<CollectionStats, AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let db = client.database(&database);

    let result = db
        .run_command(doc! { "collStats": &collection_name })
        .await
        .map_err(AppError::Mongo)?;

    let count = get_num(&result, "count");
    let size = get_num(&result, "size");
    let avg_size = get_num(&result, "avgObjSize");
    let n_indexes = get_num(&result, "nindexes");
    let index_size = get_num(&result, "totalIndexSize");

    Ok(CollectionStats {
        document_count: count,
        total_size: size,
        avg_document_size: avg_size,
        index_count: n_indexes,
        total_index_size: index_size,
    })
}

#[tauri::command]
pub async fn list_indexes(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection_name: String,
) -> Result<Vec<IndexInfo>, AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let coll = client
        .database(&database)
        .collection::<Document>(&collection_name);

    let mut cursor = coll.list_indexes().await.map_err(AppError::Mongo)?;

    let mut indexes = Vec::new();
    use futures::StreamExt;
    while let Some(index) = cursor.next().await {
        let index = index.map_err(AppError::Mongo)?;
        let keys_json = serde_json::to_value(&index.keys)
            .unwrap_or(serde_json::Value::Object(Default::default()));

        let opts = index.options.unwrap_or_default();
        indexes.push(IndexInfo {
            name: opts.name.unwrap_or_else(|| "unknown".to_string()),
            keys: keys_json,
            unique: opts.unique.unwrap_or(false),
            sparse: opts.sparse.unwrap_or(false),
        });
    }

    Ok(indexes)
}

#[tauri::command]
pub async fn create_index(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection_name: String,
    keys: serde_json::Value,
    options: Option<CreateIndexOptions>,
) -> Result<String, AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let coll = client
        .database(&database)
        .collection::<Document>(&collection_name);

    let keys_doc: Document = serde_json::from_value(keys)
        .map_err(|e| AppError::InvalidInput(format!("无效的索引键: {e}")))?;

    let index_options = options.map(|opts| {
        let mut idx_opts = mongodb::options::IndexOptions::default();
        idx_opts.unique = opts.unique;
        idx_opts.sparse = opts.sparse;
        idx_opts.name = opts.name;
        idx_opts.expire_after = opts
            .expire_after_seconds
            .map(|ttl| std::time::Duration::from_secs(ttl as u64));
        idx_opts
    });

    let index_model = IndexModel::builder()
        .keys(keys_doc)
        .options(index_options)
        .build();

    let result = coll
        .create_index(index_model)
        .await
        .map_err(AppError::Mongo)?;

    Ok(result.index_name)
}

#[tauri::command]
pub async fn drop_index(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection_name: String,
    index_name: String,
) -> Result<(), AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let coll = client
        .database(&database)
        .collection::<Document>(&collection_name);

    coll.drop_index(index_name)
        .await
        .map_err(AppError::Mongo)?;

    Ok(())
}

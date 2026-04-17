use mongodb::bson::{doc, oid::ObjectId, Document};
use tauri::State;

use crate::connection::manager::ConnectionManager;
use crate::error::AppError;

#[tauri::command]
pub async fn insert_document(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection: String,
    document: serde_json::Value,
) -> Result<String, AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let coll = client.database(&database).collection::<Document>(&collection);

    let doc: Document = serde_json::from_value(document)
        .map_err(|e| AppError::InvalidInput(format!("无效的文档 JSON: {e}")))?;

    let result = coll.insert_one(doc).await.map_err(AppError::Mongo)?;

    Ok(format!("{}", result.inserted_id))
}

#[tauri::command]
pub async fn update_document(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection: String,
    id: String,
    document: serde_json::Value,
) -> Result<(), AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let coll = client.database(&database).collection::<Document>(&collection);

    let mut doc: Document = serde_json::from_value(document)
        .map_err(|e| AppError::InvalidInput(format!("无效的文档 JSON: {e}")))?;

    // 移除 _id 字段（不能更新 _id）
    doc.remove("_id");

    let filter = if let Ok(oid) = ObjectId::parse_str(&id) {
        doc! { "_id": oid }
    } else {
        doc! { "_id": &id }
    };

    let result = coll
        .replace_one(filter, doc)
        .await
        .map_err(AppError::Mongo)?;

    if result.matched_count == 0 {
        return Err(AppError::NotFound(format!("文档 {id} 不存在")));
    }

    Ok(())
}

#[tauri::command]
pub async fn delete_document(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection: String,
    id: String,
) -> Result<(), AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let coll = client.database(&database).collection::<Document>(&collection);

    let filter = if let Ok(oid) = ObjectId::parse_str(&id) {
        doc! { "_id": oid }
    } else {
        doc! { "_id": &id }
    };

    let result = coll.delete_one(filter).await.map_err(AppError::Mongo)?;

    if result.deleted_count == 0 {
        return Err(AppError::NotFound(format!("文档 {id} 不存在")));
    }

    Ok(())
}

#[tauri::command]
pub async fn delete_documents(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection: String,
    filter: serde_json::Value,
) -> Result<i64, AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let coll = client.database(&database).collection::<Document>(&collection);

    let filter_doc: Document = serde_json::from_value(filter)
        .map_err(|e| AppError::InvalidInput(format!("无效的过滤条件: {e}")))?;

    let result = coll
        .delete_many(filter_doc)
        .await
        .map_err(AppError::Mongo)?;

    Ok(result.deleted_count as i64)
}

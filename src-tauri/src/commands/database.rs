use std::collections::HashMap;

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

    // 只返 name/sizeOnDisk, collectionCount 用 -1 表示"未加载" —— 让前端立刻能画树.
    // 之前每个 DB fan-out 一次 list_collection_names, 高 RTT 网络下累计几秒等待, 是打开
    // 连接后长时间转圈的主因. 现在改由前端在后台再调 count_database_collections 补数.
    let result = dbs
        .into_iter()
        .map(|db| DatabaseInfo {
            name: db.name,
            size_on_disk: db.size_on_disk as i64,
            empty: db.size_on_disk == 0,
            collection_count: -1,
        })
        .collect();

    Ok(result)
}

/// 并行取所有 DB 的集合数, 单次调用返回 { dbName -> collectionCount } 的 map.
/// 用于 list_databases 返回后的后台补数, 不阻塞初始 UI 渲染.
#[tauri::command]
pub async fn count_database_collections(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
) -> Result<HashMap<String, i64>, AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let names = client
        .list_database_names()
        .await
        .map_err(AppError::Mongo)?;

    let futs = names.into_iter().map(|name| {
        let client = client.clone();
        async move {
            let count = client
                .database(&name)
                .list_collection_names()
                .await
                .map(|c| c.len() as i64)
                .unwrap_or(0);
            (name, count)
        }
    });
    let pairs = join_all(futs).await;
    Ok(pairs.into_iter().collect())
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameDatabaseSummary {
    pub copied_collections: i64,
    pub copied_documents: i64,
}

/// 重命名数据库. MongoDB 没有原生 rename database 命令: 把旧库每个集合
/// (文档 + 非 _id_ 索引) 复制到新库名, 全部成功后再 drop 旧库.
/// 系统集合 (system.*) 跳过. 大库会较慢 —— 本质是一次数据拷贝.
#[tauri::command]
pub async fn rename_database(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    old_name: String,
    new_name: String,
) -> Result<RenameDatabaseSummary, AppError> {
    if mgr.is_read_only(&connection_id).await {
        return Err(AppError::InvalidInput("只读连接: 不允许重命名数据库".into()));
    }
    if new_name.trim().is_empty() {
        return Err(AppError::InvalidInput("新数据库名不能为空".into()));
    }
    if old_name == new_name {
        return Err(AppError::InvalidInput("新数据库名不能与原名相同".into()));
    }
    // 库名非法字符 (与前端校验一致)
    if new_name.contains(|c: char| "/\\. \"$*<>:|?".contains(c)) {
        return Err(AppError::InvalidInput(
            "数据库名不能含 / \\ . \" $ * < > : | ? 及空格".into(),
        ));
    }
    let client = mgr.get_client(&connection_id).await?;
    let existing_dbs = client
        .list_database_names()
        .await
        .map_err(AppError::Mongo)?;
    if existing_dbs.iter().any(|n| n == &new_name) {
        return Err(AppError::InvalidInput(format!(
            "数据库 \"{new_name}\" 已存在, 请换一个名字"
        )));
    }

    let old_db = client.database(&old_name);
    let new_db = client.database(&new_name);
    let coll_names = old_db
        .list_collection_names()
        .await
        .map_err(AppError::Mongo)?;

    let mut copied_collections = 0i64;
    let mut copied_documents = 0i64;
    for cname in &coll_names {
        if cname.starts_with("system.") {
            continue; // 系统集合不迁移
        }
        // 先建空集合, 保证无文档的集合也迁过去
        new_db
            .create_collection(cname)
            .await
            .map_err(AppError::Mongo)?;
        let src = old_db.collection::<Document>(cname);
        let dst = new_db.collection::<Document>(cname);
        copied_documents +=
            crate::commands::collection::copy_collection_contents(&src, &dst).await?;
        copied_collections += 1;
    }
    // 全部复制完成后再删旧库
    old_db.drop().await.map_err(AppError::Mongo)?;

    Ok(RenameDatabaseSummary {
        copied_collections,
        copied_documents,
    })
}

use std::collections::HashMap;

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
    /// MongoDB 4.2 之前生效, 之后 driver 忽略此选项 (后台构建已成默认).
    pub background: Option<bool>,
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
        idx_opts.background = opts.background;
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

/// 重建集合所有索引: `db.runCommand({reIndex: <collName>})`.
/// 注意: 单节点/独立 mongod 可用; 副本集/分片集群上 reIndex 已弃用并要求 stop 副本.
#[tauri::command]
pub async fn re_index(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection_name: String,
) -> Result<(), AppError> {
    if mgr.is_read_only(&connection_id).await {
        return Err(AppError::InvalidInput("只读连接: 不允许重建索引".into()));
    }
    let client = mgr.get_client(&connection_id).await?;
    let db = client.database(&database);
    db.run_command(mongodb::bson::doc! { "reIndex": &collection_name })
        .await
        .map_err(AppError::Mongo)?;
    Ok(())
}

/// 查询单个索引的详细信息. 等效于前端那段 `getCollectionIndexInfo` 函数:
/// 把 listIndexes 的 index 定义 + collStats 的 indexSize / indexDetails + $indexStats 的命中统计合并成一个 doc.
#[tauri::command]
pub async fn get_index_info(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection_name: String,
    index_name: String,
) -> Result<Document, AppError> {
    use futures::StreamExt;
    let client = mgr.get_client(&connection_id).await?;
    let db = client.database(&database);
    let coll = db.collection::<Document>(&collection_name);

    // 1) listIndexes -> 找到 name 匹配的 index 定义文档
    let list_doc = db
        .run_command(mongodb::bson::doc! { "listIndexes": &collection_name })
        .await
        .map_err(AppError::Mongo)?;
    let mut result = Document::new();
    if let Ok(cursor) = list_doc.get_document("cursor") {
        if let Ok(batch) = cursor.get_array("firstBatch") {
            for b in batch {
                if let Bson::Document(idx) = b {
                    if idx.get_str("name").ok() == Some(&index_name) {
                        for (k, v) in idx.iter() {
                            result.insert(k.clone(), v.clone());
                        }
                        break;
                    }
                }
            }
        }
    }

    // 2) collStats { indexDetails: true, indexDetailsName: <idxName> }
    //    取 indexSize / indexDetails / (分片场景) shards
    let stats_result = db
        .run_command(mongodb::bson::doc! {
            "collStats": &collection_name,
            "indexDetails": true,
            "indexDetailsName": &index_name,
        })
        .await
        .ok();

    if let Some(stats) = stats_result {
        if let Ok(sizes) = stats.get_document("indexSizes") {
            if let Some(size_val) = sizes.get(&index_name) {
                result.insert("indexSize", size_val.clone());
            }
        }
        if let Ok(details) = stats.get_document("indexDetails") {
            if let Some(d) = details.get(&index_name) {
                result.insert("index details", d.clone());
            }
        } else if let Ok(shards) = stats.get_document("shards") {
            // 分片场景: 把每个 shard 的 indexSize / indexDetails 重组到 "index details"
            let mut shard_block = Document::new();
            for (shard_name, shard_val) in shards.iter() {
                if let Bson::Document(shard) = shard_val {
                    let mut entry = Document::new();
                    entry.insert("name", index_name.clone());
                    if let Ok(sizes) = shard.get_document("indexSizes") {
                        if let Some(s) = sizes.get(&index_name) {
                            entry.insert("indexSize", s.clone());
                        }
                    }
                    if let Ok(details) = shard.get_document("indexDetails") {
                        if let Some(Bson::Document(d)) = details.get(&index_name) {
                            for (dk, dv) in d.iter() {
                                entry.insert(dk.clone(), dv.clone());
                            }
                        }
                    }
                    shard_block.insert(shard_name.clone(), entry);
                }
            }
            if !shard_block.is_empty() {
                result.insert("index details", shard_block);
            }
        }
    }

    // 3) $indexStats: 取该索引的命中统计 (可能多个 shard 各一条)
    let mut usage_arr: Vec<Bson> = Vec::new();
    if let Ok(mut agg_cursor) = coll
        .aggregate(vec![mongodb::bson::doc! { "$indexStats": {} }])
        .await
    {
        while let Some(item) = agg_cursor.next().await {
            if let Ok(d) = item {
                if d.get_str("name").ok() == Some(&index_name) {
                    usage_arr.push(Bson::Document(d));
                }
            }
        }
    }
    if !usage_arr.is_empty() {
        result.insert("usage stats", Bson::Array(usage_arr));
    }

    Ok(result)
}

/// BSON 值是否"空" (lodash _.isEmpty 语义):
/// Null / "" / [] / {} 都算空; 数字/布尔/非空字符串/非空容器不算空.
fn bson_is_empty(b: &Bson) -> bool {
    match b {
        Bson::Null => true,
        Bson::String(s) => s.is_empty(),
        Bson::Document(d) => d.is_empty(),
        Bson::Array(a) => a.is_empty(),
        _ => false,
    }
}

fn bson_num_to_i64(b: Option<&Bson>) -> i64 {
    match b {
        Some(Bson::Int64(n)) => *n,
        Some(Bson::Int32(n)) => *n as i64,
        Some(Bson::Double(n)) => *n as i64,
        _ => 0,
    }
}

/// 格式化 BSON DateTime 为 ISO 8601 字符串 (用于 accesses.since 展示)
fn bson_dt_iso(b: Option<&Bson>) -> String {
    if let Some(Bson::DateTime(dt)) = b {
        let millis = dt.timestamp_millis();
        let secs = millis / 1000;
        let nsecs = ((millis % 1000) * 1_000_000) as u32;
        if let Some(d) = chrono::DateTime::from_timestamp(secs, nsecs) {
            return d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        }
    }
    "?".to_string()
}

/// 列出某个集合的所有索引, 每条索引返回一个汇总文档:
///   name / key / type / size / ns / (accesses 或 "usage stats") / properties / v / host
/// 等效于前端那段 `getCollectionIndexes(col)` 脚本.
#[tauri::command]
pub async fn get_collection_indexes(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection_name: String,
) -> Result<Vec<Document>, AppError> {
    use futures::StreamExt;
    let client = mgr.get_client(&connection_id).await?;
    let db = client.database(&database);
    let coll = db.collection::<Document>(&collection_name);

    // 1) listIndexes -> 全部索引定义
    let list_doc = db
        .run_command(doc! { "listIndexes": &collection_name })
        .await
        .map_err(AppError::Mongo)?;
    let index_defs: Vec<Document> = list_doc
        .get_document("cursor")
        .ok()
        .and_then(|c| c.get_array("firstBatch").ok().cloned())
        .map(|arr| {
            arr.into_iter()
                .filter_map(|b| match b {
                    Bson::Document(d) => Some(d),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    // 2) $indexStats -> 按 name 分组 (副本集每个节点一条)
    let mut stats_by_name: HashMap<String, Vec<Document>> = HashMap::new();
    if let Ok(mut cursor) = coll.aggregate(vec![doc! { "$indexStats": {} }]).await {
        while let Some(item) = cursor.next().await {
            if let Ok(d) = item {
                if let Ok(name) = d.get_str("name") {
                    stats_by_name.entry(name.to_string()).or_default().push(d);
                }
            }
        }
    }

    // 3) collStats.indexSizes
    let mut index_sizes: HashMap<String, i64> = HashMap::new();
    if let Ok(stats) = db
        .run_command(doc! { "collStats": &collection_name })
        .await
    {
        if let Ok(sizes) = stats.get_document("indexSizes") {
            for (k, v) in sizes.iter() {
                index_sizes.insert(k.clone(), bson_num_to_i64(Some(v)));
            }
        }
    }

    // 4) 汇总
    let default_ns = format!("{}.{}", database, collection_name);
    const COMMON: &[&str] = &["name", "key", "type", "size", "ns", "accesses", "usage stats"];
    const TRAILING: &[&str] = &["v", "host"];

    let mut results: Vec<Document> = Vec::with_capacity(index_defs.len());
    for info in index_defs {
        let name = info.get_str("name").unwrap_or("").to_string();

        // type: 第一个 string 值的 key value (text/hashed/2d/2dsphere), 默认 regular
        let type_str = info
            .get_document("key")
            .ok()
            .and_then(|key| {
                key.values().find_map(|v| match v {
                    Bson::String(s) => Some(s.clone()),
                    _ => None,
                })
            })
            .unwrap_or_else(|| "regular".to_string())
            .to_uppercase();

        let size = index_sizes.get(&name).copied().unwrap_or(0);

        // 构建 row, 保持插入顺序: name -> key -> type -> size -> ns -> accesses/usage stats -> properties -> v -> host
        let mut row = Document::new();
        row.insert("name", name.clone());
        if let Ok(key) = info.get_document("key") {
            row.insert("key", key.clone());
        }
        row.insert("type", type_str);
        row.insert("size", size);
        row.insert(
            "ns",
            info.get_str("ns")
                .map(String::from)
                .unwrap_or_else(|_| default_ns.clone()),
        );

        // accesses / usage stats
        let stats_list = stats_by_name.remove(&name).unwrap_or_default();
        if stats_list.is_empty() {
            row.insert("usage stats", "not available");
        } else if stats_list.len() == 1 {
            // 单节点: 展开为 accesses="<ops> since <since>"
            let s = &stats_list[0];
            if let Ok(acc) = s.get_document("accesses") {
                let ops = bson_num_to_i64(acc.get("ops"));
                let since = bson_dt_iso(acc.get("since"));
                row.insert("accesses", format!("{} since {}", ops, since));
            }
        } else {
            // 多节点 (sharded / replica): "usage stats": { "0": {host, accesses}, "1": ... }
            let mut usage = Document::new();
            for (i, s) in stats_list.iter().enumerate() {
                let mut entry = Document::new();
                if let Ok(h) = s.get_str("host") {
                    entry.insert("host", h.to_string());
                }
                if let Ok(acc) = s.get_document("accesses") {
                    let ops = bson_num_to_i64(acc.get("ops"));
                    let since = bson_dt_iso(acc.get("since"));
                    entry.insert("accesses", format!("{} since {}", ops, since));
                }
                usage.insert(i.to_string(), entry);
            }
            row.insert("usage stats", usage);
        }

        // properties: info 里不在 COMMON / TRAILING 的字段
        let mut properties = Document::new();
        for (k, v) in info.iter() {
            if COMMON.contains(&k.as_str()) || TRAILING.contains(&k.as_str()) {
                continue;
            }
            properties.insert(k.clone(), v.clone());
        }
        if !properties.is_empty() {
            row.insert("properties", properties);
        }

        // v, host
        if let Some(v) = info.get("v") {
            row.insert("v", v.clone());
        }
        // 单节点 host 从 stats[0] 取 (若有)
        if stats_list.len() == 1 {
            if let Ok(h) = stats_list[0].get_str("host") {
                row.insert("host", h.to_string());
            }
        }

        // _.omitBy(row, _.isEmpty): 丢空字段
        let mut cleaned = Document::new();
        for (k, v) in row.iter() {
            if !bson_is_empty(v) {
                cleaned.insert(k.clone(), v.clone());
            }
        }

        results.push(cleaned);
    }

    Ok(results)
}

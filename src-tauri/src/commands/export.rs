use mongodb::bson::{Bson, Document};
use tauri::{AppHandle, Emitter, State};

use crate::connection::manager::ConnectionManager;
use crate::error::AppError;
use crate::query::exporter::{self, ExportRequest};

/// 将文本内容写入指定文件路径
#[tauri::command]
pub async fn write_export_file(path: String, content: String) -> Result<(), AppError> {
    tokio::fs::write(&path, content.as_bytes())
        .await
        .map_err(|e| AppError::InvalidInput(format!("写入文件失败: {e}")))?;
    Ok(())
}

/// 将二进制内容写入指定文件路径（用于 xlsx 等二进制格式）
#[tauri::command]
pub async fn write_export_binary(path: String, data: Vec<u8>) -> Result<(), AppError> {
    tokio::fs::write(&path, &data)
        .await
        .map_err(|e| AppError::InvalidInput(format!("写入文件失败: {e}")))?;
    Ok(())
}

/// 读取文件内容
#[tauri::command]
pub async fn read_import_file(path: String) -> Result<String, AppError> {
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| AppError::InvalidInput(format!("读取文件失败: {e}")))
}

/// 导入文档到集合（小批量，前端传入）
#[tauri::command]
pub async fn import_documents(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection: String,
    documents: Vec<Document>,
) -> Result<u64, AppError> {
    if mgr.is_read_only(&connection_id).await {
        return Err(AppError::InvalidInput("只读连接：不允许导入数据".into()));
    }
    let client = mgr.get_client(&connection_id).await?;
    let coll = client.database(&database).collection::<Document>(&collection);
    let count = documents.len() as u64;
    coll.insert_many(documents).await.map_err(AppError::Mongo)?;
    Ok(count)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportProgress {
    imported: u64,
    total: u64,
    phase: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamImportRequest {
    pub connection_id: String,
    pub database: String,
    pub collection: String,
    pub file_path: String,
    /// "overwrite" | "skip" | "insert"
    pub insertion_mode: Option<String>,
}

/// 流式导入：后端读取文件、分批插入、发送进度
#[tauri::command]
pub async fn stream_import(
    app: AppHandle,
    mgr: State<'_, ConnectionManager>,
    request: StreamImportRequest,
) -> Result<u64, AppError> {
    if mgr.is_read_only(&request.connection_id).await {
        return Err(AppError::InvalidInput("只读连接：不允许导入数据".into()));
    }
    let client = mgr.get_client(&request.connection_id).await?;
    let coll = client.database(&request.database).collection::<Document>(&request.collection);

    // 读取文件
    let _ = app.emit("import-progress", ImportProgress { imported: 0, total: 0, phase: "读取文件...".into() });
    let content = tokio::fs::read_to_string(&request.file_path)
        .await
        .map_err(|e| AppError::InvalidInput(format!("读取文件失败: {e}")))?;

    // 解析
    let _ = app.emit("import-progress", ImportProgress { imported: 0, total: 0, phase: "解析文件...".into() });
    let ext = request.file_path.rsplit('.').next().unwrap_or("json").to_lowercase();
    let documents: Vec<Document> = match ext.as_str() {
        "csv" => parse_csv_to_docs(&content)?,
        // json / jsonl / ejson / simple-json 等文本 JSON 一律走同一套解析:
        // 兼容 数组 / 单对象 / NDJSON(换行分隔多对象, EJSON 导出即此格式)
        _ => parse_json_docs(&content)?,
    };

    let total = documents.len() as u64;
    if total == 0 {
        return Err(AppError::InvalidInput("文件中没有可导入的数据".into()));
    }

    let _ = app.emit("import-progress", ImportProgress { imported: 0, total, phase: "导入中...".into() });

    let mode = request.insertion_mode.as_deref().unwrap_or("overwrite");
    let batch_size = 500;
    let mut imported: u64 = 0;

    for chunk in documents.chunks(batch_size) {
        match mode {
            "overwrite" => {
                // 逐条 upsert：有相同 _id 则替换，没有则插入
                for doc in chunk {
                    if let Some(id) = doc.get("_id") {
                        let filter = mongodb::bson::doc! { "_id": id.clone() };
                        coll.replace_one(filter, doc.clone()).upsert(true).await.map_err(AppError::Mongo)?;
                    } else {
                        coll.insert_one(doc.clone()).await.map_err(AppError::Mongo)?;
                    }
                    imported += 1;
                    if imported % 100 == 0 {
                        let _ = app.emit("import-progress", ImportProgress { imported, total, phase: "导入中(覆盖模式)...".into() });
                    }
                }
            }
            "skip" => {
                // insert_many + ordered:false，重复 _id 跳过不报错
                match coll.insert_many(chunk.to_vec()).ordered(false).await {
                    Ok(result) => { imported += result.inserted_ids.len() as u64; }
                    Err(_) => {
                        // 部分成功：重复的被跳过，无法精确知道插入数，按批次计
                        imported += chunk.len() as u64;
                    }
                }
                let _ = app.emit("import-progress", ImportProgress { imported, total, phase: "导入中(跳过重复)...".into() });
            }
            _ => {
                // "insert": 直接插入，重复报错
                coll.insert_many(chunk.to_vec()).await.map_err(AppError::Mongo)?;
                imported += chunk.len() as u64;
                let _ = app.emit("import-progress", ImportProgress { imported, total, phase: "导入中...".into() });
            }
        }
    }

    let _ = app.emit("import-progress", ImportProgress { imported, total, phase: "完成".into() });
    Ok(imported)
}

/// 解析 JSON / NDJSON / EJSON 文本为文档列表.
/// 兼容三种结构:
///   1) JSON 数组:        `[ {...}, {...} ]`
///   2) 单个 JSON 对象:    `{...}`
///   3) NDJSON / 换行或空白分隔的多个对象 (EJSON 导出即此格式): `{...}\n{...}\n...`
///
/// 每个值按 `Bson` 反序列化, 保留 Extended JSON 类型 ($oid/$date/$numberLong 等).
/// 其中 $date 记录的是 UTC 瞬时值 (毫秒/带 Z 的 ISO), 原样回灌, **不做任何时区偏移**.
fn parse_json_docs(content: &str) -> Result<Vec<Document>, AppError> {
    let mut docs = Vec::new();
    // StreamDeserializer 依次读取顶层的多个 JSON 值 (空白/换行分隔), 单个值也适用
    let stream = serde_json::Deserializer::from_str(content).into_iter::<Bson>();
    for item in stream {
        let bson = item.map_err(|e| AppError::InvalidInput(format!("JSON 解析失败: {e}")))?;
        match bson {
            Bson::Document(mut d) => { revive_datetimes(&mut d); docs.push(d); }
            Bson::Array(arr) => {
                for v in arr {
                    match v {
                        Bson::Document(mut d) => { revive_datetimes(&mut d); docs.push(d); }
                        Bson::Null => continue,
                        _ => return Err(AppError::InvalidInput("数组元素必须是 JSON 对象".into())),
                    }
                }
            }
            Bson::Null => continue,
            _ => return Err(AppError::InvalidInput("每条记录必须是 JSON 对象".into())),
        }
    }
    Ok(docs)
}

/// 递归把文档里"带时区的 RFC3339 时间戳字符串"还原成 BSON Date.
/// 用于 Simple JSON / CSV 等把 Date 导成字符串的格式往返: 例如 Simple JSON 把日期
/// 写成 `2026-01-26T01:52:13.000Z` (UTC), 若原样以字符串导入, 前端按本地时区显示会差几小时.
///
/// **只识别严格 RFC3339 且带明确时区 (Z 或 ±HH:MM) 的字符串**; 裸日期 (无时区) 或普通文本
/// 保持字符串不变, 避免把恰好像日期的普通字段误转成 Date. `$date` 类型的 EJSON 本就已是
/// Date, 不受影响.
fn revive_datetimes(doc: &mut Document) {
    for (_k, v) in doc.iter_mut() {
        revive_bson(v);
    }
}

fn revive_bson(v: &mut Bson) {
    match v {
        Bson::String(s) => {
            if let Some(dt) = str_to_bson_datetime(s) {
                *v = dt;
            }
        }
        Bson::Document(d) => revive_datetimes(d),
        Bson::Array(arr) => {
            for item in arr.iter_mut() {
                revive_bson(item);
            }
        }
        _ => {}
    }
}

/// 严格判断: 带时区的 RFC3339 时间戳字符串 -> Bson::DateTime; 否则 None.
fn str_to_bson_datetime(s: &str) -> Option<Bson> {
    // 粗筛: 至少 "YYYY-MM-DDTHH:MM:SSZ" 长度, 且第 10 位是 'T', 减少无谓 parse
    if s.len() < 20 || s.as_bytes().get(10) != Some(&b'T') {
        return None;
    }
    // parse_from_rfc3339 要求必须带时区 (Z 或 ±HH:MM), 裸时间会被拒绝 —— 正是我们想要的
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| Bson::DateTime(mongodb::bson::DateTime::from_millis(dt.timestamp_millis())))
}

fn parse_csv_to_docs(content: &str) -> Result<Vec<Document>, AppError> {
    let mut lines = content.lines();
    let header_line = lines.next().ok_or_else(|| AppError::InvalidInput("CSV 文件为空".into()))?;
    let headers: Vec<&str> = parse_csv_line_to_vec(header_line);

    let mut docs = Vec::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() { continue; }
        let vals = parse_csv_line_to_vec(line);
        let mut doc = Document::new();
        for (i, h) in headers.iter().enumerate() {
            let v = vals.get(i).copied().unwrap_or("");
            if v.is_empty() || v == "null" {
                doc.insert(h.to_string(), mongodb::bson::Bson::Null);
            } else if v == "true" {
                doc.insert(h.to_string(), true);
            } else if v == "false" {
                doc.insert(h.to_string(), false);
            } else if let Ok(n) = v.parse::<i64>() {
                doc.insert(h.to_string(), n);
            } else if let Ok(n) = v.parse::<f64>() {
                doc.insert(h.to_string(), n);
            } else {
                doc.insert(h.to_string(), v);
            }
        }
        // 带时区的 RFC3339 时间字符串还原成 Date (与 JSON 导入一致)
        revive_datetimes(&mut doc);
        docs.push(doc);
    }
    Ok(docs)
}

fn parse_csv_line_to_vec(line: &str) -> Vec<&str> {
    // 简单 CSV 解析（不处理引号内的逗号——复杂情况建议用 JSON）
    line.split(',').map(|s| s.trim().trim_matches('"')).collect()
}

/// 流式导出：后端直接从 MongoDB 读取并写入文件，支持大数据量
#[tauri::command]
pub async fn export_query(
    app: AppHandle,
    mgr: State<'_, ConnectionManager>,
    request: ExportRequest,
) -> Result<u64, AppError> {
    let client = mgr.get_client(&request.connection_id).await?;
    exporter::stream_export(&client, &app, &request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::{doc, DateTime};

    /// 模拟 EJSON 导出的一行: bson Document -> serde_json 字符串 (relaxed extjson)
    fn ejson_line(d: &Document) -> String {
        serde_json::to_string(d).unwrap()
    }

    #[test]
    fn ndjson_roundtrip_preserves_datetime_utc() {
        // 一个普通 UTC 瞬时 + 一个 0001-01-01 (超出 relaxed 范围, 走 $numberLong)
        let ms_normal: i64 = 1_737_884_733_123;
        let ms_year1: i64 = -62_135_596_800_000; // 0001-01-01T00:00:00Z
        let d1 = doc! { "_id": 1i32, "t": DateTime::from_millis(ms_normal) };
        let d2 = doc! { "_id": 2i32, "t": DateTime::from_millis(ms_year1) };

        // EJSON 导出 = 换行分隔的多行 (NDJSON)
        let content = format!("{}\n{}", ejson_line(&d1), ejson_line(&d2));

        let docs = parse_json_docs(&content).unwrap();
        assert_eq!(docs.len(), 2);
        // 日期原样回灌, 无时区偏移
        assert_eq!(docs[0].get_datetime("t").unwrap().timestamp_millis(), ms_normal);
        assert_eq!(docs[1].get_datetime("t").unwrap().timestamp_millis(), ms_year1);
    }

    #[test]
    fn parses_json_array_and_single_object() {
        assert_eq!(parse_json_docs(r#"[{"a":1},{"a":2}]"#).unwrap().len(), 2);
        assert_eq!(parse_json_docs(r#"{"a":1}"#).unwrap().len(), 1);
        // 末尾多余空行不影响
        assert_eq!(parse_json_docs("{\"a\":1}\n{\"a\":2}\n").unwrap().len(), 2);
    }

    #[test]
    fn preserves_objectid_ext_json() {
        let content = r#"{"_id":{"$oid":"507f1f77bcf86cd799439011"},"n":1}"#;
        let docs = parse_json_docs(content).unwrap();
        assert_eq!(docs.len(), 1);
        assert!(docs[0].get_object_id("_id").is_ok());
    }

    #[test]
    fn simple_json_utc_string_revived_to_datetime() {
        // Simple JSON 把 Date 导成 UTC 字符串 (…Z); 导入应还原为 Date, 且瞬时值一致
        let ms: i64 = 1_769_392_333_000; // 2026-01-26T01:52:13Z
        let content = r#"{"t":"2026-01-26T01:52:13.000Z"}"#;
        let docs = parse_json_docs(content).unwrap();
        assert_eq!(docs[0].get_datetime("t").unwrap().timestamp_millis(), ms);
    }

    #[test]
    fn local_offset_string_revived_and_instant_preserved() {
        // 带 +08:00 偏移的字符串: 应还原为同一 UTC 瞬时 (09:52:13+08 == 01:52:13Z)
        let content = r#"{"t":"2026-01-26T09:52:13.000+08:00"}"#;
        let docs = parse_json_docs(content).unwrap();
        assert_eq!(docs[0].get_datetime("t").unwrap().timestamp_millis(), 1_769_392_333_000);
    }

    #[test]
    fn naive_or_plain_strings_stay_strings() {
        // 无时区的裸时间、纯日期、普通文本都不应被误转成 Date
        let docs = parse_json_docs(
            r#"{"a":"2026-01-26T09:52:13","b":"2026-01-26","c":"hello world","d":"12345"}"#,
        )
        .unwrap();
        for k in ["a", "b", "c", "d"] {
            assert!(docs[0].get_str(k).is_ok(), "字段 {k} 应保持字符串");
        }
    }

    #[test]
    fn csv_import_revives_datetime_column() {
        // CSV 导出的日期是带时区字符串; 导入应还原成 Date, 其它列不受影响
        let csv = "name,t,note\nfoo,2026-01-26T01:52:13.000Z,hello\n";
        let docs = parse_csv_to_docs(csv).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].get_str("name").unwrap(), "foo");
        assert_eq!(docs[0].get_str("note").unwrap(), "hello");
        assert_eq!(docs[0].get_datetime("t").unwrap().timestamp_millis(), 1_769_392_333_000);
    }

    #[test]
    fn revives_nested_datetime_strings() {
        let content = r#"{"meta":{"created":"2026-01-26T01:52:13.000Z"},"tags":["2026-01-26T01:52:13.000Z"]}"#;
        let docs = parse_json_docs(content).unwrap();
        let meta = docs[0].get_document("meta").unwrap();
        assert!(meta.get_datetime("created").is_ok());
        let arr = docs[0].get_array("tags").unwrap();
        assert!(matches!(arr[0], Bson::DateTime(_)));
    }
}

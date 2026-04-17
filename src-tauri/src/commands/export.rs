use mongodb::bson::Document;
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
        "jsonl" => {
            let mut docs = Vec::new();
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() { continue; }
                let doc: Document = serde_json::from_str(line)
                    .map_err(|e| AppError::InvalidInput(format!("JSON Lines 解析失败: {e}")))?;
                docs.push(doc);
            }
            docs
        }
        _ => {
            // JSON: 数组或单个对象
            let val: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| AppError::InvalidInput(format!("JSON 解析失败: {e}")))?;
            match val {
                serde_json::Value::Array(arr) => {
                    arr.into_iter().map(|v| {
                        serde_json::from_value(v).map_err(|e| AppError::InvalidInput(format!("文档解析失败: {e}")))
                    }).collect::<Result<Vec<_>, _>>()?
                }
                serde_json::Value::Object(_) => {
                    vec![serde_json::from_value(val).map_err(|e| AppError::InvalidInput(format!("文档解析失败: {e}")))?]
                }
                _ => return Err(AppError::InvalidInput("JSON 文件内容必须是数组或对象".into())),
            }
        }
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

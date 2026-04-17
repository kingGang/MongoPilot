use std::io::Write;

use futures::StreamExt;
use mongodb::bson::{Bson, Document};
use mongodb::Client;
use rust_xlsxwriter::{Workbook, Format};
use serde::Deserialize;
use tauri::{AppHandle, Emitter};

use crate::error::AppError;

use super::executor::{parse_json_arg, parse_chained_arg, extract_parens};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub connection_id: String,
    pub database: String,
    pub query_text: String,
    pub format: String,
    pub fields: Vec<String>,
    pub target_path: String,
    pub delimiter: Option<String>,
    pub collection_name: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportProgress {
    exported: u64,
    total: i64,
}

/// 将 BSON 值转为可读的纯值
fn bson_to_simple(val: &Bson) -> serde_json::Value {
    match val {
        Bson::ObjectId(oid) => serde_json::Value::String(oid.to_hex()),
        Bson::DateTime(dt) => {
            let millis = dt.timestamp_millis();
            let secs = millis / 1000;
            let nsecs = ((millis % 1000) * 1_000_000) as u32;
            if let Some(dt) = chrono::DateTime::from_timestamp(secs, nsecs) {
                serde_json::Value::String(dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
            } else {
                serde_json::Value::String(millis.to_string())
            }
        }
        Bson::Int32(n) => serde_json::json!(*n),
        Bson::Int64(n) => serde_json::Value::String(n.to_string()),
        Bson::Double(n) => serde_json::json!(*n),
        Bson::Boolean(b) => serde_json::json!(*b),
        Bson::String(s) => serde_json::Value::String(s.clone()),
        Bson::Null => serde_json::Value::Null,
        Bson::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(bson_to_simple).collect())
        }
        Bson::Document(doc) => {
            let mut map = serde_json::Map::new();
            for (k, v) in doc.iter() {
                map.insert(k.clone(), bson_to_simple(v));
            }
            serde_json::Value::Object(map)
        }
        Bson::Decimal128(d) => serde_json::Value::String(d.to_string()),
        Bson::Timestamp(ts) => serde_json::Value::String(format!("Timestamp({}, {})", ts.time, ts.increment)),
        Bson::Binary(bin) => {
            use base64::Engine;
            serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(&bin.bytes))
        }
        Bson::RegularExpression(re) => serde_json::Value::String(format!("/{}/{}", re.pattern, re.options)),
        _ => {
            // fallback: use bson's serde
            serde_json::to_value(val).unwrap_or(serde_json::Value::Null)
        }
    }
}

/// 将文档按字段过滤并转为简单值 map
fn doc_to_simple_map(doc: &Document, fields: &[String]) -> Vec<String> {
    fields.iter().map(|f| {
        match doc.get(f) {
            Some(val) => {
                let simple = bson_to_simple(val);
                match &simple {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Null => String::new(),
                    other => other.to_string(),
                }
            }
            None => String::new(),
        }
    }).collect()
}

fn doc_to_simple_json(doc: &Document, fields: &[String]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for f in fields {
        if let Some(val) = doc.get(f) {
            map.insert(f.clone(), bson_to_simple(val));
        }
    }
    serde_json::Value::Object(map)
}

fn escape_csv(s: &str, delim: &str) -> String {
    if s.contains(delim) || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn escape_sql(s: &str) -> String {
    s.replace('\'', "''")
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// 从查询文本解析出 filter/sort/projection/limit/skip 并构建 MongoDB cursor
async fn build_find_cursor(
    collection: &mongodb::Collection<Document>,
    rest: &str,
) -> Result<(mongodb::Cursor<Document>, i64), AppError> {
    let filter_str = extract_parens(rest, "find")?;
    let filter: Document = parse_json_arg(&filter_str)?;

    let after_find = &rest[rest.find(')').unwrap_or(rest.len()) + 1..];
    let projection = parse_chained_arg(after_find, ".projection(");
    let sort = parse_chained_arg(after_find, ".sort(");
    let limit = parse_chained_arg(after_find, ".limit(").and_then(|s| s.trim().parse::<i64>().ok());
    let skip = parse_chained_arg(after_find, ".skip(").and_then(|s| s.trim().parse::<u64>().ok());

    let mut proj_doc: Option<Document> = None;
    if let Some(proj_str) = projection {
        let d: Document = parse_json_arg(&proj_str)?;
        if !d.is_empty() { proj_doc = Some(d); }
    }
    let mut sort_doc: Option<Document> = None;
    if let Some(sort_str) = sort {
        let d: Document = parse_json_arg(&sort_str)?;
        if !d.is_empty() { sort_doc = Some(d); }
    }

    let total_count = collection.count_documents(filter.clone()).await.map_err(AppError::Mongo)? as i64;
    let effective_total = match limit {
        Some(l) => std::cmp::min(total_count, l),
        None => total_count,
    };

    let mut find = collection.find(filter);
    if let Some(p) = proj_doc { find = find.projection(p); }
    if let Some(s) = sort_doc { find = find.sort(s); }
    if let Some(sk) = skip { find = find.skip(sk); }
    if let Some(l) = limit { find = find.limit(l); }

    let cursor = find.await.map_err(AppError::Mongo)?;
    Ok((cursor, effective_total))
}

/// 流式导出：从 MongoDB 逐批读取，直接写入文件
pub async fn stream_export(
    client: &Client,
    app: &AppHandle,
    req: &ExportRequest,
) -> Result<u64, AppError> {
    let query = req.query_text.trim();
    let query = query.strip_prefix("db.").ok_or_else(|| {
        AppError::InvalidInput("查询必须以 db. 开头".into())
    })?;
    let (coll_name, rest) = if query.starts_with("getCollection(") {
        let gc_end = query.find(')').ok_or_else(|| {
            AppError::InvalidInput("getCollection() 括号不匹配".into())
        })?;
        let inner = &query["getCollection(".len()..gc_end];
        let name = inner.trim().trim_matches(|c| c == '"' || c == '\'');
        let after = &query[gc_end + 1..];
        let after = after.strip_prefix('.').unwrap_or(after);
        (name.to_string(), after)
    } else {
        let dot_pos = query.find('.').ok_or_else(|| {
            AppError::InvalidInput("格式错误".into())
        })?;
        (query[..dot_pos].to_string(), &query[dot_pos + 1..])
    };

    if !rest.starts_with("find(") {
        return Err(AppError::InvalidInput("导出仅支持 find 查询".into()));
    }

    let db = client.database(&req.database);
    let collection = db.collection::<Document>(&coll_name);
    let (mut cursor, total) = build_find_cursor(&collection, rest).await?;

    // xlsx 格式走单独路径
    if req.format == "xlsx" {
        return export_xlsx(&mut cursor, app, req, total).await;
    }

    let file = std::fs::File::create(&req.target_path)
        .map_err(|e| AppError::InvalidInput(format!("创建文件失败: {e}")))?;
    let mut writer = std::io::BufWriter::new(file);
    let delim = req.delimiter.as_deref().unwrap_or(",");
    let table_name = req.collection_name.as_deref().unwrap_or(&coll_name);

    let mut count: u64 = 0;
    let fmt = req.format.as_str();

    // 写头部
    match fmt {
        "csv" | "txt" => {
            let sep = if fmt == "txt" { "\t" } else { delim };
            let header: Vec<String> = req.fields.iter().map(|f| escape_csv(f, sep)).collect();
            writeln!(writer, "{}", header.join(sep)).ok();
        }
        "simple-json" | "mongoshell" => { write!(writer, "[\n").ok(); }
        "html" => {
            write!(writer, "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>Export</title>\
            <style>table{{border-collapse:collapse;font-family:sans-serif;font-size:13px}}\
            th,td{{border:1px solid #ccc;padding:4px 8px;text-align:left}}\
            th{{background:#f0f0f0;font-weight:600}}tr:nth-child(even){{background:#fafafa}}</style>\
            </head><body><table><thead><tr>").ok();
            for f in &req.fields {
                write!(writer, "<th>{}</th>", escape_html(f)).ok();
            }
            write!(writer, "</tr></thead><tbody>\n").ok();
        }
        "sql" => {
            writeln!(writer, "-- Exported from MongoPilot").ok();
        }
        _ => {}
    }

    while let Some(doc_result) = cursor.next().await {
        let doc = doc_result.map_err(AppError::Mongo)?;
        count += 1;

        match fmt {
            "simple-json" => {
                let json = doc_to_simple_json(&doc, &req.fields);
                if count > 1 { write!(writer, ",\n").ok(); }
                write!(writer, "  {}", serde_json::to_string_pretty(&json).unwrap_or_default()).ok();
            }
            "ejson" => {
                let mut filtered = Document::new();
                for f in &req.fields {
                    if let Some(v) = doc.get(f) { filtered.insert(f.clone(), v.clone()); }
                }
                let json_str = serde_json::to_string(&filtered).unwrap_or_default();
                if count > 1 { writeln!(writer).ok(); }
                write!(writer, "{json_str}").ok();
            }
            "mongoshell" => {
                let json = doc_to_simple_json(&doc, &req.fields);
                if count > 1 { write!(writer, ",\n").ok(); }
                write!(writer, "  {}", serde_json::to_string_pretty(&json).unwrap_or_default()).ok();
            }
            "jsonl" => {
                let json = doc_to_simple_json(&doc, &req.fields);
                writeln!(writer, "{}", serde_json::to_string(&json).unwrap_or_default()).ok();
            }
            "csv" | "txt" => {
                let sep = if fmt == "txt" { "\t" } else { delim };
                let vals = doc_to_simple_map(&doc, &req.fields);
                let escaped: Vec<String> = vals.iter().map(|v| escape_csv(v, sep)).collect();
                writeln!(writer, "{}", escaped.join(sep)).ok();
            }
            "sql" => {
                let vals = doc_to_simple_map(&doc, &req.fields);
                let col_list: String = req.fields.iter().map(|f| format!("`{f}`")).collect::<Vec<_>>().join(", ");
                let val_list: String = vals.iter().map(|v| {
                    if v.is_empty() { "NULL".to_string() }
                    else if v.parse::<f64>().is_ok() { v.clone() }
                    else { format!("'{}'", escape_sql(v)) }
                }).collect::<Vec<_>>().join(", ");
                writeln!(writer, "INSERT INTO `{table_name}` ({col_list}) VALUES ({val_list});").ok();
            }
            "html" => {
                let vals = doc_to_simple_map(&doc, &req.fields);
                write!(writer, "<tr>").ok();
                for v in &vals {
                    write!(writer, "<td>{}</td>", escape_html(v)).ok();
                }
                writeln!(writer, "</tr>").ok();
            }
            _ => {
                let json = doc_to_simple_json(&doc, &req.fields);
                writeln!(writer, "{}", serde_json::to_string(&json).unwrap_or_default()).ok();
            }
        }

        if count % 500 == 0 {
            let _ = app.emit("export-progress", ExportProgress { exported: count, total });
        }
    }

    // 写尾部
    match fmt {
        "simple-json" | "mongoshell" => { write!(writer, "\n]").ok(); }
        "html" => { write!(writer, "</tbody></table></body></html>").ok(); }
        _ => {}
    }

    writer.flush().map_err(|e| AppError::InvalidInput(format!("写入文件失败: {e}")))?;
    let _ = app.emit("export-progress", ExportProgress { exported: count, total });
    Ok(count)
}

/// 导出为 xlsx 格式
async fn export_xlsx(
    cursor: &mut mongodb::Cursor<Document>,
    app: &AppHandle,
    req: &ExportRequest,
    total: i64,
) -> Result<u64, AppError> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("Export").map_err(|e| AppError::InvalidInput(format!("xlsx error: {e}")))?;

    // 表头样式：加粗 + 浅灰背景
    let header_fmt = Format::new()
        .set_bold()
        .set_background_color(rust_xlsxwriter::Color::RGB(0xF0F0F0))
        .set_border(rust_xlsxwriter::FormatBorder::Thin);

    // 写表头（字段名作为列名）
    for (col, field) in req.fields.iter().enumerate() {
        worksheet.write_string_with_format(0, col as u16, field, &header_fmt)
            .map_err(|e| AppError::InvalidInput(format!("xlsx error: {e}")))?;
    }

    let mut row: u32 = 1;
    let mut count: u64 = 0;

    while let Some(doc_result) = cursor.next().await {
        let doc = doc_result.map_err(AppError::Mongo)?;
        count += 1;

        for (col, field) in req.fields.iter().enumerate() {
            let col = col as u16;
            match doc.get(field) {
                None => { worksheet.write_string(row, col, "").ok(); }
                Some(val) => {
                    write_bson_to_cell(worksheet, row, col, val);
                }
            }
        }

        row += 1;
        if count % 500 == 0 {
            let _ = app.emit("export-progress", ExportProgress { exported: count, total });
        }
    }

    // 自动列宽
    for col in 0..req.fields.len() {
        worksheet.set_column_width(col as u16, 18)
            .map_err(|e| AppError::InvalidInput(format!("xlsx error: {e}")))?;
    }

    workbook.save(&req.target_path)
        .map_err(|e| AppError::InvalidInput(format!("保存 xlsx 失败: {e}")))?;

    let _ = app.emit("export-progress", ExportProgress { exported: count, total });
    Ok(count)
}

/// 将 BSON 值写入 xlsx 单元格（保留类型）
fn write_bson_to_cell(ws: &mut rust_xlsxwriter::Worksheet, row: u32, col: u16, val: &Bson) {
    match val {
        Bson::String(s) => { ws.write_string(row, col, s).ok(); }
        Bson::Int32(n) => { ws.write_number(row, col, *n as f64).ok(); }
        Bson::Int64(n) => { ws.write_number(row, col, *n as f64).ok(); }
        Bson::Double(n) => { ws.write_number(row, col, *n).ok(); }
        Bson::Boolean(b) => { ws.write_boolean(row, col, *b).ok(); }
        Bson::Null => { ws.write_string(row, col, "").ok(); }
        Bson::ObjectId(oid) => { ws.write_string(row, col, &oid.to_hex()).ok(); }
        Bson::DateTime(dt) => {
            let millis = dt.timestamp_millis();
            let secs = millis / 1000;
            let nsecs = ((millis % 1000) * 1_000_000) as u32;
            if let Some(dt) = chrono::DateTime::from_timestamp(secs, nsecs) {
                let iso = dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                ws.write_string(row, col, &iso).ok();
            } else {
                ws.write_number(row, col, millis as f64).ok();
            }
        }
        Bson::Decimal128(d) => { ws.write_string(row, col, &d.to_string()).ok(); }
        Bson::Array(_) | Bson::Document(_) => {
            let json = serde_json::to_string(&bson_to_simple(val)).unwrap_or_default();
            ws.write_string(row, col, &json).ok();
        }
        Bson::Binary(bin) => {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bin.bytes);
            ws.write_string(row, col, &b64).ok();
        }
        Bson::RegularExpression(re) => {
            ws.write_string(row, col, &format!("/{}/{}", re.pattern, re.options)).ok();
        }
        Bson::Timestamp(ts) => {
            ws.write_string(row, col, &format!("Timestamp({}, {})", ts.time, ts.increment)).ok();
        }
        _ => {
            let s = serde_json::to_string(&bson_to_simple(val)).unwrap_or_default();
            ws.write_string(row, col, &s).ok();
        }
    }
}

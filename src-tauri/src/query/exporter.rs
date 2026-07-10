use std::collections::HashMap;
use std::io::Write;

use futures::StreamExt;
use mongodb::bson::{Bson, Document};
use mongodb::Client;
use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};
use serde::Deserialize;
use tauri::{AppHandle, Emitter};

use crate::error::AppError;

use super::executor::{aggregate_pipeline_from_arg, extract_parens, parse_chained_arg, parse_json_arg};

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
    /// 按字段名指定的导出类型 override.
    /// 支持: "string" | "number" | "boolean" | "json".
    /// 未在 map 中或为 None 的字段走 Format 默认行为.
    pub field_types: Option<HashMap<String, String>>,
    /// 按字段名指定的 Excel num_format pattern, 仅 xlsx 格式使用.
    /// 未在 map 中的 Date 字段使用默认 `yyyy-mm-dd hh:mm:ss`.
    pub date_formats: Option<HashMap<String, String>>,
}

/// BSON Date -> **本地时区**带偏移的 RFC3339 字符串 (如 `2026-01-26T09:52:13.000+08:00`).
/// 与前端按本地时区的显示一致, 且带明确偏移量, 导入时能无损还原回同一 UTC 瞬时.
/// 超出可表示范围时回退为毫秒数字符串.
/// ejson 格式**不**走这里 —— 它保留 BSON 的 $date (UTC), 作为精确交换格式.
fn bson_datetime_to_local_rfc3339(dt: &mongodb::bson::DateTime) -> String {
    let millis = dt.timestamp_millis();
    let secs = millis / 1000;
    let nsecs = ((millis % 1000) * 1_000_000) as u32;
    match chrono::DateTime::from_timestamp(secs, nsecs) {
        Some(utc) => utc
            .with_timezone(&chrono::Local)
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, false),
        None => millis.to_string(),
    }
}

/// 把单个 BSON 值按 override 类型强制转换为另一个 BSON 值.
/// 失败 (如把 ObjectId 转 number) 落到 Bson::Null.
fn apply_override(val: &Bson, override_type: &str) -> Bson {
    match override_type {
        "string" => match val {
            Bson::Null => Bson::String(String::new()),
            Bson::String(s) => Bson::String(s.clone()),
            Bson::ObjectId(oid) => Bson::String(oid.to_hex()),
            Bson::Int32(n) => Bson::String(n.to_string()),
            Bson::Int64(n) => Bson::String(n.to_string()),
            Bson::Double(n) => Bson::String(n.to_string()),
            Bson::Boolean(b) => Bson::String(b.to_string()),
            Bson::DateTime(dt) => Bson::String(bson_datetime_to_local_rfc3339(dt)),
            Bson::Decimal128(d) => Bson::String(d.to_string()),
            other => Bson::String(
                serde_json::to_string(&bson_to_simple(other)).unwrap_or_default(),
            ),
        },
        "number" => match val {
            Bson::Int32(_) | Bson::Int64(_) | Bson::Double(_) => val.clone(),
            Bson::Boolean(b) => Bson::Int32(if *b { 1 } else { 0 }),
            Bson::String(s) => s.parse::<f64>().map(Bson::Double).unwrap_or(Bson::Null),
            _ => Bson::Null,
        },
        "boolean" => match val {
            Bson::Boolean(_) => val.clone(),
            Bson::Null => Bson::Boolean(false),
            Bson::Int32(n) => Bson::Boolean(*n != 0),
            Bson::Int64(n) => Bson::Boolean(*n != 0),
            Bson::Double(n) => Bson::Boolean(*n != 0.0),
            Bson::String(s) => {
                let lower = s.to_lowercase();
                Bson::Boolean(!s.is_empty() && lower != "false" && lower != "0" && lower != "null")
            }
            _ => Bson::Boolean(true),
        },
        "json" => Bson::String(
            serde_json::to_string(&bson_to_simple(val)).unwrap_or_default(),
        ),
        _ => val.clone(),
    }
}

/// 取字段的 (可能被 override 后的) BSON 值.
fn field_value(doc: &Document, field: &str, overrides: Option<&HashMap<String, String>>) -> Option<Bson> {
    let raw = doc.get(field)?;
    if let Some(map) = overrides {
        if let Some(t) = map.get(field) {
            return Some(apply_override(raw, t));
        }
    }
    Some(raw.clone())
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
        Bson::DateTime(dt) => serde_json::Value::String(bson_datetime_to_local_rfc3339(dt)),
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
fn doc_to_simple_map(
    doc: &Document,
    fields: &[String],
    overrides: Option<&HashMap<String, String>>,
) -> Vec<String> {
    fields.iter().map(|f| {
        match field_value(doc, f, overrides) {
            Some(val) => {
                let simple = bson_to_simple(&val);
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

fn doc_to_simple_json(
    doc: &Document,
    fields: &[String],
    overrides: Option<&HashMap<String, String>>,
) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for f in fields {
        if let Some(val) = field_value(doc, f, overrides) {
            map.insert(f.clone(), bson_to_simple(&val));
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

/// 把 aggregate(...) 解析成 pipeline 并返回 cursor + 总数估算.
/// 总数: 复制一份 pipeline 末尾追加 $count 拿到; 拿不到则返回 -1.
async fn build_aggregate_cursor(
    collection: &mongodb::Collection<Document>,
    rest: &str,
) -> Result<(mongodb::Cursor<Document>, i64), AppError> {
    let arg_str = extract_parens(rest, "aggregate")?;
    let pipeline = aggregate_pipeline_from_arg(&arg_str)?;

    // 先做一次 count: clone pipeline + $count 一阶段, 用于前端进度条
    let mut count_pipeline = pipeline.clone();
    count_pipeline.push(mongodb::bson::doc! { "$count": "total" });
    let total = match collection.aggregate(count_pipeline).await {
        Ok(mut c) => {
            if let Some(Ok(doc)) = c.next().await {
                doc.get_i64("total")
                    .or(doc.get_i32("total").map(|v| v as i64))
                    .unwrap_or(-1)
            } else {
                0
            }
        }
        Err(_) => -1,
    };

    let cursor = collection.aggregate(pipeline).await.map_err(AppError::Mongo)?;
    Ok((cursor, total))
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

    let db = client.database(&req.database);
    let collection = db.collection::<Document>(&coll_name);

    let (mut cursor, total) = if rest.starts_with("find(") {
        build_find_cursor(&collection, rest).await?
    } else if rest.starts_with("aggregate(") {
        build_aggregate_cursor(&collection, rest).await?
    } else {
        return Err(AppError::InvalidInput(
            "导出仅支持 find 或 aggregate 查询".into(),
        ));
    };

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

        let overrides = req.field_types.as_ref();
        match fmt {
            "simple-json" => {
                let json = doc_to_simple_json(&doc, &req.fields, overrides);
                if count > 1 { write!(writer, ",\n").ok(); }
                write!(writer, "  {}", serde_json::to_string_pretty(&json).unwrap_or_default()).ok();
            }
            "ejson" => {
                // ejson 默认保留 BSON 类型, 但若用户设置了 override 则按 override 走
                let mut filtered = Document::new();
                for f in &req.fields {
                    if let Some(v) = field_value(&doc, f, overrides) {
                        filtered.insert(f.clone(), v);
                    }
                }
                let json_str = serde_json::to_string(&filtered).unwrap_or_default();
                if count > 1 { writeln!(writer).ok(); }
                write!(writer, "{json_str}").ok();
            }
            "mongoshell" => {
                let json = doc_to_simple_json(&doc, &req.fields, overrides);
                if count > 1 { write!(writer, ",\n").ok(); }
                write!(writer, "  {}", serde_json::to_string_pretty(&json).unwrap_or_default()).ok();
            }
            "jsonl" => {
                let json = doc_to_simple_json(&doc, &req.fields, overrides);
                writeln!(writer, "{}", serde_json::to_string(&json).unwrap_or_default()).ok();
            }
            "csv" | "txt" => {
                let sep = if fmt == "txt" { "\t" } else { delim };
                let vals = doc_to_simple_map(&doc, &req.fields, overrides);
                let escaped: Vec<String> = vals.iter().map(|v| escape_csv(v, sep)).collect();
                writeln!(writer, "{}", escaped.join(sep)).ok();
            }
            "sql" => {
                let vals = doc_to_simple_map(&doc, &req.fields, overrides);
                let col_list: String = req.fields.iter().map(|f| format!("`{f}`")).collect::<Vec<_>>().join(", ");
                let val_list: String = vals.iter().map(|v| {
                    if v.is_empty() { "NULL".to_string() }
                    else if v.parse::<f64>().is_ok() { v.clone() }
                    else { format!("'{}'", escape_sql(v)) }
                }).collect::<Vec<_>>().join(", ");
                writeln!(writer, "INSERT INTO `{table_name}` ({col_list}) VALUES ({val_list});").ok();
            }
            "html" => {
                let vals = doc_to_simple_map(&doc, &req.fields, overrides);
                write!(writer, "<tr>").ok();
                for v in &vals {
                    write!(writer, "<td>{}</td>", escape_html(v)).ok();
                }
                writeln!(writer, "</tr>").ok();
            }
            _ => {
                let json = doc_to_simple_json(&doc, &req.fields, overrides);
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

    // 每个 Date 字段独立 num_format. 缓存 Format 对象 (Format 同 pattern 复用 1 个) 减少内存.
    const DEFAULT_DATE_PATTERN: &str = "yyyy-mm-dd hh:mm:ss";
    let mut date_fmt_cache: HashMap<String, Format> = HashMap::new();
    date_fmt_cache.insert(
        DEFAULT_DATE_PATTERN.to_string(),
        Format::new().set_num_format(DEFAULT_DATE_PATTERN),
    );
    // 把用户传入的 pattern 都预先 build 好
    if let Some(map) = req.date_formats.as_ref() {
        for p in map.values() {
            let p = p.trim();
            if p.is_empty() { continue; }
            date_fmt_cache
                .entry(p.to_string())
                .or_insert_with(|| Format::new().set_num_format(p));
        }
    }
    // 取字段对应的 date Format 引用
    let date_fmt_for = |field: &str| -> &Format {
        let pattern = req
            .date_formats
            .as_ref()
            .and_then(|m| m.get(field))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_DATE_PATTERN);
        date_fmt_cache.get(pattern).expect("pattern must be pre-cached")
    };

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

        let overrides = req.field_types.as_ref();
        for (col, field) in req.fields.iter().enumerate() {
            let col = col as u16;
            match field_value(&doc, field, overrides) {
                None => { worksheet.write_string(row, col, "").ok(); }
                Some(val) => {
                    write_bson_to_cell(worksheet, row, col, &val, date_fmt_for(field));
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

/// 把 BSON DateTime 转成 ExcelDateTime.
/// rust_xlsxwriter 0.94 的 ExcelDateTime 没有 `from_ymd_hms` 这种合体构造, 走 ISO 8601 字符串.
fn bson_dt_to_excel(dt: &mongodb::bson::DateTime) -> Option<ExcelDateTime> {
    let millis = dt.timestamp_millis();
    let secs = millis / 1000;
    let nsecs = ((millis % 1000) * 1_000_000) as u32;
    let cdt = chrono::DateTime::from_timestamp(secs, nsecs)?;
    // Excel 无时区概念: 写入本地时区的墙钟时间, 与应用内显示一致 (原来写的是 UTC, 会差几小时)
    let iso = cdt
        .with_timezone(&chrono::Local)
        .naive_local()
        .format("%Y-%m-%dT%H:%M:%S%.3f")
        .to_string();
    ExcelDateTime::parse_from_str(&iso).ok()
}

/// 将 BSON 值写入 xlsx 单元格（保留类型）.
/// DateTime 字段写为真正的 Excel 日期 cell + `date_fmt` num_format,
/// 失败时回退到 ISO 字符串.
fn write_bson_to_cell(
    ws: &mut rust_xlsxwriter::Worksheet,
    row: u32,
    col: u16,
    val: &Bson,
    date_fmt: &Format,
) {
    match val {
        Bson::String(s) => { ws.write_string(row, col, s).ok(); }
        Bson::Int32(n) => { ws.write_number(row, col, *n as f64).ok(); }
        Bson::Int64(n) => { ws.write_number(row, col, *n as f64).ok(); }
        Bson::Double(n) => { ws.write_number(row, col, *n).ok(); }
        Bson::Boolean(b) => { ws.write_boolean(row, col, *b).ok(); }
        Bson::Null => { ws.write_string(row, col, "").ok(); }
        Bson::ObjectId(oid) => { ws.write_string(row, col, &oid.to_hex()).ok(); }
        Bson::DateTime(dt) => {
            if let Some(excel_dt) = bson_dt_to_excel(dt) {
                ws.write_datetime_with_format(row, col, &excel_dt, date_fmt).ok();
            } else {
                // 极端 fallback: 写 millis 数字
                ws.write_number(row, col, dt.timestamp_millis() as f64).ok();
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

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::DateTime;

    /// 无论构建机器在哪个时区, 本地 RFC3339 字符串都带偏移量, 解析回来必须是同一 UTC 瞬时.
    #[test]
    fn local_rfc3339_roundtrips_to_same_instant() {
        for ms in [1_769_392_333_000i64, 0, 1_609_459_200_123, -62_135_596_800_000] {
            let s = bson_datetime_to_local_rfc3339(&DateTime::from_millis(ms));
            let back = chrono::DateTime::parse_from_rfc3339(&s)
                .unwrap_or_else(|_| panic!("应能解析: {s}"))
                .timestamp_millis();
            assert_eq!(back, ms, "字符串 {s} 应还原为同一瞬时");
        }
    }

    /// bson_to_simple 的日期输出也应带偏移量并可还原
    #[test]
    fn simple_date_output_has_offset_and_roundtrips() {
        let v = bson_to_simple(&Bson::DateTime(DateTime::from_millis(1_769_392_333_000)));
        let s = v.as_str().unwrap();
        // 带 Z 或 ±HH:MM 偏移
        assert!(s.ends_with('Z') || s.contains('+') || s[19..].contains('-'), "应含时区: {s}");
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis(),
            1_769_392_333_000
        );
    }
}

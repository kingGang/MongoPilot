use mongodb::bson::{doc, Document};
use mongodb::Client;
use serde::Serialize;

use crate::error::AppError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub documents: Vec<Document>,
    /// 本次返回的文档数
    pub count: i64,
    /// 匹配条件的总文档数（不受 limit 限制）
    pub total_count: i64,
    pub execution_time_ms: i64,
}

/// 分页参数
#[derive(Debug, Clone, Copy)]
pub struct Pagination {
    pub skip: u64,
    pub page_size: i64,
}

/// 执行简易 Shell 风格查询（支持分页）。
pub async fn execute_shell_query(
    client: &Client,
    database: &str,
    query_text: &str,
    pagination: Option<Pagination>,
) -> Result<QueryResult, AppError> {
    let query = query_text.trim();
    let start = std::time::Instant::now();

    let query = if let Some(stripped) = query.strip_prefix("db.") {
        stripped
    } else {
        return Err(AppError::InvalidInput(
            "查询必须以 db. 开头，例如 db.collection.find({})".into(),
        ));
    };

    // ---- db.getSiblingDB("otherDb").xxx() → 切换目标数据库 ----
    let (effective_db_name, query) = if query.starts_with("getSiblingDB(") {
        let gc_end = query.find(')').ok_or_else(|| {
            AppError::InvalidInput("getSiblingDB() 括号不匹配".into())
        })?;
        let inner = &query["getSiblingDB(".len()..gc_end];
        let sibling = inner.trim().trim_matches(|c| c == '"' || c == '\'');
        let after = &query[gc_end + 1..];
        let after = after.strip_prefix('.').unwrap_or(after);
        (sibling.to_string(), after)
    } else {
        (database.to_string(), query)
    };

    let db = client.database(&effective_db_name);

    // ---- 数据库级命令（不需要集合名）----
    if query.starts_with("getUser(") {
        let arg_str = extract_parens(query, "getUser")?;
        let username = arg_str.trim().trim_matches(|c: char| c == '"' || c == '\'');
        let result_doc = db
            .run_command(doc! { "usersInfo": { "user": username, "db": &effective_db_name } })
            .await
            .map_err(AppError::Mongo)?;
        let users = result_doc.get_array("users").ok()
            .map(|arr| arr.iter().filter_map(|b| b.as_document().cloned()).collect::<Vec<_>>())
            .unwrap_or_default();
        let count = users.len() as i64;
        let elapsed = start.elapsed().as_millis() as i64;
        return Ok(QueryResult { documents: users, count, total_count: count, execution_time_ms: elapsed });
    }

    if query.starts_with("getRole(") {
        let arg_str = extract_parens(query, "getRole")?;
        // getRole("roleName", {showPrivileges:true, showBuiltinRoles: true})
        let parts: Vec<&str> = arg_str.splitn(2, ',').collect();
        let role_name = parts[0].trim().trim_matches(|c: char| c == '"' || c == '\'');
        let mut cmd = doc! { "rolesInfo": { "role": role_name, "db": &effective_db_name } };
        if parts.len() > 1 {
            let opts_str = parts[1].trim();
            if let Ok(opts) = parse_json_arg(opts_str) {
                if opts.get_bool("showPrivileges").unwrap_or(false) {
                    cmd.insert("showPrivileges", true);
                }
                if opts.get_bool("showBuiltinRoles").unwrap_or(false) {
                    cmd.insert("showBuiltinRoles", true);
                }
            }
        }
        let result_doc = db.run_command(cmd).await.map_err(AppError::Mongo)?;
        let roles = result_doc.get_array("roles").ok()
            .map(|arr| arr.iter().filter_map(|b| b.as_document().cloned()).collect::<Vec<_>>())
            .unwrap_or_default();
        let count = roles.len() as i64;
        let elapsed = start.elapsed().as_millis() as i64;
        return Ok(QueryResult { documents: roles, count, total_count: count, execution_time_ms: elapsed });
    }

    if query.starts_with("getUsers(") {
        let result_doc = db.run_command(doc! { "usersInfo": 1 }).await.map_err(AppError::Mongo)?;
        let users = result_doc.get_array("users").ok()
            .map(|arr| arr.iter().filter_map(|b| b.as_document().cloned()).collect::<Vec<_>>())
            .unwrap_or_default();
        let count = users.len() as i64;
        let elapsed = start.elapsed().as_millis() as i64;
        return Ok(QueryResult { documents: users, count, total_count: count, execution_time_ms: elapsed });
    }

    if query.starts_with("getRoles(") {
        let arg_str = extract_parens(query, "getRoles")?;
        let mut cmd = doc! { "rolesInfo": 1 };
        if !arg_str.trim().is_empty() {
            if let Ok(opts) = parse_json_arg(&arg_str) {
                if opts.get_bool("showBuiltinRoles").unwrap_or(false) {
                    cmd.insert("showBuiltinRoles", true);
                }
                if opts.get_bool("showPrivileges").unwrap_or(false) {
                    cmd.insert("showPrivileges", true);
                }
            }
        }
        let result_doc = db.run_command(cmd).await.map_err(AppError::Mongo)?;
        let roles = result_doc.get_array("roles").ok()
            .map(|arr| arr.iter().filter_map(|b| b.as_document().cloned()).collect::<Vec<_>>())
            .unwrap_or_default();
        let count = roles.len() as i64;
        let elapsed = start.elapsed().as_millis() as i64;
        return Ok(QueryResult { documents: roles, count, total_count: count, execution_time_ms: elapsed });
    }

    if query.starts_with("dropUser(") {
        let arg_str = extract_parens(query, "dropUser")?;
        let username = arg_str.trim().trim_matches(|c: char| c == '"' || c == '\'');
        db.run_command(doc! { "dropUser": username }).await.map_err(AppError::Mongo)?;
        let result_doc = doc! { "ok": 1, "dropped": username };
        let elapsed = start.elapsed().as_millis() as i64;
        return Ok(QueryResult { documents: vec![result_doc], count: 1, total_count: 1, execution_time_ms: elapsed });
    }

    if query.starts_with("createUser(") {
        let arg_str = extract_parens(query, "createUser")?;
        let converted = convert_shell_types(&arg_str);
        let relaxed = relax_json(&converted);
        let user_doc: Document = serde_json::from_str(&relaxed)
            .map_err(|e| AppError::InvalidInput(format!("无法解析用户文档: {e}")))?;
        let mut cmd = doc! { "createUser": user_doc.get_str("user").unwrap_or("") };
        if let Some(pwd) = user_doc.get_str("pwd").ok() { cmd.insert("pwd", pwd); }
        if let Some(roles) = user_doc.get_array("roles").ok() { cmd.insert("roles", roles.clone()); }
        db.run_command(cmd).await.map_err(AppError::Mongo)?;
        let result_doc = doc! { "ok": 1, "user": user_doc.get_str("user").unwrap_or("") };
        let elapsed = start.elapsed().as_millis() as i64;
        return Ok(QueryResult { documents: vec![result_doc], count: 1, total_count: 1, execution_time_ms: elapsed });
    }

    // 支持 db.getCollection("name.with.dots").method() 和 db.collName.method()
    let (collection_name, rest) = if query.starts_with("getCollection(") {
        // db.getCollection("system.version").find({})
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
            AppError::InvalidInput("格式错误，需要 db.collection.method(...)".into())
        })?;
        (query[..dot_pos].to_string(), &query[dot_pos + 1..])
    };

    let collection = db.collection::<Document>(&collection_name);

    let result = if rest.starts_with("find(") {
        execute_find(&collection, rest, pagination).await?
    } else if rest.starts_with("countDocuments(") {
        execute_count(&collection, rest).await?
    } else if rest.starts_with("aggregate(") {
        execute_aggregate(&collection, rest, pagination).await?
    } else if rest.starts_with("findOne(") {
        execute_find_one(&collection, rest).await?
    } else if rest.starts_with("distinct(") {
        execute_distinct(&collection, rest).await?
    } else if rest.starts_with("insertOne(") {
        execute_insert_one(&collection, rest).await?
    } else if rest.starts_with("insertMany(") {
        execute_insert_many(&collection, rest).await?
    } else if rest.starts_with("updateOne(") {
        execute_update(&collection, rest, "updateOne", false).await?
    } else if rest.starts_with("updateMany(") {
        execute_update(&collection, rest, "updateMany", true).await?
    } else if rest.starts_with("deleteOne(") {
        execute_delete(&collection, rest, "deleteOne", false).await?
    } else if rest.starts_with("deleteMany(") {
        execute_delete(&collection, rest, "deleteMany", true).await?
    } else if rest.starts_with("replaceOne(") {
        execute_replace_one(&collection, rest).await?
    } else {
        return Err(AppError::InvalidInput(
            format!("不支持的操作: {rest}"),
        ));
    };

    let elapsed = start.elapsed().as_millis() as i64;
    Ok(QueryResult {
        execution_time_ms: elapsed,
        ..result
    })
}

async fn execute_find(
    collection: &mongodb::Collection<Document>,
    rest: &str,
    pagination: Option<Pagination>,
) -> Result<QueryResult, AppError> {
    let filter_str = extract_parens(rest, "find")?;
    let filter: Document = parse_json_arg(&filter_str)?;

    // 解析链式调用：.projection({}) .sort({}) .limit(N) .skip(N)
    let after_find = &rest[rest.find(')').unwrap_or(rest.len()) + 1..];
    let projection = parse_chained_arg(after_find, ".projection(");
    let sort = parse_chained_arg(after_find, ".sort(");
    let user_limit = parse_chained_limit(after_find);
    let user_skip = parse_chained_skip(after_find);

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

    // 查询总数（与 find 使用相同的 filter）
    let total_count = collection
        .count_documents(filter.clone())
        .await
        .map_err(AppError::Mongo)? as i64;

    // 计算有效总数（考虑用户 limit）
    let effective_total = match user_limit {
        Some(ul) => std::cmp::min(total_count, ul),
        None => total_count,
    };

    let mut find = collection.find(filter);
    if let Some(p) = proj_doc { find = find.projection(p); }
    if let Some(s) = sort_doc { find = find.sort(s); }

    // 分页逻辑
    if let Some(pg) = pagination {
        // 后端分页：在用户 skip 基础上叠加分页 skip
        let base_skip = user_skip.unwrap_or(0) as u64;
        find = find.skip(base_skip + pg.skip);

        // limit = 每页大小，但不超过用户 limit 剩余量
        let page_limit = if let Some(ul) = user_limit {
            let already_skipped = pg.skip as i64;
            let remaining = ul - already_skipped;
            if remaining <= 0 { 0 } else { std::cmp::min(pg.page_size, remaining) }
        } else {
            pg.page_size
        };
        find = find.limit(page_limit);
    } else {
        // 无分页参数：兼容旧行为，用用户原始 skip/limit
        if let Some(sk) = user_skip { find = find.skip(sk as u64); }
        if let Some(lv) = user_limit { find = find.limit(lv); }
    }

    let mut cursor = find.await.map_err(AppError::Mongo)?;

    let mut docs = Vec::new();
    use futures::StreamExt;
    while let Some(doc) = cursor.next().await {
        let doc = doc.map_err(AppError::Mongo)?;
        docs.push(doc);
    }

    let count = docs.len() as i64;
    Ok(QueryResult { documents: docs, count, total_count: effective_total, execution_time_ms: 0 })
}

async fn execute_find_one(
    collection: &mongodb::Collection<Document>,
    rest: &str,
) -> Result<QueryResult, AppError> {
    let arg_str = extract_parens(rest, "findOne")?;
    let filter: Document = if arg_str.is_empty() { doc! {} } else { parse_json_arg(&arg_str)? };

    let result = collection.find_one(filter).await.map_err(AppError::Mongo)?;

    match result {
        Some(doc) => Ok(QueryResult { documents: vec![doc], count: 1, total_count: 1, execution_time_ms: 0 }),
        None => Ok(QueryResult { documents: vec![], count: 0, total_count: 0, execution_time_ms: 0 }),
    }
}

async fn execute_count(
    collection: &mongodb::Collection<Document>,
    rest: &str,
) -> Result<QueryResult, AppError> {
    let arg_str = extract_parens(rest, "countDocuments")?;
    let filter: Document = if arg_str.is_empty() { doc! {} } else { parse_json_arg(&arg_str)? };

    let count = collection.count_documents(filter).await.map_err(AppError::Mongo)? as i64;

    let result_doc = doc! { "count": count };
    Ok(QueryResult { documents: vec![result_doc], count, total_count: count, execution_time_ms: 0 })
}

async fn execute_aggregate(
    collection: &mongodb::Collection<Document>,
    rest: &str,
    pagination: Option<Pagination>,
) -> Result<QueryResult, AppError> {
    let arg_str = extract_parens(rest, "aggregate")?;
    let converted = convert_shell_types(&arg_str);
    let relaxed = relax_json(&converted);
    let mut pipeline: Vec<Document> = serde_json::from_str(&relaxed)
        .map_err(|e| AppError::InvalidInput(format!("无法解析聚合管道: {e}")))?;

    // 先用不带 skip/limit 的 pipeline 计算总数
    let mut count_pipeline = pipeline.clone();
    count_pipeline.push(doc! { "$count": "total" });
    let mut count_cursor = collection.aggregate(count_pipeline).await.map_err(AppError::Mongo)?;
    use futures::StreamExt;
    let total_count = if let Some(Ok(doc)) = count_cursor.next().await {
        doc.get_i64("total").or(doc.get_i32("total").map(|v| v as i64)).unwrap_or(0)
    } else {
        0
    };

    // 分页：在 pipeline 末尾追加 $skip 和 $limit
    if let Some(pg) = pagination {
        if pg.skip > 0 {
            pipeline.push(doc! { "$skip": pg.skip as i64 });
        }
        pipeline.push(doc! { "$limit": pg.page_size });
    }

    let mut cursor = collection.aggregate(pipeline).await.map_err(AppError::Mongo)?;

    let mut docs = Vec::new();
    while let Some(doc) = cursor.next().await {
        let doc = doc.map_err(AppError::Mongo)?;
        docs.push(doc);
    }

    let count = docs.len() as i64;
    Ok(QueryResult { documents: docs, count, total_count, execution_time_ms: 0 })
}

// ---- distinct ----

async fn execute_distinct(
    collection: &mongodb::Collection<Document>,
    rest: &str,
) -> Result<QueryResult, AppError> {
    let arg_str = extract_parens(rest, "distinct")?;
    // distinct("fieldName", filter?)
    let parts: Vec<&str> = arg_str.splitn(2, ',').collect();
    let field = parts[0].trim().trim_matches(|c| c == '"' || c == '\'');
    let filter: Document = if parts.len() > 1 {
        parse_json_arg(parts[1].trim())?
    } else {
        doc! {}
    };

    let values = collection.distinct(field, filter).await.map_err(AppError::Mongo)?;
    let docs: Vec<Document> = values.iter().enumerate().map(|(i, v)| {
        doc! { "_index": i as i64, "value": v.clone() }
    }).collect();
    let count = docs.len() as i64;
    Ok(QueryResult { documents: docs, count, total_count: count, execution_time_ms: 0 })
}

// ---- insertOne / insertMany ----

async fn execute_insert_one(
    collection: &mongodb::Collection<Document>,
    rest: &str,
) -> Result<QueryResult, AppError> {
    let arg_str = extract_parens(rest, "insertOne")?;
    let doc_to_insert: Document = parse_json_arg(&arg_str)?;
    let result = collection.insert_one(doc_to_insert).await.map_err(AppError::Mongo)?;
    let id = result.inserted_id;
    let result_doc = doc! { "acknowledged": true, "insertedId": id };
    Ok(QueryResult { documents: vec![result_doc], count: 1, total_count: 1, execution_time_ms: 0 })
}

async fn execute_insert_many(
    collection: &mongodb::Collection<Document>,
    rest: &str,
) -> Result<QueryResult, AppError> {
    let arg_str = extract_parens(rest, "insertMany")?;
    let converted = convert_shell_types(&arg_str);
    let relaxed = relax_json(&converted);
    let docs: Vec<Document> = serde_json::from_str(&relaxed)
        .map_err(|e| AppError::InvalidInput(format!("无法解析文档数组: {e}")))?;
    let count = docs.len() as i64;
    let result = collection.insert_many(docs).await.map_err(AppError::Mongo)?;
    let ids: Vec<mongodb::bson::Bson> = result.inserted_ids.values().cloned().collect();
    let result_doc = doc! { "acknowledged": true, "insertedCount": count, "insertedIds": ids };
    Ok(QueryResult { documents: vec![result_doc], count: 1, total_count: 1, execution_time_ms: 0 })
}

// ---- updateOne / updateMany ----

async fn execute_update(
    collection: &mongodb::Collection<Document>,
    rest: &str,
    method: &str,
    many: bool,
) -> Result<QueryResult, AppError> {
    let arg_str = extract_parens(rest, method)?;
    // 需要拆分两个参数: filter, update
    let (filter_str, update_str) = split_two_args(&arg_str)?;
    let filter: Document = parse_json_arg(&filter_str)?;
    let update: Document = parse_json_arg(&update_str)?;

    let result_doc = if many {
        let r = collection.update_many(filter, update).await.map_err(AppError::Mongo)?;
        doc! { "acknowledged": true, "matchedCount": r.matched_count as i64, "modifiedCount": r.modified_count as i64 }
    } else {
        let r = collection.update_one(filter, update).await.map_err(AppError::Mongo)?;
        doc! { "acknowledged": true, "matchedCount": r.matched_count as i64, "modifiedCount": r.modified_count as i64 }
    };
    Ok(QueryResult { documents: vec![result_doc], count: 1, total_count: 1, execution_time_ms: 0 })
}

// ---- deleteOne / deleteMany ----

async fn execute_delete(
    collection: &mongodb::Collection<Document>,
    rest: &str,
    method: &str,
    many: bool,
) -> Result<QueryResult, AppError> {
    let arg_str = extract_parens(rest, method)?;
    let filter: Document = parse_json_arg(&arg_str)?;

    let deleted = if many {
        collection.delete_many(filter).await.map_err(AppError::Mongo)?.deleted_count
    } else {
        collection.delete_one(filter).await.map_err(AppError::Mongo)?.deleted_count
    };
    let result_doc = doc! { "acknowledged": true, "deletedCount": deleted as i64 };
    Ok(QueryResult { documents: vec![result_doc], count: 1, total_count: 1, execution_time_ms: 0 })
}

// ---- replaceOne ----

async fn execute_replace_one(
    collection: &mongodb::Collection<Document>,
    rest: &str,
) -> Result<QueryResult, AppError> {
    let arg_str = extract_parens(rest, "replaceOne")?;
    let (filter_str, replacement_str) = split_two_args(&arg_str)?;
    let filter: Document = parse_json_arg(&filter_str)?;
    let replacement: Document = parse_json_arg(&replacement_str)?;
    let r = collection.replace_one(filter, replacement).await.map_err(AppError::Mongo)?;
    let result_doc = doc! { "acknowledged": true, "matchedCount": r.matched_count as i64, "modifiedCount": r.modified_count as i64 };
    Ok(QueryResult { documents: vec![result_doc], count: 1, total_count: 1, execution_time_ms: 0 })
}

/// 拆分函数参数中的两个顶层对象参数 (filter, update/replacement)
fn split_two_args(s: &str) -> Result<(String, String), AppError> {
    let mut depth = 0;
    let mut in_string = false;
    let mut str_char = ' ';
    for (i, ch) in s.char_indices() {
        if in_string {
            if ch == str_char && !s[..i].ends_with('\\') { in_string = false; }
            continue;
        }
        if ch == '"' || ch == '\'' { in_string = true; str_char = ch; continue; }
        if ch == '{' || ch == '[' { depth += 1; }
        if ch == '}' || ch == ']' { depth -= 1; }
        if ch == ',' && depth == 0 {
            let first = s[..i].trim().to_string();
            let second = s[i + 1..].trim().to_string();
            return Ok((first, second));
        }
    }
    Err(AppError::InvalidInput("需要两个参数: (filter, update/replacement)".into()))
}

/// 从链式调用中提取指定方法的参数
pub fn parse_chained_arg(chain: &str, method: &str) -> Option<String> {
    let start = chain.find(method)?;
    let after = &chain[start + method.len()..];
    let mut depth = 1;
    let mut end = 0;
    for (i, ch) in after.char_indices() {
        match ch {
            '(' | '{' | '[' => depth += 1,
            ')' => { depth -= 1; if depth == 0 { end = i; break; } }
            '}' | ']' => depth -= 1,
            _ => {}
        }
    }
    if depth != 0 { return None; }
    Some(after[..end].trim().to_string())
}

/// 从链式调用中提取 .limit(N)
fn parse_chained_limit(chain: &str) -> Option<i64> {
    let arg = parse_chained_arg(chain, ".limit(")?;
    arg.trim().parse::<i64>().ok()
}

/// 从链式调用中提取 .skip(N)
fn parse_chained_skip(chain: &str) -> Option<i64> {
    let arg = parse_chained_arg(chain, ".skip(")?;
    arg.trim().parse::<i64>().ok()
}

pub fn extract_parens(rest: &str, method: &str) -> Result<String, AppError> {
    let prefix = format!("{method}(");
    let inner = rest
        .strip_prefix(&prefix)
        .ok_or_else(|| AppError::InvalidInput(format!("预期 {method}(...)")))?;

    let mut depth = 1;
    let mut end = 0;
    for (i, ch) in inner.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => { depth -= 1; if depth == 0 { end = i; break; } }
            _ => {}
        }
    }

    if depth != 0 {
        return Err(AppError::InvalidInput("括号不匹配".into()));
    }

    Ok(inner[..end].trim().to_string())
}

pub fn parse_json_arg(s: &str) -> Result<Document, AppError> {
    if s.is_empty() { return Ok(doc! {}); }
    // 先尝试标准 JSON
    if let Ok(doc) = serde_json::from_str(s) {
        return Ok(doc);
    }
    // 宽松模式：处理 Shell 类型 + 给无引号的 key 加引号
    let shell_converted = convert_shell_types(s);
    let relaxed = relax_json(&shell_converted);
    serde_json::from_str(&relaxed)
        .map_err(|e| AppError::InvalidInput(format!("JSON 解析失败: {e}")))
}

/// 将 MongoDB Shell 类型构造器转为 Extended JSON 格式
/// ObjectId("abc") → {"$oid": "abc"}
/// ISODate("...") / new Date("...") → {"$date": "..."}
/// NumberLong("123") / NumberLong(123) → {"$numberLong": "123"}
/// NumberInt(42) → 42
/// NumberDecimal("1.5") → {"$numberDecimal": "1.5"}
fn convert_shell_types(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 64);
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        // 跳过字符串
        if c == '"' || c == '\'' {
            let quote = c;
            result.push('"'); // 统一用双引号
            i += 1;
            while i < len {
                let sc = chars[i];
                if sc == '\\' && i + 1 < len {
                    result.push(sc);
                    result.push(chars[i + 1]);
                    i += 2;
                } else if sc == quote {
                    result.push('"');
                    i += 1;
                    break;
                } else {
                    result.push(sc);
                    i += 1;
                }
            }
            continue;
        }

        // 尝试匹配 Shell 类型构造器
        if let Some((replacement, consumed)) = try_match_shell_type(&chars, i, len) {
            result.push_str(&replacement);
            i += consumed;
            continue;
        }

        result.push(c);
        i += 1;
    }

    result
}

/// 尝试从 chars[pos..] 匹配 MongoDB Shell 类型，返回 (替换文本, 消耗的字符数)
fn try_match_shell_type(chars: &[char], pos: usize, len: usize) -> Option<(String, usize)> {
    let remaining: String = chars[pos..std::cmp::min(pos + 20, len)].iter().collect();

    // 匹配的类型列表
    let type_names: &[(&str, &str)] = &[
        ("ObjectId(", "$oid"),
        ("ISODate(", "$date"),
        ("NumberLong(", "$numberLong"),
        ("NumberDecimal(", "$numberDecimal"),
    ];

    // new Date("...")
    if remaining.starts_with("new ") {
        let after_new: String = chars[pos + 4..std::cmp::min(pos + 20, len)].iter().collect();
        if after_new.starts_with("Date(") {
            let paren_start = pos + 9; // "new Date(" = 9 chars
            if let Some((arg, end)) = extract_paren_arg(chars, paren_start, len) {
                let val = arg.trim().trim_matches(|c| c == '"' || c == '\'');
                return Some((format!("{{\"$date\": \"{val}\"}}"), end - pos));
            }
        }
    }

    // NumberInt(42) → 直接输出数字
    if remaining.starts_with("NumberInt(") {
        let paren_start = pos + 10;
        if let Some((arg, end)) = extract_paren_arg(chars, paren_start, len) {
            let val = arg.trim().trim_matches(|c| c == '"' || c == '\'');
            return Some((val.to_string(), end - pos));
        }
    }

    for (prefix, ejson_key) in type_names {
        if remaining.starts_with(prefix) {
            let paren_start = pos + prefix.len();
            if let Some((arg, end)) = extract_paren_arg(chars, paren_start, len) {
                let val = arg.trim().trim_matches(|c| c == '"' || c == '\'');
                return Some((format!("{{\"{ejson_key}\": \"{val}\"}}"), end - pos));
            }
        }
    }

    None
}

/// 从 chars[start..] 提取括号内的参数，返回 (参数文本, 结束位置含')')
fn extract_paren_arg(chars: &[char], start: usize, len: usize) -> Option<(String, usize)> {
    let mut depth = 1;
    let mut i = start;
    let mut arg = String::new();
    while i < len {
        let c = chars[i];
        if c == '(' { depth += 1; }
        else if c == ')' {
            depth -= 1;
            if depth == 0 {
                return Some((arg, i + 1));
            }
        }
        arg.push(c);
        i += 1;
    }
    None
}

/// 将 MongoDB Shell 风格的宽松 JSON 转为标准 JSON
/// 处理: {name: "test", _id: 1, $gt: 5} → {"name": "test", "_id": 1, "$gt": 5}
fn relax_json(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 32);
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        // 跳过字符串内容
        if c == '"' {
            result.push(c);
            i += 1;
            while i < len {
                let sc = chars[i];
                result.push(sc);
                i += 1;
                if sc == '\\' && i < len {
                    result.push(chars[i]);
                    i += 1;
                } else if sc == '"' {
                    break;
                }
            }
            continue;
        }

        // 在 { 或 , 之后遇到无引号的标识符，后面跟 : → 加引号
        if (c.is_alphabetic() || c == '_' || c == '$') && is_key_position(&result) {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$' || chars[i] == '.') {
                i += 1;
            }
            // 跳过空白
            let mut j = i;
            while j < len && chars[j].is_whitespace() { j += 1; }
            // 如果后面是冒号，说明是 key
            if j < len && chars[j] == ':' {
                let key: String = chars[start..i].iter().collect();
                result.push('"');
                result.push_str(&key);
                result.push('"');
            } else {
                // 不是 key，可能是 true/false/null 或其他
                let word: String = chars[start..i].iter().collect();
                result.push_str(&word);
            }
            continue;
        }

        result.push(c);
        i += 1;
    }

    result
}

/// 检查当前位置是否是 JSON key 的合法起始位置
fn is_key_position(preceding: &str) -> bool {
    let trimmed = preceding.trim_end();
    if trimmed.is_empty() { return false; }
    let last = trimmed.chars().last().unwrap_or(' ');
    last == '{' || last == ','
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_parens_simple() {
        let result = extract_parens("find({})", "find").unwrap();
        assert_eq!(result, "{}");
    }

    #[test]
    fn extract_parens_nested() {
        let result = extract_parens(r#"find({"a": {"$gt": 1}})"#, "find").unwrap();
        assert_eq!(result, r#"{"a": {"$gt": 1}}"#);
    }

    #[test]
    fn parse_chained_limit_none() {
        let result = parse_chained_limit(".sort({_id:1})");
        assert_eq!(result, None);
    }

    #[test]
    fn parse_chained_limit_value() {
        let result = parse_chained_limit(".sort({_id:1}).limit(100)");
        assert_eq!(result, Some(100));
    }

    #[test]
    fn parse_chained_sort() {
        let result = parse_chained_arg(".projection({}).sort({_id:1}).limit(100)", ".sort(");
        assert_eq!(result.as_deref(), Some("{_id:1}"));
    }

    #[test]
    fn parse_chained_projection() {
        let result = parse_chained_arg(".projection({name:1}).sort({_id:1})", ".projection(");
        assert_eq!(result.as_deref(), Some("{name:1}"));
    }

    #[test]
    fn parse_json_empty() {
        let doc = parse_json_arg("").unwrap();
        assert_eq!(doc, doc! {});
    }

    #[test]
    fn parse_json_filter() {
        let doc = parse_json_arg(r#"{"name": "test"}"#).unwrap();
        assert_eq!(doc.get_str("name").unwrap(), "test");
    }

    #[test]
    fn parse_relaxed_json() {
        let doc = parse_json_arg(r#"{_id: 1, name: "test"}"#).unwrap();
        assert_eq!(doc.get_i64("_id").or(doc.get_i32("_id").map(|v| v as i64)).unwrap(), 1);
        assert_eq!(doc.get_str("name").unwrap(), "test");
    }

    #[test]
    fn parse_relaxed_dollar_key() {
        let doc = parse_json_arg(r#"{$gt: 5}"#).unwrap();
        assert!(doc.get("$gt").is_some());
    }

    #[test]
    fn parse_objectid() {
        let doc = parse_json_arg(r#"{_id: ObjectId("692fb5a01252331990f2914e")}"#).unwrap();
        assert!(doc.get("_id").is_some());
    }

    #[test]
    fn convert_objectid() {
        let result = convert_shell_types(r#"{_id: ObjectId("abc123")}"#);
        assert!(result.contains(r#"{"$oid": "abc123"}"#));
    }

    #[test]
    fn convert_isodate() {
        let result = convert_shell_types(r#"{date: ISODate("2024-01-01")}"#);
        assert!(result.contains(r#"{"$date": "2024-01-01"}"#));
    }

    #[test]
    fn convert_new_date() {
        let result = convert_shell_types(r#"{date: new Date("2024-01-01")}"#);
        assert!(result.contains(r#"{"$date": "2024-01-01"}"#));
    }

    #[test]
    fn convert_number_long() {
        let result = convert_shell_types(r#"{n: NumberLong("123")}"#);
        assert!(result.contains(r#"{"$numberLong": "123"}"#));
    }

    #[test]
    fn convert_number_int() {
        let result = convert_shell_types(r#"{n: NumberInt(42)}"#);
        assert!(result.contains("42"));
        assert!(!result.contains("NumberInt"));
    }
}

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
    /// 匹配条件的总文档数（不受 limit 限制）。值为 -1 表示后端正在异步计数，
    /// 前端应等待 `query:count-ready` 事件到达后再显示精确总数。
    pub total_count: i64,
    pub execution_time_ms: i64,
    /// 非 None 时表示调用方需要在后台补算 total_count（过滤非空时的延迟 count）。
    /// 不序列化到前端。
    #[serde(skip)]
    pub pending_count: Option<PendingCount>,
}

/// 延迟计数任务的上下文：`run_query` 拿到后会 spawn 一个 tokio task
/// 调用 `count_documents` 并通过 Tauri 事件推回前端。
#[derive(Debug, Clone)]
pub struct PendingCount {
    pub collection_name: String,
    pub filter: Document,
    /// 用户原始 `.limit(N)`，用于 UI 侧 `min(count, limit)` 的效果计算。
    pub user_limit: Option<i64>,
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
    // 剥掉前导/中间的 //... 与 /* ... */ 注释 (允许脚本里写说明),
    // 否则 db. 前缀检查会被首行注释卡住.
    let cleaned = strip_comments(query_text);
    let query = cleaned.trim();
    let start = std::time::Instant::now();

    // ---- show 元命令 (show dbs / collections / users / roles / profile) ----
    if let Some(rest) = query.strip_prefix("show ") {
        let what = rest.trim();
        let documents: Vec<Document> = match what {
            "dbs" | "databases" => {
                let dbs = client.list_databases().await.map_err(AppError::Mongo)?;
                dbs.into_iter()
                    .map(|d| {
                        doc! {
                            "name": d.name,
                            "sizeOnDisk": d.size_on_disk as i64,
                            "empty": d.size_on_disk == 0,
                        }
                    })
                    .collect()
            }
            "collections" | "tables" => {
                let names = client
                    .database(database)
                    .list_collection_names()
                    .await
                    .map_err(AppError::Mongo)?;
                names.into_iter().map(|n| doc! { "name": n }).collect()
            }
            "users" => {
                let result = client
                    .database(database)
                    .run_command(doc! { "usersInfo": 1 })
                    .await
                    .map_err(AppError::Mongo)?;
                result
                    .get_array("users")
                    .ok()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|b| b.as_document().cloned())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            }
            "roles" => {
                let result = client
                    .database(database)
                    .run_command(doc! { "rolesInfo": 1, "showBuiltinRoles": true })
                    .await
                    .map_err(AppError::Mongo)?;
                result
                    .get_array("roles")
                    .ok()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|b| b.as_document().cloned())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            }
            "profile" => {
                let coll = client
                    .database(database)
                    .collection::<Document>("system.profile");
                let mut cursor = coll
                    .find(doc! {})
                    .sort(doc! { "ts": -1 })
                    .limit(20)
                    .await
                    .map_err(AppError::Mongo)?;
                let mut docs = Vec::new();
                use futures::StreamExt;
                while let Some(d) = cursor.next().await {
                    docs.push(d.map_err(AppError::Mongo)?);
                }
                docs
            }
            other => {
                return Err(AppError::InvalidInput(format!(
                    "不支持的 show 命令: \"{}\" (支持 dbs / collections / users / roles / profile)",
                    other
                )));
            }
        };
        let count = documents.len() as i64;
        let elapsed = start.elapsed().as_millis() as i64;
        return Ok(QueryResult {
            documents,
            count,
            total_count: count,
            execution_time_ms: elapsed,
            pending_count: None,
        });
    }

    let query = if let Some(stripped) = query.strip_prefix("db.") {
        stripped
    } else {
        return Err(AppError::InvalidInput(
            "查询必须以 db. 开头，例如 db.collection.find({})".into(),
        ));
    };

    // ---- db.getSiblingDB("otherDb").xxx() → 切换目标数据库 ----
    let (effective_db_name, query) = if query.starts_with("getSiblingDB(") {
        let gc_end = query
            .find(')')
            .ok_or_else(|| AppError::InvalidInput("getSiblingDB() 括号不匹配".into()))?;
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
        let users = result_doc
            .get_array("users")
            .ok()
            .map(|arr| {
                arr.iter()
                    .filter_map(|b| b.as_document().cloned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let count = users.len() as i64;
        let elapsed = start.elapsed().as_millis() as i64;
        return Ok(QueryResult {
            documents: users,
            count,
            total_count: count,
            execution_time_ms: elapsed,
            pending_count: None,
        });
    }

    if query.starts_with("getRole(") {
        let arg_str = extract_parens(query, "getRole")?;
        // getRole("roleName", {showPrivileges:true, showBuiltinRoles: true})
        let parts: Vec<&str> = arg_str.splitn(2, ',').collect();
        let role_name = parts[0]
            .trim()
            .trim_matches(|c: char| c == '"' || c == '\'');
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
        let roles = result_doc
            .get_array("roles")
            .ok()
            .map(|arr| {
                arr.iter()
                    .filter_map(|b| b.as_document().cloned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let count = roles.len() as i64;
        let elapsed = start.elapsed().as_millis() as i64;
        return Ok(QueryResult {
            documents: roles,
            count,
            total_count: count,
            execution_time_ms: elapsed,
            pending_count: None,
        });
    }

    if query.starts_with("getUsers(") {
        let result_doc = db
            .run_command(doc! { "usersInfo": 1 })
            .await
            .map_err(AppError::Mongo)?;
        let users = result_doc
            .get_array("users")
            .ok()
            .map(|arr| {
                arr.iter()
                    .filter_map(|b| b.as_document().cloned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let count = users.len() as i64;
        let elapsed = start.elapsed().as_millis() as i64;
        return Ok(QueryResult {
            documents: users,
            count,
            total_count: count,
            execution_time_ms: elapsed,
            pending_count: None,
        });
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
        let roles = result_doc
            .get_array("roles")
            .ok()
            .map(|arr| {
                arr.iter()
                    .filter_map(|b| b.as_document().cloned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let count = roles.len() as i64;
        let elapsed = start.elapsed().as_millis() as i64;
        return Ok(QueryResult {
            documents: roles,
            count,
            total_count: count,
            execution_time_ms: elapsed,
            pending_count: None,
        });
    }

    if query.starts_with("dropUser(") {
        let arg_str = extract_parens(query, "dropUser")?;
        let username = arg_str.trim().trim_matches(|c: char| c == '"' || c == '\'');
        db.run_command(doc! { "dropUser": username })
            .await
            .map_err(AppError::Mongo)?;
        let result_doc = doc! { "ok": 1, "dropped": username };
        let elapsed = start.elapsed().as_millis() as i64;
        return Ok(QueryResult {
            documents: vec![result_doc],
            count: 1,
            total_count: 1,
            execution_time_ms: elapsed,
            pending_count: None,
        });
    }

    if query.starts_with("createUser(") {
        let arg_str = extract_parens(query, "createUser")?;
        let no_comments = strip_comments(&arg_str);
        let converted = convert_shell_types(&no_comments);
        let relaxed = relax_json(&converted);
        let cleaned = strip_trailing_commas(&relaxed);
        let user_doc: Document = serde_json::from_str(&cleaned)
            .map_err(|e| AppError::InvalidInput(format!("无法解析用户文档: {e}")))?;
        let mut cmd = doc! { "createUser": user_doc.get_str("user").unwrap_or("") };
        if let Some(pwd) = user_doc.get_str("pwd").ok() {
            cmd.insert("pwd", pwd);
        }
        if let Some(roles) = user_doc.get_array("roles").ok() {
            cmd.insert("roles", roles.clone());
        }
        db.run_command(cmd).await.map_err(AppError::Mongo)?;
        let result_doc = doc! { "ok": 1, "user": user_doc.get_str("user").unwrap_or("") };
        let elapsed = start.elapsed().as_millis() as i64;
        return Ok(QueryResult {
            documents: vec![result_doc],
            count: 1,
            total_count: 1,
            execution_time_ms: elapsed,
            pending_count: None,
        });
    }

    // 支持 db.getCollection("name.with.dots").method() 和 db.collName.method()
    let (collection_name, rest) = if query.starts_with("getCollection(") {
        // db.getCollection("system.version").find({})
        let gc_end = query
            .find(')')
            .ok_or_else(|| AppError::InvalidInput("getCollection() 括号不匹配".into()))?;
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
        return Err(AppError::InvalidInput(format!("不支持的操作: {rest}")));
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
    let args_str = extract_parens(rest, "find")?;
    // find 接受 (filter) 或 (filter, projection); 用顶层逗号拆分.
    let (filter_str, inline_proj_str) = match split_two_args(&args_str) {
        Ok((f, p)) => (f, Some(p)),
        Err(_) => (args_str.clone(), None),
    };
    let filter: Document = if filter_str.trim().is_empty() {
        doc! {}
    } else {
        parse_json_arg(&filter_str)?
    };

    // 解析链式调用：.projection({}) .sort({}) .limit(N) .skip(N)
    let after_find = &rest[rest.find(')').unwrap_or(rest.len()) + 1..];
    let chained_projection = parse_chained_arg(after_find, ".projection(");
    let sort = parse_chained_arg(after_find, ".sort(");
    let user_limit = parse_chained_limit(after_find);
    let user_skip = parse_chained_skip(after_find);

    // projection 优先级: 链式 .projection() > 内联第 2 参 (mongosh 行为)
    let proj_to_use = chained_projection.or(inline_proj_str);
    let mut proj_doc: Option<Document> = None;
    if let Some(proj_str) = proj_to_use {
        let d: Document = parse_json_arg(&proj_str)?;
        if !d.is_empty() {
            proj_doc = Some(d);
        }
    }
    let mut sort_doc: Option<Document> = None;
    if let Some(sort_str) = sort {
        let d: Document = parse_json_arg(&sort_str)?;
        if !d.is_empty() {
            sort_doc = Some(d);
        }
    }

    // 计数策略：
    //  · 空 filter → 走 O(1) 的 estimated_document_count (读集合元数据)
    //  · 非空 filter → 设 -1 哨兵, 让 run_query 在后台异步 count_documents
    // 这样大集合 find 不再被前置的精确 count 卡几秒.
    let (total_count, pending_count) = if filter.is_empty() {
        let c = collection
            .estimated_document_count()
            .await
            .map_err(AppError::Mongo)? as i64;
        (c, None)
    } else {
        (
            -1_i64,
            Some(PendingCount {
                collection_name: collection.name().to_string(),
                filter: filter.clone(),
                user_limit,
            }),
        )
    };

    // 计算有效总数（考虑用户 limit）。pending 时保留 -1.
    let effective_total = if total_count < 0 {
        -1
    } else {
        match user_limit {
            Some(ul) => std::cmp::min(total_count, ul),
            None => total_count,
        }
    };

    let mut find = collection.find(filter);
    if let Some(p) = proj_doc {
        find = find.projection(p);
    }
    if let Some(s) = sort_doc {
        find = find.sort(s);
    }

    // 分页逻辑
    if let Some(pg) = pagination {
        // 后端分页：在用户 skip 基础上叠加分页 skip
        let base_skip = user_skip.unwrap_or(0) as u64;
        find = find.skip(base_skip + pg.skip);

        // limit = 每页大小，但不超过用户 limit 剩余量
        let page_limit = if let Some(ul) = user_limit {
            let already_skipped = pg.skip as i64;
            let remaining = ul - already_skipped;
            if remaining <= 0 {
                0
            } else {
                std::cmp::min(pg.page_size, remaining)
            }
        } else {
            pg.page_size
        };
        find = find.limit(page_limit);
    } else {
        // 无分页参数：兼容旧行为，用用户原始 skip/limit
        if let Some(sk) = user_skip {
            find = find.skip(sk as u64);
        }
        if let Some(lv) = user_limit {
            find = find.limit(lv);
        }
    }

    let mut cursor = find.await.map_err(AppError::Mongo)?;

    let mut docs = Vec::new();
    use futures::StreamExt;
    while let Some(doc) = cursor.next().await {
        let doc = doc.map_err(AppError::Mongo)?;
        docs.push(doc);
    }

    let count = docs.len() as i64;
    Ok(QueryResult {
        documents: docs,
        count,
        total_count: effective_total,
        execution_time_ms: 0,
        pending_count,
    })
}

async fn execute_find_one(
    collection: &mongodb::Collection<Document>,
    rest: &str,
) -> Result<QueryResult, AppError> {
    let arg_str = extract_parens(rest, "findOne")?;
    let filter: Document = if arg_str.is_empty() {
        doc! {}
    } else {
        parse_json_arg(&arg_str)?
    };

    let result = collection.find_one(filter).await.map_err(AppError::Mongo)?;

    match result {
        Some(doc) => Ok(QueryResult {
            documents: vec![doc],
            count: 1,
            total_count: 1,
            execution_time_ms: 0,
            pending_count: None,
        }),
        None => Ok(QueryResult {
            documents: vec![],
            count: 0,
            total_count: 0,
            execution_time_ms: 0,
            pending_count: None,
        }),
    }
}

async fn execute_count(
    collection: &mongodb::Collection<Document>,
    rest: &str,
) -> Result<QueryResult, AppError> {
    let arg_str = extract_parens(rest, "countDocuments")?;
    let filter: Document = if arg_str.is_empty() {
        doc! {}
    } else {
        parse_json_arg(&arg_str)?
    };

    let count = collection
        .count_documents(filter)
        .await
        .map_err(AppError::Mongo)? as i64;

    let result_doc = doc! { "count": count };
    Ok(QueryResult {
        documents: vec![result_doc],
        count,
        total_count: count,
        execution_time_ms: 0,
        pending_count: None,
    })
}

async fn execute_aggregate(
    collection: &mongodb::Collection<Document>,
    rest: &str,
    pagination: Option<Pagination>,
) -> Result<QueryResult, AppError> {
    let arg_str = extract_parens(rest, "aggregate")?;
    let mut pipeline = aggregate_pipeline_from_arg(&arg_str)?;

    // 先用不带 skip/limit 的 pipeline 计算总数
    let mut count_pipeline = pipeline.clone();
    count_pipeline.push(doc! { "$count": "total" });
    let mut count_cursor = collection
        .aggregate(count_pipeline)
        .await
        .map_err(AppError::Mongo)?;
    use futures::StreamExt;
    let total_count = if let Some(Ok(doc)) = count_cursor.next().await {
        doc.get_i64("total")
            .or(doc.get_i32("total").map(|v| v as i64))
            .unwrap_or(0)
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

    let mut cursor = collection
        .aggregate(pipeline)
        .await
        .map_err(AppError::Mongo)?;

    let mut docs = Vec::new();
    while let Some(doc) = cursor.next().await {
        let doc = doc.map_err(AppError::Mongo)?;
        docs.push(doc);
    }

    let count = docs.len() as i64;
    Ok(QueryResult {
        documents: docs,
        count,
        total_count,
        execution_time_ms: 0,
        pending_count: None,
    })
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

    let values = collection
        .distinct(field, filter)
        .await
        .map_err(AppError::Mongo)?;
    let docs: Vec<Document> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            doc! { "_index": i as i64, "value": v.clone() }
        })
        .collect();
    let count = docs.len() as i64;
    Ok(QueryResult {
        documents: docs,
        count,
        total_count: count,
        execution_time_ms: 0,
        pending_count: None,
    })
}

// ---- insertOne / insertMany ----

async fn execute_insert_one(
    collection: &mongodb::Collection<Document>,
    rest: &str,
) -> Result<QueryResult, AppError> {
    let arg_str = extract_parens(rest, "insertOne")?;
    let doc_to_insert: Document = parse_json_arg(&arg_str)?;
    let result = collection
        .insert_one(doc_to_insert)
        .await
        .map_err(AppError::Mongo)?;
    let id = result.inserted_id;
    let result_doc = doc! { "acknowledged": true, "insertedId": id };
    Ok(QueryResult {
        documents: vec![result_doc],
        count: 1,
        total_count: 1,
        execution_time_ms: 0,
        pending_count: None,
    })
}

async fn execute_insert_many(
    collection: &mongodb::Collection<Document>,
    rest: &str,
) -> Result<QueryResult, AppError> {
    let arg_str = extract_parens(rest, "insertMany")?;
    let no_comments = strip_comments(&arg_str);
    let converted = convert_shell_types(&no_comments);
    let relaxed = relax_json(&converted);
    let cleaned = strip_trailing_commas(&relaxed);
    let docs: Vec<Document> = serde_json::from_str(&cleaned)
        .map_err(|e| AppError::InvalidInput(format!("无法解析文档数组: {e}")))?;
    let count = docs.len() as i64;
    let result = collection
        .insert_many(docs)
        .await
        .map_err(AppError::Mongo)?;
    let ids: Vec<mongodb::bson::Bson> = result.inserted_ids.values().cloned().collect();
    let result_doc = doc! { "acknowledged": true, "insertedCount": count, "insertedIds": ids };
    Ok(QueryResult {
        documents: vec![result_doc],
        count: 1,
        total_count: 1,
        execution_time_ms: 0,
        pending_count: None,
    })
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
        let r = collection
            .update_many(filter, update)
            .await
            .map_err(AppError::Mongo)?;
        doc! { "acknowledged": true, "matchedCount": r.matched_count as i64, "modifiedCount": r.modified_count as i64 }
    } else {
        let r = collection
            .update_one(filter, update)
            .await
            .map_err(AppError::Mongo)?;
        doc! { "acknowledged": true, "matchedCount": r.matched_count as i64, "modifiedCount": r.modified_count as i64 }
    };
    Ok(QueryResult {
        documents: vec![result_doc],
        count: 1,
        total_count: 1,
        execution_time_ms: 0,
        pending_count: None,
    })
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
        collection
            .delete_many(filter)
            .await
            .map_err(AppError::Mongo)?
            .deleted_count
    } else {
        collection
            .delete_one(filter)
            .await
            .map_err(AppError::Mongo)?
            .deleted_count
    };
    let result_doc = doc! { "acknowledged": true, "deletedCount": deleted as i64 };
    Ok(QueryResult {
        documents: vec![result_doc],
        count: 1,
        total_count: 1,
        execution_time_ms: 0,
        pending_count: None,
    })
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
    let r = collection
        .replace_one(filter, replacement)
        .await
        .map_err(AppError::Mongo)?;
    let result_doc = doc! { "acknowledged": true, "matchedCount": r.matched_count as i64, "modifiedCount": r.modified_count as i64 };
    Ok(QueryResult {
        documents: vec![result_doc],
        count: 1,
        total_count: 1,
        execution_time_ms: 0,
        pending_count: None,
    })
}

/// 从 chars[start] (须是 " ' ` 之一) 跳过整个字符串字面量, 返回闭合引号后的下标.
/// 未闭合时返回 chars.len(). 转义对 (\x) 整体跳过, "C:\\" 不会误判.
fn skip_string_literal(chars: &[char], start: usize) -> usize {
    let quote = chars[start];
    let len = chars.len();
    let mut i = start + 1;
    while i < len {
        let c = chars[i];
        if c == '\\' {
            i += 2;
            continue;
        }
        if c == quote {
            return i + 1;
        }
        i += 1;
    }
    len
}

/// 拆分函数参数中的两个顶层对象参数 (filter, update/replacement)
fn split_two_args(s: &str) -> Result<(String, String), AppError> {
    let chars: Vec<char> = s.chars().collect();
    let mut depth = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' || c == '`' {
            i = skip_string_literal(&chars, i);
            continue;
        }
        match c {
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth -= 1,
            ',' if depth == 0 => {
                let first: String = chars[..i].iter().collect();
                let second: String = chars[i + 1..].iter().collect();
                return Ok((first.trim().to_string(), second.trim().to_string()));
            }
            _ => {}
        }
        i += 1;
    }
    Err(AppError::InvalidInput(
        "需要两个参数: (filter, update/replacement)".into(),
    ))
}

/// 从链式调用中提取指定方法的参数
pub fn parse_chained_arg(chain: &str, method: &str) -> Option<String> {
    let start = chain.find(method)?;
    let after: Vec<char> = chain[start + method.len()..].chars().collect();
    let mut depth = 1;
    let mut i = 0;
    while i < after.len() {
        let c = after[i];
        if c == '"' || c == '\'' || c == '`' {
            i = skip_string_literal(&after, i);
            continue;
        }
        match c {
            '(' | '{' | '[' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let arg: String = after[..i].iter().collect();
                    return Some(arg.trim().to_string());
                }
            }
            '}' | ']' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    None
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

    let chars: Vec<char> = inner.chars().collect();
    let mut depth = 1;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' || c == '`' {
            i = skip_string_literal(&chars, i);
            continue;
        }
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let result: String = chars[..i].iter().collect();
                    return Ok(result.trim().to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }

    Err(AppError::InvalidInput("括号不匹配".into()))
}

/// 把 `aggregate(...)` 括号内的原始字符串解析成 pipeline 文档数组.
/// 与 `execute_aggregate` 共用一致的宽松处理: 注释 + shell 类型 + 无引号 key + 尾随逗号.
pub fn aggregate_pipeline_from_arg(arg_str: &str) -> Result<Vec<Document>, AppError> {
    let no_comments = strip_comments(arg_str);
    let converted = convert_shell_types(&no_comments);
    let relaxed = relax_json(&converted);
    let cleaned = strip_trailing_commas(&relaxed);
    serde_json::from_str(&cleaned)
        .map_err(|e| AppError::InvalidInput(format!("无法解析聚合管道: {e}")))
}

pub fn parse_json_arg(s: &str) -> Result<Document, AppError> {
    if s.is_empty() {
        return Ok(doc! {});
    }
    // 先尝试标准 JSON
    if let Ok(doc) = serde_json::from_str(s) {
        return Ok(doc);
    }
    // 宽松模式: 注释 + shell 类型 + 无引号 key + 尾随逗号
    let no_comments = strip_comments(s);
    let shell_converted = convert_shell_types(&no_comments);
    let relaxed = relax_json(&shell_converted);
    let cleaned = strip_trailing_commas(&relaxed);
    serde_json::from_str(&cleaned)
        .map_err(|e| AppError::InvalidInput(format!("JSON 解析失败: {e}")))
}

/// 剥掉 JS 风格注释 (`//...` 行注释 与 `/* ... */` 块注释).
/// 字符串内的同样写法会被保留.
fn strip_comments(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;
    while i < len {
        let c = chars[i];
        // 字符串: 复制到结束引号 (含反引号模板串, 内容可跨行)
        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            out.push(c);
            i += 1;
            while i < len {
                let sc = chars[i];
                out.push(sc);
                i += 1;
                if sc == '\\' && i < len {
                    out.push(chars[i]);
                    i += 1;
                } else if sc == quote {
                    break;
                }
            }
            continue;
        }
        // 行注释 //... 到行尾
        if c == '/' && i + 1 < len && chars[i + 1] == '/' {
            i += 2;
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        // 块注释 /* ... */
        if c == '/' && i + 1 < len && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(len);
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
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

        // 字符串: 重新输出为合法 JSON 双引号字符串 (含反引号模板串; ${} 不求值, 按字面输出)
        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            result.push('"'); // 统一用双引号
            i += 1;
            while i < len {
                let sc = chars[i];
                if sc == '\\' && i + 1 < len {
                    let next = chars[i + 1];
                    if next == '\'' || next == '`' || next == '$' {
                        // JSON 不认 \' \` \$ 转义, 还原字符本身
                        result.push(next);
                    } else {
                        result.push(sc);
                        result.push(next);
                    }
                    i += 2;
                } else if sc == quote {
                    result.push('"');
                    i += 1;
                    break;
                } else if sc == '"' {
                    // 单引号字符串里的裸双引号要转义
                    result.push_str("\\\"");
                    i += 1;
                } else if (sc as u32) < 0x20 {
                    // 粘贴的多行文本会把真实换行/制表符带进字符串, JSON 不容裸控制字符
                    match sc {
                        '\n' => result.push_str("\\n"),
                        '\r' => result.push_str("\\r"),
                        '\t' => result.push_str("\\t"),
                        _ => result.push_str(&format!("\\u{:04x}", sc as u32)),
                    }
                    i += 1;
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
        let after_new: String = chars[pos + 4..std::cmp::min(pos + 20, len)]
            .iter()
            .collect();
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

    // Double("3.4") / Double(3.4) → 直接输出数字 (serde_json 会当成 f64 解析)
    if remaining.starts_with("Double(") {
        let paren_start = pos + 7;
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
        if c == '(' {
            depth += 1;
        } else if c == ')' {
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
            while i < len
                && (chars[i].is_alphanumeric()
                    || chars[i] == '_'
                    || chars[i] == '$'
                    || chars[i] == '.')
            {
                i += 1;
            }
            // 跳过空白
            let mut j = i;
            while j < len && chars[j].is_whitespace() {
                j += 1;
            }
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
    if trimmed.is_empty() {
        return false;
    }
    let last = trimmed.chars().last().unwrap_or(' ');
    last == '{' || last == ','
}

/// 去除尾随逗号: `{a:1, b:2,}` → `{a:1, b:2}`, `[1, 2,]` → `[1, 2]`
/// 字符串内的逗号不动.
fn strip_trailing_commas(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut result = String::with_capacity(len);
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
        if c == ',' {
            // 看后面是否只跟空白后跟 } 或 ]
            let mut j = i + 1;
            while j < len && chars[j].is_whitespace() {
                j += 1;
            }
            if j < len && (chars[j] == '}' || chars[j] == ']') {
                // 丢弃这个逗号
                i += 1;
                continue;
            }
        }
        result.push(c);
        i += 1;
    }
    result
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
        assert_eq!(
            doc.get_i64("_id")
                .or(doc.get_i32("_id").map(|v| v as i64))
                .unwrap(),
            1
        );
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
    fn parse_multiline_string_value() {
        // 粘贴的多行文本: 字符串里带真实换行/制表符 (JSON 不容裸控制字符)
        let doc = parse_json_arg("{desc: \"line1\nline2\tend\"}").unwrap();
        assert_eq!(doc.get_str("desc").unwrap(), "line1\nline2\tend");
    }

    #[test]
    fn parse_single_quoted_with_double_quote() {
        // 单引号字符串里的裸双引号与 \' 转义都要转成合法 JSON
        let doc = parse_json_arg(r#"{note: 'say "hi" don\'t stop'}"#).unwrap();
        assert_eq!(doc.get_str("note").unwrap(), r#"say "hi" don't stop"#);
    }

    #[test]
    fn parse_backtick_template_string() {
        // 反引号模板串: 跨行 + 裸双引号 + 单引号
        let doc = parse_json_arg("{desc: `<p>line1</p>\n<p>say \"hi\" it's</p>`}").unwrap();
        assert_eq!(
            doc.get_str("desc").unwrap(),
            "<p>line1</p>\n<p>say \"hi\" it's</p>"
        );
    }

    #[test]
    fn split_two_args_ignores_string_content() {
        // 模板串里的逗号/花括号不参与参数拆分
        let (f, u) = split_two_args("{a:1}, {$set:{d:`x, y {z}`}}").unwrap();
        assert_eq!(f, "{a:1}");
        assert_eq!(u, "{$set:{d:`x, y {z}`}}");
    }

    #[test]
    fn extract_parens_ignores_paren_in_string() {
        let result = extract_parens(r#"find({name: "a(b"})"#, "find").unwrap();
        assert_eq!(result, r#"{name: "a(b"}"#);
    }

    #[test]
    fn extjson_oid_array_deserializes_as_objectid() {
        // 模拟前端发来的 delete 过滤: $in 数组里是 {$oid: "..."}
        let json = r#"{"_id":{"$in":[{"$oid":"695dbe26a0777a90234d4f29"},{"$oid":"69adfb78a0777a90234d7d73"}]}}"#;
        let val: serde_json::Value = serde_json::from_str(json).unwrap();
        let doc: mongodb::bson::Document = serde_json::from_value(val).unwrap();
        let id = doc
            .get("_id")
            .unwrap()
            .as_document()
            .expect("_id 应是嵌套文档");
        let arr = id.get_array("$in").expect("$in 应是数组");
        for v in arr {
            // 这里就是问题点: 若 $oid 没被识别成 ObjectId, 这个 assert 会挂掉
            assert!(
                v.as_object_id().is_some(),
                "$in 元素必须是 ObjectId, 实际是 {:?}",
                v.element_type()
            );
        }
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

    #[test]
    fn convert_double_quoted() {
        let result = convert_shell_types(r#"{n: Double("3.4")}"#);
        assert!(result.contains("3.4"));
        assert!(!result.contains("Double"));
    }

    #[test]
    fn convert_double_unquoted() {
        let result = convert_shell_types(r#"{n: Double(3.4)}"#);
        assert!(result.contains("3.4"));
        assert!(!result.contains("Double"));
    }

    #[test]
    fn parse_double_in_set() {
        let doc = parse_json_arg(r#"{count: Double("3.4")}"#).unwrap();
        let v = doc.get("count").unwrap();
        // 解析后应是 f64
        assert!(v.as_f64().is_some());
    }

    #[test]
    fn strip_trailing_comma_object() {
        let result = strip_trailing_commas(r#"{"a": 1, "b": 2,}"#);
        assert_eq!(result, r#"{"a": 1, "b": 2}"#);
    }

    #[test]
    fn strip_trailing_comma_array() {
        let result = strip_trailing_commas(r#"[1, 2, 3,]"#);
        assert_eq!(result, r#"[1, 2, 3]"#);
    }

    #[test]
    fn strip_trailing_comma_nested() {
        let result = strip_trailing_commas(r#"[{"a": 1,}, {"b": 2,},]"#);
        assert_eq!(result, r#"[{"a": 1}, {"b": 2}]"#);
    }

    #[test]
    fn strip_trailing_comma_preserves_string_comma() {
        // 字符串里的 ", ]" 不应被剥掉
        let result = strip_trailing_commas(r#"{"k": "v,]"}"#);
        assert_eq!(result, r#"{"k": "v,]"}"#);
    }

    #[test]
    fn parse_json_arg_trailing_comma() {
        // 用户报错的 case 简化版
        let doc = parse_json_arg(r#"{"$unwind": "$attachment",}"#).unwrap();
        assert_eq!(doc.get_str("$unwind").unwrap(), "$attachment");
    }

    #[test]
    fn strip_line_comment() {
        let result = strip_comments("{a:1, // hello\nb:2}");
        assert!(!result.contains("hello"));
        assert!(result.contains("a:1"));
        assert!(result.contains("b:2"));
    }

    #[test]
    fn strip_block_comment() {
        let result = strip_comments("{a:1, /* hello */ b:2}");
        assert!(!result.contains("hello"));
        assert!(result.contains("a:1"));
        assert!(result.contains("b:2"));
    }

    #[test]
    fn strip_comment_inside_string_preserved() {
        // 字符串里的 // 不应被剥
        let result = strip_comments(r#"{url: "http://example.com"}"#);
        assert!(result.contains("http://example.com"));
    }

    #[test]
    fn parse_json_arg_with_line_comment() {
        let doc = parse_json_arg("{ a: 1, // tail\n b: 2 }").unwrap();
        assert_eq!(doc.get_i32("a").unwrap_or(0), 1);
        assert_eq!(doc.get_i32("b").unwrap_or(0), 2);
    }
}

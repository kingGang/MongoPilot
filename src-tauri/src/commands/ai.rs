use serde::{Deserialize, Serialize};
use tauri::State;

use crate::ai::client::{self, AgentMessage, AiConfig, AiTurn, ChatMessage, ToolDef};
use crate::ai::prompt;
use crate::ai::schema::{self, SchemaInfo};
use crate::connection::manager::ConnectionManager;
use crate::error::AppError;
use crate::storage::ai_repo::{self, ConversationRow, FactRow, MessageRow};
use crate::storage::database::DbPool;
use crate::storage::settings_repo;

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
    pub temperature: Option<f64>,
}

#[tauri::command]
pub async fn get_ai_settings(pool: State<'_, DbPool>) -> Result<Option<AiSettings>, AppError> {
    let json = settings_repo::get_setting(&pool, "ai.config").await?;
    match json {
        Some(s) => {
            let settings: AiSettings = serde_json::from_str(&s)
                .map_err(|e| AppError::InvalidInput(format!("解析 AI 配置失败: {e}")))?;
            Ok(Some(settings))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn save_ai_settings(
    pool: State<'_, DbPool>,
    settings: AiSettings,
) -> Result<(), AppError> {
    let json = serde_json::to_string(&settings)
        .map_err(|e| AppError::InvalidInput(format!("序列化 AI 配置失败: {e}")))?;
    settings_repo::set_setting(&pool, "ai.config", &json).await
}

/// AI 规范 (Rules) —— 用户手写的一段 markdown/纯文本, 会被拼进每轮 system prompt.
/// scope = "global" 或 "conn:<connectionId>" (连接级), 复用 app_settings 表 (key = "ai.rules.<scope>").
fn rules_key(scope: &str) -> String {
    format!("ai.rules.{}", scope)
}

#[tauri::command]
pub async fn get_ai_rules(
    pool: State<'_, DbPool>,
    scope: String,
) -> Result<String, AppError> {
    Ok(settings_repo::get_setting(&pool, &rules_key(&scope))
        .await?
        .unwrap_or_default())
}

#[tauri::command]
pub async fn save_ai_rules(
    pool: State<'_, DbPool>,
    scope: String,
    content: String,
) -> Result<(), AppError> {
    settings_repo::set_setting(&pool, &rules_key(&scope), &content).await
}

async fn load_ai_config(pool: &DbPool) -> Result<AiConfig, AppError> {
    let json = settings_repo::get_setting(pool, "ai.config")
        .await?
        .ok_or_else(|| AppError::InvalidInput("请先配置 AI 设置".into()))?;
    let settings: AiSettings = serde_json::from_str(&json)
        .map_err(|e| AppError::InvalidInput(format!("解析 AI 配置失败: {e}")))?;
    Ok(AiConfig {
        provider: settings.provider,
        api_key: settings.api_key,
        model: settings.model,
        base_url: settings.base_url,
        temperature: settings.temperature,
    })
}

#[tauri::command]
pub async fn ai_chat(
    pool: State<'_, DbPool>,
    messages: Vec<ChatMessage>,
) -> Result<String, AppError> {
    let config = load_ai_config(&pool).await?;
    client::chat_completion(&config, &messages).await
}

/// Agent 一轮: 前端传完整对话历史 + 工具定义, 返回模型的文本回复 / 工具调用请求.
/// agent 循环在前端跑 (UI 工具如改编辑器需要前端执行), 这里只做一次模型往返.
#[tauri::command]
pub async fn ai_agent_turn(
    pool: State<'_, DbPool>,
    messages: Vec<AgentMessage>,
    tools: Vec<ToolDef>,
) -> Result<AiTurn, AppError> {
    let config = load_ai_config(&pool).await?;
    client::chat_turn(&config, &messages, &tools).await
}

#[tauri::command]
pub async fn nl2query(
    pool: State<'_, DbPool>,
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection: String,
    natural_language: String,
) -> Result<String, AppError> {
    let config = load_ai_config(&pool).await?;
    let client = mgr.get_client(&connection_id).await?;

    // 先分析 Schema
    let schema_info = schema::analyze_schema(&client, &database, &collection, 100).await?;
    let schema_text = schema::schema_to_text(&schema_info);
    let system_prompt = prompt::nl2query_system_prompt(&schema_text);

    let messages = vec![
        ChatMessage { role: "system".to_string(), content: system_prompt },
        ChatMessage { role: "user".to_string(), content: natural_language },
    ];

    client::chat_completion(&config, &messages).await
}

#[tauri::command]
pub async fn analyze_collection_schema(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection: String,
    sample_size: Option<i64>,
) -> Result<SchemaInfo, AppError> {
    let client = mgr.get_client(&connection_id).await?;
    schema::analyze_schema(&client, &database, &collection, sample_size.unwrap_or(100)).await
}

#[tauri::command]
pub async fn suggest_indexes(
    pool: State<'_, DbPool>,
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    collection: String,
) -> Result<String, AppError> {
    let config = load_ai_config(&pool).await?;
    let client = mgr.get_client(&connection_id).await?;

    let schema_info = schema::analyze_schema(&client, &database, &collection, 100).await?;
    let schema_text = schema::schema_to_text(&schema_info);

    // 获取最近的慢查询
    let profiler_coll = client.database(&database).collection::<mongodb::bson::Document>("system.profile");
    let mut cursor = profiler_coll
        .find(mongodb::bson::doc! { "ns": format!("{database}.{collection}") })
        .sort(mongodb::bson::doc! { "ts": -1 })
        .limit(10)
        .await
        .unwrap_or_else(|_| {
            // 如果 profiler 没开，返回空游标
            futures::executor::block_on(async {
                profiler_coll.find(mongodb::bson::doc! { "_id": mongodb::bson::doc! { "$exists": false } }).await.unwrap()
            })
        });

    let mut slow_queries = Vec::new();
    use futures::StreamExt;
    while let Some(doc) = cursor.next().await {
        if let Ok(doc) = doc {
            slow_queries.push(format!("{doc}"));
        }
    }

    let slow_text = if slow_queries.is_empty() {
        "无慢查询数据".to_string()
    } else {
        slow_queries.join("\n")
    };

    let system = prompt::index_suggestion_prompt(&schema_text, &slow_text);
    let messages = vec![
        ChatMessage { role: "system".to_string(), content: system },
        ChatMessage { role: "user".to_string(), content: format!("请为 {collection} 集合提供索引优化建议") },
    ];

    client::chat_completion(&config, &messages).await
}

// ============================================================================
// AI 会话持久化 (P1)
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertConversationReq {
    pub id: String,
    pub title: String,
    pub connection_id: Option<String>,
    pub database: Option<String>,
    pub collection: Option<String>,
}

#[tauri::command]
pub async fn list_ai_conversations(
    pool: State<'_, DbPool>,
) -> Result<Vec<ConversationRow>, AppError> {
    ai_repo::list_conversations(&pool).await
}

#[tauri::command]
pub async fn upsert_ai_conversation(
    pool: State<'_, DbPool>,
    req: UpsertConversationReq,
) -> Result<(), AppError> {
    ai_repo::upsert_conversation(
        &pool,
        &req.id,
        &req.title,
        req.connection_id.as_deref(),
        req.database.as_deref(),
        req.collection.as_deref(),
        now_ms(),
    )
    .await
}

#[tauri::command]
pub async fn update_ai_conversation_title(
    pool: State<'_, DbPool>,
    id: String,
    title: String,
) -> Result<(), AppError> {
    ai_repo::update_conversation_title(&pool, &id, &title, now_ms()).await
}

#[tauri::command]
pub async fn touch_ai_conversation(pool: State<'_, DbPool>, id: String) -> Result<(), AppError> {
    ai_repo::touch_conversation(&pool, &id, now_ms()).await
}

#[tauri::command]
pub async fn delete_ai_conversation(pool: State<'_, DbPool>, id: String) -> Result<(), AppError> {
    ai_repo::delete_conversation(&pool, &id).await
}

#[tauri::command]
pub async fn clear_ai_conversation(pool: State<'_, DbPool>, id: String) -> Result<(), AppError> {
    ai_repo::clear_conversation_messages(&pool, &id).await
}

#[tauri::command]
pub async fn get_ai_messages(
    pool: State<'_, DbPool>,
    conversation_id: String,
) -> Result<Vec<MessageRow>, AppError> {
    ai_repo::get_messages(&pool, &conversation_id).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendMessageReq {
    pub conversation_id: String,
    pub position: i64,
    /// AgentMessage 序列化后的 JSON 字符串
    pub payload: String,
}

#[tauri::command]
pub async fn append_ai_message(
    pool: State<'_, DbPool>,
    req: AppendMessageReq,
) -> Result<i64, AppError> {
    let ts = now_ms();
    let id = ai_repo::append_message(&pool, &req.conversation_id, req.position, &req.payload, ts)
        .await?;
    // 顺手把会话的 updated_at 顶到最新
    ai_repo::touch_conversation(&pool, &req.conversation_id, ts).await?;
    Ok(id)
}

// ---- Facts (remember_fact 工具) ----

#[tauri::command]
pub async fn list_ai_facts(
    pool: State<'_, DbPool>,
    scopes: Vec<String>,
) -> Result<Vec<FactRow>, AppError> {
    ai_repo::list_facts(&pool, &scopes).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RememberFactReq {
    pub scope: String,
    pub key: String,
    pub value: String,
}

#[tauri::command]
pub async fn remember_ai_fact(
    pool: State<'_, DbPool>,
    req: RememberFactReq,
) -> Result<(), AppError> {
    if req.key.trim().is_empty() {
        return Err(AppError::InvalidInput("fact key 不能为空".into()));
    }
    ai_repo::upsert_fact(&pool, &req.scope, &req.key, &req.value, now_ms()).await
}

#[tauri::command]
pub async fn forget_ai_fact(
    pool: State<'_, DbPool>,
    scope: String,
    key: String,
) -> Result<bool, AppError> {
    ai_repo::delete_fact(&pool, &scope, &key).await
}

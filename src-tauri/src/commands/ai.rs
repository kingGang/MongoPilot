use serde::{Deserialize, Serialize};
use tauri::State;

use crate::ai::client::{self, AiConfig, ChatMessage};
use crate::ai::prompt;
use crate::ai::schema::{self, SchemaInfo};
use crate::connection::manager::ConnectionManager;
use crate::error::AppError;
use crate::storage::database::DbPool;
use crate::storage::settings_repo;

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

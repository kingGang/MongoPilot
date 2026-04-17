use serde::{Deserialize, Serialize};
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    pub provider: String,      // "claude" | "openai" | "custom"
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
    pub temperature: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,   // "user" | "assistant" | "system"
    pub content: String,
}

pub async fn chat_completion(
    config: &AiConfig,
    messages: &[ChatMessage],
) -> Result<String, AppError> {
    match config.provider.as_str() {
        "claude" => call_claude(config, messages).await,
        "openai" => call_openai(config, messages).await,
        "custom" => {
            let base = config.base_url.as_deref().ok_or_else(|| {
                AppError::InvalidInput("自定义提供商需要 base_url".into())
            })?;
            call_openai_compatible(config, messages, base).await
        }
        _ => Err(AppError::InvalidInput(format!("不支持的 AI 提供商: {}", config.provider))),
    }
}

async fn call_claude(config: &AiConfig, messages: &[ChatMessage]) -> Result<String, AppError> {
    let client = reqwest::Client::new();

    // Separate system message from conversation
    let system = messages.iter()
        .find(|m| m.role == "system")
        .map(|m| m.content.clone());

    let msgs: Vec<serde_json::Value> = messages.iter()
        .filter(|m| m.role != "system")
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();

    let mut body = serde_json::json!({
        "model": config.model,
        "max_tokens": 4096,
        "messages": msgs,
    });

    if let Some(sys) = system {
        body["system"] = serde_json::json!(sys);
    }
    if let Some(temp) = config.temperature {
        body["temperature"] = serde_json::json!(temp);
    }

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Connection(format!("Claude API 请求失败: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Connection(format!("Claude API 错误 {status}: {text}")));
    }

    let json: serde_json::Value = resp.json().await
        .map_err(|e| AppError::Connection(format!("解析响应失败: {e}")))?;

    json["content"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::Connection("无法提取 Claude 响应文本".into()))
}

async fn call_openai(config: &AiConfig, messages: &[ChatMessage]) -> Result<String, AppError> {
    let base = "https://api.openai.com/v1";
    call_openai_compatible(config, messages, base).await
}

async fn call_openai_compatible(
    config: &AiConfig,
    messages: &[ChatMessage],
    base_url: &str,
) -> Result<String, AppError> {
    let client = reqwest::Client::new();

    let msgs: Vec<serde_json::Value> = messages.iter()
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();

    let mut body = serde_json::json!({
        "model": config.model,
        "messages": msgs,
    });

    if let Some(temp) = config.temperature {
        body["temperature"] = serde_json::json!(temp);
    }

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Connection(format!("OpenAI API 请求失败: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Connection(format!("OpenAI API 错误 {status}: {text}")));
    }

    let json: serde_json::Value = resp.json().await
        .map_err(|e| AppError::Connection(format!("解析响应失败: {e}")))?;

    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::Connection("无法提取 OpenAI 响应文本".into()))
}

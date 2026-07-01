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

/// 共享的带超时 HTTP 客户端.
/// - 复用同一个 client (内部 Arc) → 连接池 + keep-alive, 少做 TLS 握手, 更稳更快;
///   每次新建 client 在某些网络下会偶发 "error sending request".
/// - 超时: 没有超时的话, 端点接受连接后不响应会让请求 (和前端转圈) 永久卡死.
fn http_client() -> reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .timeout(std::time::Duration::from_secs(90))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        })
        .clone()
}

/// 发送请求, 只对"连接没建起来"这种瞬时错误重试 (最多 2 次).
///
/// **关键: 超时 (is_timeout) 绝不重试。** 超时意味着服务端慢/卡住, 再赔进去一个 90s 超时窗口
/// 只会让前端转圈时间翻倍/三倍 —— 这正是"执行一步后一直转圈"的元凶。
/// HTTP 错误状态码 (4xx/5xx) 不会让 send() 返回 Err, 所以本来就不会被这里重试。
async fn send_with_retry(
    make_req: impl Fn() -> reqwest::RequestBuilder,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut last: Option<reqwest::Error> = None;
    for attempt in 0..2u32 {
        match make_req().send().await {
            Ok(resp) => return Ok(resp),
            Err(e) => {
                // 只重试: 连接/发送失败 且 不是超时
                let retryable = (e.is_connect() || e.is_request()) && !e.is_timeout();
                last = Some(e);
                if !retryable || attempt == 1 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
    Err(last.expect("loop runs at least once"))
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
    let client = http_client();

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

    let resp = send_with_retry(|| {
        client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
    })
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

/// 由用户填的 base_url 拼出 chat/completions 端点.
/// 容错: 用户可能填了带 /chat/completions 的完整地址, 或带/不带末尾斜杠.
fn openai_chat_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

// ============================================================================
// Tool-calling agent 层: 模型可以请求调用工具, 前端执行后把结果回传, 循环到最终答复.
// ============================================================================

/// 工具定义 (传给模型, 让它知道有哪些能力可调用)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema 描述参数
    pub input_schema: serde_json::Value,
}

/// 模型发起的一次工具调用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// agent 对话里的一条消息. 前端构建完整历史, 后端按 provider 翻译.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    /// "system" | "user" | "assistant" | "tool"
    pub role: String,
    /// 文本内容
    #[serde(default)]
    pub content: Option<String>,
    /// assistant 发起的工具调用
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// role=="tool" 时, 对应的 tool_call id
    #[serde(default)]
    pub tool_call_id: Option<String>,
    /// role=="system" 时: 该段是否可被 Anthropic prompt caching 缓存 (稳定段填 true,
    /// 每轮都在变的实时状态段填 false / 不填). 对非 Anthropic provider 无效.
    #[serde(default)]
    pub cacheable: Option<bool>,
}

/// 一轮模型返回: 文本回复 + (可选)一批工具调用. tool_calls 非空表示 agent 循环要继续.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTurn {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
}

/// 跑一轮带工具的对话. tools 为空时等价于普通聊天.
///
/// 某些 provider / 模型不支持 function calling —— 带工具会直接报错 (报错里通常含
/// "tool" / "function" 字样). 这种情况自动降级: 不带工具重试一次, 至少能正常对话.
pub async fn chat_turn(
    config: &AiConfig,
    messages: &[AgentMessage],
    tools: &[ToolDef],
) -> Result<AiTurn, AppError> {
    let result = chat_turn_dispatch(config, messages, tools).await;
    if !tools.is_empty() {
        if let Err(e) = &result {
            let m = e.to_string().to_lowercase();
            if m.contains("tool") || m.contains("function") {
                return chat_turn_dispatch(config, messages, &[]).await;
            }
        }
    }
    result
}

async fn chat_turn_dispatch(
    config: &AiConfig,
    messages: &[AgentMessage],
    tools: &[ToolDef],
) -> Result<AiTurn, AppError> {
    match config.provider.as_str() {
        "claude" => claude_turn(config, messages, tools).await,
        "openai" => openai_turn(config, messages, tools, "https://api.openai.com/v1").await,
        "custom" => {
            let base = config
                .base_url
                .as_deref()
                .ok_or_else(|| AppError::InvalidInput("自定义提供商需要 base_url".into()))?;
            openai_turn(config, messages, tools, base).await
        }
        _ => Err(AppError::InvalidInput(format!(
            "不支持的 AI 提供商: {}",
            config.provider
        ))),
    }
}

async fn claude_turn(
    config: &AiConfig,
    messages: &[AgentMessage],
    tools: &[ToolDef],
) -> Result<AiTurn, AppError> {
    let client = http_client();

    // system 段可能是多条 (分层 prompt: 稳定的工具指南 / 半稳定的 Rules / 实时状态).
    // 分别转成 text block, cacheable=true 的段加 cache_control 让 Anthropic 缓存,
    // 前两段 (工具指南 + Rules) 稳定不变 -> 大部分轮次都命中 cache -> 大幅省 token.
    let system_blocks: Vec<serde_json::Value> = messages
        .iter()
        .filter(|m| m.role == "system")
        .filter_map(|m| {
            let text = m.content.as_deref()?.to_string();
            if text.is_empty() {
                return None;
            }
            let mut block = serde_json::json!({ "type": "text", "text": text });
            if m.cacheable.unwrap_or(false) {
                block["cache_control"] = serde_json::json!({ "type": "ephemeral" });
            }
            Some(block)
        })
        .collect();
    let any_cached = system_blocks
        .iter()
        .any(|b| b.get("cache_control").is_some());

    // Claude 要求: assistant 的 tool_use 之后必须跟一个 user 消息, 里面是 tool_result 块.
    // 连续的 tool 消息要合并进同一个 user 消息.
    let mut claude_msgs: Vec<serde_json::Value> = Vec::new();
    let mut pending_results: Vec<serde_json::Value> = Vec::new();

    for m in messages {
        match m.role.as_str() {
            "system" => {}
            "tool" => {
                pending_results.push(serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                    "content": m.content.clone().unwrap_or_default(),
                }));
            }
            "user" => {
                if !pending_results.is_empty() {
                    claude_msgs.push(serde_json::json!({
                        "role": "user", "content": std::mem::take(&mut pending_results),
                    }));
                }
                claude_msgs.push(serde_json::json!({
                    "role": "user", "content": m.content.clone().unwrap_or_default(),
                }));
            }
            "assistant" => {
                if !pending_results.is_empty() {
                    claude_msgs.push(serde_json::json!({
                        "role": "user", "content": std::mem::take(&mut pending_results),
                    }));
                }
                if let Some(calls) = &m.tool_calls {
                    let mut blocks: Vec<serde_json::Value> = Vec::new();
                    if let Some(t) = &m.content {
                        if !t.is_empty() {
                            blocks.push(serde_json::json!({"type": "text", "text": t}));
                        }
                    }
                    for c in calls {
                        blocks.push(serde_json::json!({
                            "type": "tool_use", "id": c.id, "name": c.name, "input": c.input,
                        }));
                    }
                    claude_msgs.push(serde_json::json!({"role": "assistant", "content": blocks}));
                } else {
                    claude_msgs.push(serde_json::json!({
                        "role": "assistant", "content": m.content.clone().unwrap_or_default(),
                    }));
                }
            }
            _ => {}
        }
    }
    if !pending_results.is_empty() {
        claude_msgs.push(serde_json::json!({"role": "user", "content": pending_results}));
    }

    let mut body = serde_json::json!({
        "model": config.model,
        "max_tokens": 4096,
        "messages": claude_msgs,
    });
    if !system_blocks.is_empty() {
        body["system"] = serde_json::Value::Array(system_blocks);
    }
    if let Some(temp) = config.temperature {
        body["temperature"] = serde_json::json!(temp);
    }
    if !tools.is_empty() {
        body["tools"] = serde_json::json!(tools
            .iter()
            .map(|t| serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            }))
            .collect::<Vec<_>>());
    }

    let resp = send_with_retry(|| {
        let mut req = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json");
        // 只在真正用到 cache_control 时带 beta header, 避免不必要的请求头噪声
        if any_cached {
            req = req.header("anthropic-beta", "prompt-caching-2024-07-31");
        }
        req.json(&body)
    })
    .await
    .map_err(|e| AppError::Connection(format!("Claude API 请求失败: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Connection(format!("Claude API 错误 {status}: {text}")));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Connection(format!("解析响应失败: {e}")))?;

    let mut text = String::new();
    let mut tool_calls = Vec::new();
    if let Some(blocks) = json["content"].as_array() {
        for b in blocks {
            match b["type"].as_str() {
                Some("text") => text.push_str(b["text"].as_str().unwrap_or_default()),
                Some("tool_use") => tool_calls.push(ToolCall {
                    id: b["id"].as_str().unwrap_or_default().to_string(),
                    name: b["name"].as_str().unwrap_or_default().to_string(),
                    input: b["input"].clone(),
                }),
                _ => {}
            }
        }
    }

    Ok(AiTurn {
        text: if text.is_empty() { None } else { Some(text) },
        tool_calls,
    })
}

async fn openai_turn(
    config: &AiConfig,
    messages: &[AgentMessage],
    tools: &[ToolDef],
    base_url: &str,
) -> Result<AiTurn, AppError> {
    let client = http_client();

    let mut oai_msgs: Vec<serde_json::Value> = Vec::new();
    for m in messages {
        match m.role.as_str() {
            "system" => oai_msgs.push(serde_json::json!({
                "role": "system", "content": m.content.clone().unwrap_or_default(),
            })),
            "user" => oai_msgs.push(serde_json::json!({
                "role": "user", "content": m.content.clone().unwrap_or_default(),
            })),
            "tool" => oai_msgs.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": m.tool_call_id.clone().unwrap_or_default(),
                "content": m.content.clone().unwrap_or_default(),
            })),
            "assistant" => {
                if let Some(calls) = &m.tool_calls {
                    oai_msgs.push(serde_json::json!({
                        "role": "assistant",
                        "content": m.content.clone(),
                        "tool_calls": calls.iter().map(|c| serde_json::json!({
                            "id": c.id,
                            "type": "function",
                            "function": {
                                "name": c.name,
                                "arguments": serde_json::to_string(&c.input)
                                    .unwrap_or_else(|_| "{}".to_string()),
                            },
                        })).collect::<Vec<_>>(),
                    }));
                } else {
                    oai_msgs.push(serde_json::json!({
                        "role": "assistant", "content": m.content.clone().unwrap_or_default(),
                    }));
                }
            }
            _ => {}
        }
    }

    let mut body = serde_json::json!({
        "model": config.model,
        "messages": oai_msgs,
    });
    if let Some(temp) = config.temperature {
        body["temperature"] = serde_json::json!(temp);
    }
    if !tools.is_empty() {
        body["tools"] = serde_json::json!(tools
            .iter()
            .map(|t| serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                },
            }))
            .collect::<Vec<_>>());
    }

    let url = openai_chat_url(base_url);
    let resp = send_with_retry(|| {
        client
            .post(&url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("content-type", "application/json")
            .json(&body)
    })
    .await
    .map_err(|e| AppError::Connection(format!("OpenAI API 请求失败 ({url}): {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Connection(format!(
            "OpenAI API 错误 {status} ({url}): {text}"
        )));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Connection(format!("解析响应失败: {e}")))?;

    let msg = &json["choices"][0]["message"];
    let text = msg["content"]
        .as_str()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    let mut tool_calls = Vec::new();
    if let Some(calls) = msg["tool_calls"].as_array() {
        for c in calls {
            let args_str = c["function"]["arguments"].as_str().unwrap_or("{}");
            let input = serde_json::from_str(args_str).unwrap_or(serde_json::json!({}));
            tool_calls.push(ToolCall {
                id: c["id"].as_str().unwrap_or_default().to_string(),
                name: c["function"]["name"].as_str().unwrap_or_default().to_string(),
                input,
            });
        }
    }

    Ok(AiTurn { text, tool_calls })
}

async fn call_openai_compatible(
    config: &AiConfig,
    messages: &[ChatMessage],
    base_url: &str,
) -> Result<String, AppError> {
    let client = http_client();

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

    let url = openai_chat_url(base_url);

    let resp = send_with_retry(|| {
        client
            .post(&url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("content-type", "application/json")
            .json(&body)
    })
    .await
    .map_err(|e| AppError::Connection(format!("OpenAI API 请求失败 ({url}): {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Connection(format!(
            "OpenAI API 错误 {status} ({url}): {text}"
        )));
    }

    let json: serde_json::Value = resp.json().await
        .map_err(|e| AppError::Connection(format!("解析响应失败: {e}")))?;

    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::Connection("无法提取 OpenAI 响应文本".into()))
}

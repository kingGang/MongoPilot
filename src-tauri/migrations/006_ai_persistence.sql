-- AI 会话 (对话) 元数据: 一条 = 一个会话
CREATE TABLE IF NOT EXISTS ai_conversations (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    connection_id TEXT,
    database_name TEXT,
    collection_name TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ai_conversations_updated_at
    ON ai_conversations(updated_at DESC);

-- AI 消息: 每条消息一整条记录, payload 存 AgentMessage 序列化 JSON
--   (含 role/content/toolCalls/toolCallId/durationMs 全部字段, 前端反序列化直接就能用)
CREATE TABLE IF NOT EXISTS ai_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    payload TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(conversation_id) REFERENCES ai_conversations(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_ai_messages_conv_pos
    ON ai_messages(conversation_id, position);

-- AI facts: agent 主动记录的一次性事实 (通过 remember_fact 工具)
--   scope = "global" | "conn:<id>" | "conn:<id>:db:<db>" | "conn:<id>:db:<db>:coll:<coll>"
--   key   = 短 slug, 用 (scope, key) 唯一约束保证同一作用域内同名事实被覆盖而不是复读
CREATE TABLE IF NOT EXISTS ai_facts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scope TEXT NOT NULL,
    fact_key TEXT NOT NULL,
    fact_value TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(scope, fact_key)
);
CREATE INDEX IF NOT EXISTS idx_ai_facts_scope ON ai_facts(scope);

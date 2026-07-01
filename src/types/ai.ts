export interface AiSettings {
  provider: "claude" | "openai" | "custom";
  apiKey: string;
  model: string;
  baseUrl?: string;
  temperature?: number;
}

export interface ChatMessage {
  role: "user" | "assistant" | "system";
  content: string;
}

// ---- Tool-calling agent ----

/** 模型发起的一次工具调用 */
export interface AgentToolCall {
  id: string;
  name: string;
  input: Record<string, unknown>;
}

/** agent 对话里的一条消息 (与后端 AgentMessage 对应) */
export interface AgentMessage {
  role: "system" | "user" | "assistant" | "tool";
  content?: string;
  /** assistant 发起的工具调用 */
  toolCalls?: AgentToolCall[];
  /** role==="tool" 时对应的 tool_call id */
  toolCallId?: string;
  /** UI 用: 这一步耗时毫秒 (assistant=模型往返, tool=工具执行)。后端会忽略此字段。 */
  durationMs?: number;
  /** role==="system" 时: 是否让 Anthropic prompt caching 缓存这段. 稳定段 true. */
  cacheable?: boolean;
}

/** 后端持久化返回的会话元信息 */
export interface StoredConversation {
  id: string;
  title: string;
  connectionId?: string;
  database?: string;
  collection?: string;
  createdAt: number;
  updatedAt: number;
}

/** 后端持久化返回的一条消息 (payload 是 AgentMessage 的 JSON 字符串) */
export interface StoredMessage {
  id: number;
  position: number;
  payload: string;
  createdAt: number;
}

/** AI facts (agent 主动记的一次性事实) */
export interface StoredFact {
  id: number;
  scope: string;
  key: string;
  value: string;
  createdAt: number;
  updatedAt: number;
}

/** 传给模型的工具定义 */
export interface ToolDef {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
}

/** 一轮模型返回: 文本回复 + (可选)工具调用 */
export interface AiTurn {
  text?: string;
  toolCalls: AgentToolCall[];
}

export interface SchemaInfo {
  collection: string;
  sampleCount: number;
  fields: FieldInfo[];
}

export interface FieldInfo {
  name: string;
  fieldTypes: { bsonType: string; count: number }[];
  occurrencePercent: number;
  sampleValues: string[];
}

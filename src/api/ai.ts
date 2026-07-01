import { invoke } from "@tauri-apps/api/core";
import type {
  AiSettings,
  ChatMessage,
  SchemaInfo,
  AgentMessage,
  ToolDef,
  AiTurn,
  StoredConversation,
  StoredMessage,
  StoredFact,
} from "@/types/ai";

export async function getAiSettings(): Promise<AiSettings | null> {
  return invoke<AiSettings | null>("get_ai_settings");
}

export async function saveAiSettings(settings: AiSettings): Promise<void> {
  return invoke("save_ai_settings", { settings });
}

/** AI 规范 (Rules): scope = "global" 或 "conn:<connectionId>" */
export async function getAiRules(scope: string): Promise<string> {
  return invoke<string>("get_ai_rules", { scope });
}

export async function saveAiRules(scope: string, content: string): Promise<void> {
  return invoke("save_ai_rules", { scope, content });
}

// ---- 会话持久化 ----

export async function listAiConversations(): Promise<StoredConversation[]> {
  return invoke<StoredConversation[]>("list_ai_conversations");
}

export async function upsertAiConversation(req: {
  id: string;
  title: string;
  connectionId?: string;
  database?: string;
  collection?: string;
}): Promise<void> {
  return invoke("upsert_ai_conversation", { req });
}

export async function updateAiConversationTitle(id: string, title: string): Promise<void> {
  return invoke("update_ai_conversation_title", { id, title });
}

export async function touchAiConversation(id: string): Promise<void> {
  return invoke("touch_ai_conversation", { id });
}

export async function deleteAiConversation(id: string): Promise<void> {
  return invoke("delete_ai_conversation", { id });
}

export async function clearAiConversation(id: string): Promise<void> {
  return invoke("clear_ai_conversation", { id });
}

export async function getAiMessages(conversationId: string): Promise<StoredMessage[]> {
  return invoke<StoredMessage[]>("get_ai_messages", { conversationId });
}

export async function appendAiMessage(req: {
  conversationId: string;
  position: number;
  payload: string;
}): Promise<number> {
  return invoke<number>("append_ai_message", { req });
}

// ---- Facts ----

export async function listAiFacts(scopes: string[]): Promise<StoredFact[]> {
  return invoke<StoredFact[]>("list_ai_facts", { scopes });
}

export async function rememberAiFact(req: {
  scope: string;
  key: string;
  value: string;
}): Promise<void> {
  return invoke("remember_ai_fact", { req });
}

export async function forgetAiFact(scope: string, key: string): Promise<boolean> {
  return invoke<boolean>("forget_ai_fact", { scope, key });
}

export async function aiChat(messages: ChatMessage[]): Promise<string> {
  return invoke<string>("ai_chat", { messages });
}

/** Agent 一轮: 传完整对话历史 + 工具定义, 返回模型文本回复 / 工具调用请求 */
export async function aiAgentTurn(messages: AgentMessage[], tools: ToolDef[]): Promise<AiTurn> {
  return invoke<AiTurn>("ai_agent_turn", { messages, tools });
}

export async function nl2query(
  connectionId: string,
  database: string,
  collection: string,
  naturalLanguage: string,
): Promise<string> {
  return invoke<string>("nl2query", { connectionId, database, collection, naturalLanguage });
}

export async function analyzeSchema(
  connectionId: string,
  database: string,
  collection: string,
  sampleSize?: number,
): Promise<SchemaInfo> {
  return invoke<SchemaInfo>("analyze_collection_schema", {
    connectionId,
    database,
    collection,
    sampleSize,
  });
}

export async function suggestIndexes(
  connectionId: string,
  database: string,
  collection: string,
): Promise<string> {
  return invoke<string>("suggest_indexes", { connectionId, database, collection });
}

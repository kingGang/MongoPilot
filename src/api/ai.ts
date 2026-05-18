import { invoke } from "@tauri-apps/api/core";
import type {
  AiSettings,
  ChatMessage,
  SchemaInfo,
  AgentMessage,
  ToolDef,
  AiTurn,
} from "@/types/ai";

export async function getAiSettings(): Promise<AiSettings | null> {
  return invoke<AiSettings | null>("get_ai_settings");
}

export async function saveAiSettings(settings: AiSettings): Promise<void> {
  return invoke("save_ai_settings", { settings });
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

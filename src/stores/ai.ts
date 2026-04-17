import { defineStore } from "pinia";
import { ref, computed } from "vue";
import type { AiSettings, ChatMessage } from "@/types/ai";
import * as aiApi from "@/api/ai";

export interface AiConversation {
  id: string;
  title: string;
  messages: ChatMessage[];
  /** 关联的连接/数据库/集合上下文 */
  connectionId?: string;
  database?: string;
  collection?: string;
  createdAt: number;
}

export const useAiStore = defineStore("ai", () => {
  // ---- Settings ----
  const settings = ref<AiSettings | null>(null);
  const settingsLoaded = ref(false);

  async function loadSettings() {
    if (settingsLoaded.value) return;
    settings.value = await aiApi.getAiSettings();
    settingsLoaded.value = true;
  }

  async function updateSettings(s: AiSettings) {
    await aiApi.saveAiSettings(s);
    settings.value = s;
  }

  const isConfigured = computed(() => !!settings.value?.apiKey);

  // ---- Conversations ----
  const conversations = ref<AiConversation[]>([]);
  const activeConversationId = ref<string | null>(null);

  const activeConversation = computed(
    () => conversations.value.find((c) => c.id === activeConversationId.value) ?? null,
  );

  function createConversation(
    connectionId?: string,
    database?: string,
    collection?: string,
  ): string {
    const id = crypto.randomUUID();
    const title = collection ? `${database}.${collection}` : database || "New Chat";
    conversations.value.push({
      id,
      title,
      messages: [],
      connectionId,
      database,
      collection,
      createdAt: Date.now(),
    });
    activeConversationId.value = id;
    return id;
  }

  /** 确保有活跃会话，如果没有则创建 */
  function ensureConversation(
    connectionId?: string,
    database?: string,
    collection?: string,
  ): AiConversation {
    // 复用同上下文的会话
    let conv = conversations.value.find(
      (c) =>
        c.connectionId === connectionId && c.database === database && c.collection === collection,
    );
    if (!conv) {
      const id = createConversation(connectionId, database, collection);
      conv = conversations.value.find((c) => c.id === id)!;
    }
    activeConversationId.value = conv.id;
    return conv;
  }

  function clearConversation(id?: string) {
    const convId = id ?? activeConversationId.value;
    if (!convId) return;
    const conv = conversations.value.find((c) => c.id === convId);
    if (conv) conv.messages = [];
  }

  function deleteConversation(id: string) {
    const idx = conversations.value.findIndex((c) => c.id === id);
    if (idx === -1) return;
    conversations.value.splice(idx, 1);
    if (activeConversationId.value === id) {
      activeConversationId.value = conversations.value[0]?.id ?? null;
    }
  }

  // ---- Chat ----
  const loading = ref(false);

  async function sendMessage(
    text: string,
    connectionId?: string,
    database?: string,
    collection?: string,
  ): Promise<string> {
    const conv = ensureConversation(connectionId, database, collection);
    conv.messages.push({ role: "user", content: text });
    loading.value = true;

    try {
      let reply: string;

      // 判断是否为查询请求 → 使用 NL2Query
      const isQuery = connectionId && database && collection && isQueryRequest(text);
      if (isQuery) {
        reply = await aiApi.nl2query(connectionId!, database!, collection!, text);
      } else {
        const chatMessages: ChatMessage[] = [
          { role: "system", content: "你是 MongoPilot 的 AI 助手，帮助用户操作 MongoDB。" },
          ...conv.messages,
        ];
        reply = await aiApi.aiChat(chatMessages);
      }

      conv.messages.push({ role: "assistant", content: reply });
      // 更新标题（取第一条用户消息的前 20 字）
      if (conv.messages.filter((m) => m.role === "user").length === 1) {
        conv.title = text.slice(0, 20) + (text.length > 20 ? "..." : "");
      }
      return reply;
    } catch (e) {
      const errMsg = `错误: ${e}`;
      conv.messages.push({ role: "assistant", content: errMsg });
      throw e;
    } finally {
      loading.value = false;
    }
  }

  function isQueryRequest(text: string): boolean {
    const keywords = [
      "查询",
      "查找",
      "搜索",
      "统计",
      "计算",
      "find",
      "query",
      "count",
      "aggregate",
      "筛选",
      "过滤",
    ];
    return keywords.some((k) => text.toLowerCase().includes(k));
  }

  return {
    // settings
    settings,
    settingsLoaded,
    isConfigured,
    loadSettings,
    updateSettings,
    // conversations
    conversations,
    activeConversationId,
    activeConversation,
    createConversation,
    ensureConversation,
    clearConversation,
    deleteConversation,
    // chat
    loading,
    sendMessage,
  };
});

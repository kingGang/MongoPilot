import { defineStore, acceptHMRUpdate } from "pinia";
import { ref, computed } from "vue";
import type { AiSettings, AgentMessage } from "@/types/ai";
import * as aiApi from "@/api/ai";
import { AGENT_TOOLS, executeTool } from "@/ai/tools";
import { useEditorStore } from "@/stores/editor";
import { useConnectionStore } from "@/stores/connection";

export interface AiConversation {
  id: string;
  title: string;
  messages: AgentMessage[];
  /** 关联的连接/数据库/集合上下文 */
  connectionId?: string;
  database?: string;
  collection?: string;
  createdAt: number;
}

/** 每轮 runAgent 迭代都重新构建的 system prompt —— 带**实时**的应用状态 */
function buildSystemPrompt(): string {
  const editorStore = useEditorStore();
  const connStore = useConnectionStore();
  const tab = editorStore.activeTab;
  const activeConns = connStore.connections.filter((c) => connStore.isActive(c.id));
  const hasSelection = tab ? !!editorStore.getSelection(tab.id) : false;

  return [
    "你是 MongoPilot 内置的 AI 助手 —— 一个能操作整个应用的 agent。MongoPilot 是 MongoDB 桌面客户端。",
    "",
    "你能调用的工具 (按类别):",
    "- 交互: ask_user (指令不清时反问用户)",
    "- 连接: list_connections / open_connection",
    "- 浏览: list_databases / list_collections / get_schema",
    "- 查询: write_query (把查询写进编辑器交给用户运行)",
    "- 编辑器: get_editor_content / get_editor_selection / propose_editor_edit /",
    "  list_query_tabs / open_query_tab / switch_query_tab / set_active_context",
    "- 脚本库: list_scripts / get_script",
    "",
    "工作方式:",
    "- **确定库和集合**: 当前标签页已经绑定了库/集合 (见下方「当前实时状态」), 用户请求又是针对它的 ——",
    "  就**直接用**, 不要再 list_databases / list_collections / get_schema 探查一遍。",
    "  如果不清楚该用哪个库/集合/字段 —— **直接 ask_user 问用户**, 给几个候选让他选。",
    "  问用户比你一通分析又快又准, 别为了「确认」而反复探查。",
    "- **指令不清晰、有歧义、有多个合理选择时, 用 ask_user 反问用户, 不要自己随便猜。**",
    "  有多个连接不知道连哪个、要做写操作或危险操作前 —— 都先 ask_user。",
    "- **你不能直接执行任何查询或写操作。** 要让用户跑一条查询时, 用 write_query 把查询",
    "  **直接写进编辑器的代码框** —— 然后由用户自己点 Run 执行。不要把查询语句只丢在聊天里。",
    "- get_schema 只在「用户明确要分析结构」或「你确实拿不准字段、又问不出来」时才用 ——",
    "  不要每次写查询前都习惯性分析一遍, 那样又慢又费 token。",
    "  换库先 set_active_context, 库所在连接没打开先 open_connection。",
    "- 要修改/重构编辑器里**已有的脚本**时, 用 propose_editor_edit (以 diff 形式让用户确认 Accept)。",
    "  content 要给**完整**的新内容, 不是 diff 片段。",
    '- 用户说"分析选中的代码/这段"时, 用 get_editor_selection 拿到选区再分析。',
    "- 写脚本前可以 list_scripts 看脚本库里有什么参考, get_script 读具体内容。",
    "- 编辑器内容是 mongosh 脚本语法 (db.coll.find(...), 支持 ObjectId()/ISODate() 等)。",
    "",
    "当前实时状态:",
    `- 已连接的服务器: ${activeConns.length > 0 ? activeConns.map((c) => c.name).join(", ") : "(无)"}`,
    `- 当前标签页: ${
      tab
        ? `"${tab.title}" (库:${tab.database || "无"}, 集合:${tab.collection || "无"})`
        : "(没有打开的标签页)"
    }`,
    `- 用户当前${hasSelection ? "**有**" : "没有"}在编辑器里选中代码`,
    "",
    "回复用中文, 简洁。完成任务后简短说明你做了什么。",
  ].join("\n");
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
    const title = collection ? `${database}.${collection}` : database || "新对话";
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

  /**
   * 返回当前活跃会话, 没有就新建一个空会话.
   * 注意: agent 现在能跨连接/库操作, 所以**不再**按上下文给会话分组 ——
   * 否则 agent 一调 set_active_context / open_connection, 会话就被切换、界面看着像"消息清空了"。
   */
  function ensureActiveConversation(): AiConversation {
    const existing = conversations.value.find((c) => c.id === activeConversationId.value);
    if (existing) return existing;
    const id = createConversation();
    return conversations.value.find((c) => c.id === id)!;
  }

  function clearConversation(id?: string) {
    // 有挂起的提问 → 先取消掉, 否则 agent 循环会一直卡着
    if (pendingQuestion.value) answerQuestion("(用户清空了对话, 取消本次询问)");
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

  // ---- 向用户提问 (ask_user 工具) ----
  /** agent 正在等用户回答的问题; null 表示没有挂起的问题 */
  const pendingQuestion = ref<{ question: string; options: string[] } | null>(null);
  let questionResolver: ((answer: string) => void) | null = null;

  /** agent 调 ask_user 时调用: 展示问题, 返回 promise, 用户选择/输入后 resolve */
  function askUser(question: string, options: string[]): Promise<string> {
    if (questionResolver) questionResolver("(被新的提问取代)");
    pendingQuestion.value = { question, options };
    return new Promise<string>((resolve) => {
      questionResolver = resolve;
    });
  }

  /** 用户点了选项 / 输入了自定义答案 → 把 agent 循环从等待中唤醒 */
  function answerQuestion(answer: string) {
    if (!questionResolver) return;
    const resolve = questionResolver;
    questionResolver = null;
    pendingQuestion.value = null;
    resolve(answer);
  }

  // ---- Agent ----
  const loading = ref(false);
  /** agent 循环最多跑几轮 (一轮 = 一次模型往返 + 工具执行); 纯粹是防失控/防烧 token 的安全上限 */
  const MAX_AGENT_STEPS = 16;
  /** 用户点了"停止" */
  const abortRequested = ref(false);
  /** 解开它能让正在 await 模型请求的 runAgent 立刻醒过来 (不必干等请求超时) */
  let releaseAbortBarrier: (() => void) | null = null;

  /** 停止正在跑的 agent 循环 —— 立刻停转圈, 在途的后端请求会自行结束 */
  function stopAgent() {
    if (!loading.value) return;
    abortRequested.value = true;
    // 若正卡在 ask_user 等用户回答 → 先放它过去, 循环才能走到检查点停下
    if (pendingQuestion.value) answerQuestion("(用户停止了对话)");
    // 若正卡在等模型请求 → 解开 barrier, runAgent 立刻从 race 中醒来
    releaseAbortBarrier?.();
  }

  /**
   * 跑一轮完整 agent 对话: push 用户消息 → 循环 (模型 → 执行工具 → 回传) 直到模型给出最终文本。
   * 上下文 (连接/库/当前标签页) 由工具实时从 store 读取, 所以 agent 在一轮里切了库/标签页,
   * 后续工具能看到最新状态。system prompt 每轮迭代都重建, 反映实时状态。
   */
  async function runAgent(text: string): Promise<void> {
    // 用当前活跃会话 (没有就新建). 整个 agent 循环里这个引用都不变,
    // 即使 agent 中途切了连接/库, 会话也不会被换走。
    const conv = ensureActiveConversation();
    conv.messages.push({ role: "user", content: text });
    if (conv.messages.filter((m) => m.role === "user").length === 1) {
      conv.title = text.slice(0, 20) + (text.length > 20 ? "..." : "");
    }
    loading.value = true;
    abortRequested.value = false;

    try {
      for (let step = 0; step < MAX_AGENT_STEPS; step++) {
        if (abortRequested.value) break;
        const systemMsg: AgentMessage = { role: "system", content: buildSystemPrompt() };

        // 模型请求 与 "用户点停止" 赛跑 —— 点了停止立刻停转圈, 不必干等请求超时
        const t0 = performance.now();
        const turnPromise = aiApi.aiAgentTurn([systemMsg, ...conv.messages], AGENT_TOOLS);
        turnPromise.catch(() => {}); // 被 race 丢弃时不报 unhandled rejection
        const abortBarrier = new Promise<"abort">((resolve) => {
          releaseAbortBarrier = () => resolve("abort");
        });
        const raced = await Promise.race([turnPromise, abortBarrier]);
        releaseAbortBarrier = null;
        if (raced === "abort") break;
        const turn = raced;
        const turnMs = Math.round(performance.now() - t0);

        conv.messages.push({
          role: "assistant",
          content: turn.text ?? undefined,
          toolCalls: turn.toolCalls.length > 0 ? turn.toolCalls : undefined,
          durationMs: turnMs,
        });

        // 没有工具调用 → 最终答复, 结束
        if (turn.toolCalls.length === 0) return;

        // 执行每个工具调用, 结果回传
        for (const call of turn.toolCalls) {
          if (abortRequested.value) break;
          const tt0 = performance.now();
          let result: string;
          if (call.name === "ask_user") {
            // ask_user 不走 executeTool —— 它要暂停循环、等用户在 UI 上回答
            const q = String(call.input.question ?? "");
            const rawOpts = call.input.options;
            const opts = Array.isArray(rawOpts) ? rawOpts.map((o) => String(o)) : [];
            result = `用户回答: ${await askUser(q, opts)}`;
          } else {
            try {
              result = await executeTool(call.name, call.input);
            } catch (e) {
              result = `工具执行异常: ${e}`;
            }
          }
          conv.messages.push({
            role: "tool",
            toolCallId: call.id,
            content: result,
            durationMs: Math.round(performance.now() - tt0),
          });
        }
      }

      // 循环到这里: 要么用户停止了, 要么到达步数上限
      conv.messages.push({
        role: "assistant",
        content: abortRequested.value
          ? "(已按你的要求停止。可以再追问让我继续。)"
          : `(到达 ${MAX_AGENT_STEPS} 步安全上限, 自动停下了 —— 这是防止 agent 失控空转的保护, ` +
            `不是任务失败。可以直接追问"继续"让我接着做。)`,
      });
    } catch (e) {
      conv.messages.push({ role: "assistant", content: `错误: ${e}` });
      throw e;
    } finally {
      loading.value = false;
      abortRequested.value = false;
      releaseAbortBarrier = null;
    }
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
    ensureActiveConversation,
    clearConversation,
    deleteConversation,
    // agent
    loading,
    runAgent,
    stopAgent,
    // ask_user
    pendingQuestion,
    answerQuestion,
  };
});

// Vite HMR: 让 ai store (及它依赖的 ai/tools.ts) 改动能干净热替换,
// 不再出现"代码改了但应用还跑旧逻辑"。
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useAiStore, import.meta.hot));
}

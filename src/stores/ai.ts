import { defineStore, acceptHMRUpdate } from "pinia";
import { ref, computed } from "vue";
import type { AiSettings, AgentMessage, StoredFact } from "@/types/ai";
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
  updatedAt: number;
  /** 已从 DB 加载过消息 (延迟加载, 打开时才拉) */
  messagesHydrated?: boolean;
  /** 下一条消息该占的 position (追加时用) */
  nextPosition: number;
  /** 后端已经存在这条会话记录 —— 避免每条消息前都 upsert */
  persistedMeta?: boolean;
}

/** 从所有 tabs 里挑一条"最近的错误结果", 用来喂给 AI 做失败反馈闭环.
 *  仅取最近 5 分钟内产生的, 避免把陈年错误当参考。*/
function findRecentEditorError(): { query: string; error: string; tabTitle: string } | null {
  const editorStore = useEditorStore();
  const cutoff = Date.now() - 5 * 60 * 1000;
  let best: {
    rt: (typeof editorStore.tabs)[number]["resultTabs"][number];
    tabTitle: string;
  } | null = null;
  for (const t of editorStore.tabs) {
    for (const rt of t.resultTabs) {
      if (!rt.error || rt.aborted) continue;
      if (rt.createdAt < cutoff) continue;
      if (!best || rt.createdAt > best.rt.createdAt) best = { rt, tabTitle: t.title };
    }
  }
  if (!best) return null;
  return {
    query: (best.rt.queryText || "").slice(0, 400),
    error: (best.rt.error || "").slice(0, 400),
    tabTitle: best.tabTitle,
  };
}

/** 分层 system prompt.
 *  返回三段独立文本, 每段在后端会变成一个 system block, 前两段带 cache_control 让 Anthropic 缓存.
 *
 *  - stable:   工具指南 + 工作方式 —— 上百上千轮都不变, 命中缓存收益最高
 *  - rules:    用户手写 Rules + 相关 facts —— 修改频率低 (改一次能命中一段时间的对话)
 *  - volatile: 实时状态 (激活连接/tab/选区) + 最近错误 —— 每轮都在变, 不能缓存 */
function buildSystemPromptLayers(input: {
  globalRules: string;
  connRules: string;
  facts: StoredFact[];
}): { stable: string; rules: string; volatile: string } {
  const editorStore = useEditorStore();
  const connStore = useConnectionStore();
  const tab = editorStore.activeTab;
  const activeConns = connStore.connections.filter((c) => connStore.isActive(c.id));
  const hasSelection = tab ? !!editorStore.getSelection(tab.id) : false;
  const recentErr = findRecentEditorError();

  // ---- Layer 1: stable ----
  const stable = [
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
    "- 记忆: remember_fact (存一次性事实, 之后所有对话都能看到) / forget_fact",
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
    "- write_query 内部会做**语法校验**: 语法错会被打回, 你收到错误信息后**必须自己改正重发**, 不要照原样再发一次。",
    "- write_query 成功写入后, 返回文本里会附带**当前集合的字段结构** (如果已绑定 collection)。",
    "  下一轮基于用户反馈修改时, 参考这个字段列表, 别乱猜字段名。",
    "- get_schema 只在「用户明确要分析结构」或「你确实拿不准字段、又问不出来」时才用 ——",
    "  不要每次写查询前都习惯性分析一遍, 那样又慢又费 token。",
    "  换库先 set_active_context, 库所在连接没打开先 open_connection。",
    "- 要修改/重构编辑器里**已有的脚本**时, 用 propose_editor_edit (以 diff 形式让用户确认 Accept)。",
    "  content 要给**完整**的新内容, 不是 diff 片段。",
    '- 用户说"分析选中的代码/这段"时, 用 get_editor_selection 拿到选区再分析。',
    "- 写脚本前可以 list_scripts 看脚本库里有什么参考, get_script 读具体内容。",
    "- 编辑器内容是 mongosh 脚本语法 (db.coll.find(...), 支持 ObjectId()/ISODate() 等)。",
    "- **remember_fact 使用时机**: 用户告知了应该跨会话生效的非常识性事实 (集合特殊字段含义 / 隐藏约定 / 数据坑),",
    "  或者一次任务里挖出来的关键知识值得留档 —— 就存下来。反之: 转瞬即逝的对话状态不要存, 存了会污染。",
    "  用简短 key (如 'users.phone.encrypted') + 一句 value。写规则性的偏好请让用户去 AI 设置 → 规范里写, 别用 remember_fact。",
    "",
    "回复用中文, 简洁。完成任务后简短说明你做了什么。",
  ].join("\n");

  // ---- Layer 2: rules + facts (半稳定, 修改后新的 cache 建立) ----
  const rulesLines: string[] = [];
  if (input.globalRules.trim()) {
    rulesLines.push("用户全局规范 (Rules) —— 每个任务都要遵守:", input.globalRules.trim());
  }
  if (input.connRules.trim()) {
    if (rulesLines.length) rulesLines.push("");
    rulesLines.push("当前连接的规范 —— 覆盖全局:", input.connRules.trim());
  }
  if (input.facts.length) {
    if (rulesLines.length) rulesLines.push("");
    rulesLines.push("已记住的事实 (由 remember_fact 累积, 按作用域):");
    for (const f of input.facts) {
      rulesLines.push(`- [${f.scope}] ${f.key}: ${f.value}`);
    }
  }
  const rules = rulesLines.join("\n");

  // ---- Layer 3: volatile (每轮都在变) ----
  const volLines: string[] = [
    "当前实时状态:",
    `- 已连接的服务器: ${activeConns.length > 0 ? activeConns.map((c) => c.name).join(", ") : "(无)"}`,
    `- 当前标签页: ${
      tab
        ? `"${tab.title}" (库:${tab.database || "无"}, 集合:${tab.collection || "无"})`
        : "(没有打开的标签页)"
    }`,
    `- 用户当前${hasSelection ? "**有**" : "没有"}在编辑器里选中代码`,
  ];
  if (recentErr) {
    volLines.push(
      "",
      "**最近一次查询执行报错** (5 分钟内, 供你自我修正参考):",
      `- 出错标签页: ${recentErr.tabTitle}`,
      `- 查询语句: ${recentErr.query}`,
      `- 错误信息: ${recentErr.error}`,
      "如果用户接下来让你 '改一下' / '修下' 之类, 直接基于这条错误改, 不要反问已知信息。",
    );
  }
  const volatile = volLines.join("\n");

  return { stable, rules, volatile };
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

  // ---- Rules (用户规范) ----
  // 全局规范 + 每个连接单独一份, 拼进 system prompt 让 AI 遵守用户偏好, 免得每次都重新说.
  const globalRules = ref<string>("");
  const connRulesCache = ref<Record<string, string>>({});
  /** 全局 rules 已加载过 —— 避免重复请求 */
  const globalRulesLoaded = ref(false);
  /** 已加载过的连接级 rules 的 connId 集合 */
  const connRulesLoaded = ref<Set<string>>(new Set());

  async function loadGlobalRules() {
    if (globalRulesLoaded.value) return;
    try {
      globalRules.value = await aiApi.getAiRules("global");
    } catch {
      /* 数据库暂不可用 -> 静默 */
    }
    globalRulesLoaded.value = true;
  }

  async function saveGlobalRules(content: string) {
    await aiApi.saveAiRules("global", content);
    globalRules.value = content;
    globalRulesLoaded.value = true;
  }

  async function loadConnRules(connId: string) {
    if (!connId || connRulesLoaded.value.has(connId)) return;
    try {
      connRulesCache.value = {
        ...connRulesCache.value,
        [connId]: await aiApi.getAiRules(`conn:${connId}`),
      };
    } catch {
      /* ignore */
    }
    const next = new Set(connRulesLoaded.value);
    next.add(connId);
    connRulesLoaded.value = next;
  }

  async function saveConnRules(connId: string, content: string) {
    if (!connId) return;
    await aiApi.saveAiRules(`conn:${connId}`, content);
    connRulesCache.value = { ...connRulesCache.value, [connId]: content };
    const next = new Set(connRulesLoaded.value);
    next.add(connId);
    connRulesLoaded.value = next;
  }

  // ---- Conversations (持久化到 SQLite: ai_conversations + ai_messages) ----
  const conversations = ref<AiConversation[]>([]);
  const activeConversationId = ref<string | null>(null);
  const conversationsListLoaded = ref(false);

  const activeConversation = computed(
    () => conversations.value.find((c) => c.id === activeConversationId.value) ?? null,
  );

  /** 从 DB 拉最近 200 条会话的元信息 (不含消息, 打开某条时才 hydrate 消息) */
  async function loadConversationList() {
    if (conversationsListLoaded.value) return;
    try {
      const rows = await aiApi.listAiConversations();
      conversations.value = rows.map((r) => ({
        id: r.id,
        title: r.title,
        messages: [],
        connectionId: r.connectionId,
        database: r.database,
        collection: r.collection,
        createdAt: r.createdAt,
        updatedAt: r.updatedAt,
        messagesHydrated: false,
        nextPosition: 0,
        persistedMeta: true,
      }));
    } catch {
      /* DB 未就绪 -> 静默, 用户还是能开新对话 */
    }
    conversationsListLoaded.value = true;
  }

  /** 打开某条历史会话 —— 首次会从 DB 拉消息, 再次打开走内存. */
  async function selectConversation(id: string) {
    const conv = conversations.value.find((c) => c.id === id);
    if (!conv) return;
    activeConversationId.value = id;
    if (!conv.messagesHydrated) {
      try {
        const rows = await aiApi.getAiMessages(id);
        const msgs: AgentMessage[] = [];
        let maxPos = -1;
        for (const r of rows) {
          try {
            msgs.push(JSON.parse(r.payload) as AgentMessage);
          } catch {
            /* 损坏的行跳过 */
          }
          if (r.position > maxPos) maxPos = r.position;
        }
        conv.messages = msgs;
        conv.nextPosition = maxPos + 1;
        conv.messagesHydrated = true;
      } catch {
        conv.messagesHydrated = true;
      }
    }
  }

  /** 只往内存加一条空会话, 不落库. 用户发第一条消息时 pushMessage 会触发 upsert. */
  function createConversation(
    connectionId?: string,
    database?: string,
    collection?: string,
  ): string {
    const id = crypto.randomUUID();
    const now = Date.now();
    const title = collection ? `${database}.${collection}` : database || "新对话";
    conversations.value.unshift({
      id,
      title,
      messages: [],
      connectionId,
      database,
      collection,
      createdAt: now,
      updatedAt: now,
      messagesHydrated: true, // 新建的会话就是空, 不需要 hydrate
      nextPosition: 0,
      persistedMeta: false,
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

  /** 返回当前活跃会话, 没有就新建一个空会话. */
  function ensureActiveConversation(): AiConversation {
    const existing = conversations.value.find((c) => c.id === activeConversationId.value);
    if (existing) return existing;
    const id = createConversation();
    return conversations.value.find((c) => c.id === id)!;
  }

  /** 追加一条消息到内存 + 后端 —— **runAgent 内所有 push 都用它**, 保证落库不漏. */
  async function pushMessage(conv: AiConversation, msg: AgentMessage) {
    conv.messages.push(msg);
    if (!conv.persistedMeta) {
      // 第一次落地: 先 upsert 会话元信息, 后追加消息
      try {
        await aiApi.upsertAiConversation({
          id: conv.id,
          title: conv.title,
          connectionId: conv.connectionId,
          database: conv.database,
          collection: conv.collection,
        });
        conv.persistedMeta = true;
      } catch {
        /* DB 挂了就纯内存运行, 不影响本次对话 */
        return;
      }
    }
    const pos = conv.nextPosition++;
    conv.updatedAt = Date.now();
    try {
      await aiApi.appendAiMessage({
        conversationId: conv.id,
        position: pos,
        payload: JSON.stringify(msg),
      });
    } catch {
      /* DB 短暂挂了 -> 消息仍在内存里, 不阻断 UI */
    }
  }

  async function clearConversation(id?: string) {
    if (pendingQuestion.value) answerQuestion("(用户清空了对话, 取消本次询问)");
    const convId = id ?? activeConversationId.value;
    if (!convId) return;
    const conv = conversations.value.find((c) => c.id === convId);
    if (!conv) return;
    conv.messages = [];
    conv.nextPosition = 0;
    if (conv.persistedMeta) {
      try {
        await aiApi.clearAiConversation(convId);
      } catch {
        /* ignore */
      }
    }
  }

  async function deleteConversation(id: string) {
    const idx = conversations.value.findIndex((c) => c.id === id);
    if (idx === -1) return;
    const conv = conversations.value[idx];
    conversations.value.splice(idx, 1);
    if (activeConversationId.value === id) {
      activeConversationId.value = conversations.value[0]?.id ?? null;
    }
    if (conv.persistedMeta) {
      try {
        await aiApi.deleteAiConversation(id);
      } catch {
        /* ignore */
      }
    }
  }

  /** 手动改会话标题 */
  async function renameConversation(id: string, title: string) {
    const conv = conversations.value.find((c) => c.id === id);
    if (!conv) return;
    conv.title = title;
    if (conv.persistedMeta) {
      try {
        await aiApi.updateAiConversationTitle(id, title);
      } catch {
        /* ignore */
      }
    }
  }

  // ---- Facts (remember_fact 工具后端) ----
  // 内存缓存: scope -> facts. 每次 runAgent 前按当前上下文的 scopes 拉一次.
  const factsCache = ref<Record<string, StoredFact[]>>({});

  /** 当前上下文该看到的 fact scopes: 全局 + 连接 + 库 + 集合 (由粗到细) */
  function currentFactScopes(): string[] {
    const editorStore = useEditorStore();
    const tab = editorStore.activeTab;
    const scopes: string[] = ["global"];
    if (tab?.connectionId) {
      scopes.push(`conn:${tab.connectionId}`);
      if (tab.database) {
        scopes.push(`conn:${tab.connectionId}:db:${tab.database}`);
        if (tab.collection) {
          scopes.push(`conn:${tab.connectionId}:db:${tab.database}:coll:${tab.collection}`);
        }
      }
    }
    return scopes;
  }

  async function refreshFacts(scopes: string[]): Promise<StoredFact[]> {
    if (scopes.length === 0) return [];
    try {
      const rows = await aiApi.listAiFacts(scopes);
      // 更新缓存 (按 scope 分组)
      const grouped: Record<string, StoredFact[]> = {};
      for (const r of rows) {
        if (!grouped[r.scope]) grouped[r.scope] = [];
        grouped[r.scope].push(r);
      }
      factsCache.value = { ...factsCache.value, ...grouped };
      // 空 scope 也清一下 (删除后)
      for (const s of scopes) if (!grouped[s]) factsCache.value[s] = [];
      return rows;
    } catch {
      return [];
    }
  }

  async function rememberFact(scope: string, key: string, value: string) {
    await aiApi.rememberAiFact({ scope, key, value });
    await refreshFacts([scope]);
  }

  async function forgetFact(scope: string, key: string) {
    await aiApi.forgetAiFact(scope, key);
    await refreshFacts([scope]);
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
   *
   * 消息全部通过 pushMessage 落到 SQLite —— 重启应用后仍能翻回历史.
   */
  async function runAgent(text: string): Promise<void> {
    const conv = ensureActiveConversation();
    // 用户消息落库前先决定 title (第一条 user message 决定标题)
    const isFirstUserMsg = conv.messages.filter((m) => m.role === "user").length === 0;
    if (isFirstUserMsg) {
      conv.title = text.slice(0, 20) + (text.length > 20 ? "..." : "");
    }
    await pushMessage(conv, { role: "user", content: text });
    // 第一条消息触发了会话元信息落库, title 也已用短版本. 如果需要, 后端已有.
    // 但 pushMessage 只 upsert 一次, 后续 title 手动改要另发请求
    if (isFirstUserMsg && conv.persistedMeta) {
      try {
        await aiApi.updateAiConversationTitle(conv.id, conv.title);
      } catch {
        /* ignore */
      }
    }

    loading.value = true;
    abortRequested.value = false;

    // 首次进入 agent 循环前先把 rules + facts 拉齐 (拉过就跳过, 不阻塞)
    await loadGlobalRules();
    const currConnId = useEditorStore().activeTab?.connectionId || "";
    if (currConnId) await loadConnRules(currConnId);
    const scopes = currentFactScopes();
    await refreshFacts(scopes);

    try {
      for (let step = 0; step < MAX_AGENT_STEPS; step++) {
        if (abortRequested.value) break;
        const connId = useEditorStore().activeTab?.connectionId || "";
        // 每轮实时拿最新的 facts (agent 可能刚 remember 了一条, 下一轮要能看到)
        const currentScopes = currentFactScopes();
        const relevantFacts: StoredFact[] = [];
        for (const s of currentScopes) {
          for (const f of factsCache.value[s] || []) relevantFacts.push(f);
        }

        const layers = buildSystemPromptLayers({
          globalRules: globalRules.value,
          connRules: (connId && connRulesCache.value[connId]) || "",
          facts: relevantFacts,
        });

        // 三段独立 system —— 前两段带 cacheable=true, 后端会加 cache_control 缓存.
        // conv.messages 里可能已经有历史消息 (来自 hydrate), 全部一起发, 保持上下文.
        const systemMsgs: AgentMessage[] = [
          { role: "system", content: layers.stable, cacheable: true },
        ];
        if (layers.rules) systemMsgs.push({ role: "system", content: layers.rules, cacheable: true });
        systemMsgs.push({ role: "system", content: layers.volatile });

        // 模型请求 与 "用户点停止" 赛跑
        const t0 = performance.now();
        const turnPromise = aiApi.aiAgentTurn([...systemMsgs, ...conv.messages], AGENT_TOOLS);
        turnPromise.catch(() => {});
        const abortBarrier = new Promise<"abort">((resolve) => {
          releaseAbortBarrier = () => resolve("abort");
        });
        const raced = await Promise.race([turnPromise, abortBarrier]);
        releaseAbortBarrier = null;
        if (raced === "abort") break;
        const turn = raced;
        const turnMs = Math.round(performance.now() - t0);

        await pushMessage(conv, {
          role: "assistant",
          content: turn.text ?? undefined,
          toolCalls: turn.toolCalls.length > 0 ? turn.toolCalls : undefined,
          durationMs: turnMs,
        });

        if (turn.toolCalls.length === 0) return;

        for (const call of turn.toolCalls) {
          if (abortRequested.value) break;
          const tt0 = performance.now();
          let result: string;
          if (call.name === "ask_user") {
            const q = String(call.input.question ?? "");
            const rawOpts = call.input.options;
            const opts = Array.isArray(rawOpts) ? rawOpts.map((o) => String(o)) : [];
            result = `用户回答: ${await askUser(q, opts)}`;
          } else if (call.name === "remember_fact") {
            const scope = String(call.input.scope || "global");
            const key = String(call.input.key || "").trim();
            const value = String(call.input.value || "").trim();
            if (!key || !value) {
              result = "失败: remember_fact 需要非空的 key 和 value";
            } else {
              try {
                await rememberFact(scope, key, value);
                result = `已记住 [${scope}] ${key} = ${value}. 之后同作用域的对话都能看到.`;
              } catch (e) {
                result = `记录失败: ${e}`;
              }
            }
          } else if (call.name === "forget_fact") {
            const scope = String(call.input.scope || "global");
            const key = String(call.input.key || "").trim();
            if (!key) {
              result = "失败: forget_fact 需要 key";
            } else {
              try {
                await forgetFact(scope, key);
                result = `已删除 [${scope}] ${key}.`;
              } catch (e) {
                result = `删除失败: ${e}`;
              }
            }
          } else {
            try {
              result = await executeTool(call.name, call.input);
            } catch (e) {
              result = `工具执行异常: ${e}`;
            }
          }
          await pushMessage(conv, {
            role: "tool",
            toolCallId: call.id,
            content: result,
            durationMs: Math.round(performance.now() - tt0),
          });
        }
      }

      await pushMessage(conv, {
        role: "assistant",
        content: abortRequested.value
          ? "(已按你的要求停止。可以再追问让我继续。)"
          : `(到达 ${MAX_AGENT_STEPS} 步安全上限, 自动停下了 —— 这是防止 agent 失控空转的保护, ` +
            `不是任务失败。可以直接追问"继续"让我接着做。)`,
      });
    } catch (e) {
      await pushMessage(conv, { role: "assistant", content: `错误: ${e}` });
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
    // rules
    globalRules,
    connRulesCache,
    loadGlobalRules,
    saveGlobalRules,
    loadConnRules,
    saveConnRules,
    // conversations
    conversations,
    activeConversationId,
    activeConversation,
    conversationsListLoaded,
    loadConversationList,
    selectConversation,
    createConversation,
    ensureConversation,
    ensureActiveConversation,
    clearConversation,
    deleteConversation,
    renameConversation,
    // facts
    factsCache,
    currentFactScopes,
    refreshFacts,
    rememberFact,
    forgetFact,
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

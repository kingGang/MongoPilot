import { defineStore } from "pinia";
import { ref, computed } from "vue";
import type { EditorTab, ResultTab, ResultTabKind } from "@/types/database";
import * as queryApi from "@/api/query";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useConnectionStore } from "@/stores/connection";

const MAX_RESULT_TABS = 10;

export const useEditorStore = defineStore("editor", () => {
  const tabs = ref<EditorTab[]>([]);
  const activeTabId = ref<string | null>(null);

  const activeTab = computed(() => tabs.value.find((t) => t.id === activeTabId.value) ?? null);

  /** 当前编辑器 tab 激活的结果 tab */
  const activeResultTab = computed<ResultTab | null>(() => {
    const t = activeTab.value;
    if (!t) return null;
    return t.resultTabs.find((r) => r.id === t.activeResultTabId) ?? null;
  });

  function getHostLabel(connectionId: string): string {
    try {
      const connStore = useConnectionStore();
      const cfg = connStore.connections.find((c) => c.id === connectionId);
      if (cfg) return cfg.name || `${cfg.host}:${cfg.port}`;
    } catch {
      /* store not ready */
    }
    return "";
  }

  function createTab(connectionId: string, database: string, collection?: string) {
    const id = crypto.randomUUID();
    const host = getHostLabel(connectionId);
    let baseTitle: string;
    if (collection) {
      baseTitle = host ? `${database}:${collection}@${host}` : `${database}:${collection}`;
    } else {
      baseTitle = host ? `Query@${host}` : "Query";
    }
    // 去重：如果已存在同名 tab，加编号
    const existing = tabs.value.filter(
      (t) => t.title === baseTitle || t.title.startsWith(`${baseTitle} (`),
    ).length;
    const title = existing > 0 ? `${baseTitle} (${existing + 1})` : baseTitle;
    const tab: EditorTab = {
      id,
      title,
      connectionId,
      database,
      collection: collection || "",
      content: "",
      resultTabs: [],
      activeResultTabId: null,
    };
    tabs.value.push(tab);
    activeTabId.value = id;
    return id;
  }

  function closeTab(id: string) {
    const idx = tabs.value.findIndex((t) => t.id === id);
    if (idx === -1) return;
    tabs.value.splice(idx, 1);
    if (activeTabId.value === id) {
      activeTabId.value = tabs.value[Math.min(idx, tabs.value.length - 1)]?.id ?? null;
    }
  }

  function setContent(id: string, content: string) {
    const tab = tabs.value.find((t) => t.id === id);
    if (tab) tab.content = content;
  }

  // ---- 结果 tab 管理 ----
  function buildResultTitle(kind: ResultTabKind, existing: ResultTab[]): string {
    const base = kind === "find" ? "Find" : "Explain";
    const same = existing.filter((r) => r.kind === kind).length;
    return same === 0 ? base : `${base} (${same + 1})`;
  }

  function newResultTab(kind: ResultTabKind, queryText: string, existing: ResultTab[]): ResultTab {
    return {
      id: crypto.randomUUID(),
      kind,
      title: buildResultTitle(kind, existing),
      queryText,
      result: null,
      explainResult: null,
      error: null,
      loading: true,
      currentQueryId: null,
      currentPage: 1,
      pageSize: 50,
      createdAt: Date.now(),
      aborted: false,
    };
  }

  /** 新增 result tab 并 set active; 超过上限就淘汰最早那个 */
  function pushResultTab(editorTab: EditorTab, rt: ResultTab) {
    editorTab.resultTabs.push(rt);
    while (editorTab.resultTabs.length > MAX_RESULT_TABS) {
      const removed = editorTab.resultTabs.shift();
      if (removed && editorTab.activeResultTabId === removed.id) {
        editorTab.activeResultTabId = editorTab.resultTabs[0]?.id ?? null;
      }
    }
    editorTab.activeResultTabId = rt.id;
  }

  function activateResultTab(editorTabId: string, resultTabId: string) {
    const tab = tabs.value.find((t) => t.id === editorTabId);
    if (tab && tab.resultTabs.some((r) => r.id === resultTabId)) {
      tab.activeResultTabId = resultTabId;
    }
  }

  function closeResultTab(editorTabId: string, resultTabId: string) {
    const tab = tabs.value.find((t) => t.id === editorTabId);
    if (!tab) return;
    const idx = tab.resultTabs.findIndex((r) => r.id === resultTabId);
    if (idx === -1) return;
    tab.resultTabs.splice(idx, 1);
    if (tab.activeResultTabId === resultTabId) {
      const nextIdx = Math.min(idx, tab.resultTabs.length - 1);
      tab.activeResultTabId = nextIdx >= 0 ? tab.resultTabs[nextIdx].id : null;
    }
  }

  function closeOtherResultTabs(editorTabId: string, keepId: string) {
    const tab = tabs.value.find((t) => t.id === editorTabId);
    if (!tab) return;
    tab.resultTabs = tab.resultTabs.filter((r) => r.id === keepId);
    tab.activeResultTabId = tab.resultTabs[0]?.id ?? null;
  }

  function closeLeftOfResultTab(editorTabId: string, resultTabId: string) {
    const tab = tabs.value.find((t) => t.id === editorTabId);
    if (!tab) return;
    const idx = tab.resultTabs.findIndex((r) => r.id === resultTabId);
    if (idx <= 0) return;
    tab.resultTabs.splice(0, idx);
    tab.activeResultTabId = resultTabId;
  }

  function closeRightOfResultTab(editorTabId: string, resultTabId: string) {
    const tab = tabs.value.find((t) => t.id === editorTabId);
    if (!tab) return;
    const idx = tab.resultTabs.findIndex((r) => r.id === resultTabId);
    if (idx < 0) return;
    tab.resultTabs.splice(idx + 1);
    tab.activeResultTabId = resultTabId;
  }

  function closeAllResultTabs(editorTabId: string) {
    const tab = tabs.value.find((t) => t.id === editorTabId);
    if (!tab) return;
    tab.resultTabs = [];
    tab.activeResultTabId = null;
  }

  // ---- 执行查询 ----
  /** push 一个新结果 tab 并返回它在 reactive 数组里的代理引用 */
  function spawnResultTab(
    tab: EditorTab,
    kind: ResultTabKind,
    queryText: string,
  ): ResultTab | null {
    const draft = newResultTab(kind, queryText, tab.resultTabs);
    draft.currentQueryId = crypto.randomUUID();
    pushResultTab(tab, draft);
    // 关键: push 后从数组里按 id 捞回 reactive 代理, 直接改 draft 不会触发响应式更新
    return tab.resultTabs.find((r) => r.id === draft.id) ?? null;
  }

  /** 执行查询 —— 每次追加一个 Find 结果 tab */
  async function executeQuery(editorTabId: string, queryText?: string) {
    const tab = tabs.value.find((t) => t.id === editorTabId);
    if (!tab) return;
    const text = (queryText ?? tab.content).trim();
    if (!text) return;

    const rt = spawnResultTab(tab, "find", text);
    if (!rt) return;

    try {
      const res = await queryApi.runQuery(
        tab.connectionId,
        tab.database,
        text,
        0,
        rt.pageSize,
        rt.currentQueryId ?? undefined,
      );
      if (rt.aborted) return;
      rt.result = res;
    } catch (e) {
      if (rt.aborted) return;
      rt.error = friendlyError(e);
    } finally {
      if (!rt.aborted) rt.loading = false;
    }
  }

  /** Explain —— 每次追加一个 Explain 结果 tab */
  async function executeExplain(editorTabId: string, queryText?: string) {
    const tab = tabs.value.find((t) => t.id === editorTabId);
    if (!tab) return;
    const text = (queryText ?? tab.content).trim();
    if (!text) return;

    const rt = spawnResultTab(tab, "explain", text);
    if (!rt) return;

    try {
      const res = await invoke<Record<string, unknown>>("explain_shell_query", {
        connectionId: tab.connectionId,
        database: tab.database,
        queryText: text,
      });
      if (rt.aborted) return;
      rt.explainResult = res;
    } catch (e) {
      if (rt.aborted) return;
      rt.error = friendlyError(e);
    } finally {
      if (!rt.aborted) rt.loading = false;
    }
  }

  /** 翻页 —— 在指定结果 tab 上重跑查询 */
  async function fetchPage(
    editorTabId: string,
    resultTabId: string,
    page: number,
    pageSize?: number,
  ) {
    const tab = tabs.value.find((t) => t.id === editorTabId);
    // 这里 find 返回的是 Pinia 的 reactive 代理, 后续修改会触发更新
    const rt = tab?.resultTabs.find((r) => r.id === resultTabId);
    if (!tab || !rt || !rt.queryText) return;

    if (pageSize !== undefined) rt.pageSize = pageSize;
    rt.currentPage = page;
    rt.loading = true;
    rt.aborted = false;
    rt.currentQueryId = crypto.randomUUID();

    const skip = (page - 1) * rt.pageSize;
    try {
      const res = await queryApi.runQuery(
        tab.connectionId,
        tab.database,
        rt.queryText,
        skip,
        rt.pageSize,
        rt.currentQueryId,
      );
      if (rt.aborted) return;
      rt.result = res;
      rt.error = null;
    } catch (e) {
      if (rt.aborted) return;
      rt.error = friendlyError(e);
    } finally {
      if (!rt.aborted) rt.loading = false;
    }
  }

  /** 停止当前在途查询 (前端侧 abort —— 后端无法真正取消, 仅丢弃即将回来的结果) */
  function stopResultTab(editorTabId: string, resultTabId: string) {
    const tab = tabs.value.find((t) => t.id === editorTabId);
    const rt = tab?.resultTabs.find((r) => r.id === resultTabId);
    if (!rt || !rt.loading) return;
    rt.aborted = true;
    rt.loading = false;
    rt.error = "已取消";
  }

  // 监听后端异步计数事件, 在所有 tab 的所有 result tab 中按 queryId 定位.
  listen<{ queryId: string; totalCount: number }>("query:count-ready", (e) => {
    const { queryId, totalCount } = e.payload;
    if (totalCount === -2) return; // 计数失败, 保留 -1
    for (const t of tabs.value) {
      const rt = t.resultTabs.find((r) => r.currentQueryId === queryId);
      if (rt && rt.result) {
        rt.result = { ...rt.result, totalCount };
        return;
      }
    }
  }).catch(() => {
    /* 非 Tauri 环境静默 */
  });

  /** 将原始错误转为友好提示 */
  function friendlyError(e: unknown): string {
    const s = String(e);
    if (s.includes("E11000")) return "写入失败: 重复键值 (duplicate key)";
    if (s.includes("Authentication failed")) return "认证失败: 用户名或密码错误";
    if (s.includes("not authorized")) return "权限不足: 当前用户没有执行此操作的权限";
    if (s.includes("timed out")) return "连接超时: 无法连接到 MongoDB 服务器";
    if (s.includes("connection refused") || s.includes("Connection refused"))
      return "连接被拒绝: 请检查主机地址和端口";
    if (s.includes("ns not found")) return "集合不存在或已被删除";
    if (s.includes("10054") || s.includes("强迫关闭") || s.includes("forcibly closed"))
      return "连接已断开: 服务器关闭了连接，请重试或重新连接";
    if (s.includes("broken pipe") || s.includes("reset by peer"))
      return "连接已断开: 网络中断，请重试";
    if (s.includes("JSON 解析失败")) return s;
    if (s.includes("输入无效")) return s;
    return s;
  }

  return {
    tabs,
    activeTabId,
    activeTab,
    activeResultTab,
    createTab,
    closeTab,
    setContent,
    executeQuery,
    executeExplain,
    fetchPage,
    activateResultTab,
    closeResultTab,
    closeOtherResultTabs,
    closeLeftOfResultTab,
    closeRightOfResultTab,
    closeAllResultTabs,
    stopResultTab,
  };
});

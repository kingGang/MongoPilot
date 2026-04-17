import { defineStore } from "pinia";
import { ref, computed } from "vue";
import type { EditorTab } from "@/types/database";
import * as queryApi from "@/api/query";
import { useConnectionStore } from "@/stores/connection";

export const useEditorStore = defineStore("editor", () => {
  const tabs = ref<EditorTab[]>([]);
  const activeTabId = ref<string | null>(null);

  const activeTab = computed(() => tabs.value.find((t) => t.id === activeTabId.value) ?? null);

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
      result: null,
      error: null,
      loading: false,
      lastQueryText: "",
      currentPage: 1,
      pageSize: 50,
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

  /** 执行查询（首次或新查询，从第 1 页开始） */
  async function executeQuery(id: string, queryText?: string) {
    const tab = tabs.value.find((t) => t.id === id);
    if (!tab) return;
    const text = (queryText ?? tab.content).trim();
    if (!text) return;

    tab.loading = true;
    tab.error = null;
    tab.result = null;
    tab.lastQueryText = text;
    tab.currentPage = 1;

    try {
      tab.result = await queryApi.runQuery(tab.connectionId, tab.database, text, 0, tab.pageSize);
    } catch (e) {
      tab.error = friendlyError(e);
    } finally {
      tab.loading = false;
    }
  }

  /** 翻页：用同一条查询语句请求新的一页 */
  async function fetchPage(id: string, page: number, pageSize?: number) {
    const tab = tabs.value.find((t) => t.id === id);
    if (!tab || !tab.lastQueryText) return;

    if (pageSize !== undefined) tab.pageSize = pageSize;
    tab.currentPage = page;
    tab.loading = true;

    const skip = (page - 1) * tab.pageSize;
    try {
      tab.result = await queryApi.runQuery(
        tab.connectionId,
        tab.database,
        tab.lastQueryText,
        skip,
        tab.pageSize,
      );
      tab.error = null;
    } catch (e) {
      tab.error = friendlyError(e);
    } finally {
      tab.loading = false;
    }
  }

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

  return { tabs, activeTabId, activeTab, createTab, closeTab, setContent, executeQuery, fetchPage };
});

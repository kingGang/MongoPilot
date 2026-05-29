import { defineStore, acceptHMRUpdate } from "pinia";
import { ref, computed } from "vue";
import type { EditorTab, ResultTab, ResultTabKind, TabExecutor } from "@/types/database";
import * as queryApi from "@/api/query";
import * as collApi from "@/api/collectionMgmt";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useConnectionStore } from "@/stores/connection";
import {
  collectScriptOps,
  extractDbStatements,
  extractLoadPaths,
  extractPureHelpers,
  needsPreEvaluation,
  preEvaluateStatement,
  scriptOpToStatement,
} from "@/utils/query-preeval";

/** load("...") 引用是不是绝对文件系统路径 (Windows 盘符 / UNC / Unix 绝对路径) */
function isFilesystemPath(p: string): boolean {
  return /^[a-zA-Z]:[\\/]/.test(p) || p.startsWith("\\\\") || p.startsWith("/");
}

/**
 * 解析单个 load("...") 引用的内容.
 * 绝对路径 -> 读文件系统; 其它 -> 当成脚本库引用 (文件夹/脚本名). 任一失败时再试另一边.
 */
async function resolveLoadContent(path: string): Promise<string | null> {
  const fromFs = () => invoke<string>("read_import_file", { path });
  const fromScript = () => invoke<string>("resolve_script_ref", { reference: path });
  const looksFs = isFilesystemPath(path);
  try {
    return looksFs ? await fromFs() : await fromScript();
  } catch {
    try {
      return looksFs ? await fromScript() : await fromFs();
    } catch {
      return null;
    }
  }
}

/** 递归读取 load("...") 引用的内容 (文件系统或脚本库); 带 visited 防环 + 深度上限 */
async function readLoadedHelpers(
  content: string,
  visited: Set<string>,
  depth: number,
): Promise<string> {
  if (depth > 5) return "";
  let acc = "";
  for (const path of extractLoadPaths(content)) {
    const norm = path.replace(/\\/g, "/");
    if (visited.has(norm)) continue;
    visited.add(norm);
    const fileContent = await resolveLoadContent(path);
    if (fileContent == null) continue; // 读不到 -> 跳过, 不阻断
    // 先递归处理被引入文件里的 load()
    const nested = await readLoadedHelpers(fileContent, visited, depth + 1);
    acc += `${nested}\n// ===== loaded: ${path} =====\n${fileContent}\n`;
  }
  return acc;
}

/**
 * 检测脚本是否把 db 读操作绑到了变量上 (read-then-write 模式).
 * 例如: `var player = db.player.findOne(...)` —— 这种模式必须走完整 script mode,
 * 因为 preEvaluateStatement 里 db 是只捕获不执行的 Proxy, 变量拿到的是 Proxy 不是真数据,
 * 后续 `player._id` 求值会得到一个函数, JSON.stringify 失败 -> 后端报 "expected value".
 */
function hasDbReadInVar(content: string): boolean {
  // 先把字符串字面量替成空占位, 避免 var x = "db.xxx" 误判
  const noStrings = content.replace(/"(?:[^"\\]|\\.)*"/g, '""').replace(/'(?:[^'\\]|\\.)*'/g, "''");
  return /\b(?:var|let|const)\s+[\w$]+\s*=\s*[^;\n]*\bdb\./m.test(noStrings);
}

/**
 * 编辑器里有 helper 定义 / load() 引用时, 把语句参数里的函数调用在 webview JS 引擎里
 * 求值成纯 JSON. 求值失败时退回原文.
 */
async function resolveQueryText(fullContent: string, statement: string): Promise<string> {
  if (!needsPreEvaluation(fullContent)) return statement;
  try {
    const loadedHelpers = await readLoadedHelpers(fullContent, new Set(), 0);
    return preEvaluateStatement(fullContent, statement, loadedHelpers);
  } catch {
    return statement;
  }
}

const MAX_RESULT_TABS = 10;

/** 剥掉 //... 行注释 与 /* ... *\/ 块注释 (字符串内同样写法保留) */
function stripJsCommentsInline(text: string): string {
  let out = "";
  const n = text.length;
  let i = 0;
  while (i < n) {
    const c = text[i];
    if (c === '"' || c === "'") {
      const quote = c;
      out += c;
      i++;
      while (i < n) {
        const sc = text[i];
        out += sc;
        i++;
        if (sc === "\\" && i < n) {
          out += text[i];
          i++;
        } else if (sc === quote) {
          break;
        }
      }
      continue;
    }
    if (c === "/" && text[i + 1] === "/") {
      i += 2;
      while (i < n && text[i] !== "\n") i++;
      continue;
    }
    if (c === "/" && text[i + 1] === "*") {
      i += 2;
      while (i + 1 < n && !(text[i] === "*" && text[i + 1] === "/")) i++;
      i = Math.min(i + 2, n);
      continue;
    }
    out += c;
    i++;
  }
  return out;
}

export const useEditorStore = defineStore("editor", () => {
  const tabs = ref<EditorTab[]>([]);
  const activeTabId = ref<string | null>(null);

  /** AI 提议但未确认的编辑: tabId -> 提议的新内容. 编辑器里以 diff 展示, 用户 Accept 才应用 */
  const pendingEdits = ref<Record<string, string>>({});

  /** 每个 tab 当前在编辑器里选中的文本 (MonacoEditor 实时写入, AI 工具读取) */
  const selectionByTab = ref<Record<string, string>>({});
  function setSelection(id: string, text: string) {
    // 选区频繁变化, 只在真正变了时才写, 避免无谓的响应式触发
    if (selectionByTab.value[id] === text) return;
    selectionByTab.value = { ...selectionByTab.value, [id]: text };
  }
  function getSelection(id: string): string {
    return selectionByTab.value[id] ?? "";
  }

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

  function setTabExecutor(id: string, executor: TabExecutor | undefined) {
    const tab = tabs.value.find((t) => t.id === id);
    if (tab) tab.executor = executor;
  }

  function setTabSkipLint(id: string, skip: boolean) {
    const tab = tabs.value.find((t) => t.id === id);
    if (tab) tab.skipLint = skip;
  }

  function closeTab(id: string) {
    const idx = tabs.value.findIndex((t) => t.id === id);
    if (idx === -1) return;
    tabs.value.splice(idx, 1);
    rejectEdit(id); // 连带丢弃未确认的 AI 提议
    if (activeTabId.value === id) {
      activeTabId.value = tabs.value[Math.min(idx, tabs.value.length - 1)]?.id ?? null;
    }
  }

  function setContent(id: string, content: string) {
    const tab = tabs.value.find((t) => t.id === id);
    if (tab) tab.content = content;
  }

  // ---- AI 提议编辑 (pending edit) ----
  /** AI 提议把某个 tab 的内容替换成 content; 不立即生效, 等用户在 diff 里确认 */
  function proposeEdit(id: string, content: string) {
    if (!tabs.value.some((t) => t.id === id)) return;
    pendingEdits.value = { ...pendingEdits.value, [id]: content };
  }
  /** 接受提议: 应用新内容并清掉 pending */
  function acceptEdit(id: string) {
    const content = pendingEdits.value[id];
    if (content === undefined) return;
    setContent(id, content);
    rejectEdit(id);
  }
  /** 拒绝提议: 丢弃 pending, 不改内容 */
  function rejectEdit(id: string) {
    if (pendingEdits.value[id] === undefined) return;
    const next = { ...pendingEdits.value };
    delete next[id];
    pendingEdits.value = next;
  }

  // ---- 结果 tab 管理 ----
  function buildResultTitle(kind: ResultTabKind, existing: ResultTab[]): string {
    const base = kind === "find" ? "Find" : kind === "explain" ? "Explain" : "Console";
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
      consoleLines: kind === "console" ? [] : undefined,
    };
  }

  /** 把 print()/printjson() 输出追加到该编辑器 tab 的 Console 结果页 (单例, 复用) */
  function appendConsole(tab: EditorTab, lines: string[]) {
    if (lines.length === 0) return;
    let rt = tab.resultTabs.find((r) => r.kind === "console") ?? null;
    if (!rt) {
      const draft = newResultTab("console", "", tab.resultTabs);
      pushResultTab(tab, draft);
      rt = tab.resultTabs.find((r) => r.id === draft.id) ?? null;
    }
    if (!rt) return;
    const stamp = new Date().toLocaleTimeString();
    rt.consoleLines = [...(rt.consoleLines ?? []), `--- ${stamp} ---`, ...lines];
    rt.loading = false;
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

    // 特殊执行通道: tab.executor 指定的命令
    if (tab.executor?.kind === "indexInfo") {
      const rt = spawnResultTab(tab, "find", text);
      if (!rt) return;
      const start = Date.now();
      try {
        const doc = await collApi.getIndexInfo(
          tab.connectionId,
          tab.database,
          tab.executor.collection,
          tab.executor.indexName,
        );
        if (rt.aborted) return;
        rt.result = {
          documents: [doc],
          count: 1,
          totalCount: 1,
          executionTimeMs: Date.now() - start,
        };
      } catch (e) {
        if (rt.aborted) return;
        rt.error = friendlyError(e);
      } finally {
        if (!rt.aborted) rt.loading = false;
      }
      return;
    }

    if (tab.executor?.kind === "collectionIndexes") {
      const rt = spawnResultTab(tab, "find", text);
      if (!rt) return;
      const start = Date.now();
      try {
        const docs = await collApi.getCollectionIndexes(
          tab.connectionId,
          tab.database,
          tab.executor.collection,
        );
        if (rt.aborted) return;
        rt.result = {
          documents: docs,
          count: docs.length,
          totalCount: docs.length,
          executionTimeMs: Date.now() - start,
        };
      } catch (e) {
        if (rt.aborted) return;
        rt.error = friendlyError(e);
      } finally {
        if (!rt.aborted) rt.loading = false;
      }
      return;
    }

    // 剥掉注释后仍没有可执行内容 (例如纯注释 tab) 直接跳过, 不触发执行器
    const stripped = stripJsCommentsInline(text).trim();
    if (!stripped) return;

    // 数据库未选时 namespace 会变成 ".collection" -> 提前给友好提示, 不打后端.
    // (show dbs / show databases 不依赖当前库, 放行)
    const isShowDbs = /^show\s+(dbs|databases)\b/.test(stripped);
    if (!tab.database && !isShowDbs) {
      const rt = spawnResultTab(tab, "find", text);
      if (rt) {
        rt.error = "请先选择数据库 (工具栏右侧的数据库下拉)";
        rt.loading = false;
      }
      return;
    }

    // read-then-write 脚本: 把整段交给 script mode (preEval 单语句拿不到 var 里的真实 db 读数据).
    // 关键: 仅在用户跑"整段"或选区本身就含 var=db.xxx 的读绑定时才升级到 script mode;
    // 如果用户明确选中了一条自包含的 db.xxx 语句 (例如调试时只想跑 `db.user.find({})`),
    // 不能因为整段脚本里别处有 read-then-write 模式就把它当脚本跑.
    const explicitSelection = queryText !== undefined;
    const escalateToScript = explicitSelection
      ? hasDbReadInVar(text) // 选区里自己有 var=db.xxx 才升级
      : hasDbReadInVar(tab.content); // 跑整段时按整段判断
    if (needsPreEvaluation(tab.content) && escalateToScript) {
      await executeScript(tab, explicitSelection ? text : tab.content);
      return;
    }

    // 选中的不是直接以 db. 开头 (例如选了 "helper 函数 + 一条查询" 整段):
    //   - 里面有 db.xxx 语句  -> 抽出最后一条当作要跑的查询, 走预求值
    //   - 完全没有 db 语句、又有 helper/load -> 走脚本模式, 但**只跑选区**:
    //     拼 "tab.content 里的纯 helper (函数 + 字面量 var)" + "选区" 成迷你脚本.
    //     这样脚本里其它 print() / db read / 控制流不会被跑掉.
    let runText = text;
    if (!stripped.startsWith("db.") && !stripped.startsWith("use ")) {
      const dbStmts = extractDbStatements(text);
      if (dbStmts.length > 0) {
        runText = dbStmts[dbStmts.length - 1];
      } else if (needsPreEvaluation(tab.content)) {
        // 用户跑整段时, text 已经包含整个脚本 -> 不需要拼 miniScript (拼 helper + 选区
        // 是为了 "选了非 db.* 行只跑选区" 场景). 整段直接走 executeScript 用默认
        // codeToRun = tab.content, 避免 extractPureHelpers 把跨多行 var 截一行
        // 引入 "var X={" 不闭合的语法错.
        const isWholeContent = text.trim() === tab.content.trim();
        if (isWholeContent) {
          await executeScript(tab, text);
        } else {
          const miniScript = `${extractPureHelpers(tab.content)}\n${text}`;
          await executeScript(tab, text, miniScript);
        }
        return;
      }
    }

    // 预求值: helper 函数调用 / load() 引入 -> 纯 JSON 参数. resolved 同时存进 result tab,
    // 翻页时直接复用 resolved 文本, 无需重复求值.
    const resolved = await resolveQueryText(tab.content, runText);

    const rt = spawnResultTab(tab, "find", resolved);
    if (!rt) return;

    try {
      const res = await queryApi.runQuery(
        tab.connectionId,
        tab.database,
        resolved,
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

  /**
   * 脚本模式执行: 命令式脚本在 webview JS 引擎里跑一遍 ——
   * db 读操作真发后端拿数据, 写操作收集后批量执行, print() 输出捕获展示。
   *
   * @param execCode 实际丢给 collectScriptOps 跑的代码; 不传则用 tab.content 整段.
   *                 用户选了非 db.* 行 (例如 `print(...)`) 跑时, executeQuery 会传
   *                 "纯 helper + 选区" 的迷你脚本进来, 避免把整段脚本里其它 print /
   *                 控制流也跑掉, 让用户看到一堆不期望的输出。
   */
  async function executeScript(tab: EditorTab, text: string, execCode?: string) {
    const rt = spawnResultTab(tab, "find", text);
    if (!rt) return;
    // 只读连接早拦: 脚本模式会在 webview 里跑用户的 for 循环 / load() 进来的 helper,
    // 里面常含 insert/update/delete (例如 batchGenDolls 这种造数脚本) -> 即使后端
    // run_script_ops 会拒绝, 前端在 collectScriptOps 里跑循环 + 串行 await read 也
    // 可能让用户以为"一直转圈". 直接在入口拒绝, 给明确提示。
    try {
      const connStore = useConnectionStore();
      if (connStore.isReadOnly(tab.connectionId)) {
        rt.error = "只读连接: 不允许执行脚本 (脚本可能包含 insert/update/delete 等写操作)";
        rt.loading = false;
        return;
      }
    } catch {
      /* store not ready, 放行让后续逻辑处理 */
    }
    const start = Date.now();
    try {
      const loadedHelpers = await readLoadedHelpers(tab.content, new Set(), 0);
      // 脚本里的 db 读操作真去后端执行 (read-then-write 脚本需要真实数据)
      const runRead = async (statement: string) => {
        const res = await queryApi.runQuery(tab.connectionId, tab.database, statement, 0, 1000);
        return { documents: res.documents, count: res.count };
      };
      const codeToRun = execCode ?? tab.content;
      const { ops, output, error } = await collectScriptOps(codeToRun, loadedHelpers, runRead);
      if (rt.aborted) return;

      if (error) {
        rt.error = `脚本执行出错: ${error}`;
        rt.loading = false;
        return;
      }

      // 收集到写操作 -> 批量发后端执行
      let summary: {
        total: number;
        ok: number;
        failed: number;
        firstError: string | null;
        elapsedMs: number;
      } | null = null;
      if (ops.length > 0) {
        const statements = ops.map(scriptOpToStatement);
        summary = await invoke("run_script_ops", {
          request: {
            connectionId: tab.connectionId,
            database: tab.database,
            statements,
          },
        });
        if (rt.aborted) return;
      }

      // print() 输出单独进 Console 结果页
      if (output.length > 0) appendConsole(tab, output);

      // 结果文档: 写操作汇总
      const doc: Record<string, unknown> = {};
      if (summary) {
        doc["写操作"] = `${summary.ok} 成功 / ${summary.failed} 失败 (共 ${summary.total})`;
        if (summary.firstError) doc["首个错误"] = summary.firstError;
      } else {
        doc["写操作"] = "无 (脚本只有读操作, 或写操作被注释掉了)";
      }
      if (output.length > 0) doc["脚本输出"] = `${output.length} 行 -> 见 Console 结果页`;
      else if (!summary) doc["提示"] = "脚本跑完了, 但既没有写操作也没有 print 输出。";

      rt.result = {
        documents: [doc],
        count: 1,
        totalCount: 1,
        executionTimeMs: Date.now() - start,
      };
      // appendConsole 会把 Console 页设为 active; 焦点拉回写操作汇总页
      tab.activeResultTabId = rt.id;
    } catch (e) {
      if (rt.aborted) return;
      rt.error = friendlyError(e);
    } finally {
      if (!rt.aborted) rt.loading = false;
    }
  }

  /**
   * 运行编辑器里的**全部**语句 —— 顺序执行每一条 db.xxx 语句, 各自产生一个结果 tab。
   * 没有可拆分的 db 语句、但有 helper/load -> 退回脚本模式整体跑。
   */
  async function executeAll(editorTabId: string) {
    const tab = tabs.value.find((t) => t.id === editorTabId);
    if (!tab) return;
    const content = tab.content.trim();
    if (!content) return;
    const stripped = stripJsCommentsInline(content).trim();
    if (!stripped) return;

    const isShowDbs = /^show\s+(dbs|databases)\b/.test(stripped);
    if (!tab.database && !isShowDbs) {
      const rt = spawnResultTab(tab, "find", content);
      if (rt) {
        rt.error = "请先选择数据库 (工具栏右侧的数据库下拉)";
        rt.loading = false;
      }
      return;
    }

    // read-then-write 脚本: 同样必须整段走 script mode
    if (needsPreEvaluation(tab.content) && hasDbReadInVar(tab.content)) {
      await executeScript(tab, content);
      return;
    }

    const statements = extractDbStatements(content);
    if (statements.length === 0) {
      if (needsPreEvaluation(tab.content)) {
        await executeScript(tab, content);
      } else {
        // 单条普通语句 (整段就是一条 db.xxx 或 use)
        await executeQuery(editorTabId, content);
      }
      return;
    }
    // 逐条顺序执行, 每条一个结果 tab
    for (const stmt of statements) {
      await executeQuery(editorTabId, stmt);
    }
  }

  /**
   * 用一个**外部**已经拿到的单文档结果填充一个新 result tab.
   * 不走 run_query 执行器, 用于 "查看索引" 等场景: 后端复合命令直接返回一个 doc.
   */
  function pushExternalResult(
    editorTabId: string,
    queryText: string,
    doc: Record<string, unknown>,
  ) {
    const tab = tabs.value.find((t) => t.id === editorTabId);
    if (!tab) return;
    const rt = spawnResultTab(tab, "find", queryText);
    if (!rt) return;
    rt.result = {
      documents: [doc],
      count: 1,
      totalCount: 1,
      executionTimeMs: 0,
    };
    rt.loading = false;
  }

  /** Explain —— 每次追加一个 Explain 结果 tab */
  async function executeExplain(editorTabId: string, queryText?: string) {
    const tab = tabs.value.find((t) => t.id === editorTabId);
    if (!tab) return;
    const text = (queryText ?? tab.content).trim();
    if (!text) return;
    const stripped = stripJsCommentsInline(text).trim();
    if (!stripped) return;

    if (!tab.database) {
      const rt = spawnResultTab(tab, "explain", text);
      if (rt) {
        rt.error = "请先选择数据库 (工具栏右侧的数据库下拉)";
        rt.loading = false;
      }
      return;
    }

    const resolved = await resolveQueryText(tab.content, text);

    const rt = spawnResultTab(tab, "explain", resolved);
    if (!rt) return;

    try {
      const res = await invoke<Record<string, unknown>>("explain_shell_query", {
        connectionId: tab.connectionId,
        database: tab.database,
        queryText: resolved,
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
    pendingEdits,
    proposeEdit,
    acceptEdit,
    rejectEdit,
    selectionByTab,
    setSelection,
    getSelection,
    createTab,
    closeTab,
    setContent,
    executeQuery,
    executeAll,
    executeExplain,
    appendConsole,
    pushExternalResult,
    setTabExecutor,
    setTabSkipLint,
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

// Vite HMR: 让 editor store 改动能干净热替换, 不再触发整页 reload / 旧代码残留
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useEditorStore, import.meta.hot));
}

<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from "vue";
import {
  NLayout, NLayoutSider, NLayoutContent, NMessageProvider, NSplit, NModal, NIcon,
  NInput, NTree, NButton, useMessage,
} from "naive-ui";
import { h, type VNodeChild } from "vue";
import {
  Folder as FolderIcon,
  FolderOpen as FolderOpenIcon,
} from "@vicons/ionicons5";
import type { TreeOption } from "naive-ui";
import {
  DocumentTextOutline as ScriptIcon,
  SparklesOutline as AiIcon,
} from "@vicons/ionicons5";
import ImportDialog from "@/components/result/ImportDialog.vue";
import ExportDialog from "@/components/result/ExportDialog.vue";
import ConnectionManagerDialog from "@/components/connection/ConnectionManager.vue";
import MenuBar from "@/components/layout/MenuBar.vue";
import StatusBar from "@/components/layout/StatusBar.vue";
import ServerTree from "@/components/layout/ServerTree.vue";
import ScriptTree from "@/components/script/ScriptTree.vue";
import type { ScriptInfo } from "@/types/script";
import { useScriptStore } from "@/stores/script";
import { renderSnippet } from "@/utils/mongo-snippets";
import ConnectionDialog from "@/components/connection/ConnectionDialog.vue";
import EditorTabs from "@/components/editor/EditorTabs.vue";
import MonacoEditor from "@/components/editor/MonacoEditor.vue";
import QueryToolbar from "@/components/editor/QueryToolbar.vue";
import VisualQueryBuilder from "@/components/editor/VisualQueryBuilder.vue";
import ResultPanel from "@/components/result/ResultPanel.vue";
import ResultTabsBar from "@/components/result/ResultTabsBar.vue";
import QueryHistory from "@/components/editor/QueryHistory.vue";
import AiChatPanel from "@/components/ai/AiChatPanel.vue";
import AiSettings from "@/components/ai/AiSettings.vue";
import ServerMonitor from "@/components/server/ServerMonitor.vue";
import UserPanel from "@/components/server/UserPanel.vue";
import ProfilerPanel from "@/components/server/ProfilerPanel.vue";
import { useConnectionStore } from "@/stores/connection";
import { useEditorStore } from "@/stores/editor";
import { useDatabaseStore } from "@/stores/database";
import { useAiStore } from "@/stores/ai";
import { createDefaultConnection } from "@/types/connection";
import type { ConnectionConfig } from "@/types/connection";
import type { EditorTab } from "@/types/database";

const connStore = useConnectionStore();
const editorStore = useEditorStore();
const dbStore = useDatabaseStore();
const scriptStore = useScriptStore();
const aiStore = useAiStore();

const showDialog = ref(false);
const editingConfig = ref<ConnectionConfig>(createDefaultConnection());
const showHistory = ref(false);
const showAiSettings = ref(false);
const showServerMonitor = ref(false);
const showUserPanel = ref(false);
const showProfiler = ref(false);
const sidebarCollapsed = ref(false);

/** 右侧活动栏: 当前打开的工具窗口. null=全部关闭, "script"=脚本树, "ai"=AI 助手 */
const activeRightTool = ref<"script" | "ai" | null>(null);
function toggleRightTool(tool: "script" | "ai") {
  activeRightTool.value = activeRightTool.value === tool ? null : tool;
}
const rightPanelWidth = computed(() => (activeRightTool.value === "ai" ? 360 : 300));
/** 已经打开过的 script id -> editor tab id, 防止同一脚本被重复开 */
const scriptTabMap = new Map<string, string>();

function handleOpenScript(script: ScriptInfo) {
  // 已经有打开的 tab 就激活, 否则新开
  const existingId = scriptTabMap.get(script.id);
  if (existingId && editorStore.tabs.find((t) => t.id === existingId)) {
    editorStore.activeTabId = existingId;
    return;
  }
  const connId = script.connectionId ?? connStore.connections[0]?.id ?? "";
  const dbName = script.databaseName ?? "";
  const tabId = editorStore.createTab(connId, dbName);
  editorStore.setContent(tabId, script.content);
  // 把脚本元数据塞进 tab title (方便保存时识别)
  const tab = editorStore.tabs.find((t) => t.id === tabId);
  if (tab) tab.title = `📜 ${script.name}`;
  scriptTabMap.set(script.id, tabId);
}

/** 切换当前 tab 的连接, 同时清掉旧数据库 (新连接不一定有同名库) */
async function handleSwitchTabConn(newConnId: string) {
  const tab = editorStore.activeTab;
  if (!tab || tab.connectionId === newConnId) return;
  tab.connectionId = newConnId;
  tab.database = "";
  tab.collection = "";
  // 拉新连接的库列表 (没拉过的情况下)
  try {
    await dbStore.fetchDatabases(newConnId);
  } catch {
    /* 忽略, 用户后续手动选 */
  }
}

/** 切换当前 tab 的数据库 */
function handleSwitchTabDb(newDb: string) {
  const tab = editorStore.activeTab;
  if (!tab || tab.database === newDb) return;
  tab.database = newDb;
  tab.collection = "";
}

/** 反查: editor tab id -> 已绑定的脚本 id (scriptTabMap 是 scriptId -> tabId) */
function findScriptIdByTab(tabId: string): string | null {
  for (const [sid, tid] of scriptTabMap) {
    if (tid === tabId) return sid;
  }
  return null;
}

// ---- 保存为脚本 (另存) 对话框状态 ----
const showSaveScriptDialog = ref(false);
const saveScriptName = ref("");
/** 选中的目录路径; "" = 根目录 */
const saveScriptFolder = ref<string>("");
const savingScript = ref(false);
const folderTreeExpandedKeys = ref<string[]>([""]);

interface FolderTreeNode extends TreeOption {
  key: string;
  label: string;
  children?: FolderTreeNode[];
  isLeaf?: boolean;
  prefix?: () => VNodeChild;
}

/** 文件夹树: 显式 folder + 从脚本路径里隐式推导的层级, 顶层包一层 "(根目录)" */
const folderTree = computed<FolderTreeNode[]>(() => {
  const allPaths = new Set<string>();
  for (const f of scriptStore.folders) allPaths.add(f.path);
  for (const s of scriptStore.scripts) {
    if (!s.folderPath) continue;
    const parts = s.folderPath.split("/");
    for (let i = 1; i <= parts.length; i++) {
      allPaths.add(parts.slice(0, i).join("/"));
    }
  }
  // 按路径升序保证父先于子, 然后挂到对应父节点
  const sorted = [...allPaths].sort();
  const nodeMap = new Map<string, FolderTreeNode>();
  const roots: FolderTreeNode[] = [];
  const folderPrefix = (path: string) => () =>
    h(
      NIcon,
      { size: 14, color: "#e8a838" },
      {
        default: () =>
          h(folderTreeExpandedKeys.value.includes(path) ? FolderOpenIcon : FolderIcon),
      },
    );
  for (const path of sorted) {
    const slashIdx = path.lastIndexOf("/");
    const name = slashIdx === -1 ? path : path.slice(slashIdx + 1);
    const parent = slashIdx === -1 ? "" : path.slice(0, slashIdx);
    const node: FolderTreeNode = {
      key: path,
      label: name,
      children: [],
      isLeaf: false,
      prefix: folderPrefix(path),
    };
    nodeMap.set(path, node);
    if (parent === "") roots.push(node);
    else nodeMap.get(parent)?.children?.push(node);
  }
  // 把所有顶层目录包在 "(根目录)" 下, 让用户也能选根目录本身
  return [
    {
      key: "",
      label: "(根目录)",
      children: roots,
      isLeaf: false,
      prefix: folderPrefix(""),
    },
  ];
});

/**
 * "保存为脚本" 按钮入口:
 *   - 已经绑定到某个保存过的脚本 -> 静默更新内容 (用原 name / folderPath / id)
 *   - 还没绑定 -> 弹窗问名字 + 目录, 另存为新脚本
 */
async function handleSaveAsScript() {
  const tab = editorStore.activeTab;
  if (!tab) return;

  const existingId = findScriptIdByTab(tab.id);
  const existing = existingId
    ? scriptStore.scripts.find((s) => s.id === existingId) ?? null
    : null;

  if (existing) {
    // 已绑定 -> 静默更新, 不弹窗
    try {
      const saved = await scriptStore.save({
        id: existing.id,
        name: existing.name,
        folderPath: existing.folderPath,
        content: tab.content,
        connectionId: tab.connectionId || null,
        databaseName: tab.database || null,
      });
      scriptTabMap.set(saved.id, tab.id);
      tab.title = `📜 ${saved.name}`;
      msg.success(`已更新脚本 ${saved.name}`);
    } catch (e) {
      msg.error(`保存失败: ${e}`);
    }
    return;
  }

  // 未绑定 -> 打开对话框选名字 + 目录
  saveScriptName.value = tab.title.replace(/^📜\s*/, "").trim() || "未命名脚本";
  saveScriptFolder.value = "";
  folderTreeExpandedKeys.value = [""];
  showSaveScriptDialog.value = true;
}

/** 在树里"新建子文件夹": 在当前选中目录下建 leafName, 并自动选中新建的路径 */
async function promptCreateFolderInDialog() {
  const parent = saveScriptFolder.value;
  // eslint-disable-next-line no-alert
  const input = window.prompt(parent ? `在 "${parent}" 下新建文件夹:` : "在根目录下新建文件夹:");
  if (!input || !input.trim()) return;
  const name = input.trim();
  if (name.includes("/")) {
    msg.warning("名称中不能包含 /");
    return;
  }
  const path = parent ? `${parent}/${name}` : name;
  try {
    await scriptStore.createFolder(path);
    saveScriptFolder.value = path;
    // 展开父链, 让新建的目录可见
    const expand = new Set(folderTreeExpandedKeys.value);
    expand.add("");
    const parts = path.split("/");
    for (let i = 0; i < parts.length; i++) {
      expand.add(parts.slice(0, i + 1).join("/"));
    }
    folderTreeExpandedKeys.value = [...expand];
  } catch (e) {
    msg.error(`创建失败: ${e}`);
  }
}

async function confirmSaveScript() {
  const tab = editorStore.activeTab;
  if (!tab) {
    showSaveScriptDialog.value = false;
    return;
  }
  const name = saveScriptName.value.trim();
  if (!name) {
    msg.warning("请输入脚本名称");
    return;
  }
  // 规范化路径: 去掉首尾 /, 折叠多余 /
  const folder = saveScriptFolder.value
    .trim()
    .replace(/^\/+|\/+$/g, "")
    .replace(/\/{2,}/g, "/");

  savingScript.value = true;
  try {
    // 新路径先建文件夹 (后端接受逐级 create; 已存在静默忽略)
    if (folder && !scriptStore.folders.some((f) => f.path === folder)) {
      try { await scriptStore.createFolder(folder); } catch { /* 已存在或无权 */ }
    }
    const saved = await scriptStore.save({
      name,
      folderPath: folder,
      content: tab.content,
      connectionId: tab.connectionId || null,
      databaseName: tab.database || null,
    });
    scriptTabMap.set(saved.id, tab.id);
    tab.title = `📜 ${saved.name}`;
    msg.success(`已保存脚本 ${saved.name}${folder ? ` -> ${folder}` : ""}`);
    activeRightTool.value = "script";
    showSaveScriptDialog.value = false;
  } catch (e) {
    msg.error(`保存失败: ${e}`);
  } finally {
    savingScript.value = false;
  }
}
const showQueryBuilder = ref(false);
const queryBuilderInitialText = ref("");

const activeTab = computed(() => editorStore.activeTab);

/** 模板里取指定 editor tab 当前激活的结果 tab */
function activeResultOf(tab: EditorTab) {
  return tab.resultTabs.find((r) => r.id === tab.activeResultTabId) ?? null;
}

/** JSON Viewer 点 Edit in new tab → 开一个新编辑器 tab, 放入 updateOne 脚本 */
function handleEditInNewTab(sourceTab: EditorTab, queryText: string) {
  const newTabId = editorStore.createTab(
    sourceTab.connectionId,
    sourceTab.database,
    sourceTab.collection,
  );
  editorStore.setContent(newTabId, queryText);
}

/** 工具栏 Query 按钮 → 打开 Visual Query Builder; 若编辑器有内容则作为初始值回填 */
function handleOpenQueryBuilder() {
  const tab = editorStore.activeTab;
  if (!tab) {
    msg.warning("请先打开一个查询页");
    return;
  }
  queryBuilderInitialText.value = (tab.content || "").trim();
  showQueryBuilder.value = true;
}

/** Visual Query Builder 提交 (Run) —— 写回编辑器并立即执行 */
function handleQueryBuilderRun(queryText: string) {
  const tab = editorStore.activeTab;
  if (!tab) return;
  editorStore.setContent(tab.id, queryText);
  editorStore.executeQuery(tab.id, queryText);
}

/** Visual Query Builder 提交 (Insert 不执行) —— 仅写回编辑器 */
function handleQueryBuilderInsert(queryText: string) {
  const tab = editorStore.activeTab;
  if (!tab) return;
  editorStore.setContent(tab.id, queryText);
}

// 连接操作
function handleCreateConnection() {
  editingConfig.value = createDefaultConnection();
  showDialog.value = true;
}

function handleEditConnection(config: ConnectionConfig) {
  editingConfig.value = { ...config };
  showDialog.value = true;
}

async function handleConnect(config: ConnectionConfig) {
  const loading = msg.loading("正在连接...", { duration: 0 });
  try {
    await connStore.connect(config);
    await dbStore.fetchDatabases(config.id);
    loading.destroy();
    msg.success("连接成功");
  } catch (e) {
    loading.destroy();
    msg.error(`连接失败: ${e}`, { duration: 5000 });
  }
}

async function handleDisconnect(id: string) {
  await connStore.disconnectConn(id);
}

async function handleDeleteConnection(id: string) {
  await connStore.remove(id);
}

// 查询操作
const runTrigger = ref(0);
const formatTrigger = ref(0);

/** 工具栏 Run 按钮 → 递增 trigger → MonacoEditor 内部读取选区并 emit run */
function handleRunQuery() {
  runTrigger.value++;
}

/** F5 默认是浏览器刷新整页 (在 webview 里会丢掉所有 tab / 结果), 拦下来改成执行当前编辑器选中或当前语句 */
function onWindowKeyDown(e: KeyboardEvent) {
  if (e.key === "F5" && !e.ctrlKey && !e.shiftKey && !e.altKey) {
    e.preventDefault();
    if (editorStore.activeTab) handleRunQuery();
  }
}
onMounted(() => window.addEventListener("keydown", onWindowKeyDown));
onBeforeUnmount(() => window.removeEventListener("keydown", onWindowKeyDown));

/** MonacoEditor emit run → 携带正确的语句文本 → 执行 */
function handleEditorRun(statement: string) {
  if (editorStore.activeTab) {
    editorStore.executeQuery(editorStore.activeTab.id, statement);
  }
}

/** 运行全部语句 (工具栏 Run All / Ctrl+Shift+Enter) */
function handleRunAll() {
  if (editorStore.activeTab) {
    editorStore.executeAll(editorStore.activeTab.id);
  }
}

/** 工具栏 Format 按钮 → 递增 trigger → MonacoEditor 内部格式化 */
function handleFormat() {
  formatTrigger.value++;
}

/** 插入代码片段 → 渲染 ${COLL} 占位后追加到当前编辑器 tab 末尾 */
function handleInsertSnippet(body: string) {
  const tab = editorStore.activeTab;
  if (!tab) {
    msg.warning("请先打开一个查询页");
    return;
  }
  const rendered = renderSnippet(body, tab.collection);
  const cur = tab.content.trim();
  editorStore.setContent(tab.id, cur ? `${cur}\n\n${rendered}` : rendered);
}

/** 编辑器右键 "AI 分析选中代码" → 打开 AI 面板并让 agent 分析选区 */
function handleAiAnalyze() {
  activeRightTool.value = "ai";
  aiStore
    .runAgent("请分析我在编辑器里选中的这段代码:解释它在做什么、有没有问题或可以改进的地方。")
    .catch(() => {
      /* 错误已由 store 追加到对话里 */
    });
}

/** 工具栏 Explain 按钮 → 直接用当前 tab 的整段内容跑 explain */
function handleExplain() {
  const tab = editorStore.activeTab;
  if (!tab) {
    msg.warning("请先打开一个查询页");
    return;
  }
  const text = (tab.content || "").trim();
  if (!text) {
    msg.warning("编辑器内容为空");
    return;
  }
  editorStore.executeExplain(tab.id, text);
}

function handleHistorySelect(queryText: string) {
  if (editorStore.activeTab) {
    editorStore.setContent(editorStore.activeTab.id, queryText);
  }
}

const msg = useMessage();

// ---- 导入/导出 ----
const showImportDialog = ref(false);
const showExportDialog = ref(false);
const showConnManager = ref(false);

function handleImport() {
  const tab = editorStore.activeTab;
  if (!tab) {
    msg.warning("请先打开一个查询页");
    return;
  }
  showImportDialog.value = true;
}

function handleImported(count: number) {
  msg.success(`导入成功，共 ${count} 条文档`);
  const tab = editorStore.activeTab;
  if (!tab) return;
  const active = editorStore.activeResultTab;
  const replayText = active?.queryText || tab.content;
  editorStore.executeQuery(tab.id, replayText);
}

function handleExport() {
  const tab = editorStore.activeTab;
  const active = editorStore.activeResultTab;
  if (!tab || !active?.result || active.result.documents.length === 0) {
    msg.warning("没有可导出的数据，请先执行查询");
    return;
  }
  showExportDialog.value = true;
}

function handleExported(count: number) {
  msg.success(`导出完成，共 ${count.toLocaleString()} 条`);
}

// 从 ServerTree 右键触发的导入/导出（指定集合）
const importCollCtx = ref<{ connId: string; db: string; coll: string } | null>(null);
const exportCollCtx = ref<{ connId: string; db: string; coll: string } | null>(null);

function handleImportColl(connId: string, db: string, coll: string) {
  importCollCtx.value = { connId, db, coll };
  showImportDialog.value = true;
}

function handleExportColl(connId: string, db: string, coll: string) {
  // 先打开查询 tab 执行查询，然后打开导出
  const tabId = editorStore.createTab(connId, db, coll);
  const collRef = coll.includes(".") ? `db.getCollection("${coll}")` : `db.${coll}`;
  const query = `${collRef}.find({})`;
  editorStore.setContent(tabId, query);
  editorStore.executeQuery(tabId).then(() => {
    exportCollCtx.value = { connId, db, coll };
    showExportDialog.value = true;
  });
}

// 菜单栏
function handleMenuAction(key: string) {
  switch (key) {
    case "file.new-connection":
      handleCreateConnection();
      break;
    case "file.conn-manager":
      showConnManager.value = true;
      break;
    case "file.new-query":
      // 如果有活跃连接，打开新查询 tab
      if (activeTab.value) {
        editorStore.createTab(activeTab.value.connectionId, activeTab.value.database, "Query");
      }
      break;
    case "view.sidebar":
      sidebarCollapsed.value = !sidebarCollapsed.value;
      break;
    case "view.script-panel":
      toggleRightTool("script");
      break;
    case "view.ai-panel":
      toggleRightTool("ai");
      break;
    case "run.execute":
      handleRunQuery();
      break;
    case "run.explain":
      handleExplain();
      break;
    case "tools.ai":
      activeRightTool.value = "ai";
      break;
    case "tools.ai-settings":
      showAiSettings.value = true;
      break;
    case "tools.monitor":
      showServerMonitor.value = true;
      break;
    case "tools.users":
      showUserPanel.value = true;
      break;
    case "tools.profiler":
      showProfiler.value = true;
      break;
    case "file.import":
      handleImport();
      break;
    case "file.export":
      handleExport();
      break;
  }
}


</script>

<template>
  <n-message-provider>
    <div class="app-root">
      <MenuBar @action="handleMenuAction" />

      <div class="app-body">
        <n-layout has-sider style="height: 100%; flex: 1; min-width: 0">
          <n-layout-sider
            bordered
            :width="280"
            :collapsed-width="0"
            :collapsed="sidebarCollapsed"
            show-trigger
            collapse-mode="width"
            @update:collapsed="sidebarCollapsed = $event"
          >
            <div class="sidebar-content">
              <ServerTree
                @create-connection="handleCreateConnection"
                @edit-connection="handleEditConnection"
                @connect-server="handleConnect"
                @disconnect-server="handleDisconnect"
                @delete-connection="handleDeleteConnection"
                @import-coll="handleImportColl"
                @export-coll="handleExportColl"
              />
            </div>
          </n-layout-sider>

          <n-layout has-sider>
            <n-layout-content style="height: 100%">
              <div class="main-content">
                <EditorTabs>
                  <template #default="{ tab }">
                    <div class="editor-area">
                      <QueryToolbar
                        :loading="activeResultOf(tab)?.loading ?? false"
                        :execution-time="activeResultOf(tab)?.result?.executionTimeMs"
                        :result-count="activeResultOf(tab)?.result?.count"
                        :connection-id="tab.connectionId"
                        :database="tab.database"
                        :collection="tab.collection"
                        :error="activeResultOf(tab)?.error ?? null"
                        @run="handleRunQuery"
                        @run-all="handleRunAll"
                        @stop="() => {
                          const rt = activeResultOf(tab);
                          if (rt) editorStore.stopResultTab(tab.id, rt.id);
                        }"
                        @explain="handleExplain"
                        @query-builder="handleOpenQueryBuilder"
                        @format="handleFormat"
                        @insert-snippet="handleInsertSnippet"
                        @import="handleImport"
                        @export="handleExport"
                        @history="showHistory = true"
                        @save-script="handleSaveAsScript"
                        @update:connection-id="handleSwitchTabConn"
                        @update:database="handleSwitchTabDb"
                      />
                      <n-split direction="vertical" :default-size="0.5" :min="0.2" :max="0.8" style="flex:1;min-height:0">
                        <template #1>
                          <div class="split-pane">
                            <MonacoEditor
                              :model-value="tab.content"
                              :run-trigger="runTrigger"
                              :format-trigger="formatTrigger"
                              :tab-id="tab.id"
                              @update:model-value="editorStore.setContent(tab.id, $event)"
                              @run="handleEditorRun"
                              @run-all="handleRunAll"
                              @ai-analyze="handleAiAnalyze"
                            />
                          </div>
                        </template>
                        <template #2>
                          <div class="split-pane result-pane">
                            <ResultTabsBar
                              v-if="tab.resultTabs.length > 0"
                              :result-tabs="tab.resultTabs"
                              :active-result-tab-id="tab.activeResultTabId"
                              @activate="editorStore.activateResultTab(tab.id, $event)"
                              @close="editorStore.closeResultTab(tab.id, $event)"
                              @close-left="editorStore.closeLeftOfResultTab(tab.id, $event)"
                              @close-right="editorStore.closeRightOfResultTab(tab.id, $event)"
                              @close-others="editorStore.closeOtherResultTabs(tab.id, $event)"
                              @close-all="editorStore.closeAllResultTabs(tab.id)"
                            />
                            <ResultPanel
                              :result-tab="activeResultOf(tab)"
                              :connection-id="tab.connectionId"
                              :database="tab.database"
                              :collection="tab.collection"
                              @edit-in-tab="(p) => handleEditInNewTab(tab, p.queryText)"
                              @page-change="(page, size) => {
                                const rt = activeResultOf(tab);
                                if (rt) editorStore.fetchPage(tab.id, rt.id, page, size);
                              }"
                              @refresh="() => {
                                const rt = activeResultOf(tab);
                                editorStore.executeQuery(tab.id, rt?.queryText || tab.content);
                              }"
                            />
                          </div>
                        </template>
                      </n-split>
                    </div>
                  </template>
                </EditorTabs>
              </div>
            </n-layout-content>

            <n-layout-sider
              v-if="activeRightTool"
              bordered
              :width="rightPanelWidth"
              style="height: 100%"
            >
              <div v-if="activeRightTool === 'script'" class="sidebar-content">
                <ScriptTree @open-script="handleOpenScript" />
              </div>
              <AiChatPanel v-else-if="activeRightTool === 'ai'" />
            </n-layout-sider>
          </n-layout>
        </n-layout>

        <!-- 右侧活动栏: 切换工具窗口 (脚本 / AI) -->
        <div class="activity-bar">
          <button
            class="ab-item"
            :class="{ active: activeRightTool === 'script' }"
            title="脚本管理"
            @click="toggleRightTool('script')"
          >
            <n-icon :size="20"><ScriptIcon /></n-icon>
            <span class="ab-label">脚本</span>
          </button>
          <button
            class="ab-item"
            :class="{ active: activeRightTool === 'ai' }"
            title="AI 助手"
            @click="toggleRightTool('ai')"
          >
            <n-icon :size="20"><AiIcon /></n-icon>
            <span class="ab-label">AI</span>
          </button>
        </div>
      </div>

      <StatusBar />
    </div>

    <ConnectionDialog v-model:show="showDialog" :config="editingConfig" />
    <VisualQueryBuilder
      v-if="activeTab"
      v-model:show="showQueryBuilder"
      :connection-id="activeTab.connectionId"
      :database="activeTab.database"
      :collection="activeTab.collection"
      :initial-query-text="queryBuilderInitialText"
      @run="handleQueryBuilderRun"
      @insert="handleQueryBuilderInsert"
    />
    <QueryHistory
      v-model:show="showHistory"
      @select="handleHistorySelect"
    />

    <!-- 连接管理 -->
    <ConnectionManagerDialog
      v-model:show="showConnManager"
      @connect="handleConnect"
      @edit="(cfg) => { editingConfig = cfg; showDialog = true; }"
      @create="handleCreateConnection"
    />

    <!-- AI 设置 -->
    <AiSettings v-model:show="showAiSettings" />

    <!-- 保存为脚本 (新脚本: 选名字 + 目录) -->
    <n-modal
      v-model:show="showSaveScriptDialog"
      preset="card"
      title="保存为脚本"
      style="width: 440px"
      :bordered="false"
      :mask-closable="!savingScript"
    >
      <div style="display: flex; flex-direction: column; gap: 12px">
        <div>
          <div style="font-size: 12px; color: #666; margin-bottom: 4px">脚本名称</div>
          <n-input
            v-model:value="saveScriptName"
            placeholder="脚本名称"
            autofocus
            @keydown.enter.prevent="confirmSaveScript"
          />
        </div>
        <div>
          <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:4px">
            <span style="font-size:12px;color:#666">
              目标文件夹: <strong>{{ saveScriptFolder || "(根目录)" }}</strong>
            </span>
            <n-button size="tiny" quaternary @click="promptCreateFolderInDialog">
              + 新建文件夹
            </n-button>
          </div>
          <div class="folder-tree-box">
            <n-tree
              :data="folderTree"
              :selected-keys="[saveScriptFolder]"
              :expanded-keys="folderTreeExpandedKeys"
              block-line
              selectable
              @update:selected-keys="(k: string[]) => {
                // 不允许取消选择, 始终保持有一个目标 (默认根)
                saveScriptFolder = k[0] ?? '';
              }"
              @update:expanded-keys="(k: string[]) => (folderTreeExpandedKeys = k)"
            />
          </div>
        </div>
      </div>
      <template #action>
        <div style="display: flex; justify-content: flex-end; gap: 8px">
          <n-button size="small" :disabled="savingScript" @click="showSaveScriptDialog = false">
            取消
          </n-button>
          <n-button type="primary" size="small" :loading="savingScript" @click="confirmSaveScript">
            保存
          </n-button>
        </div>
      </template>
    </n-modal>

    <!-- 导入 -->
    <ImportDialog
      v-if="activeTab || importCollCtx"
      v-model:show="showImportDialog"
      :connection-id="importCollCtx?.connId ?? activeTab?.connectionId ?? ''"
      :database="importCollCtx?.db ?? activeTab?.database ?? ''"
      :collection="importCollCtx?.coll ?? activeTab?.collection ?? ''"
      @imported="(c) => { handleImported(c); importCollCtx = null; }"
    />

    <!-- 导出 -->
    <ExportDialog
      v-if="(activeTab && editorStore.activeResultTab?.result) || exportCollCtx"
      v-model:show="showExportDialog"
      :documents="editorStore.activeResultTab?.result?.documents ?? []"
      :connection-id="exportCollCtx?.connId ?? activeTab?.connectionId ?? ''"
      :database="exportCollCtx?.db ?? activeTab?.database ?? ''"
      :collection="exportCollCtx?.coll ?? activeTab?.collection ?? ''"
      :query-text="activeTab?.content ?? ''"
      @exported="(c) => { handleExported(c); exportCollCtx = null; }"
    />

    <!-- Server Monitor -->
    <n-modal v-model:show="showServerMonitor" preset="card" title="Server Monitor" style="width:800px;max-height:80vh" :bordered="false">
      <ServerMonitor v-if="showServerMonitor && activeTab" :connection-id="activeTab.connectionId" />
      <div v-else style="color:#999;text-align:center;padding:24px">请先连接到 MongoDB 服务器</div>
    </n-modal>

    <!-- User Management -->
    <n-modal v-model:show="showUserPanel" preset="card" title="User Management" style="width:800px;max-height:80vh" :bordered="false">
      <UserPanel v-if="showUserPanel && activeTab" :connection-id="activeTab.connectionId" :database="activeTab.database" />
      <div v-else style="color:#999;text-align:center;padding:24px">请先连接到 MongoDB 服务器</div>
    </n-modal>

    <!-- Profiler -->
    <n-modal v-model:show="showProfiler" preset="card" title="Profiler" style="width:800px;max-height:80vh" :bordered="false">
      <ProfilerPanel v-if="showProfiler && activeTab" :connection-id="activeTab.connectionId" :database="activeTab.database" />
      <div v-else style="color:#999;text-align:center;padding:24px">请先连接到 MongoDB 服务器</div>
    </n-modal>
  </n-message-provider>
</template>

<style scoped>
.app-root {
  height: 100vh;
  display: flex;
  flex-direction: column;
}
.app-body {
  flex: 1;
  overflow: hidden;
  display: flex;
}
.activity-bar {
  width: 48px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  padding-top: 6px;
  background: #f3f3f3;
  border-left: 1px solid #e0e0e0;
}
.ab-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 3px;
  padding: 8px 0 7px;
  border: none;
  border-left: 2px solid transparent;
  background: transparent;
  color: #888;
  font-size: 10px;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.ab-item:hover {
  background: rgba(0, 0, 0, 0.05);
  color: #555;
}
.ab-item.active {
  color: #2080f0;
  border-left-color: #2080f0;
  background: #fff;
}
.ab-label {
  line-height: 1;
}
.sidebar-content {
  height: 100%;
  overflow-y: auto;
}
.main-content {
  height: 100%;
  display: flex;
  flex-direction: column;
}
.editor-area {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: #fff;
}
.split-pane {
  height: 100%;
  overflow: hidden;
}
.split-pane.result-pane {
  display: flex;
  flex-direction: column;
}
.folder-tree-box {
  max-height: 260px;
  overflow: auto;
  border: 1px solid #e0e0e0;
  border-radius: 4px;
  padding: 4px 6px;
  background: #fafafa;
}
</style>

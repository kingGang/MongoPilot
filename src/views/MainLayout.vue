<script setup lang="ts">
import { ref, computed } from "vue";
import {
  NLayout, NLayoutSider, NLayoutContent, NMessageProvider, NSplit, NModal, useMessage,
} from "naive-ui";
import ImportDialog from "@/components/result/ImportDialog.vue";
import ExportDialog from "@/components/result/ExportDialog.vue";
import ConnectionManagerDialog from "@/components/connection/ConnectionManager.vue";
import MenuBar from "@/components/layout/MenuBar.vue";
import StatusBar from "@/components/layout/StatusBar.vue";
import ServerTree from "@/components/layout/ServerTree.vue";
import ConnectionDialog from "@/components/connection/ConnectionDialog.vue";
import EditorTabs from "@/components/editor/EditorTabs.vue";
import MonacoEditor from "@/components/editor/MonacoEditor.vue";
import QueryToolbar from "@/components/editor/QueryToolbar.vue";
import ResultPanel from "@/components/result/ResultPanel.vue";
import QueryHistory from "@/components/editor/QueryHistory.vue";
import AiChatPanel from "@/components/ai/AiChatPanel.vue";
import AiSettings from "@/components/ai/AiSettings.vue";
import ServerMonitor from "@/components/server/ServerMonitor.vue";
import UserPanel from "@/components/server/UserPanel.vue";
import ProfilerPanel from "@/components/server/ProfilerPanel.vue";
import { useConnectionStore } from "@/stores/connection";
import { useEditorStore } from "@/stores/editor";
import { useDatabaseStore } from "@/stores/database";
import { createDefaultConnection } from "@/types/connection";
import type { ConnectionConfig } from "@/types/connection";

const connStore = useConnectionStore();
const editorStore = useEditorStore();
const dbStore = useDatabaseStore();

const showDialog = ref(false);
const editingConfig = ref<ConnectionConfig>(createDefaultConnection());
const showHistory = ref(false);
const showAiPanel = ref(false);
const showAiSettings = ref(false);
const showServerMonitor = ref(false);
const showUserPanel = ref(false);
const showProfiler = ref(false);
const sidebarCollapsed = ref(false);

const activeTab = computed(() => editorStore.activeTab);
const aiConnectionId = computed(() => activeTab.value?.connectionId ?? undefined);
const aiDatabase = computed(() => activeTab.value?.database ?? undefined);

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

/** 工具栏 Run 按钮 → 递增 trigger → MonacoEditor 内部读取选区并 emit run */
function handleRunQuery() {
  runTrigger.value++;
}

/** MonacoEditor emit run → 携带正确的语句文本 → 执行 */
function handleEditorRun(statement: string) {
  if (editorStore.activeTab) {
    editorStore.executeQuery(editorStore.activeTab.id, statement);
  }
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
  if (tab) editorStore.executeQuery(tab.id, tab.lastQueryText || tab.content);
}

function handleExport() {
  const tab = editorStore.activeTab;
  if (!tab || !tab.result || tab.result.documents.length === 0) {
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

function handleAiExecuteQuery(query: string) {
  if (editorStore.activeTab) {
    editorStore.setContent(editorStore.activeTab.id, query);
    editorStore.executeQuery(editorStore.activeTab.id);
  }
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
    case "view.ai-panel":
      showAiPanel.value = !showAiPanel.value;
      break;
    case "run.execute":
      handleRunQuery();
      break;
    case "tools.ai":
      showAiPanel.value = true;
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

// 查询历史需要知道当前活跃连接
const historyConnectionId = computed(() => activeTab.value?.connectionId ?? null);
</script>

<template>
  <n-message-provider>
    <div class="app-root">
      <MenuBar @action="handleMenuAction" />

      <div class="app-body">
        <n-layout has-sider style="height: 100%">
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
                        :loading="tab.loading"
                        :execution-time="tab.result?.executionTimeMs"
                        :result-count="tab.result?.count"
                        :connection-id="tab.connectionId"
                        :database="tab.database"
                        :collection="tab.collection"
                        :error="tab.error"
                        @run="handleRunQuery"
                        @import="handleImport"
                        @export="handleExport"
                        @history="showHistory = true"
                      />
                      <n-split direction="vertical" :default-size="0.5" :min="0.2" :max="0.8" style="flex:1;min-height:0">
                        <template #1>
                          <div class="split-pane">
                            <MonacoEditor
                              :model-value="tab.content"
                              :run-trigger="runTrigger"
                              @update:model-value="editorStore.setContent(tab.id, $event)"
                              @run="handleEditorRun"
                            />
                          </div>
                        </template>
                        <template #2>
                          <div class="split-pane">
                            <ResultPanel
                              :result="tab.result"
                              :error="tab.error"
                              :loading="tab.loading"
                              :connection-id="tab.connectionId"
                              :database="tab.database"
                              :collection="tab.collection"
                              :query-text="tab.content"
                              :current-page="tab.currentPage"
                              :page-size="tab.pageSize"
                              @page-change="(page, size) => editorStore.fetchPage(tab.id, page, size)"
                              @refresh="editorStore.executeQuery(tab.id, tab.lastQueryText)"
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
              v-if="showAiPanel"
              bordered
              :width="340"
              style="height: 100%"
            >
              <AiChatPanel
                :connection-id="aiConnectionId"
                :database="aiDatabase"
                @execute-query="handleAiExecuteQuery"
              />
            </n-layout-sider>
          </n-layout>
        </n-layout>
      </div>

      <StatusBar />
    </div>

    <ConnectionDialog v-model:show="showDialog" :config="editingConfig" />
    <QueryHistory
      v-if="historyConnectionId"
      v-model:show="showHistory"
      :connection-id="historyConnectionId"
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
      v-if="(activeTab && activeTab.result) || exportCollCtx"
      v-model:show="showExportDialog"
      :documents="activeTab?.result?.documents ?? []"
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
</style>

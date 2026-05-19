<script setup lang="ts">
import { ref, computed, onErrorCaptured, onMounted, onBeforeUnmount, watch, h } from "vue";
import { NEmpty, NAlert, NButton, NButtonGroup, NModal, NInput, useMessage, useDialog } from "naive-ui";
import ResultToolbar from "./ResultToolbar.vue";
import TreeDocView from "./TreeDocView.vue";
import TableView from "./TableView.vue";
import JsonTreeView from "./JsonTreeView.vue";
import ExplainView from "./ExplainView.vue";
import ExportDialog from "./ExportDialog.vue";
import type { ResultTab } from "@/types/database";
import * as docApi from "@/api/document";

const props = defineProps<{
  resultTab: ResultTab | null;
  connectionId: string;
  database: string;
  collection: string;
}>();

const emit = defineEmits<{
  refresh: [];
  pageChange: [page: number, pageSize: number];
  editInTab: [payload: { doc: Record<string, unknown>; queryText: string }];
}>();

const viewMode = ref<"tree" | "table" | "json">("tree");
const renderError = ref<string | null>(null);

// 从 resultTab 派生所有字段
const result = computed(() => props.resultTab?.result ?? null);
const explainResult = computed(() => props.resultTab?.explainResult ?? null);
const isConsole = computed(() => props.resultTab?.kind === "console");
const consoleLines = computed(() => props.resultTab?.consoleLines ?? []);
const error = computed(() => props.resultTab?.error ?? null);
const loading = computed(() => props.resultTab?.loading ?? false);
const queryText = computed(() => props.resultTab?.queryText ?? "");
const currentPage = computed(() => props.resultTab?.currentPage ?? 1);
const pageSize = computed(() => props.resultTab?.pageSize ?? 50);

const totalCount = computed(() => result.value?.totalCount ?? result.value?.count ?? 0);
const returnedCount = computed(() => result.value?.documents.length ?? 0);
const rowOffset = computed(() => (currentPage.value - 1) * pageSize.value);

// ---- 结果搜索 (Excel 风格: 高亮 + 上下导航, 不过滤行) ----
const showSearchBar = ref(false);
const searchKeyword = ref("");
const matchCase = ref(false);
const currentMatchIdx = ref(0);

/** 匹配到关键字的文档下标列表 —— 顺序即 prev/next 的遍历顺序 */
const matchDocIndexes = computed<number[]>(() => {
  const kw = searchKeyword.value;
  if (!kw) return [];
  const docs = result.value?.documents ?? [];
  const needle = matchCase.value ? kw : kw.toLowerCase();
  const hits: number[] = [];
  for (let i = 0; i < docs.length; i++) {
    const hay = matchCase.value
      ? JSON.stringify(docs[i])
      : JSON.stringify(docs[i]).toLowerCase();
    if (hay.includes(needle)) hits.push(i);
  }
  return hits;
});

/** 当前高亮的"当前匹配"文档下标 */
const activeMatchDocIndex = computed<number>(() => {
  const list = matchDocIndexes.value;
  if (list.length === 0) return -1;
  const idx = Math.min(Math.max(0, currentMatchIdx.value), list.length - 1);
  return list[idx];
});

function gotoNextMatch() {
  const n = matchDocIndexes.value.length;
  if (n === 0) return;
  currentMatchIdx.value = (currentMatchIdx.value + 1) % n;
}
function gotoPrevMatch() {
  const n = matchDocIndexes.value.length;
  if (n === 0) return;
  currentMatchIdx.value = (currentMatchIdx.value - 1 + n) % n;
}

// 关键字变化时重置当前匹配游标
watch(searchKeyword, () => { currentMatchIdx.value = 0; });
watch(matchCase, () => { currentMatchIdx.value = 0; });

// 展示文档仍然是整页 (Excel 不隐藏不匹配的行, 只做高亮 + 跳转)
const pagedDocuments = computed(() => result.value?.documents ?? []);

// ---- 多选状态 ----
/** 从文档里取出稳定的选择 key (ObjectId / 字面量 / 其他对象的 JSON) */
function docSelectionKey(doc: Record<string, unknown>): string | null {
  const id = doc._id;
  if (id === undefined || id === null) return null;
  if (typeof id === "object") {
    const obj = id as Record<string, unknown>;
    if (typeof obj.$oid === "string") return `oid:${obj.$oid}`;
    try { return `json:${JSON.stringify(id)}`; } catch { return null; }
  }
  return `lit:${String(id)}`;
}

const selectedKeys = ref<Set<string>>(new Set());

/** 修改过的字段标识: 集合元素为 `${docSelectionKey}::${fieldName}`.
 *  翻页 / 切结果 tab / 重新查询都会清空, 视觉上提示用户"这条数据本会话被改过". */
const dirtyFields = ref<Set<string>>(new Set());
function markDirty(docKey: string, field: string) {
  if (!docKey || !field) return;
  const next = new Set(dirtyFields.value);
  next.add(`${docKey}::${field}`);
  dirtyFields.value = next;
}

function toggleSelect(key: string) {
  const next = new Set(selectedKeys.value);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  selectedKeys.value = next;
}

function setSelectedKeys(keys: string[]) {
  selectedKeys.value = new Set(keys);
}

/** 转发子组件的 editInTab 事件 —— 写在 script 里避免 template 里内联泛型 (<) 被 vue-tsc 误读为 HTML 标签 */
function forwardEditInTab(payload: { doc: Record<string, unknown>; queryText: string }) {
  emit("editInTab", payload);
}

// 搜索栏与搜索状态绑定到当前结果 tab:
//   · 翻页 / 换结果 / 关闭当前 result tab 都会让 resultTab 变化 -> 关掉搜索栏 + 清勾选
watch(() => props.resultTab?.id, () => {
  showSearchBar.value = false;
  searchKeyword.value = "";
  currentMatchIdx.value = 0;
  selectedKeys.value = new Set();
  dirtyFields.value = new Set();
});
// 同一 result tab 内 result 换了(翻页 / refresh) —— 保留搜索栏开关,但清关键字 / dirty
watch(() => result.value, () => {
  searchKeyword.value = "";
  selectedKeys.value = new Set();
  dirtyFields.value = new Set();
});

function handlePageSizeChange(size: number) { emit("pageChange", 1, size); }
function handlePageChange(page: number) { emit("pageChange", page, pageSize.value); }

const message = useMessage();
const dialog = useDialog();

// ---- 导出 ----
const showExportDialog = ref(false);
function openExportDialog() {
  if (!result.value || result.value.documents.length === 0) {
    message.warning("没有可导出的数据");
    return;
  }
  showExportDialog.value = true;
}
function handleExported(count: number) {
  message.success(`导出完成，共 ${count.toLocaleString()} 条`);
}

// ---- 复制 ----
async function handleCopyDocs() {
  if (!result.value || result.value.documents.length === 0) {
    message.warning("没有可复制的数据");
    return;
  }
  try {
    const text = JSON.stringify(result.value.documents, null, 2);
    await navigator.clipboard.writeText(text);
    message.success(`已复制 ${result.value.documents.length} 条文档到剪贴板`);
  } catch { message.error("复制失败"); }
}

// ---- 插入文档 ----
const showInsertDialog = ref(false);
const insertDocText = ref("{\n  \n}");
const inserting = ref(false);

function openInsertDialog() {
  if (!props.collection) { message.warning("请先选择集合"); return; }
  insertDocText.value = "{\n  \n}";
  showInsertDialog.value = true;
}

async function handleInsertDoc() {
  if (!props.collection) return;
  inserting.value = true;
  try {
    const doc = JSON.parse(insertDocText.value);
    await docApi.insertDocument(props.connectionId, props.database, props.collection, doc);
    message.success("插入成功");
    showInsertDialog.value = false;
    emit("refresh");
  } catch (e) {
    message.error(`插入失败: ${e}`);
  } finally {
    inserting.value = false;
  }
}

// ---- 删除文档 (仅勾选项) ----
/** 把 _id 格式化成 shell 风格展示, 用于确认弹窗里列出 */
function formatIdForDisplay(id: unknown): string {
  if (id === null || id === undefined) return "—";
  if (typeof id === "object") {
    const obj = id as Record<string, unknown>;
    if (typeof obj.$oid === "string") return `ObjectId("${obj.$oid}")`;
    if (typeof obj.$numberLong === "string") return `NumberLong("${obj.$numberLong}")`;
    if (typeof obj.$date !== "undefined") return `ISODate(${JSON.stringify(obj.$date)})`;
    try { return JSON.stringify(id); } catch { return String(id); }
  }
  if (typeof id === "string") return `"${id}"`;
  return String(id);
}

function handleDeleteSelected() {
  if (!result.value || !props.collection) return;

  const selectedDocs = result.value.documents.filter((d) => {
    const key = docSelectionKey(d);
    return key !== null && selectedKeys.value.has(key);
  });

  if (selectedDocs.length === 0) {
    message.warning("请先勾选要删除的文档");
    return;
  }

  const ids = selectedDocs
    .map((d) => d._id)
    .filter((id): id is Record<string, unknown> => id !== undefined && id !== null);

  if (ids.length === 0) {
    message.warning("勾选的文档缺少 _id, 无法删除");
    return;
  }

  // 弹窗里最多列出前 20 条, 超过就折叠
  const MAX_PREVIEW = 20;
  const previewIds = ids.slice(0, MAX_PREVIEW).map(formatIdForDisplay);
  const overflow = ids.length - previewIds.length;

  dialog.warning({
    title: "确认删除",
    content: () => h("div", [
      h("p", { style: "margin:0 0 8px" }, `确定要删除以下 ${ids.length} 条文档吗？此操作不可撤销。`),
      h(
        "div",
        {
          style: "max-height:200px;overflow:auto;padding:8px 10px;background:#f7f7f7;"
            + "border:1px solid #eee;border-radius:3px;font-family:'Fira Code','Consolas',monospace;"
            + "font-size:12px;line-height:1.6",
        },
        [
          ...previewIds.map((s) => h("div", { style: "word-break:break-all" }, s)),
          overflow > 0
            ? h("div", { style: "margin-top:4px;color:#888;font-style:italic" },
              `… 还有 ${overflow} 条未展示`)
            : null,
        ],
      ),
    ]),
    positiveText: "删除",
    negativeText: "取消",
    onPositiveClick: async () => {
      try {
        const filter = { _id: { $in: ids } };
        await docApi.deleteDocuments(props.connectionId, props.database, props.collection, filter);
        message.success(`已删除 ${ids.length} 条文档`);
        selectedKeys.value = new Set();
        emit("refresh");
      } catch (e) {
        message.error(`删除失败: ${e}`);
      }
    },
  });
}

onErrorCaptured((err) => {
  renderError.value = String(err);
  console.error("ResultPanel child error:", err);
  return false;
});

// Ctrl+F -> 打开搜索栏
function onKeyDown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f" && !e.shiftKey && !e.altKey) {
    // 只拦截 ResultPanel 作为前景视图时的全局 Ctrl+F
    if (!result.value) return;
    e.preventDefault();
    showSearchBar.value = true;
  }
}
onMounted(() => window.addEventListener("keydown", onKeyDown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeyDown));
</script>

<template>
  <div class="result-panel">
    <ResultToolbar
      v-if="result && !explainResult"
      :total-count="totalCount"
      :returned-count="returnedCount"
      :selected-count="selectedKeys.size"
      :page-size="pageSize"
      :current-page="currentPage"
      :view-mode="viewMode"
      :execution-time-ms="result.executionTimeMs"
      :collection="collection"
      :loading="loading"
      @update:page-size="handlePageSizeChange"
      @update:current-page="handlePageChange"
      @update:view-mode="viewMode = $event"
      @refresh="emit('refresh')"
      @insert-doc="openInsertDialog"
      @delete-selected="handleDeleteSelected"
      @copy-docs="handleCopyDocs"
      @export-docs="openExportDialog"
      @toggle-search="showSearchBar = !showSearchBar; if (!showSearchBar) searchKeyword = ''"
    />

    <!-- 搜索栏 (Excel 风格) -->
    <div v-if="showSearchBar" class="search-bar">
      <n-input
        v-model:value="searchKeyword"
        size="small"
        placeholder="在当前结果里搜索..."
        clearable
        autofocus
        style="max-width: 320px"
        @keydown.enter.exact.prevent="gotoNextMatch"
        @keydown.shift.enter.prevent="gotoPrevMatch"
        @keydown.escape="showSearchBar = false; searchKeyword = ''"
      />
      <span v-if="searchKeyword" class="search-count">
        <template v-if="matchDocIndexes.length === 0">0 结果</template>
        <template v-else>
          {{ currentMatchIdx + 1 }} / {{ matchDocIndexes.length }} 行
        </template>
      </span>
      <n-button-group size="tiny">
        <n-button
          :disabled="matchDocIndexes.length === 0"
          quaternary
          title="上一个 (Shift+Enter)"
          @click="gotoPrevMatch"
        >↑</n-button>
        <n-button
          :disabled="matchDocIndexes.length === 0"
          quaternary
          title="下一个 (Enter)"
          @click="gotoNextMatch"
        >↓</n-button>
      </n-button-group>
      <n-button
        size="tiny"
        quaternary
        :type="matchCase ? 'primary' : 'default'"
        title="区分大小写"
        @click="matchCase = !matchCase"
      >Aa</n-button>
      <n-button
        size="tiny"
        quaternary
        title="关闭 (Esc)"
        @click="showSearchBar = false; searchKeyword = ''"
      >×</n-button>
    </div>

    <div class="result-body">
      <div v-if="loading" class="result-loading">
        <div class="loading-spinner" />
      </div>
      <div v-else-if="error" class="result-error">
        <n-alert type="error" title="查询错误">{{ error }}</n-alert>
      </div>
      <ExplainView v-else-if="explainResult" :explain-result="explainResult" />
      <div v-else-if="isConsole" class="console-view">
        <div v-if="consoleLines.length === 0" class="result-empty">
          <n-empty description="暂无 print() 输出" />
        </div>
        <pre v-else class="console-output">{{ consoleLines.join("\n") }}</pre>
      </div>
      <div v-else-if="result" class="result-content">
        <div v-if="renderError" class="result-error">
          <n-alert type="warning" title="渲染错误">
            {{ renderError }}
            <n-button size="small" style="margin-top:8px" @click="renderError = null">重试</n-button>
          </n-alert>
        </div>
        <TreeDocView
          v-else-if="viewMode === 'tree'"
          :documents="pagedDocuments"
          :row-offset="rowOffset"
          :connection-id="connectionId"
          :database="database"
          :collection="collection"
          :doc-key-fn="docSelectionKey"
          :selected-keys="selectedKeys"
          :dirty-fields="dirtyFields"
          :search-keyword="searchKeyword"
          :match-case="matchCase"
          :active-match-doc-index="activeMatchDocIndex"
          :match-doc-indexes="matchDocIndexes"
          @toggle-select="toggleSelect"
          @set-selection="setSelectedKeys"
          @edit-in-tab="forwardEditInTab"
          @dirty="(k: string, f: string) => markDirty(k, f)"
        />
        <TableView
          v-else-if="viewMode === 'table'"
          :documents="pagedDocuments"
          :row-offset="rowOffset"
          :connection-id="connectionId"
          :database="database"
          :collection="collection"
          :doc-key-fn="docSelectionKey"
          :selected-keys="selectedKeys"
          :dirty-fields="dirtyFields"
          :search-keyword="searchKeyword"
          :match-case="matchCase"
          :active-match-doc-index="activeMatchDocIndex"
          :match-doc-indexes="matchDocIndexes"
          @set-selection="setSelectedKeys"
          @edit-in-tab="forwardEditInTab"
          @dirty="(k: string, f: string) => markDirty(k, f)"
        />
        <JsonTreeView v-else :documents="pagedDocuments" :row-offset="rowOffset" />
      </div>
      <div v-else class="result-empty">
        <n-empty description="运行查询查看结果" />
      </div>
    </div>

    <!-- 导出对话框 -->
    <ExportDialog
      v-if="result"
      v-model:show="showExportDialog"
      :documents="result.documents"
      :connection-id="connectionId"
      :database="database"
      :collection="collection"
      :query-text="queryText"
      @exported="handleExported"
    />

    <!-- 插入文档弹窗 -->
    <n-modal v-model:show="showInsertDialog" preset="card" title="插入文档" style="width:520px" :bordered="false">
      <n-input
        v-model:value="insertDocText"
        type="textarea"
        :rows="12"
        placeholder='{"key": "value"}'
        style="font-family: Consolas, Monaco, monospace; font-size: 13px"
      />
      <template #action>
        <div style="display:flex;justify-content:flex-end;gap:8px">
          <n-button size="small" @click="showInsertDialog = false">取消</n-button>
          <n-button type="primary" size="small" :loading="inserting" @click="handleInsertDoc">插入</n-button>
        </div>
      </template>
    </n-modal>
  </div>
</template>

<style scoped>
.result-panel { flex: 1 1 auto; min-height: 0; height: 100%; display: flex; flex-direction: column; overflow: hidden; }
.result-body { flex: 1; min-height: 0; overflow: hidden; position: relative; }
.result-content { height: 100%; overflow: hidden; display: flex; flex-direction: column; }
.result-content > * { flex: 1; min-height: 0; }
.result-loading { display: flex; align-items: center; justify-content: center; height: 100%; }
.loading-spinner { width: 24px; height: 24px; border: 3px solid #e8e8e8; border-top-color: #3875d7; border-radius: 50%; animation: spin 0.6s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
.result-error { padding: 16px; overflow: auto; }
.result-empty { display: flex; align-items: center; justify-content: center; height: 100%; }
.console-view { height: 100%; overflow: auto; background: #1e1e1e; }
.console-output {
  margin: 0;
  padding: 10px 14px;
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 12px;
  line-height: 1.6;
  color: #d4d4d4;
  white-space: pre-wrap;
  word-break: break-all;
}
.search-bar { display: flex; align-items: center; gap: 6px; padding: 4px 8px; background: #fffbe6; border-bottom: 1px solid #e0e0e0; flex-shrink: 0; }
.search-count {
  font-size: 12px;
  color: #666;
  white-space: nowrap;
  min-width: 70px;
  font-variant-numeric: tabular-nums;
}
</style>

<script setup lang="ts">
import { ref, computed, onErrorCaptured, watch } from "vue";
import { NEmpty, NAlert, NButton, NModal, NInput, useMessage, useDialog } from "naive-ui";
import ResultToolbar from "./ResultToolbar.vue";
import TreeDocView from "./TreeDocView.vue";
import TableView from "./TableView.vue";
import JsonTreeView from "./JsonTreeView.vue";
import ExportDialog from "./ExportDialog.vue";
import type { QueryResult } from "@/types/database";
import * as docApi from "@/api/document";

const props = defineProps<{
  result: QueryResult | null;
  error: string | null;
  loading: boolean;
  connectionId: string;
  database: string;
  collection: string;
  queryText?: string;
  currentPage: number;
  pageSize: number;
}>();

const emit = defineEmits<{
  refresh: [];
  pageChange: [page: number, pageSize: number];
}>();

const viewMode = ref<"tree" | "table" | "json">("tree");
const renderError = ref<string | null>(null);

const totalCount = computed(() => props.result?.totalCount ?? props.result?.count ?? 0);
const returnedCount = computed(() => props.result?.documents.length ?? 0);
const rowOffset = computed(() => (props.currentPage - 1) * props.pageSize);

// ---- 结果搜索 ----
const showSearchBar = ref(false);
const searchKeyword = ref("");

const filteredDocuments = computed(() => {
  const docs = props.result?.documents ?? [];
  const kw = searchKeyword.value.trim().toLowerCase();
  if (!kw) return docs;
  return docs.filter((doc) => JSON.stringify(doc).toLowerCase().includes(kw));
});

const pagedDocuments = computed(() => filteredDocuments.value);

// 搜索关键词变化时重置
watch(() => props.result, () => { searchKeyword.value = ""; });

function handlePageSizeChange(size: number) { emit("pageChange", 1, size); }
function handlePageChange(page: number) { emit("pageChange", page, props.pageSize); }

const message = useMessage();
const dialog = useDialog();

// ---- 导出 ----
const showExportDialog = ref(false);
function openExportDialog() {
  if (!props.result || props.result.documents.length === 0) {
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
  if (!props.result || props.result.documents.length === 0) {
    message.warning("没有可复制的数据");
    return;
  }
  try {
    const text = JSON.stringify(props.result.documents, null, 2);
    await navigator.clipboard.writeText(text);
    message.success(`已复制 ${props.result.documents.length} 条文档到剪贴板`);
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

// ---- 删除文档 ----
function handleDeleteSelected() {
  if (!props.result || props.result.documents.length === 0 || !props.collection) return;

  const ids = props.result.documents
    .map((d) => d._id)
    .filter((id): id is Record<string, unknown> => id !== undefined && id !== null);

  if (ids.length === 0) {
    message.warning("没有可删除的文档（缺少 _id 字段）");
    return;
  }

  dialog.warning({
    title: "确认删除",
    content: `确定要删除当前页 ${ids.length} 条文档吗？此操作不可撤销。`,
    positiveText: "删除",
    negativeText: "取消",
    onPositiveClick: async () => {
      try {
        // 用 $in 批量删除
        const filter = { _id: { $in: ids } };
        await docApi.deleteDocuments(props.connectionId, props.database, props.collection, filter);
        message.success(`已删除 ${ids.length} 条文档`);
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
</script>

<template>
  <div class="result-panel">
    <ResultToolbar
      v-if="result"
      :total-count="totalCount"
      :returned-count="returnedCount"
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

    <!-- 搜索栏 -->
    <div v-if="showSearchBar" class="search-bar">
      <n-input
        v-model:value="searchKeyword"
        size="small"
        placeholder="搜索当前页文档..."
        clearable
        style="max-width: 400px"
      />
      <span class="search-count">{{ filteredDocuments.length }} / {{ result?.documents.length ?? 0 }}</span>
    </div>

    <div class="result-body">
      <div v-if="loading" class="result-loading">
        <div class="loading-spinner" />
      </div>
      <div v-else-if="error" class="result-error">
        <n-alert type="error" title="查询错误">{{ error }}</n-alert>
      </div>
      <div v-else-if="result" class="result-content">
        <div v-if="renderError" class="result-error">
          <n-alert type="warning" title="渲染错误">
            {{ renderError }}
            <n-button size="small" style="margin-top:8px" @click="renderError = null">重试</n-button>
          </n-alert>
        </div>
        <TreeDocView v-else-if="viewMode === 'tree'" :documents="pagedDocuments" :row-offset="rowOffset" />
        <TableView
          v-else-if="viewMode === 'table'"
          :documents="pagedDocuments"
          :row-offset="rowOffset"
          :connection-id="connectionId"
          :database="database"
          :collection="collection"
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
.result-panel { height: 100%; display: flex; flex-direction: column; overflow: hidden; }
.result-body { flex: 1; min-height: 0; overflow: hidden; position: relative; }
.result-content { height: 100%; overflow: hidden; display: flex; flex-direction: column; }
.result-content > * { flex: 1; min-height: 0; }
.result-loading { display: flex; align-items: center; justify-content: center; height: 100%; }
.loading-spinner { width: 24px; height: 24px; border: 3px solid #e8e8e8; border-top-color: #3875d7; border-radius: 50%; animation: spin 0.6s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
.result-error { padding: 16px; overflow: auto; }
.result-empty { display: flex; align-items: center; justify-content: center; height: 100%; }
.search-bar { display: flex; align-items: center; gap: 8px; padding: 4px 8px; background: #fffbe6; border-bottom: 1px solid #e0e0e0; flex-shrink: 0; }
.search-count { font-size: 12px; color: #999; white-space: nowrap; }
</style>

<script setup lang="ts">
import { ref, computed, watch, onUnmounted } from "vue";
import {
  NModal, NCard, NButton, NSelect, NInput, NCheckbox,
  NCheckboxGroup, NSpace, NIcon, NScrollbar, NProgress,
} from "naive-ui";
import {
  Download as ExportIcon,
  FolderOpen as BrowseIcon,
} from "@vicons/ionicons5";
import { save } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useConnectionStore } from "@/stores/connection";
import { useDatabaseStore } from "@/stores/database";
import { FORMAT_LIST, type ExportFormat } from "@/api/export";

const props = defineProps<{
  show: boolean;
  documents: Record<string, unknown>[];
  connectionId: string;
  database: string;
  collection?: string;
  queryText?: string;
}>();

const emit = defineEmits<{
  "update:show": [val: boolean];
  exported: [count: number];
}>();

const connStore = useConnectionStore();
const dbStore = useDatabaseStore();

// ---- 连接/数据库选择 ----
const selectedConnId = ref(props.connectionId);
const selectedDb = ref(props.database);

// 连接列表（显示所有已连接的）
const connOptions = computed(() => {
  const opts = connStore.connections
    .filter((c) => connStore.isActive(c.id))
    .map((c) => ({ label: c.name || `${c.host}:${c.port}`, value: c.id }));
  // 确保当前连接在列表中
  if (opts.length === 0) {
    const cfg = connStore.connections.find((c) => c.id === selectedConnId.value);
    if (cfg) opts.push({ label: cfg.name || `${cfg.host}:${cfg.port}`, value: cfg.id });
  }
  return opts;
});

// 选中连接的数据库列表
const dbOptions = computed(() => {
  const dbs = dbStore.getDatabases(selectedConnId.value);
  if (dbs.length > 0) return dbs.map((d) => ({ label: d.name, value: d.name }));
  // fallback: 至少包含当前数据库
  if (selectedDb.value) return [{ label: selectedDb.value, value: selectedDb.value }];
  return [];
});

// 切换连接时重置数据库
function handleConnChange(connId: string) {
  selectedConnId.value = connId;
  const dbs = dbStore.getDatabases(connId);
  selectedDb.value = dbs[0]?.name ?? "";
}

// ---- 表单 ----
const format = ref<ExportFormat>("simple-json");
const targetPath = ref("");
const delimiter = ref(",");
const exporting = ref(false);
const exportedCount = ref(0);
const exportTotal = ref(0);
const exportError = ref("");

const formatOptions = FORMAT_LIST.map((f) => ({
  label: f.label,
  value: f.value,
}));

const currentFormatInfo = computed(() => FORMAT_LIST.find((f) => f.value === format.value)!);

const delimiterOptions = [
  { label: "Comma (,)", value: "," },
  { label: "Tab (\\t)", value: "\t" },
  { label: "Semicolon (;)", value: ";" },
  { label: "Pipe (|)", value: "|" },
];

// ---- 字段 ----
const allFields = computed(() => {
  const fieldSet = new Set<string>();
  for (const doc of props.documents) {
    for (const key of Object.keys(doc)) fieldSet.add(key);
  }
  return Array.from(fieldSet);
});

const selectedFields = ref<string[]>([]);

watch(() => props.show, (show) => {
  if (show) {
    selectedConnId.value = props.connectionId;
    selectedDb.value = props.database;
    selectedFields.value = [...allFields.value];
    targetPath.value = "";
    exportError.value = "";
    exporting.value = false;
    exportedCount.value = 0;
    exportTotal.value = 0;
  }
});

const allChecked = computed(() => selectedFields.value.length === allFields.value.length);
const someChecked = computed(() => selectedFields.value.length > 0 && !allChecked.value);

function toggleAll() {
  selectedFields.value = allChecked.value ? [] : [...allFields.value];
}

// ---- 浏览文件 ----
async function browseTarget() {
  const info = currentFormatInfo.value;
  const name = props.collection || "export";
  const path = await save({
    title: "Export Target",
    defaultPath: `${name}.${info.ext}`,
    filters: [{ name: info.label, extensions: [info.ext] }],
  });
  if (path) targetPath.value = path;
}

// ---- 进度监听 ----
let unlisten: UnlistenFn | null = null;

async function startProgressListener() {
  unlisten = await listen<{ exported: number; total: number }>("export-progress", (ev) => {
    exportedCount.value = ev.payload.exported;
    exportTotal.value = ev.payload.total;
  });
}

function stopProgressListener() {
  if (unlisten) { unlisten(); unlisten = null; }
}

onUnmounted(stopProgressListener);

const progressPercent = computed(() => {
  if (exportTotal.value <= 0) return 0;
  return Math.min(100, Math.round((exportedCount.value / exportTotal.value) * 100));
});

// ---- 导出 ----
async function handleExport() {
  if (selectedFields.value.length === 0 || !targetPath.value) return;

  exporting.value = true;
  exportedCount.value = 0;
  exportTotal.value = 0;
  exportError.value = "";

  await startProgressListener();

  try {
    const count = await invoke<number>("export_query", {
      request: {
        connectionId: selectedConnId.value,
        database: selectedDb.value,
        queryText: props.queryText || "",
        format: format.value,
        fields: selectedFields.value,
        targetPath: targetPath.value,
        delimiter: delimiter.value,
        collectionName: props.collection || null,
      },
    });
    exportedCount.value = count;
    emit("exported", count);
    // 短暂显示完成状态
    setTimeout(() => {
      emit("update:show", false);
    }, 800);
  } catch (e) {
    exportError.value = String(e);
  } finally {
    stopProgressListener();
    exporting.value = false;
  }
}

function handleClose() {
  if (exporting.value) return; // 导出中不允许关闭
  emit("update:show", false);
}

const canExport = computed(() =>
  selectedFields.value.length > 0 && targetPath.value.length > 0 && !exporting.value,
);
</script>

<template>
  <n-modal :show="show" :mask-closable="!exporting" :trap-focus="false" @update:show="emit('update:show', $event)">
    <n-card
      title="Export"
      :bordered="false"
      :closable="!exporting"
      role="dialog"
      style="width: 600px"
      @close="handleClose"
    >
      <div class="export-form">
        <!-- MongoDB -->
        <div class="export-row">
          <label class="export-label">MongoDB</label>
          <n-select
            :value="selectedConnId"
            :options="connOptions"
            size="small"
            style="width: 220px"
            :disabled="exporting"
            @update:value="handleConnChange"
          />
          <n-select
            v-model:value="selectedDb"
            :options="dbOptions"
            size="small"
            style="width: 180px; margin-left: 8px"
            :disabled="exporting"
          />
        </div>

        <!-- Source -->
        <div class="export-row">
          <label class="export-label">Source</label>
          <div class="export-value">
            <span class="source-tag">Query Result</span>
            <span v-if="collection" class="source-coll">{{ collection }}</span>
          </div>
        </div>

        <!-- Query -->
        <div class="export-row export-row-top">
          <label class="export-label">Query</label>
          <div class="query-preview">{{ queryText || '—' }}</div>
        </div>

        <!-- Format -->
        <div class="export-row">
          <label class="export-label">Format</label>
          <n-select v-model:value="format" :options="formatOptions" size="small" style="flex:1" :disabled="exporting" />
        </div>
        <div class="export-row">
          <label class="export-label" />
          <span class="format-desc">{{ currentFormatInfo.description }}</span>
        </div>

        <!-- Delimiter (CSV / TXT) -->
        <div v-if="format === 'csv' || format === 'txt'" class="export-row">
          <label class="export-label">Delimiter</label>
          <n-select v-model:value="delimiter" :options="delimiterOptions" size="small" style="flex:1" :disabled="exporting" />
        </div>

        <!-- Target -->
        <div class="export-row">
          <label class="export-label">Target</label>
          <n-input
            v-model:value="targetPath"
            size="small"
            placeholder="Export path..."
            readonly
            style="flex:1"
            :disabled="exporting"
          />
          <n-button size="small" style="margin-left:6px" :disabled="exporting" @click="browseTarget">
            <template #icon><n-icon :size="14"><BrowseIcon /></n-icon></template>
          </n-button>
        </div>
      </div>

      <!-- 进度条 -->
      <div v-if="exporting || exportedCount > 0" class="progress-section">
        <n-progress
          type="line"
          :percentage="progressPercent"
          :status="exportError ? 'error' : (progressPercent >= 100 ? 'success' : 'default')"
          :indicator-placement="'inside'"
          :height="20"
        />
        <div class="progress-text">
          <span v-if="exporting">正在导出... {{ exportedCount.toLocaleString() }} / {{ exportTotal.toLocaleString() }}</span>
          <span v-else-if="exportError" class="error-text">导出失败: {{ exportError }}</span>
          <span v-else class="success-text">导出完成，共 {{ exportedCount.toLocaleString() }} 条</span>
        </div>
      </div>

      <!-- 字段选择 -->
      <div class="field-section">
        <div class="field-header">
          <span class="field-title">Fields Selection:</span>
          <span class="field-hint">May not list all fields due to MongoDB schema-less feature</span>
        </div>

        <div class="field-toolbar">
          <n-checkbox
            :checked="allChecked"
            :indeterminate="someChecked"
            :disabled="exporting"
            @update:checked="toggleAll"
          >
            Field
          </n-checkbox>
          <span class="field-count">{{ selectedFields.length }} / {{ allFields.length }}</span>
        </div>

        <n-scrollbar style="max-height: 280px">
          <n-checkbox-group v-model:value="selectedFields" :disabled="exporting">
            <div v-for="field in allFields" :key="field" class="field-item">
              <n-checkbox :value="field" :label="field" />
            </div>
          </n-checkbox-group>
        </n-scrollbar>
      </div>

      <template #action>
        <n-space justify="end">
          <n-button size="small" :disabled="exporting" @click="handleClose">Cancel</n-button>
          <n-button
            type="primary"
            size="small"
            :disabled="!canExport"
            :loading="exporting"
            @click="handleExport"
          >
            <template #icon><n-icon :size="14"><ExportIcon /></n-icon></template>
            Execute
          </n-button>
        </n-space>
      </template>
    </n-card>
  </n-modal>
</template>

<style scoped>
.export-form {
  margin-bottom: 12px;
}
.export-row {
  display: flex;
  align-items: center;
  margin-bottom: 10px;
}
.export-row-top {
  align-items: flex-start;
}
.export-label {
  width: 72px;
  font-size: 13px;
  font-weight: 500;
  color: #333;
  flex-shrink: 0;
}
.export-value {
  display: flex;
  align-items: center;
  font-size: 13px;
  gap: 6px;
}
.mongo-info {
  font-weight: 500;
}
.db-badge {
  background: #e8a838;
  color: #fff;
  padding: 1px 8px;
  border-radius: 3px;
  font-size: 12px;
  margin-left: 4px;
}
.source-tag {
  color: #333;
  font-weight: 500;
}
.source-coll {
  color: #18a058;
  font-weight: 500;
}
.format-desc {
  font-size: 12px;
  color: #888;
  margin-top: -4px;
  padding-left: 2px;
}
.query-preview {
  flex: 1;
  background: #f8f8f8;
  border: 1px solid #e0e0e0;
  border-radius: 4px;
  padding: 6px 10px;
  font-family: "Consolas", "Monaco", monospace;
  font-size: 12px;
  color: #333;
  max-height: 80px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-all;
}
.progress-section {
  margin-bottom: 12px;
}
.progress-text {
  font-size: 12px;
  color: #666;
  margin-top: 4px;
  text-align: center;
}
.error-text { color: #d03050; }
.success-text { color: #18a058; }
.field-section {
  border: 1px solid #e0e0e0;
  border-radius: 4px;
  overflow: hidden;
}
.field-header {
  background: #f0f0f0;
  padding: 6px 10px;
  border-bottom: 1px solid #e0e0e0;
  display: flex;
  align-items: center;
  gap: 8px;
}
.field-title {
  font-size: 13px;
  font-weight: 600;
  color: #333;
}
.field-hint {
  font-size: 12px;
  color: #d03050;
}
.field-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 10px;
  border-bottom: 1px solid #eee;
  background: #fafafa;
}
.field-count {
  font-size: 12px;
  color: #999;
}
.field-item {
  padding: 4px 10px;
  border-bottom: 1px solid #f5f5f5;
}
.field-item:hover {
  background: #f0f7ff;
}
</style>

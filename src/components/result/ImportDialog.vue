<script setup lang="ts">
import { ref, computed, watch, onUnmounted } from "vue";
import {
  NModal, NCard, NButton, NSelect, NInput, NSpace, NIcon, NProgress,
} from "naive-ui";
import { FolderOpen as BrowseIcon, Download as ImportIcon } from "@vicons/ionicons5";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useConnectionStore } from "@/stores/connection";
import { useDatabaseStore } from "@/stores/database";

const props = defineProps<{
  show: boolean;
  connectionId: string;
  database: string;
  collection: string;
}>();

const emit = defineEmits<{
  "update:show": [val: boolean];
  imported: [count: number];
}>();

const connStore = useConnectionStore();
const dbStore = useDatabaseStore();

// ---- Target 选择 ----
const selectedConnId = ref(props.connectionId);
const selectedDb = ref(props.database);
const selectedColl = ref(props.collection);

const connOptions = computed(() => {
  const opts = connStore.connections
    .filter((c) => connStore.isActive(c.id))
    .map((c) => ({ label: c.name || `${c.host}:${c.port}`, value: c.id }));
  if (opts.length === 0) {
    const cfg = connStore.connections.find((c) => c.id === selectedConnId.value);
    if (cfg) opts.push({ label: cfg.name || `${cfg.host}:${cfg.port}`, value: cfg.id });
  }
  return opts;
});

const dbOptions = computed(() => {
  const dbs = dbStore.getDatabases(selectedConnId.value);
  if (dbs.length > 0) return dbs.map((d) => ({ label: d.name, value: d.name }));
  if (selectedDb.value) return [{ label: selectedDb.value, value: selectedDb.value }];
  return [];
});

const collOptions = computed(() => {
  const colls = dbStore.getCollections(selectedConnId.value, selectedDb.value);
  if (colls.length > 0) return colls.map((c) => ({ label: c.name, value: c.name }));
  if (selectedColl.value) return [{ label: selectedColl.value, value: selectedColl.value }];
  return [];
});

async function handleConnChange(connId: string) {
  selectedConnId.value = connId;
  const dbs = dbStore.getDatabases(connId);
  selectedDb.value = dbs[0]?.name ?? "";
  selectedColl.value = "";
  if (selectedDb.value) await dbStore.fetchCollections(connId, selectedDb.value);
}

async function handleDbChange(dbName: string) {
  selectedDb.value = dbName;
  await dbStore.fetchCollections(selectedConnId.value, dbName);
  const colls = dbStore.getCollections(selectedConnId.value, dbName);
  selectedColl.value = colls[0]?.name ?? "";
}

// ---- 表单 ----
const fromType = ref("json-csv");
const filePath = ref("");
const insertionMode = ref("overwrite");
const importing = ref(false);
const importedCount = ref(0);
const importTotal = ref(0);
const importPhase = ref("");
const importError = ref("");

// ---- 进度监听 ----
let unlisten: UnlistenFn | null = null;

async function startProgressListener() {
  unlisten = await listen<{ imported: number; total: number; phase: string }>("import-progress", (ev) => {
    importedCount.value = ev.payload.imported;
    importTotal.value = ev.payload.total;
    importPhase.value = ev.payload.phase;
  });
}

function stopProgressListener() {
  if (unlisten) { unlisten(); unlisten = null; }
}

onUnmounted(stopProgressListener);

const progressPercent = computed(() => {
  if (importTotal.value <= 0) return 0;
  return Math.min(100, Math.round((importedCount.value / importTotal.value) * 100));
});

const fromOptions = [
  { label: "JSON or CSV File", value: "json-csv" },
];

const insertionOptions = [
  { label: "覆盖相同 _id 的文档", value: "overwrite" },
  { label: "跳过相同 _id 的文档", value: "skip" },
  { label: "始终插入（可能重复）", value: "insert" },
];

// ---- 初始化 ----
watch(() => props.show, async (show) => {
  if (show) {
    selectedConnId.value = props.connectionId;
    selectedDb.value = props.database;
    selectedColl.value = props.collection;
    filePath.value = "";
    importError.value = "";
    importing.value = false;
    importedCount.value = 0;
    previewData.value = null;
    // 加载集合列表
    if (selectedDb.value) {
      await dbStore.fetchCollections(selectedConnId.value, selectedDb.value);
    }
  }
});

// ---- 浏览文件 ----
async function browseFile() {
  const path = await openFileDialog({
    title: "选择导入文件",
    filters: [
      { name: "JSON / CSV", extensions: ["json", "jsonl", "csv"] },
    ],
  });
  if (path) {
    filePath.value = path as string;
    previewData.value = null;
    // 从文件名提取集合名：playerFollow.json → playerFollow
    const fileName = String(path).replace(/\\/g, "/").split("/").pop() ?? "";
    const nameWithoutExt = fileName.replace(/\.(json|jsonl|csv)$/i, "");
    if (nameWithoutExt) {
      selectedColl.value = nameWithoutExt;
    }
  }
}

// ---- 预览 ----
const previewData = ref<Record<string, unknown>[] | null>(null);

async function handlePreview() {
  if (!filePath.value) return;
  try {
    const content = await invoke<string>("read_import_file", { path: filePath.value });
    const docs = parseFileContent(content, filePath.value);
    previewData.value = docs.slice(0, 20); // 只预览前 20 条
  } catch (e) {
    importError.value = `预览失败: ${e}`;
  }
}

// ---- 解析文件 ----

/** 将 MongoShell 格式转为标准 JSON */
function convertShellToJson(text: string): string {
  let s = text;
  // 1. Shell 类型构造器 → Extended JSON（支持单/双引号和转义）
  s = s.replace(/ObjectId\(["']([^"']*)["']\)/g, '{"$oid":"$1"}');
  s = s.replace(/ISODate\(["']([^"']*)["']\)/g, '{"$date":"$1"}');
  s = s.replace(/new\s+Date\(["']([^"']*)["']\)/g, '{"$date":"$1"}');
  s = s.replace(/NumberLong\(["']?(-?\d+)["']?\)/g, '{"$numberLong":"$1"}');
  s = s.replace(/NumberInt\((-?\d+)\)/g, '$1');
  s = s.replace(/NumberDecimal\(["']([^"']*)["']\)/g, '{"$numberDecimal":"$1"}');
  s = s.replace(/Timestamp\((\d+),\s*(\d+)\)/g, '{"$timestamp":{"t":$1,"i":$2}}');
  s = s.replace(/BinData\((\d+),\s*["']([^"']*)["']\)/g, '{"$binary":{"base64":"$2","subType":"$1"}}');
  s = s.replace(/UUID\(["']([^"']*)["']\)/g, '{"$uuid":"$1"}');

  // 2. Unquoted keys → quoted keys: {_id: ... , name: ...} → {"_id": ..., "name": ...}
  s = s.replace(/([{,]\s*)([a-zA-Z_$][a-zA-Z0-9_$.]*)(\s*:)/g, '$1"$2"$3');

  // 3. 单引号字符串值 → 双引号（简单替换，不处理嵌套）
  // 只替换值位置的单引号（: 后面的）
  s = s.replace(/:\s*'([^']*)'/g, ':"$1"');

  return s;
}

function parseJsonSafe(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    try {
      const converted = convertShellToJson(text);
      return JSON.parse(converted);
    } catch (e2) {
      throw new Error(`JSON 解析失败，请检查文件格式: ${(e2 as Error).message}`);
    }
  }
}

function parseFileContent(content: string, path: string): Record<string, unknown>[] {
  const ext = path.split(".").pop()?.toLowerCase();
  if (ext === "csv") return parseCsv(content);
  if (ext === "jsonl") return content.trim().split("\n").filter(Boolean).map((l) => parseJsonSafe(l) as Record<string, unknown>);
  const parsed = parseJsonSafe(content);
  return Array.isArray(parsed) ? parsed as Record<string, unknown>[] : [parsed as Record<string, unknown>];
}

function parseCsv(content: string): Record<string, unknown>[] {
  const lines = content.trim().split("\n");
  if (lines.length < 2) return [];
  const headers = parseCsvLine(lines[0]);
  return lines.slice(1).filter(Boolean).map((line) => {
    const vals = parseCsvLine(line);
    const doc: Record<string, unknown> = {};
    headers.forEach((h, i) => {
      const v = vals[i] ?? "";
      if (v === "true") doc[h] = true;
      else if (v === "false") doc[h] = false;
      else if (v === "" || v === "null") doc[h] = null;
      else if (/^-?\d+$/.test(v)) doc[h] = parseInt(v);
      else if (/^-?\d+\.\d+$/.test(v)) doc[h] = parseFloat(v);
      else doc[h] = v;
    });
    return doc;
  });
}

function parseCsvLine(line: string): string[] {
  const result: string[] = [];
  let current = "";
  let inQuote = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (inQuote) {
      if (ch === '"' && line[i + 1] === '"') { current += '"'; i++; }
      else if (ch === '"') inQuote = false;
      else current += ch;
    } else {
      if (ch === '"') inQuote = true;
      else if (ch === ",") { result.push(current); current = ""; }
      else current += ch;
    }
  }
  result.push(current);
  return result;
}

// ---- 执行导入 ----
async function handleExecute() {
  if (connStore.isReadOnly(selectedConnId.value)) {
    importError.value = "只读连接: 不允许导入数据";
    return;
  }
  if (!filePath.value || !selectedColl.value) return;
  importing.value = true;
  importError.value = "";
  importedCount.value = 0;
  importTotal.value = 0;
  importPhase.value = "准备中...";

  await startProgressListener();

  try {
    const count = await invoke<number>("stream_import", {
      request: {
        connectionId: selectedConnId.value,
        database: selectedDb.value,
        collection: selectedColl.value,
        filePath: filePath.value,
        insertionMode: insertionMode.value,
      },
    });

    importedCount.value = count;
    importPhase.value = "完成";
    emit("imported", count);
    setTimeout(() => emit("update:show", false), 800);
  } catch (e) {
    importError.value = String(e);
  } finally {
    stopProgressListener();
    importing.value = false;
  }
}

function handleClose() {
  if (importing.value) return;
  emit("update:show", false);
}

const targetIsReadOnly = computed(() => connStore.isReadOnly(selectedConnId.value));

const canExecute = computed(() =>
  filePath.value.length > 0
  && selectedColl.value.length > 0
  && !importing.value
  && !targetIsReadOnly.value,
);
</script>

<template>
  <n-modal :show="show" :mask-closable="!importing" :trap-focus="false" @update:show="emit('update:show', $event)">
    <n-card
      title="Import Documents Into the Collection"
      :bordered="false"
      :closable="!importing"
      role="dialog"
      style="width: 620px"
      @close="handleClose"
    >
      <div class="import-form">
        <!-- Target -->
        <div class="import-row">
          <label class="import-label">Target</label>
          <n-select
            :value="selectedConnId"
            :options="connOptions"
            size="small"
            style="width: 180px"
            :disabled="importing"
            @update:value="handleConnChange"
          />
          <n-select
            :value="selectedDb"
            :options="dbOptions"
            size="small"
            style="width: 150px; margin-left: 6px"
            :disabled="importing"
            @update:value="handleDbChange"
          />
          <n-select
            v-model:value="selectedColl"
            :options="collOptions"
            size="small"
            style="width: 150px; margin-left: 6px"
            :disabled="importing"
            filterable
            tag
          />
        </div>

        <!-- From -->
        <div class="import-row">
          <label class="import-label">From</label>
          <n-select v-model:value="fromType" :options="fromOptions" size="small" style="flex:1" :disabled="importing" />
        </div>

        <!-- File -->
        <div class="import-row">
          <label class="import-label">File</label>
          <n-input
            v-model:value="filePath"
            size="small"
            placeholder="选择 JSON 或 CSV 文件..."
            readonly
            style="flex:1"
            :disabled="importing"
          />
          <n-button size="small" style="margin-left:6px" :disabled="importing" @click="browseFile">
            <template #icon><n-icon :size="14"><BrowseIcon /></n-icon></template>
          </n-button>
        </div>

        <!-- Insertion Mode -->
        <div class="import-row">
          <label class="import-label">Insertion</label>
          <n-select v-model:value="insertionMode" :options="insertionOptions" size="small" style="flex:1" :disabled="importing" />
        </div>
      </div>

      <!-- 进度 -->
      <div v-if="importing || importedCount > 0 || importError" class="import-status">
        <n-progress
          v-if="importing || importedCount > 0"
          type="line"
          :percentage="progressPercent"
          :status="importError ? 'error' : (progressPercent >= 100 ? 'success' : 'default')"
          :indicator-placement="'inside'"
          :height="20"
          style="margin-bottom: 6px"
        />
        <div class="status-text">
          <span v-if="importing" class="status-loading">{{ importPhase }} {{ importedCount.toLocaleString() }} / {{ importTotal.toLocaleString() }}</span>
          <span v-else-if="importError" class="status-error">{{ importError }}</span>
          <span v-else class="status-success">导入完成，共 {{ importedCount.toLocaleString() }} 条</span>
        </div>
      </div>

      <!-- 只读连接横幅 -->
      <div v-if="targetIsReadOnly" class="import-status status-error">
        只读连接, 禁用导入
      </div>

      <!-- 预览 -->
      <div v-if="previewData" class="preview-section">
        <div class="preview-header">Imported JSON Preview (前 {{ previewData.length }} 条)</div>
        <pre class="preview-content">{{ JSON.stringify(previewData, null, 2).slice(0, 3000) }}</pre>
      </div>

      <template #action>
        <n-space justify="space-between" style="width:100%">
          <n-button size="small" :disabled="!filePath || importing" @click="handlePreview">
            Imported JSON Preview
          </n-button>
          <n-space>
            <n-button size="small" :disabled="importing" @click="handleClose">取消</n-button>
            <n-button
              type="primary"
              size="small"
              :disabled="!canExecute"
              :loading="importing"
              :title="targetIsReadOnly ? '只读连接, 禁用导入' : ''"
              @click="handleExecute"
            >
              <template #icon><n-icon :size="14"><ImportIcon /></n-icon></template>
              Execute
            </n-button>
          </n-space>
        </n-space>
      </template>
    </n-card>
  </n-modal>
</template>

<style scoped>
.import-form { margin-bottom: 12px; }
.import-row { display: flex; align-items: center; margin-bottom: 10px; }
.import-label { width: 72px; font-size: 13px; font-weight: 500; color: #333; flex-shrink: 0; }
.import-status { text-align: center; padding: 8px; margin-bottom: 8px; }
.status-loading { color: #3875d7; }
.status-error { color: #d03050; }
.status-success { color: #18a058; }
.preview-section { border: 1px solid #e0e0e0; border-radius: 4px; overflow: hidden; margin-bottom: 8px; }
.preview-header { background: #f0f0f0; padding: 6px 10px; font-size: 13px; font-weight: 500; border-bottom: 1px solid #e0e0e0; }
.preview-content { margin: 0; padding: 8px 10px; font-size: 12px; max-height: 200px; overflow: auto; background: #fafafa; font-family: Consolas, Monaco, monospace; }
</style>

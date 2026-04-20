<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { NModal, NCard, NTabs, NTabPane, NButton, NIcon, NSpace } from "naive-ui";
import {
  ChevronBack as PrevIcon,
  ChevronForward as NextIcon,
  Refresh as ReloadIcon,
  Create as EditIcon,
} from "@vicons/ionicons5";
import { getBsonType, getValueColor } from "@/utils/bson-format";
import { buildUpdateOneQuery } from "@/utils/mongo-shell-format";

const props = defineProps<{
  show: boolean;
  documents: Record<string, unknown>[];
  initialIndex: number;
  /** 集合名 —— 传入后 Edit in new tab 会拼成 `db.<coll>.updateOne(...)` */
  collection?: string;
}>();

const emit = defineEmits<{
  "update:show": [value: boolean];
  editInTab: [payload: { doc: Record<string, unknown>; queryText: string }];
}>();

const currentIndex = ref(0);
const activeViewTab = ref("json");

watch(
  () => props.show,
  (show) => {
    if (show) {
      currentIndex.value = props.initialIndex;
      activeViewTab.value = "json";
    }
  },
);

const currentDoc = computed(() => props.documents[currentIndex.value] ?? null);

const titleId = computed(() => {
  if (!currentDoc.value) return "";
  const id = currentDoc.value._id;
  if (!id) return "";
  if (typeof id === "object" && id !== null && (id as Record<string, unknown>).$oid) {
    return `ObjectId("${(id as Record<string, unknown>).$oid}")`;
  }
  return String(id);
});

const title = computed(() =>
  `JSON Viewer — _id:${titleId.value}`,
);

// Shell 格式 JSON
function toShellValue(val: unknown): string {
  if (val === null || val === undefined) return "null";
  const type = getBsonType(val);
  const obj = val as Record<string, unknown>;
  switch (type) {
    case "ObjectId": return `ObjectId("${obj.$oid ?? val}")`;
    case "Date": {
      const d = obj.$date;
      if (typeof d === "string") return `ISODate("${d}")`;
      if (typeof d === "object" && d && (d as Record<string, unknown>).$numberLong)
        return `ISODate("${new Date(parseInt(String((d as Record<string, unknown>).$numberLong))).toISOString()}")`;
      if (typeof d === "number") return `ISODate("${new Date(d).toISOString()}")`;
      return `ISODate("${d}")`;
    }
    case "Int64": return `NumberLong("${obj.$numberLong ?? val}")`;
    case "Decimal128": return `NumberDecimal("${obj.$numberDecimal ?? val}")`;
    case "String": return `"${String(val).replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
    case "Boolean": return String(val);
    case "Int32":
      if (typeof val === "object" && obj.$numberInt) return String(obj.$numberInt);
      return String(val);
    case "Double": return String(val);
    default: return String(val);
  }
}

function toShellString(val: unknown, indent: number): string {
  const pad = "  ".repeat(indent);
  const pad1 = "  ".repeat(indent + 1);
  if (val === null || val === undefined) return "null";
  const type = getBsonType(val);
  if (type !== "Document" && type !== "Array") return toShellValue(val);
  if (Array.isArray(val)) {
    if (val.length === 0) return "[]";
    return `[\n${val.map((item) => `${pad1}${toShellString(item, indent + 1)}`).join(",\n")}\n${pad}]`;
  }
  const entries = Object.entries(val as Record<string, unknown>);
  if (entries.length === 0) return "{}";
  const lines = entries.map(([k, v]) => {
    const ks = /[^a-zA-Z0-9_$]/.test(k) ? `"${k}"` : k;
    return `${pad1}${ks}: ${toShellString(v, indent + 1)}`;
  });
  return `{\n${lines.join(",\n")}\n${pad}}`;
}

function escHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function colorSpan(text: string, color: string): string {
  return `<span style="color:${color}">${escHtml(text)}</span>`;
}

function highlightLine(line: string): string {
  const parts: string[] = [];
  const tokenRe = /ObjectId\("[0-9a-fA-F]*"\)|ISODate\("[^"]*"\)|NumberLong\("[^"]*"\)|NumberDecimal\("[^"]*"\)|"(?:[^"\\]|\\.)*"(?=\s*:)|"(?:[^"\\]|\\.)*"|(?:true|false)\b|null\b|\d+(?:\.\d+)?|[a-zA-Z_$][a-zA-Z0-9_$]*(?=\s*:)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = tokenRe.exec(line)) !== null) {
    if (m.index > last) parts.push(escHtml(line.slice(last, m.index)));
    last = m.index + m[0].length;
    const token = m[0];
    if (token.startsWith("ObjectId(")) parts.push(colorSpan(token, getValueColor("ObjectId")));
    else if (token.startsWith("ISODate(")) parts.push(colorSpan(token, getValueColor("Date")));
    else if (token.startsWith("NumberLong(")) parts.push(colorSpan(token, getValueColor("Int64")));
    else if (token.startsWith("NumberDecimal(")) parts.push(colorSpan(token, getValueColor("Decimal128")));
    else if (token === "true" || token === "false") parts.push(colorSpan(token, getValueColor("Boolean")));
    else if (token === "null") parts.push(`<span style="color:#999;font-style:italic">null</span>`);
    else if (token.startsWith('"')) {
      const afterToken = line.slice(last).trimStart();
      if (afterToken.startsWith(":")) parts.push(colorSpan(token, "#e06c75"));
      else parts.push(colorSpan(token, getValueColor("String")));
    }
    else if (/^\d/.test(token)) parts.push(colorSpan(token, getValueColor("Int32")));
    else parts.push(colorSpan(token, "#e06c75")); // unquoted key
  }
  if (last < line.length) parts.push(escHtml(line.slice(last)));
  return parts.join("");
}

const shellText = computed(() => {
  if (!currentDoc.value) return "";
  try {
    return toShellString(JSON.parse(JSON.stringify(currentDoc.value)), 0);
  } catch {
    return JSON.stringify(currentDoc.value, null, 2);
  }
});

const highlightedLines = computed(() =>
  shellText.value.split("\n").map((line) => highlightLine(line)),
);

const rawJson = computed(() => {
  if (!currentDoc.value) return "";
  try {
    return JSON.stringify(JSON.parse(JSON.stringify(currentDoc.value)), null, 2);
  } catch {
    return String(currentDoc.value);
  }
});

function goPrev() {
  if (currentIndex.value > 0) currentIndex.value--;
}
function goNext() {
  if (currentIndex.value < props.documents.length - 1) currentIndex.value++;
}
function reload() {
  // 重新触发 computed
  const idx = currentIndex.value;
  currentIndex.value = -1;
  setTimeout(() => { currentIndex.value = idx; }, 0);
}
function editInTab() {
  if (!currentDoc.value) return;
  const coll = props.collection || "<collection>";
  const queryText = buildUpdateOneQuery(coll, currentDoc.value);
  emit("editInTab", { doc: currentDoc.value, queryText });
  emit("update:show", false);
}
</script>

<template>
  <n-modal :show="props.show" @update:show="emit('update:show', $event)">
    <n-card
      style="width: min(1100px, 92vw); height: 82vh; display: flex; flex-direction: column"
      :title="title"
      :bordered="false"
      closable
      @close="emit('update:show', false)"
    >
      <div class="viewer-body">
        <n-tabs v-model:value="activeViewTab" type="line" size="small">
          <n-tab-pane name="json" tab="{ } JSON">
            <div class="code-area">
              <div
                v-for="(html, i) in highlightedLines"
                :key="i"
                class="code-line"
              >
                <span class="line-num">{{ i + 1 }}</span>
                <span class="line-content" v-html="html || '&nbsp;'"></span>
              </div>
            </div>
          </n-tab-pane>
          <n-tab-pane name="value" tab="value">
            <pre class="raw-json">{{ rawJson }}</pre>
          </n-tab-pane>
        </n-tabs>
      </div>

      <template #footer>
        <div class="viewer-footer">
          <n-space>
            <n-button size="small" :disabled="currentIndex <= 0" @click="goPrev">
              <template #icon><n-icon><PrevIcon /></n-icon></template>
              Previous
            </n-button>
            <n-button size="small" :disabled="currentIndex >= documents.length - 1" @click="goNext">
              <template #icon><n-icon><NextIcon /></n-icon></template>
              Next
            </n-button>
          </n-space>
          <n-space>
            <n-button size="small" quaternary @click="reload">
              <template #icon><n-icon><ReloadIcon /></n-icon></template>
              Reload
            </n-button>
            <n-button size="small" quaternary @click="editInTab">
              <template #icon><n-icon><EditIcon /></n-icon></template>
              Edit in new tab
            </n-button>
            <n-button size="small" @click="emit('update:show', false)">Cancel</n-button>
          </n-space>
        </div>
      </template>
    </n-card>
  </n-modal>
</template>

<style scoped>
.viewer-body {
  flex: 1;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}
.viewer-body :deep(.n-tabs) {
  height: 100%;
  display: flex;
  flex-direction: column;
}
.viewer-body :deep(.n-tabs-pane-wrapper) {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}
.viewer-body :deep(.n-tab-pane) {
  height: 100%;
  padding: 0;
  overflow: hidden;
}

.code-area {
  height: 100%;
  overflow-y: auto;
  overflow-x: hidden;
  background: #fafafa;
  border: 1px solid #e8e8e8;
  border-radius: 4px;
  padding: 8px 0;
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 13px;
  line-height: 1.5;
}
.code-line {
  display: flex;
  align-items: flex-start;
  padding: 0 8px 0 0;
}
.code-line:hover {
  background: #f0f4f8;
}
.line-num {
  flex: 0 0 auto;
  min-width: 40px;
  padding: 0 8px;
  text-align: right;
  color: #999;
  user-select: none;
  background: #f0f0f0;
  border-right: 1px solid #e0e0e0;
  margin-right: 8px;
}
.line-content {
  flex: 1 1 auto;
  min-width: 0;
  color: #333;
  white-space: pre-wrap;
  word-break: break-word;
  overflow-wrap: anywhere;
}
.raw-json {
  margin: 0;
  padding: 12px;
  height: 100%;
  overflow: auto;
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 13px;
  line-height: 1.5;
  color: #333;
  background: #fafafa;
  border: 1px solid #e8e8e8;
  border-radius: 4px;
  white-space: pre-wrap;
  word-break: break-word;
  overflow-wrap: anywhere;
}

.viewer-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
</style>

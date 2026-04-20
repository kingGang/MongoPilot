<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { NModal, NCard, NButton, NIcon, NSpace } from "naive-ui";
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

const rawLines = computed(() => shellText.value.split("\n"));

const highlightedLines = computed(() => rawLines.value.map((line) => highlightLine(line)));

/** 可折叠块: startLine/endLine 都是 rawLines 的 0 基下标, closeChar 是结束字符 */
interface FoldRange {
  startLine: number;
  endLine: number;
  closeChar: string; // } / ] / 引号
}

/** 扫 raw 文本, 找到跨行的 {...} / [...] / "..." 作为可折叠块 */
const foldRanges = computed<FoldRange[]>(() => {
  const lines = rawLines.value;
  const out: FoldRange[] = [];
  const stack: { ch: string; line: number }[] = [];
  let inString = false;
  let strCh = "";
  let strStart = -1;

  for (let li = 0; li < lines.length; li++) {
    const line = lines[li];
    for (let ci = 0; ci < line.length; ci++) {
      const ch = line[ci];
      if (inString) {
        if (ch === "\\") { ci++; continue; }
        if (ch === strCh) {
          if (strStart >= 0 && strStart < li) {
            out.push({ startLine: strStart, endLine: li, closeChar: strCh });
          }
          inString = false;
          strStart = -1;
        }
      } else {
        if (ch === '"' || ch === "'") {
          inString = true;
          strCh = ch;
          strStart = li;
        } else if (ch === "{" || ch === "[") {
          stack.push({ ch, line: li });
        } else if (ch === "}" || ch === "]") {
          const open = stack.pop();
          if (open && open.line < li) {
            out.push({
              startLine: open.line,
              endLine: li,
              closeChar: ch,
            });
          }
        }
      }
    }
  }
  return out;
});

/** startLine -> FoldRange 的映射, 便于 O(1) 查找 */
const foldByStart = computed(() => {
  const m = new Map<number, FoldRange>();
  for (const f of foldRanges.value) m.set(f.startLine, f);
  return m;
});

/** 当前处于折叠态的起始行集合 */
const foldedStarts = ref<Set<number>>(new Set());

/** 当前被任一折叠块隐藏的行集合 (start+1 ~ end, 含 end) */
const hiddenLines = computed(() => {
  const s = new Set<number>();
  for (const startLine of foldedStarts.value) {
    const f = foldByStart.value.get(startLine);
    if (!f) continue;
    for (let i = f.startLine + 1; i <= f.endLine; i++) s.add(i);
  }
  return s;
});

function toggleFold(line: number) {
  if (!foldByStart.value.has(line)) return;
  const next = new Set(foldedStarts.value);
  if (next.has(line)) next.delete(line);
  else next.add(line);
  foldedStarts.value = next;
}

/** 折叠时, 把起始行末尾追加 ` … <close>` 让视觉上自洽 */
function foldSuffix(line: number): string {
  if (!foldedStarts.value.has(line)) return "";
  const f = foldByStart.value.get(line);
  if (!f) return "";
  // 尝试带上原闭合行末尾的逗号
  const endLine = rawLines.value[f.endLine] || "";
  const trailingComma = /,\s*$/.test(endLine) ? "," : "";
  return `<span class="fold-ellipsis"> … </span>${escapeHtmlInline(f.closeChar)}${trailingComma}`;
}

function escapeHtmlInline(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** 在每次切换文档时重置折叠状态 */
watch(currentDoc, () => {
  foldedStarts.value = new Set();
});

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
        <div class="tabs-header">
          <button
            class="tab-btn"
            :class="{ active: activeViewTab === 'json' }"
            @click="activeViewTab = 'json'"
          >{ } JSON</button>
          <button
            class="tab-btn"
            :class="{ active: activeViewTab === 'value' }"
            @click="activeViewTab = 'value'"
          >value</button>
        </div>
        <div class="tab-body">
          <div v-if="activeViewTab === 'json'" class="code-area">
            <div
              v-for="(html, i) in highlightedLines"
              v-show="!hiddenLines.has(i)"
              :key="i"
              class="code-line"
            >
              <div class="line-num">{{ i + 1 }}</div>
              <div class="line-content">
                <span
                  v-if="foldByStart.has(i)"
                  class="fold-toggle"
                  :title="foldedStarts.has(i) ? '展开' : '折叠'"
                  @click="toggleFold(i)"
                >{{ foldedStarts.has(i) ? '▶' : '▼' }}</span>
                <span v-else class="fold-toggle-placeholder" />
                <span v-html="(html || '&nbsp;') + foldSuffix(i)"></span>
              </div>
            </div>
          </div>
          <pre v-else class="raw-json">{{ rawJson }}</pre>
        </div>
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
  flex: 1 1 0;
  min-height: 0;
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}
/* Naive UI v2 里内容 slot 包裹类名是 .n-card-content (单横线, 不是 __content) */
:deep(.n-card-content) {
  flex: 1 1 0 !important;
  min-height: 0 !important;
  min-width: 0 !important;
  max-width: 100%;
  padding: 0 !important;
  overflow: hidden !important;
  display: flex;
  flex-direction: column;
}
.tabs-header {
  flex: 0 0 auto;
  display: flex;
  gap: 0;
  border-bottom: 1px solid #e0e0e0;
  padding: 0 4px;
  background: #fafafa;
}
.tab-btn {
  background: transparent;
  border: none;
  padding: 8px 14px;
  margin: 0;
  font-size: 13px;
  color: #666;
  cursor: pointer;
  border-bottom: 2px solid transparent;
  font-family: inherit;
}
.tab-btn:hover { color: #333; background: #f0f0f0; }
.tab-btn.active {
  color: #18a058;
  border-bottom-color: #18a058;
  font-weight: 500;
}
.tab-body {
  /* flex 占满 viewer-body 剩余高度, 作为内部 absolute 子项的定位参考 */
  flex: 1 1 0;
  min-height: 0;
  position: relative;
  overflow: hidden;
}

.code-area {
  /* absolute: 相对于 .tab-body (position:relative) 铺满 */
  display: grid;
  grid-template-columns: 48px minmax(0, 1fr);
  align-content: start;
  position: absolute;
  inset: 0;
  box-sizing: border-box;
  /* 只纵向滚动; 横向绝不滚, 强制换行 */
  overflow-x: hidden;
  overflow-y: auto;
  background: #fafafa;
  border: 1px solid #e8e8e8;
  border-radius: 4px;
  padding: 8px 0;
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 13px;
  line-height: 1.5;
}
.code-line {
  display: contents;
}
.code-line:hover .line-num,
.code-line:hover .line-content {
  background: #f0f4f8;
}
.line-num {
  padding: 0 8px;
  text-align: right;
  color: #999;
  user-select: none;
  background: #f0f0f0;
  border-right: 1px solid #e0e0e0;
}
.line-content {
  padding: 0 12px 0 8px;
  min-width: 0;
  max-width: 100%;
  color: #333;
  /* 保留缩进 + 强制任意处换行 (含长 URL / 不带空格的长串) */
  white-space: pre-wrap;
  word-break: break-all;
  overflow-wrap: anywhere;
  box-sizing: border-box;
}
.fold-toggle {
  display: inline-block;
  width: 12px;
  font-size: 9px;
  color: #3875d7;
  cursor: pointer;
  user-select: none;
  margin-right: 2px;
  vertical-align: middle;
}
.fold-toggle:hover { color: #18a058; }
.fold-toggle-placeholder {
  display: inline-block;
  width: 12px;
  margin-right: 2px;
}
:deep(.fold-ellipsis) {
  color: #999;
  font-style: italic;
  background: #f0f0f0;
  padding: 0 4px;
  margin: 0 2px;
  border-radius: 3px;
}
.raw-json {
  margin: 0;
  padding: 12px;
  position: absolute;
  inset: 0;
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

<script setup lang="ts">
import { ref, computed, toRaw } from "vue";
import { NButton, NIcon, NSpace } from "naive-ui";
import { Copy as CopyIcon, Expand as ExpandIcon, Contract as CollapseIcon } from "@vicons/ionicons5";
import { getBsonType, getValueColor } from "@/utils/bson-format";

const props = defineProps<{
  documents: Record<string, unknown>[];
  rowOffset?: number;
}>();

const collapsed = ref<Set<number>>(new Set());

function toggleDoc(index: number) {
  const next = new Set(collapsed.value);
  if (next.has(index)) next.delete(index);
  else next.add(index);
  collapsed.value = next;
}

function expandAll() {
  collapsed.value = new Set();
}

function collapseAll() {
  collapsed.value = new Set(props.documents.map((_, i) => i));
}

function copyAll() {
  const text = props.documents
    .map((doc, i) => `/* ${i + 1} */\n${toShellString(toRaw(doc), 0)}`)
    .join(",\n\n");
  navigator.clipboard.writeText(text);
}

// ---- Shell 格式化 ----

function esc(s: string): string {
  return s.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
}

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
    case "String": return `"${esc(String(val))}"`;
    case "Boolean": return String(val);
    case "Int32":
      if (typeof val === "object" && obj.$numberInt) return String(obj.$numberInt);
      return String(val);
    case "Double": return String(val);
    case "Regex": {
      const re = obj.$regex ?? (obj.$regularExpression as Record<string, unknown>)?.pattern;
      return `/${re}/`;
    }
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
    const ks = /[^a-zA-Z0-9_$]/.test(k) ? `"${esc(k)}"` : k;
    return `${pad1}${ks}: ${toShellString(v, indent + 1)}`;
  });
  return `{\n${lines.join(",\n")}\n${pad}}`;
}

// ---- HTML 语法高亮 ----

function escHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function span(text: string, color: string): string {
  return `<span style="color:${color}">${escHtml(text)}</span>`;
}

function highlightShell(text: string): string {
  // 逐行处理，每行用正则匹配 token
  return text.split("\n").map(highlightLine).join("\n");
}

function highlightLine(line: string): string {
  const parts: string[] = [];
  // 匹配各种 token
  const tokenRe = /ObjectId\("[0-9a-fA-F]*"\)|ISODate\("[^"]*"\)|NumberLong\("[^"]*"\)|NumberDecimal\("[^"]*"\)|"(?:[^"\\]|\\.)*"(?=\s*:)|"(?:[^"\\]|\\.)*"|(?:true|false)\b|null\b|\d+(?:\.\d+)?|[a-zA-Z_$][a-zA-Z0-9_$]*(?=\s*:)/g;

  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = tokenRe.exec(line)) !== null) {
    // 未匹配的部分（标点、空白、冒号等）
    if (m.index > last) {
      parts.push(escHtml(line.slice(last, m.index)));
    }
    last = m.index + m[0].length;
    const token = m[0];

    if (token.startsWith("ObjectId(")) {
      parts.push(span(token, getValueColor("ObjectId")));
    } else if (token.startsWith("ISODate(")) {
      parts.push(span(token, getValueColor("Date")));
    } else if (token.startsWith("NumberLong(")) {
      parts.push(span(token, getValueColor("Int64")));
    } else if (token.startsWith("NumberDecimal(")) {
      parts.push(span(token, getValueColor("Decimal128")));
    } else if (token === "true" || token === "false") {
      parts.push(span(token, getValueColor("Boolean")));
    } else if (token === "null") {
      parts.push(`<span style="color:#999;font-style:italic">null</span>`);
    } else if (token.startsWith('"')) {
      // 看后面是否跟 colon → key vs value
      const afterToken = line.slice(last).trimStart();
      if (afterToken.startsWith(":")) {
        parts.push(span(token, "#e06c75")); // key
      } else {
        parts.push(span(token, getValueColor("String"))); // value
      }
    } else if (/^\d/.test(token)) {
      parts.push(span(token, getValueColor("Int32")));
    } else {
      // unquoted key (matched by lookahead (?=\s*:))
      parts.push(span(token, "#e06c75"));
    }
  }
  // 剩余未匹配文本
  if (last < line.length) {
    parts.push(escHtml(line.slice(last)));
  }
  return parts.join("");
}

// 预计算——用 JSON 深拷贝彻底脱离 Vue proxy
const docHtmlList = computed(() => {
  try {
    const rawDocs = JSON.parse(JSON.stringify(props.documents)) as Record<string, unknown>[];
    return rawDocs.map((doc) => {
      try {
        return highlightShell(toShellString(doc, 0));
      } catch {
        return escHtml(JSON.stringify(doc, null, 2));
      }
    });
  } catch {
    return props.documents.map((doc) => escHtml(JSON.stringify(doc, null, 2)));
  }
});
</script>

<template>
  <div class="json-view">
    <div class="json-toolbar">
      <n-space size="small">
        <n-button size="tiny" quaternary @click="expandAll()">
          <template #icon><n-icon :size="14"><ExpandIcon /></n-icon></template>
          展开全部
        </n-button>
        <n-button size="tiny" quaternary @click="collapseAll()">
          <template #icon><n-icon :size="14"><CollapseIcon /></n-icon></template>
          折叠全部
        </n-button>
        <n-button size="tiny" quaternary @click="copyAll()">
          <template #icon><n-icon :size="14"><CopyIcon /></n-icon></template>
          复制
        </n-button>
      </n-space>
    </div>
    <div class="json-body">
      <div v-for="(_, index) in documents" :key="index" class="doc-block">
        <div class="doc-comment" @click="toggleDoc(index)">
          <span class="toggle-icon">{{ collapsed.has(index) ? '▶' : '▼' }}</span>
          <span>/* {{ (rowOffset ?? 0) + index + 1 }} */</span>
        </div>
        <pre
          v-if="!collapsed.has(index)"
          class="doc-code"
          v-html="docHtmlList[index]"
        ></pre>
      </div>
    </div>
  </div>
</template>

<style scoped>
.json-view {
  height: 100%;
  display: flex;
  flex-direction: column;
}
.json-toolbar {
  padding: 4px 8px;
  border-bottom: 1px solid var(--n-border-color);
  flex-shrink: 0;
}
.json-body {
  flex: 1;
  overflow: auto;
  background: #fff;
  padding: 4px 0;
}
.doc-block {
  margin-bottom: 2px;
}
.doc-comment {
  padding: 2px 12px;
  cursor: pointer;
  color: #999;
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 13px;
  font-style: italic;
  user-select: none;
}
.doc-comment:hover {
  background: rgba(0, 0, 0, 0.04);
}
.toggle-icon {
  font-size: 9px;
  margin-right: 6px;
  font-style: normal;
}
.doc-code {
  margin: 0;
  padding: 0 12px 8px;
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 13px;
  line-height: 1.6;
  color: #333;
  border-bottom: 1px solid #eee;
  white-space: pre-wrap;
  word-break: break-all;
}
</style>

<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { NModal, NCard, NSelect, NButton, NSpace, NAlert } from "naive-ui";
import { getBsonType, getValueColor } from "@/utils/bson-format";
import * as docApi from "@/api/document";
import { useEditorStore } from "@/stores/editor";

const props = defineProps<{
  show: boolean;
  field: string;
  value: unknown;
  // 保存到数据库需要的上下文
  connectionId?: string;
  database?: string;
  collection?: string;
  documentId?: string;
  document?: Record<string, unknown>;
}>();

const emit = defineEmits<{
  "update:show": [value: boolean];
  saved: [];
}>();

const editorStore = useEditorStore();

const typeName = ref("");
const editText = ref("");
const validationError = ref<string | null>(null);
const saving = ref(false);

// Overlay editor refs
const textareaRef = ref<HTMLTextAreaElement | null>(null);
const highlightRef = ref<HTMLPreElement | null>(null);
const lineNumRef = ref<HTMLElement | null>(null);

const typeOptions = [
  { label: "String", value: "String" },
  { label: "ObjectId", value: "ObjectId" },
  { label: "Bool", value: "Boolean" },
  { label: "Int32", value: "Int32" },
  { label: "Int64", value: "Int64" },
  { label: "Double", value: "Double" },
  { label: "Decimal", value: "Decimal128" },
  { label: "Date", value: "Date" },
  { label: "Timestamp", value: "Timestamp" },
  { label: "Array", value: "Array" },
  { label: "Object", value: "Document" },
  { label: "LUUID", value: "LUUID" },
  { label: "UUID", value: "UUID" },
  { label: "MD5", value: "MD5" },
  { label: "RegEx", value: "Regex" },
  { label: "Binary", value: "Binary" },
  { label: "Code", value: "Code" },
  { label: "Code(with scope)", value: "CodeWithScope" },
  { label: "Reference", value: "DBRef" },
  { label: "Null", value: "Null" },
  { label: "Symbol", value: "Symbol" },
  { label: "MinKey", value: "MinKey" },
  { label: "MaxKey", value: "MaxKey" },
];

watch(
  () => props.show,
  (show) => {
    if (show) {
      typeName.value = getBsonType(props.value);
      validationError.value = null;
      try {
        if (typeName.value === "String") {
          editText.value = String(props.value ?? "");
        } else {
          editText.value = JSON.stringify(props.value, null, 2);
        }
      } catch {
        editText.value = String(props.value);
      }
    }
  },
);

// 实时校验
watch([() => editText.value, () => typeName.value], () => {
  validationError.value = validateValue(editText.value, typeName.value);
});

function validateValue(text: string, type: string): string | null {
  const trimmed = text.trim();
  if (!trimmed && type !== "String" && type !== "Null") {
    return "值不能为空";
  }
  switch (type) {
    case "String":
      return null;
    case "Int32": {
      const n = Number(trimmed);
      if (!Number.isInteger(n)) return "Int32 必须是整数";
      if (n < -2147483648 || n > 2147483647) return "Int32 超出范围 (-2^31 ~ 2^31-1)";
      return null;
    }
    case "Int64": {
      if (!/^-?\d+$/.test(trimmed)) return "Int64 必须是整数";
      return null;
    }
    case "Double": {
      if (isNaN(Number(trimmed))) return "Double 必须是数字";
      return null;
    }
    case "Decimal128": {
      if (isNaN(Number(trimmed))) return "Decimal 必须是数字";
      return null;
    }
    case "Boolean":
      if (trimmed !== "true" && trimmed !== "false") return "Bool 必须是 true 或 false";
      return null;
    case "Null":
      if (trimmed !== "null" && trimmed !== "") return "Null 类型值必须为 null";
      return null;
    case "ObjectId":
      if (!/^[0-9a-fA-F]{24}$/.test(trimmed)) return "ObjectId 必须是 24 位十六进制字符串";
      return null;
    case "Date":
      if (isNaN(new Date(trimmed).getTime())) return "Date 格式无效，示例: 2024-01-01T00:00:00Z";
      return null;
    case "Document": {
      try {
        const parsed = JSON.parse(trimmed);
        if (typeof parsed !== "object" || Array.isArray(parsed) || parsed === null)
          return "Object 类型的值必须是 JSON 对象 {}";
      } catch {
        return "JSON 格式错误";
      }
      return null;
    }
    case "Array": {
      try {
        const parsed = JSON.parse(trimmed);
        if (!Array.isArray(parsed)) return "Array 类型的值必须是 JSON 数组 []";
      } catch {
        return "JSON 格式错误";
      }
      return null;
    }
    case "Regex":
      if (!trimmed.startsWith("/")) return "RegEx 格式: /pattern/flags";
      return null;
    default:
      return null;
  }
}

function buildBsonValue(text: string, type: string): unknown {
  const trimmed = text.trim();
  switch (type) {
    case "String": return trimmed;
    case "Int32": return parseInt(trimmed);
    case "Double": return parseFloat(trimmed);
    case "Boolean": return trimmed === "true";
    case "Null": return null;
    case "Int64": return { $numberLong: trimmed };
    case "Decimal128": return { $numberDecimal: trimmed };
    case "ObjectId": return { $oid: trimmed };
    case "Date": return { $date: new Date(trimmed).toISOString() };
    case "Document":
    case "Array":
      return JSON.parse(trimmed);
    case "Regex": {
      const m = trimmed.match(/^\/(.*)\/([gimsuy]*)$/);
      if (m) return { $regex: m[1], $options: m[2] };
      return { $regex: trimmed };
    }
    case "MinKey": return { $minKey: 1 };
    case "MaxKey": return { $maxKey: 1 };
    default:
      try { return JSON.parse(trimmed); }
      catch { return trimmed; }
  }
}

async function handleSave() {
  const error = validateValue(editText.value, typeName.value);
  if (error) {
    validationError.value = error;
    return;
  }

  const newValue = buildBsonValue(editText.value, typeName.value);

  if (props.connectionId && props.database && props.collection && props.document) {
    saving.value = true;
    try {
      const updatedDoc = { ...JSON.parse(JSON.stringify(props.document)), [props.field]: newValue };
      const idStr = props.documentId || extractId(props.document);
      await docApi.updateDocument(props.connectionId, props.database, props.collection, idStr, updatedDoc);
      emit("saved");
      emit("update:show", false);
    } catch (e) {
      validationError.value = `保存失败: ${e}`;
    } finally {
      saving.value = false;
    }
  } else {
    emit("saved");
    emit("update:show", false);
  }
}

function extractId(doc: Record<string, unknown>): string {
  const id = doc._id;
  if (typeof id === "object" && id !== null) {
    return String((id as Record<string, unknown>).$oid ?? JSON.stringify(id));
  }
  return String(id);
}

// ---- Syntax Highlighting (overlay) ----

function escHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

const highlightedHtml = computed(() => {
  const text = editText.value;
  const type = typeName.value;

  // Document/Array → JSON 语法高亮
  if (type === "Document" || type === "Array") {
    return text.split("\n").map(highlightJsonLine).join("\n");
  }

  // 简单类型 → 单色
  const color = getValueColor(type);
  return `<span style="color:${color || '#333'}">${escHtml(text)}</span>`;
});

function highlightJsonLine(line: string): string {
  const parts: string[] = [];
  const tokenRe = /"(?:[^"\\]|\\.)*"(?=\s*:)|"(?:[^"\\]|\\.)*"|(?:true|false)\b|null\b|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/g;

  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = tokenRe.exec(line)) !== null) {
    if (m.index > last) parts.push(escHtml(line.slice(last, m.index)));
    last = m.index + m[0].length;
    const token = m[0];

    if (token.startsWith('"')) {
      const after = line.slice(last).trimStart();
      if (after.startsWith(":")) {
        // key
        parts.push(`<span style="color:#e06c75">${escHtml(token)}</span>`);
      } else {
        // string value
        parts.push(`<span style="color:#98c379">${escHtml(token)}</span>`);
      }
    } else if (token === "true" || token === "false") {
      parts.push(`<span style="color:#56b6c2">${escHtml(token)}</span>`);
    } else if (token === "null") {
      parts.push(`<span style="color:#999;font-style:italic">${escHtml(token)}</span>`);
    } else if (/^-?\d/.test(token)) {
      parts.push(`<span style="color:#d19a66">${escHtml(token)}</span>`);
    } else {
      parts.push(escHtml(token));
    }
  }
  if (last < line.length) parts.push(escHtml(line.slice(last)));
  return parts.join("");
}

function syncScroll() {
  if (textareaRef.value && highlightRef.value) {
    highlightRef.value.scrollTop = textareaRef.value.scrollTop;
    highlightRef.value.scrollLeft = textareaRef.value.scrollLeft;
  }
  if (textareaRef.value && lineNumRef.value) {
    lineNumRef.value.scrollTop = textareaRef.value.scrollTop;
  }
}

// ---- View Generated Script ----

const canViewScript = computed(() =>
  !!(props.connectionId && props.database && props.collection && props.document),
);

function escStr(s: string): string {
  return s.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
}

function toShellVal(val: unknown): string {
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
      return `ISODate("${d}")`;
    }
    case "Int64": return `NumberLong("${obj.$numberLong ?? val}")`;
    case "Decimal128": return `NumberDecimal("${obj.$numberDecimal ?? val}")`;
    case "String": return `"${escStr(String(val))}"`;
    case "Boolean": return String(val);
    case "Int32": return String(typeof val === "object" ? obj.$numberInt ?? val : val);
    case "Double": return String(val);
    default: return String(val);
  }
}

function toShellStr(val: unknown, indent: number): string {
  const pad = "    ".repeat(indent);
  const pad1 = "    ".repeat(indent + 1);
  if (val === null || val === undefined) return "null";
  const type = getBsonType(val);
  if (type !== "Document" && type !== "Array") return toShellVal(val);
  if (Array.isArray(val)) {
    if (val.length === 0) return "[]";
    return `[\n${val.map((item) => `${pad1}${toShellStr(item, indent + 1)}`).join(",\n")}\n${pad}]`;
  }
  const entries = Object.entries(val as Record<string, unknown>);
  if (entries.length === 0) return "{}";
  const lines = entries.map(([k, v]) =>
    `${pad1}"${escStr(k)}": ${toShellStr(v, indent + 1)}`,
  );
  return `{\n${lines.join(",\n")}\n${pad}}`;
}

function handleViewScript() {
  if (!canViewScript.value) return;

  const error = validateValue(editText.value, typeName.value);
  if (error) {
    validationError.value = error;
    return;
  }

  const newValue = buildBsonValue(editText.value, typeName.value);
  const doc = props.document!;
  const id = doc._id;
  const idType = getBsonType(id);

  let idStr: string;
  if (idType === "ObjectId") {
    idStr = `ObjectId("${(id as Record<string, unknown>).$oid || id}")`;
  } else if (idType === "String") {
    idStr = `"${escStr(String(id))}"`;
  } else if (idType === "Int64") {
    idStr = `NumberLong("${(id as Record<string, unknown>).$numberLong}")`;
  } else {
    idStr = JSON.stringify(id);
  }

  const valueStr = toShellStr(newValue, 3);

  const script = `db.getCollection("${props.collection}").updateOne({ _id: ${idStr} },\n    {\n        $set: {\n            "${props.field}": ${valueStr}\n        }\n    }\n)`;

  const tabId = editorStore.createTab(props.connectionId!, props.database!, props.collection);
  editorStore.setContent(tabId, script);
  emit("update:show", false);
}

// 行号
const lineCount = computed(() => editText.value.split("\n").length);
</script>

<template>
  <n-modal :show="props.show" @update:show="emit('update:show', $event)">
    <n-card
      style="width: 680px; max-height: 85vh; display: flex; flex-direction: column"
      title="Editor: Type and Value"
      :bordered="false"
      closable
      @close="emit('update:show', false)"
    >
      <div class="detail-form">
        <div class="form-row">
          <label class="form-label">Field</label>
          <div class="form-value field-name">{{ field }}</div>
        </div>
        <div class="form-row">
          <label class="form-label">Type</label>
          <div class="form-value">
            <n-select
              v-model:value="typeName"
              :options="typeOptions"
              size="small"
            />
          </div>
        </div>
        <div class="form-row">
          <label class="form-label">Value</label>
        </div>

        <n-alert v-if="validationError" type="error" style="margin-bottom: 8px" :bordered="false">
          {{ validationError }}
        </n-alert>

        <div class="editor-wrapper" :class="{ 'has-error': !!validationError }">
          <div ref="lineNumRef" class="line-numbers">
            <div v-for="n in lineCount" :key="n" class="line-num">{{ n }}</div>
          </div>
          <div class="editor-overlay">
            <pre
              ref="highlightRef"
              class="editor-highlight"
              v-html="highlightedHtml"
            ></pre>
            <textarea
              ref="textareaRef"
              v-model="editText"
              class="code-editor"
              spellcheck="false"
              wrap="off"
              @scroll="syncScroll"
              @keydown.ctrl.s.prevent="handleSave"
            />
          </div>
        </div>
      </div>
      <template #footer>
        <div class="dialog-footer">
          <n-button size="small" quaternary :disabled="!canViewScript" @click="handleViewScript">
            View generated Script...
          </n-button>
          <n-space>
            <n-button size="small" type="primary" :loading="saving" :disabled="!!validationError" @click="handleSave">
              Save (Ctrl+s)
            </n-button>
            <n-button size="small" @click="emit('update:show', false)">Cancel</n-button>
          </n-space>
        </div>
      </template>
    </n-card>
  </n-modal>
</template>

<style scoped>
.detail-form {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.form-row {
  display: flex;
  align-items: center;
  gap: 12px;
}
.form-label {
  font-weight: 600;
  font-size: 13px;
  color: #666;
  min-width: 45px;
}
.form-value {
  flex: 1;
}
.field-name {
  font-size: 13px;
  color: #333;
}
.editor-wrapper {
  display: flex;
  border: 1px solid #d9d9d9;
  border-radius: 4px;
  overflow: hidden;
  min-height: 200px;
  max-height: 50vh;
  background: #fff;
}
.editor-wrapper.has-error {
  border-color: #e03e3e;
}
.line-numbers {
  padding: 8px 0;
  background: #f5f5f5;
  border-right: 1px solid #e8e8e8;
  text-align: right;
  user-select: none;
  min-width: 36px;
  overflow: hidden;
}
.line-num {
  padding: 0 8px;
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 13px;
  line-height: 1.5;
  color: #999;
}
/* Overlay editor: highlighted pre behind, transparent textarea on top */
.editor-overlay {
  flex: 1;
  position: relative;
  overflow: hidden;
}
.editor-highlight {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  padding: 8px;
  margin: 0;
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 13px;
  line-height: 1.5;
  white-space: pre;
  overflow: auto;
  pointer-events: none;
  background: #fff;
  color: #333;
}
.code-editor {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  width: 100%;
  height: 100%;
  padding: 8px;
  border: none;
  outline: none;
  resize: none;
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 13px;
  line-height: 1.5;
  background: transparent;
  color: transparent;
  caret-color: #333;
  overflow: auto;
  white-space: pre;
  tab-size: 2;
  z-index: 1;
  box-sizing: border-box;
}
.dialog-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
</style>

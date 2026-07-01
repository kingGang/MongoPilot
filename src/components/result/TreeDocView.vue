<script setup lang="ts">
import { ref, computed, watch, nextTick } from "vue";
import { NTooltip, NDropdown, NCheckbox, useMessage } from "naive-ui";
import { getBsonType, formatTreeValue, extractIdDisplay, getTypeColor, getValueColor } from "@/utils/bson-format";
import { highlightKeyword } from "@/utils/text-highlight";
import ValueDetail from "./ValueDetail.vue";
import DocumentViewer from "./DocumentViewer.vue";
import * as docApi from "@/api/document";
import {
  buildUpdateOneQuery,
  buildInsertOneQuery,
  buildDeleteOneQuery,
  buildFindByIdQuery,
} from "@/utils/mongo-shell-format";

const props = defineProps<{
  documents: Record<string, unknown>[];
  rowOffset?: number;
  /** 数据库写回需要的上下文 (打开 ValueDetail 编辑后保存用) */
  connectionId?: string;
  database?: string;
  collection?: string;
  /** 只读连接 —— 禁止内联编辑 / 打开 ValueDetail */
  readOnly?: boolean;
  docKeyFn?: (doc: Record<string, unknown>) => string | null;
  selectedKeys?: Set<string>;
  /** 已编辑字段集合, 元素格式: `${docKey}::${field}` —— 用来给单元格画 dirty 标识 */
  dirtyFields?: Set<string>;
  searchKeyword?: string;
  matchCase?: boolean;
  activeMatchDocIndex?: number;
  matchDocIndexes?: number[];
}>();

const emit = defineEmits<{
  toggleSelect: [key: string];
  setSelection: [keys: string[]];
  editInTab: [payload: { doc: Record<string, unknown>; queryText: string }];
  /** 用户保存了改动 (本地已 mutate, 无需父组件重查) —— 仅作为可选信号 */
  updated: [];
  /** 编辑成功 -> 上报 (docKey, field) 让父组件维护 dirty 集合 */
  dirty: [docKey: string, field: string];
}>();

// 是否启用多选 UI (需父组件提供 key 函数)
const selectionEnabled = computed(() => !!props.docKeyFn);

function docKey(docIdx: number): string | null {
  const fn = props.docKeyFn;
  if (!fn) return null;
  const doc = props.documents[docIdx];
  return doc ? fn(doc) : null;
}

function isRowChecked(docIdx: number): boolean {
  if (!props.selectedKeys) return false;
  const k = docKey(docIdx);
  return k !== null && props.selectedKeys.has(k);
}

// 全选状态: 仅统计有合法 id 的文档
const selectableKeys = computed<string[]>(() => {
  if (!props.docKeyFn) return [];
  const out: string[] = [];
  for (const d of props.documents) {
    const k = props.docKeyFn(d);
    if (k) out.push(k);
  }
  return out;
});
const allChecked = computed(() =>
  selectableKeys.value.length > 0
  && props.selectedKeys
  && selectableKeys.value.every((k) => props.selectedKeys!.has(k)),
);
const someChecked = computed(() =>
  !allChecked.value
  && !!props.selectedKeys
  && selectableKeys.value.some((k) => props.selectedKeys!.has(k)),
);

/** 转发 DocumentViewer 的 editInTab —— 写在 script 里避免 template 泛型 < 被 vue-tsc 误读 */
function forwardEditInTab(payload: { doc: Record<string, unknown>; queryText: string }) {
  emit("editInTab", payload);
}

function toggleRow(docIdx: number) {
  const k = docKey(docIdx);
  if (!k) return;
  emit("toggleSelect", k);
}

function toggleAll() {
  if (allChecked.value) emit("setSelection", []);
  else emit("setSelection", selectableKeys.value);
}

// ---- 搜索高亮 ----
const matchSet = computed(() => new Set(props.matchDocIndexes ?? []));

function hl(text: string): string {
  if (!props.searchKeyword) return text;
  return highlightKeyword(text, props.searchKeyword, !!props.matchCase);
}

// 发现 activeMatchDocIndex 改变 -> 自动展开该文档并滚动到可视区
watch(
  () => props.activeMatchDocIndex,
  async (docIdx) => {
    if (docIdx === undefined || docIdx < 0) return;
    // 展开该文档, 方便看到内部哪一行匹配
    const docPath = `doc:${docIdx}`;
    if (!expanded.value.has(docPath)) {
      const next = new Set(expanded.value);
      next.add(docPath);
      expanded.value = next;
    }
    await nextTick();
    const el = document.querySelector<HTMLElement>(`tr[data-doc-index="${docIdx}"]`);
    el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  },
);

const expanded = ref<Set<string>>(new Set());
const selectedPath = ref<string | null>(null);

// 值详情 (ValueDetail 弹窗)
const showDetail = ref(false);
const detailField = ref("");
const detailValue = ref<unknown>(null);
/** 当前正在编辑的整文档 + 其 _id 字符串, 传给 ValueDetail 让保存能落库 */
const detailDoc = ref<Record<string, unknown> | undefined>(undefined);
const detailDocId = ref("");

function extractDocId(doc: Record<string, unknown>): string {
  const id = doc._id;
  if (typeof id === "object" && id !== null) {
    return String((id as Record<string, unknown>).$oid ?? JSON.stringify(id));
  }
  return String(id ?? "");
}

/** 上报: 这一行的某字段被改过 -> 父组件加进 dirtyFields */
function emitDirty(docIdx: number, field: string) {
  if (!props.docKeyFn) return;
  const doc = props.documents[docIdx];
  if (!doc) return;
  const k = props.docKeyFn(doc);
  if (k) emit("dirty", k, field);
}

/** 给定一行: 该字段在 dirtyFields 里? */
function isCellDirty(row: RowItem): boolean {
  if (!props.dirtyFields || props.dirtyFields.size === 0) return false;
  if (row.docIndex < 0 || row.isDocRoot || row.key === "_id") return false;
  if (!props.docKeyFn) return false;
  const doc = props.documents[row.docIndex];
  if (!doc) return false;
  const k = props.docKeyFn(doc);
  if (!k) return false;
  return props.dirtyFields.has(`${k}::${row.key}`);
}

/** 只对顶层字段开放编辑: docIndex 跟得上, 非根行, 非 _id (改 _id 危险, 不允许); 只读连接一律不可编辑 */
function isEditableLeaf(row: RowItem): boolean {
  if (props.readOnly) return false;
  return row.docIndex >= 0 && !row.isDocRoot && row.key !== "_id";
}

/** 这些 BSON 类型可以用单行 input/select 内联编辑;
 *  其余 (Null / Undefined / Document / Array / Regex / Binary 等) 必须走 ValueDetail */
function canInlineEdit(type: string): boolean {
  return ["String", "Int32", "Int64", "Double", "Decimal128", "Boolean", "Date", "ObjectId"].includes(type);
}

// ---- 内联编辑状态 ----
const editingDocIndex = ref<number>(-1);
const editingKey = ref<string | null>(null);
const editingType = ref<string>("");
const editingValue = ref<string>("");
const editingOriginal = ref<string>("");

function isEditing(docIdx: number, key: string): boolean {
  return editingDocIndex.value === docIdx && editingKey.value === key;
}

function cancelInlineEdit() {
  editingDocIndex.value = -1;
  editingKey.value = null;
}

/** 双击入口: 简单类型走格子内联; null / 复合类型弹 ValueDetail
 *  只读连接下, 简单类型 (canInlineEdit) 才拒绝; 复合类型 (Document/Array/Regex/Binary/Null) 允许开 ValueDetail 查看. */
function onValueDblclick(row: RowItem, e: MouseEvent) {
  e.stopPropagation();
  if (row.docIndex < 0 || row.isDocRoot || row.key === "_id") return;
  if (canInlineEdit(row.type)) {
    if (props.readOnly) { message.warning("只读连接: 不允许修改文档"); return; }
    startInlineEdit(row);
  } else {
    openValueEditor(row, e);
  }
}

function startInlineEdit(row: RowItem) {
  if (props.readOnly) { message.warning("只读连接: 不允许修改文档"); return; }
  if (!props.connectionId || !props.database || !props.collection) {
    message.warning("缺少存储上下文, 无法编辑");
    return;
  }
  editingDocIndex.value = row.docIndex;
  editingKey.value = row.key;
  editingType.value = row.type;
  const val = row.value;
  const obj = val as Record<string, unknown>;
  let text: string;
  if (row.type === "Boolean") text = String(val);
  else if (row.type === "Int64") text = String(obj?.$numberLong ?? val);
  else if (row.type === "Decimal128") text = String(obj?.$numberDecimal ?? val);
  else if (row.type === "ObjectId") text = String(obj?.$oid ?? val);
  else if (row.type === "Date") {
    const d = obj?.$date;
    let ms: number;
    if (typeof d === "string") ms = new Date(d).getTime();
    else if (typeof d === "number") ms = d;
    else if (typeof d === "object" && d && (d as Record<string, unknown>).$numberLong)
      ms = parseInt(String((d as Record<string, unknown>).$numberLong));
    else ms = Date.now();
    const date = new Date(ms);
    if (isNaN(date.getTime())) text = String(d);
    else {
      const offset = -date.getTimezoneOffset();
      const sign = offset >= 0 ? "+" : "-";
      const pad = (n: number) => String(n).padStart(2, "0");
      const tzH = pad(Math.floor(Math.abs(offset) / 60));
      const tzM = pad(Math.abs(offset) % 60);
      text = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
        + `T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}${sign}${tzH}:${tzM}`;
    }
  } else text = String(val ?? "");
  editingValue.value = text;
  editingOriginal.value = text;
}

async function commitInlineEdit() {
  if (editingDocIndex.value < 0 || editingKey.value === null) return;
  const docIdx = editingDocIndex.value;
  const key = editingKey.value;
  const type = editingType.value;
  const raw = editingValue.value;

  if (raw === editingOriginal.value) { cancelInlineEdit(); return; }

  let finalVal: unknown = raw;
  if (type === "Int32") finalVal = parseInt(raw) || 0;
  else if (type === "Double") finalVal = parseFloat(raw) || 0;
  else if (type === "Boolean") finalVal = raw === "true";
  else if (type === "Int64") finalVal = { $numberLong: raw };
  else if (type === "Decimal128") finalVal = { $numberDecimal: raw };
  else if (type === "ObjectId") {
    if (!/^[0-9a-fA-F]{24}$/.test(raw.trim())) {
      message.error("ObjectId 必须是 24 位十六进制字符串");
      return; // 留在编辑态, 用户修正
    }
    finalVal = { $oid: raw.trim() };
  } else if (type === "Date") {
    const parsed = new Date(raw);
    if (isNaN(parsed.getTime())) {
      message.error(`时间格式错误: "${raw}", 请使用 ISO 格式如 2025-04-01T09:34:00+08:00`);
      return;
    }
    finalVal = { $date: parsed.toISOString() };
  }

  const doc = props.documents[docIdx];
  if (!doc || !props.connectionId || !props.database || !props.collection) {
    cancelInlineEdit();
    return;
  }
  const docId = extractDocId(doc);
  try {
    const updatedDoc = { ...JSON.parse(JSON.stringify(doc)), [key]: finalVal };
    await docApi.updateDocument(props.connectionId, props.database, props.collection, docId, updatedDoc);
    (doc as Record<string, unknown>)[key] = finalVal;
    emitDirty(docIdx, key);
    message.success("已保存");
    emit("updated");
  } catch (e) {
    message.error(`保存失败: ${e}`);
  }
  cancelInlineEdit();
}

/** ValueDetail 保存成功 -> 标 dirty + 通知父组件 */
function onDetailSaved() {
  // 用 detailField/detailDoc 反推 docIdx
  const doc = detailDoc.value;
  if (doc) {
    const idx = props.documents.indexOf(doc);
    if (idx >= 0) emitDirty(idx, detailField.value);
  }
  emit("updated");
}

/** 打开 Type-and-Value 编辑器 (只读模式下 ValueDetail 会自动隐藏 Save 键, 相当于查看器) */
function openValueEditor(row: RowItem, e: MouseEvent) {
  e.stopPropagation();
  if (row.docIndex < 0 || row.isDocRoot || row.key === "_id") return;
  const doc = props.documents[row.docIndex];
  if (!doc) return;
  detailField.value = row.key;
  detailValue.value = row.value;
  detailDoc.value = doc;
  detailDocId.value = extractDocId(doc);
  showDetail.value = true;
}

// 文档查看器
const showDocViewer = ref(false);
const docViewerIndex = ref(0);

// 右键菜单
const message = useMessage();
const showCtxMenu = ref(false);
const ctxMenuX = ref(0);
const ctxMenuY = ref(0);
const ctxRow = ref<RowItem | null>(null);

type CtxItem =
  | { label: string; key: string }
  | { type: "divider"; key: string }
  | { label: string; key: string; children: { label: string; key: string }[] };

const ctxMenuOptions = computed<CtxItem[]>(() => {
  const r = ctxRow.value;
  if (!r) return [];
  const items: CtxItem[] = [{ label: "复制值", key: "copy-value" }];
  if (r.isDocRoot) {
    items.unshift({ label: "复制文档 (JSON)", key: "copy-doc" });
  } else {
    items.push({ label: "复制字段名", key: "copy-key" });
  }
  if (r.isDocRoot) {
    items.push({ label: "查看文档", key: "view-doc" });
    items.push({ type: "divider", key: "d-stmt" });
    // 只读连接: 写操作模板会被后端拒, 干脆只暴露 find
    const genChildren = props.readOnly
      ? [{ label: "find — 按 _id 查询", key: "gen-find" }]
      : [
          { label: "find — 按 _id 查询", key: "gen-find" },
          { label: "updateOne — $set 该文档", key: "gen-update" },
          { label: "insertOne — 复制该文档", key: "gen-insert" },
          { label: "deleteOne — 按 _id 删除", key: "gen-delete" },
        ];
    items.push({
      label: "生成语句到新标签页",
      key: "gen",
      children: genChildren,
    });
  }
  return items;
});

function handleCtxMenu(e: MouseEvent, row: RowItem) {
  e.preventDefault();
  ctxRow.value = row;
  ctxMenuX.value = e.clientX;
  ctxMenuY.value = e.clientY;
  showCtxMenu.value = true;
}

async function handleCtxSelect(action: string) {
  showCtxMenu.value = false;
  const r = ctxRow.value;
  if (!r) return;
  try {
    if (action === "copy-value") {
      const text = typeof r.value === "object" ? JSON.stringify(r.value, null, 2) : String(r.displayValue);
      await navigator.clipboard.writeText(text);
      message.success("已复制值");
    } else if (action === "copy-key") {
      await navigator.clipboard.writeText(r.key);
      message.success("已复制字段名");
    } else if (action === "copy-doc") {
      const doc = props.documents[r.docIndex];
      if (doc) {
        await navigator.clipboard.writeText(JSON.stringify(doc, null, 2));
        message.success("已复制文档");
      }
    } else if (action === "view-doc") {
      openDocViewer(r.docIndex, new MouseEvent("click"));
    } else if (action.startsWith("gen-")) {
      const doc = props.documents[r.docIndex];
      if (!doc) return;
      const coll = props.collection || "collection";
      let queryText = "";
      if (action === "gen-find") queryText = buildFindByIdQuery(coll, doc);
      else if (action === "gen-update") queryText = buildUpdateOneQuery(coll, doc);
      else if (action === "gen-insert") queryText = buildInsertOneQuery(coll, doc);
      else if (action === "gen-delete") queryText = buildDeleteOneQuery(coll, doc);
      if (queryText) emit("editInTab", { doc, queryText });
    }
  } catch {
    message.error("操作失败");
  }
}

function openDocViewer(docIdx: number, e: MouseEvent) {
  e.stopPropagation();
  docViewerIndex.value = docIdx;
  showDocViewer.value = true;
}

function toggle(path: string) {
  const next = new Set(expanded.value);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  expanded.value = next;
}

function isExpanded(path: string): boolean {
  return expanded.value.has(path);
}

function selectRow(path: string) {
  selectedPath.value = path;
}

function objectPreview(val: unknown, type: string): string {
  try {
    if (type === "Document") {
      const entries = Object.entries(val as Record<string, unknown>);
      const preview = entries.slice(0, 3).map(([k, v]) => {
        const vt = getBsonType(v);
        const vv = vt === "String" ? `"${String(v).slice(0, 30)}"` : formatTreeValue(v, vt);
        return `${k}: ${vv}`;
      }).join(", ");
      return `{ ${preview}${entries.length > 3 ? ", ..." : ""} }`;
    }
    if (type === "Array") {
      const arr = val as unknown[];
      const preview = arr.slice(0, 3).map((v) => formatTreeValue(v)).join(", ");
      return `[ ${preview}${arr.length > 3 ? ", ..." : ""} ]`;
    }
  } catch { /* ignore */ }
  return "";
}

interface RowItem {
  path: string;
  key: string;
  value: unknown;
  type: string;
  displayValue: string;
  depth: number;
  expandable: boolean;
  isDocRoot: boolean;
  isObjectField: boolean;
  docIndex: number;
}

const flatRows = computed<RowItem[]>(() => {
  const rows: RowItem[] = [];
  props.documents.forEach((doc, docIdx) => {
    const docPath = `doc:${docIdx}`;
    const idStr = extractIdDisplay(doc);
    const fieldCount = Object.keys(doc).length;
    rows.push({
      path: docPath,
      key: `(${(props.rowOffset ?? 0) + docIdx + 1})`,
      value: doc,
      type: "Document",
      displayValue: idStr ? `${idStr}  { ${fieldCount} fields }` : `{ ${fieldCount} fields }`,
      depth: 0,
      expandable: true,
      isDocRoot: true,
      isObjectField: false,
      docIndex: docIdx,
    });
    if (isExpanded(docPath)) {
      flattenFields(doc, docPath, 1, rows, docIdx);
    }
  });
  return rows;
});

function flattenFields(obj: Record<string, unknown>, parentPath: string, depth: number, rows: RowItem[], docIdx: number = -1) {
  for (const [key, val] of Object.entries(obj)) {
    const path = `${parentPath}.${key}`;
    const type = getBsonType(val);
    const expandable = type === "Document" || type === "Array";

    rows.push({
      path, key, value: val, type,
      displayValue: formatTreeValue(val, type),
      depth, expandable,
      isDocRoot: false,
      isObjectField: expandable,
      docIndex: docIdx,
    });

    if (expandable && isExpanded(path)) {
      if (type === "Array") {
        (val as unknown[]).forEach((item, idx) => {
          const itemPath = `${path}[${idx}]`;
          const itemType = getBsonType(item);
          const itemExpandable = itemType === "Document" || itemType === "Array";
          rows.push({
            path: itemPath, key: `[${idx}]`, value: item, type: itemType,
            displayValue: formatTreeValue(item, itemType),
            depth: depth + 1, expandable: itemExpandable,
            isDocRoot: false, isObjectField: itemExpandable, docIndex: -1,
          });
          if (itemExpandable && isExpanded(itemPath) && typeof item === "object" && item) {
            flattenFields(item as Record<string, unknown>, itemPath, depth + 2, rows);
          }
        });
      } else {
        flattenFields(val as Record<string, unknown>, path, depth + 1, rows);
      }
    }
  }
}
</script>

<template>
  <div class="tree-doc-view">
    <table class="doc-table">
      <thead>
        <tr>
          <th v-if="selectionEnabled" class="col-check">
            <n-checkbox
              :checked="!!allChecked"
              :indeterminate="someChecked"
              :disabled="selectableKeys.length === 0"
              @update:checked="toggleAll"
              @click.stop
            />
          </th>
          <th class="col-key">Key</th>
          <th class="col-value">Value</th>
          <th class="col-type">Type</th>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="(row, rowIdx) in flatRows"
          :key="row.path"
          class="doc-row"
          :data-doc-index="row.isDocRoot ? row.docIndex : undefined"
          :class="{
            expandable: row.expandable,
            'doc-root': row.isDocRoot,
            'row-even': rowIdx % 2 === 0,
            'row-selected': selectedPath === row.path,
            'row-matched': searchKeyword && row.isDocRoot && matchSet.has(row.docIndex),
            'row-active-match': searchKeyword && row.isDocRoot && row.docIndex === activeMatchDocIndex,
          }"
          @click="selectRow(row.path); row.expandable && toggle(row.path)"
          @contextmenu="handleCtxMenu($event, row)"
        >
          <td v-if="selectionEnabled" class="col-check">
            <n-checkbox
              v-if="row.isDocRoot && docKey(row.docIndex) !== null"
              :checked="isRowChecked(row.docIndex)"
              @update:checked="toggleRow(row.docIndex)"
              @click.stop
            />
          </td>
          <td class="col-key" :style="{ paddingLeft: `${row.depth * 18 + 6}px` }">
            <span v-if="row.expandable" class="toggle-icon">{{ isExpanded(row.path) ? '▼' : '▶' }}</span>
            <span v-else class="toggle-placeholder" />
            <span class="key-name" v-html="hl(row.key)" />
          </td>
          <td class="col-value" :class="{ 'cell-dirty': isCellDirty(row) }">
            <!-- 文档根行或 _id 字段：点击打开文档查看器 -->
            <template v-if="row.isDocRoot">
              <span
                class="clickable-value"
                @click.stop="openDocViewer(row.docIndex, $event)"
                v-html="hl(row.displayValue)"
              />
            </template>
            <template v-else-if="row.key === '_id'">
              <span
                class="clickable-value"
                style="color:#c678dd"
                @click.stop="openDocViewer(row.docIndex >= 0 ? row.docIndex : 0, $event)"
                v-html="hl(row.displayValue)"
              />
            </template>
            <!-- Object/Array 字段: hover 预览, 双击弹 ValueDetail (复合类型无法用单 input 表达) -->
            <template v-else-if="row.isObjectField">
              <n-tooltip trigger="hover" placement="bottom-start" :delay="400" style="max-width: 500px">
                <template #trigger>
                  <span
                    class="clickable-value"
                    :title="readOnly ? '双击查看' : '双击修改类型和值'"
                    @dblclick.stop="openValueEditor(row, $event)"
                    v-html="hl(row.displayValue)"
                  />
                </template>
                <pre class="tooltip-preview">{{ objectPreview(row.value, row.type) }}</pre>
              </n-tooltip>
            </template>
            <!-- 简单类型: 内联编辑; null / Undefined 由 onValueDblclick 路由到 ValueDetail -->
            <template v-else>
              <select
                v-if="isEditing(row.docIndex, row.key) && row.type === 'Boolean'"
                v-model="editingValue"
                class="tv-inline-select"
                @vue:mounted="(v: any) => v.el?.focus?.()"
                @blur="commitInlineEdit"
                @keydown.enter.prevent="commitInlineEdit"
                @keydown.escape="cancelInlineEdit"
                @click.stop
              >
                <option value="true">true</option>
                <option value="false">false</option>
              </select>
              <input
                v-else-if="isEditing(row.docIndex, row.key)"
                v-model="editingValue"
                class="tv-inline-input"
                :type="row.type === 'Int32' || row.type === 'Double' ? 'number' : 'text'"
                @vue:mounted="(v: any) => v.el?.focus?.()"
                @blur="commitInlineEdit"
                @keydown.enter.prevent="commitInlineEdit"
                @keydown.escape="cancelInlineEdit"
                @click.stop
              />
              <span
                v-else
                :style="{
                  color: getValueColor(row.type),
                  cursor: isEditableLeaf(row) ? 'text' : 'default',
                }"
                :title="isEditableLeaf(row) ? '双击修改' : ''"
                @dblclick.stop="onValueDblclick(row, $event)"
                v-html="hl(row.displayValue)"
              />
            </template>
          </td>
          <td class="col-type">
            <span :style="{ color: getTypeColor(row.type) }">{{ row.type }}</span>
          </td>
        </tr>
      </tbody>
    </table>
    <ValueDetail
      v-model:show="showDetail"
      :field="detailField"
      :value="detailValue"
      :connection-id="connectionId"
      :database="database"
      :collection="collection"
      :read-only="readOnly"
      :document-id="detailDocId"
      :document="detailDoc"
      @saved="onDetailSaved"
    />
    <DocumentViewer
      v-model:show="showDocViewer"
      :documents="documents"
      :initial-index="docViewerIndex"
      :collection="collection"
      :read-only="readOnly"
      @edit-in-tab="forwardEditInTab"
    />
    <n-dropdown
      trigger="manual"
      placement="bottom-start"
      :show="showCtxMenu"
      :options="ctxMenuOptions"
      :x="ctxMenuX"
      :y="ctxMenuY"
      @select="handleCtxSelect"
      @clickoutside="showCtxMenu = false"
    />
  </div>
</template>

<style scoped>
.tree-doc-view {
  height: 100%;
  overflow: auto;
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 12px;
  line-height: 1;
}
.doc-table {
  min-width: 100%;
  border-collapse: collapse;
}
.col-check {
  width: 32px;
  padding: 4px 4px;
  text-align: center;
}
.col-check :deep(.n-checkbox) {
  vertical-align: middle;
}
thead {
  position: sticky;
  top: 0;
  z-index: 1;
}
th {
  padding: 5px 8px;
  text-align: left;
  font-weight: 600;
  font-size: 11px;
  color: #888;
  border: 1px solid #e0e0e0;
  background: #fafafa;
  overflow: hidden;
  resize: horizontal;
}
.col-key { min-width: 150px; width: 30%; }
.col-value { min-width: 200px; width: 52%; }
.col-type { min-width: 80px; width: 18%; }

.doc-row { height: 24px; }
.doc-row.row-even { background: rgba(0, 0, 0, 0.02); }
.doc-row:hover { background: #e8f4fd; }
.doc-row.expandable { cursor: pointer; }
.doc-row.doc-root {
  background: rgba(0, 0, 0, 0.03);
  border-top: 1px solid #eee;
}
.doc-row.row-selected {
  background: #3875d7 !important;
}
.doc-row.row-selected td,
.doc-row.row-selected .key-name,
.doc-row.row-selected .toggle-icon,
.doc-row.row-selected span {
  color: #fff !important;
}

/* 搜索命中高亮 */
.doc-row.row-matched.doc-root {
  background: #fff8e1;
}
.doc-row.row-active-match.doc-root {
  background: #ffe082 !important;
  box-shadow: inset 3px 0 0 #ff8f00;
}
:deep(mark.kw-hit) {
  background: #fff59d;
  color: inherit;
  padding: 0 1px;
  border-radius: 2px;
}
.doc-row.row-active-match :deep(mark.kw-hit) {
  background: #ff8f00;
  color: #fff;
}

td {
  padding: 3px 8px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  vertical-align: middle;
  border-right: 1px solid #eee;
}
td:last-child {
  border-right: none;
}
.toggle-icon {
  display: inline-block;
  width: 14px;
  font-size: 8px;
  color: #666;
  text-align: center;
}
.toggle-placeholder { display: inline-block; width: 14px; }
.key-name {
  font-weight: 500;
  color: #333;
  margin-left: 2px;
}
.clickable-value {
  color: #61afef;
  cursor: pointer;
}
.clickable-value:hover {
  text-decoration: underline;
}
.tooltip-preview {
  margin: 0;
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 12px;
  line-height: 1.4;
  color: #d4d4d4;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 200px;
  overflow: auto;
}
/* 已修改字段标识 —— 浅黄底 + 橙色左条, 翻页 / 重查询时由父组件清除 */
.cell-dirty {
  background: rgba(232, 168, 56, 0.12) !important;
  box-shadow: inset 3px 0 0 #e8a838;
}
/* 内联编辑 input / select —— 跟 TableView 风格保持一致 */
.tv-inline-input,
.tv-inline-select {
  width: 100%;
  max-width: 100%;
  padding: 1px 4px;
  border: 1px solid #3875d7;
  border-radius: 2px;
  outline: none;
  font-family: inherit;
  font-size: 12px;
  background: #fff;
  box-sizing: border-box;
}
</style>

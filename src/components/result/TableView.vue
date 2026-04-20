<script setup lang="ts">
import { computed, h, ref, nextTick, watch } from "vue";
import { NDataTable, NTooltip, NDropdown, useMessage } from "naive-ui";
import type { DataTableColumns } from "naive-ui";
import { getBsonType, formatBsonCell, getValueColor } from "@/utils/bson-format";
import { highlightKeyword } from "@/utils/text-highlight";
import * as docApi from "@/api/document";
import ValueDetail from "./ValueDetail.vue";
import DocumentViewer from "./DocumentViewer.vue";

const props = defineProps<{
  connectionId: string;
  database: string;
  collection: string;
  documents: Record<string, unknown>[];
  rowOffset?: number;
  docKeyFn?: (doc: Record<string, unknown>) => string | null;
  selectedKeys?: Set<string>;
  searchKeyword?: string;
  matchCase?: boolean;
  activeMatchDocIndex?: number;
  matchDocIndexes?: number[];
}>();

const emit = defineEmits<{
  updated: [];
  setSelection: [keys: string[]];
  editInTab: [payload: { doc: Record<string, unknown>; queryText: string }];
}>();

const message = useMessage();

// 对话框
const showDetail = ref(false);
const detailField = ref("");
const detailValue = ref<unknown>(null);
const detailDoc = ref<Record<string, unknown> | undefined>(undefined);
const detailDocId = ref("");
const showDocViewer = ref(false);
const docViewerIndex = ref(0);

// 内联编辑
const editingRow = ref<number | null>(null);
const editingKey = ref<string | null>(null);
const editingValue = ref<string>("");
const editingType = ref<string>("");

function openDetail(field: string, val: unknown, doc?: Record<string, unknown>) {
  detailField.value = field;
  detailValue.value = val;
  detailDoc.value = doc;
  if (doc) {
    const id = doc._id;
    detailDocId.value = typeof id === "object" && id !== null
      ? String((id as Record<string, unknown>).$oid ?? JSON.stringify(id))
      : String(id ?? "");
  }
  showDetail.value = true;
}

/** 记录编辑前的原始值，用于比较是否有变化 */
const editingOriginal = ref<string>("");

function canEdit(rowIdx: number): boolean {
  // 没有集合名（命令结果）不可编辑
  if (!props.collection) return false;
  // 没有 _id 的文档不可编辑
  const doc = props.documents[rowIdx];
  if (!doc || doc._id === undefined) return false;
  return true;
}

function startEdit(rowIdx: number, key: string, val: unknown, type: string) {
  if (!canEdit(rowIdx)) {
    message.warning("该结果不可编辑");
    return;
  }
  editingRow.value = rowIdx;
  editingKey.value = key;
  editingType.value = type;
  // 转为可编辑的文本
  let text: string;
  if (type === "Boolean") {
    text = String(val);
  } else if (type === "Date") {
    // 将 BSON Date 转为本地带时区的 ISO 格式：2025-04-01T09:34:00+08:00
    const obj = val as Record<string, unknown>;
    const d = obj?.$date;
    let ms: number;
    if (typeof d === "string") {
      ms = new Date(d).getTime();
    } else if (typeof d === "number") {
      ms = d;
    } else if (typeof d === "object" && d && (d as Record<string, unknown>).$numberLong) {
      ms = parseInt(String((d as Record<string, unknown>).$numberLong));
    } else {
      ms = Date.now();
    }
    const date = new Date(ms);
    if (isNaN(date.getTime())) {
      text = String(d);
    } else {
      // 格式化为本地 ISO：yyyy-MM-ddTHH:mm:ss±HH:mm
      const offset = -date.getTimezoneOffset();
      const sign = offset >= 0 ? "+" : "-";
      const pad = (n: number) => String(n).padStart(2, "0");
      const tzH = pad(Math.floor(Math.abs(offset) / 60));
      const tzM = pad(Math.abs(offset) % 60);
      const y = date.getFullYear();
      const mo = pad(date.getMonth() + 1);
      const da = pad(date.getDate());
      const h = pad(date.getHours());
      const mi = pad(date.getMinutes());
      const s = pad(date.getSeconds());
      text = `${y}-${mo}-${da}T${h}:${mi}:${s}${sign}${tzH}:${tzM}`;
    }
  } else if (type === "Int64") {
    text = String((val as Record<string, unknown>)?.$numberLong ?? val);
  } else if (type === "Decimal128") {
    text = String((val as Record<string, unknown>)?.$numberDecimal ?? val);
  } else {
    text = String(val ?? "");
  }
  editingValue.value = text;
  editingOriginal.value = text;
}

async function commitEdit() {
  if (editingRow.value === null || editingKey.value === null) return;
  const key = editingKey.value;
  const rowIdx = editingRow.value;
  const rawVal = editingValue.value;
  const type = editingType.value;

  // 值没变化，直接取消
  if (rawVal === editingOriginal.value) {
    cancelEdit();
    return;
  }

  let finalVal: unknown = rawVal;
  if (type === "Int32") finalVal = parseInt(rawVal) || 0;
  else if (type === "Double") finalVal = parseFloat(rawVal) || 0;
  else if (type === "Boolean") finalVal = rawVal === "true";
  else if (type === "Int64") finalVal = { $numberLong: rawVal };
  else if (type === "Decimal128") finalVal = { $numberDecimal: rawVal };
  else if (type === "Date") {
    const parsed = new Date(rawVal);
    if (isNaN(parsed.getTime())) {
      message.error(`时间格式错误: "${rawVal}"，请使用 ISO 格式如 2025-04-01T09:34:00+08:00`);
      return; // 不关闭编辑状态，让用户修正
    }
    finalVal = { $date: parsed.toISOString() };
  }

  // 获取文档 _id
  const doc = props.documents[rowIdx];
  if (!doc) { cancelEdit(); return; }
  const docId = doc._id;
  const idStr = typeof docId === "object" && docId !== null
    ? String((docId as Record<string, unknown>).$oid ?? JSON.stringify(docId))
    : String(docId);

  // 构建更新后的完整文档
  const updatedDoc = { ...JSON.parse(JSON.stringify(doc)), [key]: finalVal };

  try {
    await docApi.updateDocument(props.connectionId, props.database, props.collection, idStr, updatedDoc);
    // 更新本地数据
    (doc as Record<string, unknown>)[key] = finalVal;
    message.success("已保存");
    emit("updated");
  } catch (e) {
    message.error(`保存失败: ${e}`);
  }

  cancelEdit();
}

function cancelEdit() {
  editingRow.value = null;
  editingKey.value = null;
}

function isEditing(rowIdx: number, key: string): boolean {
  return editingRow.value === rowIdx && editingKey.value === key;
}

function objectPreview(val: unknown): string {
  try { return JSON.stringify(val, null, 2).slice(0, 300); }
  catch { return String(val); }
}

const columns = computed<DataTableColumns>(() => {
  if (props.documents.length === 0) return [];

  // 多选列 (仅当父组件传入 docKeyFn 时启用)
  const selectionCol: Record<string, unknown> | null = props.docKeyFn
    ? { type: "selection", width: 36, fixed: "left" as const }
    : null;

  // 序号列
  const indexCol = {
    title: "#",
    key: "__index",
    width: 50,
    render(row: Record<string, unknown>) {
      const idx = (row as Record<string, unknown>).__rowKey as number;
      return h("span", { style: "color:#999;font-size:11px" }, String((props.rowOffset ?? 0) + idx + 1));
    },
  };

  const keySet = new Set<string>();
  for (const doc of props.documents) {
    for (const key of Object.keys(doc)) keySet.add(key);
  }
  const dataCols = Array.from(keySet).map((key) => ({
    title: key,
    key,
    width: key === "_id" ? 240 : 160,
    resizable: true,
    ellipsis: { tooltip: false },
    render(row: Record<string, unknown>) {
      const val = row[key];
      const rowIdx = (row as Record<string, unknown>).__rowKey as number;

      if (val === null || val === undefined) {
        return h("span", { style: "color:#999;font-style:italic" }, "null");
      }

      const type = getBsonType(val);
      const color = getValueColor(type);

      // _id 列：紫色可点击，无下划线
      if (key === "_id") {
        const oid = type === "ObjectId"
          ? String((val as Record<string, unknown>).$oid ?? val)
          : formatBsonCell(val);
        return h("span", {
          style: "color:#c678dd;cursor:pointer",
          innerHTML: props.searchKeyword
            ? highlightKeyword(oid, props.searchKeyword, !!props.matchCase)
            : oid,
          onClick: (e: MouseEvent) => {
            e.stopPropagation();
            docViewerIndex.value = rowIdx;
            showDocViewer.value = true;
          },
        });
      }

      // 内联编辑状态
      if (isEditing(rowIdx, key)) {
        if (type === "Boolean") {
          return h("div", { class: "inline-editing" }, [
            h("select", {
              value: editingValue.value,
              class: "inline-select",
              onChange: (e: Event) => { editingValue.value = (e.target as HTMLSelectElement).value; },
              onBlur: () => commitEdit(),
              onKeydown: (e: KeyboardEvent) => {
                if (e.key === "Enter") commitEdit();
                if (e.key === "Escape") cancelEdit();
              },
            }, [
              h("option", { value: "true" }, "true"),
              h("option", { value: "false" }, "false"),
            ]),
          ]);
        }
        return h("div", { class: "inline-editing" }, [
          h("input", {
            value: editingValue.value,
            class: "inline-input",
            type: (type === "Int32" || type === "Double") ? "number" : "text",
            autofocus: true,
            onInput: (e: Event) => { editingValue.value = (e.target as HTMLInputElement).value; },
            onBlur: () => commitEdit(),
            onKeydown: (e: KeyboardEvent) => {
              if (e.key === "Enter") commitEdit();
              if (e.key === "Escape") cancelEdit();
            },
            onVnodeMounted: (vnode: any) => {
              nextTick(() => vnode.el?.focus?.());
            },
          }),
        ]);
      }

      // 复杂类型：hover 预览 + 点击编辑对话框
      if (type === "Document" || type === "Array") {
        const label = type === "Document"
          ? `{${Object.keys(val as object).length} fields}`
          : `[${(val as unknown[]).length}]`;
        return h(
          NTooltip,
          { trigger: "hover", placement: "bottom-start", delay: 300, style: "max-width:500px" },
          {
            trigger: () => h("span", {
              style: "color:#61afef;cursor:pointer",
              onClick: (e: MouseEvent) => { e.stopPropagation(); openDetail(key, val, row); },
            }, label),
            default: () => h("pre", {
              style: "margin:0;font-size:12px;color:#d4d4d4;white-space:pre-wrap;max-height:200px;overflow:auto",
            }, objectPreview(val)),
          },
        );
      }

      // 简单类型：双击进入编辑
      const text = formatBsonCell(val);
      if (props.searchKeyword) {
        return h("span", {
          style: `${color ? `color:${color};` : ""}cursor:text`,
          innerHTML: highlightKeyword(text, props.searchKeyword, !!props.matchCase),
          onDblclick: (e: MouseEvent) => {
            e.stopPropagation();
            startEdit(rowIdx, key, val, type);
          },
        });
      }
      return h("span", {
        style: `${color ? `color:${color};` : ""}cursor:text`,
        onDblclick: (e: MouseEvent) => {
          e.stopPropagation();
          startEdit(rowIdx, key, val, type);
        },
      }, text);
    },
  }));
  return selectionCol ? [selectionCol, indexCol, ...dataCols] : [indexCol, ...dataCols];
});

const scrollX = computed(() => {
  let total = 0;
  for (const col of columns.value) total += (col.width as number) || 160;
  return Math.max(total, 800);
});

/** 每行的 row-key = docSelectionKey 返回值 (父组件指定), 失败时 fallback 为行号 */
const data = computed(() => props.documents.map((doc, i) => {
  const sel = props.docKeyFn ? props.docKeyFn(doc) : null;
  return { ...doc, __rowKey: i, __selectKey: sel ?? `idx:${i}` };
}));

// NDataTable v-model:checked-row-keys 的绑定值, 跟父组件 selectedKeys 同步
const checkedRowKeys = computed<string[]>(() => {
  if (!props.selectedKeys) return [];
  return Array.from(props.selectedKeys);
});

function onCheckedChange(keys: (string | number)[]) {
  emit("setSelection", keys.map(String));
}

// ---- 右键菜单 ----
const showCtxMenu = ref(false);
const ctxMenuX = ref(0);
const ctxMenuY = ref(0);
const ctxRowIdx = ref(-1);

const ctxMenuOptions = [
  { label: "复制文档 (JSON)", key: "copy-doc" },
  { label: "复制 _id", key: "copy-id" },
  { label: "查看文档", key: "view-doc" },
];

const matchSet = computed(() => new Set(props.matchDocIndexes ?? []));

function rowProps(row: Record<string, unknown>) {
  const rowIdx = row.__rowKey as number;
  const classes: string[] = [];
  if (props.searchKeyword && matchSet.value.has(rowIdx)) classes.push("row-matched");
  if (props.searchKeyword && rowIdx === props.activeMatchDocIndex) classes.push("row-active-match");
  return {
    class: classes.join(" ") || undefined,
    "data-doc-index": String(rowIdx),
    onContextmenu: (e: MouseEvent) => {
      e.preventDefault();
      ctxRowIdx.value = rowIdx;
      ctxMenuX.value = e.clientX;
      ctxMenuY.value = e.clientY;
      showCtxMenu.value = true;
    },
  };
}

// 当前匹配变化 -> 滚到对应行 (vdom 更新后)
watch(
  () => props.activeMatchDocIndex,
  async (idx) => {
    if (idx === undefined || idx < 0) return;
    await nextTick();
    const el = document.querySelector<HTMLElement>(`tr[data-doc-index="${idx}"]`);
    el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  },
);

async function handleCtxSelect(action: string) {
  showCtxMenu.value = false;
  const idx = ctxRowIdx.value;
  if (idx < 0 || idx >= props.documents.length) return;
  const doc = props.documents[idx];
  try {
    if (action === "copy-doc") {
      await navigator.clipboard.writeText(JSON.stringify(doc, null, 2));
      message.success("已复制文档");
    } else if (action === "copy-id") {
      const id = doc._id;
      const text = typeof id === "object" && id && (id as Record<string, unknown>).$oid
        ? String((id as Record<string, unknown>).$oid)
        : JSON.stringify(id);
      await navigator.clipboard.writeText(text);
      message.success("已复制 _id");
    } else if (action === "view-doc") {
      docViewerIndex.value = idx;
      showDocViewer.value = true;
    }
  } catch { message.error("操作失败"); }
}
</script>

<template>
  <div class="table-view">
    <n-data-table
      :columns="columns"
      :data="data"
      :row-key="(row: any) => row.__selectKey"
      :row-props="(row: any) => rowProps(row)"
      :scroll-x="scrollX"
      :checked-row-keys="checkedRowKeys"
      flex-height
      style="height: 100%"
      striped
      virtual-scroll
      size="small"
      @update:checked-row-keys="onCheckedChange"
    />
    <ValueDetail
      v-model:show="showDetail"
      :field="detailField"
      :value="detailValue"
      :connection-id="connectionId"
      :database="database"
      :collection="collection"
      :document-id="detailDocId"
      :document="detailDoc"
      @saved="emit('updated')"
    />
    <DocumentViewer
      v-model:show="showDocViewer"
      :documents="documents"
      :initial-index="docViewerIndex"
      :collection="collection"
      @edit-in-tab="(payload: { doc: Record<string, unknown>; queryText: string }) => emit('editInTab', payload)"
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
.table-view {
  height: 100%;
}
.table-view :deep(tr.row-matched > td) {
  background: #fff8e1 !important;
}
.table-view :deep(tr.row-active-match > td) {
  background: #ffe082 !important;
  box-shadow: inset 3px 0 0 #ff8f00;
}
.table-view :deep(mark.kw-hit) {
  background: #fff59d;
  color: inherit;
  padding: 0 1px;
  border-radius: 2px;
}
.table-view :deep(tr.row-active-match mark.kw-hit) {
  background: #ff8f00;
  color: #fff;
}
.table-view :deep(.inline-editing) {
  border-left: 3px solid #e8a838;
  padding-left: 3px;
  margin: -4px -12px;
  padding: 4px;
  width: calc(100% + 24px);
  box-sizing: border-box;
}
.table-view :deep(.inline-input) {
  width: 100%;
  max-width: 100%;
  padding: 2px 4px;
  border: 1px solid #3875d7;
  border-radius: 2px;
  outline: none;
  font-family: inherit;
  font-size: 12px;
  background: #fff;
  box-sizing: border-box;
}
.table-view :deep(.inline-select) {
  width: 100%;
  max-width: 100%;
  padding: 2px 4px;
  border: 1px solid #3875d7;
  border-radius: 2px;
  box-sizing: border-box;
  outline: none;
  font-family: inherit;
  font-size: 12px;
  background: #fff;
  box-sizing: border-box;
}
</style>

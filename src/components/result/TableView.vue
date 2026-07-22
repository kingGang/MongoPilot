<script setup lang="ts">
import { computed, h, ref, shallowRef, nextTick, watch, onBeforeUnmount } from "vue";
import { NDataTable, NDropdown, useMessage } from "naive-ui";
import type { DataTableColumns, DataTableColumn } from "naive-ui";
import { getBsonType, formatBsonCell, getValueColor, objectIdHoverText } from "@/utils/bson-format";
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
  /** 只读连接 —— 禁止任何 startEdit / openDetail */
  readOnly?: boolean;
  docKeyFn?: (doc: Record<string, unknown>) => string | null;
  selectedKeys?: Set<string>;
  /** 已编辑字段集合, 元素格式: `${docKey}::${field}` —— 给单元格画 dirty 标识 */
  dirtyFields?: Set<string>;
  searchKeyword?: string;
  matchCase?: boolean;
  activeMatchDocIndex?: number;
  matchDocIndexes?: number[];
}>();

const emit = defineEmits<{
  updated: [];
  setSelection: [keys: string[]];
  editInTab: [payload: { doc: Record<string, unknown>; queryText: string }];
  /** 编辑成功 -> 上报 (docKey, field) */
  dirty: [docKey: string, field: string];
}>();

/** 给定一行 + 字段名: 是否在 dirty 集合里 */
function isCellDirty(row: Record<string, unknown>, key: string): boolean {
  if (!props.dirtyFields || props.dirtyFields.size === 0) return false;
  if (!props.docKeyFn) return false;
  const k = props.docKeyFn(row);
  if (!k) return false;
  return props.dirtyFields.has(`${k}::${key}`);
}

/** 上报某行某字段 dirty */
function emitDirty(doc: Record<string, unknown>, field: string) {
  if (!props.docKeyFn) return;
  const k = props.docKeyFn(doc);
  if (k) emit("dirty", k, field);
}

/** ValueDetail 保存成功 -> 标 dirty + 通知父组件 */
function onDetailSaved() {
  if (detailDoc.value) emitDirty(detailDoc.value, detailField.value);
  emit("updated");
}

/** 转发 DocumentViewer 的 editInTab —— 写在 script 里避免 template 泛型 < 被 vue-tsc 误读 */
function forwardEditInTab(payload: { doc: Record<string, unknown>; queryText: string }) {
  emit("editInTab", payload);
}

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
  // 只读连接一律不可编辑
  if (props.readOnly) return false;
  // 没有集合名（命令结果）不可编辑
  if (!props.collection) return false;
  // 没有 _id 的文档不可编辑
  const doc = props.documents[rowIdx];
  if (!doc || doc._id === undefined) return false;
  return true;
}

function startEdit(rowIdx: number, key: string, val: unknown, type: string) {
  if (props.readOnly) {
    message.warning("只读连接: 不允许修改文档");
    return;
  }
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
    emitDirty(doc, key);
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

// ---- 复杂单元格 hover 浮层 (Document / Array): 完整内容 + 逐字段可复制 ----
// 换掉原来的原生 title (会被视口裁掉、无法选中复制) + objectPreview 300 字截断。
// 单实例 Teleport 浮层, 只在 mouseenter/leave 时切换, 不给每格挂组件, 拖列不卡。
const hoverShow = ref(false);
const hoverX = ref(0);
const hoverY = ref(0);
const hoverValue = shallowRef<unknown>(null);
let hoverHideTimer: ReturnType<typeof setTimeout> | null = null;

interface HoverRow {
  label: string;
  preview: string;
  color: string;
  raw: unknown;
}

const hoverTitle = computed(() => {
  const val = hoverValue.value;
  if (Array.isArray(val)) return `Array · ${val.length} 项`;
  if (val && typeof val === "object") return `Document · ${Object.keys(val).length} 字段`;
  return "";
});

function fieldPreview(v: unknown, t: string): string {
  if (v === null || v === undefined) return "null";
  if (t === "Document") return `{${Object.keys(v as object).length} fields}`;
  if (t === "Array") return `[${(v as unknown[]).length}]`;
  const s = formatBsonCell(v);
  return s.length > 200 ? s.slice(0, 200) + "…" : s;
}

const hoverRows = computed<HoverRow[]>(() => {
  const val = hoverValue.value;
  if (val === null || typeof val !== "object") return [];
  const entries: [string, unknown][] = Array.isArray(val)
    ? (val as unknown[]).map((v, i) => [`[${i}]`, v] as [string, unknown])
    : Object.entries(val as Record<string, unknown>);
  return entries.map(([k, v]) => {
    const t = getBsonType(v);
    return { label: k, preview: fieldPreview(v, t), color: getValueColor(t), raw: v };
  });
});

const hoverStyle = computed(() => {
  const pad = 12;
  const w = 460;
  const hMax = 380;
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  let left = hoverX.value + pad;
  if (left + w > vw - 8) left = hoverX.value - w - pad; // 右边放不下 -> 翻到光标左边
  if (left < 8) left = Math.max(8, vw - w - 8);
  let top = hoverY.value + pad;
  if (top + hMax > vh - 8) top = Math.max(8, vh - hMax - 8);
  return { left: `${left}px`, top: `${top}px`, width: `${w}px`, maxHeight: `${hMax}px` };
});

function showHover(e: MouseEvent, val: unknown) {
  if (hoverHideTimer) {
    clearTimeout(hoverHideTimer);
    hoverHideTimer = null;
  }
  hoverValue.value = val;
  hoverX.value = e.clientX;
  hoverY.value = e.clientY;
  hoverShow.value = true;
}
function scheduleHideHover() {
  if (hoverHideTimer) clearTimeout(hoverHideTimer);
  hoverHideTimer = setTimeout(() => {
    hoverShow.value = false;
    hoverHideTimer = null;
  }, 160);
}
function cancelHideHover() {
  if (hoverHideTimer) {
    clearTimeout(hoverHideTimer);
    hoverHideTimer = null;
  }
}
async function copyText(text: string, okMsg: string) {
  try {
    await navigator.clipboard.writeText(text);
    message.success(okMsg);
  } catch {
    message.error("复制失败");
  }
}
function copyHoverAll() {
  try {
    copyText(JSON.stringify(hoverValue.value, null, 2), "已复制 JSON");
  } catch {
    message.error("复制失败");
  }
}
function copyHoverField(row: HoverRow) {
  const v = row.raw;
  const text = v !== null && typeof v === "object" ? JSON.stringify(v) : formatBsonCell(v);
  copyText(text, `已复制 ${row.label}`);
}
onBeforeUnmount(() => cancelHideHover());

/** 列宽持久化: key -> width. 拖动结束 snapshot 一次, 下次重建列时沿用 */
const colWidthMap = ref<Record<string, number>>({});

/** 字段名集合的稳定签名 (排序后用 | 拼接) —— 仅在它变化时重建列定义,
 *  避免 props.documents 每次变化 (翻页/搜索/编辑) 都重算列, 重置用户调好的列宽 */
const columnKeysSig = computed(() => {
  if (props.documents.length === 0) return "";
  const set = new Set<string>();
  for (const doc of props.documents) for (const k of Object.keys(doc)) set.add(k);
  return [...set].sort().join("|");
});

/** 默认列宽 */
function defaultWidth(key: string): number {
  return key === "_id" ? 240 : 160;
}

/** 排序键提取: 把一个 BSON 值转成可比较的原始类型 (number | string).
 *  返回 null/undefined 时代表 "空值", sortKey 里用一个足够大的哨兵值让它排最后. */
function toSortable(val: unknown): number | string | null {
  if (val === null || val === undefined) return null;
  if (typeof val === "number") return val;
  if (typeof val === "boolean") return val ? 1 : 0;
  if (typeof val === "string") return val;
  if (typeof val === "object") {
    const obj = val as Record<string, unknown>;
    // ObjectId: 前 8 字符是十六进制时间戳, 可代表创建时间
    if (obj.$oid && typeof obj.$oid === "string") return obj.$oid;
    // Date
    if (obj.$date) {
      const d = obj.$date;
      if (typeof d === "string") return new Date(d).getTime();
      if (typeof d === "object" && d && typeof (d as Record<string, unknown>).$numberLong === "string") {
        return Number((d as Record<string, unknown>).$numberLong);
      }
    }
    if (typeof obj.$numberLong === "string") return Number(obj.$numberLong);
    if (typeof obj.$numberInt === "string") return Number(obj.$numberInt);
    if (typeof obj.$numberDecimal === "string") return Number(obj.$numberDecimal);
    if (obj.$timestamp && typeof obj.$timestamp === "object") {
      const t = (obj.$timestamp as Record<string, unknown>).t;
      if (typeof t === "number") return t;
    }
    // 复杂对象/数组 -> 拿序列化后的字符串比较 (稳定但不一定语义正确, 至少有确定顺序)
    try {
      return JSON.stringify(val);
    } catch {
      return String(val);
    }
  }
  return String(val);
}

/** NDataTable 的 sorter: 空值恒排最后, 其它按类型比较 (数值 vs 字符串).
 *  两侧类型不一样时统一 fallback 为字符串比较, 保证有序. */
function makeColSorter(key: string) {
  return (rowA: Record<string, unknown>, rowB: Record<string, unknown>): number => {
    const a = toSortable(rowA[key]);
    const b = toSortable(rowB[key]);
    if (a === null && b === null) return 0;
    if (a === null) return 1;
    if (b === null) return -1;
    if (typeof a === "number" && typeof b === "number") return a - b;
    if (typeof a === "string" && typeof b === "string") return a.localeCompare(b);
    // 类型不同 → 字符串化再比
    return String(a).localeCompare(String(b));
  };
}

/** naive-ui 不会改 column.width —— 拖动中的宽度存在其内部 map, 这里用官方
 *  onUnstableColumnResize 回调实时同步: 1) 记进 colWidthMap 供列重建时沿用
 *  2) scrollX 跟着重算, 表格总宽始终等于列宽之和 —— 否则两者不一致时
 *  浏览器会按比例摊派差值, 拖一列其他列全跟着动 */
function onColumnResize(_resized: number, limited: number, column: { key?: string | number }) {
  const key = column.key;
  if (typeof key !== "string" || key.startsWith("__")) return;
  colWidthMap.value[key] = Math.round(limited);
}

const columns = shallowRef<DataTableColumns>([]);

watch(
  [columnKeysSig, () => !!props.docKeyFn, () => props.dirtyFields?.size ?? 0],
  () => {
    if (props.documents.length === 0) {
      columns.value = [];
      return;
    }

    const selectionCol: DataTableColumn | null = props.docKeyFn
      ? ({ type: "selection", width: 36, fixed: "left" } as DataTableColumn)
      : null;

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
    for (const doc of props.documents) for (const k of Object.keys(doc)) keySet.add(k);

    const dataCols = Array.from(keySet).map((key) => ({
      title: key,
      key,
      width: colWidthMap.value[key] ?? defaultWidth(key),
      resizable: true,
      // 三态点击排序 (无 → asc → desc → 无). NDataTable 内置支持, 只要传 sorter 即可.
      sorter: makeColSorter(key),
      ellipsis: { tooltip: false },
      // 单元格 td 上加 dirty class (NDataTable render 时调用, 读 props.dirtyFields 走 Vue 响应式)
      cellProps: (row: Record<string, unknown>) =>
        isCellDirty(row, key) ? { class: "tv-cell-dirty" } : {},
      render(row: Record<string, unknown>) {
        const val = row[key];
        const rowIdx = (row as Record<string, unknown>).__rowKey as number;

        // null / undefined: 只读连接下纯展示, 否则双击打开 Type and Value 编辑器
        if (val === null || val === undefined) {
          if (props.readOnly) {
            return h("span", { style: "color:#999;font-style:italic" }, "null");
          }
          return h("span", {
            style: "color:#999;font-style:italic;cursor:pointer",
            title: "双击修改类型和值",
            onDblclick: (e: MouseEvent) => {
              e.stopPropagation();
              if (!canEdit(rowIdx)) {
                message.warning("该结果不可编辑");
                return;
              }
              openDetail(key, val, row);
            },
          }, "null");
        }

        const type = getBsonType(val);
        const color = getValueColor(type);

        // _id 列：紫色可点击
        if (key === "_id") {
          const oid = type === "ObjectId"
            ? String((val as Record<string, unknown>).$oid ?? val)
            : formatBsonCell(val);
          return h("span", {
            style: "color:#c678dd;cursor:pointer",
            title: type === "ObjectId" ? objectIdHoverText(val) : oid,
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

        // 复杂类型: 用原生 title (浏览器气泡, 无 Vue 开销) 做 hover 预览, 点击进 ValueDetail.
        //   —— 之前每格挂一个 NTooltip 实例, 列宽拖动时每次 mousemove 都要重渲染, 卡顿主因.
        //   只读连接下 ValueDetail 也能打开 (它内部按 readOnly 隐藏 Save), 仅供查看不写库.
        if (type === "Document" || type === "Array") {
          const label = type === "Document"
            ? `{${Object.keys(val as object).length} fields}`
            : `[${(val as unknown[]).length}]`;
          return h("span", {
            style: "color:#61afef;cursor:pointer",
            onMouseenter: (e: MouseEvent) => showHover(e, val),
            onMouseleave: () => scheduleHideHover(),
            onClick: (e: MouseEvent) => {
              e.stopPropagation();
              hoverShow.value = false;
              openDetail(key, val, row);
            },
          }, label);
        }

        // 简单类型: 双击进入编辑; 原生 title 悬停看完整内容 (列窄被截断时)。
        // ObjectId 的 title 额外带上内嵌创建时间。
        const text = formatBsonCell(val);
        const cellTitle = type === "ObjectId" ? objectIdHoverText(val) : text;
        if (props.searchKeyword) {
          return h("span", {
            style: `${color ? `color:${color};` : ""}cursor:text`,
            title: cellTitle,
            innerHTML: highlightKeyword(text, props.searchKeyword, !!props.matchCase),
            onDblclick: (e: MouseEvent) => {
              e.stopPropagation();
              startEdit(rowIdx, key, val, type);
            },
          });
        }
        return h("span", {
          style: `${color ? `color:${color};` : ""}cursor:text`,
          title: cellTitle,
          onDblclick: (e: MouseEvent) => {
            e.stopPropagation();
            startEdit(rowIdx, key, val, type);
          },
        }, text);
      },
    }));
    // 填充列: 无宽度, 表格比面板窄时吸收多余空间 (table-layout:fixed 下
    // 没写宽度的列独占余量), 真实列的宽度不会被拉伸/挤压
    const spacerCol = {
      key: "__spacer",
      title: "",
      minWidth: 0,
      render: () => null,
    } as DataTableColumn;

    columns.value = selectionCol
      ? [selectionCol, indexCol, ...dataCols, spacerCol]
      : [indexCol, ...dataCols, spacerCol];
  },
  { immediate: true },
);

const scrollX = computed(() => {
  let total = 0;
  for (const col of columns.value) {
    const k = "key" in col && typeof col.key === "string" ? col.key : "";
    // colWidthMap 是拖动后的实时值; 没有数字宽度的列 (填充列) 不计
    total += (k && colWidthMap.value[k]) || (typeof col.width === "number" ? col.width : 0);
  }
  return total;
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
  const selKey = row.__selectKey as string;
  if (props.selectedKeys && props.selectedKeys.has(selKey)) classes.push("row-selected");
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
      @unstable-column-resize="onColumnResize"
    />
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
    <Teleport to="body">
      <div
        v-if="hoverShow"
        class="cell-hover-float"
        :style="hoverStyle"
        @mouseenter="cancelHideHover"
        @mouseleave="scheduleHideHover"
      >
        <div class="cell-hover-head">
          <span class="chh-title">{{ hoverTitle }}</span>
          <button class="chh-copy-all" @click="copyHoverAll">复制 JSON</button>
        </div>
        <div class="cell-hover-body">
          <div
            v-for="(row, i) in hoverRows"
            :key="i"
            class="chh-row"
            :title="'点击复制 ' + row.label"
            @click="copyHoverField(row)"
          >
            <span class="chh-key">{{ row.label }}</span>
            <span class="chh-colon">:</span>
            <span class="chh-val" :style="{ color: row.color || undefined }">{{ row.preview }}</span>
            <span class="chh-copy-icon">⧉</span>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.table-view {
  height: 100%;
}
/* 列宽拖动时减小重排范围 + 关掉单元格上的 CSS 过渡, 让拖动感觉跟手 */
.table-view :deep(.n-data-table-td) {
  contain: layout paint;
  transition: none !important;
}
/* 鼠标悬停行 —— 淡灰底 */
.table-view :deep(tr:hover > td) {
  background-color: rgba(0, 0, 0, 0.04) !important;
}
/* 选中行 —— 淡绿底 (与勾选框颜色一致), 悬停时略深 */
.table-view :deep(tr.row-selected > td) {
  background-color: rgba(24, 160, 88, 0.1) !important;
}
.table-view :deep(tr.row-selected:hover > td) {
  background-color: rgba(24, 160, 88, 0.16) !important;
}
/* 已修改字段标识 —— 浅黄底 + 橙色左条; 翻页 / 重查询时由父组件清除 */
.table-view :deep(.n-data-table-td.tv-cell-dirty) {
  background: rgba(232, 168, 56, 0.12) !important;
  box-shadow: inset 3px 0 0 #e8a838;
}
.table-view :deep(.n-data-table-resize-button) {
  /* 加宽拖动热区, 更容易抓住 */
  width: 8px !important;
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

/* 复杂单元格 hover 浮层 (Teleport 到 body) */
.cell-hover-float {
  position: fixed;
  z-index: 9999;
  display: flex;
  flex-direction: column;
  background: #fff;
  border: 1px solid #e0e0e6;
  border-radius: 6px;
  box-shadow: 0 6px 24px rgba(0, 0, 0, 0.16);
  overflow: hidden;
  font-size: 12px;
}
.cell-hover-head {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 10px;
  border-bottom: 1px solid #eee;
  background: #fafafa;
}
.chh-title {
  font-weight: 600;
  color: #666;
}
.chh-copy-all {
  flex: 0 0 auto;
  border: 1px solid #d0d0d6;
  background: #fff;
  border-radius: 4px;
  padding: 2px 8px;
  font-size: 12px;
  color: #333;
  cursor: pointer;
}
.chh-copy-all:hover {
  background: #f0f0f0;
}
.cell-hover-body {
  flex: 1 1 auto;
  min-height: 0;
  overflow: auto;
  padding: 4px 0;
}
.chh-row {
  display: flex;
  align-items: baseline;
  gap: 6px;
  padding: 3px 10px;
  cursor: pointer;
  font-family: "Consolas", "Menlo", monospace;
  white-space: nowrap;
}
.chh-row:hover {
  background: #f2f6ff;
}
.chh-key {
  flex: 0 0 auto;
  color: #c678dd;
}
.chh-colon {
  flex: 0 0 auto;
  color: #999;
}
.chh-val {
  flex: 0 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}
.chh-copy-icon {
  margin-left: auto;
  flex: 0 0 auto;
  color: #ccc;
}
.chh-row:hover .chh-copy-icon {
  color: #61afef;
}
</style>

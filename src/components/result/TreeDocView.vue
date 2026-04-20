<script setup lang="ts">
import { ref, computed, watch, nextTick } from "vue";
import { NTooltip, NDropdown, NCheckbox, useMessage } from "naive-ui";
import { getBsonType, formatTreeValue, extractIdDisplay, getTypeColor, getValueColor } from "@/utils/bson-format";
import { highlightKeyword } from "@/utils/text-highlight";
import ValueDetail from "./ValueDetail.vue";
import DocumentViewer from "./DocumentViewer.vue";

const props = defineProps<{
  documents: Record<string, unknown>[];
  rowOffset?: number;
  collection?: string;
  docKeyFn?: (doc: Record<string, unknown>) => string | null;
  selectedKeys?: Set<string>;
  searchKeyword?: string;
  matchCase?: boolean;
  activeMatchDocIndex?: number;
  matchDocIndexes?: number[];
}>();

const emit = defineEmits<{
  toggleSelect: [key: string];
  setSelection: [keys: string[]];
  editInTab: [payload: { doc: Record<string, unknown>; queryText: string }];
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

// 值详情
const showDetail = ref(false);
const detailField = ref("");
const detailValue = ref<unknown>(null);

// 文档查看器
const showDocViewer = ref(false);
const docViewerIndex = ref(0);

// 右键菜单
const message = useMessage();
const showCtxMenu = ref(false);
const ctxMenuX = ref(0);
const ctxMenuY = ref(0);
const ctxRow = ref<RowItem | null>(null);

const ctxMenuOptions = computed(() => {
  const r = ctxRow.value;
  if (!r) return [];
  const items = [
    { label: "复制值", key: "copy-value" },
  ];
  if (r.isDocRoot) {
    items.unshift({ label: "复制文档 (JSON)", key: "copy-doc" });
  } else {
    items.push({ label: "复制字段名", key: "copy-key" });
  }
  if (r.isDocRoot) {
    items.push({ label: "查看文档", key: "view-doc" });
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
    }
  } catch {
    message.error("复制失败");
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

function openDetail(field: string, val: unknown, e: MouseEvent) {
  e.stopPropagation();
  detailField.value = field;
  detailValue.value = val;
  showDetail.value = true;
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
          <td class="col-value">
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
            <!-- Object/Array 字段：hover 预览，点击弹详情 -->
            <template v-else-if="row.isObjectField">
              <n-tooltip trigger="hover" placement="bottom-start" :delay="400" style="max-width: 500px">
                <template #trigger>
                  <span
                    class="clickable-value"
                    @click.stop="openDetail(row.key, row.value, $event)"
                    v-html="hl(row.displayValue)"
                  />
                </template>
                <pre class="tooltip-preview">{{ objectPreview(row.value, row.type) }}</pre>
              </n-tooltip>
            </template>
            <span
              v-else
              :style="{ color: getValueColor(row.type) }"
              v-html="hl(row.displayValue)"
            />
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
</style>

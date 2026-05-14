<script setup lang="ts">
import { ref, computed, watch } from "vue";
import {
  NModal, NCard, NButton, NIcon, NInput, NCheckbox, NSelect, NScrollbar,
  NInputNumber, useMessage,
} from "naive-ui";
import {
  AddCircle as AddIcon,
  Play as RunIcon,
  EyeOutline as PreviewIcon,
} from "@vicons/ionicons5";
import * as collApi from "@/api/collectionMgmt";
import * as aiApi from "@/api/ai";
import type { IndexInfo } from "@/types/document";

const props = defineProps<{
  show: boolean;
  connectionId: string;
  database: string;
  collection: string;
  /** 编辑模式: 提供已存在的索引信息, 提交时会先 dropIndex 再 createIndex */
  editingIndex?: IndexInfo | null;
}>();

const emit = defineEmits<{
  "update:show": [value: boolean];
  created: [];
}>();

interface FieldRow {
  name: string;
  checked: boolean;
  /** "ascending" | "descending" | "text" | "hashed" | "2d" | "2dsphere" */
  type: string;
}

const rows = ref<FieldRow[]>([]);
const showCheckedOnly = ref(false);
const buildBackground = ref(false);
const unique = ref(false);
const sparse = ref(false);
const ttlEnabled = ref(false);
const ttlSeconds = ref<number | null>(null);
const customName = ref("");
const previewVisible = ref(false);
const submitting = ref(false);
const loadingFields = ref(false);

const message = useMessage();

const indexTypeOptions = [
  { label: "ascending", value: "ascending" },
  { label: "descending", value: "descending" },
  { label: "text", value: "text" },
  { label: "hashed", value: "hashed" },
  { label: "2d", value: "2d" },
  { label: "2dsphere", value: "2dsphere" },
];

/** 把 BSON key 值映射回下拉选项 */
function keyValueToType(v: unknown): string {
  if (v === 1 || v === "1") return "ascending";
  if (v === -1 || v === "-1") return "descending";
  if (v === "text") return "text";
  if (v === "hashed") return "hashed";
  if (v === "2d") return "2d";
  if (v === "2dsphere") return "2dsphere";
  return "ascending";
}

async function loadFields() {
  loadingFields.value = true;
  try {
    const schema = await aiApi.analyzeSchema(
      props.connectionId,
      props.database,
      props.collection,
      50,
    );
    const fields: FieldRow[] = [];
    // Wildcard index 候选放最前
    fields.push({ name: "$**", checked: false, type: "text" });
    for (const f of schema.fields) {
      // 默认 _id 已存在但仍允许加复合索引中作为前缀; 用户挑就行
      fields.push({ name: f.name, checked: false, type: "ascending" });
    }
    rows.value = fields;
  } catch (e) {
    // 拉不到 schema 不阻塞: 至少给个 $** + 手动加字段
    rows.value = [{ name: "$**", checked: false, type: "text" }];
    message.warning(`无法加载字段列表: ${e}`);
  } finally {
    loadingFields.value = false;
  }

  // 编辑模式: 用 editingIndex 预填行 + 选项
  const ei = props.editingIndex;
  if (ei) {
    const keysEntries = Object.entries(ei.keys || {});
    // schema 里没出现过的字段补上 (置顶, 已勾选)
    for (const [fname, fval] of keysEntries) {
      const exists = rows.value.find((r) => r.name === fname);
      if (exists) {
        exists.checked = true;
        exists.type = keyValueToType(fval);
      } else {
        rows.value.unshift({ name: fname, checked: true, type: keyValueToType(fval) });
      }
    }
    unique.value = !!ei.unique;
    sparse.value = !!ei.sparse;
    customName.value = ei.name || "";
    showCheckedOnly.value = true;
  }
}

watch(() => props.show, (v) => {
  if (v) {
    loadFields();
    showCheckedOnly.value = false;
    buildBackground.value = false;
    unique.value = false;
    sparse.value = false;
    ttlEnabled.value = false;
    ttlSeconds.value = null;
    customName.value = "";
    previewVisible.value = false;
    submitting.value = false;
  }
});

const visibleRows = computed(() =>
  showCheckedOnly.value ? rows.value.filter((r) => r.checked) : rows.value,
);
const checkedCount = computed(() => rows.value.filter((r) => r.checked).length);
const allChecked = computed(
  () => rows.value.length > 0 && rows.value.every((r) => r.checked),
);
const someChecked = computed(() => checkedCount.value > 0 && !allChecked.value);

function toggleAll(v: boolean) {
  for (const r of rows.value) r.checked = v;
}

function addCustomField() {
  const name = window.prompt("输入字段名 (支持嵌套路径, 如 user.profile.age):");
  const trimmed = name?.trim();
  if (!trimmed) return;
  if (rows.value.some((r) => r.name === trimmed)) {
    // 已存在就勾上
    const exists = rows.value.find((r) => r.name === trimmed);
    if (exists) exists.checked = true;
    return;
  }
  rows.value.unshift({ name: trimmed, checked: true, type: "ascending" });
}

function buildKeys(): Record<string, number | string> {
  const keys: Record<string, number | string> = {};
  for (const r of rows.value) {
    if (!r.checked) continue;
    switch (r.type) {
      case "ascending": keys[r.name] = 1; break;
      case "descending": keys[r.name] = -1; break;
      case "text": keys[r.name] = "text"; break;
      case "hashed": keys[r.name] = "hashed"; break;
      case "2d": keys[r.name] = "2d"; break;
      case "2dsphere": keys[r.name] = "2dsphere"; break;
    }
  }
  return keys;
}

function buildOptions() {
  return {
    name: customName.value.trim() || undefined,
    unique: unique.value || undefined,
    sparse: sparse.value || undefined,
    background: buildBackground.value || undefined,
    expireAfterSeconds:
      ttlEnabled.value && ttlSeconds.value != null && ttlSeconds.value >= 0
        ? Number(ttlSeconds.value)
        : undefined,
  };
}

const previewCommand = computed(() => {
  const keys = buildKeys();
  if (Object.keys(keys).length === 0) return "请先勾选字段";
  const opts = buildOptions();
  const optsTrimmed: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(opts)) if (v !== undefined) optsTrimmed[k] = v;
  const collRef = props.collection.includes(".")
    ? `db.getCollection("${props.collection}")`
    : `db.${props.collection}`;
  const keysStr = JSON.stringify(keys);
  return Object.keys(optsTrimmed).length > 0
    ? `${collRef}.createIndex(${keysStr}, ${JSON.stringify(optsTrimmed)})`
    : `${collRef}.createIndex(${keysStr})`;
});

async function handleRun() {
  const keys = buildKeys();
  if (Object.keys(keys).length === 0) {
    message.warning("请至少勾选一个字段");
    return;
  }
  submitting.value = true;
  try {
    // 编辑模式: 先 drop 老索引 (_id_ 不允许删)
    const ei = props.editingIndex;
    if (ei && ei.name && ei.name !== "_id_") {
      try {
        await collApi.dropIndex(props.connectionId, props.database, props.collection, ei.name);
      } catch (e) {
        message.error(`删除旧索引失败: ${e}`);
        submitting.value = false;
        return;
      }
    }
    const name = await collApi.createIndex(
      props.connectionId,
      props.database,
      props.collection,
      keys,
      buildOptions(),
    );
    message.success(ei ? `索引 ${name} 已更新` : `索引 ${name} 已创建`);
    emit("created");
    emit("update:show", false);
  } catch (e) {
    message.error(`${props.editingIndex ? "更新" : "创建"}失败: ${e}`);
  } finally {
    submitting.value = false;
  }
}

function handlePreview() {
  previewVisible.value = !previewVisible.value;
}

function handleCancel() {
  if (submitting.value) return;
  emit("update:show", false);
}
</script>

<template>
  <n-modal :show="show" :mask-closable="!submitting" @update:show="emit('update:show', $event)">
    <n-card
      :title="editingIndex ? `Edit Index — ${editingIndex.name}` : 'Add Index'"
      :bordered="false"
      :closable="!submitting"
      style="width: 600px"
      @close="handleCancel"
    >
      <!-- 工具栏 -->
      <div class="ai-toolbar">
        <n-button size="small" :disabled="submitting" @click="addCustomField">
          <template #icon><n-icon :size="14"><AddIcon /></n-icon></template>
          Add Field
        </n-button>
        <n-checkbox v-model:checked="showCheckedOnly" :disabled="submitting">
          Show Checked
        </n-checkbox>
        <span class="ai-counter">{{ checkedCount }} / {{ rows.length }}</span>
      </div>

      <!-- 字段表格 -->
      <div class="ai-table">
        <div class="ai-row ai-header">
          <div class="col-check">
            <n-checkbox
              :checked="allChecked"
              :indeterminate="someChecked"
              :disabled="submitting"
              @update:checked="toggleAll"
            />
          </div>
          <div class="col-name">Field</div>
          <div class="col-type">Index Type</div>
        </div>
        <n-scrollbar style="max-height: 260px">
          <div v-if="loadingFields" class="ai-empty">正在加载字段...</div>
          <div v-else-if="visibleRows.length === 0" class="ai-empty">
            {{ showCheckedOnly ? "暂无勾选字段" : "暂无字段" }}
          </div>
          <div
            v-for="(row) in visibleRows"
            :key="row.name"
            class="ai-row"
            :class="{ checked: row.checked }"
          >
            <div class="col-check">
              <n-checkbox v-model:checked="row.checked" :disabled="submitting" />
            </div>
            <div class="col-name" :title="row.name">{{ row.name }}</div>
            <div class="col-type">
              <n-select
                v-model:value="row.type"
                :options="indexTypeOptions"
                size="small"
                :disabled="submitting || !row.checked"
                style="width: 100%"
              />
            </div>
          </div>
        </n-scrollbar>
      </div>

      <!-- 选项 -->
      <div class="ai-options">
        <n-checkbox v-model:checked="buildBackground" :disabled="submitting">
          Build index in the background
          <span class="hint">(MongoDB 4.2+ 已自动后台构建)</span>
        </n-checkbox>
        <n-checkbox v-model:checked="unique" :disabled="submitting">Create unique index</n-checkbox>
        <n-checkbox v-model:checked="sparse" :disabled="submitting">Create sparse index</n-checkbox>
        <div class="ai-ttl">
          <n-checkbox v-model:checked="ttlEnabled" :disabled="submitting">
            Expire after
          </n-checkbox>
          <n-input-number
            v-model:value="ttlSeconds"
            :disabled="!ttlEnabled || submitting"
            :min="0"
            size="small"
            placeholder=""
            style="width: 140px"
          />
          <span class="hint">seconds (TTL Index)</span>
        </div>
        <n-input
          v-model:value="customName"
          size="small"
          placeholder="Enter a custom index name here"
          :disabled="submitting"
        />
      </div>

      <!-- Preview 内容 -->
      <div v-if="previewVisible" class="ai-preview">
        <div class="ai-preview-label">Shell 等效命令:</div>
        <pre class="ai-preview-code">{{ previewCommand }}</pre>
      </div>

      <template #action>
        <div class="ai-footer">
          <n-button
            type="primary"
            size="small"
            :loading="submitting"
            :disabled="checkedCount === 0"
            @click="handleRun"
          >
            <template #icon><n-icon :size="14"><RunIcon /></n-icon></template>
            Run
          </n-button>
          <n-button size="small" :disabled="submitting" @click="handlePreview">
            <template #icon><n-icon :size="14"><PreviewIcon /></n-icon></template>
            Preview
          </n-button>
          <n-button size="small" :disabled="submitting" @click="handleCancel">Cancel</n-button>
        </div>
      </template>
    </n-card>
  </n-modal>
</template>

<style scoped>
.ai-toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 0 10px;
  border-bottom: 1px solid #eee;
  margin-bottom: 6px;
}
.ai-counter {
  margin-left: auto;
  color: #999;
  font-size: 12px;
}
.ai-table {
  border: 1px solid #e8e8e8;
  border-radius: 4px;
  overflow: hidden;
}
.ai-row {
  display: grid;
  grid-template-columns: 36px minmax(0, 1fr) 150px;
  align-items: center;
  column-gap: 8px;
  padding: 4px 8px;
  border-bottom: 1px solid #f5f5f5;
}
.ai-row.ai-header {
  background: #fafafa;
  font-weight: 600;
  font-size: 12px;
  color: #555;
}
.ai-row.checked {
  background: #f0f7ff;
}
.ai-row:not(.ai-header):hover {
  background: #f6f9fd;
}
.col-check {
  display: flex;
  align-items: center;
  justify-content: center;
}
.col-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
}
.col-type {
  min-width: 0;
}
.ai-empty {
  padding: 20px;
  text-align: center;
  color: #999;
  font-size: 12px;
}
.ai-options {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px 0 4px;
}
.ai-options .hint {
  margin-left: 6px;
  color: #999;
  font-size: 12px;
}
.ai-ttl {
  display: flex;
  align-items: center;
  gap: 8px;
}
.ai-preview {
  margin-top: 10px;
  padding: 8px 10px;
  background: #f7f9fb;
  border: 1px solid #e0e0e0;
  border-radius: 4px;
}
.ai-preview-label {
  font-size: 12px;
  color: #888;
  margin-bottom: 4px;
}
.ai-preview-code {
  margin: 0;
  font-family: "Consolas", "Monaco", monospace;
  font-size: 12px;
  color: #333;
  white-space: pre-wrap;
  word-break: break-all;
}
.ai-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>

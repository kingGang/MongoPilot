<script setup lang="ts">
import { ref, watch, computed, h } from "vue";
import { NModal, NCard, NButton, NInput, NSelect, NCheckbox, NDropdown, NIcon } from "naive-ui";
import { Refresh as ClearIcon, CheckmarkCircle as OkIcon } from "@vicons/ionicons5";
import QueryRuleGroupNode from "./QueryRuleGroupNode.vue";
import {
  buildQueryString,
  newGroup,
  parseFindQuery,
  type BuilderExtras,
  type Group,
} from "@/utils/query-builder";
import * as aiApi from "@/api/ai";

const props = defineProps<{
  show: boolean;
  /** 用于拉集合 schema —— 无则不做字段补全 */
  connectionId?: string;
  database: string;
  collection: string;
  /** 编辑器里的原始文本, 用于反向解析回填 */
  initialQueryText?: string;
}>();

const emit = defineEmits<{
  "update:show": [value: boolean];
  /** OK and Run —— 生成 + 执行 */
  run: [queryText: string];
  /** Just OK —— 生成贴到编辑器, 不执行 */
  insert: [queryText: string];
}>();

const rootGroup = ref<Group>(newGroup({ logic: "And" }));
const extras = ref<BuilderExtras>({
  projection: [],
  sort: ["-_id"],
  skip: "",
  limit: "100",
  useFluentApi: false,
});

// 集合字段名缓存, 用于 Field / Projection / Sort 下拉候选
const fields = ref<string[]>([]);

/** projection 选项: 每个字段两选 (include / exclude) */
const projectionOptions = computed(() => {
  const items: { label: string; value: string }[] = [];
  for (const f of fields.value) {
    items.push({ label: f, value: f });
    items.push({ label: `-${f}  (exclude)`, value: `-${f}` });
  }
  return items;
});

/** sort 选项: 每个字段两选 (asc / desc) */
const sortOptions = computed(() => {
  const items: { label: string; value: string }[] = [];
  for (const f of fields.value) {
    items.push({ label: `${f}  (asc)`, value: f });
    items.push({ label: `-${f}  (desc)`, value: `-${f}` });
  }
  return items;
});

async function loadFields() {
  if (!props.connectionId || !props.database || !props.collection) {
    fields.value = [];
    return;
  }
  try {
    const schema = await aiApi.analyzeSchema(
      props.connectionId,
      props.database,
      props.collection,
      50,
    );
    fields.value = schema.fields.map((f) => f.name);
  } catch {
    fields.value = [];
  }
}

/** 弹窗显示时回填 + 拉字段 */
watch(() => props.show, (v) => {
  if (!v) return;
  loadFields();
  if (props.initialQueryText && props.initialQueryText.trim()) {
    try {
      const parsed = parseFindQuery(props.initialQueryText, props.collection);
      rootGroup.value = parsed.filter;
      extras.value = { ...parsed.extras };
      if (!extras.value.sort || extras.value.sort.length === 0) extras.value.sort = ["-_id"];
      if (!extras.value.limit) extras.value.limit = "100";
      return;
    } catch {
      /* fall through to empty */
    }
  }
  rootGroup.value = newGroup({ logic: "And" });
  extras.value = {
    projection: [],
    sort: ["-_id"],
    skip: "",
    limit: "100",
    useFluentApi: false,
  };
});

const title = computed(
  () => `Visual Query Builder --- database: ${props.database || "(none)"} collection: ${props.collection || "(none)"}`,
);

function handleClear() {
  rootGroup.value = newGroup({ logic: "And" });
  extras.value = {
    projection: [],
    sort: ["-_id"],
    skip: "",
    limit: "100",
    useFluentApi: extras.value.useFluentApi,
  };
}

function buildText(): string {
  return buildQueryString(props.collection || "collection", rootGroup.value, extras.value);
}

function handleOkAndRun() {
  emit("run", buildText());
  emit("update:show", false);
}

function handleOkInsert() {
  emit("insert", buildText());
  emit("update:show", false);
}

function handleCancel() {
  emit("update:show", false);
}

const okMenuOptions = [
  {
    label: "OK and Run",
    key: "run",
    icon: () => h(NIcon, null, { default: () => h(OkIcon) }),
  },
  {
    label: "OK (插入编辑器不执行)",
    key: "insert",
  },
];

function onOkMenuSelect(key: string) {
  if (key === "run") handleOkAndRun();
  else if (key === "insert") handleOkInsert();
}
</script>

<template>
  <n-modal :show="props.show" @update:show="emit('update:show', $event)">
    <n-card
      style="width: min(1040px, 94vw); max-height: 88vh; display: flex; flex-direction: column"
      :title="title"
      :bordered="false"
      closable
      @close="handleCancel"
    >
      <div class="qb-body">
        <QueryRuleGroupNode :group="rootGroup" :is-root="true" :fields="fields" />

        <div class="qb-extras">
          <div class="extra-row">
            <label class="extra-label">Projection</label>
            <n-select
              v-model:value="extras.projection"
              :options="projectionOptions"
              multiple
              filterable
              tag
              clearable
              size="small"
              placeholder="选择字段 (前缀 - 表示 exclude; 可手写)"
              style="flex: 1"
            />
          </div>
          <div class="extra-row">
            <label class="extra-label"><strong>Sort</strong></label>
            <n-select
              v-model:value="extras.sort"
              :options="sortOptions"
              multiple
              filterable
              tag
              clearable
              size="small"
              placeholder="选择字段 (前缀 - 表示 desc; 可手写)"
              style="flex: 1"
            />
            <label class="extra-label inline"><strong>Skip</strong></label>
            <n-input
              v-model:value="extras.skip"
              size="small"
              placeholder=""
              style="width: 80px"
            />
            <label class="extra-label inline"><strong>Limit</strong></label>
            <n-input
              v-model:value="extras.limit"
              size="small"
              placeholder=""
              style="width: 90px"
            />
          </div>
        </div>
      </div>

      <template #footer>
        <div class="qb-footer">
          <div class="qb-footer-left">
            <n-checkbox v-model:checked="extras.useFluentApi">Use Fluent API</n-checkbox>
            <n-button size="small" quaternary @click="handleClear">
              <template #icon><n-icon><ClearIcon /></n-icon></template>
              Clear
            </n-button>
          </div>
          <div class="qb-footer-right">
            <n-dropdown
              trigger="click"
              :options="okMenuOptions"
              @select="onOkMenuSelect"
            >
              <n-button type="primary" size="small">
                <template #icon><n-icon><OkIcon /></n-icon></template>
                OK and Run ▾
              </n-button>
            </n-dropdown>
            <n-button size="small" @click="handleCancel">Cancel</n-button>
          </div>
        </div>
      </template>
    </n-card>
  </n-modal>
</template>

<style scoped>
.qb-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow-y: auto;
  max-height: calc(88vh - 160px);
}
.qb-extras {
  border-top: 1px solid #e0e0e0;
  padding-top: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.extra-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.extra-label {
  flex: 0 0 90px;
  font-size: 13px;
  color: #444;
}
.extra-label.inline {
  flex: 0 0 auto;
  margin-left: 8px;
  padding: 0 4px;
}
.qb-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.qb-footer-left,
.qb-footer-right {
  display: flex;
  gap: 8px;
  align-items: center;
}
</style>

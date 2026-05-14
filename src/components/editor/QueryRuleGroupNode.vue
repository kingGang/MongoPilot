<script setup lang="ts">
import { NButton, NIcon, NSelect, NInput, NCheckbox } from "naive-ui";
import {
  AddCircle as AddIcon,
  Layers as GroupIcon,
  CloseCircle as DeleteIcon,
  PowerSharp as TogglePowerIcon,
  CopyOutline as CopyIcon,
} from "@vicons/ionicons5";
import {
  type Group,
  type Rule,
  type GroupLogic,
  type RuleOp,
  type ValueType,
  OP_LABELS,
  newRule,
  newGroup,
} from "@/utils/query-builder";

import { computed } from "vue";

const props = defineProps<{
  group: Group;
  /** 是否是根 group (根节点不显示左上角删除按钮) */
  isRoot?: boolean;
  /** 集合字段名列表 (用于 Field 下拉候选) */
  fields?: string[];
}>();

const fieldOptions = computed(() =>
  (props.fields ?? []).map((f) => ({ label: f, value: f })),
);

const emit = defineEmits<{
  removeGroup: [];
}>();

const logicOptions: { value: GroupLogic; label: string }[] = [
  { value: "And", label: "And" },
  { value: "Or", label: "Or" },
];

const opOptions = (Object.keys(OP_LABELS) as RuleOp[]).map((op) => ({
  value: op,
  label: OP_LABELS[op],
}));

const typeOptions: { value: ValueType; label: string }[] = [
  { value: "String", label: "String" },
  { value: "Number", label: "Number" },
  { value: "Boolean", label: "Boolean" },
  { value: "Date", label: "Date" },
  { value: "ObjectId", label: "ObjectId" },
  { value: "Null", label: "Null" },
];

function addRule(g: Group) {
  g.items.push(newRule());
}
function addGroup(g: Group) {
  g.items.push(newGroup());
}
function removeItem(g: Group, idx: number) {
  g.items.splice(idx, 1);
}
function duplicateItem(g: Group, idx: number) {
  const it = g.items[idx];
  if (it.kind === "rule") {
    g.items.splice(idx + 1, 0, newRule({ ...it, id: undefined as unknown as string }));
  } else {
    // 复制组 —— 浅复制 items 引用(简单处理; 实际深拷贝更安全但暂不需要)
    g.items.splice(idx + 1, 0, newGroup({ logic: it.logic, items: [...it.items] }));
  }
}
function toggleRuleEnabled(r: Rule) {
  r.disabled = !r.disabled;
}
</script>

<template>
  <div class="rule-group" :class="{ 'is-root': isRoot }">
    <div class="group-header">
      <n-checkbox :checked="group.logic === 'Not'" @update:checked="group.logic = $event ? 'Not' : 'And'">
        Not
      </n-checkbox>
      <div class="logic-toggle">
        <button
          v-for="opt in logicOptions"
          :key="opt.value"
          class="logic-btn"
          :class="{ active: group.logic === opt.value }"
          type="button"
          @click="group.logic = opt.value"
        >{{ opt.label }}</button>
      </div>
      <div class="group-actions">
        <n-button size="tiny" text type="success" @click="addRule(group)">
          <template #icon><n-icon><AddIcon /></n-icon></template>
          Rule
        </n-button>
        <n-button size="tiny" text type="success" @click="addGroup(group)">
          <template #icon><n-icon><GroupIcon /></n-icon></template>
          Group
        </n-button>
        <n-button
          v-if="!isRoot"
          size="tiny"
          text
          type="error"
          title="删除该组"
          @click="emit('removeGroup')"
        >
          <template #icon><n-icon><DeleteIcon /></n-icon></template>
        </n-button>
      </div>
    </div>

    <div class="group-body">
      <template v-for="(item, idx) in group.items" :key="item.id">
        <div v-if="item.kind === 'rule'" class="rule-row" :class="{ disabled: item.disabled }">
          <n-select
            v-model:value="item.field"
            :options="fieldOptions"
            size="small"
            filterable
            tag
            clearable
            placeholder="Field"
            style="flex: 1 1 180px; min-width: 120px"
          />
          <n-select
            v-model:value="item.op"
            :options="opOptions"
            size="small"
            style="flex: 0 0 120px"
          />
          <n-select
            v-if="!['exists', 'regex', 'type'].includes(item.op)"
            v-model:value="item.valueType"
            :options="typeOptions"
            size="small"
            style="flex: 0 0 100px"
          />
          <n-input
            v-model:value="item.value"
            size="small"
            :placeholder="item.op === 'in' || item.op === 'nin' ? '逗号分隔: a, b, c' : 'value'"
            style="flex: 1 1 160px; min-width: 120px"
          />
          <n-button
            size="tiny"
            text
            :type="item.disabled ? 'default' : 'success'"
            :title="item.disabled ? '启用此规则' : '禁用此规则'"
            @click="toggleRuleEnabled(item)"
          >
            <template #icon><n-icon><TogglePowerIcon /></n-icon></template>
          </n-button>
          <n-button size="tiny" text title="复制" @click="duplicateItem(group, idx)">
            <template #icon><n-icon><CopyIcon /></n-icon></template>
          </n-button>
          <n-button size="tiny" text type="error" title="删除" @click="removeItem(group, idx)">
            <template #icon><n-icon><DeleteIcon /></n-icon></template>
          </n-button>
        </div>
        <QueryRuleGroupNode
          v-else
          :group="item"
          :fields="fields"
          @remove-group="removeItem(group, idx)"
        />
      </template>
    </div>
  </div>
</template>

<style scoped>
.rule-group {
  border: 1px dashed #d0d0d0;
  border-radius: 4px;
  background: #fafafa;
  padding: 8px 10px;
  margin: 6px 0;
}
.rule-group.is-root {
  border: 1px solid #c8e6c9;
  background: #fff;
}
.group-header {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 6px;
}
.logic-toggle {
  display: inline-flex;
  gap: 2px;
  background: #eef2f5;
  padding: 2px;
  border-radius: 4px;
}
.logic-btn {
  border: none;
  background: transparent;
  padding: 3px 12px;
  border-radius: 3px;
  font-size: 12px;
  color: #555;
  cursor: pointer;
}
.logic-btn.active {
  background: #18a058;
  color: #fff;
  font-weight: 500;
}
.group-actions {
  margin-left: auto;
  display: inline-flex;
  gap: 6px;
}
.group-body {
  padding-left: 12px;
  border-left: 2px solid #e0e0e0;
}
.rule-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 0;
  flex-wrap: wrap;
}
.rule-row.disabled {
  opacity: 0.45;
}
</style>

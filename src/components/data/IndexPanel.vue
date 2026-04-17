<script setup lang="ts">
import { ref, onMounted } from "vue";
import {
  NDataTable, NButton, NModal, NCard, NForm, NFormItem, NInput, NSwitch,
  NInputNumber, NSpace, useMessage, useDialog,
} from "naive-ui";
import type { DataTableColumns } from "naive-ui";
import * as collApi from "@/api/collectionMgmt";
import type { IndexInfo, CreateIndexOptions } from "@/types/document";

const props = defineProps<{
  connectionId: string;
  database: string;
  collection: string;
}>();

const message = useMessage();
const dialog = useDialog();
const indexes = ref<IndexInfo[]>([]);
const showCreate = ref(false);
const newKeyField = ref("");
const newKeyOrder = ref(1);
const newKeys = ref<{ field: string; order: number }[]>([]);
const newOptions = ref<CreateIndexOptions>({ unique: false, sparse: false });

onMounted(loadIndexes);

async function loadIndexes() {
  try {
    indexes.value = await collApi.listIndexes(props.connectionId, props.database, props.collection);
  } catch (e) {
    message.error(`获取索引失败: ${e}`);
  }
}

function addKey() {
  if (newKeyField.value.trim()) {
    newKeys.value.push({ field: newKeyField.value, order: newKeyOrder.value });
    newKeyField.value = "";
  }
}

function removeKey(index: number) {
  newKeys.value.splice(index, 1);
}

async function handleCreate() {
  if (newKeys.value.length === 0) {
    message.warning("请至少添加一个索引字段");
    return;
  }
  const keys: Record<string, number> = {};
  for (const k of newKeys.value) keys[k.field] = k.order;

  try {
    const name = await collApi.createIndex(
      props.connectionId, props.database, props.collection, keys, newOptions.value,
    );
    message.success(`索引 ${name} 已创建`);
    showCreate.value = false;
    newKeys.value = [];
    newOptions.value = { unique: false, sparse: false };
    await loadIndexes();
  } catch (e) {
    message.error(`创建失败: ${e}`);
  }
}

async function handleDrop(indexName: string) {
  if (indexName === "_id_") {
    message.warning("不能删除 _id 索引");
    return;
  }
  dialog.warning({
    title: "确认删除",
    content: `确定要删除索引 "${indexName}" 吗？`,
    positiveText: "删除",
    negativeText: "取消",
    onPositiveClick: async () => {
      try {
        await collApi.dropIndex(props.connectionId, props.database, props.collection, indexName);
        message.success(`索引 ${indexName} 已删除`);
        await loadIndexes();
      } catch (e) {
        message.error(`删除失败: ${e}`);
      }
    },
  });
}

const columns: DataTableColumns = [
  { title: "名称", key: "name", width: 180 },
  { title: "键", key: "keys", render: (row: any) => JSON.stringify(row.keys) },
  { title: "唯一", key: "unique", width: 80, render: (row: any) => row.unique ? "是" : "否" },
  { title: "稀疏", key: "sparse", width: 80, render: (row: any) => row.sparse ? "是" : "否" },
  {
    title: "操作", key: "actions", width: 80,
    render: (row: any) => {
      if (row.name === "_id_") return "";
      return `删除|${handleDrop.name}`.split("|")[0];
    },
  },
];
</script>

<template>
  <div class="index-panel">
    <n-space vertical>
      <n-button size="small" type="primary" @click="showCreate = true">新建索引</n-button>

      <n-data-table :columns="columns" :data="indexes" :row-key="(row: any) => row.name" size="small" />
    </n-space>

    <n-modal v-model:show="showCreate">
      <n-card style="width: 500px" title="新建索引" closable @close="showCreate = false">
        <n-form label-placement="left" label-width="80">
          <n-form-item label="字段">
            <n-space>
              <n-input v-model:value="newKeyField" placeholder="字段名" style="width: 200px" />
              <n-input-number v-model:value="newKeyOrder" :min="-1" :max="1" style="width: 80px" />
              <n-button size="small" @click="addKey">添加</n-button>
            </n-space>
          </n-form-item>
          <div v-for="(k, i) in newKeys" :key="i" class="key-item">
            {{ k.field }}: {{ k.order === 1 ? "升序" : "降序" }}
            <n-button size="tiny" quaternary type="error" @click="removeKey(i)">移除</n-button>
          </div>
          <n-form-item label="唯一">
            <n-switch v-model:value="newOptions.unique" />
          </n-form-item>
          <n-form-item label="稀疏">
            <n-switch v-model:value="newOptions.sparse" />
          </n-form-item>
        </n-form>
        <template #footer>
          <n-space justify="end">
            <n-button @click="showCreate = false">取消</n-button>
            <n-button type="primary" @click="handleCreate">创建</n-button>
          </n-space>
        </template>
      </n-card>
    </n-modal>
  </div>
</template>

<style scoped>
.index-panel { padding: 8px; }
.key-item { padding: 4px 8px; margin-bottom: 4px; display: flex; justify-content: space-between; align-items: center; background: rgba(128,128,128,0.1); border-radius: 4px; font-size: 13px; }
</style>

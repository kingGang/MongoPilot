<script setup lang="ts">
import { ref } from "vue";
import { NDataTable, NButton, useMessage } from "naive-ui";
import type { DataTableColumns } from "naive-ui";
import * as aiApi from "@/api/ai";
import type { SchemaInfo, FieldInfo } from "@/types/ai";

const props = defineProps<{
  connectionId: string;
  database: string;
  collection: string;
}>();

const message = useMessage();
const schema = ref<SchemaInfo | null>(null);
const loading = ref(false);

async function analyze() {
  loading.value = true;
  try {
    schema.value = await aiApi.analyzeSchema(props.connectionId, props.database, props.collection);
  } catch (e) {
    message.error(`Schema 分析失败: ${e}`);
  } finally {
    loading.value = false;
  }
}

const columns: DataTableColumns<FieldInfo> = [
  { title: "字段", key: "name", width: 200, ellipsis: { tooltip: true } },
  {
    title: "类型", key: "fieldTypes", width: 180,
    render: (row) => row.fieldTypes.map((t) => `${t.bsonType}(${t.count})`).join(", "),
  },
  {
    title: "出现率", key: "occurrencePercent", width: 120,
    render: (row) => `${row.occurrencePercent.toFixed(1)}%`,
  },
  {
    title: "样本值", key: "sampleValues",
    render: (row) => row.sampleValues.slice(0, 2).join(", "),
    ellipsis: { tooltip: true },
  },
];
</script>

<template>
  <div class="schema-viewer">
    <n-button size="small" type="primary" :loading="loading" @click="analyze">
      分析 Schema
    </n-button>

    <template v-if="schema">
      <p style="margin: 8px 0; font-size: 13px; color: #999">
        集合: {{ schema.collection }} | 采样: {{ schema.sampleCount }} 文档 | {{ schema.fields.length }} 个字段
      </p>
      <n-data-table
        :columns="columns"
        :data="schema.fields"
        :row-key="(row: FieldInfo) => row.name"
        size="small"
        :max-height="400"
      />
    </template>
  </div>
</template>

<style scoped>
.schema-viewer { padding: 8px; }
</style>

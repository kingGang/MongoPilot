<script setup lang="ts">
import { ref, computed } from "vue";
import {
  NButton, NSpace, NInput, NModal, NCard, NDescriptions, NDescriptionsItem,
  useMessage, useDialog,
} from "naive-ui";
import * as collApi from "@/api/collectionMgmt";
import { useDatabaseStore } from "@/stores/database";
import { useConnectionStore } from "@/stores/connection";
import type { CollectionStats } from "@/types/document";

const props = defineProps<{
  connectionId: string;
  database: string;
}>();

const message = useMessage();
const dialog = useDialog();
const dbStore = useDatabaseStore();
const connStore = useConnectionStore();
const isReadOnly = computed(() => connStore.isReadOnly(props.connectionId));

const showCreate = ref(false);
const newCollName = ref("");
const showStats = ref(false);
const stats = ref<CollectionStats | null>(null);
const statsCollName = ref("");

async function handleCreate() {
  if (isReadOnly.value) {
    message.warning("只读连接: 不允许创建集合");
    return;
  }
  if (!newCollName.value.trim()) return;
  try {
    await collApi.createCollection(props.connectionId, props.database, newCollName.value);
    message.success(`集合 ${newCollName.value} 已创建`);
    showCreate.value = false;
    newCollName.value = "";
    await dbStore.fetchCollections(props.connectionId, props.database);
  } catch (e) {
    message.error(`创建失败: ${e}`);
  }
}

async function handleDrop(collName: string) {
  if (isReadOnly.value) {
    message.warning("只读连接: 不允许删除集合");
    return;
  }
  dialog.warning({
    title: "确认删除",
    content: `确定要删除集合 "${collName}" 吗？此操作不可恢复。`,
    positiveText: "删除",
    negativeText: "取消",
    onPositiveClick: async () => {
      try {
        await collApi.dropCollection(props.connectionId, props.database, collName);
        message.success(`集合 ${collName} 已删除`);
        await dbStore.fetchCollections(props.connectionId, props.database);
      } catch (e) {
        message.error(`删除失败: ${e}`);
      }
    },
  });
}

async function handleStats(collName: string) {
  try {
    statsCollName.value = collName;
    stats.value = await collApi.getCollectionStats(props.connectionId, props.database, collName);
    showStats.value = true;
  } catch (e) {
    message.error(`获取统计信息失败: ${e}`);
  }
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
</script>

<template>
  <div class="collection-panel">
    <n-space vertical>
      <n-button
        size="small"
        type="primary"
        :disabled="isReadOnly"
        :title="isReadOnly ? '只读连接, 禁用创建集合' : ''"
        @click="showCreate = true"
      >
        新建集合
      </n-button>

      <div
        v-for="coll in dbStore.getCollections(connectionId, database)"
        :key="coll.name"
        class="coll-item"
      >
        <span class="coll-name">{{ coll.name }}</span>
        <n-space size="small">
          <n-button size="tiny" quaternary @click="handleStats(coll.name)">统计</n-button>
          <n-button
            size="tiny"
            quaternary
            type="error"
            :disabled="isReadOnly"
            :title="isReadOnly ? '只读连接, 禁用删除' : ''"
            @click="handleDrop(coll.name)"
          >删除</n-button>
        </n-space>
      </div>
    </n-space>

    <n-modal v-model:show="showCreate">
      <n-card style="width: 400px" title="新建集合" closable @close="showCreate = false">
        <n-input v-model:value="newCollName" placeholder="集合名称" />
        <template #footer>
          <n-space justify="end">
            <n-button @click="showCreate = false">取消</n-button>
            <n-button type="primary" @click="handleCreate">创建</n-button>
          </n-space>
        </template>
      </n-card>
    </n-modal>

    <n-modal v-model:show="showStats">
      <n-card style="width: 400px" :title="`集合统计 — ${statsCollName}`" closable @close="showStats = false">
        <n-descriptions v-if="stats" label-placement="left" :column="1" bordered>
          <n-descriptions-item label="文档数">{{ stats.documentCount }}</n-descriptions-item>
          <n-descriptions-item label="总大小">{{ formatSize(stats.totalSize) }}</n-descriptions-item>
          <n-descriptions-item label="平均文档大小">{{ formatSize(stats.avgDocumentSize) }}</n-descriptions-item>
          <n-descriptions-item label="索引数量">{{ stats.indexCount }}</n-descriptions-item>
          <n-descriptions-item label="索引总大小">{{ formatSize(stats.totalIndexSize) }}</n-descriptions-item>
        </n-descriptions>
      </n-card>
    </n-modal>
  </div>
</template>

<style scoped>
.collection-panel { padding: 8px; }
.coll-item { display: flex; justify-content: space-between; align-items: center; padding: 4px 8px; border-radius: 4px; }
.coll-item:hover { background: rgba(128,128,128,0.1); }
.coll-name { font-size: 13px; }
</style>

<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount } from "vue";
import { NDescriptions, NDescriptionsItem, NStatistic, NGrid, NGi, NCard, NSpin, NButton } from "naive-ui";
import * as serverApi from "@/api/server";
import type { ServerStatus } from "@/types/server";

const props = defineProps<{ connectionId: string }>();

const status = ref<ServerStatus | null>(null);
const loading = ref(false);
let timer: ReturnType<typeof setInterval> | null = null;

onMounted(async () => {
  await refresh();
  timer = setInterval(refresh, 5000);
});

onBeforeUnmount(() => {
  if (timer) clearInterval(timer);
});

async function refresh() {
  loading.value = true;
  try {
    status.value = await serverApi.getServerStatus(props.connectionId);
  } finally {
    loading.value = false;
  }
}

function formatUptime(seconds: number): string {
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (d > 0) return `${d}天 ${h}小时`;
  if (h > 0) return `${h}小时 ${m}分钟`;
  return `${m}分钟`;
}
</script>

<template>
  <div class="server-monitor">
    <n-spin :show="loading && !status">
      <template v-if="status">
        <n-card title="服务器概览" size="small">
          <n-descriptions :column="2" label-placement="left" bordered size="small">
            <n-descriptions-item label="主机">{{ status.host }}</n-descriptions-item>
            <n-descriptions-item label="版本">MongoDB {{ status.version }}</n-descriptions-item>
            <n-descriptions-item label="运行时间">{{ formatUptime(status.uptime) }}</n-descriptions-item>
            <n-descriptions-item label="存储引擎">{{ status.storageEngine }}</n-descriptions-item>
          </n-descriptions>
        </n-card>

        <n-grid :cols="3" :x-gap="12" :y-gap="12" style="margin-top: 12px">
          <n-gi>
            <n-card title="连接" size="small">
              <n-statistic label="当前" :value="status.connections.current" />
              <n-statistic label="可用" :value="status.connections.available" />
            </n-card>
          </n-gi>
          <n-gi>
            <n-card title="操作计数" size="small">
              <n-statistic label="查询" :value="status.opcounters.query" />
              <n-statistic label="插入" :value="status.opcounters.insert" />
              <n-statistic label="更新" :value="status.opcounters.update" />
              <n-statistic label="删除" :value="status.opcounters.delete" />
            </n-card>
          </n-gi>
          <n-gi>
            <n-card title="内存 (MB)" size="small">
              <n-statistic label="常驻" :value="status.memory.resident" />
              <n-statistic label="虚拟" :value="status.memory.virtualMem" />
            </n-card>
          </n-gi>
        </n-grid>

        <div style="margin-top: 8px; text-align: right">
          <n-button size="small" quaternary @click="refresh">刷新</n-button>
        </div>
      </template>
    </n-spin>
  </div>
</template>

<style scoped>
.server-monitor { padding: 16px; }
</style>

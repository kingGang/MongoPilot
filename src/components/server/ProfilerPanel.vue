<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import {
  NDataTable, NButton, NSpace, NRadioGroup, NRadio, NInputNumber, NCard, NTag,
  useMessage,
} from "naive-ui";
import type { DataTableColumns } from "naive-ui";
import * as serverApi from "@/api/server";
import type { ProfileEntry } from "@/types/server";
import { useConnectionStore } from "@/stores/connection";

const props = defineProps<{ connectionId: string; database: string }>();

const message = useMessage();
const connStore = useConnectionStore();
const isReadOnly = computed(() => connStore.isReadOnly(props.connectionId));
const entries = ref<ProfileEntry[]>([]);
const profilerLevel = ref(0);
const slowMs = ref(100);
const loading = ref(false);

onMounted(async () => {
  await loadStatus();
  await loadData();
});

async function loadStatus() {
  try {
    const status = await serverApi.getProfilerStatus(props.connectionId, props.database);
    profilerLevel.value = (status.was as number) ?? 0;
    slowMs.value = (status.slowms as number) ?? 100;
  } catch (e) {
    message.error(`获取 Profiler 状态失败: ${e}`);
  }
}

async function loadData() {
  loading.value = true;
  try {
    entries.value = await serverApi.getProfilerData(props.connectionId, props.database, 50);
  } catch (e) {
    message.error(`获取慢查询数据失败: ${e}`);
  } finally {
    loading.value = false;
  }
}

async function handleSetLevel() {
  if (isReadOnly.value) {
    message.warning("只读连接: 不允许修改 Profiler 配置");
    return;
  }
  try {
    await serverApi.setProfilerLevel(props.connectionId, props.database, profilerLevel.value, slowMs.value);
    message.success("Profiler 级别已更新");
  } catch (e) {
    message.error(`设置失败: ${e}`);
  }
}

const columns: DataTableColumns = [
  { title: "操作", key: "op", width: 80 },
  { title: "命名空间", key: "ns", width: 200, ellipsis: { tooltip: true } },
  { title: "耗时(ms)", key: "millis", width: 100 },
  { title: "执行计划", key: "planSummary", width: 150, ellipsis: { tooltip: true } },
  { title: "时间", key: "ts", width: 180 },
];
</script>

<template>
  <div class="profiler-panel">
    <n-card title="Profiler 设置" size="small">
      <n-space align="center">
        <span>级别：</span>
        <n-radio-group v-model:value="profilerLevel">
          <n-radio :value="0">关闭</n-radio>
          <n-radio :value="1">慢查询</n-radio>
          <n-radio :value="2">全部</n-radio>
        </n-radio-group>
        <span>慢查询阈值：</span>
        <n-input-number v-model:value="slowMs" :min="0" :max="60000" size="small" style="width: 100px" />
        <span>ms</span>
        <n-button
          size="small"
          type="primary"
          :disabled="isReadOnly"
          :title="isReadOnly ? '只读连接, 禁用修改' : ''"
          @click="handleSetLevel"
        >应用</n-button>
      </n-space>
    </n-card>

    <n-space style="margin-top: 12px">
      <n-button size="small" @click="loadData" :loading="loading">刷新数据</n-button>
      <n-tag>{{ entries.length }} 条记录</n-tag>
    </n-space>

    <n-data-table
      :columns="columns"
      :data="entries"
      :row-key="(row: any) => row.ts || String(Math.random())"
      :loading="loading"
      size="small"
      style="margin-top: 8px"
    />
  </div>
</template>

<style scoped>
.profiler-panel { padding: 16px; }
</style>

<script setup lang="ts">
import { ref, onMounted } from "vue";
import { NDrawer, NDrawerContent, NInput, NList, NListItem, NButton, NSpace, NEmpty, NTag } from "naive-ui";
import * as queryApi from "@/api/query";
import type { HistoryEntry } from "@/types/database";

const props = defineProps<{ show: boolean; connectionId: string }>();
const emit = defineEmits<{ "update:show": [value: boolean]; select: [queryText: string] }>();

const history = ref<HistoryEntry[]>([]);
const searchKeyword = ref("");
const loading = ref(false);

onMounted(loadHistory);

async function loadHistory() {
  loading.value = true;
  try { history.value = await queryApi.getQueryHistory(props.connectionId, 100, 0); }
  finally { loading.value = false; }
}

async function handleSearch() {
  if (searchKeyword.value.trim()) {
    history.value = await queryApi.searchQueryHistory(props.connectionId, searchKeyword.value);
  } else { await loadHistory(); }
}

async function handleClear() {
  await queryApi.clearQueryHistory(props.connectionId);
  history.value = [];
}

function handleSelect(entry: HistoryEntry) {
  emit("select", entry.queryText);
  emit("update:show", false);
}
</script>

<template>
  <n-drawer :show="props.show" :width="480" placement="right" @update:show="emit('update:show', $event)">
    <n-drawer-content title="查询历史">
      <!-- @vue-ignore -->
      <template #header-extra>
        <n-button size="small" quaternary type="error" @click="handleClear">清空</n-button>
      </template>
      <n-space vertical>
        <n-input v-model:value="searchKeyword" placeholder="搜索查询..." clearable @input="handleSearch" />
        <n-list v-if="history.length > 0" hoverable clickable>
          <n-list-item v-for="entry in history" :key="entry.id" @click="handleSelect(entry)">
            <div class="history-item">
              <code class="query-text">{{ entry.queryText }}</code>
              <div class="history-meta">
                <n-tag v-if="entry.errorMessage" type="error" size="small">失败</n-tag>
                <n-tag v-else type="success" size="small">{{ entry.resultCount ?? 0 }} 条</n-tag>
                <span v-if="entry.executionTimeMs" class="exec-time">{{ entry.executionTimeMs }}ms</span>
                <span class="created-at">{{ entry.createdAt }}</span>
              </div>
            </div>
          </n-list-item>
        </n-list>
        <n-empty v-else description="暂无查询历史" />
      </n-space>
    </n-drawer-content>
  </n-drawer>
</template>

<style scoped>
.history-item { width: 100%; }
.query-text { display: block; font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; margin-bottom: 4px; }
.history-meta { display: flex; align-items: center; gap: 8px; font-size: 12px; color: #999; }
</style>

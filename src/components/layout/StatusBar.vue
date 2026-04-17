<script setup lang="ts">
import { computed } from "vue";
import { useConnectionStore } from "@/stores/connection";
import { useEditorStore } from "@/stores/editor";

const connStore = useConnectionStore();
const editorStore = useEditorStore();

const activeCount = computed(() => connStore.activeIds.size);
const tab = computed(() => editorStore.activeTab);

const connLabel = computed(() => {
  if (!tab.value) return "";
  const cfg = connStore.connections.find((c) => c.id === tab.value!.connectionId);
  return cfg ? (cfg.name || `${cfg.host}:${cfg.port}`) : "";
});

const pageInfo = computed(() => {
  if (!tab.value) return "";
  return `Page ${tab.value.currentPage} (${tab.value.pageSize}/page)`;
});

const execInfo = computed(() => {
  const r = tab.value?.result;
  if (!r) return "";
  const ms = r.executionTimeMs;
  const time = ms >= 1000 ? `${(ms / 1000).toFixed(3)}s` : `${ms}ms`;
  return `${r.count} rows / ${r.totalCount} total | ${time}`;
});
</script>

<template>
  <div class="status-bar">
    <div class="status-left">
      <span class="status-item conn-info">
        <span :class="activeCount > 0 ? 'dot-active' : 'dot-inactive'" />
        {{ activeCount }} connections
      </span>
      <template v-if="tab">
        <span class="status-item">{{ connLabel }}</span>
        <span class="status-item">{{ tab.database }}</span>
        <span v-if="tab.collection" class="status-item coll-info">{{ tab.collection }}</span>
      </template>
    </div>
    <div class="status-right">
      <span v-if="tab?.loading" class="status-item loading-text">Executing...</span>
      <span v-else-if="execInfo" class="status-item">{{ execInfo }}</span>
      <span v-if="tab" class="status-item">{{ pageInfo }}</span>
      <span class="status-item version">MongoPilot v0.1.0</span>
    </div>
  </div>
</template>

<style scoped>
.status-bar {
  height: 24px;
  border-top: 1px solid #d9d9d9;
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0 12px;
  font-size: 11px;
  color: #888;
  background: #f5f5f5;
  flex-shrink: 0;
}
.status-left, .status-right { display: flex; gap: 12px; align-items: center; }
.status-item { white-space: nowrap; }
.conn-info { display: flex; align-items: center; gap: 4px; }
.coll-info { color: #18a058; font-weight: 500; }
.loading-text { color: #3875d7; }
.version { color: #bbb; }
.dot-active, .dot-inactive { width: 6px; height: 6px; border-radius: 50%; display: inline-block; }
.dot-active { background: #18a058; }
.dot-inactive { background: #ccc; }
</style>

<script setup lang="ts">
import { computed } from "vue";
import { NButton, NIcon, NSpace, NTooltip, NDivider } from "naive-ui";
import {
  Play as PlayIcon,
  Stop as StopIcon,
  Time as HistoryIcon,
  CloudDownload as ImportIcon,
  CloudUpload as ExportIcon,
  Search as ExplainIcon,
  Server as ServerIcon,
  Layers as DbIcon,
} from "@vicons/ionicons5";
import { useConnectionStore } from "@/stores/connection";

const props = defineProps<{
  loading: boolean;
  executionTime?: number | null;
  resultCount?: number | null;
  connectionId?: string;
  database?: string;
  collection?: string;
  error?: string | null;
}>();

const emit = defineEmits<{
  run: [];
  stop: [];
  history: [];
  explain: [];
  queryBuilder: [];
  import: [];
  export: [];
}>();

const connStore = useConnectionStore();

const connectionLabel = computed(() => {
  if (!props.connectionId) return "";
  const cfg = connStore.connections.find((c: any) => c.id === props.connectionId);
  if (!cfg) return "";
  return cfg.name || `${cfg.host}:${cfg.port}`;
});
</script>

<template>
  <div class="query-toolbar" @mousedown.prevent>
    <div class="toolbar-left">
      <n-space align="center" :size="2">
        <!-- Run -->
        <n-tooltip trigger="hover" :delay="500">
          <template #trigger>
            <n-button
              type="primary"
              size="tiny"
              :loading="props.loading"
              @click="emit('run')"
            >
              <template #icon><n-icon :size="14"><PlayIcon /></n-icon></template>
              Run
            </n-button>
          </template>
          Run query (Ctrl+Enter)
        </n-tooltip>

        <!-- Stop -->
        <n-tooltip trigger="hover" :delay="500">
          <template #trigger>
            <n-button size="tiny" :disabled="!props.loading" quaternary @click="emit('stop')">
              <template #icon><n-icon :size="14"><StopIcon /></n-icon></template>
              Stop
            </n-button>
          </template>
          Stop running query
        </n-tooltip>

        <n-divider vertical style="margin: 0 4px" />

        <!-- Import -->
        <n-tooltip trigger="hover" :delay="500">
          <template #trigger>
            <n-button size="tiny" quaternary @click="emit('import')">
              <template #icon><n-icon :size="14"><ImportIcon /></n-icon></template>
              Import
            </n-button>
          </template>
          Import data
        </n-tooltip>

        <!-- Export -->
        <n-tooltip trigger="hover" :delay="500">
          <template #trigger>
            <n-button size="tiny" quaternary @click="emit('export')">
              <template #icon><n-icon :size="14"><ExportIcon /></n-icon></template>
              Export
            </n-button>
          </template>
          Export data
        </n-tooltip>

        <n-divider vertical style="margin: 0 4px" />

        <!-- History -->
        <n-button size="tiny" quaternary @click="emit('history')">
          <template #icon><n-icon :size="14"><HistoryIcon /></n-icon></template>
          History
        </n-button>
      </n-space>
    </div>

    <div class="toolbar-right">
      <n-space align="center" :size="6">
        <!-- 错误提示 -->
        <span v-if="props.error" class="error-badge" :title="props.error">
          ✕ Error
        </span>

        <n-divider v-if="connectionLabel" vertical style="margin: 0 2px" />

        <!-- 当前连接和数据库 -->
        <span v-if="connectionLabel" class="conn-info">
          <n-icon :size="12" style="margin-right:2px;vertical-align:middle"><ServerIcon /></n-icon>
          <span class="conn-label">{{ connectionLabel }}</span>
        </span>
        <span v-if="props.database" class="conn-info">
          <n-icon :size="12" style="margin-right:2px;vertical-align:middle"><DbIcon /></n-icon>
          <span class="db-label">{{ props.database }}</span>
        </span>

        <n-divider vertical style="margin: 0 2px" />

        <!-- Query / Explain -->
        <n-button size="tiny" quaternary @click="emit('queryBuilder')">
          Query
        </n-button>
        <n-button size="tiny" quaternary @click="emit('explain')">
          <template #icon><n-icon :size="12"><ExplainIcon /></n-icon></template>
          Explain
        </n-button>
      </n-space>
    </div>
  </div>
</template>

<style scoped>
.query-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 3px 8px;
  background: #f3f3f3;
  border-bottom: 1px solid #e0e0e0;
  flex-shrink: 0;
  gap: 8px;
}
.toolbar-left {
  display: flex;
  align-items: center;
}
.toolbar-right {
  display: flex;
  align-items: center;
}
.stats {
  font-size: 11px;
  color: #666;
}
.error-badge {
  font-size: 11px;
  color: #e03e3e;
  font-weight: 500;
  cursor: default;
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.conn-info {
  font-size: 11px;
  color: #555;
  display: inline-flex;
  align-items: center;
}
.conn-label {
  color: #d48806;
  font-weight: 500;
}
.db-label {
  color: #389e0d;
  font-weight: 500;
}
</style>

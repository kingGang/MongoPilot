<script setup lang="ts">
import { computed } from "vue";
import { NButton, NIcon, NSelect, NInputNumber, NTooltip } from "naive-ui";
import {
  Copy as CopyIcon,
  Download as ExportIcon,
  Search as FindIcon,
  Add as AddIcon,
  Trash as DeleteIcon,
  PlayBack as FirstIcon,
  ChevronBack as PrevIcon,
  ChevronForward as NextIcon,
  PlayForward as LastIcon,
  Refresh as RefreshIcon,
  Grid as TableIcon,
  List as TreeIcon,
  Code as JsonIcon,
  Time as TimeIcon,
  Documents as DocsIcon,
  Layers as CollIcon,
} from "@vicons/ionicons5";

const props = defineProps<{
  /** 匹配条件的真实总数（信息展示） */
  totalCount: number;
  /** 本次返回的文档数（分页计算） */
  returnedCount: number;
  pageSize: number;
  currentPage: number;
  viewMode: "tree" | "table" | "json";
  executionTimeMs: number;
  collection?: string;
  loading?: boolean;
}>();

const emit = defineEmits<{
  "update:pageSize": [size: number];
  "update:currentPage": [page: number];
  "update:viewMode": [mode: "tree" | "table" | "json"];
  refresh: [];
  insertDoc: [];
  deleteSelected: [];
  copyDocs: [];
  exportDocs: [];
  toggleSearch: [];
}>();

// 分页基于 totalCount（数据库真实总数）
const totalPages = computed(() => Math.max(1, Math.ceil(props.totalCount / props.pageSize)));
const rangeStart = computed(() => props.totalCount === 0 ? 0 : (props.currentPage - 1) * props.pageSize + 1);
const rangeEnd = computed(() => Math.min(props.currentPage * props.pageSize, props.totalCount));

const execTimeDisplay = computed(() => {
  const ms = props.executionTimeMs;
  return ms >= 1000 ? `${(ms / 1000).toFixed(3)} s` : `${ms} ms`;
});

const pageSizeOptions = [
  { label: "25", value: 25 },
  { label: "50", value: 50 },
  { label: "100", value: 100 },
  { label: "200", value: 200 },
  { label: "500", value: 500 },
];

function goFirst() { emit("update:currentPage", 1); }
function goPrev() { if (props.currentPage > 1) emit("update:currentPage", props.currentPage - 1); }
function goNext() { if (props.currentPage < totalPages.value) emit("update:currentPage", props.currentPage + 1); }
function goLast() { emit("update:currentPage", totalPages.value); }
</script>

<template>
  <div class="result-toolbar" @mousedown.prevent>
    <!-- 左侧: 集合名 + 执行时间 + 文档数 -->
    <div class="toolbar-left">
      <span v-if="collection" class="info-item coll-name">
        <n-icon :size="12" style="margin-right:3px"><CollIcon /></n-icon>
        {{ collection }}
      </span>
      <span class="info-item exec-time">
        <n-icon :size="12" style="margin-right:3px"><TimeIcon /></n-icon>
        {{ execTimeDisplay }}
      </span>
      <span class="info-item doc-count">
        <n-icon :size="12" style="margin-right:3px"><DocsIcon /></n-icon>
        {{ returnedCount }}<template v-if="totalCount > returnedCount"> / {{ totalCount }}</template> Docs
      </span>
    </div>

    <!-- 右侧: 操作 + 分页 + 视图 -->
    <div class="toolbar-right">
      <!-- 文档操作按钮 -->
      <n-tooltip trigger="hover" :delay="500">
        <template #trigger>
          <n-button size="tiny" quaternary @click="emit('insertDoc')">
            <template #icon><n-icon :size="14"><AddIcon /></n-icon></template>
          </n-button>
        </template>
        插入文档
      </n-tooltip>
      <n-tooltip trigger="hover" :delay="500">
        <template #trigger>
          <n-button size="tiny" quaternary @click="emit('copyDocs')">
            <template #icon><n-icon :size="14"><CopyIcon /></n-icon></template>
          </n-button>
        </template>
        复制
      </n-tooltip>
      <n-tooltip trigger="hover" :delay="500">
        <template #trigger>
          <n-button size="tiny" quaternary @click="emit('exportDocs')">
            <template #icon><n-icon :size="14"><ExportIcon /></n-icon></template>
          </n-button>
        </template>
        导出
      </n-tooltip>
      <n-tooltip trigger="hover" :delay="500">
        <template #trigger>
          <n-button size="tiny" quaternary @click="emit('toggleSearch')">
            <template #icon><n-icon :size="14"><FindIcon /></n-icon></template>
          </n-button>
        </template>
        搜索 (Ctrl+F)
      </n-tooltip>
      <n-tooltip trigger="hover" :delay="500">
        <template #trigger>
          <n-button size="tiny" quaternary @click="emit('deleteSelected')">
            <template #icon><n-icon :size="14"><DeleteIcon /></n-icon></template>
          </n-button>
        </template>
        删除
      </n-tooltip>

      <div class="toolbar-divider" />

      <!-- 每页条数 -->
      <n-select
        :value="pageSize"
        :options="pageSizeOptions"
        size="tiny"
        style="width: 64px"
        @update:value="emit('update:pageSize', $event)"
      />

      <div class="toolbar-divider" />

      <!-- 分页导航 -->
      <n-button size="tiny" quaternary :disabled="loading || currentPage <= 1" @click="goFirst">
        <template #icon><n-icon :size="12"><FirstIcon /></n-icon></template>
      </n-button>
      <n-button size="tiny" quaternary :disabled="loading || currentPage <= 1" @click="goPrev">
        <template #icon><n-icon :size="12"><PrevIcon /></n-icon></template>
      </n-button>
      <n-button size="tiny" quaternary :disabled="loading || currentPage >= totalPages" @click="goNext">
        <template #icon><n-icon :size="12"><NextIcon /></n-icon></template>
      </n-button>
      <n-button size="tiny" quaternary :disabled="loading || currentPage >= totalPages" @click="goLast">
        <template #icon><n-icon :size="12"><LastIcon /></n-icon></template>
      </n-button>

      <span class="page-info">p.</span>
      <n-input-number
        :value="currentPage"
        :min="1"
        :max="totalPages"
        size="tiny"
        style="width: 46px"
        :show-button="false"
        @update:value="(v: number | null) => v && emit('update:currentPage', v)"
      />
      <span class="range-info">{{ rangeStart }}-{{ rangeEnd }}</span>

      <div class="toolbar-divider" />

      <!-- 视图切换 -->
      <n-tooltip trigger="hover" :delay="500">
        <template #trigger>
          <n-button
            size="tiny"
            :type="viewMode === 'tree' ? 'primary' : 'default'"
            :quaternary="viewMode !== 'tree'"
            @click="emit('update:viewMode', 'tree')"
          >
            <template #icon><n-icon :size="14"><TreeIcon /></n-icon></template>
          </n-button>
        </template>
        Tree
      </n-tooltip>
      <n-tooltip trigger="hover" :delay="500">
        <template #trigger>
          <n-button
            size="tiny"
            :type="viewMode === 'table' ? 'primary' : 'default'"
            :quaternary="viewMode !== 'table'"
            @click="emit('update:viewMode', 'table')"
          >
            <template #icon><n-icon :size="14"><TableIcon /></n-icon></template>
          </n-button>
        </template>
        Table
      </n-tooltip>
      <n-tooltip trigger="hover" :delay="500">
        <template #trigger>
          <n-button
            size="tiny"
            :type="viewMode === 'json' ? 'primary' : 'default'"
            :quaternary="viewMode !== 'json'"
            @click="emit('update:viewMode', 'json')"
          >
            <template #icon><n-icon :size="14"><JsonIcon /></n-icon></template>
          </n-button>
        </template>
        JSON
      </n-tooltip>

      <!-- 刷新 -->
      <n-button size="tiny" quaternary :loading="loading" @click="emit('refresh')">
        <template #icon><n-icon :size="14"><RefreshIcon /></n-icon></template>
      </n-button>
    </div>
  </div>
</template>

<style scoped>
.result-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 2px 8px;
  border-bottom: 1px solid #e0e0e0;
  background: #f8f8f8;
  flex-shrink: 0;
  min-height: 30px;
}
.toolbar-left {
  display: flex;
  align-items: center;
  gap: 12px;
}
.toolbar-right {
  display: flex;
  align-items: center;
  gap: 2px;
}
.info-item {
  display: inline-flex;
  align-items: center;
  font-size: 12px;
  white-space: nowrap;
}
.coll-name {
  color: #333;
  font-weight: 500;
}
.exec-time {
  color: #666;
}
.doc-count {
  color: #666;
}
.toolbar-divider {
  width: 1px;
  height: 18px;
  background: #d9d9d9;
  margin: 0 4px;
}
.page-info {
  font-size: 11px;
  color: #666;
  margin: 0 2px;
}
.range-info {
  font-size: 11px;
  color: #666;
  margin-left: 4px;
  white-space: nowrap;
}
</style>

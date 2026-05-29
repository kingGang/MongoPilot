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
  /** 已勾选文档数 */
  selectedCount?: number;
  pageSize: number;
  currentPage: number;
  viewMode: "tree" | "table" | "json";
  executionTimeMs: number;
  collection?: string;
  loading?: boolean;
  /** 当前连接是否只读 —— true 时插入/删除按钮置灰并改 tooltip 文案 */
  readOnly?: boolean;
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

// totalCount === -1 表示后端异步计数还没回来
const isPendingCount = computed(() => props.totalCount < 0);

// 分页基于 totalCount（数据库真实总数）; pending 时用当前页 + 本次 returned 估算,
// 允许 Next/Last 按钮在满一页时继续翻, 不满一页时禁用.
const totalPages = computed(() => {
  if (isPendingCount.value) {
    const est = props.currentPage + (props.returnedCount >= props.pageSize ? 1 : 0);
    return Math.max(1, est);
  }
  return Math.max(1, Math.ceil(props.totalCount / props.pageSize));
});
const rangeStart = computed(() =>
  (isPendingCount.value ? props.returnedCount === 0 : props.totalCount === 0)
    ? 0
    : (props.currentPage - 1) * props.pageSize + 1,
);
const rangeEnd = computed(() => {
  if (isPendingCount.value) {
    return (props.currentPage - 1) * props.pageSize + props.returnedCount;
  }
  return Math.min(props.currentPage * props.pageSize, props.totalCount);
});

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
        {{ returnedCount }}
        <template v-if="isPendingCount">
          / <span class="pending-count" title="后端正在计数...">…</span>
        </template>
        <template v-else-if="totalCount > returnedCount">
          / {{ totalCount }}
        </template>
        Docs
      </span>
      <span v-if="(selectedCount ?? 0) > 0" class="info-item selected-count">
        已选 {{ selectedCount }}
      </span>
    </div>

    <!-- 右侧: 操作 + 分页 + 视图 -->
    <div class="toolbar-right">
      <!-- 文档操作按钮 -->
      <n-tooltip trigger="hover" :delay="500">
        <template #trigger>
          <n-button
            size="tiny"
            quaternary
            :disabled="readOnly"
            @click="emit('insertDoc')"
          >
            <template #icon><n-icon :size="14"><AddIcon /></n-icon></template>
          </n-button>
        </template>
        {{ readOnly ? "只读连接, 禁用插入" : "插入文档" }}
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
          <n-button
            size="tiny"
            quaternary
            :disabled="readOnly || (selectedCount ?? 0) === 0"
            @click="emit('deleteSelected')"
          >
            <template #icon><n-icon :size="14"><DeleteIcon /></n-icon></template>
          </n-button>
        </template>
        <template v-if="readOnly">只读连接, 禁用删除</template>
        <template v-else-if="(selectedCount ?? 0) === 0">先勾选要删除的文档</template>
        <template v-else>删除 {{ selectedCount }} 条</template>
      </n-tooltip>

      <div class="toolbar-divider" />

      <!-- 每页条数 -->
      <n-select
        :value="pageSize"
        :options="pageSizeOptions"
        size="tiny"
        style="width: 64px"
        :consistent-menu-width="false"
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
.pending-count {
  display: inline-block;
  color: #999;
  font-style: italic;
  animation: pending-pulse 1.2s ease-in-out infinite;
}
.selected-count {
  color: #3875d7;
  font-weight: 600;
  font-size: 11px;
}
@keyframes pending-pulse {
  0%, 100% { opacity: 0.5; }
  50% { opacity: 1; }
}
</style>

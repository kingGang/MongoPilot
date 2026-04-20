<script setup lang="ts">
import { ref, h } from "vue";
import { NIcon, NDropdown } from "naive-ui";
import {
  SearchOutline as FindIcon,
  BarChartOutline as ExplainIcon,
} from "@vicons/ionicons5";
import type { ResultTab } from "@/types/database";

defineProps<{
  resultTabs: ResultTab[];
  activeResultTabId: string | null;
}>();

const emit = defineEmits<{
  activate: [resultTabId: string];
  close: [resultTabId: string];
  closeOthers: [resultTabId: string];
  closeLeft: [resultTabId: string];
  closeRight: [resultTabId: string];
  closeAll: [];
}>();

// 右键菜单状态
const ctxMenuShow = ref(false);
const ctxMenuX = ref(0);
const ctxMenuY = ref(0);
const ctxTargetId = ref<string>("");

function onTabClick(id: string) {
  emit("activate", id);
}

function onTabClose(e: MouseEvent, id: string) {
  e.stopPropagation();
  emit("close", id);
}

function onCtxMenu(e: MouseEvent, id: string) {
  e.preventDefault();
  ctxTargetId.value = id;
  ctxMenuX.value = e.clientX;
  ctxMenuY.value = e.clientY;
  ctxMenuShow.value = true;
}

const ctxOptions = [
  { label: "关闭当前", key: "close" },
  { label: "关闭左侧", key: "closeLeft" },
  { label: "关闭右侧", key: "closeRight" },
  { type: "divider" as const, key: "d1" },
  { label: "关闭其他", key: "closeOthers" },
  { label: "全部关闭", key: "closeAll" },
];

function onCtxSelect(key: string) {
  ctxMenuShow.value = false;
  const id = ctxTargetId.value;
  switch (key) {
    case "close": emit("close", id); break;
    case "closeLeft": emit("closeLeft", id); break;
    case "closeRight": emit("closeRight", id); break;
    case "closeOthers": emit("closeOthers", id); break;
    case "closeAll": emit("closeAll"); break;
  }
}

function kindIcon(kind: "find" | "explain") {
  return kind === "find" ? FindIcon : ExplainIcon;
}

function kindColor(kind: "find" | "explain") {
  return kind === "find" ? "#18a058" : "#3875d7";
}

// 用于 h() 内部
function renderIcon(tab: ResultTab) {
  return h(NIcon, { size: 13, color: kindColor(tab.kind) }, {
    default: () => h(kindIcon(tab.kind)),
  });
}
</script>

<template>
  <div class="result-tabs-bar">
    <div
      v-for="tab in resultTabs"
      :key="tab.id"
      class="result-tab"
      :class="{ active: tab.id === activeResultTabId, loading: tab.loading, error: !!tab.error }"
      :title="tab.queryText"
      @click="onTabClick(tab.id)"
      @contextmenu="onCtxMenu($event, tab.id)"
    >
      <component :is="renderIcon(tab)" />
      <span class="tab-title">{{ tab.title }}</span>
      <span
        v-if="tab.loading"
        class="tab-loading-dot"
        title="查询中"
      />
      <span
        class="tab-close"
        title="关闭"
        @click="onTabClose($event, tab.id)"
      >×</span>
    </div>

    <n-dropdown
      trigger="manual"
      placement="bottom-start"
      :show="ctxMenuShow"
      :options="ctxOptions"
      :x="ctxMenuX"
      :y="ctxMenuY"
      @select="onCtxSelect"
      @clickoutside="ctxMenuShow = false"
    />
  </div>
</template>

<style scoped>
.result-tabs-bar {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 4px 4px 0;
  background: #f5f5f5;
  border-bottom: 1px solid #e0e0e0;
  overflow-x: auto;
  flex-shrink: 0;
  min-height: 32px;
}
.result-tabs-bar::-webkit-scrollbar { height: 4px; }
.result-tabs-bar::-webkit-scrollbar-thumb { background: #ccc; }

.result-tab {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 4px 8px 4px 10px;
  height: 26px;
  border: 1px solid transparent;
  border-bottom: none;
  border-radius: 3px 3px 0 0;
  background: #e9e9e9;
  cursor: pointer;
  font-size: 12px;
  color: #555;
  user-select: none;
  max-width: 160px;
  white-space: nowrap;
}
.result-tab:hover { background: #f0f0f0; }
.result-tab.active {
  background: #fff;
  border-color: #d0d0d0;
  color: #333;
  margin-bottom: -1px;
}
.result-tab.error .tab-title { color: #d03050; }

.tab-title {
  overflow: hidden;
  text-overflow: ellipsis;
}

.tab-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border-radius: 2px;
  font-size: 14px;
  line-height: 1;
  color: #999;
  margin-left: 2px;
}
.tab-close:hover { background: #d5d5d5; color: #333; }

.tab-loading-dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  border: 2px solid #d0d0d0;
  border-top-color: #3875d7;
  border-radius: 50%;
  animation: result-tab-spin 0.6s linear infinite;
}
@keyframes result-tab-spin {
  to { transform: rotate(360deg); }
}
</style>

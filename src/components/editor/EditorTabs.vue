<script setup lang="ts">
import { ref, computed, nextTick } from "vue";
import { NTabs, NTabPane, NDropdown, NButton, NIcon, NPopselect } from "naive-ui";
import {
  ChevronBackOutline as ChevronLeftIcon,
  ChevronForwardOutline as ChevronRightIcon,
  ListOutline as TabListIcon,
} from "@vicons/ionicons5";
import { useEditorStore } from "@/stores/editor";

const store = useEditorStore();

const rootRef = ref<HTMLElement | null>(null);

/** tab 栏溢出时 naive-ui 只支持滚轮横滚, 这里补可见的左右滚动按钮 */
function scrollTabs(dir: 1 | -1) {
  const el = rootRef.value?.querySelector<HTMLElement>(".v-x-scroll");
  el?.scrollBy({ left: dir * 240, behavior: "smooth" });
}

const tabListOptions = computed(() =>
  store.tabs.map((t) => ({ label: t.title, value: t.id })),
);

function onPickTab(id: string) {
  store.activeTabId = id;
  nextTick(() => {
    rootRef.value
      ?.querySelector(".n-tabs-tab--active")
      ?.scrollIntoView({ behavior: "smooth", inline: "nearest", block: "nearest" });
  });
}

const ctxMenuShow = ref(false);
const ctxMenuX = ref(0);
const ctxMenuY = ref(0);
const ctxTargetId = ref<string>("");

const ctxOptions = computed(() => {
  const idx = store.tabs.findIndex((t) => t.id === ctxTargetId.value);
  const total = store.tabs.length;
  const hasLeft = idx > 0;
  const hasRight = idx >= 0 && idx < total - 1;
  const hasOthers = total > 1;
  return [
    { label: "关闭当前", key: "close" },
    { label: "关闭左侧", key: "closeLeft", disabled: !hasLeft },
    { label: "关闭右侧", key: "closeRight", disabled: !hasRight },
    { type: "divider" as const, key: "d1" },
    { label: "关闭其他", key: "closeOthers", disabled: !hasOthers },
    { label: "全部关闭", key: "closeAll" },
  ];
});

function onCtxMenu(e: MouseEvent, id: string) {
  e.preventDefault();
  ctxTargetId.value = id;
  ctxMenuX.value = e.clientX;
  ctxMenuY.value = e.clientY;
  ctxMenuShow.value = true;
}

function onCtxSelect(key: string) {
  ctxMenuShow.value = false;
  const id = ctxTargetId.value;
  switch (key) {
    case "close":
      store.closeTab(id);
      break;
    case "closeLeft":
      store.closeLeftOfTab(id);
      break;
    case "closeRight":
      store.closeRightOfTab(id);
      break;
    case "closeOthers":
      store.closeOtherTabs(id);
      break;
    case "closeAll":
      store.closeAllTabs();
      break;
  }
}
</script>

<template>
  <div class="editor-tabs" v-if="store.tabs.length > 0" ref="rootRef">
    <n-tabs
      type="card"
      closable
      :value="store.activeTabId ?? undefined"
      @update:value="store.activeTabId = $event"
      @close="store.closeTab($event as string)"
    >
      <n-tab-pane v-for="tab in store.tabs" :key="tab.id" :name="tab.id">
        <template #tab>
          <span
            class="editor-tab-label"
            :title="tab.title"
            @contextmenu="onCtxMenu($event, tab.id)"
          >
            {{ tab.title }}
          </span>
        </template>
        <slot :tab="tab" />
      </n-tab-pane>
      <template #suffix>
        <div class="tab-nav-controls">
          <n-button quaternary size="tiny" title="向左滚动" @click="scrollTabs(-1)">
            <n-icon :size="14"><ChevronLeftIcon /></n-icon>
          </n-button>
          <n-button quaternary size="tiny" title="向右滚动" @click="scrollTabs(1)">
            <n-icon :size="14"><ChevronRightIcon /></n-icon>
          </n-button>
          <n-popselect
            :value="store.activeTabId"
            :options="tabListOptions"
            trigger="click"
            placement="bottom-end"
            scrollable
            @update:value="onPickTab"
          >
            <n-button quaternary size="tiny" title="全部标签页">
              <n-icon :size="14"><TabListIcon /></n-icon>
            </n-button>
          </n-popselect>
        </div>
      </template>
    </n-tabs>

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
  <div v-else class="empty-editor">
    <p>双击集合名称打开查询标签页</p>
  </div>
</template>

<style scoped>
.editor-tabs {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.editor-tabs :deep(.n-tabs) {
  height: 100%;
  display: flex;
  flex-direction: column;
}
.editor-tabs :deep(.n-tabs-nav) {
  flex-shrink: 0;
  background: #f3f3f3;
}
.editor-tabs :deep(.n-tabs-nav .n-tabs-tab) {
  background: #ececec;
  color: #666;
  border-color: #d9d9d9;
}
.editor-tabs :deep(.n-tabs-nav .n-tabs-tab--active) {
  background: #fff;
  color: #333;
}
.editor-tabs :deep(.n-tabs-pane-wrapper) {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}
.editor-tabs :deep(.n-tab-pane) {
  height: 100%;
  padding: 0;
  overflow: hidden;
}
.editor-tab-label {
  display: inline-block;
  user-select: none;
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  vertical-align: bottom;
}
.editor-tabs :deep(.n-tabs-nav__suffix) {
  padding: 0 4px;
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}
.empty-editor {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: #999;
  background: #fafafa;
}
</style>

<script setup lang="ts">
import { NTabs, NTabPane } from "naive-ui";
import { useEditorStore } from "@/stores/editor";

const store = useEditorStore();
</script>

<template>
  <div class="editor-tabs" v-if="store.tabs.length > 0">
    <n-tabs
      type="card"
      closable
      :value="store.activeTabId ?? undefined"
      @update:value="store.activeTabId = $event"
      @close="store.closeTab($event as string)"
    >
      <n-tab-pane v-for="tab in store.tabs" :key="tab.id" :name="tab.id" :tab="tab.title">
        <slot :tab="tab" />
      </n-tab-pane>
    </n-tabs>
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
.empty-editor {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: #999;
  background: #fafafa;
}
</style>

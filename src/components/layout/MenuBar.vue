<script setup lang="ts">
import { NMenu } from "naive-ui";
import type { MenuOption } from "naive-ui";

const emit = defineEmits<{
  action: [key: string];
}>();

const menuOptions: MenuOption[] = [
  {
    label: "File",
    key: "file",
    children: [
      { label: "New Connection", key: "file.new-connection" },
      { label: "Connection Manager...", key: "file.conn-manager" },
      { label: "New Query", key: "file.new-query" },
      { type: "divider", key: "d1" },
      { label: "Import Data...", key: "file.import" },
      { label: "Export Data...", key: "file.export" },
    ],
  },
  {
    label: "Edit",
    key: "edit",
    children: [
      { label: "Undo", key: "edit.undo" },
      { label: "Redo", key: "edit.redo" },
      { type: "divider", key: "d2" },
      { label: "Cut", key: "edit.cut" },
      { label: "Copy", key: "edit.copy" },
      { label: "Paste", key: "edit.paste" },
    ],
  },
  {
    label: "View",
    key: "view",
    children: [
      { label: "Toggle Sidebar", key: "view.sidebar" },
      { label: "Toggle AI Panel", key: "view.ai-panel" },
      { type: "divider", key: "d3" },
      { label: "Zoom In", key: "view.zoom-in" },
      { label: "Zoom Out", key: "view.zoom-out" },
    ],
  },
  {
    label: "Run",
    key: "run",
    children: [
      { label: "Execute Query (Ctrl+Enter)", key: "run.execute" },
      { label: "Explain Query", key: "run.explain" },
    ],
  },
  {
    label: "Tools",
    key: "tools",
    children: [
      { label: "Server Monitor", key: "tools.monitor" },
      { label: "User Management", key: "tools.users" },
      { label: "Profiler", key: "tools.profiler" },
      { type: "divider", key: "d4" },
      { label: "AI Assistant", key: "tools.ai" },
      { label: "AI Settings", key: "tools.ai-settings" },
    ],
  },
  {
    label: "Help",
    key: "help",
    children: [
      { label: "Documentation", key: "help.docs" },
      { label: "About MongoPilot", key: "help.about" },
    ],
  },
];

function handleSelect(key: string) {
  emit("action", key);
}
</script>

<template>
  <div class="menu-bar">
    <n-menu
      mode="horizontal"
      :options="menuOptions"
      @update:value="handleSelect"
      responsive
    />
  </div>
</template>

<style scoped>
.menu-bar {
  border-bottom: 1px solid var(--n-border-color);
  height: 32px;
  display: flex;
  align-items: center;
}
.menu-bar :deep(.n-menu) {
  font-size: 13px;
}
.menu-bar :deep(.n-menu-item-content) {
  padding: 0 12px !important;
}
</style>

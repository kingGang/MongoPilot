<script setup lang="ts">
import { onMounted, computed, ref, h } from "vue";
import { NTree, NButton, NIcon, NDropdown } from "naive-ui";
import {
  Add as AddIcon,
  CloudDone as ConnectedIcon,
  CloudOffline as DisconnectedIcon,
} from "@vicons/ionicons5";
import { useConnectionStore } from "@/stores/connection";
import type { ConnectionConfig } from "@/types/connection";
import type { TreeOption } from "naive-ui";

const emit = defineEmits<{
  create: [];
  edit: [config: ConnectionConfig];
  connect: [config: ConnectionConfig];
  disconnect: [id: string];
  delete: [id: string];
}>();

const store = useConnectionStore();

onMounted(() => {
  store.fetchConnections();
});

interface TreeNode extends TreeOption {
  key: string;
  label: string;
  isLeaf: boolean;
  children?: TreeNode[];
  raw?: ConnectionConfig;
}

const treeData = computed<TreeNode[]>(() => {
  const nodes: TreeNode[] = [];

  for (const [group, conns] of store.connectionTree) {
    if (group === "(未分组)") {
      for (const conn of conns) {
        nodes.push(connToNode(conn));
      }
    } else {
      nodes.push({
        key: `group-${group}`,
        label: group,
        isLeaf: false,
        children: conns.map(connToNode),
      });
    }
  }

  return nodes;
});

function connToNode(conn: ConnectionConfig): TreeNode {
  const active = store.isActive(conn.id);
  return {
    key: conn.id,
    label: active ? `${conn.name}` : conn.name,
    isLeaf: true,
    raw: conn,
    prefix: () =>
      h(NIcon, { size: 16, color: active ? "#18a058" : "#999" }, {
        default: () => h(active ? ConnectedIcon : DisconnectedIcon),
      }),
    suffix: active
      ? () => h("span", { style: "font-size:11px;color:#18a058;margin-left:4px" }, "已连接")
      : undefined,
  };
}

// 右键菜单
const showContextMenu = ref(false);
const contextMenuX = ref(0);
const contextMenuY = ref(0);
const contextNode = ref<TreeNode | null>(null);

const contextMenuOptions = computed(() => {
  if (!contextNode.value?.raw) return [];
  const active = store.isActive(contextNode.value.raw.id);
  return [
    ...(active
      ? [{ label: "断开连接", key: "disconnect" }]
      : [{ label: "连接", key: "connect" }]),
    { type: "divider" as const, key: "d1" },
    { label: "编辑", key: "edit" },
    { label: "删除", key: "delete" },
  ];
});

function handleContextMenu(e: MouseEvent, node: TreeNode) {
  if (!node.raw) return;
  e.preventDefault();
  contextNode.value = node;
  contextMenuX.value = e.clientX;
  contextMenuY.value = e.clientY;
  showContextMenu.value = true;
}

function handleContextSelect(key: string) {
  showContextMenu.value = false;
  if (!contextNode.value?.raw) return;
  const conn = contextNode.value.raw;
  switch (key) {
    case "connect":
      emit("connect", conn);
      break;
    case "disconnect":
      emit("disconnect", conn.id);
      break;
    case "edit":
      emit("edit", conn);
      break;
    case "delete":
      emit("delete", conn.id);
      break;
  }
}
</script>

<template>
  <div class="connection-tree">
    <div class="tree-toolbar">
      <n-button size="small" quaternary @click="$emit('create')">
        <template #icon>
          <n-icon><AddIcon /></n-icon>
        </template>
        New Connection
      </n-button>
    </div>
    <n-tree
      :data="treeData"
      block-line
      selectable
      default-expand-all
      :node-props="({ option }: any) => ({
        onContextmenu: (e: MouseEvent) => handleContextMenu(e, option as TreeNode),
        onDblclick: () => {
          const node = option as TreeNode;
          if (node.raw && !store.isActive(node.raw.id)) {
            emit('connect', node.raw);
          }
        },
      })"
    />
    <n-dropdown
      trigger="manual"
      placement="bottom-start"
      :show="showContextMenu"
      :options="contextMenuOptions"
      :x="contextMenuX"
      :y="contextMenuY"
      @select="handleContextSelect"
      @clickoutside="showContextMenu = false"
    />
  </div>
</template>

<style scoped>
.connection-tree {
  display: flex;
  flex-direction: column;
}
.tree-toolbar {
  padding: 4px 8px;
  border-bottom: 1px solid var(--n-border-color);
}
</style>

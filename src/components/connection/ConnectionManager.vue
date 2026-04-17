<script setup lang="ts">
import { ref, computed, h } from "vue";
import {
  NModal, NCard, NButton, NIcon, NDataTable, NSpace, useMessage, useDialog,
} from "naive-ui";
import type { DataTableColumns } from "naive-ui";
import {
  Add as AddIcon,
  Create as EditIcon,
  Trash as DeleteIcon,
  Copy as CloneIcon,
  Link as UriIcon,
  Flask as TestIcon,
  CloudDone as ConnectIcon,
} from "@vicons/ionicons5";
import { useConnectionStore } from "@/stores/connection";
import type { ConnectionConfig } from "@/types/connection";
import { exportUri } from "@/api/connection";

defineProps<{ show: boolean }>();

const emit = defineEmits<{
  "update:show": [val: boolean];
  connect: [config: ConnectionConfig];
  edit: [config: ConnectionConfig];
  create: [];
}>();

const connStore = useConnectionStore();
const message = useMessage();
const dialog = useDialog();

const selectedId = ref<string | null>(null);

const selectedConfig = computed(() =>
  connStore.connections.find((c) => c.id === selectedId.value) ?? null,
);

// ---- 表格列 ----
const columns: DataTableColumns<ConnectionConfig> = [
  {
    title: "Name",
    key: "name",
    width: 180,
    sorter: (a, b) => a.name.localeCompare(b.name),
    render(row) {
      const active = connStore.isActive(row.id);
      const color = row.color || (active ? "#18a058" : "#333");
      return h("span", { style: `color:${color};font-weight:${active ? 600 : 400}` }, row.name);
    },
  },
  {
    title: "Server",
    key: "host",
    width: 220,
    render(row) {
      const addr = row.srv ? row.host : `${row.host}:${row.port}`;
      return h("span", { style: "color:#666" }, addr);
    },
  },
  {
    title: "Security",
    key: "security",
    width: 200,
    render(row) {
      const parts: string[] = [];
      if (row.sshEnabled) parts.push("[SSH]");
      if (row.tls) parts.push("[TLS]");
      if (row.readOnly) parts.push("[只读]");
      const user = row.username || "undefined";
      const db = row.authDb || "";
      return h("span", { style: "color:#666" }, `${parts.join("")} ${user} @ ${db}`);
    },
  },
  {
    title: "Status",
    key: "status",
    width: 100,
    render(row) {
      const active = connStore.isActive(row.id);
      return active
        ? h("span", { style: "color:#18a058;font-weight:500" }, "已连接")
        : h("span", { style: "color:#999" }, "未连接");
    },
  },
];

const rowProps = (row: ConnectionConfig) => ({
  style: "cursor: pointer",
  onClick: () => { selectedId.value = row.id; },
  onDblclick: () => { handleConnect(); },
});

const rowKey = (row: ConnectionConfig) => row.id;

// ---- 操作 ----
function handleCreate() {
  emit("create");
  emit("update:show", false);
}

function handleEdit() {
  if (!selectedConfig.value) { message.warning("请先选择一个连接"); return; }
  emit("edit", selectedConfig.value);
  emit("update:show", false);
}

async function handleDelete() {
  if (!selectedConfig.value) { message.warning("请先选择一个连接"); return; }
  const name = selectedConfig.value.name;
  const id = selectedConfig.value.id;
  dialog.warning({
    title: "删除连接",
    content: `确定要删除连接 "${name}" 吗？`,
    positiveText: "删除",
    negativeText: "取消",
    onPositiveClick: async () => {
      await connStore.remove(id);
      selectedId.value = null;
      message.success(`已删除 ${name}`);
    },
  });
}

async function handleClone() {
  if (!selectedConfig.value) { message.warning("请先选择一个连接"); return; }
  const src = selectedConfig.value;
  const clone: ConnectionConfig = {
    ...JSON.parse(JSON.stringify(src)),
    id: crypto.randomUUID(),
    name: `${src.name} (copy)`,
  };
  await connStore.save(clone);
  message.success(`已克隆为 "${clone.name}"`);
}

async function handleToUri() {
  if (!selectedConfig.value) { message.warning("请先选择一个连接"); return; }
  try {
    const uri = await exportUri(selectedConfig.value);
    await navigator.clipboard.writeText(uri);
    message.success("URI 已复制到剪贴板");
  } catch (e) {
    message.error(`导出 URI 失败: ${e}`);
  }
}

async function handleTest() {
  if (!selectedConfig.value) { message.warning("请先选择一个连接"); return; }
  try {
    const info = await connStore.testConn(selectedConfig.value);
    message.success(`连接成功！版本: ${info.version}, 拓扑: ${info.topology}`);
  } catch (e) {
    message.error(`连接测试失败: ${e}`);
  }
}

function handleConnect() {
  if (!selectedConfig.value) { message.warning("请先选择一个连接"); return; }
  emit("connect", selectedConfig.value);
  emit("update:show", false);
}

function handleClose() {
  emit("update:show", false);
}
</script>

<template>
  <n-modal :show="show" :trap-focus="false" @update:show="emit('update:show', $event)">
    <n-card
      title="Connections"
      :bordered="false"
      closable
      role="dialog"
      style="width: 820px; height: 560px"
      @close="handleClose"
    >
      <!-- 工具栏 -->
      <div class="cm-toolbar">
        <n-button size="small" @click="handleCreate">
          <template #icon><n-icon :size="14"><AddIcon /></n-icon></template>
          New Connection
        </n-button>
        <n-button size="small" :disabled="!selectedId" @click="handleEdit">
          <template #icon><n-icon :size="14"><EditIcon /></n-icon></template>
          Edit
        </n-button>
        <n-button size="small" :disabled="!selectedId" @click="handleDelete">
          <template #icon><n-icon :size="14"><DeleteIcon /></n-icon></template>
          Delete
        </n-button>
        <n-button size="small" :disabled="!selectedId" @click="handleClone">
          <template #icon><n-icon :size="14"><CloneIcon /></n-icon></template>
          Clone
        </n-button>
        <n-button size="small" :disabled="!selectedId" @click="handleToUri">
          <template #icon><n-icon :size="14"><UriIcon /></n-icon></template>
          To URI
        </n-button>
        <n-button size="small" :disabled="!selectedId" @click="handleTest">
          <template #icon><n-icon :size="14"><TestIcon /></n-icon></template>
          Test
        </n-button>
      </div>

      <!-- 表格 -->
      <div class="cm-table">
        <n-data-table
          :columns="columns"
          :data="connStore.connections"
          :row-key="rowKey"
          :row-props="rowProps"
          :row-class-name="(row: ConnectionConfig) => row.id === selectedId ? 'row-selected' : ''"
          size="small"
          flex-height
          style="height: 100%"
        />
      </div>

      <template #action>
        <n-space justify="end">
          <n-button size="small" @click="handleClose">Cancel</n-button>
          <n-button type="primary" size="small" :disabled="!selectedId" @click="handleConnect">
            <template #icon><n-icon :size="14"><ConnectIcon /></n-icon></template>
            Connect
          </n-button>
        </n-space>
      </template>
    </n-card>
  </n-modal>
</template>

<style scoped>
.cm-toolbar {
  display: flex;
  gap: 6px;
  margin-bottom: 10px;
  flex-wrap: wrap;
}
.cm-table {
  flex: 1;
  height: 360px;
}
.cm-table :deep(.row-selected td) {
  background: #e8f4fd !important;
}
</style>

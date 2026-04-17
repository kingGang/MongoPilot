<script setup lang="ts">
import { ref, onMounted } from "vue";
import {
  NDataTable, NButton, NModal, NCard, NForm, NFormItem, NInput, NSpace,
  NSelect, useMessage, useDialog,
} from "naive-ui";
import type { DataTableColumns } from "naive-ui";
import * as serverApi from "@/api/server";
import type { UserInfo } from "@/types/server";

const props = defineProps<{ connectionId: string; database: string }>();

const message = useMessage();
const dialog = useDialog();
const users = ref<UserInfo[]>([]);
const showCreate = ref(false);
const newUser = ref({ username: "", password: "", role: "read", roleDb: "" });

onMounted(loadUsers);

async function loadUsers() {
  try {
    users.value = await serverApi.listUsers(props.connectionId, props.database);
  } catch (e) {
    message.error(`获取用户失败: ${e}`);
  }
}

const roleOptions = [
  { label: "read", value: "read" },
  { label: "readWrite", value: "readWrite" },
  { label: "dbAdmin", value: "dbAdmin" },
  { label: "dbOwner", value: "dbOwner" },
  { label: "userAdmin", value: "userAdmin" },
  { label: "root", value: "root" },
];

async function handleCreate() {
  if (!newUser.value.username || !newUser.value.password) return;
  try {
    await serverApi.createUser(props.connectionId, props.database, {
      username: newUser.value.username,
      password: newUser.value.password,
      roles: [{ role: newUser.value.role, db: newUser.value.roleDb || props.database }],
    });
    message.success("用户已创建");
    showCreate.value = false;
    newUser.value = { username: "", password: "", role: "read", roleDb: "" };
    await loadUsers();
  } catch (e) {
    message.error(`创建失败: ${e}`);
  }
}

async function handleDrop(username: string) {
  dialog.warning({
    title: "确认删除",
    content: `确定要删除用户 "${username}" 吗？`,
    positiveText: "删除",
    negativeText: "取消",
    onPositiveClick: async () => {
      try {
        await serverApi.dropUser(props.connectionId, props.database, username);
        message.success("用户已删除");
        await loadUsers();
      } catch (e) {
        message.error(`删除失败: ${e}`);
      }
    },
  });
}

const columns: DataTableColumns = [
  { title: "用户名", key: "user", width: 150 },
  { title: "数据库", key: "database", width: 120 },
  {
    title: "角色", key: "roles",
    render: (row: any) => row.roles.map((r: any) => `${r.role}@${r.db}`).join(", "),
  },
  {
    title: "操作", key: "actions", width: 80,
    render: (row: any) => `删除|${handleDrop.name}`.split("|")[0] + (row ? "" : ""),
  },
];
</script>

<template>
  <div class="user-panel">
    <n-space vertical>
      <n-button size="small" type="primary" @click="showCreate = true">新建用户</n-button>
      <n-data-table :columns="columns" :data="users" :row-key="(row: any) => row.user" size="small" />
    </n-space>

    <n-modal v-model:show="showCreate">
      <n-card style="width: 400px" title="新建用户" closable @close="showCreate = false">
        <n-form label-placement="left" label-width="80">
          <n-form-item label="用户名">
            <n-input v-model:value="newUser.username" />
          </n-form-item>
          <n-form-item label="密码">
            <n-input v-model:value="newUser.password" type="password" show-password-on="click" />
          </n-form-item>
          <n-form-item label="角色">
            <n-select v-model:value="newUser.role" :options="roleOptions" />
          </n-form-item>
          <n-form-item label="角色数据库">
            <n-input v-model:value="newUser.roleDb" :placeholder="props.database" />
          </n-form-item>
        </n-form>
        <template #footer>
          <n-space justify="end">
            <n-button @click="showCreate = false">取消</n-button>
            <n-button type="primary" @click="handleCreate">创建</n-button>
          </n-space>
        </template>
      </n-card>
    </n-modal>
  </div>
</template>

<style scoped>
.user-panel { padding: 8px; }
</style>

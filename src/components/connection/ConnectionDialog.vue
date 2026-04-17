<script setup lang="ts">
import { NModal, NCard, useMessage } from "naive-ui";
import ConnectionForm from "./ConnectionForm.vue";
import { useConnectionStore } from "@/stores/connection";
import type { ConnectionConfig, ServerInfo } from "@/types/connection";

const props = defineProps<{
  show: boolean;
  config: ConnectionConfig;
}>();

const emit = defineEmits<{
  "update:show": [value: boolean];
}>();

const store = useConnectionStore();
const message = useMessage();

async function handleSave(config: ConnectionConfig) {
  try {
    await store.save(config);
    message.success("连接已保存");
    emit("update:show", false);
  } catch (e) {
    message.error(`保存失败: ${e}`);
  }
}

async function handleTest(config: ConnectionConfig) {
  try {
    const info: ServerInfo = await store.testConn(config);
    message.success(
      `连接成功！MongoDB ${info.version} (${info.topology})${
        info.replicaSet ? ` — ${info.replicaSet}` : ""
      }`,
    );
  } catch (e) {
    message.error(`连接失败: ${e}`);
  }
}
</script>

<template>
  <n-modal :show="props.show" @update:show="emit('update:show', $event)">
    <n-card
      style="width: 640px"
      :title="config.id ? '编辑连接' : '新建连接'"
      :bordered="false"
      closable
      @close="emit('update:show', false)"
    >
      <ConnectionForm
        :config="config"
        @save="handleSave"
        @test="handleTest"
        @cancel="emit('update:show', false)"
      />
    </n-card>
  </n-modal>
</template>

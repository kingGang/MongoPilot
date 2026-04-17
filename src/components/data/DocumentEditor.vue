<script setup lang="ts">
import { ref, watch } from "vue";
import { NModal, NCard, NButton, NSpace, useMessage } from "naive-ui";
import * as docApi from "@/api/document";

const props = defineProps<{
  show: boolean;
  mode: "insert" | "edit";
  connectionId: string;
  database: string;
  collection: string;
  document?: Record<string, unknown>;
}>();

const emit = defineEmits<{
  "update:show": [value: boolean];
  saved: [];
}>();

const message = useMessage();
const jsonText = ref("");
const saving = ref(false);

watch(() => props.show, (show) => {
  if (show) {
    if (props.mode === "edit" && props.document) {
      jsonText.value = JSON.stringify(props.document, null, 2);
    } else {
      jsonText.value = "{\n  \n}";
    }
  }
});

async function handleSave() {
  saving.value = true;
  try {
    const doc = JSON.parse(jsonText.value);
    if (props.mode === "insert") {
      await docApi.insertDocument(props.connectionId, props.database, props.collection, doc);
      message.success("文档已插入");
    } else {
      const id = String(props.document?._id ?? "");
      await docApi.updateDocument(props.connectionId, props.database, props.collection, id, doc);
      message.success("文档已更新");
    }
    emit("saved");
    emit("update:show", false);
  } catch (e) {
    message.error(`保存失败: ${e}`);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <n-modal :show="props.show" @update:show="emit('update:show', $event)">
    <n-card
      style="width: 640px; height: 500px"
      :title="mode === 'insert' ? '插入文档' : '编辑文档'"
      :bordered="false" closable
      @close="emit('update:show', false)"
    >
      <textarea
        v-model="jsonText"
        class="json-textarea"
        spellcheck="false"
      />
      <template #footer>
        <n-space justify="end">
          <n-button @click="emit('update:show', false)">取消</n-button>
          <n-button type="primary" :loading="saving" @click="handleSave">
            {{ mode === "insert" ? "插入" : "保存" }}
          </n-button>
        </n-space>
      </template>
    </n-card>
  </n-modal>
</template>

<style scoped>
.json-textarea {
  width: 100%;
  height: 320px;
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 13px;
  padding: 8px;
  border: 1px solid var(--n-border-color);
  border-radius: 4px;
  background: #1e1e1e;
  color: #d4d4d4;
  resize: none;
  outline: none;
}
</style>

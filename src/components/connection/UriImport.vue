<script setup lang="ts">
import { ref } from "vue";
import { NInput, NButton, NSpace, NAlert } from "naive-ui";
import { parseUri } from "@/api/connection";
import type { ConnectionConfig } from "@/types/connection";

const emit = defineEmits<{
  import: [config: ConnectionConfig];
}>();

const uri = ref("");
const error = ref<string | null>(null);
const loading = ref(false);

async function handleImport() {
  if (!uri.value.trim()) return;
  loading.value = true;
  error.value = null;
  try {
    const config = await parseUri(uri.value.trim());
    emit("import", config);
    uri.value = "";
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <div class="uri-import">
    <n-space vertical>
      <n-input
        v-model:value="uri"
        type="textarea"
        placeholder="粘贴 MongoDB URI，例如 mongodb://user:pass@host:27017/db"
        :rows="3"
      />
      <n-alert v-if="error" type="error">{{ error }}</n-alert>
      <n-button
        type="primary"
        size="small"
        :loading="loading"
        :disabled="!uri.trim()"
        @click="handleImport"
      >
        导入 URI
      </n-button>
    </n-space>
  </div>
</template>

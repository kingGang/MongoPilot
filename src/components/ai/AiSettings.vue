<script setup lang="ts">
import { ref, onMounted } from "vue";
import { NModal, NCard, NForm, NFormItem, NInput, NSelect, NInputNumber, NButton, NSpace, useMessage } from "naive-ui";
import * as aiApi from "@/api/ai";
import type { AiSettings } from "@/types/ai";

const props = defineProps<{ show: boolean }>();
const emit = defineEmits<{ "update:show": [value: boolean] }>();

const message = useMessage();
const settings = ref<AiSettings>({
  provider: "claude",
  apiKey: "",
  model: "claude-sonnet-4-20250514",
  baseUrl: undefined,
  temperature: 0.3,
});

const providerOptions = [
  { label: "Claude (Anthropic)", value: "claude" },
  { label: "OpenAI", value: "openai" },
  { label: "自定义 (OpenAI 兼容)", value: "custom" },
];

const modelOptions: Record<string, { label: string; value: string }[]> = {
  claude: [
    { label: "Claude Sonnet 4", value: "claude-sonnet-4-20250514" },
    { label: "Claude Opus 4", value: "claude-opus-4-20250514" },
    { label: "Claude Haiku 3.5", value: "claude-3-5-haiku-20241022" },
  ],
  openai: [
    { label: "GPT-4o", value: "gpt-4o" },
    { label: "GPT-4o mini", value: "gpt-4o-mini" },
    { label: "GPT-4 Turbo", value: "gpt-4-turbo" },
  ],
};

onMounted(async () => {
  const saved = await aiApi.getAiSettings();
  if (saved) Object.assign(settings.value, saved);
});

async function handleSave() {
  if (!settings.value.apiKey) {
    message.warning("请输入 API Key");
    return;
  }
  try {
    await aiApi.saveAiSettings(settings.value);
    message.success("AI 设置已保存");
    emit("update:show", false);
  } catch (e) {
    message.error(`保存失败: ${e}`);
  }
}
</script>

<template>
  <n-modal :show="props.show" @update:show="emit('update:show', $event)">
    <n-card style="width: 500px" title="AI 设置" closable @close="emit('update:show', false)">
      <n-form label-placement="left" label-width="100">
        <n-form-item label="提供商">
          <n-select v-model:value="settings.provider" :options="providerOptions" />
        </n-form-item>
        <n-form-item label="API Key">
          <n-input v-model:value="settings.apiKey" type="password" show-password-on="click" placeholder="sk-..." />
        </n-form-item>
        <n-form-item label="模型">
          <n-select
            v-if="settings.provider !== 'custom'"
            v-model:value="settings.model"
            :options="modelOptions[settings.provider] || []"
          />
          <n-input v-else v-model:value="settings.model" placeholder="模型名称" />
        </n-form-item>
        <n-form-item v-if="settings.provider === 'custom'" label="Base URL">
          <n-input v-model:value="settings.baseUrl" placeholder="https://your-api.com/v1" />
        </n-form-item>
        <n-form-item label="Temperature">
          <n-input-number v-model:value="settings.temperature" :min="0" :max="2" :step="0.1" style="width: 120px" />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="emit('update:show', false)">取消</n-button>
          <n-button type="primary" @click="handleSave">保存</n-button>
        </n-space>
      </template>
    </n-card>
  </n-modal>
</template>

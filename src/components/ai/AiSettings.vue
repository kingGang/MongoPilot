<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import {
  NModal,
  NCard,
  NForm,
  NFormItem,
  NInput,
  NSelect,
  NInputNumber,
  NButton,
  NSpace,
  NTabs,
  NTabPane,
  NAlert,
  useMessage,
} from "naive-ui";
import { useAiStore } from "@/stores/ai";
import { useConnectionStore } from "@/stores/connection";
import { useEditorStore } from "@/stores/editor";
import type { AiSettings } from "@/types/ai";

const props = defineProps<{ show: boolean }>();
const emit = defineEmits<{ "update:show": [value: boolean] }>();

const message = useMessage();
const aiStore = useAiStore();
const connStore = useConnectionStore();
const editorStore = useEditorStore();
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

// ---- AI Rules 编辑状态 ----
const activeSection = ref<"connection" | "rules">("connection");
const rulesTab = ref<"global" | "conn">("global");
const globalRulesDraft = ref("");
const connRulesDraft = ref("");
const savingRules = ref(false);

/** 当前 tab 绑定的连接 (用于连接级 rules) */
const currentConnId = computed(() => editorStore.activeTab?.connectionId || "");
const currentConnName = computed(() => {
  const id = currentConnId.value;
  if (!id) return "";
  return connStore.connections.find((c) => c.id === id)?.name || id;
});

onMounted(async () => {
  await aiStore.loadSettings();
  if (aiStore.settings) Object.assign(settings.value, aiStore.settings);
  await aiStore.loadGlobalRules();
  globalRulesDraft.value = aiStore.globalRules;
  if (currentConnId.value) {
    await aiStore.loadConnRules(currentConnId.value);
    connRulesDraft.value = aiStore.connRulesCache[currentConnId.value] || "";
  }
});

// 弹窗每次打开都重刷一次 draft, 避免上次没保存的修改污染
watch(
  () => props.show,
  async (v) => {
    if (!v) return;
    await aiStore.loadGlobalRules();
    globalRulesDraft.value = aiStore.globalRules;
    if (currentConnId.value) {
      await aiStore.loadConnRules(currentConnId.value);
      connRulesDraft.value = aiStore.connRulesCache[currentConnId.value] || "";
    } else {
      connRulesDraft.value = "";
    }
  },
);

async function handleSave() {
  if (!settings.value.apiKey) {
    message.warning("请输入 API Key");
    return;
  }
  if (settings.value.provider === "custom" && !settings.value.baseUrl?.trim()) {
    message.warning("自定义提供商需要填写 Base URL");
    return;
  }
  try {
    await aiStore.updateSettings({ ...settings.value });
    message.success("AI 设置已保存");
    emit("update:show", false);
  } catch (e) {
    message.error(`保存失败: ${e}`);
  }
}

async function handleSaveRules() {
  savingRules.value = true;
  try {
    if (rulesTab.value === "global") {
      await aiStore.saveGlobalRules(globalRulesDraft.value);
    } else {
      if (!currentConnId.value) {
        message.warning("当前没有绑定连接, 无法保存连接级规范");
        return;
      }
      await aiStore.saveConnRules(currentConnId.value, connRulesDraft.value);
    }
    message.success("规范已保存, 下一轮对话生效");
  } catch (e) {
    message.error(`保存失败: ${e}`);
  } finally {
    savingRules.value = false;
  }
}

const globalRulesPlaceholder = [
  "用来告诉 AI 你的偏好和规范, 每轮对话都会自动带上。例如:",
  "",
  "- 字段命名一律 lowerCamelCase",
  "- 日期字段用 ISODate() 生成, 不要用 new Date()",
  "- 生成 find 查询默认加 .sort({_id: -1}).limit(50)",
  "- 用户 (users) 集合里 phone 字段是加密的, 查询用 hashed 字段",
].join("\n");

const connRulesPlaceholder = [
  "只作用于当前连接的规范, 例如:",
  "",
  "- 这是生产环境, 任何写操作都必须 ask_user 确认",
  "- 库 app_server 里 _id 是 String 类型, 不要用 ObjectId 包裹",
].join("\n");
</script>

<template>
  <n-modal :show="props.show" @update:show="emit('update:show', $event)">
    <n-card style="width: 640px" title="AI 设置" closable @close="emit('update:show', false)">
      <n-tabs v-model:value="activeSection" type="line" animated>
        <n-tab-pane name="connection" tab="模型 / API">
          <n-form label-placement="left" label-width="100" style="margin-top: 8px">
            <n-form-item label="提供商">
              <n-select v-model:value="settings.provider" :options="providerOptions" />
            </n-form-item>
            <n-form-item label="API Key">
              <n-input
                v-model:value="settings.apiKey"
                type="password"
                show-password-on="click"
                placeholder="sk-..."
              />
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
              <n-input-number
                v-model:value="settings.temperature"
                :min="0"
                :max="2"
                :step="0.1"
                style="width: 120px"
              />
            </n-form-item>
          </n-form>
        </n-tab-pane>

        <n-tab-pane name="rules" tab="规范 (Rules)">
          <n-alert type="info" :show-icon="false" style="margin: 4px 0 12px">
            这段文本会拼进每轮 AI 对话的 system prompt。写你的偏好、命名约定、集合特殊字段等 ——
            AI 每次任务都会遵守, 免得每次都重新说一遍。
          </n-alert>
          <n-tabs v-model:value="rulesTab" type="segment" style="margin-bottom: 10px">
            <n-tab-pane name="global" tab="全局" />
            <n-tab-pane
              name="conn"
              :tab="currentConnName ? `当前连接 (${currentConnName})` : '当前连接 (未绑定)'"
              :disabled="!currentConnId"
            />
          </n-tabs>
          <n-input
            v-if="rulesTab === 'global'"
            v-model:value="globalRulesDraft"
            type="textarea"
            :rows="12"
            :placeholder="globalRulesPlaceholder"
          />
          <n-input
            v-else
            v-model:value="connRulesDraft"
            type="textarea"
            :rows="12"
            :placeholder="connRulesPlaceholder"
          />
          <n-space justify="end" style="margin-top: 12px">
            <n-button :loading="savingRules" type="primary" @click="handleSaveRules">
              保存规范
            </n-button>
          </n-space>
        </n-tab-pane>
      </n-tabs>

      <template #footer>
        <n-space justify="end">
          <n-button @click="emit('update:show', false)">关闭</n-button>
          <n-button v-if="activeSection === 'connection'" type="primary" @click="handleSave">
            保存模型设置
          </n-button>
        </n-space>
      </template>
    </n-card>
  </n-modal>
</template>

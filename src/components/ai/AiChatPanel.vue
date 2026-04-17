<script setup lang="ts">
import { ref, computed, nextTick, watch, onMounted } from "vue";
import { NInput, NButton, NIcon, NSpin } from "naive-ui";
import {
  Send as SendIcon,
  Settings as SettingsIcon,
  Trash as ClearIcon,
} from "@vicons/ionicons5";
import { useAiStore } from "@/stores/ai";
import AiSettings from "./AiSettings.vue";

const props = defineProps<{
  connectionId?: string;
  database?: string;
  collection?: string;
}>();

const emit = defineEmits<{
  executeQuery: [query: string];
}>();

const aiStore = useAiStore();
const inputText = ref("");
const showSettings = ref(false);
const scrollRef = ref<HTMLDivElement>();

// 加载设置
onMounted(() => aiStore.loadSettings());

// 当上下文变化时，切换到对应会话
watch(
  () => [props.connectionId, props.database, props.collection],
  () => {
    if (props.connectionId && props.database) {
      aiStore.ensureConversation(props.connectionId, props.database, props.collection);
    }
  },
  { immediate: true },
);

const messages = computed(() => aiStore.activeConversation?.messages ?? []);

async function sendMessage() {
  const text = inputText.value.trim();
  if (!text || aiStore.loading) return;
  inputText.value = "";

  try {
    await aiStore.sendMessage(text, props.connectionId, props.database, props.collection);
  } catch {
    // error already added to messages by store
  }
  await nextTick();
  scrollToBottom();
}

function handleUseQuery(content: string) {
  // 匹配完整的 MongoDB 链式调用
  const match = content.match(/db\.\w+\.\w+\([^]*?\)(?:\s*\.\w+\([^]*?\))*/);
  if (match) {
    emit("executeQuery", match[0]);
  }
}

function scrollToBottom() {
  if (scrollRef.value) {
    scrollRef.value.scrollTop = scrollRef.value.scrollHeight;
  }
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendMessage();
  }
}

function clearChat() {
  aiStore.clearConversation();
}
</script>

<template>
  <div class="ai-chat-panel">
    <div class="chat-header">
      <span class="chat-title">AI 助手</span>
      <div class="chat-header-actions">
        <n-button size="tiny" quaternary title="清空对话" @click="clearChat">
          <template #icon><n-icon :size="14"><ClearIcon /></n-icon></template>
        </n-button>
        <n-button size="tiny" quaternary title="设置" @click="showSettings = true">
          <template #icon><n-icon :size="14"><SettingsIcon /></n-icon></template>
        </n-button>
      </div>
    </div>

    <div v-if="!aiStore.isConfigured && aiStore.settingsLoaded" class="chat-unconfigured">
      <p>请先配置 AI API Key</p>
      <n-button size="small" type="primary" @click="showSettings = true">打开设置</n-button>
    </div>

    <div ref="scrollRef" class="chat-messages">
      <div v-if="messages.length === 0" class="chat-empty">
        输入问题开始对话，支持自然语言生成 MongoDB 查询
      </div>
      <div v-for="(msg, i) in messages" :key="i" :class="['message', msg.role]">
        <div class="message-content">
          {{ msg.content }}
          <n-button
            v-if="msg.role === 'assistant' && msg.content.includes('db.')"
            size="tiny" type="primary" quaternary style="margin-top: 4px"
            @click="handleUseQuery(msg.content)"
          >
            使用此查询
          </n-button>
        </div>
      </div>
      <div v-if="aiStore.loading" class="message assistant">
        <n-spin size="small" />
      </div>
    </div>

    <div class="chat-input">
      <n-input
        v-model:value="inputText"
        type="textarea"
        :rows="2"
        placeholder="输入问题或描述你想查询的数据..."
        :disabled="aiStore.loading"
        @keydown="handleKeydown"
      />
      <n-button
        type="primary"
        :loading="aiStore.loading"
        :disabled="!inputText.trim()"
        @click="sendMessage"
      >
        <template #icon><n-icon><SendIcon /></n-icon></template>
      </n-button>
    </div>

    <AiSettings v-model:show="showSettings" />
  </div>
</template>

<style scoped>
.ai-chat-panel { height: 100%; display: flex; flex-direction: column; }
.chat-header { padding: 8px 12px; border-bottom: 1px solid var(--n-border-color); display: flex; justify-content: space-between; align-items: center; }
.chat-header-actions { display: flex; gap: 2px; }
.chat-title { font-weight: 600; font-size: 14px; }
.chat-unconfigured { padding: 24px; text-align: center; color: #999; }
.chat-messages { flex: 1; overflow-y: auto; padding: 12px; }
.chat-empty { color: #bbb; text-align: center; padding: 24px; font-size: 13px; }
.message { margin-bottom: 12px; }
.message.user .message-content { background: #e3f2fd; color: #1a1a1a; padding: 8px 12px; border-radius: 12px 12px 4px 12px; max-width: 85%; margin-left: auto; }
.message.assistant .message-content { background: #f5f5f5; color: #1a1a1a; padding: 8px 12px; border-radius: 12px 12px 12px 4px; max-width: 85%; white-space: pre-wrap; font-family: "Fira Code", monospace; font-size: 13px; }
.chat-input { padding: 8px 12px; border-top: 1px solid var(--n-border-color); display: flex; gap: 8px; align-items: flex-end; }
</style>

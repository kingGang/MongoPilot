<script setup lang="ts">
import { ref, computed, nextTick, watch, onMounted } from "vue";
import { NInput, NButton, NIcon, NSpin } from "naive-ui";
import {
  Send as SendIcon,
  Settings as SettingsIcon,
  Trash as ClearIcon,
  Stop as StopIcon,
} from "@vicons/ionicons5";
import { useAiStore } from "@/stores/ai";
import type { AgentToolCall } from "@/types/ai";
import AiSettings from "./AiSettings.vue";

const aiStore = useAiStore();
const inputText = ref("");
const showSettings = ref(false);
const scrollRef = ref<HTMLDivElement>();

onMounted(() => aiStore.loadSettings());

const messages = computed(() => aiStore.activeConversation?.messages ?? []);

// 消息增长时滚到底
watch(
  () => messages.value.length,
  async () => {
    await nextTick();
    scrollToBottom();
  },
);

const TOOL_LABELS: Record<string, string> = {
  ask_user: "向你提问",
  list_connections: "列出连接",
  open_connection: "打开连接",
  list_databases: "列出数据库",
  list_collections: "列出集合",
  get_schema: "分析集合结构",
  get_editor_content: "读取编辑器",
  get_editor_selection: "读取选中代码",
  propose_editor_edit: "提议修改编辑器",
  list_query_tabs: "列出查询标签页",
  open_query_tab: "打开新查询标签页",
  switch_query_tab: "切换查询标签页",
  set_active_context: "设置连接/库/集合",
  write_query: "写入查询到编辑器",
  list_scripts: "扫描脚本库",
  get_script: "读取脚本",
};
function toolLabel(name: string): string {
  return TOOL_LABELS[name] || name;
}
/** 毫秒 → 友好显示: >=1s 用 X.Xs, 否则 Xms */
function fmtDuration(ms?: number): string {
  if (ms === undefined) return "";
  return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;
}
function toolSummary(tc: AgentToolCall): string {
  const i = tc.input;
  if (tc.name === "write_query") return String(i.query ?? "");
  if (tc.name === "propose_editor_edit") return String(i.explanation ?? "提议新内容");
  if (tc.name === "get_schema") return String(i.collection ?? "(当前集合)");
  if (tc.name === "open_connection" || tc.name === "list_databases") {
    return String(i.connection ?? "");
  }
  if (tc.name === "list_collections" || tc.name === "set_active_context") {
    return [i.connection, i.database, i.collection].filter(Boolean).join(" / ");
  }
  if (tc.name === "open_query_tab") {
    return [i.database, i.collection].filter(Boolean).join(" / ") || "新标签页";
  }
  if (tc.name === "switch_query_tab") return String(i.tabId ?? "");
  if (tc.name === "get_script") return String(i.ref ?? "");
  if (tc.name === "ask_user") return String(i.question ?? "");
  return "";
}

/** 用户点了 ask_user 的某个选项 */
function pickOption(option: string) {
  aiStore.answerQuestion(option);
}

async function send() {
  const text = inputText.value.trim();
  if (!text) return;
  // 有挂起的提问 → 这次输入当作对该问题的自定义回答, 不是新一轮对话
  if (aiStore.pendingQuestion) {
    inputText.value = "";
    aiStore.answerQuestion(text);
    return;
  }
  if (aiStore.loading) return;
  inputText.value = "";
  try {
    await aiStore.runAgent(text);
  } catch {
    // 错误已由 store 追加到消息里
  }
  await nextTick();
  scrollToBottom();
}

function scrollToBottom() {
  if (scrollRef.value) scrollRef.value.scrollTop = scrollRef.value.scrollHeight;
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    send();
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
        我能读/改编辑器、执行查询、分析集合结构。<br />
        试试: "帮我写一个查询 users 集合里最近注册的 10 个用户"
      </div>

      <template v-for="(msg, i) in messages" :key="i">
        <!-- 用户消息 -->
        <div v-if="msg.role === 'user'" class="message user">
          <div class="bubble user-bubble">{{ msg.content }}</div>
        </div>

        <!-- 助手消息: 文本 + 工具调用 -->
        <div v-else-if="msg.role === 'assistant'" class="message assistant">
          <div v-if="msg.content" class="bubble assistant-bubble">{{ msg.content }}</div>
          <div v-if="msg.toolCalls && msg.toolCalls.length" class="tool-calls">
            <div v-for="tc in msg.toolCalls" :key="tc.id" class="tool-chip">
              <span class="tool-name">🔧 {{ toolLabel(tc.name) }}</span>
              <span v-if="toolSummary(tc)" class="tool-summary">{{ toolSummary(tc) }}</span>
            </div>
          </div>
          <div v-if="msg.durationMs !== undefined" class="step-time">
            ⏱ 模型耗时 {{ fmtDuration(msg.durationMs) }}
          </div>
        </div>

        <!-- 工具结果 (可折叠) -->
        <details v-else-if="msg.role === 'tool'" class="message tool-result">
          <summary>
            ✓ 工具结果
            <span v-if="msg.durationMs !== undefined" class="step-time-inline">
              · {{ fmtDuration(msg.durationMs) }}
            </span>
          </summary>
          <pre>{{ msg.content }}</pre>
        </details>
      </template>

      <!-- ask_user: agent 反问, 等用户选择 -->
      <div v-if="aiStore.pendingQuestion" class="ask-card">
        <div class="ask-question">{{ aiStore.pendingQuestion.question }}</div>
        <div class="ask-options">
          <button
            v-for="(opt, oi) in aiStore.pendingQuestion.options"
            :key="oi"
            class="ask-option"
            @click="pickOption(opt)"
          >
            {{ opt }}
          </button>
        </div>
        <div class="ask-hint">选一个，或在下方直接输入你的答案</div>
      </div>

      <div v-if="aiStore.loading && !aiStore.pendingQuestion" class="message assistant">
        <n-spin size="small" />
      </div>
    </div>

    <div class="chat-input">
      <n-input
        v-model:value="inputText"
        type="textarea"
        :rows="2"
        :placeholder="
          aiStore.pendingQuestion
            ? '选上面的选项，或在这里输入你的答案...'
            : '让 AI 帮你写查询、改脚本、查数据...'
        "
        :disabled="aiStore.loading && !aiStore.pendingQuestion"
        @keydown="handleKeydown"
      />
      <n-button
        v-if="aiStore.loading && !aiStore.pendingQuestion"
        type="error"
        title="停止 AI"
        @click="aiStore.stopAgent()"
      >
        <template #icon><n-icon><StopIcon /></n-icon></template>
        停止
      </n-button>
      <n-button
        v-else
        type="primary"
        :disabled="!inputText.trim()"
        @click="send"
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
.chat-empty { color: #bbb; text-align: center; padding: 24px; font-size: 13px; line-height: 1.7; }
.message { margin-bottom: 12px; }
.bubble { padding: 8px 12px; max-width: 90%; font-size: 13px; }
.message.user { display: flex; }
.user-bubble { background: #e3f2fd; color: #1a1a1a; border-radius: 12px 12px 4px 12px; margin-left: auto; white-space: pre-wrap; }
.assistant-bubble { background: #f5f5f5; color: #1a1a1a; border-radius: 12px 12px 12px 4px; white-space: pre-wrap; }
/* 工具调用 chip */
.tool-calls { display: flex; flex-direction: column; gap: 4px; margin-top: 6px; }
.tool-chip {
  display: flex;
  flex-direction: column;
  gap: 2px;
  background: #f0f7ff;
  border: 1px solid #cfe3fb;
  border-radius: 6px;
  padding: 5px 8px;
  font-size: 12px;
}
.tool-name { color: #2b6cb0; font-weight: 500; }
.step-time {
  margin-top: 4px;
  font-size: 11px;
  color: #aaa;
}
.step-time-inline {
  font-size: 11px;
  color: #bbb;
}
.tool-summary {
  color: #666;
  font-family: "Fira Code", monospace;
  font-size: 11px;
  word-break: break-all;
  white-space: pre-wrap;
}
/* ask_user 提问卡片 */
.ask-card {
  background: #fff7ed;
  border: 1px solid #fed7aa;
  border-radius: 8px;
  padding: 10px 12px;
  margin-bottom: 12px;
}
.ask-question {
  font-size: 13px;
  font-weight: 500;
  color: #9a3412;
  margin-bottom: 8px;
  white-space: pre-wrap;
}
.ask-options {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.ask-option {
  text-align: left;
  font: inherit;
  font-size: 13px;
  padding: 7px 10px;
  border: 1px solid #fdba74;
  border-radius: 6px;
  background: #fff;
  color: #1a1a1a;
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s;
}
.ask-option:hover {
  background: #fff7ed;
  border-color: #f97316;
}
.ask-hint {
  margin-top: 8px;
  font-size: 11px;
  color: #c2680f;
}
/* 工具结果折叠块 */
.tool-result {
  background: #fafafa;
  border: 1px solid #eee;
  border-radius: 6px;
  padding: 4px 8px;
  font-size: 12px;
}
.tool-result summary { cursor: pointer; color: #888; user-select: none; }
.tool-result pre {
  margin: 6px 0 2px;
  white-space: pre-wrap;
  word-break: break-all;
  font-size: 11px;
  color: #555;
  max-height: 200px;
  overflow: auto;
}
.chat-input { padding: 8px 12px; border-top: 1px solid var(--n-border-color); display: flex; gap: 8px; align-items: flex-end; }
</style>

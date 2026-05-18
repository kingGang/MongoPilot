<script setup lang="ts">
import { computed, watch } from "vue";
import { NButton, NIcon, NTooltip, NDropdown, NPopover, NInputNumber, NSwitch } from "naive-ui";
import {
  Play as PlayIcon,
  PlayForward as RunAllIcon,
  Stop as StopIcon,
  Time as HistoryIcon,
  CloudDownload as ImportIcon,
  CloudUpload as ExportIcon,
  Search as ExplainIcon,
  Server as ServerIcon,
  Layers as DbIcon,
  Save as SaveIcon,
  Options as BuilderIcon,
  ChevronDown as ChevronIcon,
  Brush as FormatIcon,
  CodeSlash as SnippetIcon,
  Settings as SettingsIcon,
} from "@vicons/ionicons5";
import { useConnectionStore } from "@/stores/connection";
import { useDatabaseStore } from "@/stores/database";
import { editorSettings } from "@/utils/editor-settings";
import { MONGO_SNIPPETS } from "@/utils/mongo-snippets";

const props = defineProps<{
  loading: boolean;
  executionTime?: number | null;
  resultCount?: number | null;
  connectionId?: string;
  database?: string;
  collection?: string;
  error?: string | null;
}>();

const emit = defineEmits<{
  run: [];
  runAll: [];
  stop: [];
  history: [];
  explain: [];
  queryBuilder: [];
  import: [];
  export: [];
  saveScript: [];
  format: [];
  insertSnippet: [body: string];
  "update:connectionId": [id: string];
  "update:database": [name: string];
}>();

const connStore = useConnectionStore();
const dbStore = useDatabaseStore();

// 代码片段下拉: 按 group 分组成二级菜单, key = 在 MONGO_SNIPPETS 里的下标
const snippetOptions = computed(() => {
  const groups = new Map<string, { label: string; key: string }[]>();
  MONGO_SNIPPETS.forEach((s, idx) => {
    if (!groups.has(s.group)) groups.set(s.group, []);
    groups.get(s.group)!.push({ label: s.label, key: String(idx) });
  });
  return [...groups.entries()].map(([group, children]) => ({
    label: group,
    key: `g:${group}`,
    children,
  }));
});

function onSnippetSelect(key: string) {
  const idx = Number(key);
  const snip = MONGO_SNIPPETS[idx];
  if (snip) emit("insertSnippet", snip.body);
}

const connectionLabel = computed(() => {
  if (!props.connectionId) return "";
  const cfg = connStore.connections.find((c: any) => c.id === props.connectionId);
  if (!cfg) return "";
  return cfg.name || `${cfg.host}:${cfg.port}`;
});

/** 已连接的服务器列表 (未连接的也列上, 加灰色标识) */
const connectionOptions = computed(() => {
  return connStore.connections.map((c: any) => {
    const active = connStore.isActive(c.id);
    const label = c.name || `${c.host}:${c.port}`;
    return {
      label: active ? label : `${label} (未连接)`,
      key: c.id,
      disabled: !active,
    };
  });
});

const databaseOptions = computed(() => {
  if (!props.connectionId) return [];
  const dbs = dbStore.getDatabases(props.connectionId);
  if (dbs.length === 0) {
    return [{ label: "(无数据库 / 未加载, 点击连接树展开)", key: "", disabled: true }];
  }
  return dbs.map((d) => ({ label: d.name, key: d.name }));
});

// 连接可用且数据库列表为空时, 自动拉一次, 让数据库下拉有候选
watch(
  () => props.connectionId,
  async (id) => {
    if (id && connStore.isActive(id) && dbStore.getDatabases(id).length === 0) {
      try {
        await dbStore.fetchDatabases(id);
      } catch {
        /* 忽略, 用户可手动展开连接树 */
      }
    }
  },
  { immediate: true },
);

function onConnSelect(id: string) {
  emit("update:connectionId", id);
}
function onDbSelect(name: string) {
  if (!name) return;
  emit("update:database", name);
}
</script>

<template>
  <div class="query-toolbar" @mousedown.prevent>
    <!-- 左侧: 执行 + 数据 + 工具 三组 -->
    <div class="toolbar-group">
      <!-- 执行组 -->
      <n-button
        type="primary"
        size="small"
        class="run-btn"
        :loading="props.loading"
        @click="emit('run')"
      >
        <template #icon><n-icon :size="15"><PlayIcon /></n-icon></template>
        Run
      </n-button>
      <n-tooltip trigger="hover" :delay="400">
        <template #trigger>
          <n-button class="icon-btn" size="small" quaternary @click="emit('runAll')">
            <template #icon><n-icon :size="15"><RunAllIcon /></n-icon></template>
          </n-button>
        </template>
        运行全部语句 (Ctrl+Shift+Enter)
      </n-tooltip>
      <n-tooltip trigger="hover" :delay="400">
        <template #trigger>
          <n-button
            class="icon-btn"
            size="small"
            tertiary
            :disabled="!props.loading"
            @click="emit('stop')"
          >
            <template #icon><n-icon :size="15"><StopIcon /></n-icon></template>
          </n-button>
        </template>
        停止查询
      </n-tooltip>

      <div class="sep" />

      <!-- 编辑组 -->
      <n-tooltip trigger="hover" :delay="400">
        <template #trigger>
          <n-button class="icon-btn" size="small" quaternary @click="emit('format')">
            <template #icon><n-icon :size="15"><FormatIcon /></n-icon></template>
          </n-button>
        </template>
        格式化代码 (Ctrl+Alt+L)
      </n-tooltip>
      <n-dropdown
        trigger="click"
        :options="snippetOptions"
        placement="bottom-start"
        @select="onSnippetSelect"
      >
        <n-tooltip trigger="hover" :delay="400">
          <template #trigger>
            <n-button class="icon-btn" size="small" quaternary>
              <template #icon><n-icon :size="15"><SnippetIcon /></n-icon></template>
            </n-button>
          </template>
          插入代码片段
        </n-tooltip>
      </n-dropdown>
      <n-popover trigger="click" placement="bottom-start">
        <template #trigger>
          <n-button class="icon-btn" size="small" quaternary>
            <template #icon><n-icon :size="15"><SettingsIcon /></n-icon></template>
          </n-button>
        </template>
        <div class="settings-pop">
          <div class="settings-title">编辑器设置</div>
          <div class="set-row">
            <span class="set-label">字号</span>
            <n-input-number
              v-model:value="editorSettings.fontSize"
              size="small"
              :min="8"
              :max="32"
              style="width: 96px"
            />
          </div>
          <div class="set-row">
            <span class="set-label">自动换行</span>
            <n-switch v-model:value="editorSettings.wordWrap" size="small" />
          </div>
          <div class="set-row">
            <span class="set-label">小地图</span>
            <n-switch v-model:value="editorSettings.minimap" size="small" />
          </div>
        </div>
      </n-popover>

      <div class="sep" />

      <!-- 数据组 -->
      <n-tooltip trigger="hover" :delay="400">
        <template #trigger>
          <n-button class="icon-btn" size="small" quaternary @click="emit('import')">
            <template #icon><n-icon :size="15"><ImportIcon /></n-icon></template>
          </n-button>
        </template>
        导入数据
      </n-tooltip>
      <n-tooltip trigger="hover" :delay="400">
        <template #trigger>
          <n-button class="icon-btn" size="small" quaternary @click="emit('export')">
            <template #icon><n-icon :size="15"><ExportIcon /></n-icon></template>
          </n-button>
        </template>
        导出数据
      </n-tooltip>

      <div class="sep" />

      <!-- 工具组 -->
      <n-tooltip trigger="hover" :delay="400">
        <template #trigger>
          <n-button class="icon-btn" size="small" quaternary @click="emit('history')">
            <template #icon><n-icon :size="15"><HistoryIcon /></n-icon></template>
          </n-button>
        </template>
        查询历史
      </n-tooltip>
      <n-tooltip trigger="hover" :delay="400">
        <template #trigger>
          <n-button class="icon-btn" size="small" quaternary @click="emit('saveScript')">
            <template #icon><n-icon :size="15"><SaveIcon /></n-icon></template>
          </n-button>
        </template>
        保存为脚本
      </n-tooltip>
    </div>

    <!-- 右侧: 上下文 (连接/库) + 分析 -->
    <div class="toolbar-group toolbar-right">
      <n-tooltip v-if="props.error" trigger="hover">
        <template #trigger>
          <span class="error-badge">✕ Error</span>
        </template>
        {{ props.error }}
      </n-tooltip>

      <!-- 当前连接 -->
      <n-dropdown trigger="click" :options="connectionOptions" @select="onConnSelect">
        <button class="ctx-pill" title="点击切换连接">
          <n-icon :size="13" class="ctx-icon"><ServerIcon /></n-icon>
          <span :class="connectionLabel ? 'ctx-val conn' : 'ctx-placeholder'">
            {{ connectionLabel || "选择连接" }}
          </span>
          <n-icon :size="11" class="ctx-chevron"><ChevronIcon /></n-icon>
        </button>
      </n-dropdown>

      <!-- 当前数据库 -->
      <n-dropdown trigger="click" :options="databaseOptions" @select="onDbSelect">
        <button class="ctx-pill" title="点击切换数据库">
          <n-icon :size="13" class="ctx-icon"><DbIcon /></n-icon>
          <span :class="props.database ? 'ctx-val db' : 'ctx-placeholder'">
            {{ props.database || "选择数据库" }}
          </span>
          <n-icon :size="11" class="ctx-chevron"><ChevronIcon /></n-icon>
        </button>
      </n-dropdown>

      <div class="sep" />

      <!-- 分析组 -->
      <n-tooltip trigger="hover" :delay="400">
        <template #trigger>
          <n-button class="icon-btn" size="small" quaternary @click="emit('queryBuilder')">
            <template #icon><n-icon :size="15"><BuilderIcon /></n-icon></template>
          </n-button>
        </template>
        可视化查询构建器
      </n-tooltip>
      <n-tooltip trigger="hover" :delay="400">
        <template #trigger>
          <n-button class="icon-btn" size="small" quaternary @click="emit('explain')">
            <template #icon><n-icon :size="15"><ExplainIcon /></n-icon></template>
          </n-button>
        </template>
        Explain 执行计划
      </n-tooltip>
    </div>
  </div>
</template>

<style scoped>
.query-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: nowrap;
  gap: 8px;
  padding: 4px 8px;
  height: 38px;
  box-sizing: border-box;
  background: #fafafa;
  border-bottom: 1px solid #e5e5e5;
  flex-shrink: 0;
}
.toolbar-group {
  display: flex;
  align-items: center;
  gap: 3px;
  flex-wrap: nowrap;
}
.toolbar-right {
  min-width: 0;
  overflow: hidden;
}

/* 组与组之间的细分隔线 */
.sep {
  width: 1px;
  align-self: stretch;
  margin: 4px 5px;
  background: #e0e0e0;
}

/* Run —— 唯一带文字的主操作 */
.run-btn {
  font-weight: 600;
  padding: 0 12px;
}

/* 图标按钮: 正方形, 统一尺寸 */
.icon-btn {
  width: 28px;
  padding: 0;
}

/* 连接 / 数据库 上下文药丸 */
.ctx-pill {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  max-width: 160px;
  height: 26px;
  padding: 0 7px;
  border: 1px solid #e0e0e0;
  border-radius: 5px;
  background: #fff;
  cursor: pointer;
  font: inherit;
  font-size: 12px;
  transition: border-color 0.15s, background 0.15s;
}
.ctx-pill:hover {
  border-color: #c0c0c0;
  background: #f5f5f5;
}
.ctx-icon {
  color: #999;
  flex-shrink: 0;
}
.ctx-chevron {
  color: #bbb;
  flex-shrink: 0;
}
.ctx-val {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 500;
}
.ctx-val.conn {
  color: #d48806;
}
.ctx-val.db {
  color: #389e0d;
}
.ctx-placeholder {
  color: #aaa;
  font-style: italic;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.error-badge {
  font-size: 11px;
  color: #e03e3e;
  font-weight: 600;
  cursor: default;
  padding: 2px 6px;
  border-radius: 4px;
  background: #fdeaea;
  white-space: nowrap;
  flex-shrink: 0;
}

/* 编辑器设置弹层 */
.settings-pop {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 180px;
  padding: 2px;
}
.settings-title {
  font-size: 12px;
  font-weight: 600;
  color: #666;
  padding-bottom: 4px;
  border-bottom: 1px solid #eee;
}
.set-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.set-label {
  font-size: 13px;
  color: #444;
}
</style>

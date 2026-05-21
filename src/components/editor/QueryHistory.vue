<script setup lang="ts">
import { ref, computed, onMounted, watch, h } from "vue";
import {
  NDrawer, NDrawerContent, NInput, NCollapse, NCollapseItem,
  NButton, NSpace, NEmpty, NTag, NModal, NCard, NTooltip, useMessage,
} from "naive-ui";
import * as queryApi from "@/api/query";
import type { HistoryEntry } from "@/types/database";
import { useConnectionStore } from "@/stores/connection";

const props = defineProps<{ show: boolean }>();
const emit = defineEmits<{
  "update:show": [value: boolean];
  /** 用户选中一条记录 -> 加载到当前编辑器 */
  select: [queryText: string];
}>();

const message = useMessage();
const connStore = useConnectionStore();

const history = ref<HistoryEntry[]>([]);
const searchKeyword = ref("");
const loading = ref(false);
/** 已展开的连接组 (key = connectionId), 默认全部展开 */
const expanded = ref<string[]>([]);

// 详情查看 modal
const showDetail = ref(false);
const detailEntry = ref<HistoryEntry | null>(null);

onMounted(loadHistory);
watch(() => props.show, (s) => { if (s) loadHistory(); });

async function loadHistory() {
  loading.value = true;
  try {
    history.value = await queryApi.listAllQueryHistory(500);
    // 默认展开所有连接组
    expanded.value = [...new Set(history.value.map((h) => h.connectionId))];
  } catch (e) {
    message.error(`加载执行记录失败: ${e}`);
  } finally {
    loading.value = false;
  }
}

async function handleClearAll() {
  try {
    await queryApi.clearAllQueryHistory();
    history.value = [];
    message.success("已清空所有执行记录");
  } catch (e) {
    message.error(`清空失败: ${e}`);
  }
}

/** 当前关键字过滤后的记录, 仍保持原始时间排序 */
const filteredHistory = computed(() => {
  const kw = searchKeyword.value.trim().toLowerCase();
  if (!kw) return history.value;
  return history.value.filter(
    (e) =>
      e.queryText.toLowerCase().includes(kw) ||
      e.databaseName.toLowerCase().includes(kw) ||
      (e.collectionName?.toLowerCase().includes(kw) ?? false),
  );
});

/** 按 connectionId 分组 (Map 保留首次出现顺序 = 时间最近的连接先出现) */
const groupedHistory = computed(() => {
  const groups = new Map<string, HistoryEntry[]>();
  for (const e of filteredHistory.value) {
    if (!groups.has(e.connectionId)) groups.set(e.connectionId, []);
    groups.get(e.connectionId)!.push(e);
  }
  return [...groups.entries()].map(([connectionId, entries]) => ({
    connectionId,
    name: getConnDisplay(connectionId),
    entries,
  }));
});

function getConnDisplay(id: string): string {
  const c = connStore.connections.find((x) => x.id === id);
  if (!c) return `(已删除/未知连接) ${id.slice(0, 8)}`;
  return c.name || `${c.host}:${c.port}`;
}

function handleSelect(entry: HistoryEntry) {
  emit("select", entry.queryText);
  emit("update:show", false);
}

function handleView(entry: HistoryEntry, e: Event) {
  e.stopPropagation();
  detailEntry.value = entry;
  showDetail.value = true;
}

async function handleCopy(entry: HistoryEntry, e: Event) {
  e.stopPropagation();
  try {
    await navigator.clipboard.writeText(entry.queryText);
    message.success("已复制查询语句");
  } catch {
    message.error("复制失败");
  }
}

async function copyDetailText() {
  if (!detailEntry.value) return;
  try {
    await navigator.clipboard.writeText(detailEntry.value.queryText);
    message.success("已复制查询语句");
  } catch {
    message.error("复制失败");
  }
}

function formatTime(s: string): string {
  // SQLite 的 datetime('now') 返回 UTC 字符串 "YYYY-MM-DD HH:MM:SS" (无 Z),
  // 这里加 Z 让 JS Date 当 UTC 解析, 再输出本地时区时间.
  if (!s) return "";
  const isoLike = s.includes("T") ? s : s.replace(" ", "T");
  const withZ = /[zZ]$|[+-]\d{2}:?\d{2}$/.test(isoLike) ? isoLike : `${isoLike}Z`;
  const d = new Date(withZ);
  if (isNaN(d.getTime())) return s;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
    + ` ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

// h() 内部用占位
void h;
</script>

<template>
  <n-drawer
    :show="props.show"
    :width="540"
    placement="right"
    @update:show="emit('update:show', $event)"
  >
    <n-drawer-content title="执行记录">
      <!-- @vue-ignore -->
      <template #header-extra>
        <n-button
          size="small"
          quaternary
          type="error"
          :disabled="history.length === 0"
          @click="handleClearAll"
        >
          清空全部
        </n-button>
      </template>

      <n-space vertical :size="12">
        <n-input
          v-model:value="searchKeyword"
          placeholder="搜索 (查询语句 / 库名 / 集合名)"
          clearable
        />

        <div v-if="loading" style="color:#888;font-size:13px">加载中...</div>
        <n-empty
          v-else-if="filteredHistory.length === 0"
          :description="searchKeyword ? '没有匹配的记录' : '暂无执行记录'"
        />
        <n-collapse v-else v-model:expanded-names="expanded" arrow-placement="left">
          <n-collapse-item
            v-for="g in groupedHistory"
            :key="g.connectionId"
            :name="g.connectionId"
          >
            <template #header>
              <span class="conn-header">
                <span class="conn-name">{{ g.name }}</span>
                <n-tag size="small" round style="margin-left: 8px">{{ g.entries.length }}</n-tag>
              </span>
            </template>
            <div class="entry-list">
              <div
                v-for="entry in g.entries"
                :key="entry.id"
                class="entry-row"
                :title="'点击加载到编辑器'"
                @click="handleSelect(entry)"
              >
                <div class="entry-line1">
                  <code class="entry-text">{{ entry.queryText }}</code>
                </div>
                <div class="entry-line2">
                  <n-tag v-if="entry.errorMessage" type="error" size="tiny" round>失败</n-tag>
                  <n-tag v-else type="success" size="tiny" round>
                    {{ entry.resultCount ?? 0 }} 条
                  </n-tag>
                  <span v-if="entry.executionTimeMs !== null" class="meta">
                    {{ entry.executionTimeMs }}ms
                  </span>
                  <span class="meta">
                    {{ entry.databaseName }}{{ entry.collectionName ? `.${entry.collectionName}` : "" }}
                  </span>
                  <span class="meta time">{{ formatTime(entry.createdAt) }}</span>
                  <span class="entry-actions" @click.stop>
                    <n-tooltip trigger="hover" :delay="300">
                      <template #trigger>
                        <n-button size="tiny" quaternary @click="handleView(entry, $event)">
                          查看
                        </n-button>
                      </template>
                      查看完整内容
                    </n-tooltip>
                    <n-tooltip trigger="hover" :delay="300">
                      <template #trigger>
                        <n-button size="tiny" quaternary @click="handleCopy(entry, $event)">
                          复制
                        </n-button>
                      </template>
                      复制到剪贴板
                    </n-tooltip>
                  </span>
                </div>
              </div>
            </div>
          </n-collapse-item>
        </n-collapse>
      </n-space>
    </n-drawer-content>

    <!-- 详情查看 modal -->
    <n-modal v-model:show="showDetail">
      <n-card
        v-if="detailEntry"
        style="width: 640px; max-height: 80vh"
        :title="`执行记录详情 — ${getConnDisplay(detailEntry.connectionId)}`"
        :bordered="false"
        closable
        @close="showDetail = false"
      >
        <div class="detail-meta">
          <span class="meta">
            {{ detailEntry.databaseName }}{{ detailEntry.collectionName ? `.${detailEntry.collectionName}` : "" }}
          </span>
          <n-tag v-if="detailEntry.errorMessage" type="error" size="small">失败</n-tag>
          <n-tag v-else type="success" size="small">
            {{ detailEntry.resultCount ?? 0 }} 条
          </n-tag>
          <span v-if="detailEntry.executionTimeMs !== null" class="meta">
            {{ detailEntry.executionTimeMs }}ms
          </span>
          <span class="meta time">{{ formatTime(detailEntry.createdAt) }}</span>
        </div>
        <pre class="detail-text">{{ detailEntry.queryText }}</pre>
        <pre v-if="detailEntry.errorMessage" class="detail-error">{{ detailEntry.errorMessage }}</pre>
        <template #footer>
          <n-space justify="end">
            <n-button size="small" @click="copyDetailText">复制语句</n-button>
            <n-button size="small" type="primary" @click="
              () => { if (detailEntry) { handleSelect(detailEntry); showDetail = false; } }
            ">加载到编辑器</n-button>
          </n-space>
        </template>
      </n-card>
    </n-modal>
  </n-drawer>
</template>

<style scoped>
.conn-header {
  display: inline-flex;
  align-items: center;
  font-size: 13px;
  color: #333;
}
.conn-name { font-weight: 600; }
.entry-list { display: flex; flex-direction: column; gap: 4px; }
.entry-row {
  padding: 6px 8px;
  border-radius: 4px;
  cursor: pointer;
  border: 1px solid transparent;
  transition: background 0.1s;
}
.entry-row:hover {
  background: #f5f7fa;
  border-color: #e0e0e0;
}
.entry-line1 { margin-bottom: 4px; }
.entry-text {
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 12px;
  color: #333;
  display: block;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.entry-line2 {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11px;
  color: #999;
}
.entry-actions {
  margin-left: auto;
  display: inline-flex;
  gap: 2px;
}
.meta { color: #888; font-size: 11px; }
.meta.time { color: #aaa; }
.detail-meta {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 10px;
  font-size: 12px;
}
.detail-text {
  margin: 0;
  padding: 10px 12px;
  background: #fafafa;
  border: 1px solid #e8e8e8;
  border-radius: 4px;
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 12px;
  line-height: 1.5;
  color: #333;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 40vh;
  overflow: auto;
}
.detail-error {
  margin: 8px 0 0;
  padding: 8px 12px;
  background: #fff1f0;
  border: 1px solid #ffccc7;
  border-radius: 4px;
  font-family: "Fira Code", "Consolas", monospace;
  font-size: 12px;
  color: #cf1322;
  white-space: pre-wrap;
}
</style>

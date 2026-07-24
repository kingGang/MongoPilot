<script setup lang="ts">
import { ref, computed, watch, onUnmounted } from "vue";
import {
  NModal, NCard, NButton, NInput, NCheckbox, NSpace, NIcon, NProgress, NScrollbar, NTag,
} from "naive-ui";
import { FolderOpen as BrowseIcon, ArchiveOutline as BackupIcon } from "@vicons/ionicons5";
import { open as openDirDialog } from "@tauri-apps/plugin-dialog";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useDatabaseStore } from "@/stores/database";
import {
  backupDatabase, formatBytes, defaultBackupFolderName, joinPath,
  type BackupProgress, type BackupSummary,
} from "@/api/backup";

const props = defineProps<{
  show: boolean;
  connectionId: string;
  database: string;
  /** 非空时只备份这一个集合 (从集合节点右键进来) */
  collection?: string;
}>();

const emit = defineEmits<{
  "update:show": [val: boolean];
  done: [summary: BackupSummary];
}>();

const dbStore = useDatabaseStore();

// ---- 表单 ----
const targetDir = ref("");
const gzip = ref(false);
const selected = ref<string[]>([]);
const running = ref(false);
const errorMsg = ref("");
const summary = ref<BackupSummary | null>(null);

// ---- 进度 ----
const progress = ref<BackupProgress | null>(null);
let unlisten: UnlistenFn | null = null;
onUnmounted(() => unlisten?.());

/** 集合完整信息 (name/count/size), 列表里展示数据量 */
const collectionInfos = computed(() =>
  dbStore.getCollections(props.connectionId, props.database),
);
const allCollections = computed(() => collectionInfos.value.map((c) => c.name));

/** 已勾选集合的文档总数 + 数据总量, 展示在表头 */
const selectedStats = computed(() => {
  let count = 0;
  let size = 0;
  for (const c of collectionInfos.value) {
    if (selected.value.includes(c.name)) {
      count += c.count;
      size += c.size;
    }
  }
  return { count, size };
});

/** 单集合模式下列表不可改, 只展示那一个 */
const singleMode = computed(() => !!props.collection);

const finalDir = computed(() =>
  targetDir.value ? joinPath(targetDir.value, props.database) : "",
);

const progressPercent = computed(() => {
  const p = progress.value;
  if (!p || p.collTotal <= 0) return 0;
  // 以"集合"为粒度算总进度, 当前集合内部再按文档数插值
  const inColl = p.docsTotal > 0 ? Math.min(1, p.docsDone / p.docsTotal) : 0;
  return Math.min(100, Math.round(((p.collIndex - 1 + inColl) / p.collTotal) * 100));
});

const canRun = computed(
  () => !!targetDir.value && !running.value && (singleMode.value || selected.value.length > 0),
);

watch(
  () => props.show,
  async (show) => {
    if (!show) return;
    targetDir.value = "";
    gzip.value = false;
    errorMsg.value = "";
    summary.value = null;
    progress.value = null;
    running.value = false;
    if (props.collection) {
      selected.value = [props.collection];
    } else {
      await dbStore.fetchCollections(props.connectionId, props.database);
      selected.value = [...allCollections.value];
    }
  },
);

async function browseDir() {
  const picked = await openDirDialog({ directory: true, title: "选择备份存放目录" });
  if (!picked) return;
  // 自动加一层带时间戳的子目录, 免得反复备份互相覆盖
  targetDir.value = joinPath(String(picked), defaultBackupFolderName(props.database));
}

function toggleAll() {
  selected.value =
    selected.value.length === allCollections.value.length ? [] : [...allCollections.value];
}

function toggleColl(name: string) {
  selected.value = selected.value.includes(name)
    ? selected.value.filter((n) => n !== name)
    : [...selected.value, name];
}

async function handleRun() {
  running.value = true;
  errorMsg.value = "";
  summary.value = null;
  progress.value = null;
  unlisten = await listen<BackupProgress>("backup-progress", (ev) => {
    progress.value = ev.payload;
  });

  try {
    const result = await backupDatabase({
      connectionId: props.connectionId,
      database: props.database,
      // 全选时传空数组 = 整库 (后端会自动带上此刻新增的集合)
      collections:
        !singleMode.value && selected.value.length === allCollections.value.length
          ? []
          : selected.value,
      targetDir: targetDir.value,
      gzip: gzip.value,
    });
    summary.value = result;
    emit("done", result);
  } catch (e) {
    errorMsg.value = String(e);
  } finally {
    unlisten?.();
    unlisten = null;
    running.value = false;
  }
}

function handleClose() {
  if (running.value) return;
  emit("update:show", false);
}
</script>

<template>
  <n-modal
    :show="show"
    :mask-closable="!running"
    :trap-focus="false"
    @update:show="emit('update:show', $event)"
  >
    <n-card
      :title="singleMode ? `备份集合 ${collection}` : `备份数据库 ${database}`"
      :bordered="false"
      :closable="!running"
      role="dialog"
      style="width: 620px"
      @close="handleClose"
    >
      <div class="bk-row">
        <label class="bk-label">备份到</label>
        <n-input
          v-model:value="targetDir"
          size="small"
          placeholder="选择一个目录..."
          style="flex: 1"
          :disabled="running"
        />
        <n-button size="small" style="margin-left: 6px" :disabled="running" @click="browseDir">
          <template #icon><n-icon :size="14"><BrowseIcon /></n-icon></template>
        </n-button>
      </div>

      <div v-if="finalDir" class="bk-hint">
        实际写入 <code>{{ finalDir }}</code>，每个集合一份
        <code>.bson</code> + <code>.metadata.json</code>（含索引定义）
      </div>

      <div class="bk-row">
        <label class="bk-label">选项</label>
        <n-checkbox v-model:checked="gzip" :disabled="running" size="small">
          gzip 压缩（文件名带 .gz，等价 mongodump --gzip）
        </n-checkbox>
      </div>

      <!-- 集合选择 -->
      <div class="bk-row" style="align-items: flex-start">
        <label class="bk-label" style="margin-top: 4px">集合</label>
        <div style="flex: 1; min-width: 0">
          <div v-if="singleMode" class="bk-single">
            <n-tag size="small" type="info">{{ collection }}</n-tag>
          </div>
          <template v-else>
            <div class="bk-list-head">
              <n-checkbox
                size="small"
                :checked="selected.length === allCollections.length && allCollections.length > 0"
                :indeterminate="selected.length > 0 && selected.length < allCollections.length"
                :disabled="running"
                @update:checked="toggleAll"
              >
                全选
              </n-checkbox>
              <span class="bk-count">
                已选 {{ selected.length }} / {{ allCollections.length }}
                <template v-if="selected.length">· {{ selectedStats.count.toLocaleString() }} 条 · {{ formatBytes(selectedStats.size) }}</template>
              </span>
            </div>
            <div class="bk-list">
              <n-scrollbar style="max-height: 180px">
                <div class="bk-grid">
                  <div v-for="c in collectionInfos" :key="c.name" class="bk-item">
                    <n-checkbox
                      size="small"
                      :checked="selected.includes(c.name)"
                      :disabled="running"
                      @update:checked="toggleColl(c.name)"
                    >
                      <span class="bk-item-name">{{ c.name }}</span>
                    </n-checkbox>
                    <span class="bk-item-meta">{{ c.count.toLocaleString() }} · {{ formatBytes(c.size) }}</span>
                  </div>
                </div>
                <div v-if="collectionInfos.length === 0" class="bk-empty">该数据库下没有集合</div>
              </n-scrollbar>
            </div>
          </template>
        </div>
      </div>

      <!-- 进度 / 结果 -->
      <div v-if="running || summary || errorMsg" class="bk-status">
        <n-progress
          v-if="running || summary"
          type="line"
          :percentage="summary ? 100 : progressPercent"
          :status="errorMsg ? 'error' : summary ? 'success' : 'default'"
          indicator-placement="inside"
          :height="20"
          style="margin-bottom: 6px"
        />
        <div v-if="running" class="bk-status-line">
          {{ progress?.phase ?? "准备中..." }}
          <template v-if="progress?.collection">
            <strong>{{ progress.collection }}</strong>
            ({{ progress.collIndex }}/{{ progress.collTotal }})
            —— {{ progress.docsDone.toLocaleString() }} 条
          </template>
        </div>
        <div v-else-if="errorMsg" class="bk-status-line bk-error">{{ errorMsg }}</div>
        <div v-else-if="summary" class="bk-status-line bk-success">
          备份完成：{{ summary.collections }} 个集合、{{ summary.documents.toLocaleString() }} 条文档、{{ formatBytes(summary.bytes) }}
          <div class="bk-hint" style="margin-top: 4px">{{ summary.outputDir }}</div>
        </div>
      </div>

      <template #action>
        <n-space justify="end">
          <n-button size="small" :disabled="running" @click="handleClose">
            {{ summary ? "关闭" : "取消" }}
          </n-button>
          <n-button
            type="primary"
            size="small"
            :disabled="!canRun"
            :loading="running"
            @click="handleRun"
          >
            <template #icon><n-icon :size="14"><BackupIcon /></n-icon></template>
            开始备份
          </n-button>
        </n-space>
      </template>
    </n-card>
  </n-modal>
</template>

<style scoped>
.bk-row { display: flex; align-items: center; margin-bottom: 10px; }
.bk-label { width: 60px; font-size: 13px; font-weight: 500; color: #333; flex-shrink: 0; }
.bk-hint { font-size: 12px; color: #888; margin: -4px 0 10px 60px; line-height: 1.6; word-break: break-all; }
.bk-hint code { background: #f2f2f2; padding: 1px 4px; border-radius: 3px; }
.bk-list-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 4px; }
.bk-count { font-size: 12px; color: #888; }
.bk-list { border: 1px solid #e0e0e0; border-radius: 4px; }
.bk-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 2px 12px; padding: 8px 10px; }
.bk-item { display: flex; align-items: center; justify-content: space-between; gap: 6px; font-size: 13px; min-width: 0; }
.bk-item :deep(.n-checkbox) { display: flex; align-items: center; min-width: 0; }
.bk-item :deep(.n-checkbox__label) { overflow: hidden; }
.bk-item-name { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.bk-item-meta { font-size: 11px; color: #999; white-space: nowrap; flex-shrink: 0; }
.bk-empty { font-size: 12px; color: #999; padding: 8px 0; text-align: center; }
.bk-single { padding: 2px 0; }
.bk-status { margin-top: 8px; padding: 8px; border-top: 1px solid #eee; }
.bk-status-line { font-size: 12px; color: #3875d7; text-align: center; }
.bk-error { color: #d03050; text-align: left; word-break: break-all; }
.bk-success { color: #18a058; }
</style>

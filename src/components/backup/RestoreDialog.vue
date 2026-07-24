<script setup lang="ts">
import { ref, computed, watch, onUnmounted, h } from "vue";
import {
  NModal, NCard, NButton, NInput, NSelect, NCheckbox, NSpace, NIcon, NProgress, NScrollbar, NAlert,
  useDialog, useMessage,
} from "naive-ui";
import { FolderOpen as BrowseIcon, CloudUploadOutline as RestoreIcon } from "@vicons/ionicons5";
import { open as openDirDialog } from "@tauri-apps/plugin-dialog";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useConnectionStore } from "@/stores/connection";
import { useDatabaseStore } from "@/stores/database";
import {
  scanBackupDir, restoreBackup, formatBytes,
  type BackupDirInfo, type RestoreMode, type RestoreProgress, type RestoreSummary,
} from "@/api/backup";

const props = defineProps<{
  show: boolean;
  connectionId: string;
  database: string;
}>();

const emit = defineEmits<{
  "update:show": [val: boolean];
  done: [summary: RestoreSummary];
}>();

const connStore = useConnectionStore();
const dbStore = useDatabaseStore();
const dlg = useDialog();
const msg = useMessage();

// ---- 表单 ----
const sourceDir = ref("");
const scanning = ref(false);
const dirInfo = ref<BackupDirInfo | null>(null);
const selectedConnId = ref(props.connectionId);
const targetDb = ref(props.database);
const selected = ref<string[]>([]);
const mode = ref<RestoreMode>("drop");
const restoreIndexes = ref(true);
const running = ref(false);
const errorMsg = ref("");
const summary = ref<RestoreSummary | null>(null);

// ---- 进度 ----
const progress = ref<RestoreProgress | null>(null);
let unlisten: UnlistenFn | null = null;
onUnmounted(() => unlisten?.());

const modeOptions = [
  { label: "先删除目标集合再恢复（与备份完全一致）", value: "drop" },
  { label: "按 _id 覆盖已有文档，其余插入", value: "overwrite" },
  { label: "跳过 _id 已存在的文档", value: "skip" },
  { label: "直接插入（_id 冲突会报错）", value: "insert" },
];

const connOptions = computed(() => {
  const opts = connStore.connections
    .filter((c) => connStore.isActive(c.id))
    .map((c) => ({ label: c.name || `${c.host}:${c.port}`, value: c.id }));
  if (opts.length === 0) {
    const cfg = connStore.connections.find((c) => c.id === selectedConnId.value);
    if (cfg) opts.push({ label: cfg.name || `${cfg.host}:${cfg.port}`, value: cfg.id });
  }
  return opts;
});

const dbOptions = computed(() => {
  const dbs = dbStore.getDatabases(selectedConnId.value).map((d) => ({ label: d.name, value: d.name }));
  // 允许恢复到一个尚不存在的新库, 所以下拉是 filterable + tag
  if (targetDb.value && !dbs.some((d) => d.value === targetDb.value)) {
    dbs.unshift({ label: targetDb.value, value: targetDb.value });
  }
  return dbs;
});

const allCollections = computed(() => dirInfo.value?.collections ?? []);

const targetIsReadOnly = computed(() => connStore.isReadOnly(selectedConnId.value));

const progressPercent = computed(() => {
  const p = progress.value;
  if (!p || p.collTotal <= 0) return 0;
  // 以"集合"为粒度算总进度, 当前集合内部再按已读文件字节插值
  const inColl = p.bytesTotal > 0 ? Math.min(1, p.bytesDone / p.bytesTotal) : 0;
  return Math.min(100, Math.round(((p.collIndex - 1 + inColl) / p.collTotal) * 100));
});

const canRun = computed(
  () => !!dirInfo.value && !!targetDb.value && selected.value.length > 0
    && !running.value && !targetIsReadOnly.value,
);

watch(
  () => props.show,
  (show) => {
    if (!show) return;
    sourceDir.value = "";
    dirInfo.value = null;
    selected.value = [];
    mode.value = "drop";
    restoreIndexes.value = true;
    errorMsg.value = "";
    summary.value = null;
    progress.value = null;
    running.value = false;
    selectedConnId.value = props.connectionId;
    targetDb.value = props.database;
  },
);

async function browseDir() {
  const picked = await openDirDialog({ directory: true, title: "选择备份目录" });
  if (!picked) return;
  sourceDir.value = String(picked);
  await scan();
}

async function scan() {
  if (!sourceDir.value) return;
  scanning.value = true;
  errorMsg.value = "";
  dirInfo.value = null;
  try {
    const info = await scanBackupDir(sourceDir.value);
    dirInfo.value = info;
    selected.value = info.collections.map((c) => c.name);
    // 默认恢复回同名库
    if (info.database) targetDb.value = info.database;
  } catch (e) {
    errorMsg.value = String(e);
  } finally {
    scanning.value = false;
  }
}

function handleConnChange(connId: string) {
  selectedConnId.value = connId;
}

function toggleAll() {
  selected.value =
    selected.value.length === allCollections.value.length
      ? []
      : allCollections.value.map((c) => c.name);
}

function toggleColl(name: string) {
  selected.value = selected.value.includes(name)
    ? selected.value.filter((n) => n !== name)
    : [...selected.value, name];
}

function connName(id: string): string {
  const c = connStore.connections.find((x) => x.id === id);
  return c?.name || (c ? `${c.host}:${c.port}` : id);
}

const modeLabel = computed(
  () => modeOptions.find((m) => m.value === mode.value)?.label ?? mode.value,
);

/** 执行前二次确认: drop 模式要求手输目标库名, 其它模式普通确认 */
function handleRun() {
  if (!dirInfo.value) return;
  const target = `${connName(selectedConnId.value)} / ${targetDb.value}`;
  const collCount = selected.value.length;

  if (mode.value === "drop") {
    // 破坏性: 逼用户看清并亲手输入目标库名, 专治"选错库"
    const inputId = "__restore_confirm_db";
    dlg.error({
      title: "确认覆盖恢复",
      content: () =>
        h("div", { style: "font-size:13px;line-height:1.8" }, [
          h("div", null, ["即将恢复到 ", h("strong", { style: "color:#d03050" }, target)]),
          h("div", null, `共 ${collCount} 个集合，模式：先删除目标集合再恢复`),
          h(
            "div",
            { style: "margin-top:6px;color:#d03050;font-weight:600" },
            "⚠ 目标库中同名集合会被删除后重建，其中现有数据将永久丢失、不可恢复。",
          ),
          h("div", { style: "margin-top:10px;color:#666" }, [
            "请输入目标库名 ",
            h("strong", null, targetDb.value),
            " 以确认：",
          ]),
          h("input", {
            id: inputId,
            autocomplete: "off",
            placeholder: targetDb.value,
            style:
              "width:100%;margin-top:6px;padding:6px 8px;border:1px solid #ddd;border-radius:4px;font-size:13px",
          }),
        ]),
      positiveText: "确认覆盖恢复",
      negativeText: "取消",
      onPositiveClick: () => {
        const input = document.getElementById(inputId) as HTMLInputElement | null;
        if ((input?.value ?? "").trim() !== targetDb.value) {
          msg.warning("库名不匹配，已取消");
          return false; // 返回 false 让确认框不关闭, 便于重输
        }
        void doRestore();
      },
    });
    return;
  }

  dlg.warning({
    title: "确认恢复",
    content: () =>
      h("div", { style: "font-size:13px;line-height:1.8" }, [
        h("div", null, ["即将恢复到 ", h("strong", null, target)]),
        h("div", null, `共 ${collCount} 个集合，冲突处理：${modeLabel.value}`),
        h("div", { style: "margin-top:6px;color:#e0803a" }, "该操作会向目标库写入数据。"),
      ]),
    positiveText: "确认恢复",
    negativeText: "取消",
    onPositiveClick: () => {
      void doRestore();
    },
  });
}

async function doRestore() {
  if (!dirInfo.value) return;
  running.value = true;
  errorMsg.value = "";
  summary.value = null;
  progress.value = null;
  unlisten = await listen<RestoreProgress>("restore-progress", (ev) => {
    progress.value = ev.payload;
  });

  try {
    const result = await restoreBackup({
      connectionId: selectedConnId.value,
      sourceDir: dirInfo.value.dir,
      targetDatabase: targetDb.value,
      collections:
        selected.value.length === allCollections.value.length ? [] : selected.value,
      mode: mode.value,
      restoreIndexes: restoreIndexes.value,
    });
    summary.value = result;
    emit("done", result);
    // 恢复后刷新目标库的集合列表, 让侧边树能看到新集合
    await dbStore.fetchCollections(selectedConnId.value, targetDb.value);
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
      title="从备份恢复"
      :bordered="false"
      :closable="!running"
      role="dialog"
      style="width: 640px"
      @close="handleClose"
    >
      <!-- 备份目录 -->
      <div class="rs-row">
        <label class="rs-label">备份目录</label>
        <n-input
          v-model:value="sourceDir"
          size="small"
          placeholder="选择 mongodump / MongoPilot 备份目录..."
          style="flex: 1"
          :disabled="running"
          @blur="scan"
        />
        <n-button size="small" style="margin-left: 6px" :disabled="running" @click="browseDir">
          <template #icon><n-icon :size="14"><BrowseIcon /></n-icon></template>
        </n-button>
      </div>
      <div v-if="dirInfo" class="rs-hint">
        识别到备份库 <strong>{{ dirInfo.database }}</strong>，共
        {{ dirInfo.collections.length }} 个集合
      </div>

      <!-- 目标 -->
      <div class="rs-row">
        <label class="rs-label">恢复到</label>
        <n-select
          :value="selectedConnId"
          :options="connOptions"
          size="small"
          style="width: 200px"
          :disabled="running"
          @update:value="handleConnChange"
        />
        <n-select
          v-model:value="targetDb"
          :options="dbOptions"
          size="small"
          style="flex: 1; margin-left: 6px"
          placeholder="目标数据库"
          filterable
          tag
          :disabled="running"
        />
      </div>

      <!-- 模式 -->
      <div class="rs-row">
        <label class="rs-label">冲突处理</label>
        <n-select v-model:value="mode" :options="modeOptions" size="small" style="flex: 1" :disabled="running" />
      </div>
      <div class="rs-row">
        <label class="rs-label" />
        <n-checkbox v-model:checked="restoreIndexes" size="small" :disabled="running">
          按备份的 metadata 重建索引
        </n-checkbox>
      </div>

      <n-alert v-if="mode === 'drop'" type="warning" :bordered="false" style="margin-bottom: 10px">
        「先删除目标集合」会 <strong>drop 掉目标库里的同名集合</strong>，其中已有数据将丢失。
      </n-alert>

      <!-- 集合选择 -->
      <div v-if="dirInfo" class="rs-row" style="align-items: flex-start">
        <label class="rs-label" style="margin-top: 4px">集合</label>
        <div style="flex: 1; min-width: 0">
          <div class="rs-list-head">
            <n-checkbox
              size="small"
              :checked="selected.length === allCollections.length && allCollections.length > 0"
              :indeterminate="selected.length > 0 && selected.length < allCollections.length"
              :disabled="running"
              @update:checked="toggleAll"
            >
              全选
            </n-checkbox>
            <span class="rs-count">已选 {{ selected.length }} / {{ allCollections.length }}</span>
          </div>
          <div class="rs-list">
            <n-scrollbar style="max-height: 180px">
              <div class="rs-list-inner">
                <div v-for="c in allCollections" :key="c.name" class="rs-item">
                  <n-checkbox
                    size="small"
                    :checked="selected.includes(c.name)"
                    :disabled="running"
                    @update:checked="toggleColl(c.name)"
                  >
                    <span class="rs-item-name">{{ c.name }}</span>
                  </n-checkbox>
                  <span class="rs-meta">
                    {{ formatBytes(c.size) }}
                    <template v-if="c.collType !== 'collection'"> · {{ c.collType }}</template>
                    <template v-if="c.indexCount > 1"> · {{ c.indexCount }} 索引</template>
                    <template v-if="!c.hasMetadata"> · 无 metadata</template>
                  </span>
                </div>
              </div>
            </n-scrollbar>
          </div>
        </div>
      </div>

      <div v-if="targetIsReadOnly" class="rs-status-line rs-error">只读连接，禁用恢复</div>

      <!-- 进度 / 结果 -->
      <div v-if="running || summary || errorMsg" class="rs-status">
        <n-progress
          v-if="running || summary"
          type="line"
          :percentage="summary ? 100 : progressPercent"
          :status="errorMsg ? 'error' : summary ? 'success' : 'default'"
          indicator-placement="inside"
          :height="20"
          style="margin-bottom: 6px"
        />
        <div v-if="running" class="rs-status-line">
          {{ progress?.phase ?? "准备中..." }}
          <template v-if="progress?.collection">
            <strong>{{ progress.collection }}</strong>
            ({{ progress.collIndex }}/{{ progress.collTotal }})
            —— {{ progress.docsDone.toLocaleString() }} 条
          </template>
        </div>
        <div v-else-if="errorMsg" class="rs-status-line rs-error">{{ errorMsg }}</div>
        <div v-else-if="summary" class="rs-status-line rs-success">
          恢复完成：{{ summary.collections }} 个集合、{{ summary.documents.toLocaleString() }} 条文档、{{ summary.indexes }} 个索引
          <div v-for="w in summary.warnings" :key="w" class="rs-warn">{{ w }}</div>
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
            :loading="running || scanning"
            :title="targetIsReadOnly ? '只读连接, 禁用恢复' : ''"
            @click="handleRun"
          >
            <template #icon><n-icon :size="14"><RestoreIcon /></n-icon></template>
            开始恢复
          </n-button>
        </n-space>
      </template>
    </n-card>
  </n-modal>
</template>

<style scoped>
.rs-row { display: flex; align-items: center; margin-bottom: 10px; }
.rs-label { width: 68px; font-size: 13px; font-weight: 500; color: #333; flex-shrink: 0; }
.rs-hint { font-size: 12px; color: #888; margin: -4px 0 10px 68px; line-height: 1.6; word-break: break-all; }
.rs-list-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 4px; }
.rs-count { font-size: 12px; color: #888; }
.rs-list { border: 1px solid #e0e0e0; border-radius: 4px; }
.rs-list-inner { padding: 6px 10px; }
.rs-item { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 2px 0; font-size: 13px; min-width: 0; }
.rs-item :deep(.n-checkbox) { display: flex; align-items: center; min-width: 0; }
.rs-item :deep(.n-checkbox__label) { overflow: hidden; }
.rs-item-name { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.rs-meta { font-size: 11px; color: #999; flex-shrink: 0; }
.rs-status { margin-top: 8px; padding: 8px; border-top: 1px solid #eee; }
.rs-status-line { font-size: 12px; color: #3875d7; text-align: center; }
.rs-error { color: #d03050; text-align: left; word-break: break-all; }
.rs-success { color: #18a058; }
.rs-warn { color: #e0803a; font-size: 11px; margin-top: 2px; word-break: break-all; }
</style>

<script setup lang="ts">
import { ref, watch, computed, h, type VNodeChild } from "vue";
import { NTree, NIcon, NEmpty, NAlert, NButton, NPopover, NDropdown } from "naive-ui";
import {
  Server as DbIcon,
  Layers as CollectionIcon,
  Refresh as RefreshIcon,
} from "@vicons/ionicons5";
import { useConnectionStore } from "@/stores/connection";
import { useDatabaseStore } from "@/stores/database";
import { useEditorStore } from "@/stores/editor";
import * as collApi from "@/api/collectionMgmt";
import * as dbApi from "@/api/database";
import type { CollectionStats } from "@/types/document";
import type { TreeOption } from "naive-ui";

const connStore = useConnectionStore();
const dbStore = useDatabaseStore();
const editorStore = useEditorStore();

const props = defineProps<{ connectionId: string }>();

const expandedKeys = ref<string[]>([]);
const refreshing = ref(false);

// 悬停统计缓存
const statsCache = ref<Record<string, CollectionStats>>({});
const loadingStatsKey = ref<string | null>(null);

// 右键菜单
const showCtxMenu = ref(false);
const ctxMenuX = ref(0);
const ctxMenuY = ref(0);
const ctxNodeKey = ref("");

watch(
  () => props.connectionId,
  async (id) => {
    if (id && connStore.isActive(id)) {
      await dbStore.fetchDatabases(id);
    }
  },
  { immediate: true },
);

interface TreeNode extends TreeOption {
  key: string;
  label: string;
  isLeaf: boolean;
  children?: TreeNode[];
}

function formatCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function formatSize(bytes: number): string {
  if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(2)}MB`;
  if (bytes >= 1_024) return `${(bytes / 1_024).toFixed(1)}KB`;
  return `${bytes}B`;
}

const treeData = computed<TreeNode[]>(() => {
  const dbs = dbStore.getDatabases(props.connectionId);
  return dbs.map((db) => ({
    key: `db:${db.name}`,
    label: `${db.name} (${db.collectionCount})`,
    isLeaf: false,
    prefix: () => h(NIcon, { size: 15, color: "#e8a838" }, { default: () => h(DbIcon) }),
    children: buildCollectionNodes(db.name),
  }));
});

function buildCollectionNodes(database: string): TreeNode[] {
  const colls = dbStore.getCollections(props.connectionId, database);
  return colls.map((c) => ({
    key: `coll:${database}.${c.name}`,
    label: `${c.name} (${formatCount(c.count)})`,
    isLeaf: true,
    prefix: () => h(NIcon, { size: 14, color: "#63e2b7" }, { default: () => h(CollectionIcon) }),
  }));
}

async function handleExpandUpdate(keys: string[]) {
  const newKeys = keys.filter((k) => !expandedKeys.value.includes(k));
  expandedKeys.value = keys;
  for (const key of newKeys) {
    if (key.startsWith("db:")) {
      await dbStore.fetchCollections(props.connectionId, key.slice(3));
    }
  }
}

async function handleRefresh() {
  refreshing.value = true;
  statsCache.value = {};

  // 先拉取数据库列表
  const dbs = await dbApi.listDatabases(props.connectionId);

  // 并行拉取所有已展开数据库的集合
  const expandedDbs = expandedKeys.value.filter((k) => k.startsWith("db:")).map((k) => k.slice(3));
  const collResults = await Promise.all(
    expandedDbs.map(async (dbName) => ({
      dbName,
      colls: await dbApi.listCollections(props.connectionId, dbName),
    })),
  );

  // 全部数据就绪后一次性更新 store，避免中间状态导致树折叠
  dbStore.databases[props.connectionId] = dbs;
  for (const { dbName, colls } of collResults) {
    dbStore.collections[`${props.connectionId}:${dbName}`] = colls;
  }

  refreshing.value = false;
}

async function refreshDatabase(dbName: string) {
  const colls = await dbApi.listCollections(props.connectionId, dbName);
  dbStore.collections[`${props.connectionId}:${dbName}`] = colls;
  // 同时刷新数据库信息
  const dbs = await dbApi.listDatabases(props.connectionId);
  dbStore.databases[props.connectionId] = dbs;
}

// 右键菜单
const ctxMenuOptions = computed(() => {
  const key = ctxNodeKey.value;
  if (key.startsWith("db:")) {
    return [
      { label: "刷新", key: "refresh" },
    ];
  }
  if (key.startsWith("coll:")) {
    return [
      { label: "查询", key: "query" },
      { label: "刷新", key: "refresh-parent" },
    ];
  }
  return [];
});

function handleCtxMenu(e: MouseEvent, nodeKey: string) {
  e.preventDefault();
  ctxNodeKey.value = nodeKey;
  ctxMenuX.value = e.clientX;
  ctxMenuY.value = e.clientY;
  showCtxMenu.value = true;
}

async function handleCtxSelect(key: string) {
  showCtxMenu.value = false;
  const nodeKey = ctxNodeKey.value;

  if (key === "refresh" && nodeKey.startsWith("db:")) {
    const dbName = nodeKey.slice(3);
    await refreshDatabase(dbName);
  }
  if (key === "refresh-parent" && nodeKey.startsWith("coll:")) {
    const dbName = nodeKey.slice(5).split(".", 2)[0];
    await refreshDatabase(dbName);
  }
  if (key === "query" && nodeKey.startsWith("coll:")) {
    const [database, collection] = nodeKey.slice(5).split(".", 2);
    const tabId = editorStore.createTab(props.connectionId, database, collection);
    editorStore.setContent(tabId, `db.${collection}.find({}).limit(20)`);
  }
}

async function loadCollStats(db: string, coll: string) {
  const cacheKey = `${db}.${coll}`;
  if (statsCache.value[cacheKey]) return;
  loadingStatsKey.value = cacheKey;
  try {
    const stats = await collApi.getCollectionStats(props.connectionId, db, coll);
    statsCache.value = { ...statsCache.value, [cacheKey]: stats };
  } catch {
    // ignore
  } finally {
    loadingStatsKey.value = null;
  }
}

function renderLabel({ option }: { option: TreeOption }): VNodeChild {
  const node = option as TreeNode;
  const key = typeof node.key === "string" ? node.key : "";

  if (!key.startsWith("coll:")) {
    return h("span", {}, node.label as string);
  }

  const [db, coll] = key.slice(5).split(".", 2);
  const cacheKey = `${db}.${coll}`;

  return h(
    NPopover,
    {
      trigger: "hover",
      placement: "right",
      delay: 300,
      onUpdateShow: (show: boolean) => {
        if (show) loadCollStats(db, coll);
      },
    },
    {
      trigger: () => h("span", { style: "cursor:pointer" }, node.label as string),
      default: () => {
        const s = statsCache.value[cacheKey];
        if (!s) {
          return h("span", { style: "color:#999;font-size:12px" }, "加载中...");
        }
        const rows: [string, string][] = [
          ["name", coll],
          ["nameSpace", `${db}.${coll}`],
          ["count", String(s.documentCount)],
          ["size", `${s.totalSize.toLocaleString()} (${formatSize(s.totalSize)})`],
          ["avgObjectSize", String(s.avgDocumentSize)],
          ["indexCount", String(s.indexCount)],
          ["totalIndexSize", `${s.totalIndexSize.toLocaleString()} (${formatSize(s.totalIndexSize)})`],
        ];
        return h(
          "table",
          { style: "font-size:12px;min-width:250px;border-collapse:collapse" },
          rows.map(([k, v]) =>
            h("tr", [
              h("td", { style: "padding:2px 8px 2px 0;font-weight:600;white-space:nowrap;color:#666" }, k),
              h("td", { style: "padding:2px 0" }, v),
            ]),
          ),
        );
      },
    },
  );
}

function handleNodeDblClick(option: TreeNode) {
  const key = option.key;
  if (typeof key === "string" && key.startsWith("coll:")) {
    const [database, collection] = key.slice(5).split(".", 2);
    const tabId = editorStore.createTab(props.connectionId, database, collection);
    editorStore.setContent(tabId, `db.${collection}.find({}).limit(20)`);
  }
}
</script>

<template>
  <div class="database-tree">
    <div class="db-tree-header">
      <span>数据库</span>
      <n-button size="tiny" quaternary :loading="refreshing" @click="handleRefresh">
        <template #icon><n-icon :size="14"><RefreshIcon /></n-icon></template>
      </n-button>
    </div>

    <n-alert v-if="dbStore.error" type="error" style="margin: 4px 8px" :bordered="false">
      {{ dbStore.error }}
    </n-alert>

    <n-empty
      v-if="!dbStore.loading && !refreshing && treeData.length === 0 && !dbStore.error"
      description="无数据库"
      style="padding: 16px"
      size="small"
    />

    <n-tree
      v-show="treeData.length > 0"
      :data="treeData"
      :expanded-keys="expandedKeys"
      block-line
      selectable
      :render-label="renderLabel"
      @update:expanded-keys="handleExpandUpdate"
      :node-props="({ option }: any) => {
        const node = option as TreeNode;
        const key = typeof node.key === 'string' ? node.key : '';
        return {
          onDblclick: () => handleNodeDblClick(node),
          onContextmenu: (e: MouseEvent) => handleCtxMenu(e, key),
        };
      }"
    />

    <n-dropdown
      trigger="manual"
      placement="bottom-start"
      :show="showCtxMenu"
      :options="ctxMenuOptions"
      :x="ctxMenuX"
      :y="ctxMenuY"
      @select="handleCtxSelect"
      @clickoutside="showCtxMenu = false"
    />
  </div>
</template>

<style scoped>
.database-tree {
  border-top: 1px solid var(--n-border-color);
}
.db-tree-header {
  padding: 4px 12px;
  font-size: 12px;
  font-weight: 600;
  color: #999;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  display: flex;
  justify-content: space-between;
  align-items: center;
}
</style>

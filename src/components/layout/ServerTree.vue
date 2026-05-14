<script setup lang="ts">
import { ref, computed, h, onMounted, type VNodeChild } from "vue";
import { NTree, NIcon, NButton, NDropdown, NPopover, NEmpty, useMessage, useDialog } from "naive-ui";
import {
  Add as AddIcon,
  CloudDone as ConnectedIcon,
  CloudOffline as DisconnectedIcon,
  Server as DbIcon,
  Layers as CollectionIcon,
  Document as SchemaIcon,
  Shield as ValidatorIcon,
  List as IndexIcon,
  Key as IndexKeyIcon,
  People as UsersIcon,
  Person as UserIcon,
  LockClosed as RoleIcon,
} from "@vicons/ionicons5";
import { useConnectionStore } from "@/stores/connection";
import { useDatabaseStore } from "@/stores/database";
import { useEditorStore } from "@/stores/editor";
import { listUsers } from "@/api/server";
import type { UserInfo } from "@/types/server";
import * as collApi from "@/api/collectionMgmt";
import * as dbApi from "@/api/database";
import AddIndexDialog from "@/components/data/AddIndexDialog.vue";
import type { ConnectionConfig } from "@/types/connection";
import type { CollectionStats, IndexInfo } from "@/types/document";
import type { TreeOption } from "naive-ui";

const emit = defineEmits<{
  createConnection: [];
  editConnection: [config: ConnectionConfig];
  connectServer: [config: ConnectionConfig];
  disconnectServer: [id: string];
  deleteConnection: [id: string];
  importColl: [connId: string, database: string, collection: string];
  exportColl: [connId: string, database: string, collection: string];
}>();

const connStore = useConnectionStore();
const dbStore = useDatabaseStore();
const editorStore = useEditorStore();
const message = useMessage();
const dlg = useDialog();

const expandedKeys = ref<string[]>([]);
const loadingKeys = ref<Set<string>>(new Set());
const connectingIds = ref<Set<string>>(new Set());
const statsCache = ref<Record<string, CollectionStats>>({});

// 右键菜单
const showCtxMenu = ref(false);
const ctxMenuX = ref(0);
const ctxMenuY = ref(0);
const ctxNodeKey = ref("");
const ctxConnConfig = ref<ConnectionConfig | null>(null);

// Add Index 弹窗
const addIndexShow = ref(false);
const addIndexCtx = ref<{ connId: string; database: string; collection: string }>({
  connId: "",
  database: "",
  collection: "",
});
const addIndexEditing = ref<IndexInfo | null>(null);

function openAddIndex(connId: string, database: string, collection: string, editing: IndexInfo | null = null) {
  addIndexCtx.value = { connId, database, collection };
  addIndexEditing.value = editing;
  addIndexShow.value = true;
}

onMounted(() => {
  connStore.fetchConnections();
});

interface TreeNode extends TreeOption {
  key: string;
  label: string;
  isLeaf?: boolean;
  children?: TreeNode[];
  connConfig?: ConnectionConfig;
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

function loadingNode(parentKey: string): TreeNode {
  return {
    key: `${parentKey}:__loading`,
    label: "加载中...",
    isLeaf: true,
    disabled: true,
    prefix: () => h("span", { class: "loading-dot" }),
  };
}

// 统一树：连接 > 数据库 > 集合
const treeData = computed<TreeNode[]>(() => {
  const nodes: TreeNode[] = [];

  for (const conn of connStore.connections) {
    const active = connStore.isActive(conn.id);
    const connecting = connectingIds.value.has(conn.id);
    const connNode: TreeNode = {
      key: `conn:${conn.id}`,
      label: conn.name,
      isLeaf: !active && !connecting,
      connConfig: conn,
      prefix: () =>
        h(NIcon, { size: 16, color: connecting ? "#3875d7" : (active ? "#18a058" : "#999") }, {
          default: () => h(active ? ConnectedIcon : DisconnectedIcon),
        }),
      suffix: connecting
        ? () => h("span", { style: "font-size:11px;color:#3875d7;margin-left:4px;display:inline-flex;align-items:center;gap:4px" }, [
            h("span", { class: "loading-dot" }),
            "连接中...",
          ])
        : active
          ? () => h("span", { style: "font-size:11px;margin-left:4px" }, [
              h("span", { style: "color:#18a058" }, "已连接"),
              ...(conn.readOnly ? [h("span", { style: "color:#e8a838;margin-left:4px" }, "只读")] : []),
            ])
          : undefined,
    };

    if (connecting && !active) {
      connNode.children = [loadingNode(`conn:${conn.id}`)];
    }

    if (active) {
      const dbs = dbStore.getDatabases(conn.id);
      if (dbs.length === 0 && loadingKeys.value.has(`conn:${conn.id}`)) {
        connNode.children = [loadingNode(`conn:${conn.id}`)];
      } else {
        const dbNodes: TreeNode[] = dbs.map((db) => {
          const dbKey = `db:${conn.id}:${db.name}`;
          // 用原始集合列表判断是否在首次加载（buildCollectionNodes 尾部会追加 users 节点，
          // 所以不能用它的返回长度来判断）
          const rawColls = dbStore.getCollections(conn.id, db.name);
          const isDbLoading = loadingKeys.value.has(dbKey);
          const children = isDbLoading && rawColls.length === 0
            ? [loadingNode(dbKey)]
            : buildCollectionNodes(conn.id, db.name);
          return {
            key: dbKey,
            label: `${db.name} (${db.collectionCount})`,
            isLeaf: false,
            prefix: () => h(NIcon, { size: 15, color: "#e8a838" }, { default: () => h(DbIcon) }),
            children,
          };
        });

        // 连接级 users 节点（全局用户列表）
        const usersKey = `users:${conn.id}`;
        const cachedUsers = usersCache.value[conn.id] || [];
        const usersNode: TreeNode = {
          key: usersKey,
          label: `users (${cachedUsers.length})`,
          isLeaf: false,
          prefix: () => h(NIcon, { size: 15, color: "#d03050" }, { default: () => h(UsersIcon) }),
          children: loadingKeys.value.has(usersKey) && cachedUsers.length === 0
            ? [loadingNode(usersKey)]
            : buildUserNodes(conn.id, "admin", conn.id),
        };

        connNode.children = [...dbNodes, usersNode];
      }
    }

    nodes.push(connNode);
  }

  return nodes;
});

// 索引缓存
const indexCache = ref<Record<string, { name: string; size: number }[]>>({});

// 用户缓存 (key: "connId" 全局用户, "connId:dbName" 库级用户)
const usersCache = ref<Record<string, UserInfo[]>>({});

function buildUserNodes(connectionId: string, _database: string, cacheKey: string): TreeNode[] {
  const cached = usersCache.value[cacheKey] || [];
  return cached.map((u) => ({
    key: `user:${connectionId}:${u.user}@${u.database}`,
    label: u.user,
    isLeaf: false,
    prefix: () => h(NIcon, { size: 13, color: "#666" }, { default: () => h(UserIcon) }),
    children: u.roles.map((r: { role: string; db: string }) => ({
      key: `role:${connectionId}:${u.user}:${r.role}@${r.db}`,
      label: `${r.role} (${r.db})`,
      isLeaf: true,
      prefix: () => h(NIcon, { size: 12, color: "#e8a838" }, { default: () => h(RoleIcon) }),
    })),
  }));
}

function buildCollectionNodes(connectionId: string, database: string): TreeNode[] {
  const colls = dbStore.getCollections(connectionId, database);
  const collNodes: TreeNode[] = colls.map((c) => {
    const collKey = `${connectionId}:${database}.${c.name}`;
    const cachedIndexes = indexCache.value[collKey] || [];
    const indexCount = cachedIndexes.length;

    return {
      key: `coll:${connectionId}:${database}.${c.name}`,
      label: `${c.name} (${formatCount(c.count)})`,
      isLeaf: false,
      prefix: () => h(NIcon, { size: 14, color: "#63e2b7" }, { default: () => h(CollectionIcon) }),
      children: [
        {
          key: `schema:${collKey}`,
          label: "schema",
          isLeaf: true,
          prefix: () => h(NIcon, { size: 13, color: "#999" }, { default: () => h(SchemaIcon) }),
        },
        {
          key: `validator:${collKey}`,
          label: "validator (empty)",
          isLeaf: true,
          prefix: () => h(NIcon, { size: 13, color: "#999" }, { default: () => h(ValidatorIcon) }),
        },
        {
          key: `indexes:${collKey}`,
          label: `indexes (${indexCount})`,
          isLeaf: false,
          prefix: () => h(NIcon, { size: 13, color: "#e8a838" }, { default: () => h(IndexIcon) }),
          children: cachedIndexes.map((idx) => ({
            // 用 | 分隔索引名, 防止 "."" 与 db/coll 名混淆
            key: `idx:${collKey}|${idx.name}`,
            label: `${idx.name} (${formatSize(idx.size)})`,
            isLeaf: true,
            prefix: () => h(NIcon, { size: 12, color: "#d19a66" }, { default: () => h(IndexKeyIcon) }),
          })),
        },
      ],
    };
  });

  // 库级 users 节点
  const dbUsersKey = `dbusers:${connectionId}:${database}`;
  const dbUsersCacheKey = `${connectionId}:${database}`;
  const dbCachedUsers = usersCache.value[dbUsersCacheKey] || [];
  const dbUsersNode: TreeNode = {
    key: dbUsersKey,
    label: "users",
    isLeaf: false,
    prefix: () => h(NIcon, { size: 13, color: "#d03050" }, { default: () => h(UsersIcon) }),
    children: loadingKeys.value.has(dbUsersKey) && dbCachedUsers.length === 0
      ? [loadingNode(dbUsersKey)]
      : buildUserNodes(connectionId, database, dbUsersCacheKey),
  };

  return [...collNodes, dbUsersNode];
}

async function handleExpandUpdate(keys: string[]) {
  const newKeys = keys.filter((k) => !expandedKeys.value.includes(k));
  expandedKeys.value = keys;

  for (const key of newKeys) {
    // 展开连接节点 → 加载数据库列表
    if (key.startsWith("conn:")) {
      const connId = key.slice(5);
      if (connStore.isActive(connId)) {
        loadingKeys.value.add(key);
        loadingKeys.value = new Set(loadingKeys.value);
        await dbStore.fetchDatabases(connId);
        loadingKeys.value.delete(key);
        loadingKeys.value = new Set(loadingKeys.value);
      }
    }
    // 展开数据库节点 → 并发加载集合列表 + 该库的用户列表
    if (key.startsWith("db:")) {
      const rest = key.slice(3);
      const colonIdx = rest.indexOf(":");
      const connId = rest.slice(0, colonIdx);
      const dbName = rest.slice(colonIdx + 1);
      const dbUsersKey = `dbusers:${connId}:${dbName}`;
      const usersCacheKey = `${connId}:${dbName}`;

      loadingKeys.value.add(key);
      // 用户列表没缓存时，提前打 users 的 loading 标记，展开 users 节点时能立刻看到转圈
      const needLoadUsers = !usersCache.value[usersCacheKey];
      if (needLoadUsers) loadingKeys.value.add(dbUsersKey);
      loadingKeys.value = new Set(loadingKeys.value);

      // users 后台并发拉取，不阻塞集合显示
      if (needLoadUsers) {
        listUsers(connId, dbName)
          .then((users) => {
            usersCache.value = { ...usersCache.value, [usersCacheKey]: users };
          })
          .catch(() => { /* ignore, 权限不足时静默失败 */ })
          .finally(() => {
            loadingKeys.value.delete(dbUsersKey);
            loadingKeys.value = new Set(loadingKeys.value);
          });
      }

      try {
        await dbStore.fetchCollections(connId, dbName);
      } finally {
        loadingKeys.value.delete(key);
        loadingKeys.value = new Set(loadingKeys.value);
      }
    }
    // 展开连接级 users 节点 → 加载全局用户列表
    if (key.startsWith("users:")) {
      const connId = key.slice(6);
      loadingKeys.value.add(key);
      loadingKeys.value = new Set(loadingKeys.value);
      try {
        const users = await listUsers(connId, "admin");
        usersCache.value = { ...usersCache.value, [connId]: users };
      } catch { /* ignore */ }
      loadingKeys.value.delete(key);
      loadingKeys.value = new Set(loadingKeys.value);
    }
    // 展开库级 users 节点 → 加载该库用户列表
    if (key.startsWith("dbusers:")) {
      const rest = key.slice(8); // connId:dbName
      const colonIdx = rest.indexOf(":");
      const connId = rest.slice(0, colonIdx);
      const dbName = rest.slice(colonIdx + 1);
      const cacheKey = `${connId}:${dbName}`;
      loadingKeys.value.add(key);
      loadingKeys.value = new Set(loadingKeys.value);
      try {
        const users = await listUsers(connId, dbName);
        usersCache.value = { ...usersCache.value, [cacheKey]: users };
      } catch { /* ignore */ }
      loadingKeys.value.delete(key);
      loadingKeys.value = new Set(loadingKeys.value);
    }
    // 展开集合节点 → 加载索引
    if (key.startsWith("coll:")) {
      const rest = key.slice(5);
      const colonIdx = rest.indexOf(":");
      const connId = rest.slice(0, colonIdx);
      const afterConn = rest.slice(colonIdx + 1);
      const dotIdx = afterConn.indexOf(".");
      const dbName = afterConn.slice(0, dotIdx);
      const collName = afterConn.slice(dotIdx + 1);
      loadingKeys.value.add(key);
      loadingKeys.value = new Set(loadingKeys.value);
      await loadIndexes(connId, dbName, collName);
      loadingKeys.value.delete(key);
      loadingKeys.value = new Set(loadingKeys.value);
    }
  }
}

async function loadIndexes(connId: string, dbName: string, collName: string) {
  const cacheKey = `${connId}:${dbName}.${collName}`;
  if (indexCache.value[cacheKey]) return;
  try {
    const indexes = await collApi.listIndexes(connId, dbName, collName);
    // 获取每个索引的大小（从 collStats）
    const stats = await collApi.getCollectionStats(connId, dbName, collName);
    const avgIdxSize = stats.indexCount > 0 ? Math.floor(stats.totalIndexSize / stats.indexCount) : 0;
    indexCache.value = {
      ...indexCache.value,
      [cacheKey]: indexes.map((idx) => ({
        name: idx.name,
        size: avgIdxSize, // 近似每个索引大小
      })),
    };
  } catch {
    // ignore
  }
}

/** 给 NDropdown 的菜单项做"右侧灰色快捷键"渲染 */
function menuRow(label: string, shortcut?: string): () => VNodeChild {
  return () => h(
    "div",
    {
      style:
        "display:flex;justify-content:space-between;align-items:center;gap:24px;min-width:220px",
    },
    [
      h("span", null, label),
      shortcut ? h("span", { style: "color:#999;font-size:12px" }, shortcut) : null,
    ],
  );
}

function parseIndexesKey(key: string): { connId: string; dbName: string; collName: string } {
  // key 格式: indexes:connId:db.coll
  const rest = key.slice("indexes:".length);
  const colonIdx = rest.indexOf(":");
  const connId = rest.slice(0, colonIdx);
  const afterConn = rest.slice(colonIdx + 1);
  const dotIdx = afterConn.indexOf(".");
  return { connId, dbName: afterConn.slice(0, dotIdx), collName: afterConn.slice(dotIdx + 1) };
}

/**
 * 渲染 "查看索引" 时贴进编辑器的 mongosh 等效脚本.
 * Tab 上挂了 executor = { kind: "indexInfo", ... }, Run 时走后端 get_index_info 命令 (等效结果);
 * 同时 tab.skipLint = true 让编辑器不对这段非 db.xxx 语法报红.
 */
function buildIndexInfoScript(collection: string, indexName: string): string {
  return `function getCollectionIndexInfo(collection, indexName) {
    const dbVersion = parseFloat(db.version());
    const indexInfo = db.getCollection(collection).getIndexes().find(it => it.name === indexName);
    const stats = (() => {
        if (dbVersion >= 3.0) {
            try{
                return db.getCollection(collection).stats({ indexDetails: true, indexDetailsName: indexName });
            } catch (error){
                console.error(error);
            }
        }
    })();

    const indexDetails=(()=>{
        if (_.isEmpty((stats))) return;

        if (stats.indexDetails) {
            return stats.indexDetails[indexName];
        }

        if (stats.shards) {
            return _.transform(stats.shards, function(result, value, key) {
                result[key] = {
                    name: indexName,
                    indexSize: _.get(value, "indexSizes." + indexName),
                    ..._.get(value, "indexDetails." + indexName)
                }
            }, {});
        }
    })();

    const indexStats = (() => {
        if (dbVersion >= 3.2) {
            try{
                return db.getCollection(collection).aggregate([{ $indexStats: {} }]).toArray().filter(it => it.name === indexName)
            } catch (error){
                console.error(error)
            }
        }
    })();

    return ({ ...indexInfo, indexSize: stats && stats.indexSizes[indexName],  "usage stats": indexStats, "index details": indexDetails })
}

getCollectionIndexInfo(${JSON.stringify(collection)}, ${JSON.stringify(indexName)});`;
}

/** "查看索引" 贴进编辑器的 mongosh 等效脚本 (Run 走后端 get_collection_indexes) */
function buildCollectionIndexesScript(collection: string): string {
  return `function getCollectionIndexes(col){
    const indexStats = (() => {
        const dbVersion = parseFloat(db.version());
        if (dbVersion >= 3.2) {
            try{
                return db.getCollection(col).aggregate([{ $indexStats: {} }]).toArray()
            }catch(err){
                console.error(err)
                return [];
            }
        }
    })();

    const indexSizes=db.getCollection(col).stats().indexSizes;

    const indexes=db.getCollection(col).getIndexes();

    return indexes.map(it=>{
        const usageStats=(()=>{
            const stats=(indexStats || []).filter(stat=>stat.name===it.name);
            if (_.isEmpty(stats)) return {"usage stats": "not available"};

            const formatAccesses=(it)=>\`\${it.ops} since \${it.since.toLocaleString()}\`
            if (stats.length>1){
                return {
                    "usage stats":stats.reduce((acc, cur, i)=> {
                        acc[i]={
                            host:cur.host,
                            accesses:formatAccesses(cur.accesses)
                        }

                        return acc;
                    }, {})
                }
            }else{
                return {
                    ...stats[0],
                    accesses: formatAccesses(stats[0].accesses),
                }
            }
        })();

        const size=indexSizes[it.name];
        const type=(_.find(_.values(it.key), v=>_.isString(v)) || "regular").toUpperCase();
        const info={
            ...it,
            size,
            type,
            ...usageStats,
        }

        const commonFields=["name","key","type","size", "ns","accesses","usage stats"];
        if (!info.ns){
            info.ns=db.getName()+"."+col;
        }

        return _.omitBy({
            ..._.pick(info,commonFields),
            ...{properties: _.omit(info, [...commonFields,"v","host"])},
            ..._.pick(info,"v","host")
        },_.isEmpty)
    })
}

getCollectionIndexes(${JSON.stringify(collection)});`;
}

function parseIdxKey(key: string): { connId: string; dbName: string; collName: string; idxName: string } {
  // key 格式: idx:connId:db.coll|idxName
  const rest = key.slice("idx:".length);
  const pipeIdx = rest.lastIndexOf("|");
  const idxName = rest.slice(pipeIdx + 1);
  const collKeyPart = rest.slice(0, pipeIdx); // connId:db.coll
  const colonIdx = collKeyPart.indexOf(":");
  const connId = collKeyPart.slice(0, colonIdx);
  const afterConn = collKeyPart.slice(colonIdx + 1);
  const dotIdx = afterConn.indexOf(".");
  return {
    connId,
    dbName: afterConn.slice(0, dotIdx),
    collName: afterConn.slice(dotIdx + 1),
    idxName,
  };
}

// 右键菜单
const ctxMenuOptions = computed(() => {
  const key = ctxNodeKey.value;
  if (key.startsWith("conn:")) {
    const connId = key.slice(5);
    const active = connStore.isActive(connId);
    return [
      ...(active
        ? [{ label: "断开连接", key: "disconnect" }, { label: "刷新", key: "refresh-conn" }]
        : [{ label: "连接", key: "connect" }]),
      { type: "divider" as const, key: "d1" },
      { label: "编辑", key: "edit" },
      { label: "删除", key: "delete-conn" },
    ];
  }
  if (key.startsWith("db:")) {
    return [
      { label: "新建查询", key: "new-query-db" },
      { label: "创建集合...", key: "create-coll" },
      { type: "divider" as const, key: "d2" },
      { label: "刷新", key: "refresh-db" },
      { type: "divider" as const, key: "d3" },
      { label: "删除数据库", key: "drop-db" },
    ];
  }
  if (key.startsWith("coll:")) {
    return [
      { label: "查询", key: "query-coll" },
      { type: "divider" as const, key: "d4a" },
      { label: "导入数据...", key: "import-coll" },
      { label: "导出数据...", key: "export-coll" },
      { type: "divider" as const, key: "d4b" },
      { label: "刷新", key: "refresh-coll-parent" },
      { type: "divider" as const, key: "d4" },
      { label: "删除集合", key: "drop-coll" },
    ];
  }
  if (key.startsWith("idx:")) {
    return [
      { label: menuRow("查看索引..."), key: "show-index" },
      { label: menuRow("修改索引..."), key: "update-index" },
      { label: menuRow("删除索引...", "Del"), key: "drop-this-index" },
      { type: "divider" as const, key: "dx1" },
      { label: menuRow("新建索引...", "Alt+N"), key: "add-index-from-idx" },
      { type: "divider" as const, key: "dx2" },
      { label: menuRow("复制名称", "Alt+Ctrl+C"), key: "copy-idx-name" },
    ];
  }
  if (key.startsWith("indexes:")) {
    return [
      { label: menuRow("查看索引", "Enter"), key: "view-indexes" },
      { label: menuRow("查看索引统计 ($indexStats)"), key: "view-index-stats" },
      { type: "divider" as const, key: "di1" },
      { label: menuRow("新建索引...", "Alt+N"), key: "add-index" },
      { type: "divider" as const, key: "di2" },
      { label: menuRow("重建索引..."), key: "rebuild-indexes" },
      { label: menuRow("删除索引..."), key: "drop-indexes" },
      { type: "divider" as const, key: "di3" },
      { label: menuRow("复制名称", "Alt+Ctrl+C"), key: "copy-coll-name" },
      { type: "divider" as const, key: "di4" },
      { label: menuRow("设置注释..."), key: "set-comment" },
      { label: menuRow("添加到收藏"), key: "add-to-favorites" },
      { type: "divider" as const, key: "di5" },
      { label: menuRow("显示节点定位器", "Ctrl+F"), key: "show-locator" },
      { label: menuRow("刷新", "Ctrl+R"), key: "refresh-indexes" },
    ];
  }
  if (key.startsWith("users:")) {
    const defaultDb = editorStore.activeTab?.database || "admin";
    return [
      { label: "查看用户列表", key: "view-users" },
      {
        label: "添加用户...",
        key: "add-user-menu",
        children: [
          { label: `只读本库 ("${defaultDb}")`, key: `add-user-read:${defaultDb}` },
          { label: `读写本库 ("${defaultDb}")`, key: `add-user-readWrite:${defaultDb}` },
          { type: "divider" as const, key: "au-d1" },
          { label: `数据库管理员 ("${defaultDb}")`, key: `add-user-dbAdmin:${defaultDb}` },
          { label: `数据库所有者 ("${defaultDb}")`, key: `add-user-dbOwner:${defaultDb}` },
          { label: `用户管理员 ("${defaultDb}")`, key: `add-user-userAdmin:${defaultDb}` },
          { type: "divider" as const, key: "au-d2" },
          { label: "只读所有库", key: "add-user-readAnyDatabase:admin" },
          { label: "读写所有库", key: "add-user-readWriteAnyDatabase:admin" },
          { label: "用户管理所有库", key: "add-user-userAdminAnyDatabase:admin" },
          { label: "数据库管理所有库", key: "add-user-dbAdminAnyDatabase:admin" },
          { type: "divider" as const, key: "au-d3" },
          { label: "超级管理员 (root)", key: "add-user-root:admin" },
          { type: "divider" as const, key: "au-d4" },
          { label: "自定义添加...", key: "add-user-custom" },
        ],
      },
      { type: "divider" as const, key: "d5" },
      { label: "查看角色列表", key: "view-roles" },
      { type: "divider" as const, key: "d5b" },
      { label: "复制名称", key: "copy-name" },
      { type: "divider" as const, key: "d6" },
      { label: "刷新", key: "refresh-users" },
    ];
  }
  if (key.startsWith("user:")) {
    return [
      { label: "查看用户", key: "view-user" },
      { type: "divider" as const, key: "d7" },
      { label: "复制名称", key: "copy-name" },
      { type: "divider" as const, key: "d8" },
      { label: "删除用户", key: "drop-user" },
    ];
  }
  if (key.startsWith("role:")) {
    return [
      { label: "查看角色", key: "view-role" },
      { type: "divider" as const, key: "d9" },
      { label: "复制名称", key: "copy-name" },
    ];
  }
  if (key.startsWith("dbusers:")) {
    const rest = key.slice(8);
    const colonIdx = rest.indexOf(":");
    const dbName = rest.slice(colonIdx + 1);
    return [
      { label: "查看用户列表", key: "view-db-users" },
      {
        label: "添加用户...",
        key: "add-dbuser-menu",
        children: [
          { label: `只读本库 ("${dbName}")`, key: `add-dbuser-read:${dbName}` },
          { label: `读写本库 ("${dbName}")`, key: `add-dbuser-readWrite:${dbName}` },
          { type: "divider" as const, key: "adu-d1" },
          { label: `数据库管理员 ("${dbName}")`, key: `add-dbuser-dbAdmin:${dbName}` },
          { label: `数据库所有者 ("${dbName}")`, key: `add-dbuser-dbOwner:${dbName}` },
          { label: `用户管理员 ("${dbName}")`, key: `add-dbuser-userAdmin:${dbName}` },
          { type: "divider" as const, key: "adu-d2" },
          { label: "只读所有库", key: `add-dbuser-readAnyDatabase:admin` },
          { label: "读写所有库", key: `add-dbuser-readWriteAnyDatabase:admin` },
          { label: "用户管理所有库", key: `add-dbuser-userAdminAnyDatabase:admin` },
          { label: "数据库管理所有库", key: `add-dbuser-dbAdminAnyDatabase:admin` },
          { type: "divider" as const, key: "adu-d3" },
          { label: "超级管理员 (root)", key: `add-dbuser-root:admin` },
          { type: "divider" as const, key: "adu-d4" },
          { label: "自定义添加...", key: `add-dbuser-custom:${dbName}` },
        ],
      },
      { type: "divider" as const, key: "d10" },
      { label: "查看角色列表", key: "view-db-roles" },
      { type: "divider" as const, key: "d10b" },
      { label: "复制名称", key: "copy-name" },
      { type: "divider" as const, key: "d10c" },
      { label: "刷新", key: "refresh-db-users" },
    ];
  }
  return [];
});

function handleCtxMenu(e: MouseEvent, node: TreeNode) {
  e.preventDefault();
  ctxNodeKey.value = typeof node.key === "string" ? node.key : "";
  ctxConnConfig.value = node.connConfig ?? null;
  ctxMenuX.value = e.clientX;
  ctxMenuY.value = e.clientY;
  showCtxMenu.value = true;
}

/** 集合名含 . 时用 getCollection()，否则用 db.xxx */
function collRef(name: string): string {
  return name.includes(".") ? `db.getCollection("${name}")` : `db.${name}`;
}

function buildDefaultQuery(collName: string): string {
  return `${collRef(collName)}.find({})\n  .projection({})\n  .sort({_id:1})\n  .limit(100)`;
}

function parseDbKey(key: string): { connId: string; dbName: string } {
  const rest = key.slice(3);
  const idx = rest.indexOf(":");
  return { connId: rest.slice(0, idx), dbName: rest.slice(idx + 1) };
}

function parseCollKey(key: string): { connId: string; dbName: string; collName: string } {
  const rest = key.slice(5); // connId:dbName.collName
  const colonIdx = rest.indexOf(":");
  const connId = rest.slice(0, colonIdx);
  const afterConn = rest.slice(colonIdx + 1);
  const dotIdx = afterConn.indexOf(".");
  return { connId, dbName: afterConn.slice(0, dotIdx), collName: afterConn.slice(dotIdx + 1) };
}

async function handleIndexCreated() {
  const { connId, database, collection } = addIndexCtx.value;
  if (!connId) return;
  delete indexCache.value[`${connId}:${database}.${collection}`];
  await loadIndexes(connId, database, collection);
}

async function handleCtxSelect(action: string) {
  showCtxMenu.value = false;
  const nodeKey = ctxNodeKey.value;

  if (action === "connect" && ctxConnConfig.value) {
    emit("connectServer", ctxConnConfig.value);
  }
  if (action === "disconnect" && nodeKey.startsWith("conn:")) {
    emit("disconnectServer", nodeKey.slice(5));
  }
  if (action === "edit" && ctxConnConfig.value) {
    emit("editConnection", ctxConnConfig.value);
  }
  if (action === "delete-conn" && nodeKey.startsWith("conn:")) {
    emit("deleteConnection", nodeKey.slice(5));
  }
  if (action === "refresh-conn" && nodeKey.startsWith("conn:")) {
    const connId = nodeKey.slice(5);
    await refreshConnection(connId);
  }
  if (action === "refresh-db" && nodeKey.startsWith("db:")) {
    const { connId, dbName } = parseDbKey(nodeKey);
    await refreshDb(connId, dbName);
  }
  if (action === "refresh-coll-parent" && nodeKey.startsWith("coll:")) {
    const { connId, dbName } = parseCollKey(nodeKey);
    await refreshDb(connId, dbName);
  }
  if (action === "new-query-db" && nodeKey.startsWith("db:")) {
    const { connId, dbName } = parseDbKey(nodeKey);
    editorStore.createTab(connId, dbName, dbName);
  }
  if (action === "query-coll" && nodeKey.startsWith("coll:")) {
    const { connId, dbName, collName } = parseCollKey(nodeKey);
    const tabId = editorStore.createTab(connId, dbName, collName);
    const defaultQuery = buildDefaultQuery(collName);
    editorStore.setContent(tabId, defaultQuery);
    editorStore.executeQuery(tabId);
  }
  if (action === "import-coll" && nodeKey.startsWith("coll:")) {
    const { connId, dbName, collName } = parseCollKey(nodeKey);
    emit("importColl", connId, dbName, collName);
  }
  if (action === "export-coll" && nodeKey.startsWith("coll:")) {
    const { connId, dbName, collName } = parseCollKey(nodeKey);
    emit("exportColl", connId, dbName, collName);
  }

  // ---- indexes 节点 ----
  if (nodeKey.startsWith("indexes:")) {
    const { connId, dbName, collName } = parseIndexesKey(nodeKey);
    const ref = collRef(collName);

    if (action === "view-indexes") {
      // mongosh 等效脚本 getCollectionIndexes(col), 后端 get_collection_indexes 命令做等效计算
      const script = buildCollectionIndexesScript(collName);
      const tabId = editorStore.createTab(connId, dbName, collName);
      editorStore.setTabExecutor(tabId, { kind: "collectionIndexes", collection: collName });
      editorStore.setTabSkipLint(tabId, true);
      editorStore.setContent(tabId, script);
      editorStore.executeQuery(tabId);
    }
    if (action === "view-index-stats") {
      const tabId = editorStore.createTab(connId, dbName, collName);
      editorStore.setContent(tabId, `${ref}.aggregate([{ $indexStats: {} }])`);
      editorStore.executeQuery(tabId);
    }
    if (action === "add-index") {
      openAddIndex(connId, dbName, collName);
    }
    if (action === "rebuild-indexes") {
      dlg.warning({
        title: "重建索引",
        content: `确定重建 "${collName}" 的所有索引吗? 期间集合会持有写锁.`,
        positiveText: "重建",
        negativeText: "取消",
        onPositiveClick: async () => {
          try {
            await collApi.reIndex(connId, dbName, collName);
            message.success("索引已重建");
            delete indexCache.value[`${connId}:${dbName}.${collName}`];
            await loadIndexes(connId, dbName, collName);
          } catch (e) {
            message.error(`重建失败: ${e}`);
          }
        },
      });
    }
    if (action === "drop-indexes") {
      const cacheKey = `${connId}:${dbName}.${collName}`;
      const cached = indexCache.value[cacheKey] || [];
      const droppable = cached.filter((i) => i.name !== "_id_");
      if (droppable.length === 0) {
        message.info("没有可删除的索引 (_id_ 索引不可删)");
        return;
      }
      // 简单交互: 列出名字让用户输入要删的索引名 (含逗号分隔批量)
      dlg.create({
        title: `删除索引 (${collName})`,
        content: () =>
          h("div", { style: "display:flex;flex-direction:column;gap:8px" }, [
            h("p", { style: "margin:0;font-size:12px" }, "现有索引:"),
            h(
              "div",
              { style: "font-family:monospace;font-size:12px;color:#555;padding:4px 8px;background:#f5f5f5;border-radius:4px" },
              droppable.map((i) => i.name).join(", "),
            ),
            h("p", { style: "margin:8px 0 0;font-size:12px" }, "输入要删除的索引名 (逗号分隔):"),
            h("input", {
              id: "__idx_drop_names",
              style: "width:100%;padding:6px 8px;border:1px solid #ddd;border-radius:4px",
            }),
          ]),
        positiveText: "删除",
        negativeText: "取消",
        onPositiveClick: async () => {
          const raw = (document.getElementById("__idx_drop_names") as HTMLInputElement)?.value?.trim();
          if (!raw) { message.warning("请输入索引名"); return false; }
          const names = raw.split(",").map((s) => s.trim()).filter(Boolean);
          let ok = 0;
          for (const n of names) {
            try {
              await collApi.dropIndex(connId, dbName, collName, n);
              ok++;
            } catch (e) {
              message.error(`删除 ${n} 失败: ${e}`);
            }
          }
          if (ok > 0) message.success(`已删除 ${ok} 个索引`);
          delete indexCache.value[cacheKey];
          await loadIndexes(connId, dbName, collName);
        },
      });
    }
    if (action === "copy-coll-name") {
      navigator.clipboard.writeText(collName).then(
        () => message.success("已复制集合名"),
        () => message.error("复制失败"),
      );
    }
    if (action === "set-comment" || action === "add-to-favorites" || action === "show-locator") {
      message.info("此功能暂未实现");
    }
    if (action === "refresh-indexes") {
      delete indexCache.value[`${connId}:${dbName}.${collName}`];
      await loadIndexes(connId, dbName, collName);
      message.success("索引列表已刷新");
    }
  }

  // ---- 单个 idx 节点 ----
  if (nodeKey.startsWith("idx:")) {
    const { connId, dbName, collName, idxName } = parseIdxKey(nodeKey);

    if (action === "show-index") {
      // 等效那段 getCollectionIndexInfo 脚本: 贴脚本 + 把执行通道指向后端 get_index_info
      const script = buildIndexInfoScript(collName, idxName);
      const tabId = editorStore.createTab(connId, dbName, collName);
      editorStore.setTabExecutor(tabId, { kind: "indexInfo", collection: collName, indexName: idxName });
      editorStore.setTabSkipLint(tabId, true);
      editorStore.setContent(tabId, script);
      // 立即触发一次执行, 走 tab.executor 通道 -> get_index_info
      editorStore.executeQuery(tabId);
    }

    if (action === "update-index") {
      if (idxName === "_id_") {
        message.warning("_id_ 索引不可修改");
        return;
      }
      // 通过 listIndexes 拿到完整定义后, 打开 AddIndexDialog 编辑模式
      try {
        const all = await collApi.listIndexes(connId, dbName, collName);
        const target = all.find((i) => i.name === idxName);
        if (!target) {
          message.error("找不到该索引, 可能已被删除");
          return;
        }
        openAddIndex(connId, dbName, collName, target);
      } catch (e) {
        message.error(`加载索引信息失败: ${e}`);
      }
    }

    if (action === "drop-this-index") {
      if (idxName === "_id_") {
        message.warning("_id_ 索引不可删除");
        return;
      }
      const cfg = connStore.connections.find((c) => c.id === connId);
      const hostLabel = cfg ? cfg.name || `${cfg.host}:${cfg.port}` : connId;
      const script = `db.getSiblingDB(${JSON.stringify(dbName)})\n  .getCollection(${JSON.stringify(collName)}).dropIndex(${JSON.stringify(idxName)})`;
      dlg.warning({
        title: "Drop Index",
        content: () =>
          h("div", { style: "display:flex;flex-direction:column;gap:12px;line-height:1.6" }, [
            h("div", null, [
              "Do you want to drop this index ",
              h("strong", null, `"${idxName}"`),
              ' on "',
              h("strong", null, hostLabel),
              '"? This operation can not be undone.',
            ]),
            h("div", null, "The following script will be executed."),
            h(
              "pre",
              {
                style:
                  "margin:0;padding:8px 10px;background:#f7f9fb;border:1px solid #e0e0e0;border-radius:4px;font-family:Consolas,Monaco,monospace;font-size:12px;color:#333;white-space:pre-wrap;word-break:break-all",
              },
              script,
            ),
          ]),
        positiveText: "Ok",
        negativeText: "Cancel",
        onPositiveClick: async () => {
          try {
            await collApi.dropIndex(connId, dbName, collName, idxName);
            message.success(`索引 ${idxName} 已删除`);
            delete indexCache.value[`${connId}:${dbName}.${collName}`];
            await loadIndexes(connId, dbName, collName);
          } catch (e) {
            message.error(`删除失败: ${e}`);
          }
        },
      });
    }

    if (action === "add-index-from-idx") {
      openAddIndex(connId, dbName, collName);
    }

    if (action === "copy-idx-name") {
      navigator.clipboard.writeText(idxName).then(
        () => message.success("已复制索引名"),
        () => message.error("复制失败"),
      );
    }
  }

  // ---- DDL 操作 ----
  if (action === "create-coll" && nodeKey.startsWith("db:")) {
    const { connId, dbName } = parseDbKey(nodeKey);
    dlg.create({
      title: "创建集合",
      content: () => h("div", [
        h("p", { style: "margin:0 0 8px" }, `在 ${dbName} 中创建新集合:`),
        h("input", {
          id: "__create_coll_input",
          style: "width:100%;padding:6px 8px;border:1px solid #ddd;border-radius:4px;font-size:13px",
          placeholder: "集合名称",
        }),
      ]),
      positiveText: "创建",
      negativeText: "取消",
      onPositiveClick: async () => {
        const input = document.getElementById("__create_coll_input") as HTMLInputElement | null;
        const name = input?.value?.trim();
        if (!name) { message.warning("请输入集合名称"); return false; }
        try {
          await collApi.createCollection(connId, dbName, name);
          message.success(`集合 ${name} 创建成功`);
          await refreshDb(connId, dbName);
        } catch (e) { message.error(`创建失败: ${e}`); }
      },
    });
  }

  if (action === "drop-db" && nodeKey.startsWith("db:")) {
    const { connId, dbName } = parseDbKey(nodeKey);
    dlg.warning({
      title: "删除数据库",
      content: `确定要删除数据库 "${dbName}" 吗？此操作不可撤销！`,
      positiveText: "删除",
      negativeText: "取消",
      onPositiveClick: async () => {
        try {
          await dbApi.dropDatabase(connId, dbName);
          message.success(`数据库 ${dbName} 已删除`);
          await refreshConnection(connId);
        } catch (e) { message.error(`删除失败: ${e}`); }
      },
    });
  }

  if (action === "drop-coll" && nodeKey.startsWith("coll:")) {
    const { connId, dbName, collName } = parseCollKey(nodeKey);
    dlg.warning({
      title: "删除集合",
      content: `确定要删除集合 "${collName}" 吗？所有数据将丢失！`,
      positiveText: "删除",
      negativeText: "取消",
      onPositiveClick: async () => {
        try {
          await collApi.dropCollection(connId, dbName, collName);
          message.success(`集合 ${collName} 已删除`);
          await refreshDb(connId, dbName);
        } catch (e) { message.error(`删除失败: ${e}`); }
      },
    });
  }

  // ---- Users/Roles 操作 ----
  // ---- 添加用户（预设角色）----
  if (action.startsWith("add-user-") && nodeKey.startsWith("users:")) {
    const connId = nodeKey.slice(6);
    const roleSpec = action.slice("add-user-".length); // e.g. "read:app_server" or "custom"

    if (roleSpec === "custom") {
      // 自定义添加：弹窗输入
      dlg.create({
        title: "添加用户",
        content: () => h("div", { style: "display:flex;flex-direction:column;gap:8px" }, [
          h("input", { id: "__add_user_name", placeholder: "用户名", style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px" }),
          h("input", { id: "__add_user_pwd", placeholder: "密码", type: "password", style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px" }),
          h("input", { id: "__add_user_db", placeholder: "数据库 (默认 admin)", style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px" }),
          h("input", { id: "__add_user_role", placeholder: "角色 (如 readWrite)", style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px" }),
        ]),
        positiveText: "创建",
        negativeText: "取消",
        onPositiveClick: async () => {
          const username = (document.getElementById("__add_user_name") as HTMLInputElement)?.value?.trim();
          const password = (document.getElementById("__add_user_pwd") as HTMLInputElement)?.value;
          const userDb = (document.getElementById("__add_user_db") as HTMLInputElement)?.value?.trim() || "admin";
          const role = (document.getElementById("__add_user_role") as HTMLInputElement)?.value?.trim() || "read";
          if (!username || !password) { message.warning("用户名和密码不能为空"); return false; }
          try {
            const tabId = editorStore.createTab(connId, userDb);
            const query = `db.createUser({user:"${username}",pwd:"${password}",roles:[{role:"${role}",db:"${userDb}"}]})`;
            editorStore.setContent(tabId, query);
            editorStore.executeQuery(tabId);
            const users = await listUsers(connId, "admin");
            usersCache.value = { ...usersCache.value, [connId]: users };
          } catch (e) { message.error(`创建失败: ${e}`); }
        },
      });
    } else {
      // 预设角色：弹窗只输入用户名和密码
      const colonIdx = roleSpec.indexOf(":");
      const roleName = roleSpec.slice(0, colonIdx);
      const roleDb = roleSpec.slice(colonIdx + 1);

      dlg.create({
        title: `添加用户 (${roleName})`,
        content: () => h("div", { style: "display:flex;flex-direction:column;gap:8px" }, [
          h("input", { id: "__add_user_name", placeholder: "用户名", style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px" }),
          h("input", { id: "__add_user_pwd", placeholder: "密码", type: "password", style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px" }),
          h("p", { style: "margin:0;font-size:12px;color:#666" }, `角色: ${roleName} (${roleDb})`),
        ]),
        positiveText: "创建",
        negativeText: "取消",
        onPositiveClick: async () => {
          const username = (document.getElementById("__add_user_name") as HTMLInputElement)?.value?.trim();
          const password = (document.getElementById("__add_user_pwd") as HTMLInputElement)?.value;
          if (!username || !password) { message.warning("用户名和密码不能为空"); return false; }
          try {
            const tabId = editorStore.createTab(connId, roleDb);
            const query = `db.getSiblingDB("${roleDb}").createUser({user:"${username}",pwd:"${password}",roles:[{role:"${roleName}",db:"${roleDb}"}]})`;
            editorStore.setContent(tabId, query);
            editorStore.executeQuery(tabId);
            const users = await listUsers(connId, "admin");
            usersCache.value = { ...usersCache.value, [connId]: users };
          } catch (e) { message.error(`创建失败: ${e}`); }
        },
      });
    }
  }

  if (action === "view-users" && nodeKey.startsWith("users:")) {
    const connId = nodeKey.slice(6);
    const tabId = editorStore.createTab(connId, "admin");
    editorStore.setContent(tabId, 'db.getUsers()');
    editorStore.executeQuery(tabId);
  }
  if (action === "view-roles" && nodeKey.startsWith("users:")) {
    const connId = nodeKey.slice(6);
    const tabId = editorStore.createTab(connId, "admin");
    editorStore.setContent(tabId, 'db.getRoles({showBuiltinRoles: true})');
    editorStore.executeQuery(tabId);
  }
  if (action === "refresh-users" && nodeKey.startsWith("users:")) {
    const connId = nodeKey.slice(6);
    try {
      const users = await listUsers(connId, "admin");
      usersCache.value = { ...usersCache.value, [connId]: users };
      message.success("已刷新用户列表");
    } catch (e) { message.error(`刷新失败: ${e}`); }
  }
  if (action === "view-user" && nodeKey.startsWith("user:")) {
    const rest = nodeKey.slice(5);
    const colonIdx = rest.indexOf(":");
    const connId = rest.slice(0, colonIdx);
    const userPart = rest.slice(colonIdx + 1);
    const atIdx = userPart.lastIndexOf("@");
    const username = userPart.slice(0, atIdx);
    const userDb = userPart.slice(atIdx + 1);
    const tabId = editorStore.createTab(connId, userDb);
    editorStore.setContent(tabId, `db.getUser("${username}")`);
    editorStore.executeQuery(tabId);
  }
  if (action === "drop-user" && nodeKey.startsWith("user:")) {
    const rest = nodeKey.slice(5);
    const colonIdx = rest.indexOf(":");
    const connId = rest.slice(0, colonIdx);
    const userPart = rest.slice(colonIdx + 1);
    const atIdx = userPart.lastIndexOf("@");
    const username = userPart.slice(0, atIdx);
    dlg.warning({
      title: "删除用户",
      content: `确定要删除用户 "${username}" 吗？`,
      positiveText: "删除",
      negativeText: "取消",
      onPositiveClick: async () => {
        try {
          const tabId = editorStore.createTab(connId, "admin");
          editorStore.setContent(tabId, `db.dropUser("${username}")`);
          editorStore.executeQuery(tabId);
          // 刷新用户列表
          const users = await listUsers(connId, "admin");
          usersCache.value = { ...usersCache.value, [connId]: users };
        } catch (e) { message.error(`删除失败: ${e}`); }
      },
    });
  }
  if (action === "view-role" && nodeKey.startsWith("role:")) {
    const rest = nodeKey.slice(5);
    const colonIdx = rest.indexOf(":");
    const connId = rest.slice(0, colonIdx);
    const afterConn = rest.slice(colonIdx + 1);
    const lastColon = afterConn.lastIndexOf(":");
    const roleAtDb = afterConn.slice(lastColon + 1);
    const atIdx = roleAtDb.lastIndexOf("@");
    const roleName = roleAtDb.slice(0, atIdx);
    const roleDb = roleAtDb.slice(atIdx + 1);
    const tabId = editorStore.createTab(connId, roleDb);
    editorStore.setContent(tabId, `db.getSiblingDB("${roleDb}").getRole("${roleName}", {showPrivileges:true, showBuiltinRoles: true})`);
    editorStore.executeQuery(tabId);
  }
  if (action === "copy-name") {
    // 从 label 提取名称
    const label = typeof ctxNodeKey.value === "string" ? ctxNodeKey.value : "";
    let name = "";
    if (label.startsWith("users:")) name = "users";
    else if (label.startsWith("user:")) {
      const rest = label.slice(5);
      const colonIdx = rest.indexOf(":");
      const userPart = rest.slice(colonIdx + 1);
      const atIdx = userPart.lastIndexOf("@");
      name = userPart.slice(0, atIdx);
    } else if (label.startsWith("role:")) {
      const rest = label.slice(5);
      const lastColon = rest.lastIndexOf(":");
      const roleAtDb = rest.slice(lastColon + 1);
      const atIdx = roleAtDb.lastIndexOf("@");
      name = roleAtDb.slice(0, atIdx);
    }
    if (name) {
      navigator.clipboard.writeText(name).then(() => message.success("已复制")).catch(() => {});
    }
  }

  // ---- 库级 users ----
  if (action === "view-db-users" && nodeKey.startsWith("dbusers:")) {
    const rest = nodeKey.slice(8);
    const colonIdx = rest.indexOf(":");
    const connId = rest.slice(0, colonIdx);
    const dbName = rest.slice(colonIdx + 1);
    const tabId = editorStore.createTab(connId, dbName);
    editorStore.setContent(tabId, "db.getUsers()");
    editorStore.executeQuery(tabId);
  }
  if (action === "refresh-db-users" && nodeKey.startsWith("dbusers:")) {
    const rest = nodeKey.slice(8);
    const colonIdx = rest.indexOf(":");
    const connId = rest.slice(0, colonIdx);
    const dbName = rest.slice(colonIdx + 1);
    const cacheKey = `${connId}:${dbName}`;
    try {
      const users = await listUsers(connId, dbName);
      usersCache.value = { ...usersCache.value, [cacheKey]: users };
      message.success("已刷新用户列表");
    } catch (e) { message.error(`刷新失败: ${e}`); }
  }

  if (action === "view-db-roles" && nodeKey.startsWith("dbusers:")) {
    const rest = nodeKey.slice(8);
    const colonIdx = rest.indexOf(":");
    const connId = rest.slice(0, colonIdx);
    const dbName = rest.slice(colonIdx + 1);
    const tabId = editorStore.createTab(connId, dbName);
    editorStore.setContent(tabId, "db.getRoles({showBuiltinRoles: true})");
    editorStore.executeQuery(tabId);
  }

  // ---- 库级添加用户 ----
  if (action.startsWith("add-dbuser-") && nodeKey.startsWith("dbusers:")) {
    const rest = nodeKey.slice(8);
    const colonIdx = rest.indexOf(":");
    const connId = rest.slice(0, colonIdx);
    const roleSpec = action.slice("add-dbuser-".length); // e.g. "read:app_server" or "custom:app_server"

    if (roleSpec.startsWith("custom:")) {
      const targetDb = roleSpec.slice(7);
      dlg.create({
        title: `添加用户 (${targetDb})`,
        content: () => h("div", { style: "display:flex;flex-direction:column;gap:8px" }, [
          h("input", { id: "__add_dbu_name", placeholder: "用户名", style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px" }),
          h("input", { id: "__add_dbu_pwd", placeholder: "密码", type: "password", style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px" }),
          h("input", { id: "__add_dbu_role", placeholder: "角色 (如 readWrite)", value: "read", style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px" }),
          h("p", { style: "margin:0;font-size:12px;color:#666" }, `数据库: ${targetDb}`),
        ]),
        positiveText: "创建",
        negativeText: "取消",
        onPositiveClick: async () => {
          const username = (document.getElementById("__add_dbu_name") as HTMLInputElement)?.value?.trim();
          const password = (document.getElementById("__add_dbu_pwd") as HTMLInputElement)?.value;
          const role = (document.getElementById("__add_dbu_role") as HTMLInputElement)?.value?.trim() || "read";
          if (!username || !password) { message.warning("用户名和密码不能为空"); return false; }
          try {
            const tabId = editorStore.createTab(connId, targetDb);
            editorStore.setContent(tabId, `db.createUser({user:"${username}",pwd:"${password}",roles:[{role:"${role}",db:"${targetDb}"}]})`);
            editorStore.executeQuery(tabId);
            const users = await listUsers(connId, targetDb);
            usersCache.value = { ...usersCache.value, [`${connId}:${targetDb}`]: users };
          } catch (e) { message.error(`创建失败: ${e}`); }
        },
      });
    } else {
      const sepIdx = roleSpec.indexOf(":");
      const roleName = roleSpec.slice(0, sepIdx);
      const targetDb = roleSpec.slice(sepIdx + 1);
      dlg.create({
        title: `添加用户 - ${roleName} (${targetDb})`,
        content: () => h("div", { style: "display:flex;flex-direction:column;gap:8px" }, [
          h("input", { id: "__add_dbu_name", placeholder: "用户名", style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px" }),
          h("input", { id: "__add_dbu_pwd", placeholder: "密码", type: "password", style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px" }),
          h("p", { style: "margin:0;font-size:12px;color:#666" }, `角色: ${roleName} | 数据库: ${targetDb}`),
        ]),
        positiveText: "创建",
        negativeText: "取消",
        onPositiveClick: async () => {
          const username = (document.getElementById("__add_dbu_name") as HTMLInputElement)?.value?.trim();
          const password = (document.getElementById("__add_dbu_pwd") as HTMLInputElement)?.value;
          if (!username || !password) { message.warning("用户名和密码不能为空"); return false; }
          try {
            const tabId = editorStore.createTab(connId, targetDb);
            editorStore.setContent(tabId, `db.createUser({user:"${username}",pwd:"${password}",roles:[{role:"${roleName}",db:"${targetDb}"}]})`);
            editorStore.executeQuery(tabId);
            const users = await listUsers(connId, targetDb);
            usersCache.value = { ...usersCache.value, [`${connId}:${targetDb}`]: users };
          } catch (e) { message.error(`创建失败: ${e}`); }
        },
      });
    }
  }
}

async function refreshConnection(connId: string) {
  const dbs = await dbApi.listDatabases(connId);
  const expandedDbs = expandedKeys.value
    .filter((k) => k.startsWith(`db:${connId}:`))
    .map((k) => k.slice(3 + connId.length + 1));

  const collResults = await Promise.all(
    expandedDbs.map(async (dbName) => ({
      dbName,
      colls: await dbApi.listCollections(connId, dbName),
    })),
  );

  dbStore.databases[connId] = dbs;
  for (const { dbName, colls } of collResults) {
    dbStore.collections[`${connId}:${dbName}`] = colls;
  }
  statsCache.value = {};
}

async function refreshDb(connId: string, dbName: string) {
  const colls = await dbApi.listCollections(connId, dbName);
  dbStore.collections[`${connId}:${dbName}`] = colls;
  // 也刷新 db 列表以更新集合数量
  const dbs = await dbApi.listDatabases(connId);
  dbStore.databases[connId] = dbs;
}

// 悬停统计
async function loadCollStats(connId: string, db: string, coll: string) {
  const cacheKey = `${connId}:${db}.${coll}`;
  if (statsCache.value[cacheKey]) return;
  try {
    const stats = await collApi.getCollectionStats(connId, db, coll);
    statsCache.value = { ...statsCache.value, [cacheKey]: stats };
  } catch {
    // ignore
  }
}

function renderLabel({ option }: { option: TreeOption }): VNodeChild {
  const node = option as TreeNode;
  const key = typeof node.key === "string" ? node.key : "";

  if (!key.startsWith("coll:")) {
    return h("span", {}, node.label as string);
  }

  const { connId, dbName, collName } = parseCollKey(key);
  const cacheKey = `${connId}:${dbName}.${collName}`;

  return h(
    NPopover,
    {
      trigger: "hover",
      placement: "right",
      delay: 300,
      onUpdateShow: (show: boolean) => {
        if (show) loadCollStats(connId, dbName, collName);
      },
    },
    {
      trigger: () => h("span", { style: "cursor:pointer" }, node.label as string),
      default: () => {
        const s = statsCache.value[cacheKey];
        if (!s) return h("span", { style: "color:#999;font-size:12px" }, "加载中...");
        const rows: [string, string][] = [
          ["name", collName],
          ["nameSpace", `${dbName}.${collName}`],
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

/** 切换节点展开/收起 */
function toggleExpand(key: string) {
  if (expandedKeys.value.includes(key)) {
    expandedKeys.value = expandedKeys.value.filter((k) => k !== key);
  } else {
    handleExpandUpdate([...expandedKeys.value, key]);
  }
}

function handleNodeDblClick(node: TreeNode) {
  const key = typeof node.key === "string" ? node.key : "";
  // 双击未连接的连接 → 发起连接并自动展开
  if (key.startsWith("conn:") && node.connConfig && !connStore.isActive(node.connConfig.id)) {
    // 显示连接中状态
    connectingIds.value.add(node.connConfig.id);
    connectingIds.value = new Set(connectingIds.value);

    emit("connectServer", node.connConfig);

    const connId = node.connConfig.id;
    const check = setInterval(() => {
      if (connStore.isActive(connId)) {
        clearInterval(check);
        connectingIds.value.delete(connId);
        connectingIds.value = new Set(connectingIds.value);
        if (!expandedKeys.value.includes(key)) {
          handleExpandUpdate([...expandedKeys.value, key]);
        }
      }
    }, 300);
    // SSH 连接可能较慢，超时 30s
    setTimeout(() => {
      clearInterval(check);
      connectingIds.value.delete(connId);
      connectingIds.value = new Set(connectingIds.value);
    }, 30000);
    return;
  }
  // 双击已连接的连接 → 展开/收起
  if (key.startsWith("conn:")) {
    toggleExpand(key);
    return;
  }
  // 双击数据库 → 展开/收起
  if (key.startsWith("db:")) {
    toggleExpand(key);
    return;
  }
  // 双击集合 → 打开查询
  if (key.startsWith("coll:")) {
    const { connId, dbName, collName } = parseCollKey(key);
    const tabId = editorStore.createTab(connId, dbName, collName);
    const defaultQuery = buildDefaultQuery(collName);
    editorStore.setContent(tabId, defaultQuery);
    editorStore.executeQuery(tabId);
    return;
  }
  // 双击用户 → 执行 db.getUser("username")
  if (key.startsWith("user:")) {
    // key: user:connId:username@database
    const rest = key.slice(5);
    const colonIdx = rest.indexOf(":");
    const connId = rest.slice(0, colonIdx);
    const userPart = rest.slice(colonIdx + 1);
    const atIdx = userPart.lastIndexOf("@");
    const username = userPart.slice(0, atIdx);
    const userDb = userPart.slice(atIdx + 1);
    const tabId = editorStore.createTab(connId, userDb);
    editorStore.setContent(tabId, `db.getUser("${username}")`);
    editorStore.executeQuery(tabId);
    return;
  }
  // 双击角色 → 执行 db.getSiblingDB("db").getRole("role", ...)
  if (key.startsWith("role:")) {
    // key: role:connId:username:roleName@roleDb
    const rest = key.slice(5);
    const colonIdx = rest.indexOf(":");
    const connId = rest.slice(0, colonIdx);
    const afterConn = rest.slice(colonIdx + 1);
    const lastColon = afterConn.lastIndexOf(":");
    const roleAtDb = afterConn.slice(lastColon + 1);
    const atIdx = roleAtDb.lastIndexOf("@");
    const roleName = roleAtDb.slice(0, atIdx);
    const roleDb = roleAtDb.slice(atIdx + 1);
    const tabId = editorStore.createTab(connId, roleDb);
    const query = `db.getSiblingDB("${roleDb}").getRole("${roleName}", {showPrivileges:true, showBuiltinRoles: true})`;
    editorStore.setContent(tabId, query);
    editorStore.executeQuery(tabId);
    return;
  }
  // 双击 users 节点 → 展开/收起
  if (key.startsWith("users:") || key.startsWith("dbusers:")) {
    toggleExpand(key);
    return;
  }
}
</script>

<template>
  <div class="server-tree">
    <div class="tree-toolbar">
      <n-button size="small" quaternary @click="$emit('createConnection')">
        <template #icon><n-icon><AddIcon /></n-icon></template>
        New Connection
      </n-button>
    </div>

    <n-empty
      v-if="connStore.connections.length === 0"
      description="暂无连接"
      style="padding: 24px"
      size="small"
    />

    <n-tree
      v-else
      :data="treeData"
      :expanded-keys="expandedKeys"
      block-line
      selectable
      :render-label="renderLabel"
      @update:expanded-keys="handleExpandUpdate"
      :node-props="({ option }: any) => {
        const node = option as TreeNode;
        return {
          onDblclick: () => handleNodeDblClick(node),
          onContextmenu: (e: MouseEvent) => handleCtxMenu(e, node),
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

    <AddIndexDialog
      v-model:show="addIndexShow"
      :connection-id="addIndexCtx.connId"
      :database="addIndexCtx.database"
      :collection="addIndexCtx.collection"
      :editing-index="addIndexEditing"
      @created="handleIndexCreated"
    />
  </div>
</template>

<style scoped>
.server-tree {
  height: 100%;
  display: flex;
  flex-direction: column;
}
.tree-toolbar {
  padding: 4px 8px;
  border-bottom: 1px solid var(--n-border-color);
  flex-shrink: 0;
}
.loading-dot {
  display: inline-block;
  width: 12px;
  height: 12px;
  border: 2px solid #e0e0e0;
  border-top-color: #3875d7;
  border-radius: 50%;
  animation: spin-dot 0.6s linear infinite;
  margin-right: 4px;
}
@keyframes spin-dot {
  to { transform: rotate(360deg); }
}
</style>

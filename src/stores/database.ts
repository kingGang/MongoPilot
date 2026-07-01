import { defineStore } from "pinia";
import { reactive, ref } from "vue";
import type { DatabaseInfo, CollectionInfo } from "@/types/database";
import * as api from "@/api/database";

export const useDatabaseStore = defineStore("database", () => {
  const databases = reactive<Record<string, DatabaseInfo[]>>({});
  const collections = reactive<Record<string, CollectionInfo[]>>({});
  const loading = ref(false);
  const error = ref<string | null>(null);

  async function fetchDatabases(connectionId: string) {
    loading.value = true;
    error.value = null;
    try {
      // 后端返回 collectionCount=-1 (未加载), 先立刻把 db 名字画到树上,
      // 再在后台 fan-out 补每个 db 的集合数, 拿到后 reactively 更新 label.
      const dbs = await api.listDatabases(connectionId);
      databases[connectionId] = dbs;

      api
        .countDatabaseCollections(connectionId)
        .then((counts) => {
          const current = databases[connectionId];
          if (!current) return;
          for (const db of current) {
            const c = counts[db.name];
            if (typeof c === "number") db.collectionCount = c;
          }
        })
        .catch(() => {
          /* 用户没权限跑 listDatabases 就静默忽略, 保持 -1 (label 不显 count) */
        });
    } catch (e) {
      error.value = String(e);
      console.error("fetchDatabases failed:", e);
    } finally {
      loading.value = false;
    }
  }

  async function fetchCollections(connectionId: string, database: string) {
    const key = `${connectionId}:${database}`;
    try {
      const colls = await api.listCollections(connectionId, database);
      collections[key] = colls;
      // 把这个 db 在 databases 里的 collectionCount 同步为真实长度 —— 用户展开这个 db
      // 就已经拿到了准确值, 不必再等 countDatabaseCollections.
      const dbs = databases[connectionId];
      if (dbs) {
        const db = dbs.find((d) => d.name === database);
        if (db) db.collectionCount = colls.length;
      }
    } catch (e) {
      console.error("fetchCollections failed:", e);
    }
  }

  function getDatabases(connectionId: string): DatabaseInfo[] {
    return databases[connectionId] || [];
  }

  function getCollections(connectionId: string, database: string): CollectionInfo[] {
    return collections[`${connectionId}:${database}`] || [];
  }

  return {
    databases,
    collections,
    loading,
    error,
    fetchDatabases,
    fetchCollections,
    getDatabases,
    getCollections,
  };
});

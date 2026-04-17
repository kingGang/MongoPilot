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
      const dbs = await api.listDatabases(connectionId);
      databases[connectionId] = dbs;
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

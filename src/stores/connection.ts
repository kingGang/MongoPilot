import { defineStore } from "pinia";
import { ref, computed } from "vue";
import type { ConnectionConfig, ServerInfo } from "@/types/connection";
import * as api from "@/api/connection";

export const useConnectionStore = defineStore("connection", () => {
  const connections = ref<ConnectionConfig[]>([]);
  const activeIds = ref<Set<string>>(new Set());
  const loading = ref(false);
  const error = ref<string | null>(null);

  const connectionTree = computed(() => {
    const groups = new Map<string, ConnectionConfig[]>();
    for (const conn of connections.value) {
      const key = conn.groupPath || "(未分组)";
      if (!groups.has(key)) {
        groups.set(key, []);
      }
      groups.get(key)!.push(conn);
    }
    return groups;
  });

  async function fetchConnections() {
    loading.value = true;
    error.value = null;
    try {
      connections.value = await api.listConnections();
      const ids = await api.activeConnections();
      activeIds.value = new Set(ids);
    } catch (e) {
      error.value = String(e);
    } finally {
      loading.value = false;
    }
  }

  async function save(config: ConnectionConfig) {
    await api.saveConnection(config);
    await fetchConnections();
  }

  async function remove(id: string) {
    await api.deleteConnection(id);
    const next = new Set(activeIds.value);
    next.delete(id);
    activeIds.value = next;
    await fetchConnections();
  }

  async function testConn(config: ConnectionConfig): Promise<ServerInfo> {
    return api.testConnection(config);
  }

  async function connect(config: ConnectionConfig) {
    await api.connectToServer(config);
    const next = new Set(activeIds.value);
    next.add(config.id);
    activeIds.value = next;
  }

  async function disconnectConn(id: string) {
    await api.disconnect(id);
    const next = new Set(activeIds.value);
    next.delete(id);
    activeIds.value = next;
  }

  function isActive(id: string): boolean {
    return activeIds.value.has(id);
  }

  /** 该连接是否只读 (写操作应在后端被拦截; 前端用它隐藏编辑按钮/灰化交互) */
  function isReadOnly(id: string): boolean {
    if (!id) return false;
    return connections.value.find((c) => c.id === id)?.readOnly === true;
  }

  return {
    connections,
    activeIds,
    loading,
    error,
    connectionTree,
    fetchConnections,
    save,
    remove,
    testConn,
    connect,
    disconnectConn,
    isActive,
    isReadOnly,
  };
});

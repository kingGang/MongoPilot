import { defineStore } from "pinia";
import { ref } from "vue";
import * as scriptApi from "@/api/script";
import type { ScriptFolderInfo, ScriptInfo, SaveScriptInput } from "@/types/script";

export const useScriptStore = defineStore("script", () => {
  const scripts = ref<ScriptInfo[]>([]);
  const folders = ref<ScriptFolderInfo[]>([]);
  const loaded = ref(false);

  async function refresh() {
    const [s, f] = await Promise.all([scriptApi.listScripts(), scriptApi.listScriptFolders()]);
    scripts.value = s;
    folders.value = f;
    loaded.value = true;
  }

  async function save(input: SaveScriptInput): Promise<ScriptInfo> {
    const saved = await scriptApi.saveScript(input);
    // 替换或插入
    const idx = scripts.value.findIndex((s) => s.id === saved.id);
    if (idx >= 0) scripts.value[idx] = saved;
    else scripts.value.push(saved);
    return saved;
  }

  async function remove(id: string) {
    await scriptApi.deleteScript(id);
    scripts.value = scripts.value.filter((s) => s.id !== id);
  }

  async function createFolder(path: string) {
    await scriptApi.createScriptFolder(path);
    if (!folders.value.some((f) => f.path === path)) {
      folders.value.push({ path, sortOrder: 0, createdAt: new Date().toISOString() });
    }
  }

  async function deleteFolder(path: string, cascade = false) {
    await scriptApi.deleteScriptFolder(path, cascade);
    // 同时从本地缓存里剔除该文件夹和子文件夹, 以及其中的脚本
    const prefix = `${path}/`;
    folders.value = folders.value.filter((f) => f.path !== path && !f.path.startsWith(prefix));
    scripts.value = scripts.value.filter(
      (s) => s.folderPath !== path && !s.folderPath.startsWith(prefix),
    );
  }

  async function renameFolder(oldPath: string, newPath: string) {
    await scriptApi.renameScriptFolder(oldPath, newPath);
    const prefix = `${oldPath}/`;
    const newPrefix = `${newPath}/`;
    folders.value = folders.value.map((f) => {
      if (f.path === oldPath) return { ...f, path: newPath };
      if (f.path.startsWith(prefix)) return { ...f, path: newPrefix + f.path.slice(prefix.length) };
      return f;
    });
    scripts.value = scripts.value.map((s) => {
      if (s.folderPath === oldPath) return { ...s, folderPath: newPath };
      if (s.folderPath.startsWith(prefix)) {
        return { ...s, folderPath: newPrefix + s.folderPath.slice(prefix.length) };
      }
      return s;
    });
  }

  return {
    scripts,
    folders,
    loaded,
    refresh,
    save,
    remove,
    createFolder,
    deleteFolder,
    renameFolder,
  };
});

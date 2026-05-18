<script setup lang="ts">
import { computed, h, onMounted, ref, type VNodeChild } from "vue";
import {
  NTree, NIcon, NButton, NDropdown, NEmpty, NInput,
  useMessage, useDialog,
} from "naive-ui";
import {
  Folder as FolderIcon,
  FolderOpen as FolderOpenIcon,
  Document as ScriptIcon,
  Add as AddIcon,
  Refresh as RefreshIcon,
  Search as SearchIcon,
  CloudDownload as ImportIcon,
} from "@vicons/ionicons5";
import type { TreeOption } from "naive-ui";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useScriptStore } from "@/stores/script";
import * as scriptApi from "@/api/script";
import type { ScriptInfo } from "@/types/script";

const emit = defineEmits<{
  /** 打开脚本到一个新编辑器 tab */
  openScript: [script: ScriptInfo];
}>();

const scriptStore = useScriptStore();
const message = useMessage();
const dlg = useDialog();

const search = ref("");

onMounted(async () => {
  if (!scriptStore.loaded) {
    try {
      await scriptStore.refresh();
    } catch (e) {
      message.error(`加载脚本列表失败: ${e}`);
    }
  }
});

interface TreeNode extends TreeOption {
  key: string;
  label: string;
  isLeaf?: boolean;
  children?: TreeNode[];
  /** 折叠图标 */
  prefix?: () => VNodeChild;
  /** 区分 folder / script / 空提示 */
  kind: "folder" | "script" | "empty";
  /** 对应数据 */
  folderPath?: string;
  script?: ScriptInfo;
}

const expandedKeys = ref<string[]>([]);

/**
 * 把脚本 + 文件夹合并成树.
 * 根节点 = 所有 folderPath = "" 的 scripts + 所有顶层文件夹 (path 不含 "/")
 */
const treeData = computed<TreeNode[]>(() => {
  const allFolderPaths = new Set<string>(scriptStore.folders.map((f) => f.path));
  // 从脚本里推导出隐式目录路径 (例如 script.folderPath = "a/b" 隐含 "a" 和 "a/b")
  for (const s of scriptStore.scripts) {
    if (!s.folderPath) continue;
    const parts = s.folderPath.split("/");
    for (let i = 1; i <= parts.length; i++) {
      allFolderPaths.add(parts.slice(0, i).join("/"));
    }
  }

  const filterFn = (s: ScriptInfo) => {
    if (!search.value.trim()) return true;
    const kw = search.value.trim().toLowerCase();
    return (
      s.name.toLowerCase().includes(kw) ||
      s.content.toLowerCase().includes(kw) ||
      s.folderPath.toLowerCase().includes(kw)
    );
  };

  // 索引: 父路径 -> 子节点
  const childrenByParent = new Map<string, TreeNode[]>();
  const ensureBucket = (parent: string) => {
    if (!childrenByParent.has(parent)) childrenByParent.set(parent, []);
    return childrenByParent.get(parent)!;
  };

  // 文件夹节点
  for (const path of allFolderPaths) {
    const slashIdx = path.lastIndexOf("/");
    const parent = slashIdx === -1 ? "" : path.slice(0, slashIdx);
    const name = slashIdx === -1 ? path : path.slice(slashIdx + 1);
    ensureBucket(parent).push({
      key: `folder:${path}`,
      label: name,
      kind: "folder",
      folderPath: path,
      isLeaf: false,
      prefix: () =>
        h(
          NIcon,
          { size: 14, color: "#e8a838" },
          { default: () => h(expandedKeys.value.includes(`folder:${path}`) ? FolderOpenIcon : FolderIcon) },
        ),
      children: [],
    });
  }

  // 脚本节点
  for (const s of scriptStore.scripts) {
    if (!filterFn(s)) continue;
    ensureBucket(s.folderPath).push({
      key: `script:${s.id}`,
      label: s.name,
      kind: "script",
      script: s,
      isLeaf: true,
      prefix: () => h(NIcon, { size: 13, color: "#63e2b7" }, { default: () => h(ScriptIcon) }),
    });
  }

  // 排序: folder 先, script 后, 同类按名字
  const sortNodes = (nodes: TreeNode[]) =>
    nodes.sort((a, b) => {
      if (a.kind !== b.kind) return a.kind === "folder" ? -1 : 1;
      return a.label.localeCompare(b.label, "zh-Hans-CN", { numeric: true, sensitivity: "base" });
    });

  // 把 children 接到对应文件夹节点
  for (const [parent, children] of childrenByParent) {
    sortNodes(children);
    if (parent === "") continue;
    const allFolders: TreeNode[] = [];
    for (const list of childrenByParent.values()) {
      for (const n of list) {
        if (n.kind === "folder") allFolders.push(n);
      }
    }
    const parentNode = allFolders.find((n) => n.folderPath === parent);
    if (parentNode) parentNode.children = children;
  }

  const roots = childrenByParent.get("") ?? [];
  // 搜索模式下: 若文件夹的所有脚本都被过滤掉就隐藏空文件夹
  if (search.value.trim()) {
    const isFolderEmpty = (n: TreeNode): boolean => {
      if (n.kind !== "folder") return false;
      const cs = n.children ?? [];
      return cs.every((c) => (c.kind === "folder" ? isFolderEmpty(c) : false));
    };
    const prune = (nodes: TreeNode[]): TreeNode[] =>
      nodes.filter((n) => n.kind === "script" || !isFolderEmpty(n)).map((n) => ({
        ...n,
        children: n.children ? prune(n.children) : n.children,
      }));
    return prune(roots);
  }
  return roots;
});

// ---- 右键菜单 ----
const showCtx = ref(false);
const ctxX = ref(0);
const ctxY = ref(0);
const ctxNode = ref<TreeNode | null>(null);

function menuRow(label: string, shortcut?: string) {
  return () =>
    h(
      "div",
      {
        style: "display:flex;justify-content:space-between;gap:24px;min-width:180px",
      },
      [
        h("span", null, label),
        shortcut ? h("span", { style: "color:#999;font-size:12px" }, shortcut) : null,
      ],
    );
}

const ctxOptions = computed(() => {
  const node = ctxNode.value;
  if (!node) {
    // 空白处右键 -> 在根目录新建
    return [
      { label: menuRow("新建脚本"), key: "new-script-root" },
      { label: menuRow("新建文件夹"), key: "new-folder-root" },
      { type: "divider" as const, key: "d0a" },
      { label: menuRow("从文件导入..."), key: "import-files-root" },
      { label: menuRow("从目录导入..."), key: "import-dir-root" },
      { type: "divider" as const, key: "d0" },
      { label: menuRow("刷新"), key: "refresh" },
    ];
  }
  if (node.kind === "folder") {
    return [
      { label: menuRow("在此处新建脚本"), key: "new-script-here" },
      { label: menuRow("新建子文件夹"), key: "new-subfolder" },
      { type: "divider" as const, key: "df0" },
      { label: menuRow("导入文件到此处..."), key: "import-files-here" },
      { label: menuRow("导入目录到此处..."), key: "import-dir-here" },
      { type: "divider" as const, key: "df1" },
      { label: menuRow("重命名文件夹"), key: "rename-folder" },
      { label: menuRow("删除文件夹"), key: "delete-folder" },
    ];
  }
  // script
  return [
    { label: menuRow("打开", "双击"), key: "open" },
    { type: "divider" as const, key: "ds1" },
    { label: menuRow("重命名"), key: "rename-script" },
    { label: menuRow("移动到..."), key: "move-script" },
    { label: menuRow("复制名称"), key: "copy-name" },
    { type: "divider" as const, key: "ds2" },
    { label: menuRow("删除"), key: "delete-script" },
  ];
});

function onCtxMenu(e: MouseEvent, node: TreeNode | null) {
  e.preventDefault();
  ctxNode.value = node;
  ctxX.value = e.clientX;
  ctxY.value = e.clientY;
  showCtx.value = true;
}

function onTreeContainerCtx(e: MouseEvent) {
  // 点击在空白区域 (没命中任何节点) 才触发
  const target = e.target as HTMLElement;
  if (target.closest(".n-tree-node")) return;
  onCtxMenu(e, null);
}

async function handleCtxSelect(action: string) {
  showCtx.value = false;
  const node = ctxNode.value;

  if (action === "refresh") {
    await scriptStore.refresh();
    return;
  }

  if (action === "new-script-root" || action === "new-script-here") {
    const folder = action === "new-script-here" && node?.kind === "folder" ? node.folderPath ?? "" : "";
    promptNewScript(folder);
    return;
  }

  if (action === "new-folder-root" || action === "new-subfolder") {
    const parent = action === "new-subfolder" && node?.kind === "folder" ? node.folderPath ?? "" : "";
    promptNewFolder(parent);
    return;
  }

  if (action === "import-files-root") { handleImportFiles(""); return; }
  if (action === "import-dir-root") { handleImportDirectory(""); return; }
  if (action === "import-files-here" && node?.kind === "folder") {
    handleImportFiles(node.folderPath ?? "");
    return;
  }
  if (action === "import-dir-here" && node?.kind === "folder") {
    handleImportDirectory(node.folderPath ?? "");
    return;
  }

  if (!node) return;

  if (node.kind === "folder" && action === "rename-folder") {
    promptRenameFolder(node.folderPath ?? "");
    return;
  }
  if (node.kind === "folder" && action === "delete-folder") {
    confirmDeleteFolder(node.folderPath ?? "");
    return;
  }

  if (node.kind === "script") {
    if (action === "open" && node.script) emit("openScript", node.script);
    if (action === "rename-script" && node.script) promptRenameScript(node.script);
    if (action === "move-script" && node.script) promptMoveScript(node.script);
    if (action === "copy-name" && node.script) {
      navigator.clipboard.writeText(node.script.name).then(
        () => message.success("已复制"),
        () => message.error("复制失败"),
      );
    }
    if (action === "delete-script" && node.script) confirmDeleteScript(node.script);
  }
}

// ---- 对话框 ----

function promptNewScript(folder: string) {
  dlg.create({
    title: folder ? `在 ${folder} 下新建脚本` : "新建脚本",
    content: () =>
      h("div", { style: "display:flex;flex-direction:column;gap:8px" }, [
        h("input", {
          id: "__new_script_name",
          placeholder: "脚本名称",
          style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px",
        }),
      ]),
    positiveText: "创建",
    negativeText: "取消",
    onPositiveClick: async () => {
      const name = (document.getElementById("__new_script_name") as HTMLInputElement)?.value?.trim();
      if (!name) {
        message.warning("请输入名称");
        return false;
      }
      try {
        const saved = await scriptStore.save({
          name,
          folderPath: folder,
          content: "",
        });
        message.success("已创建");
        emit("openScript", saved);
      } catch (e) {
        message.error(`创建失败: ${e}`);
      }
    },
  });
}

function promptNewFolder(parent: string) {
  dlg.create({
    title: parent ? `在 ${parent} 下新建文件夹` : "新建文件夹",
    content: () =>
      h("div", { style: "display:flex;flex-direction:column;gap:8px" }, [
        h("input", {
          id: "__new_folder_name",
          placeholder: "文件夹名称",
          style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px",
        }),
      ]),
    positiveText: "创建",
    negativeText: "取消",
    onPositiveClick: async () => {
      const name = (document.getElementById("__new_folder_name") as HTMLInputElement)?.value?.trim();
      if (!name) {
        message.warning("请输入名称");
        return false;
      }
      if (name.includes("/")) {
        message.warning("名称中不能包含 /");
        return false;
      }
      const path = parent ? `${parent}/${name}` : name;
      try {
        await scriptStore.createFolder(path);
        expandedKeys.value = [...new Set([...expandedKeys.value, `folder:${path}`])];
      } catch (e) {
        message.error(`创建失败: ${e}`);
      }
    },
  });
}

function promptRenameFolder(path: string) {
  const slashIdx = path.lastIndexOf("/");
  const currentName = slashIdx === -1 ? path : path.slice(slashIdx + 1);
  const parent = slashIdx === -1 ? "" : path.slice(0, slashIdx);
  dlg.create({
    title: `重命名文件夹: ${path}`,
    content: () =>
      h("div", { style: "display:flex;flex-direction:column;gap:8px" }, [
        h("input", {
          id: "__ren_folder",
          value: currentName,
          style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px",
        }),
      ]),
    positiveText: "确定",
    negativeText: "取消",
    onPositiveClick: async () => {
      const newName = (document.getElementById("__ren_folder") as HTMLInputElement)?.value?.trim();
      if (!newName || newName === currentName) return;
      if (newName.includes("/")) {
        message.warning("名称中不能包含 /");
        return false;
      }
      const newPath = parent ? `${parent}/${newName}` : newName;
      try {
        await scriptStore.renameFolder(path, newPath);
      } catch (e) {
        message.error(`重命名失败: ${e}`);
      }
    },
  });
}

function confirmDeleteFolder(path: string) {
  const inFolderScripts = scriptStore.scripts.filter(
    (s) => s.folderPath === path || s.folderPath.startsWith(`${path}/`),
  ).length;
  dlg.warning({
    title: "删除文件夹",
    content:
      inFolderScripts > 0
        ? `文件夹 "${path}" 内含 ${inFolderScripts} 个脚本, 删除将一并清除. 确定?`
        : `确定删除空文件夹 "${path}"?`,
    positiveText: "删除",
    negativeText: "取消",
    onPositiveClick: async () => {
      try {
        await scriptStore.deleteFolder(path, true);
        message.success("已删除");
      } catch (e) {
        message.error(`删除失败: ${e}`);
      }
    },
  });
}

function promptRenameScript(script: ScriptInfo) {
  dlg.create({
    title: `重命名脚本`,
    content: () =>
      h("div", { style: "display:flex;flex-direction:column;gap:8px" }, [
        h("input", {
          id: "__ren_script",
          value: script.name,
          style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px",
        }),
      ]),
    positiveText: "确定",
    negativeText: "取消",
    onPositiveClick: async () => {
      const newName = (document.getElementById("__ren_script") as HTMLInputElement)?.value?.trim();
      if (!newName || newName === script.name) return;
      try {
        await scriptStore.save({ ...script, name: newName });
      } catch (e) {
        message.error(`重命名失败: ${e}`);
      }
    },
  });
}

function promptMoveScript(script: ScriptInfo) {
  const allFolderPaths = new Set<string>([""]);
  for (const f of scriptStore.folders) allFolderPaths.add(f.path);
  for (const s of scriptStore.scripts) {
    if (!s.folderPath) continue;
    const parts = s.folderPath.split("/");
    for (let i = 1; i <= parts.length; i++) {
      allFolderPaths.add(parts.slice(0, i).join("/"));
    }
  }
  const sorted = [...allFolderPaths].sort();
  dlg.create({
    title: `移动脚本: ${script.name}`,
    content: () =>
      h("div", { style: "display:flex;flex-direction:column;gap:8px" }, [
        h("p", { style: "margin:0;font-size:12px;color:#666" }, "目标文件夹 (留空 = 根目录):"),
        h(
          "select",
          {
            id: "__move_target",
            style: "padding:6px 8px;border:1px solid #ddd;border-radius:4px",
          },
          sorted.map((p) =>
            h(
              "option",
              { value: p, selected: p === script.folderPath ? "selected" : undefined },
              p || "(根目录)",
            ),
          ),
        ),
      ]),
    positiveText: "移动",
    negativeText: "取消",
    onPositiveClick: async () => {
      const target = (document.getElementById("__move_target") as HTMLSelectElement)?.value ?? "";
      if (target === script.folderPath) return;
      try {
        await scriptStore.save({ ...script, folderPath: target });
        message.success("已移动");
      } catch (e) {
        message.error(`移动失败: ${e}`);
      }
    },
  });
}

function confirmDeleteScript(script: ScriptInfo) {
  dlg.warning({
    title: "删除脚本",
    content: `确定删除脚本 "${script.name}"? 此操作不可撤销.`,
    positiveText: "删除",
    negativeText: "取消",
    onPositiveClick: async () => {
      try {
        await scriptStore.remove(script.id);
        message.success("已删除");
      } catch (e) {
        message.error(`删除失败: ${e}`);
      }
    },
  });
}

function onNodeDblClick(node: TreeNode) {
  if (node.kind === "script" && node.script) emit("openScript", node.script);
}

async function handleRefresh() {
  try {
    await scriptStore.refresh();
  } catch (e) {
    message.error(`刷新失败: ${e}`);
  }
}

const importMenuOptions = [
  { label: "从文件导入...", key: "files" },
  { label: "从目录导入...", key: "dir" },
];

function onImportMenuSelect(key: string) {
  if (key === "files") handleImportFiles("");
  else if (key === "dir") handleImportDirectory("");
}

async function handleImportFiles(targetFolder = "") {
  try {
    const selected = await openDialog({
      multiple: true,
      filters: [
        {
          name: "Scripts",
          extensions: ["js", "ts", "sql", "json", "txt", "md", "mongosh"],
        },
      ],
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    if (paths.length === 0) return;
    const summary = await scriptApi.importScriptFiles(paths, targetFolder);
    await scriptStore.refresh();
    message.success(
      `已导入 ${summary.imported} 个文件${summary.skipped > 0 ? ` (跳过 ${summary.skipped})` : ""}`,
    );
  } catch (e) {
    message.error(`导入失败: ${e}`);
  }
}

async function handleImportDirectory(targetFolder = "") {
  try {
    const selected = await openDialog({ directory: true, multiple: false });
    if (!selected || Array.isArray(selected)) return;
    const summary = await scriptApi.importScriptDirectory(selected, targetFolder);
    await scriptStore.refresh();
    message.success(
      `已导入 ${summary.imported} 个文件, ${summary.foldersCreated} 个文件夹${
        summary.skipped > 0 ? ` (跳过 ${summary.skipped})` : ""
      }`,
    );
  } catch (e) {
    message.error(`导入失败: ${e}`);
  }
}
</script>

<template>
  <div class="script-tree" @contextmenu="onTreeContainerCtx">
    <div class="script-toolbar">
      <n-button size="small" quaternary :title="'新建脚本'" @click="() => promptNewScript('')">
        <template #icon><n-icon><AddIcon /></n-icon></template>
      </n-button>
      <n-button size="small" quaternary :title="'新建文件夹'" @click="() => promptNewFolder('')">
        <template #icon><n-icon><FolderIcon /></n-icon></template>
      </n-button>
      <n-dropdown
        trigger="click"
        :options="importMenuOptions"
        @select="onImportMenuSelect"
      >
        <n-button size="small" quaternary :title="'导入'">
          <template #icon><n-icon><ImportIcon /></n-icon></template>
        </n-button>
      </n-dropdown>
      <n-button size="small" quaternary :title="'刷新'" @click="handleRefresh">
        <template #icon><n-icon><RefreshIcon /></n-icon></template>
      </n-button>
    </div>

    <div class="script-search">
      <n-input
        v-model:value="search"
        size="small"
        placeholder="搜索脚本 (名称/内容)"
        clearable
      >
        <template #prefix><n-icon><SearchIcon /></n-icon></template>
      </n-input>
    </div>

    <n-empty
      v-if="scriptStore.loaded && scriptStore.scripts.length === 0 && scriptStore.folders.length === 0"
      description="还没有保存的脚本"
      size="small"
      style="padding: 24px"
    />
    <n-tree
      v-else
      :data="treeData"
      :expanded-keys="expandedKeys"
      block-line
      selectable
      :node-props="({ option }: any) => {
        const node = option as TreeNode;
        return {
          onDblclick: () => onNodeDblClick(node),
          onContextmenu: (e: MouseEvent) => onCtxMenu(e, node),
        };
      }"
      @update:expanded-keys="(keys: string[]) => (expandedKeys = keys)"
    />

    <n-dropdown
      trigger="manual"
      placement="bottom-start"
      :show="showCtx"
      :options="ctxOptions"
      :x="ctxX"
      :y="ctxY"
      @select="handleCtxSelect"
      @clickoutside="showCtx = false"
    />
  </div>
</template>

<style scoped>
.script-tree {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.script-toolbar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px 8px;
  border-bottom: 1px solid #eee;
}
.script-search {
  padding: 6px 8px;
  border-bottom: 1px solid #eee;
}
.n-tree {
  flex: 1;
  overflow: auto;
}
</style>

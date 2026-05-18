/**
 * AI agent 工具层
 *
 * 后端 `ai_agent_turn` 只做一次模型往返; agent 循环跑在前端 (stores/ai.ts),
 * 因为大部分工具是操作 UI 的 (连接、编辑器标签页、脚本库...).
 *
 * executeTool 不接收冻结的 context —— 每次调用都从 Pinia store **实时**读当前状态,
 * 这样 agent 在一轮里先 open_query_tab / switch_query_tab, 后续工具能看到最新状态.
 */
import { invoke } from "@tauri-apps/api/core";
import * as aiApi from "@/api/ai";
import { useEditorStore } from "@/stores/editor";
import { useConnectionStore } from "@/stores/connection";
import { useDatabaseStore } from "@/stores/database";
import { useScriptStore } from "@/stores/script";
import type { ToolDef } from "@/types/ai";

/** 传给模型的工具定义 */
export const AGENT_TOOLS: ToolDef[] = [
  // ---- 与用户交互 ----
  {
    name: "ask_user",
    description:
      "当用户的指令不清晰、有歧义、或存在多个合理选择时, 用它向用户提问、让用户来定 —— " +
      "**不要自己随便猜一个**。典型场景: 用户说“查下数据”但没说哪个集合 / 说“连数据库”但有多个连接可选 / " +
      "执行写操作或有风险的操作前先确认。给 2-4 个具体选项, 用户会选一个 (也可能自己输入答案)。",
    inputSchema: {
      type: "object",
      properties: {
        question: { type: "string", description: "要问用户的问题, 简洁明确" },
        options: {
          type: "array",
          items: { type: "string" },
          description: "2-4 个候选答案, 让用户挑",
        },
      },
      required: ["question", "options"],
    },
  },
  // ---- 连接 ----
  {
    name: "list_connections",
    description: "列出所有已保存的 MongoDB 连接及其是否已打开。",
    inputSchema: { type: "object", properties: {}, required: [] },
  },
  {
    name: "open_connection",
    description: "打开 (连接) 一个已保存的连接。执行查询前连接必须是打开状态。",
    inputSchema: {
      type: "object",
      properties: {
        connection: { type: "string", description: "连接的 id 或名称" },
      },
      required: ["connection"],
    },
  },
  // ---- 浏览 ----
  {
    name: "list_databases",
    description: "列出某个连接下的所有数据库。不传 connection 则用当前标签页绑定的连接。",
    inputSchema: {
      type: "object",
      properties: { connection: { type: "string", description: "连接 id 或名称, 可省略" } },
      required: [],
    },
  },
  {
    name: "list_collections",
    description: "列出某个数据库下的所有集合。不传则用当前标签页绑定的连接/库。",
    inputSchema: {
      type: "object",
      properties: {
        connection: { type: "string", description: "连接 id 或名称, 可省略" },
        database: { type: "string", description: "数据库名, 可省略" },
      },
      required: [],
    },
  },
  {
    name: "get_schema",
    description: "分析一个集合的字段结构 (字段名 / BSON 类型 / 出现率)。写查询前用它了解数据形状。",
    inputSchema: {
      type: "object",
      properties: {
        collection: { type: "string", description: "集合名; 不传则用当前标签页绑定的集合" },
      },
      required: [],
    },
  },
  // ---- 编辑器标签页 ----
  {
    name: "get_editor_content",
    description: "读取当前激活的编辑器标签页内容及其绑定的连接/库/集合。改编辑器前先了解现状。",
    inputSchema: { type: "object", properties: {}, required: [] },
  },
  {
    name: "get_editor_selection",
    description:
      '读取用户当前在编辑器里**选中**的代码片段。用户说"分析选中的代码/这段/解释一下"时用它。' +
      "没有选中内容时会提示。",
    inputSchema: { type: "object", properties: {}, required: [] },
  },
  {
    name: "propose_editor_edit",
    description:
      "把当前激活标签页的内容替换成新内容。不会立即生效 —— 会以 diff 形式展示, 用户点 Accept 才应用。" +
      "**这是你写查询/脚本的唯一方式** —— 你不能直接执行查询或写操作, 要让用户跑某条查询时, " +
      "就用它把查询写进编辑器, 用户确认 Accept 后自己点 Run 运行。也用于修复语法、重构。" +
      "content 必须是完整的新内容, 不是 diff 片段。",
    inputSchema: {
      type: "object",
      properties: {
        content: { type: "string", description: "编辑器的完整新内容" },
        explanation: { type: "string", description: "一句话说明你改了什么" },
      },
      required: ["content"],
    },
  },
  {
    name: "list_query_tabs",
    description: "列出所有打开的查询标签页 (id / 标题 / 绑定的库和集合 / 哪个是当前激活的)。",
    inputSchema: { type: "object", properties: {}, required: [] },
  },
  {
    name: "open_query_tab",
    description: "打开一个新的查询标签页并切到它。可指定连接/库/集合, 也可直接塞入初始内容。",
    inputSchema: {
      type: "object",
      properties: {
        connection: { type: "string", description: "连接 id 或名称, 可省略 (默认当前连接)" },
        database: { type: "string", description: "数据库名, 可省略" },
        collection: { type: "string", description: "集合名, 可省略" },
        content: { type: "string", description: "标签页的初始内容, 可省略" },
      },
      required: [],
    },
  },
  {
    name: "switch_query_tab",
    description: "切换到指定 id 的查询标签页 (id 从 list_query_tabs 获取)。",
    inputSchema: {
      type: "object",
      properties: { tabId: { type: "string", description: "目标标签页 id" } },
      required: ["tabId"],
    },
  },
  {
    name: "set_active_context",
    description:
      "设置当前激活标签页绑定的连接/数据库/集合。get_schema、write_query 都作用在这个上下文上。" +
      "换库前确保连接已 open_connection。",
    inputSchema: {
      type: "object",
      properties: {
        connection: { type: "string", description: "连接 id 或名称, 可省略" },
        database: { type: "string", description: "数据库名, 可省略" },
        collection: { type: "string", description: "集合名, 可省略" },
      },
      required: [],
    },
  },
  // ---- 查询 ----
  {
    name: "write_query",
    description:
      "把一条查询/语句**直接写进编辑器的代码输入框**, 让用户决定要不要运行。" +
      "**你不能自己执行查询** —— 写进去后, 由用户自己点 Run 按钮执行 (或不执行)。" +
      "当前标签页为空就写进去, 已有内容则自动新开一个标签页, 不会覆盖用户的东西。",
    inputSchema: {
      type: "object",
      properties: {
        query: {
          type: "string",
          description: "完整的 mongosh 语句, 如 db.user.find({ phone: '13900000000' })",
        },
      },
      required: ["query"],
    },
  },
  // ---- 脚本库 ----
  {
    name: "list_scripts",
    description: "扫描脚本库, 列出所有已保存的脚本 (文件夹/脚本名)。写脚本前可参考已有的。",
    inputSchema: { type: "object", properties: {}, required: [] },
  },
  {
    name: "get_script",
    description:
      "读取脚本库里某个脚本的完整内容。ref 是 list_scripts 给出的 文件夹/脚本名 或裸脚本名。",
    inputSchema: {
      type: "object",
      properties: { ref: { type: "string", description: "脚本引用: 文件夹/脚本名 或 脚本名" } },
      required: ["ref"],
    },
  },
];

/** 把 id 或名称解析成连接配置 */
function resolveConn(key: string) {
  const connStore = useConnectionStore();
  return connStore.connections.find((c) => c.id === key || c.name === key);
}

/** 执行一个工具调用, 返回喂回模型的文本结果 (永不抛错, 失败也返回文本) */
export async function executeTool(name: string, input: Record<string, unknown>): Promise<string> {
  const editorStore = useEditorStore();
  const connStore = useConnectionStore();
  const dbStore = useDatabaseStore();
  const scriptStore = useScriptStore();
  const str = (v: unknown) => (v === undefined || v === null ? "" : String(v));

  switch (name) {
    // ---- 连接 ----
    case "list_connections": {
      await connStore.fetchConnections();
      if (connStore.connections.length === 0) return "没有保存的连接。";
      return (
        "已保存的连接:\n" +
        connStore.connections
          .map(
            (c) =>
              `- id=${c.id} | ${c.name} (${c.host}:${c.port}) | ${
                connStore.isActive(c.id) ? "已连接" : "未连接"
              }`,
          )
          .join("\n")
      );
    }

    case "open_connection": {
      const key = str(input.connection);
      const conn = resolveConn(key);
      if (!conn) return `失败: 找不到连接 "${key}"。先用 list_connections 看有哪些。`;
      if (connStore.isActive(conn.id)) return `连接 ${conn.name} 已经是打开状态。`;
      try {
        await connStore.connect(conn);
        return `已连接到 ${conn.name} (${conn.host}:${conn.port})。`;
      } catch (e) {
        return `连接失败: ${e}`;
      }
    }

    // ---- 浏览 ----
    case "list_databases": {
      const conn = input.connection
        ? resolveConn(str(input.connection))
        : connStore.connections.find((c) => c.id === editorStore.activeTab?.connectionId);
      if (!conn) return "失败: 没有指定连接, 当前标签页也没绑定连接。";
      if (!connStore.isActive(conn.id))
        return `失败: 连接 ${conn.name} 未打开, 先 open_connection。`;
      await dbStore.fetchDatabases(conn.id);
      const dbs = dbStore.getDatabases(conn.id);
      if (dbs.length === 0) return `连接 ${conn.name} 下没有数据库。`;
      return (
        `连接 ${conn.name} 的数据库:\n` +
        dbs.map((d) => `- ${d.name} (${d.collectionCount} 个集合)`).join("\n")
      );
    }

    case "list_collections": {
      const conn = input.connection
        ? resolveConn(str(input.connection))
        : connStore.connections.find((c) => c.id === editorStore.activeTab?.connectionId);
      const db = str(input.database) || editorStore.activeTab?.database || "";
      if (!conn || !db) return "失败: 需要连接和数据库 (当前标签页也没绑定)。";
      if (!connStore.isActive(conn.id)) return `失败: 连接 ${conn.name} 未打开。`;
      await dbStore.fetchCollections(conn.id, db);
      const colls = dbStore.getCollections(conn.id, db);
      if (colls.length === 0) return `${db} 下没有集合。`;
      return `${db} 的集合 (${colls.length} 个):\n` + colls.map((c) => `- ${c.name}`).join("\n");
    }

    case "get_schema": {
      const tab = editorStore.activeTab;
      const collection = str(input.collection) || tab?.collection || "";
      if (!tab?.connectionId || !tab?.database) {
        return "失败: 当前标签页还没选择连接或数据库 (用 set_active_context 设置)。";
      }
      if (!collection) return "失败: 没有指定 collection, 当前标签页也没绑定集合。";
      try {
        const schema = await aiApi.analyzeSchema(tab.connectionId, tab.database, collection, 100);
        const lines = schema.fields.map(
          (f) =>
            `  ${f.name}: ${f.fieldTypes.map((t) => t.bsonType).join(" | ")} (${f.occurrencePercent}%)`,
        );
        return [`集合 ${collection} 的结构 (采样 ${schema.sampleCount} 条):`, ...lines].join("\n");
      } catch (e) {
        return `分析失败: ${e}`;
      }
    }

    // ---- 编辑器标签页 ----
    case "get_editor_content": {
      const tab = editorStore.activeTab;
      if (!tab) return "当前没有打开的编辑器标签页 (可以用 open_query_tab 新建一个)。";
      return [
        `标签页 id: ${tab.id}`,
        `连接: ${tab.connectionId || "(未选)"}`,
        `数据库: ${tab.database || "(未选)"}`,
        `集合: ${tab.collection || "(未绑定)"}`,
        "--- 编辑器内容 ---",
        tab.content || "(空)",
      ].join("\n");
    }

    case "get_editor_selection": {
      const tab = editorStore.activeTab;
      if (!tab) return "当前没有打开的编辑器标签页。";
      const sel = editorStore.getSelection(tab.id);
      if (!sel) {
        return "用户当前没有在编辑器里选中任何代码。可以让用户先选中一段, 或用 get_editor_content 看全文。";
      }
      return `用户在编辑器里选中的代码:\n${sel}`;
    }

    case "propose_editor_edit": {
      const content = str(input.content);
      const tab = editorStore.activeTab;
      if (!tab) return "失败: 当前没有打开的编辑器标签页 (先 open_query_tab)。";
      editorStore.proposeEdit(tab.id, content);
      const expl = input.explanation ? ` (${str(input.explanation)})` : "";
      return `已在编辑器里提议修改${expl}, 正在等待用户确认 (Accept / Reject)。`;
    }

    case "list_query_tabs": {
      if (editorStore.tabs.length === 0) return "当前没有打开的查询标签页。";
      return editorStore.tabs
        .map((t) => {
          const active = t.id === editorStore.activeTabId ? " [当前]" : "";
          return `- id=${t.id}${active} | ${t.title} | 库:${t.database || "(无)"} | 集合:${
            t.collection || "(无)"
          }`;
        })
        .join("\n");
    }

    case "open_query_tab": {
      const conn = input.connection ? resolveConn(str(input.connection)) : undefined;
      const connId =
        conn?.id || editorStore.activeTab?.connectionId || connStore.connections[0]?.id || "";
      const tabId = editorStore.createTab(
        connId,
        str(input.database),
        str(input.collection) || undefined,
      );
      if (input.content !== undefined) editorStore.setContent(tabId, str(input.content));
      return `已打开新查询标签页并切到它 (id=${tabId})。`;
    }

    case "switch_query_tab": {
      const tabId = str(input.tabId);
      if (!editorStore.tabs.some((t) => t.id === tabId)) {
        return `失败: 没有 id 为 ${tabId} 的标签页。用 list_query_tabs 查看。`;
      }
      editorStore.activeTabId = tabId;
      return `已切换到标签页 ${tabId}。`;
    }

    case "set_active_context": {
      const tab = editorStore.activeTab;
      if (!tab) return "失败: 没有活跃的编辑器标签页, 先用 open_query_tab。";
      if (input.connection) {
        const conn = resolveConn(str(input.connection));
        if (!conn) return `失败: 找不到连接 "${str(input.connection)}"。`;
        if (!connStore.isActive(conn.id)) {
          return `失败: 连接 ${conn.name} 未打开, 先 open_connection。`;
        }
        tab.connectionId = conn.id;
        tab.database = "";
        tab.collection = "";
      }
      if (input.database) {
        tab.database = str(input.database);
        tab.collection = "";
        if (tab.connectionId) await dbStore.fetchCollections(tab.connectionId, tab.database);
      }
      if (input.collection) tab.collection = str(input.collection);
      return `已设置当前标签页: 连接=${tab.connectionId || "(无)"}, 库=${
        tab.database || "(无)"
      }, 集合=${tab.collection || "(无)"}。`;
    }

    // ---- 查询 ----
    case "write_query": {
      const query = str(input.query).trim();
      if (!query) return "失败: query 为空。";
      const tab = editorStore.activeTab;
      if (!tab) return "失败: 没有打开的编辑器标签页, 先用 open_query_tab。";
      if (tab.content.trim()) {
        // 当前标签页已有内容 → 另开一个, 不覆盖用户的东西
        const newId = editorStore.createTab(tab.connectionId, tab.database, tab.collection);
        editorStore.setContent(newId, query);
        return "当前标签页已有内容, 已新开一个标签页写入查询。请用户点 Run 执行 (执行与否由用户决定)。";
      }
      editorStore.setContent(tab.id, query);
      return "已把查询写进当前编辑器标签页。请用户确认后点 Run 执行 (执行与否由用户决定)。";
    }

    // ---- 脚本库 ----
    case "list_scripts": {
      try {
        await scriptStore.refresh();
      } catch (e) {
        return `读取脚本库失败: ${e}`;
      }
      if (scriptStore.scripts.length === 0) return "脚本库为空。";
      return (
        "脚本库:\n" +
        scriptStore.scripts
          .map((s) => `- ${s.folderPath ? `${s.folderPath}/${s.name}` : s.name}`)
          .join("\n")
      );
    }

    case "get_script": {
      const ref = str(input.ref);
      if (!ref) return "失败: 需要 ref (文件夹/脚本名 或 裸脚本名)。";
      try {
        const content = await invoke<string>("resolve_script_ref", { reference: ref });
        return `脚本 ${ref} 的内容:\n${content.slice(0, 6000)}`;
      } catch (e) {
        return `读取失败: ${e}`;
      }
    }

    default:
      return `未知工具: ${name}`;
  }
}

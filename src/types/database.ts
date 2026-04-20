export interface DatabaseInfo {
  name: string;
  sizeOnDisk: number;
  empty: boolean;
  collectionCount: number;
}

export interface CollectionInfo {
  name: string;
  collectionType: string;
  count: number;
  size: number;
}

export interface QueryResult {
  documents: Record<string, unknown>[];
  /** 本次返回的文档数 */
  count: number;
  /** 匹配条件的总文档数（不受 limit 限制） */
  totalCount: number;
  executionTimeMs: number;
}

export interface HistoryEntry {
  id: number;
  connectionId: string;
  databaseName: string;
  collectionName: string | null;
  queryText: string;
  queryType: string;
  executionTimeMs: number | null;
  resultCount: number | null;
  errorMessage: string | null;
  createdAt: string;
}

export type ResultTabKind = "find" | "explain";

/** 一次查询执行产生的结果 tab —— 承载所有执行态数据.
 *  每次 Run / Explain 追加一个, 每个编辑器 tab 内最多保留 10 个 (FIFO 淘汰). */
export interface ResultTab {
  id: string;
  kind: ResultTabKind;
  /** 显示标题: "Find" / "Find (2)" / "Explain" / "Explain (3)" */
  title: string;
  /** 产生该结果的查询文本 (用于翻页重放) */
  queryText: string;
  result: QueryResult | null;
  /** Explain 原始 executionStats JSON (kind === "explain" 时用) */
  explainResult: Record<string, unknown> | null;
  error: string | null;
  loading: boolean;
  /** 当前在途查询的 UUID —— 匹配 `query:count-ready` 事件 */
  currentQueryId: string | null;
  currentPage: number;
  pageSize: number;
  /** 创建时间戳 (ms) —— 排序用 */
  createdAt: number;
  /** 用户点 Stop 后置 true, 后续 await 回来的结果会被丢弃 (后端查询无法真正取消,
   *  但 UI 立刻停止转圈). */
  aborted: boolean;
}

export interface EditorTab {
  id: string;
  title: string;
  connectionId: string;
  database: string;
  collection: string;
  content: string;
  /** 结果 tab 列表 (Find/Explain). 上限 10, 超限淘汰最早一个. */
  resultTabs: ResultTab[];
  activeResultTabId: string | null;
}

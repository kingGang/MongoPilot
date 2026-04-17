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

export interface EditorTab {
  id: string;
  title: string;
  connectionId: string;
  database: string;
  collection: string;
  content: string;
  result: QueryResult | null;
  error: string | null;
  loading: boolean;
  /** 最后执行的查询文本（用于翻页时重放） */
  lastQueryText: string;
  /** 当前页码（后端分页） */
  currentPage: number;
  /** 每页大小 */
  pageSize: number;
}

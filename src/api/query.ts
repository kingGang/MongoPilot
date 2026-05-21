import { invoke } from "@tauri-apps/api/core";
import type { QueryResult, HistoryEntry } from "@/types/database";

export async function runQuery(
  connectionId: string,
  database: string,
  queryText: string,
  skip?: number,
  pageSize?: number,
  queryId?: string,
): Promise<QueryResult> {
  return invoke<QueryResult>("run_query", {
    request: { connectionId, database, queryText, skip, pageSize, queryId },
  });
}

export async function getQueryHistory(
  connectionId: string,
  limit?: number,
  offset?: number,
): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>("get_query_history", { connectionId, limit, offset });
}

export async function searchQueryHistory(
  connectionId: string,
  keyword: string,
): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>("search_query_history", { connectionId, keyword });
}

export async function clearQueryHistory(connectionId: string): Promise<void> {
  return invoke("clear_query_history", { connectionId });
}

/** 拉取所有连接的执行记录, 前端按 connectionId 分组展示 */
export async function listAllQueryHistory(limit = 500): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>("list_all_query_history", { limit });
}

/** 清空所有连接的执行记录 */
export async function clearAllQueryHistory(): Promise<void> {
  return invoke("clear_all_query_history");
}

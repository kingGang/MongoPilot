import { invoke } from "./invoke";
import type { DatabaseInfo, CollectionInfo } from "@/types/database";

export async function listDatabases(connectionId: string): Promise<DatabaseInfo[]> {
  return invoke<DatabaseInfo[]>("list_databases", { connectionId });
}

/** 后台并行取所有 DB 的集合数, 用于 listDatabases 之后异步补全 collectionCount */
export async function countDatabaseCollections(
  connectionId: string,
): Promise<Record<string, number>> {
  return invoke<Record<string, number>>("count_database_collections", { connectionId });
}

export async function listCollections(
  connectionId: string,
  database: string,
): Promise<CollectionInfo[]> {
  return invoke<CollectionInfo[]>("list_collections", { connectionId, database });
}

export async function dropDatabase(connectionId: string, database: string): Promise<void> {
  return invoke("drop_database", { connectionId, database });
}

export interface RenameDatabaseSummary {
  copiedCollections: number;
  copiedDocuments: number;
}

/**
 * 重命名数据库. MongoDB 无原生命令, 后端把旧库所有集合(文档+索引)拷到新库名再删旧库.
 * 大库会较慢 —— 本质是一次数据拷贝.
 */
export async function renameDatabase(
  connectionId: string,
  oldName: string,
  newName: string,
): Promise<RenameDatabaseSummary> {
  return invoke<RenameDatabaseSummary>("rename_database", { connectionId, oldName, newName });
}

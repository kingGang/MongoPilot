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

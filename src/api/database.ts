import { invoke } from "@tauri-apps/api/core";
import type { DatabaseInfo, CollectionInfo } from "@/types/database";

export async function listDatabases(connectionId: string): Promise<DatabaseInfo[]> {
  return invoke<DatabaseInfo[]>("list_databases", { connectionId });
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

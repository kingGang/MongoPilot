import { invoke } from "@tauri-apps/api/core";
import type { CollectionStats, IndexInfo, CreateIndexOptions } from "@/types/document";

export async function createCollection(
  connectionId: string,
  database: string,
  collectionName: string,
): Promise<void> {
  return invoke("create_collection", { connectionId, database, collectionName });
}

export async function dropCollection(
  connectionId: string,
  database: string,
  collectionName: string,
): Promise<void> {
  return invoke("drop_collection", { connectionId, database, collectionName });
}

export async function getCollectionStats(
  connectionId: string,
  database: string,
  collectionName: string,
): Promise<CollectionStats> {
  return invoke<CollectionStats>("get_collection_stats", {
    connectionId,
    database,
    collectionName,
  });
}

export async function listIndexes(
  connectionId: string,
  database: string,
  collectionName: string,
): Promise<IndexInfo[]> {
  return invoke<IndexInfo[]>("list_indexes", { connectionId, database, collectionName });
}

export async function createIndex(
  connectionId: string,
  database: string,
  collectionName: string,
  keys: Record<string, number>,
  options?: CreateIndexOptions,
): Promise<string> {
  return invoke<string>("create_index", { connectionId, database, collectionName, keys, options });
}

export async function dropIndex(
  connectionId: string,
  database: string,
  collectionName: string,
  indexName: string,
): Promise<void> {
  return invoke("drop_index", { connectionId, database, collectionName, indexName });
}

import { invoke } from "./invoke";
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
  /** key 的值: 1 升序, -1 降序, "text" / "hashed" / "2d" / "2dsphere" 等特殊索引 */
  keys: Record<string, number | string>,
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

/** 重建集合所有索引 (db.runCommand({reIndex: collName})). 仅单节点 mongod 支持. */
export async function reIndex(
  connectionId: string,
  database: string,
  collectionName: string,
): Promise<void> {
  return invoke("re_index", { connectionId, database, collectionName });
}

/**
 * 查询单个索引的详细信息 (定义 + indexSize + indexDetails + $indexStats 用量).
 * 等效于前端那段 getCollectionIndexInfo(collection, indexName) 脚本.
 */
export async function getIndexInfo(
  connectionId: string,
  database: string,
  collectionName: string,
  indexName: string,
): Promise<Record<string, unknown>> {
  return invoke<Record<string, unknown>>("get_index_info", {
    connectionId,
    database,
    collectionName,
    indexName,
  });
}

/**
 * 列出某集合所有索引的汇总信息: 每条索引包含 name/key/type/size/ns/accesses/usage stats/properties/v/host.
 * 等效于前端那段 getCollectionIndexes(col) 脚本.
 */
export async function getCollectionIndexes(
  connectionId: string,
  database: string,
  collectionName: string,
): Promise<Record<string, unknown>[]> {
  return invoke<Record<string, unknown>[]>("get_collection_indexes", {
    connectionId,
    database,
    collectionName,
  });
}

import { invoke } from "./invoke";

export async function insertDocument(
  connectionId: string,
  database: string,
  collection: string,
  document: Record<string, unknown>,
): Promise<string> {
  return invoke<string>("insert_document", { connectionId, database, collection, document });
}

export async function updateDocument(
  connectionId: string,
  database: string,
  collection: string,
  id: string,
  document: Record<string, unknown>,
): Promise<void> {
  return invoke("update_document", { connectionId, database, collection, id, document });
}

/** 只更新被改的字段: 后端执行 `updateOne({_id}, {$set: fields})` */
export async function setDocumentFields(
  connectionId: string,
  database: string,
  collection: string,
  id: string,
  fields: Record<string, unknown>,
): Promise<void> {
  return invoke("set_document_fields", { connectionId, database, collection, id, fields });
}

export async function deleteDocument(
  connectionId: string,
  database: string,
  collection: string,
  id: string,
): Promise<void> {
  return invoke("delete_document", { connectionId, database, collection, id });
}

export async function deleteDocuments(
  connectionId: string,
  database: string,
  collection: string,
  filter: Record<string, unknown>,
): Promise<number> {
  return invoke<number>("delete_documents", { connectionId, database, collection, filter });
}

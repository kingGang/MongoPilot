import { getBsonType } from "./bson-format";

/** 单值 → shell 风格字面量 (ObjectId / ISODate / NumberLong / NumberDecimal 等) */
export function toShellValue(val: unknown): string {
  if (val === null || val === undefined) return "null";
  const type = getBsonType(val);
  const obj = val as Record<string, unknown>;
  switch (type) {
    case "ObjectId":
      return `ObjectId("${obj.$oid ?? val}")`;
    case "Date": {
      const d = obj.$date;
      if (typeof d === "string") return `ISODate("${d}")`;
      if (typeof d === "object" && d && (d as Record<string, unknown>).$numberLong) {
        return `ISODate("${new Date(parseInt(String((d as Record<string, unknown>).$numberLong))).toISOString()}")`;
      }
      if (typeof d === "number") return `ISODate("${new Date(d).toISOString()}")`;
      return `ISODate("${d}")`;
    }
    case "Int64":
      return `NumberLong("${obj.$numberLong ?? val}")`;
    case "Decimal128":
      return `NumberDecimal("${obj.$numberDecimal ?? val}")`;
    case "String":
      return `"${String(val).replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
    case "Boolean":
      return String(val);
    case "Int32":
      if (typeof val === "object" && obj.$numberInt) return String(obj.$numberInt);
      return String(val);
    case "Double":
      // mongosh 风格: Double(x) 显式标注, 避免和 Int 混淆
      return `Double("${val}")`;
    default:
      return String(val);
  }
}

/** 递归把值序列化为 shell 字符串 (Document / Array 换行缩进) */
export function toShellString(val: unknown, indent: number): string {
  const pad = "    ".repeat(indent);
  const pad1 = "    ".repeat(indent + 1);
  if (val === null || val === undefined) return "null";
  const type = getBsonType(val);
  if (type !== "Document" && type !== "Array") return toShellValue(val);
  if (Array.isArray(val)) {
    if (val.length === 0) return "[]";
    return `[\n${val.map((item) => `${pad1}${toShellString(item, indent + 1)}`).join(",\n")}\n${pad}]`;
  }
  const entries = Object.entries(val as Record<string, unknown>);
  if (entries.length === 0) return "{}";
  const lines = entries.map(([k, v]) => {
    const ks = /[^a-zA-Z0-9_$]/.test(k) ? `"${k}"` : `"${k}"`;
    return `${pad1}${ks}: ${toShellString(v, indent + 1)}`;
  });
  return `{\n${lines.join(",\n")}\n${pad}}`;
}

/**
 * 按照 mongosh 风格拼 `db.<coll>.updateOne({_id:...}, {$set: {...}})`.
 * `_id` 作为 filter, 其它字段进入 `$set`.
 */
export function buildUpdateOneQuery(collection: string, doc: Record<string, unknown>): string {
  const id = doc._id;
  const rest: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(doc)) {
    if (k === "_id") continue;
    rest[k] = v;
  }

  const collRef = collection.includes(".")
    ? `db.getCollection("${collection}")`
    : `db.${collection}`;
  const filter =
    id !== undefined
      ? `{ _id: ${toShellValue(id)} }`
      : `{ /* TODO: 填写 filter —— 该文档无 _id */ }`;
  const setBody = toShellString(rest, 1);

  return `${collRef}.updateOne(${filter}, {\n    $set: ${setBody}\n})`;
}

/** `db.<coll>` 或 `db.getCollection("<coll>")` (集合名带点时用后者) */
function collRefOf(collection: string): string {
  return collection.includes(".") ? `db.getCollection("${collection}")` : `db.${collection}`;
}

/** `db.<coll>.insertOne({...})` —— 整个文档作为插入体, 去掉 _id 让服务端生成新的 */
export function buildInsertOneQuery(collection: string, doc: Record<string, unknown>): string {
  const body: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(doc)) {
    if (k === "_id") continue;
    body[k] = v;
  }
  return `${collRefOf(collection)}.insertOne(${toShellString(body, 0)})`;
}

/** `db.<coll>.deleteOne({_id: ...})` —— 按 _id 删除该文档 */
export function buildDeleteOneQuery(collection: string, doc: Record<string, unknown>): string {
  const id = doc._id;
  const filter =
    id !== undefined
      ? `{ _id: ${toShellValue(id)} }`
      : `{ /* TODO: 填写 filter —— 该文档无 _id */ }`;
  return `${collRefOf(collection)}.deleteOne(${filter})`;
}

/** `db.<coll>.find({_id: ...})` —— 按 _id 查回该文档 */
export function buildFindByIdQuery(collection: string, doc: Record<string, unknown>): string {
  const id = doc._id;
  const filter =
    id !== undefined
      ? `{ _id: ${toShellValue(id)} }`
      : `{ /* TODO: 填写 filter —— 该文档无 _id */ }`;
  return `${collRefOf(collection)}.find(${filter})`;
}

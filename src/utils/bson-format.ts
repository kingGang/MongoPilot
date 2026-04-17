/**
 * BSON 值类型识别、格式化和颜色——供 TreeDocView / TableView / JsonTreeView 共用。
 */

export function getBsonType(val: unknown): string {
  if (val === null || val === undefined) return "Null";
  if (Array.isArray(val)) return "Array";
  if (typeof val === "object") {
    const obj = val as Record<string, unknown>;
    if (obj.$oid) return "ObjectId";
    if (obj.$date) return "Date";
    if (obj.$numberLong) return "Int64";
    if (obj.$numberInt) return "Int32";
    if (obj.$numberDecimal) return "Decimal128";
    if (obj.$binary) return "Binary";
    if (obj.$regex || obj.$regularExpression) return "Regex";
    if (obj.$timestamp) return "Timestamp";
    return "Document";
  }
  if (typeof val === "string") return "String";
  if (typeof val === "number") return Number.isInteger(val) ? "Int32" : "Double";
  if (typeof val === "boolean") return "Boolean";
  return "Unknown";
}

// ---- Date ----

function parseDate(val: unknown): Date | null {
  if (typeof val === "string") {
    const d = new Date(val);
    return isNaN(d.getTime()) ? null : d;
  }
  if (typeof val !== "object" || val === null) return null;
  const obj = val as Record<string, unknown>;
  const d = obj.$date;
  if (typeof d === "string") {
    const parsed = new Date(d);
    return isNaN(parsed.getTime()) ? null : parsed;
  }
  if (typeof d === "object" && d && (d as Record<string, unknown>).$numberLong) {
    return new Date(parseInt(String((d as Record<string, unknown>).$numberLong)));
  }
  if (typeof d === "number") return new Date(d);
  return null;
}

function formatDateStr(date: Date): string {
  const y = date.getFullYear();
  const m = date.getMonth() + 1;
  const d = date.getDate();
  const hh = String(date.getHours()).padStart(2, "0");
  const mm = String(date.getMinutes()).padStart(2, "0");
  const ss = String(date.getSeconds()).padStart(2, "0");
  return `${y}/${m}/${d} ${hh}:${mm}:${ss}`;
}

function relativeTime(date: Date): string {
  const diff = Date.now() - date.getTime();
  const sec = Math.floor(Math.abs(diff) / 1000);
  const prefix = diff < 0 ? "in " : "";
  const suffix = diff >= 0 ? " ago" : "";
  if (sec < 60) return diff >= 0 ? "just now" : "in a moment";
  const min = Math.floor(sec / 60);
  if (min < 60) return `${prefix}${min} min${min > 1 ? "s" : ""}${suffix}`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${prefix}${hr} hour${hr > 1 ? "s" : ""}${suffix}`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${prefix}${day} day${day > 1 ? "s" : ""}${suffix}`;
  const mon = Math.floor(day / 30);
  if (mon < 12) return `${prefix}${mon} month${mon > 1 ? "s" : ""}${suffix}`;
  const yr = Math.floor(mon / 12);
  return `${prefix}${yr} year${yr > 1 ? "s" : ""}${suffix}`;
}

// ---- ObjectId ----

function formatOid(val: unknown): string {
  if (typeof val === "string") return `ObjectId("${val}")`;
  if (typeof val === "object" && val !== null) {
    const oid = (val as Record<string, unknown>).$oid;
    if (oid) return `ObjectId("${oid}")`;
  }
  return String(val);
}

// ---- 统一格式化 ----

/** 将任意 BSON 值格式化为人类可读字符串 */
export function formatBsonValue(val: unknown, type?: string): string {
  const t = type ?? getBsonType(val);
  if (val === null || val === undefined) return "null";

  switch (t) {
    case "ObjectId":
      return formatOid(val);
    case "Date": {
      const date = parseDate(val);
      if (date) return `${formatDateStr(date)} — ${relativeTime(date)}`;
      return String((val as Record<string, unknown>).$date ?? val);
    }
    case "Int64":
      return String((val as Record<string, unknown>).$numberLong ?? val);
    case "Int32":
      if (typeof val === "object" && val !== null) {
        return String((val as Record<string, unknown>).$numberInt ?? val);
      }
      return String(val);
    case "Decimal128":
      return String((val as Record<string, unknown>).$numberDecimal ?? val);
    case "Boolean":
      return String(val);
    case "Double":
      return String(val);
    case "String":
      return `"${val}"`;
    case "Document": {
      const keys = Object.keys(val as object);
      return `{ ${keys.length} fields }`;
    }
    case "Array":
      return `[ ${(val as unknown[]).length} elements ]`;
    case "Binary":
      return "[Binary]";
    case "Regex": {
      const obj = val as Record<string, unknown>;
      const re = obj.$regex ?? (obj.$regularExpression as Record<string, unknown>)?.pattern;
      return `/${re}/`;
    }
    case "Timestamp": {
      const obj = val as Record<string, unknown>;
      const ts = obj.$timestamp as Record<string, unknown> | undefined;
      if (ts) return `Timestamp(${ts.t}, ${ts.i})`;
      return String(val);
    }
    case "Null":
      return "null";
    default:
      return String(val);
  }
}

/** 用于 Table 单元格的简洁格式 (不带引号和展开提示) */
export function formatBsonCell(val: unknown): string {
  if (val === null || val === undefined) return "";
  const t = getBsonType(val);
  switch (t) {
    case "ObjectId":
      return formatOid(val);
    case "Date": {
      const date = parseDate(val);
      if (date) return `${formatDateStr(date)} — ${relativeTime(date)}`;
      return String((val as Record<string, unknown>).$date ?? val);
    }
    case "Int64":
      return String((val as Record<string, unknown>).$numberLong ?? val);
    case "Decimal128":
      return String((val as Record<string, unknown>).$numberDecimal ?? val);
    case "Document":
    case "Array":
      return JSON.stringify(val);
    case "String":
      return val as string;
    default:
      return String(val);
  }
}

/** 类型对应的颜色 */
export function getTypeColor(type: string): string {
  switch (type) {
    case "ObjectId":
      return "#c678dd";
    case "String":
      return "#98c379";
    case "Int32":
    case "Int64":
    case "Double":
    case "Decimal128":
      return "#d19a66";
    case "Boolean":
      return "#56b6c2";
    case "Date":
      return "#e5c07b";
    case "Null":
      return "#999";
    case "Document":
    case "Array":
      return "#61afef";
    case "Binary":
      return "#be5046";
    case "Regex":
      return "#e06c75";
    case "Timestamp":
      return "#e5c07b";
    default:
      return "#abb2bf";
  }
}

/** Tree 视图专用：不加引号，OID 直接显示 hex */
export function formatTreeValue(val: unknown, type?: string): string {
  const t = type ?? getBsonType(val);
  if (val === null || val === undefined) return "null";

  switch (t) {
    case "ObjectId": {
      if (typeof val === "object" && val !== null) {
        return String((val as Record<string, unknown>).$oid ?? val);
      }
      return String(val);
    }
    case "Date": {
      const date = parseDate(val);
      if (date) return `${formatDateStr(date)} — ${relativeTime(date)}`;
      return String((val as Record<string, unknown>).$date ?? val);
    }
    case "Int64":
      return String((val as Record<string, unknown>).$numberLong ?? val);
    case "Int32":
      if (typeof val === "object" && val !== null) {
        return String((val as Record<string, unknown>).$numberInt ?? val);
      }
      return String(val);
    case "Decimal128":
      return String((val as Record<string, unknown>).$numberDecimal ?? val);
    case "Boolean":
      return String(val);
    case "Double":
      return String(val);
    case "String":
      return val as string;
    case "Document": {
      const keys = Object.keys(val as object);
      return `{ ${keys.length} fields }`;
    }
    case "Array":
      return `[ ${(val as unknown[]).length} elements ]`;
    case "Null":
      return "null";
    default:
      return formatBsonValue(val, t);
  }
}

/** 从文档中提取 _id 的显示文本 */
export function extractIdDisplay(doc: Record<string, unknown>): string {
  const id = doc._id;
  if (!id) return "";
  if (typeof id === "object" && id !== null) {
    const oid = (id as Record<string, unknown>).$oid;
    if (oid) return String(oid);
    return JSON.stringify(id);
  }
  return String(id);
}

/** 值文本对应的 CSS 颜色 */
export function getValueColor(type: string): string {
  switch (type) {
    case "String":
      return "#98c379";
    case "Int32":
    case "Int64":
    case "Double":
    case "Decimal128":
      return "#d19a66";
    case "Boolean":
      return "#56b6c2";
    case "ObjectId":
      return "#c678dd";
    case "Date":
      return "#e5c07b";
    case "Null":
      return "#999";
    default:
      return "";
  }
}

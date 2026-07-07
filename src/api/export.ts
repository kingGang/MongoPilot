import { invoke } from "./invoke";
import * as XLSX from "xlsx";

export type ExportFormat =
  | "mongoshell" // MongoShell BSON (.json)
  | "ejson" // mongoexport EJSON v2 (.json)
  | "simple-json" // Simple JSON Text (.json)
  | "csv" // CSV (.csv)
  | "sql" // SQL (.sql)
  | "html" // HTML Table (.html)
  | "txt" // UTF-16 Unicode Text (.txt)
  | "jsonl" // JSON Lines (.jsonl)
  | "xlsx"; // Excel 2007+ (.xlsx)

export interface FormatInfo {
  label: string;
  description: string;
  ext: string;
  value: ExportFormat;
}

export const FORMAT_LIST: FormatInfo[] = [
  {
    value: "mongoshell",
    label: "MongoShell BSON (.json)",
    description: "MongoDB Shell 使用的无损 JSON 格式，包含 ObjectId()、ISODate() 等类型",
    ext: "json",
  },
  {
    value: "ejson",
    label: "mongoexport EJSON v2 (.json)",
    description: "mongoexport 工具生成的类型保留 EJSON v2 格式，如 {$oid: ...}",
    ext: "json",
  },
  {
    value: "simple-json",
    label: "Simple JSON Text (.json)",
    description: "简单易读的纯 JSON 文本，所有特殊类型转为普通值",
    ext: "json",
  },
  {
    value: "csv",
    label: "CSV (.csv)",
    description: "逗号分隔值文件，可用 Excel 或其他表格软件打开",
    ext: "csv",
  },
  {
    value: "sql",
    label: "SQL (.sql)",
    description: "SQL INSERT 语句，可导入到关系型数据库",
    ext: "sql",
  },
  {
    value: "jsonl",
    label: "JSON Lines (.jsonl)",
    description: "每行一个 JSON 对象，兼容 mongoimport 导入",
    ext: "jsonl",
  },
  {
    value: "txt",
    label: "UTF-16 Unicode Text (.txt)",
    description: "Tab 分隔的 UTF-16 文本文件",
    ext: "txt",
  },
  {
    value: "xlsx",
    label: "Excel 2007+ XML Formats (.xlsx)",
    description: "Microsoft Excel 2007+ Open XML 格式，最大行数 1,048,576",
    ext: "xlsx",
  },
  {
    value: "html",
    label: "HTML Table (.html)",
    description: "HTML 表格，可在浏览器中查看",
    ext: "html",
  },
];

// =====================================================================
//  BSON 值转换（3 种模式）
// =====================================================================

/** MongoShell 格式：ObjectId("..."), ISODate("...") */
function toShellValue(val: unknown): unknown {
  if (val === null || val === undefined) return val;
  if (Array.isArray(val)) return val.map(toShellValue);
  if (typeof val === "object") {
    const obj = val as Record<string, unknown>;
    if (obj.$oid) return `__SHELL__ObjectId("${obj.$oid}")`;
    if (obj.$date !== undefined) {
      const iso = dateToIso(obj);
      return `__SHELL__ISODate("${iso}")`;
    }
    if (obj.$numberLong !== undefined) return `__SHELL__NumberLong("${obj.$numberLong}")`;
    if (obj.$numberInt !== undefined) return Number(obj.$numberInt);
    if (obj.$numberDecimal !== undefined) return `__SHELL__NumberDecimal("${obj.$numberDecimal}")`;
    if (obj.$regularExpression && typeof obj.$regularExpression === "object") {
      const re = obj.$regularExpression as Record<string, unknown>;
      return `__SHELL__/${re.pattern ?? ""}/${re.options ?? ""}`;
    }
    const result: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(obj)) result[k] = toShellValue(v);
    return result;
  }
  return val;
}

/** 将 Shell 占位符还原为无引号的函数调用 */
function shellStringify(docs: Record<string, unknown>[]): string {
  const json = JSON.stringify(docs, null, 2);
  return json.replace(/"__SHELL__([^"]*)"/g, "$1");
}

/** EJSON v2 canonical 格式（原样保留 Extended JSON） */
function filterFields(doc: Record<string, unknown>, fields: string[]): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const f of fields) {
    if (f in doc) out[f] = doc[f];
  }
  return out;
}

/** Simple JSON：所有 BSON 类型 → 可读纯值 */
function toSimpleValue(val: unknown): unknown {
  if (val === null || val === undefined) return val;
  if (Array.isArray(val)) return val.map(toSimpleValue);
  if (typeof val === "object") {
    const obj = val as Record<string, unknown>;
    if (obj.$oid && typeof obj.$oid === "string") return obj.$oid;
    if (obj.$date !== undefined) return dateToIso(obj);
    if (obj.$numberLong !== undefined) return String(obj.$numberLong);
    if (obj.$numberInt !== undefined) return Number(obj.$numberInt);
    if (obj.$numberDecimal !== undefined) return String(obj.$numberDecimal);
    if (obj.$binary && typeof obj.$binary === "object") {
      return (obj.$binary as Record<string, unknown>).base64 ?? JSON.stringify(obj.$binary);
    }
    if (obj.$regularExpression && typeof obj.$regularExpression === "object") {
      const re = obj.$regularExpression as Record<string, unknown>;
      return `/${re.pattern ?? ""}/${re.options ?? ""}`;
    }
    if (obj.$timestamp && typeof obj.$timestamp === "object") {
      const ts = obj.$timestamp as Record<string, unknown>;
      return `Timestamp(${ts.t}, ${ts.i})`;
    }
    const result: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(obj)) result[k] = toSimpleValue(v);
    return result;
  }
  return val;
}

function dateToIso(obj: Record<string, unknown>): string {
  const d = obj.$date;
  if (typeof d === "string") return d;
  if (typeof d === "number") return new Date(d).toISOString();
  if (typeof d === "object" && d) {
    const inner = d as Record<string, unknown>;
    if (inner.$numberLong) return new Date(parseInt(String(inner.$numberLong))).toISOString();
  }
  return String(d);
}

/** 将值扁平化为字符串（用于 CSV / SQL / TXT / HTML） */
function flatValue(val: unknown): string {
  if (val === null || val === undefined) return "";
  if (typeof val === "object") return JSON.stringify(val);
  return String(val);
}

// =====================================================================
//  格式化器
// =====================================================================

function fmtMongoShell(docs: Record<string, unknown>[], fields: string[]): string {
  const filtered = docs.map((d) => filterFields(d, fields));
  const shell = filtered.map((d) => toShellValue(d) as Record<string, unknown>);
  return shellStringify(shell);
}

function fmtEjson(docs: Record<string, unknown>[], fields: string[]): string {
  const filtered = docs.map((d) => filterFields(d, fields));
  return JSON.stringify(filtered, null, 2);
}

function fmtSimpleJson(docs: Record<string, unknown>[], fields: string[]): string {
  const filtered = docs.map((d) => filterFields(d, fields));
  const simple = filtered.map((d) => toSimpleValue(d) as Record<string, unknown>);
  return JSON.stringify(simple, null, 2);
}

function fmtJsonl(docs: Record<string, unknown>[], fields: string[]): string {
  const filtered = docs.map((d) => filterFields(d, fields));
  const simple = filtered.map((d) => toSimpleValue(d) as Record<string, unknown>);
  return simple.map((d) => JSON.stringify(d)).join("\n");
}

function fmtCsv(docs: Record<string, unknown>[], fields: string[], delimiter: string): string {
  const simple = docs.map((d) => toSimpleValue(d) as Record<string, unknown>);

  function escape(val: unknown): string {
    const s = flatValue(val);
    if (s.includes(delimiter) || s.includes('"') || s.includes("\n")) {
      return `"${s.replace(/"/g, '""')}"`;
    }
    return s;
  }

  const header = fields.map(escape).join(delimiter);
  const rows = simple.map((doc) => fields.map((f) => escape(doc[f])).join(delimiter));
  return [header, ...rows].join("\n");
}

function fmtSql(docs: Record<string, unknown>[], fields: string[], collection: string): string {
  const table = collection || "collection";
  const simple = docs.map((d) => toSimpleValue(d) as Record<string, unknown>);
  const colList = fields.map((f) => `\`${f}\``).join(", ");

  const lines = simple.map((doc) => {
    const vals = fields.map((f) => {
      const v = doc[f];
      if (v === null || v === undefined) return "NULL";
      if (typeof v === "number") return String(v);
      if (typeof v === "boolean") return v ? "1" : "0";
      const s = typeof v === "object" ? JSON.stringify(v) : String(v);
      return `'${s.replace(/'/g, "''")}'`;
    });
    return `INSERT INTO \`${table}\` (${colList}) VALUES (${vals.join(", ")});`;
  });

  return lines.join("\n");
}

function fmtHtml(docs: Record<string, unknown>[], fields: string[]): string {
  const simple = docs.map((d) => toSimpleValue(d) as Record<string, unknown>);

  function esc(s: string): string {
    return s
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  const ths = fields.map((f) => `      <th>${esc(f)}</th>`).join("\n");
  const rows = simple.map((doc) => {
    const tds = fields.map((f) => `      <td>${esc(flatValue(doc[f]))}</td>`).join("\n");
    return `    <tr>\n${tds}\n    </tr>`;
  });

  return `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>Export</title>
  <style>
    table { border-collapse: collapse; font-family: sans-serif; font-size: 13px; }
    th, td { border: 1px solid #ccc; padding: 4px 8px; text-align: left; }
    th { background: #f0f0f0; font-weight: 600; }
    tr:nth-child(even) { background: #fafafa; }
  </style>
</head>
<body>
  <table>
    <thead>
    <tr>
${ths}
    </tr>
    </thead>
    <tbody>
${rows.join("\n")}
    </tbody>
  </table>
</body>
</html>`;
}

function fmtTxt(docs: Record<string, unknown>[], fields: string[]): string {
  const simple = docs.map((d) => toSimpleValue(d) as Record<string, unknown>);
  const header = fields.join("\t");
  const rows = simple.map((doc) => fields.map((f) => flatValue(doc[f])).join("\t"));
  return [header, ...rows].join("\n");
}

// =====================================================================
//  导出入口
// =====================================================================

function fmtXlsx(docs: Record<string, unknown>[], fields: string[]): Uint8Array {
  const simple = docs.map((d) => toSimpleValue(d) as Record<string, unknown>);
  const rows: unknown[][] = [fields];
  for (const doc of simple) {
    rows.push(
      fields.map((f) => {
        const v = doc[f];
        if (v === null || v === undefined) return "";
        if (typeof v === "object") return JSON.stringify(v);
        return v;
      }),
    );
  }
  const ws = XLSX.utils.aoa_to_sheet(rows);
  const wb = XLSX.utils.book_new();
  XLSX.utils.book_append_sheet(wb, ws, "Export");
  const buf = XLSX.write(wb, { type: "array", bookType: "xlsx" }) as ArrayBuffer;
  return new Uint8Array(buf);
}

export async function exportDocuments(
  docs: Record<string, unknown>[],
  format: ExportFormat,
  fields: string[],
  targetPath: string,
  delimiter = ",",
  collection = "",
): Promise<void> {
  // xlsx 是二进制格式，单独处理
  if (format === "xlsx") {
    const data = fmtXlsx(docs, fields);
    await invoke("write_export_binary", { path: targetPath, data: Array.from(data) });
    return;
  }

  let content: string;

  switch (format) {
    case "mongoshell":
      content = fmtMongoShell(docs, fields);
      break;
    case "ejson":
      content = fmtEjson(docs, fields);
      break;
    case "simple-json":
      content = fmtSimpleJson(docs, fields);
      break;
    case "csv":
      content = fmtCsv(docs, fields, delimiter);
      break;
    case "sql":
      content = fmtSql(docs, fields, collection);
      break;
    case "html":
      content = fmtHtml(docs, fields);
      break;
    case "txt":
      content = fmtTxt(docs, fields);
      break;
    case "jsonl":
      content = fmtJsonl(docs, fields);
      break;
  }

  await invoke("write_export_file", { path: targetPath, content });
}

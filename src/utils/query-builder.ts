/**
 * Visual Query Builder 的数据模型 + shell 文本双向转换.
 * 目前只支持 find 命令及其 projection/sort/skip/limit 链.
 */

export type RuleOp =
  | "equal"
  | "notEqual"
  | "gt"
  | "gte"
  | "lt"
  | "lte"
  | "in"
  | "nin"
  | "regex"
  | "exists"
  | "type";

export const OP_LABELS: Record<RuleOp, string> = {
  equal: "equal",
  notEqual: "not equal",
  gt: ">",
  gte: ">=",
  lt: "<",
  lte: "<=",
  in: "in",
  nin: "not in",
  regex: "regex",
  exists: "exists",
  type: "type",
};

export type ValueType = "String" | "Number" | "Boolean" | "Date" | "ObjectId" | "Null";

export interface Rule {
  kind: "rule";
  id: string;
  field: string;
  op: RuleOp;
  valueType: ValueType;
  value: string; // 总是字符串形式, 根据 valueType 转换
  disabled: boolean;
}

export type GroupLogic = "And" | "Or" | "Not";

export interface Group {
  kind: "group";
  id: string;
  logic: GroupLogic;
  items: Array<Rule | Group>;
}

let _idCounter = 0;
function uid(): string {
  _idCounter++;
  return `n${_idCounter}_${Date.now().toString(36)}`;
}

export function newRule(partial?: Partial<Rule>): Rule {
  return {
    kind: "rule",
    id: uid(),
    field: "",
    op: "equal",
    valueType: "String",
    value: "",
    disabled: false,
    ...partial,
  };
}

export function newGroup(partial?: Partial<Group>): Group {
  return {
    kind: "group",
    id: uid(),
    logic: "And",
    items: [],
    ...partial,
  };
}

// ==================== 生成 shell 字符串 ====================

/** 把用户输入的 value 字符串按类型转成 shell 字面量 */
function renderTypedValue(val: string, type: ValueType): string {
  const t = val.trim();
  switch (type) {
    case "String":
      return JSON.stringify(t);
    case "Number":
      return t === "" ? "0" : t;
    case "Boolean":
      return t.toLowerCase() === "true" ? "true" : "false";
    case "Null":
      return "null";
    case "Date":
      return `ISODate(${JSON.stringify(t)})`;
    case "ObjectId":
      return `ObjectId(${JSON.stringify(t)})`;
  }
}

/** 把 "a,b,c" 拆成对应类型字面量的数组字符串 "[<a>, <b>, <c>]" */
function renderListValue(val: string, type: ValueType): string {
  const parts = val
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  return `[${parts.map((p) => renderTypedValue(p, type)).join(", ")}]`;
}

/** 单条 rule -> shell 片段 (带 field 的完整对象, e.g. `age: { $gt: 18 }`) */
function ruleToShellFragment(r: Rule): string | null {
  if (r.disabled || !r.field.trim()) return null;
  const f = r.field.trim();
  switch (r.op) {
    case "equal":
      return `${f}: ${renderTypedValue(r.value, r.valueType)}`;
    case "notEqual":
      return `${f}: { $ne: ${renderTypedValue(r.value, r.valueType)} }`;
    case "gt":
      return `${f}: { $gt: ${renderTypedValue(r.value, r.valueType)} }`;
    case "gte":
      return `${f}: { $gte: ${renderTypedValue(r.value, r.valueType)} }`;
    case "lt":
      return `${f}: { $lt: ${renderTypedValue(r.value, r.valueType)} }`;
    case "lte":
      return `${f}: { $lte: ${renderTypedValue(r.value, r.valueType)} }`;
    case "in":
      return `${f}: { $in: ${renderListValue(r.value, r.valueType)} }`;
    case "nin":
      return `${f}: { $nin: ${renderListValue(r.value, r.valueType)} }`;
    case "regex":
      return `${f}: { $regex: ${JSON.stringify(r.value)} }`;
    case "exists":
      return `${f}: { $exists: ${r.value.toLowerCase() === "false" ? "false" : "true"} }`;
    case "type":
      return `${f}: { $type: ${JSON.stringify(r.value)} }`;
  }
}

/** group -> `{ ... }` 或 `{ $and: [...] }` / `{ $or: [...] }` / `{ $nor: [...] }` */
function groupToShellObject(g: Group): string {
  const fragments: string[] = [];
  for (const item of g.items) {
    if (item.kind === "rule") {
      const f = ruleToShellFragment(item);
      if (f) fragments.push(`{ ${f} }`);
    } else {
      const sub = groupToShellObject(item);
      if (sub !== "{}") fragments.push(sub);
    }
  }
  if (fragments.length === 0) return "{}";

  if (g.logic === "And") {
    // And: 如果全是单字段平铺, 直接合并 key (不展开 $and); 有嵌套或重复字段则用 $and
    // 简单做: 总是用 $and (保证正确性)
    if (fragments.length === 1) return fragments[0];
    return `{ $and: [${fragments.join(", ")}] }`;
  }
  if (g.logic === "Or") {
    if (fragments.length === 1) return fragments[0];
    return `{ $or: [${fragments.join(", ")}] }`;
  }
  // Not -> $nor
  return `{ $nor: [${fragments.join(", ")}] }`;
}

/** 字段数组 (含可选 "-" 前缀) -> `{ field1: 1, field2: 0 }` */
function projectionArrayToShell(arr: string[]): string {
  const parts = arr.map((p) => p.trim()).filter(Boolean);
  if (parts.length === 0) return "";
  const items = parts.map((p) => {
    if (p.startsWith("-")) return `${p.slice(1)}: 0`;
    return `${p}: 1`;
  });
  return `{ ${items.join(", ")} }`;
}

function sortArrayToShell(arr: string[]): string {
  const parts = arr.map((p) => p.trim()).filter(Boolean);
  if (parts.length === 0) return "";
  const items = parts.map((p) => {
    if (p.startsWith("-")) return `${p.slice(1)}: -1`;
    return `${p}: 1`;
  });
  return `{ ${items.join(", ")} }`;
}

export interface BuilderExtras {
  /** 字段数组; 每项形如 "field" (include/asc) 或 "-field" (exclude/desc) */
  projection: string[];
  sort: string[];
  skip: string;
  limit: string;
  useFluentApi: boolean;
}

/** 根 group + extras -> 完整 shell 字符串 */
export function buildQueryString(collection: string, root: Group, extras: BuilderExtras): string {
  const collRef = collection.includes(".")
    ? `db.getCollection(${JSON.stringify(collection)})`
    : `db.${collection}`;
  const filter = groupToShellObject(root);
  const projection = projectionArrayToShell(extras.projection);
  const sort = sortArrayToShell(extras.sort);
  const skip = extras.skip.trim();
  const limit = extras.limit.trim();

  if (extras.useFluentApi) {
    // cursor 链
    let out = `${collRef}.find(${filter})`;
    if (projection) out += `\n  .projection(${projection})`;
    if (sort) out += `\n  .sort(${sort})`;
    if (skip) out += `\n  .skip(${skip})`;
    if (limit) out += `\n  .limit(${limit})`;
    return out;
  }

  // 非 fluent: 直接 find(filter, options-ish) + 链
  let out = `${collRef}.find(${filter})`;
  if (projection) out += `.projection(${projection})`;
  if (sort) out += `.sort(${sort})`;
  if (skip) out += `.skip(${skip})`;
  if (limit) out += `.limit(${limit})`;
  return out;
}

// ==================== 从 shell 字符串反向解析 ====================

/** 提取最外层 (...) 里的内容, 支持嵌套 */
function extractArgs(source: string, methodStart: number): { args: string; end: number } | null {
  const open = source.indexOf("(", methodStart);
  if (open < 0) return null;
  let depth = 1;
  let inString = false;
  let sc = "";
  for (let i = open + 1; i < source.length; i++) {
    const c = source[i];
    if (inString) {
      if (c === "\\") {
        i++;
        continue;
      }
      if (c === sc) inString = false;
      continue;
    }
    if (c === '"' || c === "'") {
      inString = true;
      sc = c;
      continue;
    }
    if (c === "(") depth++;
    else if (c === ")") {
      depth--;
      if (depth === 0) return { args: source.slice(open + 1, i), end: i + 1 };
    }
  }
  return null;
}

/** 顶层按 "," 切分 (跳过引号和括号内) */
function splitTopLevel(s: string, sep = ","): string[] {
  const out: string[] = [];
  let depth = 0;
  let inString = false;
  let sc = "";
  let start = 0;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (inString) {
      if (c === "\\") {
        i++;
        continue;
      }
      if (c === sc) inString = false;
      continue;
    }
    if (c === '"' || c === "'") {
      inString = true;
      sc = c;
      continue;
    }
    if (c === "{" || c === "[" || c === "(") depth++;
    else if (c === "}" || c === "]" || c === ")") depth--;
    else if (c === sep && depth === 0) {
      out.push(s.slice(start, i).trim());
      start = i + 1;
    }
  }
  const last = s.slice(start).trim();
  if (last) out.push(last);
  return out;
}

/** "{field: literal}" / "{field: {$op: val}}" / "{$and: [...]}" 解析为 Rule/Group 结构 */
function parseFilterObject(src: string): Group {
  const root = newGroup({ logic: "And" });
  const trimmed = src.trim();
  if (!trimmed || trimmed === "{}") return root;
  if (!trimmed.startsWith("{") || !trimmed.endsWith("}")) return root;

  const body = trimmed.slice(1, -1).trim();
  if (!body) return root;

  const entries = splitTopLevel(body, ",");
  for (const entry of entries) {
    addEntryToGroup(root, entry);
  }
  return root;
}

function addEntryToGroup(g: Group, entry: string) {
  // 分离 key: value
  const colonIdx = findTopLevelColon(entry);
  if (colonIdx < 0) return;
  const key = entry
    .slice(0, colonIdx)
    .trim()
    .replace(/^["']|["']$/g, "");
  const val = entry.slice(colonIdx + 1).trim();

  if (key === "$and" || key === "$or" || key === "$nor") {
    // 数组里每项是一个子 filter 对象
    if (!val.startsWith("[") || !val.endsWith("]")) return;
    const inner = val.slice(1, -1).trim();
    const subGroup = newGroup({
      logic: key === "$and" ? "And" : key === "$or" ? "Or" : "Not",
    });
    for (const item of splitTopLevel(inner, ",")) {
      const parsed = parseFilterObject(item);
      // 若子 group 只有一条 rule 直接合并到 subGroup
      for (const c of parsed.items) subGroup.items.push(c);
    }
    g.items.push(subGroup);
    return;
  }

  // 常规字段条件
  g.items.push(parseFieldValueToRule(key, val));
}

function findTopLevelColon(s: string): number {
  let depth = 0;
  let inString = false;
  let sc = "";
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (inString) {
      if (c === "\\") {
        i++;
        continue;
      }
      if (c === sc) inString = false;
      continue;
    }
    if (c === '"' || c === "'") {
      inString = true;
      sc = c;
      continue;
    }
    if (c === "{" || c === "[" || c === "(") depth++;
    else if (c === "}" || c === "]" || c === ")") depth--;
    else if (c === ":" && depth === 0) return i;
  }
  return -1;
}

/** 识别字面量类型 + 原始值文本 */
function detectTypeAndValue(raw: string): { type: ValueType; value: string } {
  const s = raw.trim();
  if (s === "null") return { type: "Null", value: "" };
  if (s === "true" || s === "false") return { type: "Boolean", value: s };
  if (/^-?\d+(\.\d+)?$/.test(s)) return { type: "Number", value: s };
  if (s.startsWith('"') && s.endsWith('"')) return { type: "String", value: s.slice(1, -1) };
  if (s.startsWith("'") && s.endsWith("'")) return { type: "String", value: s.slice(1, -1) };
  const oid = /^ObjectId\(\s*["']([^"']*)["']\s*\)$/.exec(s);
  if (oid) return { type: "ObjectId", value: oid[1] };
  const iso = /^ISODate\(\s*["']([^"']*)["']\s*\)$/.exec(s);
  if (iso) return { type: "Date", value: iso[1] };
  return { type: "String", value: s };
}

function parseFieldValueToRule(field: string, valRaw: string): Rule {
  const v = valRaw.trim();
  // {$op: X}
  if (v.startsWith("{") && v.endsWith("}")) {
    const inner = v.slice(1, -1).trim();
    const colonIdx = findTopLevelColon(inner);
    if (colonIdx >= 0) {
      const opKey = inner
        .slice(0, colonIdx)
        .trim()
        .replace(/^["']|["']$/g, "");
      const opVal = inner.slice(colonIdx + 1).trim();
      const map: Record<string, RuleOp> = {
        $ne: "notEqual",
        $gt: "gt",
        $gte: "gte",
        $lt: "lt",
        $lte: "lte",
        $in: "in",
        $nin: "nin",
        $regex: "regex",
        $exists: "exists",
        $type: "type",
      };
      const op = map[opKey];
      if (op) {
        if (op === "in" || op === "nin") {
          // 数组
          if (opVal.startsWith("[") && opVal.endsWith("]")) {
            const items = splitTopLevel(opVal.slice(1, -1), ",");
            if (items.length > 0) {
              const first = detectTypeAndValue(items[0]);
              const values = items.map((it) => detectTypeAndValue(it).value).join(", ");
              return newRule({ field, op, valueType: first.type, value: values });
            }
          }
          return newRule({ field, op, valueType: "String", value: "" });
        }
        if (op === "exists") {
          return newRule({
            field,
            op,
            valueType: "Boolean",
            value: opVal.toLowerCase() === "false" ? "false" : "true",
          });
        }
        if (op === "regex") {
          const d = detectTypeAndValue(opVal);
          return newRule({ field, op, valueType: "String", value: d.value });
        }
        const d = detectTypeAndValue(opVal);
        return newRule({ field, op, valueType: d.type, value: d.value });
      }
    }
  }
  // 直接字面量 -> equal
  const d = detectTypeAndValue(v);
  return newRule({ field, op: "equal", valueType: d.type, value: d.value });
}

export interface ParsedFindQuery {
  collection?: string;
  filter: Group;
  extras: BuilderExtras;
}

/** 解析 `db.<coll>.find({...}).sort({...}).limit(N).skip(N).projection({...})` */
export function parseFindQuery(text: string, fallbackCollection = ""): ParsedFindQuery {
  const empty: ParsedFindQuery = {
    collection: fallbackCollection,
    filter: newGroup({ logic: "And" }),
    extras: {
      projection: [],
      sort: ["-_id"],
      skip: "",
      limit: "100",
      useFluentApi: false,
    },
  };
  const s = text.trim();
  // 允许前缀 db.xxx.
  const m = /^db\.(?:getCollection\s*\(\s*["']([^"']+)["']\s*\)|(\w+))\s*\./.exec(s);
  if (!m) return empty;
  const coll = m[1] || m[2];
  const rest = s.slice(m[0].length);
  if (!rest.startsWith("find(")) return { ...empty, collection: coll };

  // find(...)
  const findArgs = extractArgs(rest, 0);
  if (!findArgs) return { ...empty, collection: coll };
  const parts = splitTopLevel(findArgs.args);
  const filter = parts.length > 0 ? parseFilterObject(parts[0]) : empty.filter;

  // 链式调用
  const extras: BuilderExtras = {
    projection: [],
    sort: [],
    skip: "",
    limit: "",
    useFluentApi: false,
  };
  let tail = rest.slice(findArgs.end);
  const methods: Array<[string, keyof BuilderExtras]> = [
    ["projection", "projection"],
    ["sort", "sort"],
    ["skip", "skip"],
    ["limit", "limit"],
  ];
  while (tail.startsWith(".")) {
    let matched = false;
    for (const [name, key] of methods) {
      const prefix = `.${name}(`;
      if (tail.startsWith(prefix)) {
        const inner = extractArgs(tail, 0);
        if (!inner) break;
        const arg = inner.args.trim();
        if (name === "projection") {
          extras.projection = objToFieldArray(arg, "projection");
        } else if (name === "sort") {
          extras.sort = objToFieldArray(arg, "sort");
        } else if (key === "skip" || key === "limit") {
          extras[key] = arg; // skip/limit 是数字字面量
        }
        tail = tail.slice(inner.end);
        matched = true;
        break;
      }
    }
    if (!matched) break;
  }

  return { collection: coll, filter, extras };
}

/**
 * 把 `{field: 1}` / `{field: 0}` / `{field: -1}` 之类对象解析为字段数组:
 *   - projection 模式: 0/false -> "-field", 其它 -> "field"
 *   - sort 模式: -1 -> "-field", 1 -> "field"
 */
function objToFieldArray(obj: string, mode: "projection" | "sort"): string[] {
  const t = obj.trim();
  if (!t.startsWith("{") || !t.endsWith("}")) return [];
  const body = t.slice(1, -1).trim();
  if (!body) return [];
  const parts: string[] = [];
  for (const e of splitTopLevel(body, ",")) {
    const colon = findTopLevelColon(e);
    if (colon < 0) continue;
    const k = e
      .slice(0, colon)
      .trim()
      .replace(/^["']|["']$/g, "");
    const v = e.slice(colon + 1).trim();
    const negative = mode === "projection" ? v === "0" || v === "false" : v === "-1";
    parts.push(negative ? `-${k}` : k);
  }
  return parts;
}

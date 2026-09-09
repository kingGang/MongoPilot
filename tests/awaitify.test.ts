import { describe, it, expect } from "vitest";
import { Parser } from "acorn";
import { awaitifyDbCalls } from "../src/utils/query-preeval";

/** 改写结果必须是合法 JS —— 沙箱里是丢给 AsyncFunction 编译的 */
function parses(code: string): boolean {
  try {
    Parser.parse(code, {
      ecmaVersion: "latest",
      allowAwaitOutsideFunction: true,
      allowReturnOutsideFunction: true,
    });
    return true;
  } catch {
    return false;
  }
}

describe("awaitifyDbCalls", () => {
  it("顶层 db 调用加 await", () => {
    const out = awaitifyDbCalls(`const d = db.user.findOne({a:1});`);
    expect(out).toContain("await db.user.findOne");
    expect(parses(out)).toBe(true);
  });

  it("箭头回调里的 db 调用: 回调变 async + forEach 换成串行助手", () => {
    const out = awaitifyDbCalls(
      `["a","b"].forEach(c => { const n = db.getCollection(c).countDocuments({}); print(n); });`,
    );
    expect(parses(out)).toBe(true);
    expect(out).toContain("__aForEach__");
    expect(out).toContain("async c =>");
    expect(out).toContain("await db.getCollection(c)");
  });

  it("纯计算 helper 不会被改成 async (调用点也就不会插 await)", () => {
    const out = awaitifyDbCalls(
      `function norm(s) { return s.trim(); }\n[1].forEach(x => { const k = norm("a"); print(k); });`,
    );
    expect(parses(out)).toBe(true);
    expect(out).toContain("function norm(s)");
    expect(out).not.toContain("async function norm");
    expect(out).not.toContain("await norm(");
    // 回调里没有 await -> forEach 保持原样
    expect(out).not.toContain("__aForEach__");
  });

  it("含 db 的具名函数变 async, 调用点跟着 await, 并传染外层箭头", () => {
    const out = awaitifyDbCalls(
      `function cnt(c) { return db.getCollection(c).countDocuments({}); }\n["a"].forEach(c => { print(cnt(c)); });`,
    );
    expect(parses(out)).toBe(true);
    expect(out).toContain("async function cnt");
    expect(out).toContain("await cnt(c)");
    expect(out).toContain("__aForEach__");
  });

  it("嵌套 forEach: 只有真含 db 的那层被改写", () => {
    const out = awaitifyDbCalls(
      `["a"].forEach(c => {\n  const idx = db.getCollection(c).getIndexes();\n  idx.forEach(i => { print(i.name); });\n});`,
    );
    expect(parses(out)).toBe(true);
    // 外层换助手, 内层 (无 db) 保持原生 forEach
    expect(out.match(/__aForEach__/g)?.length).toBe(1);
    expect(out).toContain("idx.forEach(i =>");
  });

  it("reduce 的初始值不会被吞掉", () => {
    const out = awaitifyDbCalls(
      `const t = [1,2].reduce((acc, x) => acc + db.c.countDocuments({x}), 0);`,
    );
    expect(parses(out)).toBe(true);
    expect(out).toContain("__aReduce__([1,2], async (acc, x) =>");
    expect(out).toContain(", 0)");
  });

  it("已有 async 函数不会被重复加 async", () => {
    const out = awaitifyDbCalls(`async function f() { return db.c.findOne({}); }`);
    expect(parses(out)).toBe(true);
    expect(out).not.toContain("async async");
  });

  it("顶层最后一条表达式语句改成 return", () => {
    const out = awaitifyDbCalls(`const problems = [];\nproblems;`);
    expect(parses(out)).toBe(true);
    expect(out.trimEnd().endsWith("return problems;")).toBe(true);
  });

  it("db 只是普通标识符时不加 await", () => {
    const out = awaitifyDbCalls(`const x = { db: 1 };\nconst y = x.db;`);
    expect(out).not.toContain("await");
  });

  it("解析不了的代码退回字符扫描版, 不抛异常", () => {
    const out = awaitifyDbCalls(`const a = (;`);
    expect(typeof out).toBe("string");
  });

  it("索引巡检脚本 (真实用例) 改写后语法合法", () => {
    const script = `
const DECLS = [{c:"user", n:"uk_openid", k:'{ "openid": 1 }', u:true}];
function normDecl(s) { return s.replace(/"/g, ""); }
function normActual(k) { return Object.keys(k).join(","); }
const byColl = {};
DECLS.forEach(d => { (byColl[d.c] = byColl[d.c] || []).push(d); });
const existing = new Set(db.getCollectionNames());
let realMissing = 0;
const problems = [];
Object.keys(byColl).sort().forEach(coll => {
  if (!existing.has(coll)) { print("missing " + coll); return; }
  const actual = db.getCollection(coll).getIndexes();
  const haveName = {}, haveKeys = {};
  actual.forEach(a => { haveName[a.name] = a; haveKeys[normActual(a.key)] = a; });
  byColl[coll].forEach(d => {
    if (haveName[d.n]) return;
    if (haveKeys[normDecl(d.k)]) return;
    realMissing++;
    problems.push({ coll: coll, name: d.n });
  });
});
print("真缺 " + realMissing + " 个");
problems;
`;
    const out = awaitifyDbCalls(script);
    expect(parses(out)).toBe(true);
    // 外层遍历要串行 await; 内层纯计算的两个 forEach 保持原样
    expect(out.match(/__aForEach__/g)?.length).toBe(1);
    expect(out).toContain("await db.getCollectionNames()");
    expect(out).toContain("await db.getCollection(coll).getIndexes()");
    expect(out).not.toContain("async function normDecl");
    expect(out.trimEnd().endsWith("return problems;")).toBe(true);
  });
});

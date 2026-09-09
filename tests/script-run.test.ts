import { describe, it, expect } from "vitest";
import { collectScriptOps } from "../src/utils/query-preeval";

/** 假后端: 只认脚本里真正用到的几条读语句 */
function fakeRunner(handler: (stmt: string) => Record<string, unknown>[]) {
  const seen: string[] = [];
  const run = async (stmt: string) => {
    seen.push(stmt);
    const documents = handler(stmt);
    return { documents, count: documents.length };
  };
  return { run, seen };
}

describe("collectScriptOps", () => {
  it("巡检脚本: forEach 回调里的 db 读能串行拿到真实数据", async () => {
    const { run, seen } = fakeRunner((stmt) => {
      if (stmt === "db.getCollectionNames()") return [{ name: "user" }, { name: "order" }];
      if (stmt.includes("getIndexes")) {
        return [
          { v: 2, key: { _id: 1 }, name: "_id_" },
          { v: 2, key: { openid: 1 }, name: "openid_1", unique: true },
        ];
      }
      return [];
    });

    const script = `
const problems = [];
const names = db.getCollectionNames();
print("集合 " + names.length + " 个");
names.forEach(c => {
  const idx = db.getCollection(c).getIndexes();
  print(c + " 索引 " + idx.length + " 个");
  idx.forEach(i => { if (i.unique) problems.push({ coll: c, name: i.name }); });
});
problems;
`;
    const r = await collectScriptOps(script, "", run);

    expect(r.error).toBeNull();
    expect(r.output).toEqual(["集合 2 个", "user 索引 2 个", "order 索引 2 个"]);
    expect(r.value).toEqual([
      { coll: "user", name: "openid_1" },
      { coll: "order", name: "openid_1" },
    ]);
    expect(seen).toContain("db.getCollectionNames()");
    expect(seen).toContain('db.getCollection("user").getIndexes()');
  });

  it("游标 forEach 回调里的写操作被收集 (read-then-write)", async () => {
    const { run } = fakeRunner((stmt) =>
      stmt.startsWith("db.src.find") ? [{ _id: 1 }, { _id: 2 }] : [],
    );

    const script = `db.src.find({}).forEach(d => { db.dst.insertOne({ _id: d._id, ok: true }); });`;
    const r = await collectScriptOps(script, "", run);

    expect(r.error).toBeNull();
    expect(r.ops).toHaveLength(2);
    expect(r.ops[0].collRender).toBe("db.dst");
    expect(r.ops[0].method).toBe("insertOne");
    expect(r.ops.map((o) => (o.args[0] as { _id: number })._id)).toEqual([1, 2]);
  });

  it("纯计算 helper 在回调里照常同步用", async () => {
    const { run } = fakeRunner(() => [{ name: "a" }]);
    const script = `
function key(k) { return Object.keys(k).sort().join(","); }
const out = [];
db.getCollectionNames().forEach(c => {
  const n = db.getCollection(c).countDocuments({});
  out.push({ c: c, k: key({ b: 1, a: 1 }), n: n });
});
out;
`;
    const r = await collectScriptOps(script, "", run);
    expect(r.error).toBeNull();
    expect(r.value).toEqual([{ c: "a", k: "a,b", n: 1 }]);
  });
});

import { describe, it, expect } from "vitest";
import { collectScriptOps, type ScriptReadRunner } from "@/utils/query-preeval";

describe("collectScriptOps — 游标终结方法后可继续链式数组方法", () => {
  it("aggregate(...).toArray().map(...) 结果能喂给后续写操作", async () => {
    const runRead: ScriptReadRunner = async (stmt) => {
      if (stmt.includes("aggregate")) {
        return {
          documents: [{ keepId: { $oid: "a".repeat(24) } }, { keepId: { $oid: "b".repeat(24) } }],
          count: 2,
        };
      }
      return { documents: [], count: 0 };
    };

    const script = `
var keepIds = db.nftStock.aggregate([
  { $group: { _id: "$tokenInfo.tokenId", keepId: { $first: "$_id" } } }
]).toArray().map(doc => doc.keepId);
db.nftStock.deleteMany({ _id: { $nin: keepIds } });
`;

    const res = await collectScriptOps(script, "", runRead);
    expect(res.error).toBeNull();
    expect(res.ops).toHaveLength(1);
    expect(res.ops[0].method).toBe("deleteMany");
    const arg = res.ops[0].args[0] as { _id: { $nin: { $oid: string }[] } };
    expect(arg._id.$nin).toHaveLength(2);
    expect(arg._id.$nin[0].$oid).toBe("a".repeat(24));
    expect(arg._id.$nin[1].$oid).toBe("b".repeat(24));
  });

  it("find(...).toArray().filter(...).map(...) 多级链式", async () => {
    const runRead: ScriptReadRunner = async () => ({
      documents: [{ n: 1 }, { n: 2 }, { n: 3 }],
      count: 3,
    });
    const script = `
var xs = db.c.find({}).toArray().filter(d => d.n > 1).map(d => d.n);
db.c.insertOne({ xs: xs });
`;
    const res = await collectScriptOps(script, "", runRead);
    expect(res.error).toBeNull();
    const arg = res.ops[0].args[0] as { xs: number[] };
    expect(arg.xs).toEqual([2, 3]);
  });

  it("distinct(...).map(...) 可链式", async () => {
    const runRead: ScriptReadRunner = async () => ({
      documents: ["x", "y"] as unknown as Record<string, unknown>[],
      count: 2,
    });
    const script = `
var ds = db.c.distinct("k").map(s => s + "!");
db.c.insertOne({ ds: ds });
`;
    const res = await collectScriptOps(script, "", runRead);
    expect(res.error).toBeNull();
    const arg = res.ops[0].args[0] as { ds: string[] };
    expect(arg.ds).toEqual(["x!", "y!"]);
  });
});

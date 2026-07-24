import { describe, it, expect } from "vitest";
import {
  collectScriptOps,
  extractDbStatements,
  needsPreEvaluation,
  type ScriptReadRunner,
} from "@/utils/query-preeval";

describe("extractDbStatements — 多条 db.* 语句拆分 (多语句 Run)", () => {
  it("一串以 db. 开头、带 ; 和 // 注释的 updateOne 逐条拆开", () => {
    const script = `db.custMonthBill.updateOne({customerId:"a",date:"2026-05"},{$set:{balanceMoney:1}});  // r4 甲
db.custMonthBill.updateOne({customerId:"b",date:"2026-05"},{$set:{balanceMoney:0}});  // r6 乙
db.custMonthBill.updateOne({customerId:"c",date:"2026-05"},{$set:{balanceMoney:2}});  // r7 丙`;
    const stmts = extractDbStatements(script);
    expect(stmts).toHaveLength(3);
    expect(stmts[0]).toContain('"a"');
    expect(stmts[1]).toContain('"b"');
    expect(stmts[2]).toContain('"c"');
    // 这种纯写语句串不应被误判为需要脚本模式 (无 helper / load)
    expect(needsPreEvaluation(script)).toBe(false);
  });

  it("单条跨行链式查询算作 1 条, 不被误拆", () => {
    const script = `db.custMonthBill.find({})
  .projection({})
  .sort({_id:-1})
  .limit(100)`;
    expect(extractDbStatements(script)).toHaveLength(1);
  });
});

describe("collectScriptOps — 具名函数体内 read-then-write (mongosh 同步写法)", () => {
  it("函数里 var p=db.x.findOne(...) 能拿到真实文档, p._id.str 可用, 写操作被收集", async () => {
    const oid = "a".repeat(24);
    const seen: string[] = [];
    const runRead: ScriptReadRunner = async (stmt) => {
      seen.push(stmt);
      if (stmt.includes("findOne")) {
        return { documents: [{ _id: { $oid: oid }, nickName: "唐俊梅" }], count: 1 };
      }
      return { documents: [], count: 0 };
    };

    const script = `
function RealName(phone,cardId,name){
    var p=db.player.findOne({phone:encryptPhoneNumber(phone)})
    if (p===null){
        print(phone+"不存在")
        return
    }
    print(p.nickName)
    db.playerCarId.insertOne({
        "playerId" : p._id.str,
        "phone" : phone,
        "cardID" : cardId,
        "name" : name,
    })
}
function encryptPhoneNumber(phoneNumber) {
    const buffer = Buffer.from(phoneNumber);
    return buffer.toString('base64');
}
RealName("16676341333","510722198310218447","唐俊梅")
`;

    const res = await collectScriptOps(script, "", runRead);
    expect(res.error).toBeNull();
    // findOne 的过滤条件里 phone 应是 base64(不是 [object Promise]/{})
    const base64Phone = Buffer.from("16676341333").toString("base64");
    expect(seen.some((s) => s.includes(base64Phone))).toBe(true);
    // 写操作被收集, 且 playerId 取到了真实 _id.str
    expect(res.ops).toHaveLength(1);
    expect(res.ops[0].method).toBe("insertOne");
    const doc = res.ops[0].args[0] as Record<string, unknown>;
    expect(doc.playerId).toBe(oid);
    expect(doc.phone).toBe("16676341333");
    expect(doc.name).toBe("唐俊梅");
  });

  it("findOne 返回 null 时函数早退, 不产生写操作", async () => {
    const runRead: ScriptReadRunner = async () => ({ documents: [], count: 0 });
    const script = `
function RealName(phone){
    var p=db.player.findOne({phone:phone})
    if (p===null){ print("不存在"); return; }
    db.playerCarId.insertOne({phone:phone})
}
RealName("000")
`;
    const res = await collectScriptOps(script, "", runRead);
    expect(res.error).toBeNull();
    expect(res.ops).toHaveLength(0);
    expect(res.output.join("")).toContain("不存在");
  });
});

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

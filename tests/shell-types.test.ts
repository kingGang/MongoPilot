import { describe, it, expect } from "vitest";
import { collectScriptOps, scriptOpToStatement } from "../src/utils/query-preeval";

const noopRunner = async () => ({ documents: [] as Record<string, unknown>[], count: 0 });

describe("mongosh 类型构造器在脚本沙箱里的行为", () => {
  it("new ISODate() / new ObjectId() 能当构造器用 (以前箭头函数会报 not a constructor)", async () => {
    const script = `
const helper = 1;
db.c.insertOne({ t: new ISODate("2024-01-01T00:00:00Z"), _id: new ObjectId("507f1f77bcf86cd799439011"), n: new NumberInt(7) });
`;
    const r = await collectScriptOps(script, "", noopRunner);
    expect(r.error).toBeNull();
    expect(r.ops).toHaveLength(1);
    const arg = r.ops[0].args[0] as Record<string, unknown>;
    expect(arg.t).toEqual({ $date: "2024-01-01T00:00:00Z" });
    expect(arg._id).toEqual({ $oid: "507f1f77bcf86cd799439011" });
    expect(Number(arg.n)).toBe(7);
  });

  it("无参 ISODate() / ObjectId() 有合理默认值", async () => {
    const script = `
const helper = 1;
db.c.insertOne({ t: ISODate(), id: ObjectId(), n: NumberLong() });
`;
    const r = await collectScriptOps(script, "", noopRunner);
    expect(r.error).toBeNull();
    const arg = r.ops[0].args[0] as Record<string, Record<string, string>>;
    expect(arg.t.$date).toMatch(/^\d{4}-\d{2}-\d{2}T/);
    expect(arg.id.$oid).toHaveLength(24);
    expect(arg.n.$numberLong).toBe("0");
  });

  it("new Date() 渲染成 {$date} 而不是普通字符串 (否则会被存成 String)", async () => {
    const script = `
const helper = 1;
db.c.updateOne({ _id: 1 }, { $set: { updateAt: new Date("2024-03-04T05:06:07.000Z") } });
`;
    const r = await collectScriptOps(script, "", noopRunner);
    expect(r.error).toBeNull();
    const stmt = scriptOpToStatement(r.ops[0]);
    expect(stmt).toContain('"$date":"2024-03-04T05:06:07.000Z"');
    expect(stmt.startsWith("db.c.updateOne(")).toBe(true);
  });

  it("点号字段名 + new ISODate() 的 updateOne 能完整收集", async () => {
    const script = `
const helper = 1;
db.nfrStockV1.updateOne(
  { _id: ObjectId("6a227ed80df9bd50e453418f") },
  {
    $set: {
      "tokenInfo.owner": "cfx:aamzekx",
      updateAt: new ISODate(),
    },
  },
);
`;
    const r = await collectScriptOps(script, "", noopRunner);
    expect(r.error).toBeNull();
    const stmt = scriptOpToStatement(r.ops[0]);
    expect(stmt).toContain('"tokenInfo.owner":"cfx:aamzekx"');
    expect(stmt).toContain('"$date"');
    expect(stmt).not.toContain("new ");
  });
});

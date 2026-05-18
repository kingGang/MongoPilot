/**
 * 代码片段库 —— 工具栏 Snippets 下拉里可插入的常用模板。
 * `${COLL}` 会被替换成当前标签页绑定的集合名 (没有就用 collection 占位)。
 */
export interface SnippetDef {
  /** 分组 */
  group: string;
  /** 显示名 */
  label: string;
  /** 一句话说明 */
  desc: string;
  /** 模板内容; ${COLL} 占位符会被当前集合名替换 */
  body: string;
}

export const MONGO_SNIPPETS: SnippetDef[] = [
  // ---- 查询 ----
  {
    group: "查询",
    label: "find — 条件查询",
    desc: "按条件查询 + 排序 + 限制",
    body: `db.\${COLL}.find({
  // 字段: 值
})
  .sort({ _id: -1 })
  .limit(50)`,
  },
  {
    group: "查询",
    label: "findOne — 取单条",
    desc: "查询符合条件的第一条",
    body: `db.\${COLL}.findOne({
  _id: ObjectId("")
})`,
  },
  {
    group: "查询",
    label: "countDocuments — 计数",
    desc: "统计符合条件的文档数",
    body: `db.\${COLL}.countDocuments({
  // 字段: 值
})`,
  },
  {
    group: "查询",
    label: "distinct — 去重",
    desc: "某字段的所有不同取值",
    body: `db.\${COLL}.distinct("fieldName", {
  // 可选过滤条件
})`,
  },
  // ---- 聚合 ----
  {
    group: "聚合",
    label: "aggregate — 分组统计",
    desc: "$match + $group 分组计数",
    body: `db.\${COLL}.aggregate([
  { $match: { /* 过滤条件 */ } },
  { $group: { _id: "$groupField", count: { $sum: 1 } } },
  { $sort: { count: -1 } }
])`,
  },
  {
    group: "聚合",
    label: "aggregate — 关联查询",
    desc: "$lookup 关联另一个集合",
    body: `db.\${COLL}.aggregate([
  {
    $lookup: {
      from: "otherCollection",
      localField: "localId",
      foreignField: "_id",
      as: "joined"
    }
  },
  { $unwind: "$joined" }
])`,
  },
  // ---- 写入 ----
  {
    group: "写入",
    label: "insertOne — 插入单条",
    desc: "插入一个文档",
    body: `db.\${COLL}.insertOne({
  createdAt: ISODate()
})`,
  },
  {
    group: "写入",
    label: "updateMany — 批量更新",
    desc: "更新所有匹配的文档",
    body: `db.\${COLL}.updateMany(
  { /* 过滤条件 */ },
  { $set: { /* 要改的字段 */ } }
)`,
  },
  {
    group: "写入",
    label: "deleteMany — 批量删除",
    desc: "删除所有匹配的文档",
    body: `db.\${COLL}.deleteMany({
  // 过滤条件
})`,
  },
  // ---- 索引 ----
  {
    group: "索引",
    label: "createIndex — 建索引",
    desc: "在字段上创建索引",
    body: `db.\${COLL}.createIndex(
  { fieldName: 1 },
  { name: "idx_fieldName", background: true }
)`,
  },
  {
    group: "索引",
    label: "getIndexes — 看索引",
    desc: "列出集合的所有索引",
    body: `db.\${COLL}.getIndexes()`,
  },
  // ---- 脚本 ----
  {
    group: "脚本",
    label: "遍历更新脚本",
    desc: "查出文档逐条处理 (read-then-write)",
    body: `// 查出要处理的文档, 逐条更新
var docs = db.\${COLL}.find({ /* 过滤条件 */ }).limit(100).toArray();
print("待处理: " + docs.length + " 条");

for (var i = 0; i < docs.length; i++) {
  var doc = docs[i];
  db.\${COLL}.updateOne(
    { _id: doc._id },
    { $set: { /* 要改的字段 */ } }
  );
}
print("完成");`,
  },
  {
    group: "脚本",
    label: "批量生成脚本",
    desc: "循环插入多条文档",
    body: `// 批量生成并插入
for (var i = 1; i <= 100; i++) {
  db.\${COLL}.insertOne({
    seq: i,
    createdAt: ISODate()
  });
}
print("已插入 100 条");`,
  },
  // ---- 元命令 ----
  {
    group: "元命令",
    label: "show — 库/集合/用户",
    desc: "查看数据库、集合、用户、慢查询",
    body: `show dbs
show collections
show users
show profile`,
  },
];

/** 把片段里的 ${COLL} 占位符替换成实际集合名 */
export function renderSnippet(body: string, collection?: string): string {
  return body.replace(/\$\{COLL\}/g, collection && collection.trim() ? collection : "collection");
}

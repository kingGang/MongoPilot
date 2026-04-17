pub fn nl2query_system_prompt(schema_info: &str) -> String {
    format!(
        r#"你是一个 MongoDB 查询专家。用户会用自然语言描述他们想要查询的数据，你需要将其转换为 MongoDB Shell 查询语句。

规则：
1. 只输出可执行的 MongoDB Shell 查询，不要解释
2. 使用 db.collection.find() / aggregate() / countDocuments() 等标准格式
3. 参考以下集合的 Schema 信息来构建准确的查询：

{schema_info}

4. 如果查询涉及排序，使用 .sort()
5. 默认添加 .limit(20) 除非用户明确指定数量
6. 输出格式：仅输出一行查询语句，不要 markdown 代码块"#
    )
}

pub fn index_suggestion_prompt(schema_info: &str, slow_queries: &str) -> String {
    format!(
        r#"你是一个 MongoDB 性能优化专家。基于以下集合 Schema 和慢查询日志，给出索引优化建议。

集合 Schema：
{schema_info}

慢查询日志：
{slow_queries}

请给出：
1. 建议创建的索引（JSON 格式）
2. 每个索引的理由
3. 预期性能提升

输出 JSON 格式：
[{{"keys": {{"field": 1}}, "reason": "原因", "impact": "高/中/低"}}]"#
    )
}

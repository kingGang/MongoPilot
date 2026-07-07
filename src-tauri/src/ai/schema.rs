use std::collections::HashMap;
use mongodb::bson::{doc, Document, Bson};
use mongodb::Client;
use serde::Serialize;

use crate::error::AppError;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaInfo {
    pub collection: String,
    pub sample_count: i64,
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldInfo {
    pub name: String,
    pub field_types: Vec<TypeCount>,
    pub occurrence_percent: f64,
    pub sample_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeCount {
    pub bson_type: String,
    pub count: i64,
}

pub async fn analyze_schema(
    client: &Client,
    database: &str,
    collection: &str,
    sample_size: i64,
) -> Result<SchemaInfo, AppError> {
    let coll = client.database(database).collection::<Document>(collection);

    // 采样文档
    let pipeline = vec![doc! { "$sample": { "size": sample_size } }];
    let mut cursor = coll.aggregate(pipeline).await.map_err(AppError::Mongo)?;

    let mut field_stats: HashMap<String, FieldStats> = HashMap::new();
    let mut total = 0i64;

    use futures::StreamExt;
    while let Some(doc) = cursor.next().await {
        let doc = doc.map_err(AppError::Mongo)?;
        total += 1;
        collect_fields(&doc, "", &mut field_stats);
    }

    let mut fields: Vec<FieldInfo> = field_stats
        .into_iter()
        .map(|(name, stats)| {
            let mut type_counts: Vec<TypeCount> = stats
                .types
                .into_iter()
                .map(|(t, c)| TypeCount { bson_type: t, count: c })
                .collect();
            type_counts.sort_by(|a, b| b.count.cmp(&a.count));

            FieldInfo {
                name,
                field_types: type_counts,
                occurrence_percent: if total > 0 {
                    (stats.count as f64 / total as f64) * 100.0
                } else {
                    0.0
                },
                sample_values: stats.samples,
            }
        })
        .collect();

    fields.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(SchemaInfo {
        collection: collection.to_string(),
        sample_count: total,
        fields,
    })
}

struct FieldStats {
    count: i64,
    types: HashMap<String, i64>,
    samples: Vec<String>,
}

fn collect_fields(doc: &Document, prefix: &str, stats: &mut HashMap<String, FieldStats>) {
    for (key, value) in doc {
        let full_key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };

        let entry = stats.entry(full_key.clone()).or_insert_with(|| FieldStats {
            count: 0,
            types: HashMap::new(),
            samples: Vec::new(),
        });

        entry.count += 1;
        let type_name = bson_type_name(value);
        *entry.types.entry(type_name).or_insert(0) += 1;

        if entry.samples.len() < 3 {
            let sample = format!("{value}");
            if sample.len() < 100 {
                entry.samples.push(sample);
            }
        }

        // 递归嵌套文档
        if let Bson::Document(nested) = value {
            collect_fields(nested, &full_key, stats);
        } else if let Bson::Array(arr) = value {
            // 数组内嵌文档: 子字段按 Mongo 点路径规则归并到 `attachment.id` 这类 key,
            // 编辑器里 "$attachment." 的二级补全靠它; 每个数组只采前几个元素控制成本
            for item in arr.iter().take(5) {
                if let Bson::Document(nested) = item {
                    collect_fields(nested, &full_key, stats);
                }
            }
        }
    }
}

fn bson_type_name(value: &Bson) -> String {
    match value {
        Bson::Double(_) => "Double".to_string(),
        Bson::String(_) => "String".to_string(),
        Bson::Array(_) => "Array".to_string(),
        Bson::Document(_) => "Object".to_string(),
        Bson::Boolean(_) => "Boolean".to_string(),
        Bson::Null => "Null".to_string(),
        Bson::Int32(_) => "Int32".to_string(),
        Bson::Int64(_) => "Int64".to_string(),
        Bson::DateTime(_) => "DateTime".to_string(),
        Bson::ObjectId(_) => "ObjectId".to_string(),
        Bson::Binary(_) => "Binary".to_string(),
        Bson::Decimal128(_) => "Decimal128".to_string(),
        Bson::Timestamp(_) => "Timestamp".to_string(),
        Bson::RegularExpression(_) => "Regex".to_string(),
        _ => "Unknown".to_string(),
    }
}

/// 将 SchemaInfo 格式化为简洁的文本描述，用于 AI 提示词
pub fn schema_to_text(schema: &SchemaInfo) -> String {
    let mut lines = vec![format!("集合: {} ({} 个采样文档)", schema.collection, schema.sample_count)];
    for field in &schema.fields {
        let types: Vec<String> = field.field_types.iter().map(|t| format!("{}({})", t.bson_type, t.count)).collect();
        lines.push(format!("  {} — {} — {:.0}%", field.name, types.join(", "), field.occurrence_percent));
    }
    lines.join("\n")
}

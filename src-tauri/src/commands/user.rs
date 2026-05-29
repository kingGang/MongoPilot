use mongodb::bson::{doc, Document};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::connection::manager::ConnectionManager;
use crate::error::AppError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub user: String,
    pub database: String,
    pub roles: Vec<RoleInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleInfo {
    pub role: String,
    pub db: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub roles: Vec<RoleInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleInput {
    pub role: String,
    pub db: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileEntry {
    pub op: String,
    pub ns: String,
    pub millis: i64,
    pub ts: String,
    pub command: serde_json::Value,
    pub plan_summary: String,
}

#[tauri::command]
pub async fn list_users(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
) -> Result<Vec<UserInfo>, AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let db = client.database(&database);

    let result = db
        .run_command(doc! { "usersInfo": 1 })
        .await
        .map_err(AppError::Mongo)?;

    let users_arr = result.get_array("users").map_err(|_| {
        AppError::InvalidInput("无法获取用户列表".into())
    })?;

    let mut users = Vec::new();
    for user_bson in users_arr {
        if let Some(user_doc) = user_bson.as_document() {
            let roles_arr = user_doc.get_array("roles").unwrap_or(&Vec::new()).clone();
            let roles: Vec<RoleInfo> = roles_arr
                .iter()
                .filter_map(|r| r.as_document())
                .map(|r| RoleInfo {
                    role: r.get_str("role").unwrap_or("").to_string(),
                    db: r.get_str("db").unwrap_or("").to_string(),
                })
                .collect();

            users.push(UserInfo {
                user: user_doc.get_str("user").unwrap_or("").to_string(),
                database: user_doc.get_str("db").unwrap_or("").to_string(),
                roles,
            });
        }
    }

    Ok(users)
}

#[tauri::command]
pub async fn create_user(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    request: CreateUserRequest,
) -> Result<(), AppError> {
    if mgr.is_read_only(&connection_id).await {
        return Err(AppError::InvalidInput("只读连接: 不允许创建用户".into()));
    }
    let client = mgr.get_client(&connection_id).await?;
    let db = client.database(&database);

    let roles: Vec<Document> = request
        .roles
        .iter()
        .map(|r| doc! { "role": &r.role, "db": &r.db })
        .collect();

    db.run_command(doc! {
        "createUser": &request.username,
        "pwd": &request.password,
        "roles": roles,
    })
    .await
    .map_err(AppError::Mongo)?;

    Ok(())
}

#[tauri::command]
pub async fn drop_user(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    username: String,
) -> Result<(), AppError> {
    if mgr.is_read_only(&connection_id).await {
        return Err(AppError::InvalidInput("只读连接: 不允许删除用户".into()));
    }
    let client = mgr.get_client(&connection_id).await?;
    let db = client.database(&database);

    db.run_command(doc! { "dropUser": &username })
        .await
        .map_err(AppError::Mongo)?;

    Ok(())
}

#[tauri::command]
pub async fn get_profiler_status(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
) -> Result<serde_json::Value, AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let db = client.database(&database);

    let result = db
        .run_command(doc! { "profile": -1 })
        .await
        .map_err(AppError::Mongo)?;

    serde_json::to_value(&result)
        .map_err(|e| AppError::InvalidInput(format!("序列化失败: {e}")))
}

#[tauri::command]
pub async fn set_profiler_level(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    level: i32,
    slow_ms: Option<i64>,
) -> Result<(), AppError> {
    if mgr.is_read_only(&connection_id).await {
        return Err(AppError::InvalidInput("只读连接: 不允许修改 Profiler 配置".into()));
    }
    let client = mgr.get_client(&connection_id).await?;
    let db = client.database(&database);

    let mut cmd = doc! { "profile": level };
    if let Some(ms) = slow_ms {
        cmd.insert("slowms", ms);
    }

    db.run_command(cmd).await.map_err(AppError::Mongo)?;

    Ok(())
}

#[tauri::command]
pub async fn get_profiler_data(
    mgr: State<'_, ConnectionManager>,
    connection_id: String,
    database: String,
    limit: Option<i64>,
) -> Result<Vec<ProfileEntry>, AppError> {
    let client = mgr.get_client(&connection_id).await?;
    let coll = client
        .database(&database)
        .collection::<Document>("system.profile");

    use mongodb::options::FindOptions;
    let options = FindOptions::builder()
        .sort(doc! { "ts": -1 })
        .limit(limit.unwrap_or(50))
        .build();

    let mut cursor = coll.find(doc! {}).with_options(options).await.map_err(AppError::Mongo)?;

    let mut entries = Vec::new();
    use futures::StreamExt;
    while let Some(doc) = cursor.next().await {
        let doc = doc.map_err(AppError::Mongo)?;
        let command_json = doc
            .get_document("command")
            .ok()
            .and_then(|c| serde_json::to_value(c).ok())
            .unwrap_or(serde_json::Value::Object(Default::default()));

        entries.push(ProfileEntry {
            op: doc.get_str("op").unwrap_or("").to_string(),
            ns: doc.get_str("ns").unwrap_or("").to_string(),
            millis: doc.get_i64("millis").or_else(|_| doc.get_i32("millis").map(|v| v as i64)).unwrap_or(0),
            ts: doc
                .get_datetime("ts")
                .ok()
                .map(|dt| dt.to_string())
                .unwrap_or_default(),
            command: command_json,
            plan_summary: doc.get_str("planSummary").unwrap_or("").to_string(),
        });
    }

    Ok(entries)
}

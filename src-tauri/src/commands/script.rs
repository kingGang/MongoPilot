use serde::Deserialize;
use sqlx::SqlitePool;
use tauri::State;

use crate::error::AppError;
use crate::storage::script_repo::{self, ScriptFolderRow, ScriptRow};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveScriptInput {
    /// None 时后端生成新 id, Some 时按 id 更新
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub folder_path: String,
    #[serde(default)]
    pub content: String,
    pub connection_id: Option<String>,
    pub database_name: Option<String>,
    #[serde(default)]
    pub sort_order: i64,
}

fn uuid_v4() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

#[tauri::command]
pub async fn list_scripts(pool: State<'_, SqlitePool>) -> Result<Vec<ScriptRow>, AppError> {
    script_repo::list_scripts(&pool).await
}

#[tauri::command]
pub async fn list_script_folders(
    pool: State<'_, SqlitePool>,
) -> Result<Vec<ScriptFolderRow>, AppError> {
    script_repo::list_folders(&pool).await
}

#[tauri::command]
pub async fn get_script(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<ScriptRow, AppError> {
    script_repo::get_script(&pool, &id).await
}

/// 去掉脚本引用里的常见扩展名 (脚本库存的 name 不含扩展名)
fn strip_script_ext(s: &str) -> String {
    for ext in [".js", ".mongosh", ".ts", ".json", ".txt", ".sql", ".md"] {
        if let Some(base) = s.strip_suffix(ext) {
            return base.to_string();
        }
    }
    s.to_string()
}

/// 解析 `load("...")` 里对**脚本库**的引用, 返回脚本内容.
/// 接受 `文件夹/脚本名` 或裸 `脚本名` (扩展名可有可无).
/// 前端只在引用不是绝对文件系统路径时才走这里.
#[tauri::command]
pub async fn resolve_script_ref(
    pool: State<'_, SqlitePool>,
    reference: String,
) -> Result<String, AppError> {
    let raw = reference.trim().replace('\\', "/");
    let r = raw.trim_start_matches("./").trim_start_matches('/');
    let r_noext = strip_script_ext(r);

    let scripts = script_repo::list_scripts(&pool).await?;
    let full_key = |s: &ScriptRow| -> String {
        if s.folder_path.is_empty() {
            s.name.clone()
        } else {
            format!("{}/{}", s.folder_path, s.name)
        }
    };

    // 1. 完整路径 (folder_path/name) 精确匹配, 带不带扩展名都试
    for s in &scripts {
        let key = full_key(s);
        if key == r || key == r_noext {
            return Ok(s.content.clone());
        }
    }

    // 2. 仅按脚本名匹配 (要求唯一, 否则提示用 文件夹/脚本名)
    let name_target = r.rsplit('/').next().unwrap_or(r);
    let name_noext = strip_script_ext(name_target);
    let by_name: Vec<&ScriptRow> = scripts
        .iter()
        .filter(|s| s.name == name_target || s.name == name_noext)
        .collect();
    match by_name.len() {
        1 => Ok(by_name[0].content.clone()),
        0 => Err(AppError::NotFound(format!("脚本库里找不到: {reference}"))),
        _ => Err(AppError::InvalidInput(format!(
            "脚本库里有多个名为 \"{name_target}\" 的脚本, 请用 文件夹/脚本名 形式引用"
        ))),
    }
}

/// 保存脚本: 有 id 走 update, 无 id 生成新 id 走 insert. 返回最终的 row.
#[tauri::command]
pub async fn save_script(
    pool: State<'_, SqlitePool>,
    input: SaveScriptInput,
) -> Result<ScriptRow, AppError> {
    if input.name.trim().is_empty() {
        return Err(AppError::InvalidInput("脚本名不能为空".into()));
    }

    let id = input.id.unwrap_or_else(uuid_v4);
    let row = ScriptRow {
        id: id.clone(),
        name: input.name,
        folder_path: input.folder_path,
        content: input.content,
        connection_id: input.connection_id,
        database_name: input.database_name,
        sort_order: input.sort_order,
        created_at: String::new(),
        updated_at: String::new(),
    };

    // 先尝试更新, 0 行影响则改为插入
    let existing = script_repo::get_script(&pool, &id).await;
    match existing {
        Ok(_) => script_repo::update_script(&pool, &row).await?,
        Err(AppError::NotFound(_)) => script_repo::insert_script(&pool, &row).await?,
        Err(e) => return Err(e),
    }
    script_repo::get_script(&pool, &id).await
}

#[tauri::command]
pub async fn delete_script(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<(), AppError> {
    script_repo::delete_script(&pool, &id).await
}

#[tauri::command]
pub async fn create_script_folder(
    pool: State<'_, SqlitePool>,
    path: String,
) -> Result<(), AppError> {
    script_repo::create_folder(&pool, &path).await
}

#[tauri::command]
pub async fn delete_script_folder(
    pool: State<'_, SqlitePool>,
    path: String,
    cascade: Option<bool>,
) -> Result<(), AppError> {
    script_repo::delete_folder(&pool, &path, cascade.unwrap_or(false)).await
}

#[tauri::command]
pub async fn rename_script_folder(
    pool: State<'_, SqlitePool>,
    old_path: String,
    new_path: String,
) -> Result<(), AppError> {
    script_repo::rename_folder(&pool, &old_path, &new_path).await
}

/// 允许作为脚本导入的文件扩展名 (大小写不敏感)
const SCRIPT_EXTENSIONS: &[&str] = &["js", "sql", "json", "txt", "md", "ts", "mongosh"];
/// 单文件上限 5 MB, 超过的跳过
const MAX_FILE_SIZE: u64 = 5 * 1024 * 1024;

fn ext_allowed(p: &std::path::Path) -> bool {
    p.extension()
        .and_then(|s| s.to_str())
        .map(|e| SCRIPT_EXTENSIONS.iter().any(|allowed| allowed.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub imported: i64,
    pub skipped: i64,
    pub folders_created: i64,
}

/// 导入单个文件 (或多个文件) 到指定 folder_path. 路径由前端 dialog 拿到, 后端读取内容.
#[tauri::command]
pub async fn import_script_files(
    pool: State<'_, SqlitePool>,
    paths: Vec<String>,
    target_folder: Option<String>,
) -> Result<ImportSummary, AppError> {
    let folder = target_folder.unwrap_or_default();
    let mut imported = 0i64;
    let mut skipped = 0i64;
    for path_str in paths {
        let p = std::path::Path::new(&path_str);
        if !p.is_file() {
            skipped += 1;
            continue;
        }
        let meta = match std::fs::metadata(p) {
            Ok(m) => m,
            Err(_) => { skipped += 1; continue; }
        };
        if meta.len() > MAX_FILE_SIZE {
            skipped += 1;
            continue;
        }
        let content = match std::fs::read_to_string(p) {
            Ok(c) => c,
            Err(_) => { skipped += 1; continue; }
        };
        let name = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("imported")
            .to_string();
        let row = ScriptRow {
            id: uuid_v4(),
            name,
            folder_path: folder.clone(),
            content,
            connection_id: None,
            database_name: None,
            sort_order: 0,
            created_at: String::new(),
            updated_at: String::new(),
        };
        script_repo::insert_script(&pool, &row).await?;
        imported += 1;
    }
    Ok(ImportSummary { imported, skipped, folders_created: 0 })
}

/// 递归导入目录: 文件夹结构原样镜像到 script_folders + scripts, 顶级 = target_folder 下.
#[tauri::command]
pub async fn import_script_directory(
    pool: State<'_, SqlitePool>,
    root_path: String,
    target_folder: Option<String>,
) -> Result<ImportSummary, AppError> {
    let root = std::path::PathBuf::from(&root_path);
    if !root.is_dir() {
        return Err(AppError::InvalidInput(format!("不是目录: {root_path}")));
    }
    let base_folder = target_folder.unwrap_or_default();
    // 顶层目录名作为 base 下的一个子文件夹 (避免直接散在根目录)
    let top_name = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("imported")
        .to_string();
    let top_folder = if base_folder.is_empty() {
        top_name
    } else {
        format!("{base_folder}/{top_name}")
    };

    let mut imported = 0i64;
    let mut skipped = 0i64;
    let mut folders_created = 0i64;

    // 先把顶层目录创建出来
    script_repo::create_folder(&pool, &top_folder).await.ok();
    folders_created += 1;

    walk_dir(&root, &top_folder, &pool, &mut imported, &mut skipped, &mut folders_created).await?;

    Ok(ImportSummary { imported, skipped, folders_created })
}

/// 递归遍历目录 (异步 Box<dyn Future> 避免 async 函数递归)
fn walk_dir<'a>(
    dir: &'a std::path::Path,
    folder_path: &'a str,
    pool: &'a SqlitePool,
    imported: &'a mut i64,
    skipped: &'a mut i64,
    folders_created: &'a mut i64,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), AppError>> + Send + 'a>> {
    Box::pin(async move {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let name = match entry.file_name().into_string() {
                Ok(n) => n,
                Err(_) => { *skipped += 1; continue; }
            };
            // 跳过隐藏文件 / node_modules 之类常见噪音
            if name.starts_with('.') || name == "node_modules" || name == "target" || name == ".git" {
                continue;
            }
            if p.is_dir() {
                let sub_folder = format!("{folder_path}/{name}");
                script_repo::create_folder(pool, &sub_folder).await.ok();
                *folders_created += 1;
                walk_dir(&p, &sub_folder, pool, imported, skipped, folders_created).await?;
                continue;
            }
            if !p.is_file() {
                continue;
            }
            if !ext_allowed(&p) {
                *skipped += 1;
                continue;
            }
            let meta = match std::fs::metadata(&p) {
                Ok(m) => m,
                Err(_) => { *skipped += 1; continue; }
            };
            if meta.len() > MAX_FILE_SIZE {
                *skipped += 1;
                continue;
            }
            let content = match std::fs::read_to_string(&p) {
                Ok(c) => c,
                Err(_) => { *skipped += 1; continue; }
            };
            let script_name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("imported")
                .to_string();
            let row = ScriptRow {
                id: uuid_v4(),
                name: script_name,
                folder_path: folder_path.to_string(),
                content,
                connection_id: None,
                database_name: None,
                sort_order: 0,
                created_at: String::new(),
                updated_at: String::new(),
            };
            script_repo::insert_script(pool, &row).await?;
            *imported += 1;
        }
        Ok(())
    })
}

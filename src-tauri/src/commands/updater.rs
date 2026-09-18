use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

use crate::error::AppError;

const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/kingGang/MongoPilot/releases/latest";

/// 只允许下载本仓库 Release 下的资源 —— URL 是前端传回来的, 不能拿来下任意文件
const RELEASE_DOWNLOAD_PREFIX: &str = "https://github.com/kingGang/MongoPilot/releases/download/";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    /// GitHub Release 页面地址 (用户点"查看详情"打开这个)
    pub release_url: String,
    /// Release notes / changelog
    pub notes: String,
    /// 发布时间 ISO 字符串
    pub published_at: String,
    /// 当前平台对应的安装包下载 URL (Windows→exe, macOS→dmg, Linux→AppImage)
    pub asset_url: Option<String>,
    /// 安装包文件名 (给 UI 显示)
    pub asset_name: Option<String>,
    /// 安装包大小 (字节)
    pub asset_size: Option<u64>,
}

/// 语义化版本比较: latest > current 时返回 true.
/// 简化版本, 只处理 `major.minor.patch` 数字形式 (跟仓库当前的发版格式一致).
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> {
        v.split('.')
            .filter_map(|p| {
                p.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .ok()
            })
            .collect()
    };
    let l = parse(latest);
    let c = parse(current);
    let n = l.len().max(c.len());
    for i in 0..n {
        let lv = l.get(i).copied().unwrap_or(0);
        let cv = c.get(i).copied().unwrap_or(0);
        if lv > cv {
            return true;
        }
        if lv < cv {
            return false;
        }
    }
    false
}

/// 从 release assets 里挑当前平台对应的安装包.
/// 优先级: Windows → NSIS setup.exe > MSI; macOS → dmg (universal 优先); Linux → AppImage > deb
fn pick_asset(assets: &serde_json::Value) -> (Option<String>, Option<String>, Option<u64>) {
    let empty: Vec<serde_json::Value> = Vec::new();
    let list = assets.as_array().unwrap_or(&empty);

    #[cfg(target_os = "windows")]
    let suffixes: &[&str] = &["_x64-setup.exe", "_x64_en-US.msi"];
    #[cfg(target_os = "macos")]
    let suffixes: &[&str] = &["universal.dmg", ".dmg"];
    #[cfg(target_os = "linux")]
    let suffixes: &[&str] = &[".AppImage", "_amd64.deb"];

    for suffix in suffixes {
        for a in list {
            let name = a["name"].as_str().unwrap_or("");
            if name.to_lowercase().ends_with(&suffix.to_lowercase()) {
                let url = a["browser_download_url"].as_str().map(String::from);
                let size = a["size"].as_u64();
                return (url, Some(name.to_string()), size);
            }
        }
    }
    (None, None, None)
}

#[tauri::command]
pub async fn check_for_updates(app_handle: tauri::AppHandle) -> Result<UpdateInfo, AppError> {
    let current_version = app_handle.package_info().version.to_string();

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| AppError::Connection(format!("HTTP 客户端初始化失败: {e}")))?;

    let resp = client
        .get(GITHUB_LATEST_RELEASE_URL)
        .header("User-Agent", "MongoPilot-Updater")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AppError::Connection(format!("请求 GitHub 失败: {e}")))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        // 仓库还没发过 release
        return Err(AppError::NotFound("尚无任何已发布版本".into()));
    }
    if !resp.status().is_success() {
        let code = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::Connection(format!(
            "GitHub 返回 {code}: {}",
            body.chars().take(200).collect::<String>()
        )));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Connection(format!("解析 release JSON 失败: {e}")))?;

    let tag = json["tag_name"].as_str().unwrap_or("").to_string();
    let latest_version = tag.trim_start_matches('v').to_string();
    let release_url = json["html_url"].as_str().unwrap_or("").to_string();
    let notes = json["body"].as_str().unwrap_or("").to_string();
    let published_at = json["published_at"].as_str().unwrap_or("").to_string();

    let (asset_url, asset_name, asset_size) = pick_asset(&json["assets"]);

    let has_update = is_newer(&latest_version, &current_version);

    Ok(UpdateInfo {
        current_version,
        latest_version,
        has_update,
        release_url,
        notes,
        published_at,
        asset_url,
        asset_name,
        asset_size,
    })
}

/// 下载进度事件 `update-download-progress` 的载荷
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    downloaded: u64,
    /// 0 表示服务端没给 Content-Length
    total: u64,
}

/// 下载安装包到临时目录, 边下边 emit `update-download-progress`; 返回本地文件路径。
#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    url: String,
    file_name: String,
) -> Result<String, AppError> {
    if !url.starts_with(RELEASE_DOWNLOAD_PREFIX) {
        return Err(AppError::InvalidInput(
            "下载地址不是 MongoPilot 的 Release 资源, 已拒绝".into(),
        ));
    }
    // 只取最后一段文件名, 防目录穿越
    let safe_name = std::path::Path::new(&file_name)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("MongoPilot-update")
        .to_string();

    let dir = std::env::temp_dir().join("MongoPilot-update");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError::InvalidInput(format!("创建下载目录失败: {e}")))?;
    let path = dir.join(&safe_name);

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        // 安装包上百 MB, 不设总超时, 靠连接超时兜底
        .build()
        .map_err(|e| AppError::Connection(format!("HTTP 客户端初始化失败: {e}")))?;

    let resp = client
        .get(&url)
        .header("User-Agent", "MongoPilot-Updater")
        .send()
        .await
        .map_err(|e| AppError::Connection(format!("下载失败: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Connection(format!(
            "下载失败: HTTP {}",
            resp.status()
        )));
    }

    let total = resp.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|e| AppError::InvalidInput(format!("写入下载文件失败: {e}")))?;

    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::Connection(format!("下载中断: {e}")))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| AppError::InvalidInput(format!("写入下载文件失败: {e}")))?;
        downloaded += chunk.len() as u64;
        // 每 512KB 报一次, 别把事件通道刷爆
        if downloaded - last_emit >= 512 * 1024 {
            last_emit = downloaded;
            let _ = app.emit("update-download-progress", DownloadProgress { downloaded, total });
        }
    }
    file.flush()
        .await
        .map_err(|e| AppError::InvalidInput(format!("写入下载文件失败: {e}")))?;
    drop(file);

    if total > 0 && downloaded != total {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(AppError::Connection(format!(
            "下载不完整 ({downloaded}/{total} 字节), 请重试"
        )));
    }
    let _ = app.emit(
        "update-download-progress",
        DownloadProgress {
            downloaded,
            total: total.max(downloaded),
        },
    );

    Ok(path.to_string_lossy().to_string())
}

/// 启动下载好的安装程序。
/// Windows: 直接跑 setup.exe (msi 走 msiexec), 然后退出自己 —— 不退出的话文件被占用装不上;
/// macOS: `open` 打开 dmg, 用户拖进 Applications;
/// Linux: AppImage 加可执行位后打开所在目录 (发行版装法不一, 不代劳)。
#[tauri::command]
pub async fn install_update(app: AppHandle, path: String) -> Result<(), AppError> {
    let p = std::path::PathBuf::from(&path);
    if !p.exists() {
        return Err(AppError::NotFound("安装包不存在, 请重新下载".into()));
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        /// 独立进程组: 本进程退出后安装程序继续活着
        const DETACHED_PROCESS: u32 = 0x0000_0008;

        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        let mut cmd = if ext == "msi" {
            let mut c = std::process::Command::new("msiexec");
            c.arg("/i").arg(&p);
            c
        } else {
            std::process::Command::new(&p)
        };
        cmd.creation_flags(DETACHED_PROCESS);
        cmd.spawn()
            .map_err(|e| AppError::InvalidInput(format!("启动安装程序失败: {e}")))?;

        // 等安装程序窗口起来再退出自己
        let handle = app.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            handle.exit(0);
        });
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let _ = &app;
        std::process::Command::new("open")
            .arg(&p)
            .spawn()
            .map_err(|e| AppError::InvalidInput(format!("打开安装包失败: {e}")))?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let _ = &app;
        // AppImage 下载下来没有可执行位, 先补上
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&p) {
                let mut perm = meta.permissions();
                perm.set_mode(perm.mode() | 0o111);
                let _ = std::fs::set_permissions(&p, perm);
            }
        }
        let dir = p.parent().unwrap_or(&p).to_path_buf();
        std::process::Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .map_err(|e| AppError::InvalidInput(format!("打开下载目录失败: {e}")))?;
        return Ok(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_semver() {
        assert!(is_newer("0.1.28", "0.1.27"));
        assert!(is_newer("0.2.0", "0.1.99"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.27", "0.1.27"));
        assert!(!is_newer("0.1.27", "0.1.28"));
        assert!(!is_newer("0.1.5", "0.1.10"));
    }

    #[test]
    fn only_repo_release_urls_allowed() {
        assert!(RELEASE_DOWNLOAD_PREFIX.starts_with("https://github.com/kingGang/MongoPilot/"));
        // download_update 的第一道闸: 非本仓库 Release 前缀一律拒绝
        for bad in [
            "https://evil.example.com/setup.exe",
            "https://github.com/other/repo/releases/download/v1/setup.exe",
            "http://github.com/kingGang/MongoPilot/releases/download/v1/setup.exe",
        ] {
            assert!(!bad.starts_with(RELEASE_DOWNLOAD_PREFIX), "应拒绝: {bad}");
        }
        assert!(
            "https://github.com/kingGang/MongoPilot/releases/download/v0.1.41/MongoPilot_0.1.41_x64-setup.exe"
                .starts_with(RELEASE_DOWNLOAD_PREFIX)
        );
    }

    #[test]
    fn ignores_prerelease_tags() {
        // "0.1.28-beta" 目前会被截成 [0,1,28], 跟 "0.1.28" 视为相等 (返回 false).
        // 只要仓库不发 prerelease 就无所谓; 后续要严格时再改.
        assert!(!is_newer("0.1.28-beta", "0.1.28"));
    }
}

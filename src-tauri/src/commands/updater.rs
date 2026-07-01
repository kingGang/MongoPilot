use serde::{Deserialize, Serialize};

use crate::error::AppError;

const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/kingGang/MongoPilot/releases/latest";

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
    fn ignores_prerelease_tags() {
        // "0.1.28-beta" 目前会被截成 [0,1,28], 跟 "0.1.28" 视为相等 (返回 false).
        // 只要仓库不发 prerelease 就无所谓; 后续要严格时再改.
        assert!(!is_newer("0.1.28-beta", "0.1.28"));
    }
}

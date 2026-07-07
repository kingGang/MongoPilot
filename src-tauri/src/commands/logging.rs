use std::io::Write;
use tauri::Manager;

/// 前端错误落盘: app_data_dir/error.log, 超 1MB 轮转一份 (只留一代).
/// 用于排查偶现问题 (如首次连接偶发报错) —— 弹窗消失后仍能查到当时的报错.
#[tauri::command]
pub fn log_client_error(app: tauri::AppHandle, message: String) {
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let path = dir.join("error.log");
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 1024 * 1024 {
            let _ = std::fs::rename(&path, dir.join("error.log.1"));
        }
    }
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let msg: String = message.chars().take(4000).collect();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

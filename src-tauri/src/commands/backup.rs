use tauri::{AppHandle, State};

use crate::backup::{
    self, BackupDirInfo, BackupRequest, BackupSummary, RestoreRequest, RestoreSummary,
};
use crate::connection::manager::ConnectionManager;
use crate::error::AppError;

/// 备份整库或指定集合到 mongodump 兼容的 BSON 目录
#[tauri::command]
pub async fn backup_database(
    app: AppHandle,
    mgr: State<'_, ConnectionManager>,
    request: BackupRequest,
) -> Result<BackupSummary, AppError> {
    let client = mgr.get_client(&request.connection_id).await?;
    backup::run_backup(&client, &app, &request).await
}

/// 扫描一个备份目录, 返回里面可恢复的集合清单
#[tauri::command]
pub async fn scan_backup_dir(path: String) -> Result<BackupDirInfo, AppError> {
    backup::scan_backup_dir(&path)
}

/// 从备份目录恢复到目标库
#[tauri::command]
pub async fn restore_backup(
    app: AppHandle,
    mgr: State<'_, ConnectionManager>,
    request: RestoreRequest,
) -> Result<RestoreSummary, AppError> {
    if mgr.is_read_only(&request.connection_id).await {
        return Err(AppError::InvalidInput("只读连接: 不允许恢复数据".into()));
    }
    let client = mgr.get_client(&request.connection_id).await?;
    backup::run_restore(&client, &app, &request).await
}

use tauri::State;

use crate::connection::config::ConnectionConfig;
use crate::connection::manager::{ConnectionManager, ServerInfo};
use crate::crypto::{aes, key_store};
use crate::error::AppError;
use crate::storage::connection_repo::{self, ConnectionRow};
use crate::storage::database::DbPool;

/// Convert a ConnectionConfig into a ConnectionRow, encrypting password fields.
fn config_to_row(config: &ConnectionConfig) -> Result<ConnectionRow, AppError> {
    let passphrase = key_store::key();

    let password_encrypted = config
        .password
        .as_deref()
        .map(|p| aes::encrypt(p, passphrase))
        .transpose()?;

    let ssh_password_encrypted = config
        .ssh_password
        .as_deref()
        .map(|p| aes::encrypt(p, passphrase))
        .transpose()?;

    let ssh_passphrase_encrypted = config
        .ssh_passphrase
        .as_deref()
        .map(|p| aes::encrypt(p, passphrase))
        .transpose()?;

    Ok(ConnectionRow {
        id: config.id.clone(),
        name: config.name.clone(),
        group_path: config.group_path.clone(),
        color: config.color.clone(),
        conn_type: config.conn_type.clone(),
        host: config.host.clone(),
        port: config.port as i64,
        auth_type: config.auth_type.clone(),
        username: config.username.clone(),
        password_encrypted,
        auth_db: config.auth_db.clone(),
        replica_set: config.replica_set.clone(),
        srv: if config.srv { 1 } else { 0 },
        tls: if config.tls { 1 } else { 0 },
        tls_ca_file: config.tls_ca_file.clone(),
        tls_cert_file: config.tls_cert_file.clone(),
        tls_key_file: config.tls_key_file.clone(),
        tls_allow_invalid: if config.tls_allow_invalid { 1 } else { 0 },
        ssh_enabled: if config.ssh_enabled { 1 } else { 0 },
        ssh_host: config.ssh_host.clone(),
        ssh_port: config.ssh_port.map(|p| p as i64),
        ssh_username: config.ssh_username.clone(),
        ssh_auth_type: config.ssh_auth_type.clone(),
        ssh_password_encrypted,
        ssh_private_key: config.ssh_private_key.clone(),
        ssh_passphrase_encrypted,
        default_db: config.default_db.clone(),
        uri_options: config.uri_options.clone(),
        read_only: Some(if config.read_only { 1 } else { 0 }),
        sort_order: config.sort_order,
        created_at: String::new(),
        updated_at: String::new(),
    })
}

/// Convert a ConnectionRow into a ConnectionConfig, decrypting password fields.
fn row_to_config(row: &ConnectionRow) -> Result<ConnectionConfig, AppError> {
    let passphrase = key_store::key();

    let password = row
        .password_encrypted
        .as_deref()
        .map(|p| aes::decrypt(p, passphrase))
        .transpose()?;

    let ssh_password = row
        .ssh_password_encrypted
        .as_deref()
        .map(|p| aes::decrypt(p, passphrase))
        .transpose()?;

    let ssh_passphrase = row
        .ssh_passphrase_encrypted
        .as_deref()
        .map(|p| aes::decrypt(p, passphrase))
        .transpose()?;

    Ok(ConnectionConfig {
        id: row.id.clone(),
        name: row.name.clone(),
        group_path: row.group_path.clone(),
        color: row.color.clone(),
        conn_type: row.conn_type.clone(),
        host: row.host.clone(),
        port: row.port as u16,
        auth_type: row.auth_type.clone(),
        username: row.username.clone(),
        password,
        auth_db: row.auth_db.clone(),
        replica_set: row.replica_set.clone(),
        srv: row.srv != 0,
        tls: row.tls != 0,
        tls_ca_file: row.tls_ca_file.clone(),
        tls_cert_file: row.tls_cert_file.clone(),
        tls_key_file: row.tls_key_file.clone(),
        tls_allow_invalid: row.tls_allow_invalid != 0,
        ssh_enabled: row.ssh_enabled != 0,
        ssh_host: row.ssh_host.clone(),
        ssh_port: row.ssh_port.map(|p| p as u16),
        ssh_username: row.ssh_username.clone(),
        ssh_auth_type: row.ssh_auth_type.clone(),
        ssh_password,
        ssh_private_key: row.ssh_private_key.clone(),
        ssh_passphrase,
        default_db: row.default_db.clone(),
        uri_options: row.uri_options.clone(),
        read_only: row.read_only.unwrap_or(0) != 0,
        sort_order: row.sort_order,
    })
}

/// List all saved connections.
#[tauri::command]
pub async fn list_connections(
    pool: State<'_, DbPool>,
) -> Result<Vec<ConnectionConfig>, AppError> {
    let rows = connection_repo::list_connections(&pool).await?;
    rows.iter().map(row_to_config).collect()
}

/// Get a single connection by ID.
#[tauri::command]
pub async fn get_connection(
    pool: State<'_, DbPool>,
    id: String,
) -> Result<ConnectionConfig, AppError> {
    let row = connection_repo::get_connection(&pool, &id).await?;
    row_to_config(&row)
}

/// Save a connection — update if exists, insert if not found.
#[tauri::command]
pub async fn save_connection(
    pool: State<'_, DbPool>,
    mgr: State<'_, ConnectionManager>,
    config: ConnectionConfig,
) -> Result<(), AppError> {
    // 同步内存中的只读状态
    mgr.update_read_only(&config.id, config.read_only).await;

    let row = config_to_row(&config)?;
    match connection_repo::update_connection(&pool, &row).await {
        Ok(()) => Ok(()),
        Err(AppError::NotFound(_)) => connection_repo::insert_connection(&pool, &row).await,
        Err(e) => Err(e),
    }
}

/// Delete a connection — disconnect first if active, then remove from DB.
#[tauri::command]
pub async fn delete_connection(
    pool: State<'_, DbPool>,
    mgr: State<'_, ConnectionManager>,
    id: String,
) -> Result<(), AppError> {
    mgr.disconnect(&id).await;
    connection_repo::delete_connection(&pool, &id).await
}

/// Test a connection without persisting it.
#[tauri::command]
pub async fn test_connection(
    mgr: State<'_, ConnectionManager>,
    config: ConnectionConfig,
) -> Result<ServerInfo, AppError> {
    mgr.test_connection(&config).await
}

/// Establish and store a live connection.
#[tauri::command]
pub async fn connect(
    mgr: State<'_, ConnectionManager>,
    config: ConnectionConfig,
) -> Result<(), AppError> {
    mgr.connect(&config).await
}

/// Disconnect a live connection by ID.
#[tauri::command]
pub async fn disconnect(
    mgr: State<'_, ConnectionManager>,
    id: String,
) -> Result<(), AppError> {
    mgr.disconnect(&id).await;
    Ok(())
}

/// Parse a MongoDB URI string into a ConnectionConfig.
#[tauri::command]
pub fn parse_uri(uri: String) -> Result<ConnectionConfig, AppError> {
    ConnectionConfig::from_uri(&uri)
}

/// Export a ConnectionConfig as a MongoDB URI string.
#[tauri::command]
pub fn export_uri(config: ConnectionConfig) -> Result<String, AppError> {
    Ok(config.to_uri())
}

/// List IDs of all currently active connections.
#[tauri::command]
pub async fn active_connections(
    mgr: State<'_, ConnectionManager>,
) -> Result<Vec<String>, AppError> {
    Ok(mgr.active_ids().await)
}

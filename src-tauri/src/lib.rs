pub mod ai;
pub mod commands;
pub mod connection;
pub mod crypto;
pub mod error;
pub mod query;
pub mod storage;

use connection::manager::ConnectionManager;
use crypto::key_store;
use storage::database;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("无法解析应用数据目录");

            key_store::initialize(&app_data_dir).expect("加密密钥初始化失败");

            // 同步初始化 SQLite 数据库 (含首次启动的 5 条迁移), 保证窗口打开前
            // SqlitePool / ConnectionManager 已经 manage 上, 避免前端首次 invoke
            // (例如 ServerTree 立刻调 list_connections) 拿不到 State 而白屏.
            let pool = tauri::async_runtime::block_on(database::init_db(&app_data_dir))
                .expect("数据库初始化失败");
            app.manage(pool);
            app.manage(ConnectionManager::new());

            // 窗口显示策略:
            //   - dev (debug_assertions): 立即 show, 避免 Vite HMR / dep-optimize 触发 reload
            //     时打断 JS 的 revealWindow() 调用, 窗口永久隐藏.
            //   - release: 保留 visible:false 让 JS 侧 revealWindow 在 Vue mount 完后 show
            //     (修首次安装白屏); 另起 5s 兜底, 若 JS 完全失效也能让窗口最终露脸.
            if let Some(win) = app.get_webview_window("main") {
                #[cfg(debug_assertions)]
                {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
                #[cfg(not(debug_assertions))]
                {
                    let win_clone = win.clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        let _ = win_clone.show();
                        let _ = win_clone.set_focus();
                    });
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::logging::log_client_error,
            commands::connection::list_connections,
            commands::connection::get_connection,
            commands::connection::save_connection,
            commands::connection::delete_connection,
            commands::connection::test_connection,
            commands::connection::connect,
            commands::connection::disconnect,
            commands::connection::parse_uri,
            commands::connection::export_uri,
            commands::connection::active_connections,
            commands::database::list_databases,
            commands::database::count_database_collections,
            commands::database::list_collections,
            commands::database::drop_database,
            commands::query::run_query,
            commands::query::run_script_ops,
            commands::query::get_query_history,
            commands::query::search_query_history,
            commands::query::clear_query_history,
            commands::query::list_all_query_history,
            commands::query::clear_all_query_history,
            commands::document::insert_document,
            commands::document::update_document,
            commands::document::delete_document,
            commands::document::delete_documents,
            commands::collection::create_collection,
            commands::collection::drop_collection,
            commands::collection::get_collection_stats,
            commands::collection::list_indexes,
            commands::collection::create_index,
            commands::collection::drop_index,
            commands::collection::re_index,
            commands::collection::get_index_info,
            commands::collection::get_collection_indexes,
            commands::server::get_server_status,
            commands::server::explain_query,
            commands::server::explain_shell_query,
            commands::user::list_users,
            commands::user::create_user,
            commands::user::drop_user,
            commands::user::get_profiler_status,
            commands::user::set_profiler_level,
            commands::user::get_profiler_data,
            commands::ai::get_ai_settings,
            commands::ai::save_ai_settings,
            commands::ai::get_ai_rules,
            commands::ai::save_ai_rules,
            commands::ai::list_ai_conversations,
            commands::ai::upsert_ai_conversation,
            commands::ai::update_ai_conversation_title,
            commands::ai::touch_ai_conversation,
            commands::ai::delete_ai_conversation,
            commands::ai::clear_ai_conversation,
            commands::ai::get_ai_messages,
            commands::ai::append_ai_message,
            commands::ai::list_ai_facts,
            commands::ai::remember_ai_fact,
            commands::ai::forget_ai_fact,
            commands::ai::ai_chat,
            commands::ai::ai_agent_turn,
            commands::ai::nl2query,
            commands::ai::analyze_collection_schema,
            commands::ai::suggest_indexes,
            commands::export::write_export_file,
            commands::export::write_export_binary,
            commands::export::export_query,
            commands::export::read_import_file,
            commands::export::import_documents,
            commands::export::stream_import,
            commands::script::list_scripts,
            commands::script::list_script_folders,
            commands::script::get_script,
            commands::script::resolve_script_ref,
            commands::script::save_script,
            commands::script::delete_script,
            commands::script::create_script_folder,
            commands::script::delete_script_folder,
            commands::script::rename_script_folder,
            commands::script::import_script_files,
            commands::script::import_script_directory,
            commands::updater::check_for_updates,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败");
}

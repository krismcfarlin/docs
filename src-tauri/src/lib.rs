pub mod commands;
pub mod db;
pub mod state;
pub mod types;

use state::AppState;
use tauri::Manager;
use tauri_plugin_shell::ShellExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .setup(|app| {
            // Start VelesDB sidecar — stores embeddings locally on this device.
            // Non-fatal: vector search falls back to SQLite if it fails to start.
            let shell = app.shell();
            match shell
                .sidecar("velesdb-server")
                .expect("velesdb-server sidecar not configured")
                .args(["--data-dir", &velesdb_data_dir(app.handle())])
                .spawn()
            {
                Ok((mut rx, _child)) => {
                    // Drain stdout/stderr in background so the pipe never blocks
                    tauri::async_runtime::spawn(async move {
                        while let Some(_) = rx.recv().await {}
                    });
                    eprintln!("[velesdb] started");
                }
                Err(e) => {
                    eprintln!("[velesdb] failed to start (vector search will use SQLite fallback): {e}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::db::init_db,
            commands::db::sync_db,
            commands::db::get_spaces,
            commands::db::create_space,
            commands::db::get_pages,
            commands::db::create_page,
            commands::db::get_page_version,
            commands::db::save_page_version,
            commands::db::publish_version,
            commands::db::freeze_version,
            commands::db::fork_version,
            commands::db::list_page_versions,
            commands::import::read_file,
            commands::import::fetch_gdoc,
            commands::import::import_page,
            commands::vector::vectorize_page,
            commands::vector::search_similar_pages,
            commands::db::delete_page,
            commands::db::delete_space,
            commands::db::rename_page,
            commands::db::move_space,
            commands::db::reorder_spaces,
            commands::db::reorder_pages,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::settings::write_text_file,
            commands::oauth::start_google_oauth,
            commands::oauth::wait_google_oauth_callback,
            commands::oauth::refresh_google_token,
            commands::import::list_gdocs,
            commands::import::fetch_gdoc_by_id,
            commands::import::fetch_gdoc_tabs,
            commands::import::import_pages_bulk,
            commands::import::get_page_image,
            commands::db::rename_space,
            commands::db::get_trash_pages,
            commands::db::restore_page,
            commands::db::permanent_delete_page,
            commands::db::record_page_access,
            commands::db::get_recent_pages,
            commands::sync::connect_remote_space,
            commands::sync::sync_space,
            commands::sync::disconnect_space,
            commands::sync::get_space_token,
            commands::sync::exchange_google_token,
            commands::sync::exchange_admin_token,
            commands::sync::update_space_token,
            commands::sync::list_invites,
            commands::sync::add_invite,
            commands::sync::remove_invite,
            commands::db::upsert_presence,
            commands::db::clear_presence,
            commands::db::get_page_presence,
            commands::db::get_all_presence,
            commands::synthesis::get_space_config,
            commands::synthesis::set_space_config,
            commands::synthesis::synthesize_page,
            commands::synthesis::get_page_synthesis,
            commands::synthesis::get_entity_suggestions,
            commands::synthesis::promote_entity,
            commands::synthesis::dismiss_entity,
            commands::synthesis::update_space_overview,
            commands::synthesis::get_space_overview,
            commands::synthesis::get_page_links,
            commands::db::move_page_to_space,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Returns a stable data directory for VelesDB on this device.
/// e.g. ~/Library/Application Support/build.conductor.bamako/velesdb
fn velesdb_data_dir(app: &tauri::AppHandle) -> String {
    app.path()
        .app_data_dir()
        .map(|p| p.join("velesdb").to_string_lossy().into_owned())
        .unwrap_or_else(|_| "/tmp/bamako-velesdb".to_string())
}

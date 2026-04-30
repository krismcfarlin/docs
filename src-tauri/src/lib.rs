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
        .register_asynchronous_uri_scheme_protocol("bamakimg", |_app, request, responder| {
            let uri = request.uri().to_string();
            tauri::async_runtime::spawn(async move {
                let file_id = uri.trim_start_matches("bamakimg://").trim_end_matches('/');
                let cache_path = {
                    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                    std::path::PathBuf::from(home).join(".bamako/image_cache").join(format!("{file_id}.jpg"))
                };

                let bytes = if cache_path.exists() {
                    std::fs::read(&cache_path).ok()
                } else {
                    let settings = crate::commands::settings::load_settings();
                    let token = settings.google_access_token.filter(|t| !t.is_empty());
                    if let Some(tok) = token {
                        let url = format!("https://www.googleapis.com/drive/v3/files/{file_id}?alt=media");
                        match reqwest::Client::new().get(&url).bearer_auth(&tok).send().await {
                            Ok(r) if r.status().is_success() => {
                                let b = r.bytes().await.unwrap_or_default().to_vec();
                                if let Some(parent) = cache_path.parent() {
                                    std::fs::create_dir_all(parent).ok();
                                }
                                std::fs::write(&cache_path, &b).ok();
                                Some(b)
                            }
                            _ => None,
                        }
                    } else {
                        None
                    }
                };

                match bytes {
                    Some(b) if !b.is_empty() => {
                        responder.respond(
                            tauri::http::Response::builder()
                                .header("Content-Type", "image/jpeg")
                                .header("Cache-Control", "max-age=604800")
                                .body(b)
                                .unwrap()
                        );
                    }
                    _ => {
                        responder.respond(
                            tauri::http::Response::builder()
                                .status(404)
                                .body(b"not found".to_vec())
                                .unwrap()
                        );
                    }
                }
            });
        })
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
            commands::synthesis::clear_synthesis_data,
            commands::synthesis::force_resynthesize,
            commands::synthesis::create_wiki_stubs,
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
            commands::synthesis::get_graph_data,
            commands::synthesis::ask_wiki,
            commands::synthesis::lint_space,
            commands::synthesis::demote_entity_page,
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

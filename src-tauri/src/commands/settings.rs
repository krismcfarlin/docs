use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub sqld_url: Option<String>,
    pub sqld_token: Option<String>,
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
    pub google_access_token: Option<String>,
    pub google_refresh_token: Option<String>,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub openrouter_api_key: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            sqld_url: None,
            sqld_token: None,
            google_client_id: None,
            google_client_secret: None,
            google_access_token: None,
            google_refresh_token: None,
            user_name: Some(std::env::var("USER").unwrap_or_else(|_| "You".to_string())),
            user_email: None,
            openrouter_api_key: None,
        }
    }
}

fn settings_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".bamako").join("settings.json")
}

pub fn load_settings() -> Settings {
    let path = settings_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

#[tauri::command]
pub fn get_settings() -> Settings {
    let mut s = load_settings();
    if s.user_name.is_none() {
        s.user_name = Some(std::env::var("USER").unwrap_or_else(|_| "You".to_string()));
    }
    s
}

/// Persist only the google_access_token, leaving all other fields unchanged.
pub fn save_google_access_token(token: &str) {
    let mut s = load_settings();
    s.google_access_token = Some(token.to_string());
    if let Ok(json) = serde_json::to_string_pretty(&s) {
        let path = settings_path();
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(&path, json);
    }
}

/// Clear both Google tokens (access + refresh) when re-auth is required.
pub fn clear_google_tokens() {
    let mut s = load_settings();
    s.google_access_token = None;
    s.google_refresh_token = None;
    if let Ok(json) = serde_json::to_string_pretty(&s) {
        let path = settings_path();
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(&path, json);
    }
}

/// Write text content to a file path chosen by the user via a save dialog.
/// Returns true if saved, false if cancelled.
#[tauri::command]
pub fn write_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_settings(
    sqld_url: Option<String>,
    sqld_token: Option<String>,
    google_client_id: Option<String>,
    google_client_secret: Option<String>,
    google_access_token: Option<String>,
    google_refresh_token: Option<String>,
    user_name: Option<String>,
    user_email: Option<String>,
    openrouter_api_key: Option<String>,
) -> Result<(), String> {
    let settings = Settings {
        sqld_url,
        sqld_token,
        google_client_id,
        google_client_secret,
        google_access_token,
        google_refresh_token,
        user_name,
        user_email,
        openrouter_api_key,
    };
    let path = settings_path();
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

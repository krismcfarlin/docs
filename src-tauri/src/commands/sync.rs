use crate::commands::db::get_or_open_space_db;
use crate::state::AppState;
use crate::types::Space;
use nanoid::nanoid;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Connect to a remote sqld namespace and add it to the local registry.
#[tauri::command]
pub async fn connect_remote_space(
    state: State<'_, AppState>,
    server_url: String,
    namespace: String,
    token: String,
    permission_level: String,
    parent_space_id: Option<String>,
    admin_token: Option<String>,
) -> Result<Space, String> {
    if !["owner", "write", "read"].contains(&permission_level.as_str()) {
        return Err(format!(
            "Invalid permission_level '{}': must be one of owner, write, read",
            permission_level
        ));
    }

    // Check for existing (server_url, namespace) pair
    {
        let guard = state.registry.lock().await;
        let registry = guard.as_ref().ok_or("Registry not initialised")?;
        let conn = registry.connect().map_err(|e| e.to_string())?;
        let mut rows = conn
            .query(
                "SELECT id FROM spaces WHERE server_url = ?1 AND namespace = ?2",
                libsql::params![server_url.clone(), namespace.clone()],
            )
            .await
            .map_err(|e| e.to_string())?;
        if rows.next().await.map_err(|e| e.to_string())?.is_some() {
            return Err("already connected".to_string());
        }
    }

    let space_id = nanoid!();

    {
        let guard = state.registry.lock().await;
        let registry = guard.as_ref().ok_or("Registry not initialised")?;
        let conn = registry.connect().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO spaces (id, name, source, server_url, namespace, token, permission_level, parent_space_id, admin_token) \
             VALUES (?1, ?2, 'remote', ?3, ?4, ?5, ?6, ?7, ?8)",
            libsql::params![
                space_id.clone(),
                namespace.clone(),
                server_url.clone(),
                namespace.clone(),
                token.clone(),
                permission_level.clone(),
                parent_space_id.clone(),
                admin_token.clone(),
            ],
        )
        .await
        .map_err(|e| e.to_string())?;
    }

    if let Err(e) = get_or_open_space_db(&state, &space_id).await {
        if let Ok(guard) = state.registry.try_lock() {
            if let Some(registry) = guard.as_ref() {
                if let Ok(conn) = registry.connect() {
                    let _ = conn
                        .execute(
                            "DELETE FROM spaces WHERE id = ?1",
                            libsql::params![space_id.clone()],
                        )
                        .await;
                }
            }
        }
        return Err(format!("Failed to connect to '{}': {}", server_url, e));
    }

    let created_at = {
        let guard = state.registry.lock().await;
        let registry = guard.as_ref().ok_or("Registry not initialised")?;
        let conn = registry.connect().map_err(|e| e.to_string())?;
        let mut rows = conn
            .query(
                "SELECT created_at FROM spaces WHERE id = ?1",
                libsql::params![space_id.clone()],
            )
            .await
            .map_err(|e| e.to_string())?;
        rows.next()
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Space row disappeared after insert".to_string())?
            .get::<String>(0)
            .map_err(|e| e.to_string())?
    };

    Ok(Space {
        id: space_id,
        name: namespace.clone(),
        description: None,
        parent_space_id,
        sort_order: 0,
        created_at,
        source: "remote".to_string(),
        namespace: Some(namespace),
        server_url: Some(server_url),
        permission_level,
    })
}

/// No-op for pure HTTP spaces — every query is already live.
#[tauri::command]
pub async fn sync_space(_state: State<'_, AppState>, _space_id: String) -> Result<(), String> {
    Ok(())
}

/// Return the stored auth token for a space (used for invite generation).
#[tauri::command]
pub async fn get_space_token(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<String, String> {
    let guard = state.registry.lock().await;
    let registry = guard.as_ref().ok_or("Registry not initialised")?;
    let conn = registry.connect().map_err(|e| e.to_string())?;
    let mut rows = conn
        .query(
            "SELECT token FROM spaces WHERE id = ?1",
            libsql::params![space_id],
        )
        .await
        .map_err(|e| e.to_string())?;
    let row = rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Space not found")?;
    Ok(row.get::<String>(0).unwrap_or_default())
}

/// Remove a remote space from the registry and close its DB handle.
#[tauri::command]
pub async fn disconnect_space(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<(), String> {
    {
        let guard = state.registry.lock().await;
        let registry = guard.as_ref().ok_or("Registry not initialised")?;
        let conn = registry.connect().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM spaces WHERE id = ?1",
            libsql::params![space_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    state.space_dbs.lock().await.remove(&space_id);
    Ok(())
}

/// Exchange a Google OAuth access token for a sqld JWT via the server's auth service.
/// Returns the sqld JWT string.
#[tauri::command]
pub async fn exchange_google_token(
    server_url: String,
    access_token: String,
) -> Result<String, String> {
    let auth_url = format!("{}/auth/token", server_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let resp = client
        .post(&auth_url)
        .json(&serde_json::json!({ "access_token": access_token }))
        .send()
        .await
        .map_err(|e| format!("auth request failed: {}", e))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("auth service error: {}", body.trim()));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("parse error: {}", e))?;
    body["sqld_token"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "no sqld_token in response".to_string())
}

#[tauri::command]
pub async fn exchange_admin_token(
    server_url: String,
    admin_token: String,
) -> Result<String, String> {
    let auth_url = format!("{}/auth/admin", server_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let resp = client
        .post(&auth_url)
        .json(&serde_json::json!({ "admin_token": admin_token }))
        .send()
        .await
        .map_err(|e| format!("auth request failed: {}", e))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("auth service error: {}", body.trim()));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("parse error: {}", e))?;
    body["sqld_token"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "no sqld_token in response".to_string())
}

/// Update the stored token for a space (used after Google token exchange).
#[tauri::command]
pub async fn update_space_token(
    state: State<'_, AppState>,
    space_id: String,
    token: String,
) -> Result<(), String> {
    let guard = state.registry.lock().await;
    let registry = guard.as_ref().ok_or("Registry not initialised")?;
    let conn = registry.connect().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE spaces SET token = ?1 WHERE id = ?2",
        libsql::params![token, space_id],
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Invite management (admin endpoints) ───────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct InviteEntry {
    pub email: String,
    pub added_at: i64,
    pub added_by: Option<String>,
}

/// Fetch the admin token for a space from the registry.
async fn get_admin_token(state: &AppState, space_id: &str) -> Result<String, String> {
    let guard = state.registry.lock().await;
    let registry = guard.as_ref().ok_or("Registry not initialised")?;
    let conn = registry.connect().map_err(|e| e.to_string())?;
    let mut rows = conn
        .query(
            "SELECT admin_token, server_url FROM spaces WHERE id = ?1",
            libsql::params![space_id.to_string()],
        )
        .await
        .map_err(|e| e.to_string())?;
    let row = rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Space not found")?;
    let token = row.get::<Option<String>>(0).map_err(|e| e.to_string())?;
    token.ok_or_else(|| "No admin token for this space".to_string())
}

async fn get_server_url(state: &AppState, space_id: &str) -> Result<String, String> {
    let guard = state.registry.lock().await;
    let registry = guard.as_ref().ok_or("Registry not initialised")?;
    let conn = registry.connect().map_err(|e| e.to_string())?;
    let mut rows = conn
        .query(
            "SELECT server_url FROM spaces WHERE id = ?1",
            libsql::params![space_id.to_string()],
        )
        .await
        .map_err(|e| e.to_string())?;
    let row = rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Space not found")?;
    row.get::<String>(0).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_invites(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<Vec<InviteEntry>, String> {
    let admin_token = get_admin_token(&state, &space_id).await?;
    let server_url = get_server_url(&state, &space_id).await?;
    let url = format!("{}/auth/invites", server_url.trim_end_matches('/'));

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .bearer_auth(&admin_token)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("auth service: {}", resp.text().await.unwrap_or_default()));
    }
    resp.json::<Vec<InviteEntry>>()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_invite(
    state: State<'_, AppState>,
    space_id: String,
    email: String,
    added_by: Option<String>,
) -> Result<(), String> {
    let admin_token = get_admin_token(&state, &space_id).await?;
    let server_url = get_server_url(&state, &space_id).await?;
    let url = format!("{}/auth/invites", server_url.trim_end_matches('/'));

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({ "email": email, "added_by": added_by }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("auth service: {}", resp.text().await.unwrap_or_default()));
    }
    Ok(())
}

#[tauri::command]
pub async fn remove_invite(
    state: State<'_, AppState>,
    space_id: String,
    email: String,
) -> Result<(), String> {
    let admin_token = get_admin_token(&state, &space_id).await?;
    let server_url = get_server_url(&state, &space_id).await?;
    let url = format!(
        "{}/auth/invites/{}",
        server_url.trim_end_matches('/'),
        urlencoding::encode(&email)
    );

    let client = reqwest::Client::new();
    let resp = client
        .delete(&url)
        .bearer_auth(&admin_token)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("auth service: {}", resp.text().await.unwrap_or_default()));
    }
    Ok(())
}

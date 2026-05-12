/// Google OAuth flow for accessing private Google Docs.
///
/// Flow:
///   1. Frontend calls `start_google_oauth` → opens system browser with consent screen
///   2. Frontend calls `wait_google_oauth_callback` → blocks until browser redirects to localhost:9877
///   3. Returns (access_token, refresh_token) → frontend saves them via save_settings
///
/// The user only needs to do this once; the refresh token persists in ~/.bamako/settings.json.

use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

const REDIRECT_PORT: u16 = 9877;
const REDIRECT_URI: &str = "http://localhost:9877";
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file \
                     https://www.googleapis.com/auth/drive.readonly \
                     https://www.googleapis.com/auth/documents.readonly";

pub const DEFAULT_CLIENT_ID: &str = env!("GOOGLE_CLIENT_ID");
pub const DEFAULT_CLIENT_SECRET: &str = env!("GOOGLE_CLIENT_SECRET");

fn resolve_client_id(id: &str) -> &str {
    if id.is_empty() { DEFAULT_CLIENT_ID } else { id }
}

fn resolve_client_secret(secret: &str) -> &str {
    if secret.is_empty() { DEFAULT_CLIENT_SECRET } else { secret }
}

#[tauri::command]
pub fn start_google_oauth(app: AppHandle, client_id: String) -> Result<(), String> {
    let client_id = resolve_client_id(&client_id).to_string();
    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth\
?client_id={client_id}\
&redirect_uri={REDIRECT_URI}\
&response_type=code\
&scope={scope}\
&access_type=offline\
&prompt=consent",
        scope = SCOPE
    );
    app.shell()
        .open(&auth_url, None)
        .map_err(|e| format!("Could not open browser: {e}"))
}

#[tauri::command]
pub async fn wait_google_oauth_callback(
    client_id: String,
    client_secret: String,
) -> Result<GoogleTokens, String> {
    let client_id = resolve_client_id(&client_id).to_string();
    let client_secret = resolve_client_secret(&client_secret).to_string();
    // Listen for the browser redirect
    let listener = TcpListener::bind(format!("127.0.0.1:{REDIRECT_PORT}"))
        .await
        .map_err(|e| format!("Could not bind port {REDIRECT_PORT}: {e}"))?;

    let (stream, _) = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        listener.accept(),
    )
    .await
    .map_err(|_| "Timed out waiting for OAuth redirect (2 minutes)")?
    .map_err(|e| e.to_string())?;

    let (reader_half, mut writer_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader_half);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await.map_err(|e| e.to_string())?;

    // Extract ?code= or ?error= from the redirect path
    let path = request_line.split_whitespace().nth(1).unwrap_or("");
    let qs = path.split('?').nth(1).unwrap_or("");

    if let Some(err) = qs.split('&').find(|p| p.starts_with("error=")) {
        let msg = err.strip_prefix("error=").unwrap_or(err);
        let html = format!("<html><body><h2>Error: {msg}</h2><p>You can close this tab.</p></body></html>");
        write_http_response(&mut writer_half, 400, &html).await;
        return Err(format!("OAuth error: {msg}"));
    }

    let code = qs
        .split('&')
        .find(|p| p.starts_with("code="))
        .and_then(|p| p.strip_prefix("code="))
        .ok_or("No authorization code in redirect")?
        .to_string();

    // Acknowledge in the browser
    let html = "<html><body style='font-family:sans-serif;padding:2rem'>\
        <h2>✓ Authenticated</h2><p>You can close this tab and return to Bamako.</p></body></html>";
    write_http_response(&mut writer_half, 200, html).await;

    // Exchange code for tokens
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code.as_str()),
            ("client_id", &client_id),
            ("client_secret", &client_secret),
            ("redirect_uri", REDIRECT_URI),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|e| format!("Token exchange request failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Token exchange parse failed: {e}"))?;

    if let Some(err) = resp["error"].as_str() {
        return Err(format!("Token exchange error: {} — {}", err, resp["error_description"].as_str().unwrap_or("")));
    }

    Ok(GoogleTokens {
        access_token:  resp["access_token"].as_str().ok_or("Missing access_token")?.to_string(),
        refresh_token: resp["refresh_token"].as_str().unwrap_or("").to_string(),
    })
}

/// Refresh an expired access token using the stored refresh token.
#[tauri::command]
pub async fn refresh_google_token(
    client_id: String,
    client_secret: String,
    refresh_token: String,
) -> Result<String, String> {
    let client_id = resolve_client_id(&client_id).to_string();
    let client_secret = resolve_client_secret(&client_secret).to_string();
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", &client_secret),
            ("refresh_token", &refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    resp["access_token"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Refresh failed: {}", resp["error"].as_str().unwrap_or("unknown")))
}

async fn write_http_response(writer: &mut (impl tokio::io::AsyncWrite + Unpin), status: u16, html: &str) {
    let status_text = if status == 200 { "OK" } else { "Bad Request" };
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    writer.write_all(response.as_bytes()).await.ok();
}

#[derive(serde::Serialize)]
pub struct GoogleTokens {
    pub access_token: String,
    pub refresh_token: String,
}

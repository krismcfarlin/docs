use crate::commands::db::get_or_open_space_db;
use crate::commands::settings::{load_settings, save_google_access_token, clear_google_tokens};
use crate::commands::oauth::{DEFAULT_CLIENT_ID, DEFAULT_CLIENT_SECRET};
use crate::state::{AppState, DEMO_USER_ID};
use nanoid::nanoid;
use tauri::State;
use std::collections::HashMap;
use image::{codecs::jpeg::JpegEncoder, ExtendedColorType, ImageEncoder};
use std::io::Cursor;

/// Refresh the stored Google access token using default credentials.
/// Returns the new token on success.
async fn refresh_google_access_token() -> Result<String, String> {
    let settings = load_settings();
    let refresh_token = settings.google_refresh_token
        .filter(|t| !t.is_empty())
        .ok_or("No Google refresh token stored")?;
    let client_id = settings.google_client_id
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string());
    let client_secret = settings.google_client_secret
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| DEFAULT_CLIENT_SECRET.to_string());

    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let error_code = resp["error"].as_str().unwrap_or("");
    if error_code == "invalid_grant" {
        // Refresh token revoked — clear both tokens so UI shows re-auth prompt
        clear_google_tokens();
        return Err("invalid_grant: Google session revoked. Re-authenticate in Settings → Google Docs.".to_string());
    }
    let new_token = resp["access_token"]
        .as_str()
        .ok_or_else(|| format!("Token refresh failed: {error_code}"))?
        .to_string();

    save_google_access_token(&new_token);
    Ok(new_token)
}

// ── Google Drive file listing ─────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct GDocFile {
    pub id: String,
    pub name: String,
    pub modified_time: String,
}

/// List the user's Google Docs, ordered by most recently modified.
#[tauri::command]
pub async fn list_gdocs() -> Result<Vec<GDocFile>, String> {
    let settings = load_settings();
    let mut token = settings.google_access_token
        .filter(|t| !t.is_empty())
        .ok_or("Not connected to Google. Go to Settings → Google Docs and click Connect.")?;

    let client = reqwest::Client::new();

    let parse_files = |resp: &serde_json::Value| -> Option<Vec<GDocFile>> {
        resp["files"].as_array().map(|arr| {
            arr.iter().filter_map(|f| Some(GDocFile {
                id: f["id"].as_str()?.to_string(),
                name: f["name"].as_str()?.to_string(),
                modified_time: f["modifiedTime"].as_str().unwrap_or("").to_string(),
            })).collect()
        })
    };

    let do_request = |tok: &str| {
        client
            .get("https://www.googleapis.com/drive/v3/files")
            .query(&[
                ("q", "mimeType='application/vnd.google-apps.document' and trashed=false"),
                ("orderBy", "modifiedTime desc"),
                ("pageSize", "50"),
                ("fields", "files(id,name,modifiedTime)"),
            ])
            .bearer_auth(tok)
            .send()
    };

    let resp: serde_json::Value = do_request(&token)
        .await
        .map_err(|e| format!("Drive API request failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Drive API parse failed: {e}"))?;

    // On auth error, refresh token and retry once
    if resp["error"]["code"].as_i64() == Some(401) || resp["error"]["status"].as_str() == Some("UNAUTHENTICATED") {
        token = refresh_google_access_token().await
            .map_err(|e| format!("Token expired and refresh failed: {e}"))?;
        let resp2: serde_json::Value = do_request(&token)
            .await
            .map_err(|e| format!("Drive API request failed: {e}"))?
            .json()
            .await
            .map_err(|e| format!("Drive API parse failed: {e}"))?;
        if let Some(err) = resp2["error"]["message"].as_str() {
            return Err(format!("Drive API error: {err}"));
        }
        return parse_files(&resp2).ok_or_else(|| "Unexpected Drive API response".to_string());
    }

    if let Some(err) = resp["error"]["message"].as_str() {
        return Err(format!("Drive API error: {err}"));
    }

    parse_files(&resp).ok_or_else(|| "Unexpected Drive API response".to_string())
}

/// Fetch a Google Doc by file ID (exported as plain text).
#[tauri::command]
pub async fn fetch_gdoc_by_id(file_id: String) -> Result<String, String> {
    let url = format!("https://docs.google.com/document/d/{file_id}/edit");
    fetch_gdoc(url).await
}

// ── Google Docs API — per-tab extraction ─────────────────────────────────────

#[derive(serde::Serialize, Debug)]
pub struct GDocImport {
    pub doc_title: String,
    pub tabs: Vec<DocTab>,
}

#[derive(serde::Serialize, Debug)]
pub struct DocTab {
    pub title: String,
    pub content: String,
}

/// Fetch all tabs of a Google Doc using the Docs API.
/// Returns one entry per tab, each with its title and markdown content.
/// Requires `documents.readonly` OAuth scope.
#[tauri::command]
pub async fn fetch_gdoc_tabs(file_id: String) -> Result<GDocImport, String> {
    let settings = load_settings();
    let mut token = settings.google_access_token
        .filter(|t| !t.is_empty())
        .ok_or("Not connected to Google. Go to Settings → Google Docs and click Connect.")?;

    let client = reqwest::Client::new();

    let do_request = |tok: &str| {
        client
            .get(format!("https://docs.googleapis.com/v1/documents/{file_id}"))
            .query(&[("includeTabsContent", "true")])
            .bearer_auth(tok)
            .send()
    };

    let mut doc: serde_json::Value = do_request(&token)
        .await
        .map_err(|e| format!("Docs API request failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Docs API parse failed: {e}"))?;

    // Refresh and retry once on auth error
    if doc["error"]["code"].as_i64() == Some(401)
        || doc["error"]["status"].as_str() == Some("UNAUTHENTICATED")
    {
        token = refresh_google_access_token().await
            .map_err(|e| format!("Token expired and refresh failed: {e}"))?;
        doc = do_request(&token)
            .await
            .map_err(|e| format!("Docs API request failed: {e}"))?
            .json()
            .await
            .map_err(|e| format!("Docs API parse failed: {e}"))?;
    }

    if let Some(err) = doc["error"]["message"].as_str() {
        return Err(format!("Docs API error: {err}"));
    }

    let top_tabs = doc["tabs"].as_array()
        .ok_or("Document has no tabs field — it may be an older format")?;

    let doc_title = doc["title"].as_str().unwrap_or("Untitled").to_string();

    let images = prefetch_images(&doc, &token).await;

    if top_tabs.is_empty() {
        let content = extract_tab_markdown(&doc["body"], &images);
        return Ok(GDocImport { doc_title: doc_title.clone(), tabs: vec![DocTab { title: doc_title, content }] });
    }

    // Google Docs nests tabs via childTabs — flatten the whole tree depth-first.
    fn collect_tabs(
        tabs: &[serde_json::Value],
        images: &HashMap<String, String>,
        out: &mut Vec<DocTab>,
    ) {
        for tab in tabs {
            let title = tab.pointer("/tabProperties/title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled")
                .to_string();
            if let Some(body) = tab.pointer("/documentTab/body") {
                let content = extract_tab_markdown(body, images);
                out.push(DocTab { title, content });
            }
            if let Some(children) = tab.get("childTabs").and_then(|v| v.as_array()) {
                collect_tabs(children, images, out);
            }
        }
    }

    let mut tabs = Vec::new();
    collect_tabs(top_tabs, &images, &mut tabs);
    Ok(GDocImport { doc_title, tabs })
}

fn is_code_font(text_run: &serde_json::Value) -> bool {
    let family = text_run
        .pointer("/textStyle/weightedFontFamily/fontFamily")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    matches!(
        family,
        "Courier New" | "Courier" | "Consolas" | "Roboto Mono"
        | "Source Code Pro" | "Inconsolata" | "Fira Mono" | "Fira Code"
        | "Lucida Console" | "Monaco"
    )
}

/// Download raw image bytes from a URI with Google auth.
async fn download_image_bytes(uri: &str, token: &str) -> Option<Vec<u8>> {
    let client = reqwest::Client::new();
    let resp = client.get(uri).bearer_auth(token).send().await.ok()?;
    Some(resp.bytes().await.ok()?.to_vec())
}

/// Resize to max 1200px wide and re-encode as JPEG at quality 85.
fn optimize_image(bytes: &[u8]) -> Vec<u8> {
    let img = match image::load_from_memory(bytes) {
        Ok(i) => i,
        Err(_) => return bytes.to_vec(),
    };
    let img = if img.width() > 1200 {
        img.thumbnail(1200, u32::MAX)
    } else {
        img
    };
    let rgb = img.to_rgb8();
    let mut output = Vec::new();
    let encoder = JpegEncoder::new_with_quality(Cursor::new(&mut output), 85);
    if encoder.write_image(rgb.as_raw(), img.width(), img.height(), ExtendedColorType::Rgb8).is_err() {
        return bytes.to_vec();
    }
    output
}

/// Find or create a "Bamako Images" folder in the user's Google Drive.
/// Returns the folder ID.
async fn ensure_bamako_folder(token: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .get("https://www.googleapis.com/drive/v3/files")
        .query(&[
            ("q", "mimeType='application/vnd.google-apps.folder' and name='Bamako Images' and trashed=false"),
            ("fields", "files(id)"),
            ("pageSize", "1"),
        ])
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(id) = resp["files"][0]["id"].as_str() {
        return Ok(id.to_string());
    }

    let create_resp: serde_json::Value = client
        .post("https://www.googleapis.com/drive/v3/files")
        .bearer_auth(token)
        .query(&[("fields", "id")])
        .json(&serde_json::json!({
            "name": "Bamako Images",
            "mimeType": "application/vnd.google-apps.folder"
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    create_resp["id"].as_str()
        .ok_or_else(|| format!("Failed to create Bamako Images folder: {create_resp}"))
        .map(|s| s.to_string())
}

/// Upload JPEG bytes to Google Drive via multipart upload, make public, return HTTPS URL.
async fn upload_to_gdrive(bytes: Vec<u8>, filename: &str, folder_id: &str, token: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let boundary = "bamako_img_boundary_a3f9c2";
    let metadata = serde_json::json!({ "name": filename, "parents": [folder_id] });
    let metadata_str = serde_json::to_string(&metadata).map_err(|e| e.to_string())?;

    let mut body: Vec<u8> = format!(
        "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata_str}\r\n--{boundary}\r\nContent-Type: image/jpeg\r\n\r\n"
    ).into_bytes();
    body.extend_from_slice(&bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--").as_bytes());

    let resp: serde_json::Value = client
        .post("https://www.googleapis.com/upload/drive/v3/files")
        .query(&[("uploadType", "multipart"), ("fields", "id")])
        .header("Content-Type", format!("multipart/related; boundary={boundary}"))
        .bearer_auth(token)
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let file_id = resp["id"].as_str()
        .ok_or_else(|| format!("Drive upload failed: {resp}"))?
        .to_string();

    // Make the file publicly readable
    let _ = client
        .post(format!("https://www.googleapis.com/drive/v3/files/{file_id}/permissions"))
        .bearer_auth(token)
        .json(&serde_json::json!({ "role": "reader", "type": "anyone" }))
        .send()
        .await;

    Ok(file_id)
}

/// Download all inline images, optimize, upload to Google Drive, and return
/// object_id → public HTTPS URL for embedding directly in markdown.
async fn prefetch_images(doc: &serde_json::Value, token: &str) -> HashMap<String, String> {
    fn collect_uris(src: Option<&serde_json::Value>, out: &mut Vec<(String, String)>) {
        if let Some(obj) = src.and_then(|v| v.as_object()) {
            for (id, val) in obj {
                if let Some(uri) = val.pointer("/inlineObjectProperties/embeddedObject/imageProperties/contentUri")
                    .and_then(|v| v.as_str())
                {
                    out.push((id.clone(), uri.to_string()));
                }
            }
        }
    }
    fn collect_tabs(tabs: &[serde_json::Value], out: &mut Vec<(String, String)>) {
        for tab in tabs {
            collect_uris(tab.pointer("/documentTab/inlineObjects"), out);
            if let Some(children) = tab.get("childTabs").and_then(|v| v.as_array()) {
                collect_tabs(children, out);
            }
        }
    }
    let mut uris = Vec::new();
    collect_uris(doc.get("inlineObjects"), &mut uris);
    if let Some(tabs) = doc.get("tabs").and_then(|v| v.as_array()) {
        collect_tabs(tabs, &mut uris);
    }

    if uris.is_empty() {
        return HashMap::new();
    }

    let folder_id = match ensure_bamako_folder(token).await {
        Ok(id) => id,
        Err(e) => {
            eprintln!("[import] Failed to get/create Bamako Images folder: {e}");
            return HashMap::new();
        }
    };

    let cache_dir = {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        std::path::PathBuf::from(home).join(".bamako/image_cache")
    };
    std::fs::create_dir_all(&cache_dir).ok();

    let mut map = HashMap::new();
    for (id, uri) in uris {
        match download_image_bytes(&uri, token).await {
            Some(bytes) => {
                let optimized = optimize_image(&bytes);
                let filename = format!("{id}.jpg");
                match upload_to_gdrive(optimized.clone(), &filename, &folder_id, token).await {
                    Ok(file_id) => {
                        // Cache locally so the bamakimg:// protocol serves instantly
                        let cache_path = cache_dir.join(format!("{file_id}.jpg"));
                        std::fs::write(&cache_path, &optimized).ok();
                        eprintln!("[import] Uploaded image {id} → bamakimg://{file_id}");
                        map.insert(id, format!("bamakimg://{file_id}"));
                    }
                    Err(e) => eprintln!("[import] Failed to upload image {id}: {e}"),
                }
            }
            None => eprintln!("[import] Failed to download image {id}"),
        }
    }
    map
}

/// Tauri command: read a locally cached image (legacy spaceimg:// refs) and return as data URI.
#[tauri::command]
pub fn get_page_image(object_id: String) -> Result<String, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let path = std::path::PathBuf::from(home).join(".bamako/images").join(&object_id);
    std::fs::read_to_string(&path)
        .map_err(|e| format!("Image not found: {e}"))
}

/// Convert a Docs API body to markdown.
fn extract_tab_markdown(body: &serde_json::Value, images: &HashMap<String, String>) -> String {
    let mut md = String::new();
    let elements = match body.get("content").and_then(|c| c.as_array()) {
        Some(e) => e,
        None => return md,
    };

    let mut prev_was_list = false;

    for elem in elements {
        if let Some(para) = elem.get("paragraph") {
            let style = para.pointer("/paragraphStyle/namedStyleType")
                .and_then(|s| s.as_str())
                .unwrap_or("NORMAL_TEXT");

            let is_bullet = para.get("bullet").is_some();
            let nesting = para.pointer("/bullet/nestingLevel")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;

            let mut line = String::new();
            let mut pending_break = false; // true when a \x0b ended the previous run

            if let Some(runs) = para.get("elements").and_then(|e| e.as_array()) {
                for run in runs {
                    // Inline image — embed public Drive URL if available
                    if let Some(obj_id) = run.pointer("/inlineObjectElement/inlineObjectId").and_then(|v| v.as_str()) {
                        if !line.is_empty() { line.push_str("\n\n"); }
                        if let Some(url) = images.get(obj_id) {
                            line.push_str(&format!("![image]({url})"));
                        } else {
                            line.push_str("![image]()");
                        }
                        continue;
                    }
                    // Text run
                    if let Some(text_run) = run.get("textRun") {
                        let raw = text_run["content"].as_str().unwrap_or("");
                        let bold   = text_run.pointer("/textStyle/bold").and_then(|v| v.as_bool()).unwrap_or(false);
                        let italic = text_run.pointer("/textStyle/italic").and_then(|v| v.as_bool()).unwrap_or(false);
                        let code   = is_code_font(text_run);
                        let link   = text_run.pointer("/textStyle/link/url").and_then(|v| v.as_str());

                        // Soft line breaks are \x0b; paragraph terminator is trailing \n.
                        // Emit a break only when an actual \x0b appears (within a run via i>0,
                        // or carried from a previous run via pending_break).
                        let raw_stripped = raw.trim_end_matches('\n');
                        let parts: Vec<&str> = raw_stripped.split('\x0b').collect();

                        for (i, part) in parts.iter().enumerate() {
                            if part.is_empty() {
                                if !line.is_empty() { pending_break = true; }
                                continue;
                            }
                            if (i > 0 || pending_break) && !line.is_empty() {
                                if is_bullet { line.push_str("  \n"); } else { line.push_str("\n\n"); }
                            }
                            pending_break = false;
                            let styled = if code {
                                format!("`{part}`")
                            } else if bold && italic {
                                format!("***{part}***")
                            } else if bold {
                                format!("**{part}**")
                            } else if italic {
                                format!("*{part}*")
                            } else {
                                part.to_string()
                            };
                            if let Some(url) = link {
                                line.push_str(&format!("[{styled}]({url})"));
                            } else {
                                line.push_str(&styled);
                            }
                        }
                    }
                }
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                if prev_was_list { md.push('\n'); }
                prev_was_list = false;
                md.push('\n');
                continue;
            }

            if is_bullet {
                let indent = "  ".repeat(nesting);
                md.push_str(&format!("{indent}- {trimmed}\n"));
                prev_was_list = true;
            } else {
                if prev_was_list { md.push('\n'); }
                prev_was_list = false;
                match style {
                    "TITLE"     => md.push_str(&format!("# {trimmed}\n\n")),
                    "HEADING_1" => md.push_str(&format!("# {trimmed}\n\n")),
                    "HEADING_2" => md.push_str(&format!("## {trimmed}\n\n")),
                    "HEADING_3" => md.push_str(&format!("### {trimmed}\n\n")),
                    "HEADING_4" => md.push_str(&format!("#### {trimmed}\n\n")),
                    "HEADING_5" => md.push_str(&format!("##### {trimmed}\n\n")),
                    _           => md.push_str(&format!("{trimmed}\n\n")),
                }
            }
        } else if let Some(table) = elem.get("table") {
            if prev_was_list { md.push('\n'); }
            prev_was_list = false;
            if let Some(rows) = table.get("tableRows").and_then(|r| r.as_array()) {
                for (i, row) in rows.iter().enumerate() {
                    let cells: Vec<String> = row.get("tableCells")
                        .and_then(|c| c.as_array())
                        .map(|cells| cells.iter().map(|cell| {
                            cell.get("content").and_then(|c| c.as_array())
                                .map(|elems| extract_tab_markdown(
                                    &serde_json::json!({"content": elems}),
                                    images,
                                ).replace('\n', " ").trim().to_string())
                                .unwrap_or_default()
                        }).collect())
                        .unwrap_or_default();
                    md.push_str(&format!("| {} |\n", cells.join(" | ")));
                    // Separator row after header
                    if i == 0 {
                        let sep = cells.iter().map(|_| "---").collect::<Vec<_>>().join(" | ");
                        md.push_str(&format!("| {sep} |\n"));
                    }
                }
                md.push('\n');
            }
        }
    }
    md
}

/// Read a local file and return its text content.
#[tauri::command]
pub async fn read_file(path: String) -> Result<String, String> {
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read file: {e}"))
}

/// Fetch a Google Doc as Markdown.
/// - If Google OAuth tokens are stored in settings, uses the Drive API (works for private docs).
/// - Otherwise falls back to the public export URL (requires "Anyone with the link" sharing).
#[tauri::command]
pub async fn fetch_gdoc(url: String) -> Result<String, String> {
    let doc_id = extract_gdoc_id(&url)
        .ok_or_else(|| "Could not extract Google Doc ID from URL".to_string())?;

    let settings = load_settings();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| e.to_string())?;

    // Always export as markdown — text/plain loses all formatting and images.
    // With OAuth we can access private docs; without we try the public export URL.
    let export_url = format!(
        "https://docs.google.com/document/d/{doc_id}/export?format=md"
    );

    if let Some(mut token) = settings.google_access_token.filter(|t| !t.is_empty()) {
        let mut resp = client
            .get(&export_url)
            .bearer_auth(&token)
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        // Refresh and retry once on 401
        if resp.status() == 401 {
            token = refresh_google_access_token().await
                .map_err(|e| format!("Token expired and refresh failed: {e}"))?;
            resp = client
                .get(&export_url)
                .bearer_auth(&token)
                .header("User-Agent", "Mozilla/5.0")
                .send()
                .await
                .map_err(|e| format!("Request failed: {e}"))?;
        }
        if resp.status().is_success() {
            return resp.text().await.map_err(|e| e.to_string());
        }
        // Fall through to unauthenticated attempt
    }

    // Unauthenticated — works for docs shared "Anyone with the link"
    let resp = client
        .get(&export_url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "HTTP {}. For private docs, connect Google in Settings. For public docs, share as \"Anyone with the link\".",
            resp.status()
        ));
    }

    resp.text().await.map_err(|e| e.to_string())
}

fn extract_gdoc_id(url: &str) -> Option<String> {
    // Handles:
    //   https://docs.google.com/document/d/{ID}/edit
    //   https://docs.google.com/document/d/{ID}/view
    //   https://docs.google.com/document/d/{ID}
    url.split("/d/")
        .nth(1)
        .map(|s| s.split('/').next().unwrap_or(s).to_string())
        .filter(|id| !id.is_empty())
}

/// Import multiple pages in a single call (used when splitting a doc by headings).
#[tauri::command]
pub async fn import_pages_bulk(
    state: State<'_, AppState>,
    space_id: String,
    pages: Vec<PageInput>,
) -> Result<Vec<String>, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    let mut ids = vec![];

    for p in pages {
        // Upsert: find existing non-deleted page with same title and parent
        let mut ex = conn.query(
            "SELECT id FROM pages WHERE title = ?1 AND space_id = ?2 AND deleted_at IS NULL \
             AND (parent_page_id IS ?3) LIMIT 1",
            libsql::params![p.title.clone(), space_id.clone(), p.parent_page_id.clone()],
        ).await.map_err(|e: libsql::Error| e.to_string())?;

        let page_id = if let Some(row) = ex.next().await.map_err(|e| e.to_string())? {
            // Page exists — update its published version content
            let existing_id: String = row.get(0).map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE page_versions SET content = ?1, text_content = ?1, updated_at = datetime('now') \
                 WHERE page_id = ?2 AND is_published = 1",
                libsql::params![p.content.clone(), existing_id.clone()],
            ).await.map_err(|e: libsql::Error| e.to_string())?;
            existing_id
        } else {
            // New page
            let new_id = nanoid!();
            let version_id = nanoid!();
            conn.execute(
                "INSERT INTO pages (id, title, space_id, creator_id, parent_page_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                libsql::params![new_id.clone(), p.title.clone(), space_id.clone(), DEMO_USER_ID, p.parent_page_id.clone()],
            ).await.map_err(|e: libsql::Error| e.to_string())?;
            conn.execute(
                "INSERT INTO page_versions (id, page_id, owner_id, title, content, text_content, is_published, is_frozen, version_num) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, 1, 0, 1)",
                libsql::params![version_id, new_id.clone(), DEMO_USER_ID, p.title, p.content],
            ).await.map_err(|e: libsql::Error| e.to_string())?;
            new_id
        };
        ids.push(page_id);
    }
    Ok(ids)
}

#[derive(serde::Deserialize)]
pub struct PageInput {
    pub title: String,
    pub content: String,
    pub parent_page_id: Option<String>,
}

/// Create a page with pre-filled markdown content (import path).
#[tauri::command]
pub async fn import_page(
    state: State<'_, AppState>,
    title: String,
    space_id: String,
    content: String,
) -> Result<String, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    let page_id = nanoid!();
    let version_id = nanoid!();

    conn.execute(
        "INSERT INTO pages (id, title, space_id, creator_id, parent_page_id) VALUES (?1, ?2, ?3, ?4, NULL)",
        libsql::params![page_id.clone(), title.clone(), space_id, DEMO_USER_ID],
    )
    .await
    .map_err(|e: libsql::Error| e.to_string())?;

    conn.execute(
        "INSERT INTO page_versions (id, page_id, owner_id, title, content, text_content, is_published, is_frozen, version_num) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?5, 1, 0, 1)",
        libsql::params![version_id, page_id.clone(), DEMO_USER_ID, title, content],
    )
    .await
    .map_err(|e: libsql::Error| e.to_string())?;

    Ok(page_id)
}

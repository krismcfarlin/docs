use crate::commands::db::get_or_open_space_db;
use crate::state::{AppState, DEMO_USER_ID};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};
use tauri::State;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpaceConfig {
    pub api_key: String,
    pub model: String,
    pub synthesizer_role: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PageSynthesis {
    pub page_id: String,
    pub summary: String,
    pub key_points: Vec<String>,
    pub topics: Vec<String>,
    pub synthesized_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EntitySuggestion {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    pub description: String,
    pub mention_count: i64,
    pub status: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpaceOverview {
    pub overview: String,
    pub topics: Vec<String>,
    pub synthesized_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PageLink {
    pub relationship: String,
    pub description: String,
    pub other_page_id: String,
    pub other_page_title: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn ensure_synthesis_tables(conn: &libsql::Connection) -> Result<(), String> {
    let stmts = [
        "CREATE TABLE IF NOT EXISTS space_config (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        "CREATE TABLE IF NOT EXISTS page_summaries (
            page_id TEXT PRIMARY KEY,
            summary TEXT NOT NULL DEFAULT '',
            key_points TEXT NOT NULL DEFAULT '[]',
            topics TEXT NOT NULL DEFAULT '[]',
            content_hash TEXT NOT NULL DEFAULT '',
            synthesized_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        "CREATE TABLE IF NOT EXISTS entity_registry (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            entity_type TEXT NOT NULL DEFAULT 'concept',
            description TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'candidate',
            mention_count INTEGER NOT NULL DEFAULT 0,
            page_id TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        "CREATE TABLE IF NOT EXISTS entity_mentions (
            id TEXT PRIMARY KEY,
            entity_id TEXT NOT NULL,
            page_id TEXT NOT NULL,
            excerpt TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        "CREATE TABLE IF NOT EXISTS page_links (
            id TEXT PRIMARY KEY,
            source_page_id TEXT NOT NULL,
            target_page_id TEXT NOT NULL,
            relationship TEXT NOT NULL DEFAULT 'related',
            description TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        "CREATE TABLE IF NOT EXISTS space_overview_store (
            id TEXT PRIMARY KEY DEFAULT 'singleton',
            overview TEXT NOT NULL DEFAULT '',
            topics TEXT NOT NULL DEFAULT '[]',
            synthesized_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    ];
    for stmt in stmts {
        conn.execute(stmt, ())
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn strip_code_fences(s: &str) -> String {
    let trimmed = s.trim();
    // matches ```json\n...\n``` or ```\n...\n```
    if let Some(inner) = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
    {
        if let Some(inner) = inner.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }
    trimmed.to_string()
}

fn content_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

async fn call_openrouter(
    api_key: &str,
    model: &str,
    system: &str,
    user_msg: &str,
    json_mode: bool,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user_msg}
        ],
        "temperature": 0.3,
    });
    if json_mode {
        body["response_format"] = serde_json::json!({"type": "json_object"});
    }
    let resp = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("HTTP-Referer", "https://bamako.app")
        .header("X-Title", "Bamako")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("OpenRouter request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("OpenRouter error {}: {}", status, text));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let raw = json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .or_else(|| {
            // some models nest content differently
            json["choices"][0]["message"]["reasoning_content"]
                .as_str()
                .map(|s| s.to_string())
        })
        .ok_or_else(|| format!("No content in OpenRouter response. Raw: {}", json))?;

    // strip markdown code fences (```json ... ``` or ``` ... ```)
    let content = strip_code_fences(&raw);
    Ok(content)
}

async fn load_space_config(conn: &libsql::Connection) -> Result<SpaceConfig, String> {
    let mut rows = conn
        .query(
            "SELECT key, value FROM space_config WHERE key IN ('api_key','model','synthesizer_role')",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut api_key = String::new();
    let mut model = "minimax/minimax-m2.5".to_string();
    let mut synthesizer_role = "owner".to_string();

    while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        let k: String = row.get(0).map_err(|e| e.to_string())?;
        let v: String = row.get(1).map_err(|e| e.to_string())?;
        match k.as_str() {
            "api_key" => api_key = v,
            "model" => model = v,
            "synthesizer_role" => synthesizer_role = v,
            _ => {}
        }
    }

    if api_key.is_empty() {
        api_key = crate::commands::settings::load_settings()
            .openrouter_api_key
            .filter(|k| !k.is_empty())
            .ok_or("No API key configured. Set one in Settings → API Key.")?;
    }

    Ok(SpaceConfig { api_key, model, synthesizer_role })
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_space_config(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<Option<SpaceConfig>, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;

    match load_space_config(&conn).await {
        Ok(cfg) => Ok(Some(cfg)),
        Err(_) => Ok(None),
    }
}

#[tauri::command]
pub async fn set_space_config(
    state: State<'_, AppState>,
    space_id: String,
    api_key: String,
    model: String,
    synthesizer_role: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;

    for (key, val) in [
        ("api_key", api_key.as_str()),
        ("model", model.as_str()),
        ("synthesizer_role", synthesizer_role.as_str()),
    ] {
        conn.execute(
            "INSERT OR REPLACE INTO space_config (key, value) VALUES (?1, ?2)",
            libsql::params![key, val],
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn synthesize_page(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
) -> Result<PageSynthesis, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;

    let cfg = load_space_config(&conn).await?;

    // Fetch page content
    let mut rows = conn
        .query(
            "SELECT pv.content, pv.title FROM page_versions pv \
             WHERE pv.page_id = ?1 AND pv.is_published = 1 \
             ORDER BY pv.updated_at DESC LIMIT 1",
            libsql::params![page_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;

    let row = rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Page has no published version")?;

    let content: String = row.get(0).unwrap_or_default();
    let title: String = row.get(1).unwrap_or_else(|_| "Untitled".to_string());
    let hash = content_hash(&content);

    // Check if already synthesized with same hash
    let mut existing = conn
        .query(
            "SELECT summary, key_points, topics, synthesized_at, content_hash \
             FROM page_summaries WHERE page_id = ?1",
            libsql::params![page_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;

    if let Some(ex_row) = existing.next().await.map_err(|e| e.to_string())? {
        let existing_hash: String = ex_row.get(4).unwrap_or_default();
        if existing_hash == hash {
            let summary: String = ex_row.get(0).unwrap_or_default();
            let kp_json: String = ex_row.get(1).unwrap_or_else(|_| "[]".to_string());
            let topics_json: String = ex_row.get(2).unwrap_or_else(|_| "[]".to_string());
            let synthesized_at: String = ex_row.get(3).unwrap_or_default();
            let key_points: Vec<String> = serde_json::from_str(&kp_json).unwrap_or_default();
            let topics: Vec<String> = serde_json::from_str(&topics_json).unwrap_or_default();
            return Ok(PageSynthesis { page_id, summary, key_points, topics, synthesized_at });
        }
    }

    // Call OpenRouter
    let system = "You are a knowledge synthesis assistant. Analyze the provided document and return a JSON object with exactly these fields:\n\
        - \"summary\": string (2-3 sentences capturing the main point)\n\
        - \"key_points\": array of strings (3-7 bullet points, each under 20 words)\n\
        - \"topics\": array of strings (2-5 topic tags, lowercase)\n\
        - \"entities\": array of objects with \"name\" (string), \"type\" (\"person\"|\"project\"|\"concept\"|\"decision\"), \"description\" (string, one sentence)\n\
        Return only valid JSON, no markdown wrapping.";

    let user_msg = format!("Title: {}\n\n{}", title, &content[..content.len().min(8000)]);
    let raw = call_openrouter(&cfg.api_key, &cfg.model, system, &user_msg, true).await?;

    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse synthesis JSON: {} — raw: {}", e, &raw[..raw.len().min(200)]))?;

    let summary = parsed["summary"].as_str().unwrap_or("").to_string();
    let key_points: Vec<String> = parsed["key_points"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let topics: Vec<String> = parsed["topics"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    let kp_json = serde_json::to_string(&key_points).unwrap_or_else(|_| "[]".to_string());
    let topics_json = serde_json::to_string(&topics).unwrap_or_else(|_| "[]".to_string());

    // Upsert summary
    conn.execute(
        "INSERT OR REPLACE INTO page_summaries (page_id, summary, key_points, topics, content_hash, synthesized_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
        libsql::params![page_id.clone(), summary.clone(), kp_json, topics_json, hash],
    )
    .await
    .map_err(|e| e.to_string())?;

    // Process entities
    if let Some(entities) = parsed["entities"].as_array() {
        for entity in entities {
            let name = match entity["name"].as_str() { Some(n) if !n.is_empty() => n, _ => continue };
            let etype = entity["type"].as_str().unwrap_or("concept");
            let desc = entity["description"].as_str().unwrap_or("");

            // Check existing
            let mut ex_rows = conn
                .query(
                    "SELECT id, mention_count FROM entity_registry WHERE lower(name) = lower(?1)",
                    libsql::params![name],
                )
                .await
                .map_err(|e| e.to_string())?;

            if let Some(ex) = ex_rows.next().await.map_err(|e| e.to_string())? {
                let eid: String = ex.get(0).map_err(|e| e.to_string())?;
                let count: i64 = ex.get(1).unwrap_or(0);
                conn.execute(
                    "UPDATE entity_registry SET mention_count = ?1 WHERE id = ?2",
                    libsql::params![count + 1, eid.clone()],
                )
                .await
                .map_err(|e| e.to_string())?;

                // Insert mention if not already exists for this page
                let mention_id = nanoid!();
                let excerpt = &content[..content.len().min(200)];
                conn.execute(
                    "INSERT OR IGNORE INTO entity_mentions (id, entity_id, page_id, excerpt) \
                     VALUES (?1, ?2, ?3, ?4)",
                    libsql::params![mention_id, eid, page_id.clone(), excerpt],
                )
                .await
                .ok();
            } else {
                let eid = nanoid!();
                conn.execute(
                    "INSERT INTO entity_registry (id, name, entity_type, description, status, mention_count) \
                     VALUES (?1, ?2, ?3, ?4, 'candidate', 1)",
                    libsql::params![eid.clone(), name, etype, desc],
                )
                .await
                .map_err(|e| e.to_string())?;

                let mention_id = nanoid!();
                let excerpt = &content[..content.len().min(200)];
                conn.execute(
                    "INSERT INTO entity_mentions (id, entity_id, page_id, excerpt) \
                     VALUES (?1, ?2, ?3, ?4)",
                    libsql::params![mention_id, eid, page_id.clone(), excerpt],
                )
                .await
                .ok();
            }
        }
    }

    // Fetch the synthesized_at we just wrote
    let mut ts_rows = conn
        .query(
            "SELECT synthesized_at FROM page_summaries WHERE page_id = ?1",
            libsql::params![page_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;
    let synthesized_at = ts_rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .and_then(|r| r.get::<String>(0).ok())
        .unwrap_or_default();

    Ok(PageSynthesis { page_id, summary, key_points, topics, synthesized_at })
}

#[tauri::command]
pub async fn get_page_synthesis(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
) -> Result<Option<PageSynthesis>, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;

    let mut rows = conn
        .query(
            "SELECT summary, key_points, topics, synthesized_at \
             FROM page_summaries WHERE page_id = ?1",
            libsql::params![page_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;

    if let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        let summary: String = row.get(0).unwrap_or_default();
        let kp_json: String = row.get(1).unwrap_or_else(|_| "[]".to_string());
        let topics_json: String = row.get(2).unwrap_or_else(|_| "[]".to_string());
        let synthesized_at: String = row.get(3).unwrap_or_default();
        let key_points: Vec<String> = serde_json::from_str(&kp_json).unwrap_or_default();
        let topics: Vec<String> = serde_json::from_str(&topics_json).unwrap_or_default();
        Ok(Some(PageSynthesis { page_id, summary, key_points, topics, synthesized_at }))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub async fn get_entity_suggestions(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<Vec<EntitySuggestion>, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;

    let mut rows = conn
        .query(
            "SELECT id, name, entity_type, description, mention_count, status \
             FROM entity_registry WHERE status != 'dismissed' \
             ORDER BY mention_count DESC, created_at DESC LIMIT 50",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        results.push(EntitySuggestion {
            id: row.get(0).map_err(|e| e.to_string())?,
            name: row.get(1).map_err(|e| e.to_string())?,
            entity_type: row.get(2).map_err(|e| e.to_string())?,
            description: row.get(3).unwrap_or_default(),
            mention_count: row.get(4).unwrap_or(0),
            status: row.get(5).unwrap_or_else(|_| "candidate".to_string()),
        });
    }
    Ok(results)
}

#[tauri::command]
pub async fn promote_entity(
    state: State<'_, AppState>,
    space_id: String,
    entity_id: String,
) -> Result<String, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;

    // Ensure is_entity_page column exists
    conn.execute("ALTER TABLE pages ADD COLUMN is_entity_page INTEGER NOT NULL DEFAULT 0", ())
        .await
        .ok();

    let cfg = load_space_config(&conn).await?;

    // Get entity
    let mut ent_rows = conn
        .query(
            "SELECT name, entity_type, description FROM entity_registry WHERE id = ?1",
            libsql::params![entity_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;
    let ent_row = ent_rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Entity not found")?;
    let name: String = ent_row.get(0).map_err(|e| e.to_string())?;
    let etype: String = ent_row.get(1).unwrap_or_default();
    let desc: String = ent_row.get(2).unwrap_or_default();

    // Get mentions
    let mut mention_rows = conn
        .query(
            "SELECT em.excerpt, p.title FROM entity_mentions em \
             JOIN pages p ON p.id = em.page_id \
             WHERE em.entity_id = ?1 LIMIT 20",
            libsql::params![entity_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut mentions_list = String::new();
    while let Some(row) = mention_rows.next().await.map_err(|e| e.to_string())? {
        let excerpt: String = row.get(0).unwrap_or_default();
        let page_title: String = row.get(1).unwrap_or_default();
        mentions_list.push_str(&format!("- Document \"{}\": {}\n", page_title, excerpt));
    }

    let system = "You are a knowledge base curator. Write a concise wiki-style page for the given entity in markdown. \
        Include what it is, key facts, and how it relates to the documents it appears in. Be factual and concise. \
        Use markdown headers, bullet points, and clear structure.";
    let user_msg = format!(
        "Entity: {} (type: {})\nDescription: {}\n\nMentioned in these documents:\n{}",
        name, etype, desc, mentions_list
    );

    let content = call_openrouter(&cfg.api_key, &cfg.model, system, &user_msg, false).await?;

    // Create entity page
    let page_id = nanoid!();
    let version_id = nanoid!();

    conn.execute(
        "INSERT INTO pages (id, title, space_id, creator_id, is_entity_page) \
         VALUES (?1, ?2, ?3, ?4, 1)",
        libsql::params![page_id.clone(), name.clone(), space_id.clone(), DEMO_USER_ID],
    )
    .await
    .map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO page_versions (id, page_id, owner_id, title, content, text_content, is_published, version_num) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 1)",
        libsql::params![version_id, page_id.clone(), DEMO_USER_ID, name.clone(), content.clone(), content],
    )
    .await
    .map_err(|e| e.to_string())?;

    // Update entity registry
    conn.execute(
        "UPDATE entity_registry SET status = 'promoted', page_id = ?1 WHERE id = ?2",
        libsql::params![page_id.clone(), entity_id],
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(page_id)
}

#[tauri::command]
pub async fn dismiss_entity(
    state: State<'_, AppState>,
    space_id: String,
    entity_id: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;

    conn.execute(
        "UPDATE entity_registry SET status = 'dismissed' WHERE id = ?1",
        libsql::params![entity_id],
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn update_space_overview(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<SpaceOverview, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;

    let cfg = load_space_config(&conn).await?;

    // Get current overview
    let mut ov_rows = conn
        .query("SELECT overview FROM space_overview_store WHERE id = 'singleton'", ())
        .await
        .map_err(|e| e.to_string())?;
    let existing_overview = ov_rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .and_then(|r| r.get::<String>(0).ok())
        .unwrap_or_default();

    // Get up to 20 recent summaries
    let mut sum_rows = conn
        .query(
            "SELECT ps.page_id, ps.summary, ps.topics, p.title \
             FROM page_summaries ps \
             LEFT JOIN pages p ON p.id = ps.page_id \
             ORDER BY ps.synthesized_at DESC LIMIT 20",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut summaries_text = String::new();
    while let Some(row) = sum_rows.next().await.map_err(|e| e.to_string())? {
        let title: String = row.get(3).unwrap_or_else(|_| "Untitled".to_string());
        let summary: String = row.get(1).unwrap_or_default();
        summaries_text.push_str(&format!("- \"{}\": {}\n", title, summary));
    }

    if summaries_text.is_empty() {
        return Err("No synthesized documents yet".to_string());
    }

    let system = "You are a knowledge base curator. Update the space overview to incorporate new document summaries. \
        Return JSON with:\n\
        - \"overview\": string (3-5 sentences about what this knowledge base covers)\n\
        - \"topics\": array of strings (up to 10 major topic areas, lowercase)\n\
        Keep the best parts of the existing overview and integrate new information. Return only valid JSON.";

    let user_msg = format!(
        "Current overview: {}\n\nNew/updated summaries:\n{}",
        if existing_overview.is_empty() { "(none yet)" } else { &existing_overview },
        summaries_text
    );

    let raw = call_openrouter(&cfg.api_key, &cfg.model, system, &user_msg, true).await?;
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse overview JSON: {} — raw: {}", e, &raw[..raw.len().min(300)]))?;

    let overview = parsed["overview"].as_str().unwrap_or("").to_string();
    let topics: Vec<String> = parsed["topics"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let topics_json = serde_json::to_string(&topics).unwrap_or_else(|_| "[]".to_string());

    conn.execute(
        "INSERT OR REPLACE INTO space_overview_store (id, overview, topics, synthesized_at) \
         VALUES ('singleton', ?1, ?2, datetime('now'))",
        libsql::params![overview.clone(), topics_json],
    )
    .await
    .map_err(|e| e.to_string())?;

    let mut ts_rows = conn
        .query("SELECT synthesized_at FROM space_overview_store WHERE id = 'singleton'", ())
        .await
        .map_err(|e| e.to_string())?;
    let synthesized_at = ts_rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .and_then(|r| r.get::<String>(0).ok())
        .unwrap_or_default();

    Ok(SpaceOverview { overview, topics, synthesized_at })
}

#[tauri::command]
pub async fn get_space_overview(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<Option<SpaceOverview>, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;

    let mut rows = conn
        .query(
            "SELECT overview, topics, synthesized_at FROM space_overview_store WHERE id = 'singleton'",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

    if let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        let overview: String = row.get(0).unwrap_or_default();
        let topics_json: String = row.get(1).unwrap_or_else(|_| "[]".to_string());
        let synthesized_at: String = row.get(2).unwrap_or_default();
        if overview.is_empty() { return Ok(None); }
        let topics: Vec<String> = serde_json::from_str(&topics_json).unwrap_or_default();
        Ok(Some(SpaceOverview { overview, topics, synthesized_at }))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub async fn get_page_links(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
) -> Result<Vec<PageLink>, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;

    let mut rows = conn
        .query(
            "SELECT pl.relationship, pl.description,
                    CASE WHEN pl.source_page_id = ?1 THEN pl.target_page_id ELSE pl.source_page_id END as other_page_id,
                    p.title as other_page_title
             FROM page_links pl
             JOIN pages p ON p.id = (CASE WHEN pl.source_page_id = ?1 THEN pl.target_page_id ELSE pl.source_page_id END)
             WHERE (pl.source_page_id = ?1 OR pl.target_page_id = ?1)
               AND p.deleted_at IS NULL
             ORDER BY pl.created_at DESC LIMIT 10",
            libsql::params![page_id],
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        results.push(PageLink {
            relationship: row.get(0).unwrap_or_default(),
            description: row.get(1).unwrap_or_default(),
            other_page_id: row.get(2).unwrap_or_default(),
            other_page_title: row.get(3).unwrap_or_else(|_| "Untitled".to_string()),
        });
    }
    Ok(results)
}

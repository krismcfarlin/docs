use crate::commands::db::get_or_open_space_db;
use crate::state::{AppState, DEMO_USER_ID};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

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
        "CREATE TABLE IF NOT EXISTS entity_relations (
            id TEXT PRIMARY KEY,
            from_entity_id TEXT NOT NULL,
            to_entity_id TEXT NOT NULL,
            relationship TEXT NOT NULL DEFAULT '',
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
    // Ensure is_entity_page column exists on pages (idempotent)
    conn.execute("ALTER TABLE pages ADD COLUMN is_entity_page INTEGER NOT NULL DEFAULT 0", ())
        .await
        .ok();
    // Ensure confidence and is_inferred columns exist on entity_registry (idempotent)
    conn.execute("ALTER TABLE entity_registry ADD COLUMN confidence REAL NOT NULL DEFAULT 1.0", ())
        .await
        .ok();
    conn.execute("ALTER TABLE entity_registry ADD COLUMN is_inferred INTEGER NOT NULL DEFAULT 0", ())
        .await
        .ok();
    Ok(())
}

fn update_mentioned_in_section(content: &str, src_title: &str) -> String {
    let mention_line = format!("- [[{}]]", src_title);
    if let Some(pos) = content.find("## Mentioned In") {
        if content.contains(&mention_line) {
            return content.to_string();
        }
        // Find end of the "## Mentioned In" line and insert after it
        let after_heading = pos + "## Mentioned In".len();
        let mut result = content.to_string();
        result.insert_str(after_heading, &format!("\n{}", mention_line));
        result
    } else {
        format!("{}\n\n## Mentioned In\n{}\n", content.trim_end(), mention_line)
    }
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
    let msg = &json["choices"][0]["message"];
    let raw = msg["content"].as_str().map(|s| s.to_string())
        // DeepSeek / Qwen style
        .or_else(|| msg["reasoning_content"].as_str().map(|s| s.to_string()))
        // MiniMax and other reasoning models: output lands in `reasoning`
        .or_else(|| msg["reasoning"].as_str().map(|s| s.to_string()))
        // reasoning_details array (MiniMax alternate path)
        .or_else(|| {
            msg["reasoning_details"].as_array()?.iter().find_map(|d| d["text"].as_str().map(|s| s.to_string()))
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
pub async fn clear_synthesis_data(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    // Delete page_versions for wiki/entity pages first (FK constraint)
    conn.execute(
        "DELETE FROM page_versions WHERE page_id IN (SELECT id FROM pages WHERE is_entity_page = 1)",
        (),
    ).await.map_err(|e| e.to_string())?;

    // Delete wiki/entity pages themselves
    conn.execute("DELETE FROM pages WHERE is_entity_page = 1", ()).await.map_err(|e| e.to_string())?;

    for stmt in [
        "DELETE FROM entity_registry",
        "DELETE FROM entity_mentions",
        "DELETE FROM entity_relations",
        "DELETE FROM page_summaries",
        "DELETE FROM page_links",
        "DELETE FROM space_overview_store",
        "DELETE FROM page_embeddings",
    ] {
        conn.execute(stmt, ()).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn create_wiki_stubs(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<usize, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;

    // Load all non-dismissed entities (both new candidates and existing promoted ones)
    let mut rows = conn.query(
        "SELECT id, name, entity_type, description, page_id FROM entity_registry \
         WHERE status != 'dismissed'",
        (),
    ).await.map_err(|e| e.to_string())?;

    let mut candidates: Vec<(String, String, String, String, Option<String>)> = Vec::new();
    while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        let id: String = row.get(0).map_err(|e| e.to_string())?;
        let name: String = row.get(1).map_err(|e| e.to_string())?;
        let etype: String = row.get(2).unwrap_or_else(|_| "Concept".to_string());
        let desc: String = row.get(3).unwrap_or_default();
        let page_id: Option<String> = row.get::<String>(4).ok().filter(|s| !s.is_empty());
        candidates.push((id, name, etype, desc, page_id));
    }

    if candidates.is_empty() { return Ok(0); }

    let wiki_id = ensure_wiki_root(&conn, &space_id).await?;
    let mut created = 0usize;

    for (eid, name, etype, desc, existing_page_id) in &candidates {
        // Fetch source pages that mention this entity
        let mut src_rows = conn.query(
            "SELECT DISTINCT p.title FROM entity_mentions em \
             JOIN pages p ON p.id = em.page_id \
             WHERE em.entity_id = ?1 AND p.deleted_at IS NULL AND p.is_entity_page = 0",
            libsql::params![eid.clone()],
        ).await.map_err(|e| e.to_string())?;
        let mut source_pages: Vec<String> = Vec::new();
        while let Some(row) = src_rows.next().await.map_err(|e| e.to_string())? {
            source_pages.push(row.get::<String>(0).unwrap_or_default());
        }

        // Fetch related entities (via entity_relations)
        let mut rel_rows = conn.query(
            "SELECT er2.name, erel.relationship FROM entity_relations erel \
             JOIN entity_registry er2 ON er2.id = erel.to_entity_id \
             WHERE erel.from_entity_id = ?1 \
             UNION \
             SELECT er2.name, erel.relationship FROM entity_relations erel \
             JOIN entity_registry er2 ON er2.id = erel.from_entity_id \
             WHERE erel.to_entity_id = ?1 \
             LIMIT 10",
            libsql::params![eid.clone()],
        ).await.map_err(|e| e.to_string())?;
        let mut related: Vec<(String, String)> = Vec::new();
        while let Some(row) = rel_rows.next().await.map_err(|e| e.to_string())? {
            related.push((row.get(0).unwrap_or_default(), row.get(1).unwrap_or_default()));
        }

        // Fetch mention count
        let mc_row = conn.query(
            "SELECT mention_count FROM entity_registry WHERE id = ?1",
            libsql::params![eid.clone()],
        ).await.ok();
        let mention_count: i64 = if let Some(mut r) = mc_row {
            r.next().await.ok().flatten().and_then(|row| row.get::<i64>(0).ok()).unwrap_or(0)
        } else { 0 };

        // Build rich markdown stub
        let mut stub = format!("# {}\n\n**Type:** {} · **Mentions:** {}\n\n{}\n", name, etype, mention_count, desc);

        if !source_pages.is_empty() {
            stub.push_str("\n## Mentioned In\n");
            for title in &source_pages {
                stub.push_str(&format!("- [[{}]]\n", title));
            }
        }

        if !related.is_empty() {
            stub.push_str("\n## Related Entities\n");
            for (rel_name, rel_type) in &related {
                stub.push_str(&format!("- [[{}]] — {}\n", rel_name, rel_type));
            }
        }

        if let Some(pid) = existing_page_id {
            // Update existing wiki page content
            conn.execute(
                "UPDATE page_versions SET content = ?1, text_content = ?2, updated_at = datetime('now') \
                 WHERE page_id = ?3 AND version_num = (SELECT MAX(version_num) FROM page_versions WHERE page_id = ?3)",
                libsql::params![stub.clone(), stub.clone(), pid.clone()],
            ).await.map_err(|e| e.to_string())?;
            created += 1;
        } else {
            // Create new wiki page
            let ep_id = nanoid!();
            let ev_id = nanoid!();
            conn.execute(
                "INSERT INTO pages (id, title, space_id, creator_id, parent_page_id, is_entity_page) \
                 VALUES (?1, ?2, ?3, ?4, ?5, 1)",
                libsql::params![ep_id.clone(), name.clone(), space_id.clone(), DEMO_USER_ID, wiki_id.clone()],
            ).await.map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO page_versions (id, page_id, owner_id, title, content, text_content, is_published, version_num) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 1)",
                libsql::params![ev_id, ep_id.clone(), DEMO_USER_ID, name.clone(), stub.clone(), stub],
            ).await.map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE entity_registry SET status = 'promoted', page_id = ?1 WHERE id = ?2",
                libsql::params![ep_id, eid.clone()],
            ).await.map_err(|e| e.to_string())?;
            created += 1;
        }
    }

    Ok(created)
}

#[tauri::command]
pub async fn force_resynthesize(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;
    // Clear hashes and entity graph data so next Process re-runs everything
    conn.execute("UPDATE page_summaries SET content_hash = ''", ())
        .await.map_err(|e| e.to_string())?;
    for stmt in [
        "DELETE FROM entity_registry",
        "DELETE FROM entity_mentions",
        "DELETE FROM entity_relations",
        "DELETE FROM page_links",
    ] {
        conn.execute(stmt, ()).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

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
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
) -> Result<PageSynthesis, String> {
    // ── Read phase (pre-LLM) ──────────────────────────────────────────────────
    let (content, title, hash, cfg) = {
        let db = get_or_open_space_db(&state, &space_id).await?;
        let conn = db.connect().map_err(|e| e.to_string())?;
        ensure_synthesis_tables(&conn).await?;

        let cfg = load_space_config(&conn).await?;

        let mut rows = conn
            .query(
                "SELECT pv.content, pv.title FROM page_versions pv \
                 WHERE pv.page_id = ?1 \
                 ORDER BY pv.is_published DESC, pv.updated_at DESC LIMIT 1",
                libsql::params![page_id.clone()],
            )
            .await
            .map_err(|e| e.to_string())?;

        let row = rows
            .next()
            .await
            .map_err(|e| e.to_string())?
            .ok_or("Page has no versions")?;

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
                app.emit("synthesis:stage", serde_json::json!({
                    "page_id": page_id,
                    "stage": "done",
                    "label": format!("Up to date: {}", title),
                })).ok();
                return Ok(PageSynthesis { page_id, summary, key_points, topics, synthesized_at });
            }
        }
        (content, title, hash, cfg)
        // conn and db dropped here — stream released before LLM call
    };

    // Stage 1: content read, about to call LLM for summary
    app.emit("synthesis:stage", serde_json::json!({
        "page_id": page_id,
        "stage": "summarizing",
        "label": format!("Summarizing: {}…", title),
    })).ok();

    // ── LLM call (connection-free) ────────────────────────────────────────────
    let system = crate::commands::prompts::PAGE_SUMMARIZER;

    let user_msg = format!("Title: {}\n\n{}", title, &content[..content.len().min(8000)]);
    let raw = call_openrouter(&cfg.api_key, &cfg.model, system, &user_msg, true).await?;

    // ── Write phase: fresh connection after LLM completes ────────────────────
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

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

    // Call graph-flow pipeline for entity extraction with per-step progress events
    let page_id_clone = page_id.clone();
    let app_clone = app.clone();
    let resolved = crate::commands::graph_synthesis::run_graph_synthesis(
        &bamako_synthesis::GraphInput {
            title: &title,
            content: &content,
            api_key: &cfg.api_key,
            model: &cfg.model,
        },
        move |msg| {
            app_clone.emit("synthesis:stage", serde_json::json!({
                "page_id": page_id_clone,
                "stage": "progress",
                "label": msg,
            })).ok();
        },
    ).await;

    // Process resolved entities (best-effort — errors don't fail the whole synthesis)
    if let Ok(graph) = resolved {
        let excerpt = &content[..content.len().min(200)];
        // Map graph-flow node id → entity_registry id (needed to write edges)
        let mut node_id_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();

        for node in &graph.nodes {
            let name = node.name.as_str();
            if name.is_empty() { continue; }
            let etype = node.entity_type.as_str();
            let desc = node.description.as_str();
            let confidence = node.confidence as f64;

            let mut ex_rows = conn
                .query(
                    "SELECT id, mention_count FROM entity_registry WHERE lower(name) = lower(?1)",
                    libsql::params![name],
                )
                .await
                .map_err(|e| e.to_string())?;

            let eid = if let Some(ex) = ex_rows.next().await.map_err(|e| e.to_string())? {
                let eid: String = ex.get(0).map_err(|e| e.to_string())?;
                let count: i64 = ex.get(1).unwrap_or(0);
                conn.execute(
                    "UPDATE entity_registry SET mention_count = ?1, confidence = MAX(confidence, ?2) WHERE id = ?3",
                    libsql::params![count + 1, confidence, eid.clone()],
                ).await.map_err(|e| e.to_string())?;
                eid
            } else {
                let eid = nanoid!();
                conn.execute(
                    "INSERT INTO entity_registry \
                     (id, name, entity_type, description, status, mention_count, confidence, is_inferred) \
                     VALUES (?1, ?2, ?3, ?4, 'promoted', 1, ?5, 0)",
                    libsql::params![eid.clone(), name, etype, desc, confidence],
                ).await.map_err(|e| e.to_string())?;
                eid
            };

            let mention_id = nanoid!();
            conn.execute(
                "INSERT OR IGNORE INTO entity_mentions (id, entity_id, page_id, excerpt) VALUES (?1, ?2, ?3, ?4)",
                libsql::params![mention_id, eid.clone(), page_id.clone(), excerpt],
            ).await.ok();

            node_id_map.insert(node.id.clone(), eid);
        }

        // Write entity-entity relationships
        for edge in &graph.edges {
            if let (Some(from_eid), Some(to_eid)) = (
                node_id_map.get(&edge.from_id),
                node_id_map.get(&edge.to_id),
            ) {
                let rel_id = nanoid!();
                conn.execute(
                    "INSERT OR IGNORE INTO entity_relations \
                     (id, from_entity_id, to_entity_id, relationship, description) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    libsql::params![rel_id, from_eid.clone(), to_eid.clone(),
                                    edge.relationship.clone(), edge.description.clone()],
                ).await.ok();
            }
        }

        // Auto-promote new entities as stub wiki pages
        if !node_id_map.is_empty() {
            let wiki_root_id = ensure_wiki_root(&conn, &space_id).await.ok();
            if let Some(wiki_id) = wiki_root_id {
                for node in &graph.nodes {
                    if node.name.is_empty() { continue; }
                    let eid = match node_id_map.get(&node.id) {
                        Some(e) => e.clone(),
                        None => continue,
                    };

                    // Check if entity already has a wiki page
                    let already_has_page = {
                        let mut chk = conn.query(
                            "SELECT page_id FROM entity_registry WHERE id = ?1",
                            libsql::params![eid.clone()],
                        ).await.ok();
                        if let Some(mut rows) = chk {
                            rows.next().await.ok().flatten()
                                .and_then(|r| r.get::<Option<String>>(0).ok())
                                .flatten()
                                .is_some()
                        } else {
                            false
                        }
                    };

                    if already_has_page { continue; }

                    // Build stub markdown page from in-memory graph data
                    let stub_content = format!(
                        "# {}\n\n**Type:** {}\n\n{}\n",
                        node.name, node.entity_type, node.description
                    );

                    let ep_id = nanoid!();
                    let ev_id = nanoid!();
                    conn.execute(
                        "INSERT INTO pages (id, title, space_id, creator_id, parent_page_id, is_entity_page) \
                         VALUES (?1, ?2, ?3, ?4, ?5, 1)",
                        libsql::params![ep_id.clone(), node.name.clone(), space_id.clone(), DEMO_USER_ID, wiki_id.clone()],
                    ).await.ok();
                    conn.execute(
                        "INSERT INTO page_versions (id, page_id, owner_id, title, content, text_content, is_published, version_num) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 1)",
                        libsql::params![ev_id, ep_id.clone(), DEMO_USER_ID, node.name.clone(), stub_content.clone(), stub_content],
                    ).await.ok();
                    conn.execute(
                        "UPDATE entity_registry SET status = 'promoted', page_id = ?1 WHERE id = ?2",
                        libsql::params![ep_id, eid],
                    ).await.ok();
                }
            }
        }

        // Cross-link: source page → wiki pages it touches
        // Also update wiki stub content with "Mentioned In" sections
        {
            // Collect entity IDs that have wiki pages
            let mut wiki_links: Vec<(String, String, String)> = Vec::new(); // (wiki_page_id, entity_name, entity_type)

            for (_node_id, eid) in &node_id_map {
                let rows = conn.query(
                    "SELECT page_id, name, entity_type FROM entity_registry WHERE id = ?1 AND page_id IS NOT NULL AND page_id != ''",
                    libsql::params![eid.clone()],
                ).await.ok();
                if let Some(mut r) = rows {
                    if let Ok(Some(row)) = r.next().await {
                        let wiki_pid: String = row.get(0).unwrap_or_default();
                        let ename: String = row.get(1).unwrap_or_default();
                        let etype: String = row.get(2).unwrap_or_default();
                        if !wiki_pid.is_empty() {
                            wiki_links.push((wiki_pid, ename, etype));
                        }
                    }
                }
            }

            // Write source_page → wiki_page links
            for (wiki_pid, ename, _) in &wiki_links {
                let link_id = nanoid!();
                conn.execute(
                    "INSERT OR IGNORE INTO page_links (id, source_page_id, target_page_id, relationship, description) \
                     VALUES (?1, ?2, ?3, 'mentions', ?4)",
                    libsql::params![link_id, page_id.clone(), wiki_pid.clone(), format!("Source mentions entity: {}", ename)],
                ).await.ok();
            }

            // Update wiki stub "Mentioned In" section
            for (wiki_pid, _, _) in &wiki_links {
                // Get current source page title
                let t_rows = conn.query(
                    "SELECT title FROM pages WHERE id = ?1",
                    libsql::params![page_id.clone()],
                ).await.ok();
                let src_title = if let Some(mut r) = t_rows {
                    r.next().await.ok().flatten()
                        .and_then(|row| row.get::<String>(0).ok())
                        .unwrap_or_else(|| "Untitled".to_string())
                } else { "Untitled".to_string() };

                // Get current wiki page version content
                let cv_rows = conn.query(
                    "SELECT id, content FROM page_versions WHERE page_id = ?1 ORDER BY version_num DESC LIMIT 1",
                    libsql::params![wiki_pid.clone()],
                ).await.ok();
                if let Some(mut r) = cv_rows {
                    if let Ok(Some(row)) = r.next().await {
                        let ver_id: String = row.get(0).unwrap_or_default();
                        let existing_content: String = row.get(1).unwrap_or_default();
                        let updated_content = update_mentioned_in_section(&existing_content, &src_title);
                        conn.execute(
                            "UPDATE page_versions SET content = ?1, text_content = ?1 WHERE id = ?2",
                            libsql::params![updated_content, ver_id],
                        ).await.ok();
                    }
                }
            }

            // Wiki→Wiki cross-links: find other wiki pages that share source pages with this page
            if wiki_links.len() > 1 {
                let wiki_ids: Vec<String> = wiki_links.iter().map(|(id, _, _)| id.clone()).collect();
                for i in 0..wiki_ids.len() {
                    for j in (i + 1)..wiki_ids.len() {
                        let link_id = nanoid!();
                        conn.execute(
                            "INSERT OR IGNORE INTO page_links (id, source_page_id, target_page_id, relationship, description) \
                             VALUES (?1, ?2, ?3, 'co-occurs', 'Entities co-occur in shared source documents')",
                            libsql::params![link_id, wiki_ids[i].clone(), wiki_ids[j].clone()],
                        ).await.ok();
                    }
                }
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

    // Stage 4: all done
    app.emit("synthesis:stage", serde_json::json!({
        "page_id": page_id,
        "stage": "done",
        "label": format!("Done: {}", title),
    })).ok();

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

/// Find or create the "Wiki" root page for a space.
/// All promoted entity pages are nested under this page.
async fn ensure_wiki_root(conn: &libsql::Connection, space_id: &str) -> Result<String, String> {
    let mut rows = conn
        .query(
            "SELECT id FROM pages WHERE title = 'Wiki' AND space_id = ?1 AND deleted_at IS NULL \
             AND is_entity_page = 0 ORDER BY created_at ASC LIMIT 1",
            libsql::params![space_id.to_string()],
        )
        .await
        .map_err(|e| e.to_string())?;

    if let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        return row.get::<String>(0).map_err(|e| e.to_string());
    }

    // Create it
    let wiki_id = nanoid!();
    let wiki_version_id = nanoid!();
    conn.execute(
        "INSERT INTO pages (id, title, space_id, creator_id, parent_page_id, is_entity_page) \
         VALUES (?1, 'Wiki', ?2, ?3, NULL, 0)",
        libsql::params![wiki_id.clone(), space_id.to_string(), DEMO_USER_ID],
    )
    .await
    .map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO page_versions (id, page_id, owner_id, title, content, text_content, is_published, version_num) \
         VALUES (?1, ?2, ?3, 'Wiki', '', '', 1, 1)",
        libsql::params![wiki_version_id, wiki_id.clone(), DEMO_USER_ID],
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(wiki_id)
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

    let system = crate::commands::prompts::ENTITY_PAGE_WRITER;
    let user_msg = format!(
        "Entity: {} (type: {})\nDescription: {}\n\nMentioned in these documents:\n{}",
        name, etype, desc, mentions_list
    );

    let content = call_openrouter(&cfg.api_key, &cfg.model, system, &user_msg, false).await?;

    // Fresh connection after LLM call — previous stream may have expired
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    let wiki_id = ensure_wiki_root(&conn, &space_id).await?;

    // Create entity page nested under Wiki root
    let page_id = nanoid!();
    let version_id = nanoid!();

    conn.execute(
        "INSERT INTO pages (id, title, space_id, creator_id, parent_page_id, is_entity_page) \
         VALUES (?1, ?2, ?3, ?4, ?5, 1)",
        libsql::params![page_id.clone(), name.clone(), space_id.clone(), DEMO_USER_ID, wiki_id],
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
    // Collect all data into memory first, then drop conn before the LLM call
    // to avoid "database is locked" from an active cursor during the write.
    let (cfg, existing_overview, summaries_text) = {
        let db = get_or_open_space_db(&state, &space_id).await?;
        let conn = db.connect().map_err(|e| e.to_string())?;
        ensure_synthesis_tables(&conn).await?;

        let cfg = load_space_config(&conn).await?;

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
        // conn and all cursors dropped here
        (cfg, existing_overview, summaries_text)
    };

    if summaries_text.is_empty() {
        return Err("No synthesized documents yet".to_string());
    }

    let system = crate::commands::prompts::SPACE_OVERVIEW_UPDATER;

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

    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;

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

// ── Graph ─────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub node_type: String,
    pub entity_type: Option<String>,
    pub status: Option<String>,
    pub mention_count: Option<i64>,
    pub description: Option<String>,
}

#[derive(Serialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub edge_type: String,
    pub label: Option<String>,
}

#[derive(Serialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[tauri::command]
pub async fn get_graph_data(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<GraphData, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;

    // --- Page nodes (only synthesized pages) ---
    let mut page_rows = conn
        .query(
            "SELECT p.id, p.title FROM pages p \
             INNER JOIN page_summaries ps ON ps.page_id = p.id \
             WHERE p.deleted_at IS NULL",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut page_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    while let Some(row) = page_rows.next().await.map_err(|e| e.to_string())? {
        let id: String = row.get(0).map_err(|e| e.to_string())?;
        let title: String = row.get(1).unwrap_or_else(|_| "Untitled".to_string());
        page_ids.insert(id.clone());
        nodes.push(GraphNode {
            id,
            label: title,
            node_type: "page".to_string(),
            entity_type: None,
            status: None,
            mention_count: None,
            description: None,
        });
    }

    // --- Entity nodes (non-dismissed) ---
    let mut entity_rows = conn
        .query(
            "SELECT id, name, entity_type, status, mention_count, description \
             FROM entity_registry WHERE status != 'dismissed'",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut entity_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    while let Some(row) = entity_rows.next().await.map_err(|e| e.to_string())? {
        let id: String = row.get(0).map_err(|e| e.to_string())?;
        let name: String = row.get(1).map_err(|e| e.to_string())?;
        let entity_type: String = row.get(2).unwrap_or_else(|_| "concept".to_string());
        let status: String = row.get(3).unwrap_or_else(|_| "candidate".to_string());
        let mention_count: i64 = row.get(4).unwrap_or(0);
        let description: String = row.get(5).unwrap_or_default();
        entity_ids.insert(id.clone());
        nodes.push(GraphNode {
            id,
            label: name,
            node_type: "entity".to_string(),
            entity_type: Some(entity_type),
            status: Some(status),
            mention_count: Some(mention_count),
            description: if description.is_empty() { None } else { Some(description) },
        });
    }

    // --- Edges: entity mentions (entity → page) ---
    let mut mention_rows = conn
        .query("SELECT DISTINCT entity_id, page_id FROM entity_mentions", ())
        .await
        .map_err(|e| e.to_string())?;

    let mut edges: Vec<GraphEdge> = Vec::new();

    while let Some(row) = mention_rows.next().await.map_err(|e| e.to_string())? {
        let entity_id: String = row.get(0).map_err(|e| e.to_string())?;
        let page_id: String = row.get(1).map_err(|e| e.to_string())?;
        if entity_ids.contains(&entity_id) && page_ids.contains(&page_id) {
            edges.push(GraphEdge {
                source: entity_id,
                target: page_id,
                edge_type: "mention".to_string(),
                label: None,
            });
        }
    }

    // --- Edges: page links ---
    let mut link_rows = conn
        .query(
            "SELECT source_page_id, target_page_id, relationship, description FROM page_links",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

    while let Some(row) = link_rows.next().await.map_err(|e| e.to_string())? {
        let source_id: String = row.get(0).map_err(|e| e.to_string())?;
        let target_id: String = row.get(1).map_err(|e| e.to_string())?;
        let relationship: String = row.get(2).unwrap_or_default();
        let _description: String = row.get(3).unwrap_or_default();
        if page_ids.contains(&source_id) && page_ids.contains(&target_id) {
            edges.push(GraphEdge {
                source: source_id,
                target: target_id,
                edge_type: "link".to_string(),
                label: if relationship.is_empty() { None } else { Some(relationship) },
            });
        }
    }

    // --- Edges: entity-entity relations ---
    let mut rel_rows = conn
        .query(
            "SELECT from_entity_id, to_entity_id, relationship FROM entity_relations",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

    while let Some(row) = rel_rows.next().await.map_err(|e| e.to_string())? {
        let from_id: String = row.get(0).map_err(|e| e.to_string())?;
        let to_id: String = row.get(1).map_err(|e| e.to_string())?;
        let relationship: String = row.get(2).unwrap_or_default();
        if entity_ids.contains(&from_id) && entity_ids.contains(&to_id) {
            edges.push(GraphEdge {
                source: from_id,
                target: to_id,
                edge_type: "relation".to_string(),
                label: if relationship.is_empty() { None } else { Some(relationship) },
            });
        }
    }

    Ok(GraphData { nodes, edges })
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

// ── ask_wiki ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct WikiAnswer {
    pub answer: String,
    pub sources: Vec<String>,
    pub confidence: String,
}

#[tauri::command]
pub async fn ask_wiki(
    state: State<'_, AppState>,
    space_id: String,
    question: String,
) -> Result<WikiAnswer, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;
    let cfg = load_space_config(&conn).await?;

    // Pull all page summaries with titles
    let mut sum_rows = conn.query(
        "SELECT ps.summary, ps.key_points, ps.topics, p.title \
         FROM page_summaries ps JOIN pages p ON p.id = ps.page_id \
         WHERE p.deleted_at IS NULL ORDER BY ps.synthesized_at DESC LIMIT 30",
        (),
    ).await.map_err(|e| e.to_string())?;

    let mut context_parts: Vec<String> = Vec::new();
    let mut source_titles: Vec<String> = Vec::new();

    while let Some(row) = sum_rows.next().await.map_err(|e| e.to_string())? {
        let summary: String = row.get(0).unwrap_or_default();
        let kp_json: String = row.get(1).unwrap_or_else(|_| "[]".to_string());
        let title: String = row.get(3).unwrap_or_else(|_| "Untitled".to_string());
        let kps: Vec<String> = serde_json::from_str(&kp_json).unwrap_or_default();
        source_titles.push(title.clone());
        let kp_str = if kps.is_empty() { String::new() } else {
            format!("\nKey points:\n{}", kps.iter().map(|k| format!("- {}", k)).collect::<Vec<_>>().join("\n"))
        };
        context_parts.push(format!("### {}\n{}{}", title, summary, kp_str));
    }

    // Pull relevant entities
    let mut ent_rows = conn.query(
        "SELECT name, entity_type, description FROM entity_registry \
         WHERE status != 'dismissed' ORDER BY mention_count DESC LIMIT 50",
        (),
    ).await.map_err(|e| e.to_string())?;

    let mut entity_context = String::new();
    while let Some(row) = ent_rows.next().await.map_err(|e| e.to_string())? {
        let name: String = row.get(0).unwrap_or_default();
        let etype: String = row.get(1).unwrap_or_default();
        let desc: String = row.get(2).unwrap_or_default();
        entity_context.push_str(&format!("- {} ({}): {}\n", name, etype, desc));
    }

    let context = context_parts.join("\n\n---\n\n");

    let system = crate::commands::prompts::WIKI_QA;

    let user_msg = format!(
        "Question: {}\n\n## Compiled Knowledge\n\n{}\n\n## Known Entities\n\n{}",
        question, context, entity_context
    );

    let raw = call_openrouter(&cfg.api_key, &cfg.model, system, &user_msg, true).await?;
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("Parse error: {} — raw: {}", e, &raw[..raw.len().min(200)]))?;

    Ok(WikiAnswer {
        answer: parsed["answer"].as_str().unwrap_or("No answer generated.").to_string(),
        sources: source_titles,
        confidence: parsed["confidence"].as_str().unwrap_or("medium").to_string(),
    })
}

// ── demote_entity_page ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn demote_entity_page(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    // Reset entity registry entry
    conn.execute(
        "UPDATE entity_registry SET status = 'candidate', page_id = NULL WHERE page_id = ?1",
        libsql::params![page_id.clone()],
    ).await.map_err(|e| e.to_string())?;

    // Remove all page_links involving this wiki page
    conn.execute(
        "DELETE FROM page_links WHERE source_page_id = ?1 OR target_page_id = ?1",
        libsql::params![page_id.clone()],
    ).await.map_err(|e| e.to_string())?;

    // Delete page versions
    conn.execute(
        "DELETE FROM page_versions WHERE page_id = ?1",
        libsql::params![page_id.clone()],
    ).await.map_err(|e| e.to_string())?;

    // Hard delete the page
    conn.execute(
        "DELETE FROM pages WHERE id = ?1",
        libsql::params![page_id.clone()],
    ).await.map_err(|e| e.to_string())?;

    Ok(())
}

// ── lint_space ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct LintResult {
    pub orphan_wiki_pages: Vec<String>,
    pub stale_wiki_pages: Vec<String>,
    pub unresolved_contradictions: usize,
    pub high_mention_unlinked: Vec<String>,
    pub investigation_questions: Vec<String>,
    pub suggested_sources: Vec<String>,
}

#[tauri::command]
pub async fn lint_space(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<LintResult, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_synthesis_tables(&conn).await?;
    let cfg = load_space_config(&conn).await?;

    // 1. Orphan wiki pages (is_entity_page=1 but no entity_mentions pointing to them via entity_registry)
    let mut orphan_rows = conn.query(
        "SELECT p.title FROM pages p \
         LEFT JOIN entity_registry er ON er.page_id = p.id \
         LEFT JOIN entity_mentions em ON em.entity_id = er.id \
         WHERE p.is_entity_page = 1 AND p.deleted_at IS NULL AND em.id IS NULL",
        (),
    ).await.map_err(|e| e.to_string())?;
    let mut orphan_wiki_pages = Vec::new();
    while let Some(row) = orphan_rows.next().await.map_err(|e| e.to_string())? {
        orphan_wiki_pages.push(row.get::<String>(0).unwrap_or_default());
    }

    // 2. Stale wiki pages (source page updated_at > wiki page created_at)
    let mut stale_rows = conn.query(
        "SELECT wp.title FROM pages wp \
         JOIN entity_registry er ON er.page_id = wp.id \
         JOIN entity_mentions em ON em.entity_id = er.id \
         JOIN pages sp ON sp.id = em.page_id \
         WHERE wp.is_entity_page = 1 AND sp.updated_at > wp.created_at \
         GROUP BY wp.id",
        (),
    ).await.map_err(|e| e.to_string())?;
    let mut stale_wiki_pages = Vec::new();
    while let Some(row) = stale_rows.next().await.map_err(|e| e.to_string())? {
        stale_wiki_pages.push(row.get::<String>(0).unwrap_or_default());
    }

    // 3. High-mention entities without wiki pages
    let mut hmul_rows = conn.query(
        "SELECT name FROM entity_registry \
         WHERE mention_count >= 3 AND (page_id IS NULL OR page_id = '') AND status != 'dismissed' \
         ORDER BY mention_count DESC LIMIT 10",
        (),
    ).await.map_err(|e| e.to_string())?;
    let mut high_mention_unlinked = Vec::new();
    while let Some(row) = hmul_rows.next().await.map_err(|e| e.to_string())? {
        high_mention_unlinked.push(row.get::<String>(0).unwrap_or_default());
    }

    // 4. Pull summaries to generate investigation questions via LLM
    let mut sum_rows = conn.query(
        "SELECT ps.summary, p.title FROM page_summaries ps \
         JOIN pages p ON p.id = ps.page_id WHERE p.deleted_at IS NULL LIMIT 15",
        (),
    ).await.map_err(|e| e.to_string())?;
    let mut summaries_text = String::new();
    while let Some(row) = sum_rows.next().await.map_err(|e| e.to_string())? {
        let summary: String = row.get(0).unwrap_or_default();
        let title: String = row.get(1).unwrap_or_default();
        summaries_text.push_str(&format!("- \"{}\": {}\n", title, summary));
    }

    let (investigation_questions, suggested_sources) = if !summaries_text.is_empty() {
        let system = crate::commands::prompts::SPACE_LINTER;
        let orphan_info = if orphan_wiki_pages.is_empty() { String::new() }
            else { format!("\nOrphan wiki pages: {}", orphan_wiki_pages.join(", ")) };
        let user_msg = format!("Summaries:\n{}{}", summaries_text, orphan_info);
        match call_openrouter(&cfg.api_key, &cfg.model, system, &user_msg, true).await {
            Ok(raw) => {
                let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
                let qs = parsed["questions"].as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                let ss = parsed["sources"].as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                (qs, ss)
            }
            Err(_) => (Vec::new(), Vec::new()),
        }
    } else {
        (Vec::new(), Vec::new())
    };

    Ok(LintResult {
        orphan_wiki_pages,
        stale_wiki_pages,
        unresolved_contradictions: 0,
        high_mention_unlinked,
        investigation_questions,
        suggested_sources,
    })
}

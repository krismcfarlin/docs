use crate::state::{AppState, DEMO_USER_ID};
use crate::types::{Page, PageVersion, RecentPage, Space};
use libsql::Builder;
use nanoid::nanoid;
use std::sync::Arc;
use tauri::State;

// ── Migration SQL constants ───────────────────────────────────────────────────

const REGISTRY_MIGRATIONS: &str = "
CREATE TABLE IF NOT EXISTS spaces (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    parent_space_id TEXT,
    sort_order INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now')),
    source TEXT NOT NULL DEFAULT 'local',
    server_url TEXT,
    namespace TEXT,
    token TEXT,
    permission_level TEXT NOT NULL DEFAULT 'owner',
    admin_token TEXT
);

CREATE TABLE IF NOT EXISTS recent_pages (
    page_id TEXT NOT NULL PRIMARY KEY,
    space_id TEXT NOT NULL,
    title TEXT NOT NULL,
    space_name TEXT NOT NULL,
    last_accessed_at TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'local',
    permission_level TEXT NOT NULL DEFAULT 'owner'
);
";

const SPACE_MIGRATIONS: &str = "
CREATE TABLE IF NOT EXISTS pages (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL DEFAULT 'Untitled',
    space_id TEXT NOT NULL,
    creator_id TEXT NOT NULL,
    parent_page_id TEXT,
    sort_order INTEGER DEFAULT 0,
    deleted_at TEXT,
    last_accessed_at TEXT,
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now')),
    source TEXT NOT NULL DEFAULT 'local',
    remote_id TEXT,
    permission_level TEXT NOT NULL DEFAULT 'owner',
    last_synced_at TEXT
);

CREATE TABLE IF NOT EXISTS page_versions (
    id TEXT PRIMARY KEY,
    page_id TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    based_on_version_id TEXT,
    title TEXT,
    content TEXT,
    text_content TEXT,
    is_published INTEGER DEFAULT 0,
    is_frozen INTEGER DEFAULT 0,
    version_num INTEGER DEFAULT 1,
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS page_embeddings (
    version_id TEXT PRIMARY KEY,
    page_id    TEXT NOT NULL,
    space_id   TEXT NOT NULL,
    embedding  TEXT NOT NULL
);
";

// ── Path helpers — pure functions (testable without env mutation) ─────────────

/// Pure, testable core of the registry path logic.
/// Rules:
///   1. If `bamako_data` is Some and non-blank after trimming,
///      the registry DB lives at `{bamako_data_trimmed}/registry.db`.
///   2. Otherwise fall back to `{home}/.bamako/registry.db`.
///   3. If `home` is also absent, use `/tmp`.
///
/// Trailing slashes on `bamako_data` are stripped so that
/// `/tmp/bam-alice/` and `/tmp/bam-alice` produce identical paths.
/// A bare `/` trims to empty and therefore falls back to HOME.
pub(crate) fn registry_path_from_env(bamako_data: Option<String>, home: Option<String>) -> String {
    if let Some(data) = bamako_data {
        let trimmed = data.trim().trim_end_matches('/');
        if !trimmed.is_empty() {
            return format!("{}/registry.db", trimmed);
        }
    }
    let h = home.unwrap_or_else(|| "/tmp".to_string());
    format!("{}/.bamako/registry.db", h)
}

/// Pure, testable core of the per-space DB path logic.
pub(crate) fn space_db_path_from_env(
    space_id: &str,
    bamako_data: Option<String>,
    home: Option<String>,
) -> String {
    let base = if let Some(data) = bamako_data {
        let trimmed = data.trim().trim_end_matches('/');
        if !trimmed.is_empty() {
            trimmed.to_string()
        } else {
            format!("{}/.bamako", home.unwrap_or_else(|| "/tmp".to_string()))
        }
    } else {
        format!("{}/.bamako", home.unwrap_or_else(|| "/tmp".to_string()))
    };
    format!("{}/spaces/{}.db", base, space_id)
}

/// Pure, testable — kept for legacy migration check.
pub(crate) fn db_path_from_env(bamako_data: Option<String>, home: Option<String>) -> String {
    if let Some(data) = bamako_data {
        let trimmed = data.trim().trim_end_matches('/');
        if !trimmed.is_empty() {
            return format!("{}/local.db", trimmed);
        }
    }
    let h = home.unwrap_or_else(|| "/tmp".to_string());
    format!("{}/.bamako/local.db", h)
}

fn registry_path() -> String {
    registry_path_from_env(
        std::env::var("BAMAKO_DATA").ok(),
        std::env::var("HOME").ok(),
    )
}

fn space_db_path(space_id: &str) -> String {
    space_db_path_from_env(
        space_id,
        std::env::var("BAMAKO_DATA").ok(),
        std::env::var("HOME").ok(),
    )
}

fn db_path() -> String {
    db_path_from_env(
        std::env::var("BAMAKO_DATA").ok(),
        std::env::var("HOME").ok(),
    )
}

// ── Registry migrations ───────────────────────────────────────────────────────

async fn run_registry_migrations(db: &libsql::Database) -> Result<(), String> {
    let conn = db.connect().map_err(|e| e.to_string())?;
    for stmt in REGISTRY_MIGRATIONS.split(';') {
        let s = stmt.trim();
        if s.is_empty() {
            continue;
        }
        if let Err(e) = conn.execute(s, ()).await {
            let msg = e.to_string();
            if !msg.contains("already exists") {
                return Err(msg);
            }
        }
    }
    // ALTER TABLE for existing registry DBs (idempotent)
    for alter in [
        "ALTER TABLE spaces ADD COLUMN parent_space_id TEXT",
        "ALTER TABLE spaces ADD COLUMN sort_order INTEGER DEFAULT 0",
        "ALTER TABLE spaces ADD COLUMN source TEXT NOT NULL DEFAULT 'local'",
        "ALTER TABLE spaces ADD COLUMN server_url TEXT",
        "ALTER TABLE spaces ADD COLUMN namespace TEXT",
        "ALTER TABLE spaces ADD COLUMN token TEXT",
        "ALTER TABLE spaces ADD COLUMN permission_level TEXT NOT NULL DEFAULT 'owner'",
        "ALTER TABLE spaces ADD COLUMN admin_token TEXT",
    ] {
        conn.execute(alter, ()).await.ok();
    }
    Ok(())
}

// ── Space DB migrations ───────────────────────────────────────────────────────

async fn run_space_migrations(db: &libsql::Database) -> Result<(), String> {
    let conn = db.connect().map_err(|e| e.to_string())?;
    for stmt in SPACE_MIGRATIONS.split(';') {
        let s = stmt.trim();
        if s.is_empty() {
            continue;
        }
        if let Err(e) = conn.execute(s, ()).await {
            let msg = e.to_string();
            if !msg.contains("already exists") {
                return Err(msg);
            }
        }
    }
    // ALTER TABLE for existing space DBs (idempotent)
    for alter in [
        "ALTER TABLE pages ADD COLUMN sort_order INTEGER DEFAULT 0",
        "ALTER TABLE pages ADD COLUMN deleted_at TEXT",
        "ALTER TABLE pages ADD COLUMN last_accessed_at TEXT",
        "ALTER TABLE pages ADD COLUMN source TEXT NOT NULL DEFAULT 'local'",
        "ALTER TABLE pages ADD COLUMN remote_id TEXT",
        "ALTER TABLE pages ADD COLUMN permission_level TEXT NOT NULL DEFAULT 'owner'",
        "ALTER TABLE pages ADD COLUMN last_synced_at TEXT",
        "ALTER TABLE pages ADD COLUMN is_entity_page INTEGER NOT NULL DEFAULT 0",
    ] {
        conn.execute(alter, ()).await.ok();
    }
    Ok(())
}

// ── Space source helper ───────────────────────────────────────────────────────

/// Returns the source ("local" or "remote") for a given space_id from the registry.
async fn get_space_source(state: &AppState, space_id: &str) -> Result<String, String> {
    let guard = state.registry.lock().await;
    let registry = guard.as_ref().ok_or("Registry not initialised")?;
    let conn = registry.connect().map_err(|e| e.to_string())?;
    let mut rows = conn
        .query(
            "SELECT source FROM spaces WHERE id = ?1",
            libsql::params![space_id.to_string()],
        )
        .await
        .map_err(|e| e.to_string())?;
    let row = rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Space '{}' not found in registry", space_id))?;
    Ok(row.get::<String>(0).unwrap_or_else(|_| "local".to_string()))
}

// ── get_or_open_space_db helper ───────────────────────────────────────────────

pub(crate) async fn get_or_open_space_db(
    state: &AppState,
    space_id: &str,
) -> Result<Arc<libsql::Database>, String> {
    // Check cache first
    {
        let guard = state.space_dbs.lock().await;
        if let Some(db) = guard.get(space_id) {
            return Ok(db.clone());
        }
    }

    // Look up registry for connection info
    let (source, server_url, _namespace, token) = {
        let guard = state.registry.lock().await;
        let registry = guard.as_ref().ok_or("Registry not initialised")?;
        let conn = registry.connect().map_err(|e| e.to_string())?;
        let mut rows = conn
            .query(
                "SELECT source, server_url, namespace, token FROM spaces WHERE id = ?1",
                libsql::params![space_id.to_string()],
            )
            .await
            .map_err(|e| e.to_string())?;
        let row = rows
            .next()
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Space '{}' not found in registry", space_id))?;
        (
            row.get::<String>(0).unwrap_or_else(|_| "local".to_string()),
            row.get::<Option<String>>(1).map_err(|e| e.to_string())?,
            row.get::<Option<String>>(2).map_err(|e| e.to_string())?,
            row.get::<Option<String>>(3).map_err(|e| e.to_string())?,
        )
    };

    let db = if source == "remote" {
        let url = server_url
            .filter(|s| !s.is_empty())
            .ok_or("Remote space missing server_url")?;
        let tok = token.unwrap_or_default();
        eprintln!("[space_db] opening remote HTTP space_id={} url={}", space_id, url);
        let db = libsql::Builder::new_remote(url.clone(), tok)
            .build()
            .await
            .map_err(|e| {
                eprintln!("[space_db] ERROR opening remote {}: {}", url, e);
                e.to_string()
            })?;
        eprintln!("[space_db] remote DB opened OK for space_id={}", space_id);
        db
    } else {
        let path = space_db_path(space_id);
        std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap())
            .map_err(|e| e.to_string())?;
        libsql::Builder::new_local(&path)
            .build()
            .await
            .map_err(|e| e.to_string())?
    };

    run_space_migrations(&db).await?;

    let db = Arc::new(db);
    state
        .space_dbs
        .lock()
        .await
        .insert(space_id.to_string(), db.clone());
    Ok(db)
}

// ── Legacy migration ──────────────────────────────────────────────────────────

async fn migrate_from_legacy(state: &AppState, legacy_path: &str) {
    eprintln!("[migrate] migrating legacy local.db at {}", legacy_path);

    let legacy_db = match Builder::new_local(legacy_path).build().await {
        Ok(db) => db,
        Err(e) => {
            eprintln!("[migrate] failed to open legacy DB: {e}");
            return;
        }
    };
    let legacy_conn = match legacy_db.connect() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[migrate] failed to connect legacy DB: {e}");
            return;
        }
    };

    // Read all spaces from legacy
    let mut space_rows = match legacy_conn
        .query(
            "SELECT id, name, description, parent_space_id, sort_order, created_at, source \
             FROM spaces ORDER BY sort_order ASC, created_at ASC",
            (),
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[migrate] failed to read legacy spaces: {e}");
            return;
        }
    };

    let mut space_ids: Vec<String> = vec![];

    while let Ok(Some(row)) = space_rows.next().await {
        let id: String = row.get(0).unwrap_or_default();
        let name: String = row.get(1).unwrap_or_default();
        let description: Option<String> = row.get(2).ok().flatten();
        let parent_space_id: Option<String> = row.get(3).ok().flatten();
        let sort_order: i64 = row.get(4).unwrap_or(0);
        let created_at: String = row.get(5).unwrap_or_default();
        let source: String = row.get::<String>(6).unwrap_or_else(|_| "local".to_string());

        // Insert into registry
        {
            let guard = state.registry.lock().await;
            if let Some(reg) = guard.as_ref() {
                if let Ok(conn) = reg.connect() {
                    conn.execute(
                        "INSERT OR IGNORE INTO spaces \
                         (id, name, description, parent_space_id, sort_order, created_at, source, permission_level) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'owner')",
                        libsql::params![
                            id.clone(),
                            name,
                            description,
                            parent_space_id,
                            sort_order,
                            created_at,
                            source
                        ],
                    )
                    .await
                    .ok();
                }
            }
        }
        space_ids.push(id);
    }

    // For each space, copy pages + page_versions + page_embeddings
    for space_id in &space_ids {
        let space_db = match get_or_open_space_db(state, space_id).await {
            Ok(db) => db,
            Err(e) => {
                eprintln!("[migrate] failed to open space DB for {}: {e}", space_id);
                continue;
            }
        };
        let space_conn = match space_db.connect() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[migrate] failed to connect space DB for {}: {e}", space_id);
                continue;
            }
        };

        // Copy pages
        let mut page_rows = match legacy_conn
            .query(
                "SELECT id, title, space_id, creator_id, parent_page_id, sort_order, \
                        deleted_at, last_accessed_at, created_at, updated_at, source, remote_id, \
                        permission_level, last_synced_at \
                 FROM pages WHERE space_id = ?1",
                libsql::params![space_id.clone()],
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[migrate] failed to read legacy pages for {}: {e}", space_id);
                continue;
            }
        };

        while let Ok(Some(row)) = page_rows.next().await {
            let id: String = row.get(0).unwrap_or_default();
            let title: String = row.get(1).unwrap_or_default();
            let sid: String = row.get(2).unwrap_or_default();
            let creator_id: String = row.get(3).unwrap_or_default();
            let parent_page_id: Option<String> = row.get(4).ok().flatten();
            let sort_order: i64 = row.get(5).unwrap_or(0);
            let deleted_at: Option<String> = row.get(6).ok().flatten();
            let last_accessed_at: Option<String> = row.get(7).ok().flatten();
            let created_at: String = row.get(8).unwrap_or_default();
            let updated_at: String = row.get(9).unwrap_or_default();
            let source: String = row.get::<String>(10).unwrap_or_else(|_| "local".to_string());
            let remote_id: Option<String> = row.get(11).ok().flatten();
            let permission_level: String =
                row.get::<String>(12).unwrap_or_else(|_| "owner".to_string());
            let last_synced_at: Option<String> = row.get(13).ok().flatten();

            space_conn
                .execute(
                    "INSERT OR IGNORE INTO pages \
                     (id, title, space_id, creator_id, parent_page_id, sort_order, \
                      deleted_at, last_accessed_at, created_at, updated_at, source, remote_id, \
                      permission_level, last_synced_at) \
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                    libsql::params![
                        id,
                        title,
                        sid,
                        creator_id,
                        parent_page_id,
                        sort_order,
                        deleted_at,
                        last_accessed_at,
                        created_at,
                        updated_at,
                        source,
                        remote_id,
                        permission_level,
                        last_synced_at
                    ],
                )
                .await
                .ok();
        }

        // Copy page_versions
        let mut ver_rows = match legacy_conn
            .query(
                "SELECT pv.id, pv.page_id, pv.owner_id, pv.based_on_version_id, \
                        pv.title, pv.content, pv.text_content, pv.is_published, pv.is_frozen, \
                        pv.version_num, pv.created_at, pv.updated_at \
                 FROM page_versions pv \
                 JOIN pages p ON p.id = pv.page_id \
                 WHERE p.space_id = ?1",
                libsql::params![space_id.clone()],
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "[migrate] failed to read legacy page_versions for {}: {e}",
                    space_id
                );
                continue;
            }
        };

        while let Ok(Some(row)) = ver_rows.next().await {
            let id: String = row.get(0).unwrap_or_default();
            let page_id: String = row.get(1).unwrap_or_default();
            let owner_id: String = row.get(2).unwrap_or_default();
            let based_on: Option<String> = row.get(3).ok().flatten();
            let title: Option<String> = row.get(4).ok().flatten();
            let content: Option<String> = row.get(5).ok().flatten();
            let text_content: Option<String> = row.get(6).ok().flatten();
            let is_published: i64 = row.get(7).unwrap_or(0);
            let is_frozen: i64 = row.get(8).unwrap_or(0);
            let version_num: i64 = row.get(9).unwrap_or(1);
            let created_at: String = row.get(10).unwrap_or_default();
            let updated_at: String = row.get(11).unwrap_or_default();

            space_conn
                .execute(
                    "INSERT OR IGNORE INTO page_versions \
                     (id, page_id, owner_id, based_on_version_id, title, content, text_content, \
                      is_published, is_frozen, version_num, created_at, updated_at) \
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                    libsql::params![
                        id,
                        page_id,
                        owner_id,
                        based_on,
                        title,
                        content,
                        text_content,
                        is_published,
                        is_frozen,
                        version_num,
                        created_at,
                        updated_at
                    ],
                )
                .await
                .ok();
        }

        // Copy page_embeddings
        let mut emb_rows = match legacy_conn
            .query(
                "SELECT pe.version_id, pe.page_id, pe.space_id, pe.embedding \
                 FROM page_embeddings pe \
                 WHERE pe.space_id = ?1",
                libsql::params![space_id.clone()],
            )
            .await
        {
            Ok(r) => r,
            Err(_) => continue,
        };

        while let Ok(Some(row)) = emb_rows.next().await {
            let version_id: String = row.get(0).unwrap_or_default();
            let page_id: String = row.get(1).unwrap_or_default();
            let sid: String = row.get(2).unwrap_or_default();
            let embedding: String = row.get(3).unwrap_or_default();

            space_conn
                .execute(
                    "INSERT OR IGNORE INTO page_embeddings (version_id, page_id, space_id, embedding) \
                     VALUES (?1, ?2, ?3, ?4)",
                    libsql::params![version_id, page_id, sid, embedding],
                )
                .await
                .ok();
        }

        eprintln!("[migrate] completed space {}", space_id);
    }

    eprintln!("[migrate] legacy migration complete");
}

// ── Tauri commands ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn init_db(state: State<'_, AppState>) -> Result<(), String> {
    let path = registry_path();
    let is_fresh = !std::path::Path::new(&path).exists();

    std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap())
        .map_err(|e| e.to_string())?;

    eprintln!("[init_db] opening registry at {}", path);
    let registry = Builder::new_local(&path)
        .build()
        .await
        .map_err(|e| e.to_string())?;

    run_registry_migrations(&registry).await?;

    // Seed demo user marker in registry (spaces table holds no users, but we keep the user concept
    // alive via a well-known constant — nothing to insert to registry for users)
    eprintln!("[init_db] registry migrations done");

    {
        let mut guard = state.registry.lock().await;
        *guard = Some(registry);
    }

    // If registry was freshly created AND legacy local.db exists, migrate
    if is_fresh {
        let legacy = db_path();
        if std::path::Path::new(&legacy).exists() {
            eprintln!("[init_db] detected legacy DB at {}, migrating…", legacy);
            migrate_from_legacy(&state, &legacy).await;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn sync_db(state: State<'_, AppState>) -> Result<(), String> {
    let guard = state.space_dbs.lock().await;
    for (space_id, db) in guard.iter() {
        eprintln!("[sync_db] syncing space {}", space_id);
        db.sync().await.ok();
    }
    Ok(())
}

// ── Space commands — operate on registry ─────────────────────────────────────

#[tauri::command]
pub async fn get_spaces(state: State<'_, AppState>) -> Result<Vec<Space>, String> {
    let guard = state.registry.lock().await;
    let db = guard.as_ref().ok_or("Registry not initialised")?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    let mut rows = conn
        .query(
            "SELECT id, name, description, parent_space_id, sort_order, created_at, \
                    source, namespace, server_url, permission_level \
             FROM spaces ORDER BY sort_order ASC, created_at ASC",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut spaces = vec![];
    while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        spaces.push(Space {
            id: row.get(0).map_err(|e| e.to_string())?,
            name: row.get(1).map_err(|e| e.to_string())?,
            description: row.get(2).map_err(|e| e.to_string())?,
            parent_space_id: row.get(3).map_err(|e| e.to_string())?,
            sort_order: row.get(4).map_err(|e| e.to_string())?,
            created_at: row.get(5).map_err(|e| e.to_string())?,
            source: row.get::<String>(6).unwrap_or_else(|_| "local".to_string()),
            namespace: row.get(7).map_err(|e| e.to_string())?,
            server_url: row.get(8).map_err(|e| e.to_string())?,
            permission_level: row
                .get::<String>(9)
                .unwrap_or_else(|_| "owner".to_string()),
        });
    }
    Ok(spaces)
}

#[tauri::command]
pub async fn create_space(
    state: State<'_, AppState>,
    name: String,
    description: Option<String>,
    parent_space_id: Option<String>,
) -> Result<Space, String> {
    let id = nanoid!();

    // Compute sort_order from registry
    let sort_order: i64 = {
        let guard = state.registry.lock().await;
        let db = guard.as_ref().ok_or("Registry not initialised")?;
        let conn = db.connect().map_err(|e| e.to_string())?;
        let mut r = conn
            .query(
                "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM spaces WHERE parent_space_id IS ?1",
                libsql::params![parent_space_id.clone()],
            )
            .await
            .map_err(|e| e.to_string())?;
        r.next()
            .await
            .ok()
            .flatten()
            .and_then(|row| row.get(0).ok())
            .unwrap_or(0)
    };

    // Insert into registry
    let created_at: String = {
        let guard = state.registry.lock().await;
        let db = guard.as_ref().ok_or("Registry not initialised")?;
        let conn = db.connect().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO spaces (id, name, description, parent_space_id, sort_order, source, permission_level) \
             VALUES (?1, ?2, ?3, ?4, ?5, 'local', 'owner')",
            libsql::params![
                id.clone(),
                name.clone(),
                description.clone(),
                parent_space_id.clone(),
                sort_order
            ],
        )
        .await
        .map_err(|e| e.to_string())?;

        let mut rows = conn
            .query(
                "SELECT created_at FROM spaces WHERE id = ?1",
                libsql::params![id.clone()],
            )
            .await
            .map_err(|e| e.to_string())?;
        let row = rows
            .next()
            .await
            .map_err(|e| e.to_string())?
            .ok_or("insert failed")?;
        row.get(0).map_err(|e| e.to_string())?
    };

    // Create the per-space DB file
    get_or_open_space_db(&state, &id).await?;

    Ok(Space {
        id,
        name,
        description,
        parent_space_id,
        sort_order,
        created_at,
        source: "local".to_string(),
        namespace: None,
        server_url: None,
        permission_level: "owner".to_string(),
    })
}

#[tauri::command]
pub async fn delete_space(state: State<'_, AppState>, space_id: String) -> Result<(), String> {
    // Remove from registry
    {
        let guard = state.registry.lock().await;
        let db = guard.as_ref().ok_or("Registry not initialised")?;
        let conn = db.connect().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM recent_pages WHERE space_id = ?1",
            libsql::params![space_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM spaces WHERE id = ?1",
            libsql::params![space_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;
    }

    // Remove from in-memory cache
    state.space_dbs.lock().await.remove(&space_id);

    // Delete the DB file
    let path = space_db_path(&space_id);
    std::fs::remove_file(&path).ok();

    Ok(())
}

#[tauri::command]
pub async fn rename_space(
    state: State<'_, AppState>,
    space_id: String,
    name: String,
) -> Result<(), String> {
    let guard = state.registry.lock().await;
    let db = guard.as_ref().ok_or("Registry not initialised")?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE spaces SET name = ?1 WHERE id = ?2",
        libsql::params![name, space_id],
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn move_space(
    state: State<'_, AppState>,
    space_id: String,
    parent_space_id: Option<String>,
) -> Result<(), String> {
    let guard = state.registry.lock().await;
    let db = guard.as_ref().ok_or("Registry not initialised")?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE spaces SET parent_space_id = ?1 WHERE id = ?2",
        libsql::params![parent_space_id, space_id],
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn reorder_spaces(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    let guard = state.registry.lock().await;
    let db = guard.as_ref().ok_or("Registry not initialised")?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    for (i, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE spaces SET sort_order = ?1 WHERE id = ?2",
            libsql::params![i as i64, id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ── Page commands — operate on per-space DBs ─────────────────────────────────

#[tauri::command]
pub async fn get_pages(state: State<'_, AppState>, space_id: String) -> Result<Vec<Page>, String> {
    let source = get_space_source(&state, &space_id).await?;
    eprintln!("[get_pages] space_id={} source={}", space_id, source);

    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    // For remote spaces the sqld instance IS the namespace boundary — every page in it
    // belongs to this space. The seed's space_id is a random nanoid that will never match
    // our local registry id, so we must NOT filter by space_id for remote spaces.
    let mut rows = if source == "remote" {
        eprintln!("[get_pages] remote space — querying ALL non-deleted pages (no space_id filter)");
        conn.query(
            "SELECT id, title, space_id, creator_id, parent_page_id, sort_order, created_at, updated_at, \
                    source, remote_id, permission_level, last_synced_at \
             FROM pages WHERE deleted_at IS NULL ORDER BY sort_order ASC, created_at ASC",
            (),
        )
        .await
        .map_err(|e| e.to_string())?
    } else {
        eprintln!("[get_pages] local space — querying pages WHERE space_id={}", space_id);
        conn.query(
            "SELECT id, title, space_id, creator_id, parent_page_id, sort_order, created_at, updated_at, \
                    source, remote_id, permission_level, last_synced_at \
             FROM pages WHERE space_id = ?1 AND deleted_at IS NULL ORDER BY sort_order ASC, created_at ASC",
            libsql::params![space_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?
    };

    let mut pages = vec![];
    while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        pages.push(build_page(&row)?);
    }
    eprintln!("[get_pages] returning {} pages for space_id={}", pages.len(), space_id);
    Ok(pages)
}

#[tauri::command]
pub async fn create_page(
    state: State<'_, AppState>,
    title: String,
    space_id: String,
    parent_page_id: Option<String>,
) -> Result<Page, String> {
    eprintln!("[create_page] title={:?} space_id={} parent={:?}", title, space_id, parent_page_id);
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    let page_id = nanoid!();
    let version_id = nanoid!();

    conn.execute(
        "INSERT INTO pages (id, title, space_id, creator_id, parent_page_id) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        libsql::params![
            page_id.clone(),
            title.clone(),
            space_id.clone(),
            DEMO_USER_ID,
            parent_page_id.clone()
        ],
    )
    .await
    .map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO page_versions (id, page_id, owner_id, title, is_published, version_num) \
         VALUES (?1, ?2, ?3, ?4, 1, 1)",
        libsql::params![version_id, page_id.clone(), DEMO_USER_ID, title.clone()],
    )
    .await
    .map_err(|e| e.to_string())?;

    let mut rows = conn
        .query(
            "SELECT created_at, updated_at FROM pages WHERE id = ?1",
            libsql::params![page_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;
    let row = rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("insert failed")?;

    Ok(Page {
        id: page_id,
        title,
        space_id,
        creator_id: DEMO_USER_ID.to_string(),
        parent_page_id,
        sort_order: 0,
        created_at: row.get(0).map_err(|e| e.to_string())?,
        updated_at: row.get(1).map_err(|e| e.to_string())?,
        source: "local".to_string(),
        remote_id: None,
        permission_level: "owner".to_string(),
        last_synced_at: None,
    })
}

#[tauri::command]
pub async fn get_page_version(
    state: State<'_, AppState>,
    page_id: String,
    version_id: Option<String>,
    space_id: String,
) -> Result<Option<PageVersion>, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    let sql = if version_id.is_some() {
        "SELECT id, page_id, owner_id, based_on_version_id, title, content, text_content, \
         is_published, is_frozen, version_num, created_at, updated_at \
         FROM page_versions WHERE id = ?1"
    } else {
        "SELECT id, page_id, owner_id, based_on_version_id, title, content, text_content, \
         is_published, is_frozen, version_num, created_at, updated_at \
         FROM page_versions WHERE page_id = ?1 AND is_published = 1 \
         ORDER BY updated_at DESC LIMIT 1"
    };

    let param = version_id.unwrap_or(page_id);
    let mut rows = conn
        .query(sql, libsql::params![param])
        .await
        .map_err(|e| e.to_string())?;

    if let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        Ok(Some(build_page_version(&row)?))
    } else {
        Ok(None)
    }
}

#[derive(serde::Serialize)]
#[serde(tag = "type")]
pub enum SaveResult {
    #[serde(rename = "ok")]
    Ok { new_updated_at: String },
    #[serde(rename = "conflict")]
    Conflict {
        current_content: String,
        current_updated_at: String,
    },
}

#[tauri::command]
pub async fn save_page_version(
    state: State<'_, AppState>,
    version_id: String,
    title: String,
    content: String,
    text_content: String,
    space_id: String,
    base_updated_at: String,
) -> Result<SaveResult, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    // Check for concurrent modification
    let mut rows = conn
        .query(
            "SELECT content, updated_at FROM page_versions WHERE id = ?1",
            libsql::params![version_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;

    let row = rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("version not found")?;

    let current_content: String = row.get(0).map_err(|e| e.to_string())?;
    let current_updated_at: String = row.get(1).map_err(|e| e.to_string())?;

    if current_updated_at != base_updated_at {
        return Ok(SaveResult::Conflict {
            current_content,
            current_updated_at,
        });
    }

    conn.execute(
        "UPDATE page_versions \
         SET title = ?1, content = ?2, text_content = ?3, updated_at = datetime('now') \
         WHERE id = ?4",
        libsql::params![title, content, text_content, version_id.clone()],
    )
    .await
    .map_err(|e| e.to_string())?;

    // Re-fetch the updated_at that the database just wrote
    let mut rows2 = conn
        .query(
            "SELECT updated_at FROM page_versions WHERE id = ?1",
            libsql::params![version_id],
        )
        .await
        .map_err(|e| e.to_string())?;

    let row2 = rows2
        .next()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("version not found after update")?;

    let new_updated_at: String = row2.get(0).map_err(|e| e.to_string())?;

    Ok(SaveResult::Ok { new_updated_at })
}

#[tauri::command]
pub async fn publish_version(
    state: State<'_, AppState>,
    version_id: String,
    space_id: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    let mut rows = conn
        .query(
            "SELECT page_id FROM page_versions WHERE id = ?1",
            libsql::params![version_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;
    let row = rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("version not found")?;
    let page_id: String = row.get(0).map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE page_versions SET is_published = 0 WHERE page_id = ?1",
        libsql::params![page_id],
    )
    .await
    .map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE page_versions SET is_published = 1 WHERE id = ?1",
        libsql::params![version_id],
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn freeze_version(
    state: State<'_, AppState>,
    version_id: String,
    space_id: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE page_versions SET is_frozen = 1 WHERE id = ?1",
        libsql::params![version_id],
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn fork_version(
    state: State<'_, AppState>,
    version_id: String,
    space_id: String,
) -> Result<PageVersion, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    let mut rows = conn
        .query(
            "SELECT page_id, title, content, version_num FROM page_versions WHERE id = ?1",
            libsql::params![version_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;
    let row = rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("version not found")?;
    let page_id: String = row.get(0).map_err(|e| e.to_string())?;
    let title: Option<String> = row.get(1).map_err(|e| e.to_string())?;
    let content: Option<String> = row.get(2).map_err(|e| e.to_string())?;
    let base_num: i64 = row.get(3).map_err(|e| e.to_string())?;

    let new_id = nanoid!();
    let new_num = base_num + 1;

    conn.execute(
        "INSERT INTO page_versions \
         (id, page_id, owner_id, based_on_version_id, title, content, is_published, is_frozen, version_num) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, ?7)",
        libsql::params![
            new_id.clone(),
            page_id.clone(),
            DEMO_USER_ID,
            version_id,
            title.clone(),
            content.clone(),
            new_num
        ],
    )
    .await
    .map_err(|e| e.to_string())?;

    let mut rows = conn
        .query(
            "SELECT created_at, updated_at FROM page_versions WHERE id = ?1",
            libsql::params![new_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;
    let row = rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("insert failed")?;

    Ok(PageVersion {
        id: new_id,
        page_id,
        owner_id: DEMO_USER_ID.to_string(),
        based_on_version_id: None,
        title,
        content,
        text_content: None,
        is_published: false,
        is_frozen: false,
        version_num: new_num,
        created_at: row.get(0).map_err(|e| e.to_string())?,
        updated_at: row.get(1).map_err(|e| e.to_string())?,
    })
}

#[tauri::command]
pub async fn list_page_versions(
    state: State<'_, AppState>,
    page_id: String,
    space_id: String,
) -> Result<Vec<PageVersion>, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    let mut rows = conn
        .query(
            "SELECT id, page_id, owner_id, based_on_version_id, title, content, text_content, \
             is_published, is_frozen, version_num, created_at, updated_at \
             FROM page_versions WHERE page_id = ?1 ORDER BY version_num DESC",
            libsql::params![page_id],
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut versions = vec![];
    while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        versions.push(build_page_version(&row)?);
    }
    Ok(versions)
}

#[tauri::command]
pub async fn delete_page(
    state: State<'_, AppState>,
    page_id: String,
    space_id: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE pages SET deleted_at = datetime('now') WHERE id = ?1",
        libsql::params![page_id],
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_trash_pages(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<Vec<Page>, String> {
    let source = get_space_source(&state, &space_id).await?;
    eprintln!("[get_trash_pages] space_id={} source={}", space_id, source);

    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    let mut rows = if source == "remote" {
        conn.query(
            "SELECT id, title, space_id, creator_id, parent_page_id, sort_order, created_at, updated_at, \
                    source, remote_id, permission_level, last_synced_at \
             FROM pages WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC",
            (),
        )
        .await
        .map_err(|e| e.to_string())?
    } else {
        conn.query(
            "SELECT id, title, space_id, creator_id, parent_page_id, sort_order, created_at, updated_at, \
                    source, remote_id, permission_level, last_synced_at \
             FROM pages WHERE space_id = ?1 AND deleted_at IS NOT NULL ORDER BY deleted_at DESC",
            libsql::params![space_id],
        )
        .await
        .map_err(|e| e.to_string())?
    };

    let mut pages = vec![];
    while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        pages.push(build_page(&row)?);
    }
    Ok(pages)
}

#[tauri::command]
pub async fn restore_page(
    state: State<'_, AppState>,
    page_id: String,
    space_id: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE pages SET deleted_at = NULL, parent_page_id = NULL WHERE id = ?1",
        libsql::params![page_id],
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn permanent_delete_page(
    state: State<'_, AppState>,
    page_id: String,
    space_id: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM page_embeddings WHERE page_id = ?1",
        libsql::params![page_id.clone()],
    )
    .await
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM page_versions WHERE page_id = ?1",
        libsql::params![page_id.clone()],
    )
    .await
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM pages WHERE id = ?1",
        libsql::params![page_id],
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn rename_page(
    state: State<'_, AppState>,
    page_id: String,
    title: String,
    space_id: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE pages SET title = ?1, updated_at = datetime('now') WHERE id = ?2",
        libsql::params![title, page_id],
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn reorder_pages(
    state: State<'_, AppState>,
    ids: Vec<String>,
    space_id: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    for (i, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE pages SET sort_order = ?1, updated_at = datetime('now') WHERE id = ?2",
            libsql::params![i as i64, id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn record_page_access(
    state: State<'_, AppState>,
    page_id: String,
    space_id: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE pages SET last_accessed_at = datetime('now') WHERE id = ?1",
        libsql::params![page_id.clone()],
    )
    .await
    .map_err(|e| e.to_string())?;

    // Upsert into registry recent_pages — fetch page title + space name first
    let mut rows = conn
        .query(
            "SELECT title FROM pages WHERE id = ?1",
            libsql::params![page_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;
    let page_title: String = rows
        .next()
        .await
        .map_err(|e| e.to_string())?
        .map(|r| r.get::<String>(0).unwrap_or_else(|_| "Untitled".to_string()))
        .unwrap_or_else(|| "Untitled".to_string());

    // Fetch space name from registry
    let space_name: String = {
        let reg_guard = state.registry.lock().await;
        let mut resolved = space_id.clone();
        if let Some(reg) = reg_guard.as_ref() {
            if let Ok(reg_conn) = reg.connect() {
                if let Ok(mut r) = reg_conn
                    .query(
                        "SELECT name FROM spaces WHERE id = ?1",
                        libsql::params![space_id.clone()],
                    )
                    .await
                {
                    if let Ok(Some(row)) = r.next().await {
                        resolved = row.get::<String>(0).unwrap_or_else(|_| space_id.clone());
                    }
                }
            }
        }
        resolved
    };

    // Get page source + permission_level for recent_pages record
    let mut rows2 = conn
        .query(
            "SELECT source, permission_level FROM pages WHERE id = ?1",
            libsql::params![page_id.clone()],
        )
        .await
        .map_err(|e| e.to_string())?;
    let (source, permission_level) = rows2
        .next()
        .await
        .map_err(|e| e.to_string())?
        .map(|r| {
            (
                r.get::<String>(0).unwrap_or_else(|_| "local".to_string()),
                r.get::<String>(1).unwrap_or_else(|_| "owner".to_string()),
            )
        })
        .unwrap_or_else(|| ("local".to_string(), "owner".to_string()));

    // Upsert into registry recent_pages
    let reg_guard = state.registry.lock().await;
    if let Some(reg) = reg_guard.as_ref() {
        if let Ok(reg_conn) = reg.connect() {
            reg_conn
                .execute(
                    "INSERT INTO recent_pages (page_id, space_id, title, space_name, last_accessed_at, source, permission_level) \
                     VALUES (?1, ?2, ?3, ?4, datetime('now'), ?5, ?6) \
                     ON CONFLICT(page_id) DO UPDATE SET \
                         title = excluded.title, \
                         space_name = excluded.space_name, \
                         last_accessed_at = excluded.last_accessed_at, \
                         source = excluded.source, \
                         permission_level = excluded.permission_level",
                    libsql::params![
                        page_id,
                        space_id,
                        page_title,
                        space_name,
                        source,
                        permission_level
                    ],
                )
                .await
                .ok();
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn get_recent_pages(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<RecentPage>, String> {
    let guard = state.registry.lock().await;
    let db = guard.as_ref().ok_or("Registry not initialised")?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    let n = limit.unwrap_or(8);

    let mut rows = conn
        .query(
            "SELECT rp.page_id, rp.title, rp.space_id, rp.space_name, \
                    rp.last_accessed_at, rp.source, rp.permission_level \
             FROM recent_pages rp \
             ORDER BY rp.last_accessed_at DESC \
             LIMIT ?1",
            libsql::params![n],
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut recent = vec![];
    while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        recent.push(RecentPage {
            id: row.get(0).map_err(|e| e.to_string())?,
            title: row.get(1).map_err(|e| e.to_string())?,
            space_id: row.get(2).map_err(|e| e.to_string())?,
            space_name: row.get(3).map_err(|e| e.to_string())?,
            last_accessed_at: row.get(4).map_err(|e| e.to_string())?,
            source: row
                .get::<String>(5)
                .unwrap_or_else(|_| "local".to_string()),
            permission_level: row
                .get::<String>(6)
                .unwrap_or_else(|_| "owner".to_string()),
        });
    }
    Ok(recent)
}

// ── Presence ──────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone, Debug)]
pub struct Presence {
    pub id: String,
    pub user_name: String,
    pub page_id: String,
    pub page_title: String,
    pub space_id: String,
    pub status: String,      // "viewing" | "editing"
    pub last_seen_at: String,
}

async fn ensure_presence_table(conn: &libsql::Connection) -> Result<(), String> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS presence (
            id TEXT PRIMARY KEY,
            user_name TEXT NOT NULL,
            page_id TEXT NOT NULL,
            page_title TEXT NOT NULL,
            space_id TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'viewing',
            last_seen_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        (),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn upsert_presence(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
    page_title: String,
    user_name: String,
    status: String,   // "viewing" | "editing"
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_presence_table(&conn).await?;

    // id = user_name + page_id so one row per (user, page)
    let id = format!("{}::{}", user_name, page_id);
    conn.execute(
        "INSERT INTO presence (id, user_name, page_id, page_title, space_id, status, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))
         ON CONFLICT(id) DO UPDATE SET
           status = excluded.status,
           page_title = excluded.page_title,
           last_seen_at = excluded.last_seen_at",
        libsql::params![id, user_name, page_id, page_title, space_id, status],
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn clear_presence(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
    user_name: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_presence_table(&conn).await?;

    let id = format!("{}::{}", user_name, page_id);
    conn.execute(
        "DELETE FROM presence WHERE id = ?1",
        libsql::params![id],
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_page_presence(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
) -> Result<Vec<Presence>, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_presence_table(&conn).await?;

    // Only return entries seen in the last 90 seconds
    let mut rows = conn
        .query(
            "SELECT id, user_name, page_id, page_title, space_id, status, last_seen_at
             FROM presence
             WHERE page_id = ?1
               AND last_seen_at >= datetime('now', '-90 seconds')
             ORDER BY last_seen_at DESC",
            libsql::params![page_id],
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        results.push(Presence {
            id: row.get(0).map_err(|e| e.to_string())?,
            user_name: row.get(1).map_err(|e| e.to_string())?,
            page_id: row.get(2).map_err(|e| e.to_string())?,
            page_title: row.get(3).map_err(|e| e.to_string())?,
            space_id: row.get(4).map_err(|e| e.to_string())?,
            status: row.get(5).map_err(|e| e.to_string())?,
            last_seen_at: row.get(6).map_err(|e| e.to_string())?,
        });
    }
    Ok(results)
}

#[tauri::command]
pub async fn get_all_presence(
    state: State<'_, AppState>,
    space_id: String,
) -> Result<Vec<Presence>, String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    ensure_presence_table(&conn).await?;

    let mut rows = conn
        .query(
            "SELECT id, user_name, page_id, page_title, space_id, status, last_seen_at
             FROM presence
             WHERE last_seen_at >= datetime('now', '-90 seconds')
             ORDER BY last_seen_at DESC",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        results.push(Presence {
            id: row.get(0).map_err(|e| e.to_string())?,
            user_name: row.get(1).map_err(|e| e.to_string())?,
            page_id: row.get(2).map_err(|e| e.to_string())?,
            page_title: row.get(3).map_err(|e| e.to_string())?,
            space_id: row.get(4).map_err(|e| e.to_string())?,
            status: row.get(5).map_err(|e| e.to_string())?,
            last_seen_at: row.get(6).map_err(|e| e.to_string())?,
        });
    }
    Ok(results)
}

// ── Move page to a different space ───────────────────────────────────────────

/// Moves a page and all its descendants (recursively) from one space DB to another.
///
/// Algorithm:
///   1. BFS from `page_id` in the source DB to collect the full subtree.
///   2. Read all page rows, version rows, and embedding rows for the subtree.
///   3. Insert everything into the target DB (INSERT OR REPLACE), rewriting
///      `space_id` to `to_space_id` on pages and embeddings.
///   4. Delete from source (versions → embeddings → pages) only after all inserts succeed.
#[tauri::command]
pub async fn move_page_to_space(
    state: State<'_, AppState>,
    page_id: String,
    from_space_id: String,
    to_space_id: String,
) -> Result<(), String> {
    eprintln!(
        "[move_page_to_space] page_id={} from={} to={}",
        page_id, from_space_id, to_space_id
    );

    let src_db = get_or_open_space_db(&state, &from_space_id).await?;
    let dst_db = get_or_open_space_db(&state, &to_space_id).await?;

    let src_conn = src_db.connect().map_err(|e| e.to_string())?;
    let dst_conn = dst_db.connect().map_err(|e| e.to_string())?;

    // ── Step 1: BFS to collect the full page subtree ──────────────────────────

    let mut all_ids: Vec<String> = Vec::new();
    let mut queue: Vec<String> = vec![page_id.clone()];

    while !queue.is_empty() {
        let current = std::mem::take(&mut queue);
        for pid in &current {
            all_ids.push(pid.clone());
            // Query children of this page
            let mut rows = src_conn
                .query(
                    "SELECT id FROM pages WHERE parent_page_id = ?1 AND deleted_at IS NULL",
                    libsql::params![pid.clone()],
                )
                .await
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                let child_id: String = row.get(0).map_err(|e| e.to_string())?;
                queue.push(child_id);
            }
        }
    }

    eprintln!(
        "[move_page_to_space] subtree has {} page(s)",
        all_ids.len()
    );

    // ── Step 2 + 3: Read from source, insert into target ─────────────────────

    for pid in &all_ids {
        // Read page row
        let mut page_rows = src_conn
            .query(
                "SELECT id, title, space_id, creator_id, parent_page_id, sort_order, \
                        deleted_at, last_accessed_at, created_at, updated_at, source, remote_id, \
                        permission_level, last_synced_at \
                 FROM pages WHERE id = ?1",
                libsql::params![pid.clone()],
            )
            .await
            .map_err(|e| e.to_string())?;

        let page_row = page_rows
            .next()
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("page '{}' not found in source space", pid))?;

        let p_id: String = page_row.get(0).map_err(|e| e.to_string())?;
        let p_title: String = page_row.get(1).map_err(|e| e.to_string())?;
        // Intentionally ignore source space_id (col 2) — we overwrite it
        let p_creator_id: String = page_row.get(3).map_err(|e| e.to_string())?;
        let p_parent_page_id: Option<String> = page_row.get(4).map_err(|e| e.to_string())?;
        let p_sort_order: i64 = page_row.get(5).map_err(|e| e.to_string())?;
        let p_deleted_at: Option<String> = page_row.get(6).map_err(|e| e.to_string())?;
        let p_last_accessed_at: Option<String> = page_row.get(7).map_err(|e| e.to_string())?;
        let p_created_at: String = page_row.get(8).map_err(|e| e.to_string())?;
        let p_updated_at: String = page_row.get(9).map_err(|e| e.to_string())?;
        let p_source: String = page_row
            .get::<String>(10)
            .unwrap_or_else(|_| "local".to_string());
        let p_remote_id: Option<String> = page_row.get(11).map_err(|e| e.to_string())?;
        let p_permission_level: String = page_row
            .get::<String>(12)
            .unwrap_or_else(|_| "owner".to_string());
        let p_last_synced_at: Option<String> = page_row.get(13).map_err(|e| e.to_string())?;

        // Insert page into target — overwrite space_id with to_space_id
        dst_conn
            .execute(
                "INSERT OR REPLACE INTO pages \
                 (id, title, space_id, creator_id, parent_page_id, sort_order, \
                  deleted_at, last_accessed_at, created_at, updated_at, source, remote_id, \
                  permission_level, last_synced_at) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                libsql::params![
                    p_id,
                    p_title,
                    to_space_id.clone(),
                    p_creator_id,
                    p_parent_page_id,
                    p_sort_order,
                    p_deleted_at,
                    p_last_accessed_at,
                    p_created_at,
                    p_updated_at,
                    p_source,
                    p_remote_id,
                    p_permission_level,
                    p_last_synced_at
                ],
            )
            .await
            .map_err(|e| e.to_string())?;

        // Read and insert all version rows for this page
        let mut ver_rows = src_conn
            .query(
                "SELECT id, page_id, owner_id, based_on_version_id, title, content, text_content, \
                        is_published, is_frozen, version_num, created_at, updated_at \
                 FROM page_versions WHERE page_id = ?1",
                libsql::params![pid.clone()],
            )
            .await
            .map_err(|e| e.to_string())?;

        while let Some(vrow) = ver_rows.next().await.map_err(|e| e.to_string())? {
            let v_id: String = vrow.get(0).map_err(|e| e.to_string())?;
            let v_page_id: String = vrow.get(1).map_err(|e| e.to_string())?;
            let v_owner_id: String = vrow.get(2).map_err(|e| e.to_string())?;
            let v_based_on: Option<String> = vrow.get(3).map_err(|e| e.to_string())?;
            let v_title: Option<String> = vrow.get(4).map_err(|e| e.to_string())?;
            let v_content: Option<String> = vrow.get(5).map_err(|e| e.to_string())?;
            let v_text_content: Option<String> = vrow.get(6).map_err(|e| e.to_string())?;
            let v_is_published: i64 = vrow.get(7).unwrap_or(0);
            let v_is_frozen: i64 = vrow.get(8).unwrap_or(0);
            let v_version_num: i64 = vrow.get(9).unwrap_or(1);
            let v_created_at: String = vrow.get(10).map_err(|e| e.to_string())?;
            let v_updated_at: String = vrow.get(11).map_err(|e| e.to_string())?;

            dst_conn
                .execute(
                    "INSERT OR REPLACE INTO page_versions \
                     (id, page_id, owner_id, based_on_version_id, title, content, text_content, \
                      is_published, is_frozen, version_num, created_at, updated_at) \
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                    libsql::params![
                        v_id,
                        v_page_id,
                        v_owner_id,
                        v_based_on,
                        v_title,
                        v_content,
                        v_text_content,
                        v_is_published,
                        v_is_frozen,
                        v_version_num,
                        v_created_at,
                        v_updated_at
                    ],
                )
                .await
                .map_err(|e| e.to_string())?;
        }

        // Read and insert all embedding rows for this page — overwrite space_id
        let mut emb_rows = src_conn
            .query(
                "SELECT version_id, page_id, space_id, embedding \
                 FROM page_embeddings WHERE page_id = ?1",
                libsql::params![pid.clone()],
            )
            .await
            .map_err(|e| e.to_string())?;

        while let Some(erow) = emb_rows.next().await.map_err(|e| e.to_string())? {
            let e_version_id: String = erow.get(0).map_err(|e| e.to_string())?;
            let e_page_id: String = erow.get(1).map_err(|e| e.to_string())?;
            // Ignore source space_id (col 2) — overwrite with to_space_id
            let e_embedding: String = erow.get(3).map_err(|e| e.to_string())?;

            dst_conn
                .execute(
                    "INSERT OR REPLACE INTO page_embeddings \
                     (version_id, page_id, space_id, embedding) \
                     VALUES (?1, ?2, ?3, ?4)",
                    libsql::params![e_version_id, e_page_id, to_space_id.clone(), e_embedding],
                )
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    // ── Step 4: Delete from source (versions → embeddings → pages) ───────────
    // All inserts succeeded; now clean up source.

    for pid in &all_ids {
        src_conn
            .execute(
                "DELETE FROM page_versions WHERE page_id = ?1",
                libsql::params![pid.clone()],
            )
            .await
            .map_err(|e| e.to_string())?;

        src_conn
            .execute(
                "DELETE FROM page_embeddings WHERE page_id = ?1",
                libsql::params![pid.clone()],
            )
            .await
            .map_err(|e| e.to_string())?;

        src_conn
            .execute(
                "DELETE FROM pages WHERE id = ?1",
                libsql::params![pid.clone()],
            )
            .await
            .map_err(|e| e.to_string())?;
    }

    eprintln!(
        "[move_page_to_space] done — moved {} page(s) from {} to {}",
        all_ids.len(),
        from_space_id,
        to_space_id
    );

    Ok(())
}

// ── Row-builder helpers ───────────────────────────────────────────────────────

fn build_page(row: &libsql::Row) -> Result<Page, String> {
    Ok(Page {
        id: row.get(0).map_err(|e| e.to_string())?,
        title: row.get(1).map_err(|e| e.to_string())?,
        space_id: row.get(2).map_err(|e| e.to_string())?,
        creator_id: row.get(3).map_err(|e| e.to_string())?,
        parent_page_id: row.get(4).map_err(|e| e.to_string())?,
        sort_order: row.get(5).map_err(|e| e.to_string())?,
        created_at: row.get(6).map_err(|e| e.to_string())?,
        updated_at: row.get(7).map_err(|e| e.to_string())?,
        source: row
            .get::<String>(8)
            .unwrap_or_else(|_| "local".to_string()),
        remote_id: row.get(9).map_err(|e| e.to_string())?,
        permission_level: row
            .get::<String>(10)
            .unwrap_or_else(|_| "owner".to_string()),
        last_synced_at: row.get(11).map_err(|e| e.to_string())?,
    })
}

fn build_page_version(row: &libsql::Row) -> Result<PageVersion, String> {
    Ok(PageVersion {
        id: row.get(0).map_err(|e| e.to_string())?,
        page_id: row.get(1).map_err(|e| e.to_string())?,
        owner_id: row.get(2).map_err(|e| e.to_string())?,
        based_on_version_id: row.get(3).map_err(|e| e.to_string())?,
        title: row.get(4).map_err(|e| e.to_string())?,
        content: row.get(5).map_err(|e| e.to_string())?,
        text_content: row.get(6).map_err(|e| e.to_string())?,
        is_published: {
            let v: i64 = row.get(7).map_err(|e| e.to_string())?;
            v == 1
        },
        is_frozen: {
            let v: i64 = row.get(8).map_err(|e| e.to_string())?;
            v == 1
        },
        version_num: row.get(9).map_err(|e| e.to_string())?,
        created_at: row.get(10).map_err(|e| e.to_string())?,
        updated_at: row.get(11).map_err(|e| e.to_string())?,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod db_path_tests {
    use super::{db_path_from_env, registry_path_from_env, space_db_path_from_env};

    // ── db_path_from_env (legacy) — keep all existing tests ───────────────────

    #[test]
    fn default_uses_home_dotbamako() {
        let path = db_path_from_env(None, Some("/home/alice".into()));
        assert_eq!(path, "/home/alice/.bamako/local.db");
    }

    #[test]
    fn default_without_home_falls_back_to_tmp() {
        let path = db_path_from_env(None, None);
        assert_eq!(path, "/tmp/.bamako/local.db");
    }

    #[test]
    fn bamako_data_overrides_home() {
        let path = db_path_from_env(
            Some("/tmp/bam-alice".into()),
            Some("/home/alice".into()),
        );
        assert_eq!(path, "/tmp/bam-alice/local.db");
    }

    #[test]
    fn bamako_data_works_without_home() {
        let path = db_path_from_env(Some("/tmp/bam-alice".into()), None);
        assert_eq!(path, "/tmp/bam-alice/local.db");
    }

    #[test]
    fn trailing_slash_is_stripped() {
        let path = db_path_from_env(
            Some("/tmp/bam-alice/".into()),
            Some("/home/alice".into()),
        );
        assert_eq!(path, "/tmp/bam-alice/local.db");
    }

    #[test]
    fn multiple_trailing_slashes_are_stripped() {
        let path = db_path_from_env(
            Some("/tmp/bam-alice///".into()),
            Some("/home/alice".into()),
        );
        assert_eq!(path, "/tmp/bam-alice/local.db");
    }

    #[test]
    fn empty_bamako_data_falls_back_to_home() {
        let path = db_path_from_env(Some("".into()), Some("/home/alice".into()));
        assert_eq!(path, "/home/alice/.bamako/local.db");
    }

    #[test]
    fn whitespace_only_bamako_data_falls_back_to_home() {
        let path = db_path_from_env(Some("   ".into()), Some("/home/alice".into()));
        assert_eq!(path, "/home/alice/.bamako/local.db");
    }

    #[test]
    fn bare_slash_bamako_data_falls_back_to_home() {
        let path = db_path_from_env(Some("/".into()), Some("/home/alice".into()));
        assert_eq!(path, "/home/alice/.bamako/local.db");
    }

    #[test]
    fn multiple_slashes_only_falls_back_to_home() {
        let path = db_path_from_env(Some("///".into()), Some("/home/alice".into()));
        assert_eq!(path, "/home/alice/.bamako/local.db");
    }

    #[test]
    fn two_different_users_get_different_paths() {
        let alice = db_path_from_env(Some("/tmp/bam-alice".into()), Some("/home/alice".into()));
        let bob = db_path_from_env(Some("/tmp/bam-bob".into()), Some("/home/bob".into()));
        assert_ne!(alice, bob);
        assert_eq!(alice, "/tmp/bam-alice/local.db");
        assert_eq!(bob, "/tmp/bam-bob/local.db");
    }

    #[test]
    fn path_always_ends_with_local_db() {
        let cases: Vec<(Option<String>, Option<String>)> = vec![
            (None, Some("/home/alice".into())),
            (None, None),
            (Some("/tmp/bam-a".into()), Some("/home/alice".into())),
            (Some("/tmp/bam-a/".into()), None),
            (Some("".into()), Some("/home/alice".into())),
        ];
        for (data, home) in cases {
            let path = db_path_from_env(data, home);
            assert!(
                path.ends_with("/local.db"),
                "expected path to end with /local.db, got: {path}"
            );
        }
    }

    #[test]
    fn path_never_contains_double_slash_in_middle() {
        let cases: Vec<(Option<String>, Option<String>)> = vec![
            (Some("/tmp/bam-alice".into()), Some("/home/alice".into())),
            (Some("/tmp/bam-alice/".into()), Some("/home/alice".into())),
            (None, Some("/home/alice".into())),
            (None, None),
        ];
        for (data, home) in cases {
            let path = db_path_from_env(data, home);
            let without_leading = path.trim_start_matches('/');
            assert!(
                !without_leading.contains("//"),
                "double slash in path: {path}"
            );
        }
    }

    // ── Integration tests (real env var reading) ──────────────────────────────

    use std::sync::Mutex;
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn real_env_bamako_data_is_picked_up() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("BAMAKO_DATA").ok();
        std::env::set_var("BAMAKO_DATA", "/tmp/test-sim-alice");
        let path = super::db_path();
        match prev {
            Some(v) => std::env::set_var("BAMAKO_DATA", v),
            None => std::env::remove_var("BAMAKO_DATA"),
        }
        assert_eq!(path, "/tmp/test-sim-alice/local.db");
    }

    #[test]
    fn real_env_empty_bamako_data_falls_back() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev_data = std::env::var("BAMAKO_DATA").ok();
        let prev_home = std::env::var("HOME").ok();
        std::env::set_var("BAMAKO_DATA", "");
        std::env::set_var("HOME", "/home/testuser");
        let path = super::db_path();
        match prev_data {
            Some(v) => std::env::set_var("BAMAKO_DATA", v),
            None => std::env::remove_var("BAMAKO_DATA"),
        }
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        assert_eq!(path, "/home/testuser/.bamako/local.db");
    }

    #[test]
    fn real_env_unset_bamako_data_uses_home() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev_data = std::env::var("BAMAKO_DATA").ok();
        let prev_home = std::env::var("HOME").ok();
        std::env::remove_var("BAMAKO_DATA");
        std::env::set_var("HOME", "/home/testuser2");
        let path = super::db_path();
        match prev_data {
            Some(v) => std::env::set_var("BAMAKO_DATA", v),
            None => std::env::remove_var("BAMAKO_DATA"),
        }
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        assert_eq!(path, "/home/testuser2/.bamako/local.db");
    }

    // ── registry_path_from_env tests ──────────────────────────────────────────

    #[test]
    fn registry_default_uses_home_dotbamako() {
        let path = registry_path_from_env(None, Some("/home/alice".into()));
        assert_eq!(path, "/home/alice/.bamako/registry.db");
    }

    #[test]
    fn registry_default_without_home_falls_back_to_tmp() {
        let path = registry_path_from_env(None, None);
        assert_eq!(path, "/tmp/.bamako/registry.db");
    }

    #[test]
    fn registry_bamako_data_overrides_home() {
        let path = registry_path_from_env(
            Some("/tmp/bam-alice".into()),
            Some("/home/alice".into()),
        );
        assert_eq!(path, "/tmp/bam-alice/registry.db");
    }

    #[test]
    fn registry_trailing_slash_is_stripped() {
        let path = registry_path_from_env(
            Some("/tmp/bam-alice/".into()),
            Some("/home/alice".into()),
        );
        assert_eq!(path, "/tmp/bam-alice/registry.db");
    }

    #[test]
    fn registry_empty_bamako_data_falls_back() {
        let path = registry_path_from_env(Some("".into()), Some("/home/alice".into()));
        assert_eq!(path, "/home/alice/.bamako/registry.db");
    }

    #[test]
    fn registry_whitespace_only_falls_back() {
        let path = registry_path_from_env(Some("   ".into()), Some("/home/alice".into()));
        assert_eq!(path, "/home/alice/.bamako/registry.db");
    }

    #[test]
    fn registry_bare_slash_falls_back() {
        let path = registry_path_from_env(Some("/".into()), Some("/home/alice".into()));
        assert_eq!(path, "/home/alice/.bamako/registry.db");
    }

    #[test]
    fn registry_path_always_ends_with_registry_db() {
        let cases: Vec<(Option<String>, Option<String>)> = vec![
            (None, Some("/home/alice".into())),
            (None, None),
            (Some("/tmp/bam-a".into()), Some("/home/alice".into())),
            (Some("/tmp/bam-a/".into()), None),
            (Some("".into()), Some("/home/alice".into())),
        ];
        for (data, home) in cases {
            let path = registry_path_from_env(data, home);
            assert!(
                path.ends_with("/registry.db"),
                "expected path to end with /registry.db, got: {path}"
            );
        }
    }

    #[test]
    fn registry_path_never_contains_double_slash_in_middle() {
        let cases: Vec<(Option<String>, Option<String>)> = vec![
            (Some("/tmp/bam-alice".into()), Some("/home/alice".into())),
            (Some("/tmp/bam-alice/".into()), Some("/home/alice".into())),
            (None, Some("/home/alice".into())),
            (None, None),
        ];
        for (data, home) in cases {
            let path = registry_path_from_env(data, home);
            let without_leading = path.trim_start_matches('/');
            assert!(
                !without_leading.contains("//"),
                "double slash in registry path: {path}"
            );
        }
    }

    #[test]
    fn registry_two_different_users_get_different_paths() {
        let alice = registry_path_from_env(
            Some("/tmp/bam-alice".into()),
            Some("/home/alice".into()),
        );
        let bob = registry_path_from_env(
            Some("/tmp/bam-bob".into()),
            Some("/home/bob".into()),
        );
        assert_ne!(alice, bob);
        assert_eq!(alice, "/tmp/bam-alice/registry.db");
        assert_eq!(bob, "/tmp/bam-bob/registry.db");
    }

    // ── space_db_path_from_env tests ──────────────────────────────────────────

    #[test]
    fn space_default_uses_home_dotbamako() {
        let path = space_db_path_from_env("abc123", None, Some("/home/alice".into()));
        assert_eq!(path, "/home/alice/.bamako/spaces/abc123.db");
    }

    #[test]
    fn space_default_without_home_falls_back_to_tmp() {
        let path = space_db_path_from_env("abc123", None, None);
        assert_eq!(path, "/tmp/.bamako/spaces/abc123.db");
    }

    #[test]
    fn space_bamako_data_overrides_home() {
        let path = space_db_path_from_env(
            "abc123",
            Some("/tmp/bam-alice".into()),
            Some("/home/alice".into()),
        );
        assert_eq!(path, "/tmp/bam-alice/spaces/abc123.db");
    }

    #[test]
    fn space_trailing_slash_is_stripped() {
        let path = space_db_path_from_env(
            "abc123",
            Some("/tmp/bam-alice/".into()),
            Some("/home/alice".into()),
        );
        assert_eq!(path, "/tmp/bam-alice/spaces/abc123.db");
    }

    #[test]
    fn space_empty_bamako_data_falls_back() {
        let path = space_db_path_from_env("abc123", Some("".into()), Some("/home/alice".into()));
        assert_eq!(path, "/home/alice/.bamako/spaces/abc123.db");
    }

    #[test]
    fn space_whitespace_only_falls_back() {
        let path =
            space_db_path_from_env("abc123", Some("   ".into()), Some("/home/alice".into()));
        assert_eq!(path, "/home/alice/.bamako/spaces/abc123.db");
    }

    #[test]
    fn space_bare_slash_falls_back() {
        let path = space_db_path_from_env("abc123", Some("/".into()), Some("/home/alice".into()));
        assert_eq!(path, "/home/alice/.bamako/spaces/abc123.db");
    }

    #[test]
    fn space_two_different_space_ids_produce_different_paths() {
        let path_a = space_db_path_from_env("space-aaa", None, Some("/home/alice".into()));
        let path_b = space_db_path_from_env("space-bbb", None, Some("/home/alice".into()));
        assert_ne!(path_a, path_b);
        assert_eq!(path_a, "/home/alice/.bamako/spaces/space-aaa.db");
        assert_eq!(path_b, "/home/alice/.bamako/spaces/space-bbb.db");
    }

    #[test]
    fn space_path_always_ends_with_db() {
        let cases: Vec<(Option<String>, Option<String>)> = vec![
            (None, Some("/home/alice".into())),
            (None, None),
            (Some("/tmp/bam-a".into()), Some("/home/alice".into())),
            (Some("/tmp/bam-a/".into()), None),
            (Some("".into()), Some("/home/alice".into())),
        ];
        for (data, home) in cases {
            let path = space_db_path_from_env("myspace", data, home);
            assert!(
                path.ends_with(".db"),
                "expected path to end with .db, got: {path}"
            );
        }
    }

    #[test]
    fn space_path_never_contains_double_slash_in_middle() {
        let cases: Vec<(Option<String>, Option<String>)> = vec![
            (Some("/tmp/bam-alice".into()), Some("/home/alice".into())),
            (Some("/tmp/bam-alice/".into()), Some("/home/alice".into())),
            (None, Some("/home/alice".into())),
            (None, None),
        ];
        for (data, home) in cases {
            let path = space_db_path_from_env("myspace", data, home);
            let without_leading = path.trim_start_matches('/');
            assert!(
                !without_leading.contains("//"),
                "double slash in space path: {path}"
            );
        }
    }

    #[test]
    fn space_two_different_users_same_space_id_get_different_paths() {
        let alice = space_db_path_from_env(
            "shared-space",
            Some("/tmp/bam-alice".into()),
            Some("/home/alice".into()),
        );
        let bob = space_db_path_from_env(
            "shared-space",
            Some("/tmp/bam-bob".into()),
            Some("/home/bob".into()),
        );
        assert_ne!(alice, bob);
        assert_eq!(alice, "/tmp/bam-alice/spaces/shared-space.db");
        assert_eq!(bob, "/tmp/bam-bob/spaces/shared-space.db");
    }
}

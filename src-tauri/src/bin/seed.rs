use nanoid::nanoid;

const PAGES_DDL: &str = "
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
";

struct PageSeed {
    title: &'static str,
    content: &'static str,
}

async fn seed_namespace(
    ns_url: &str,
    namespace: &str,
    pages: &[PageSeed],
) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("[seed] connecting to {} ...", ns_url);

    let db = libsql::Builder::new_remote(ns_url.to_string(), String::new())
        .build()
        .await?;
    let conn = db.connect()?;

    // Run schema
    for stmt in PAGES_DDL.split(';') {
        let s = stmt.trim();
        if s.is_empty() {
            continue;
        }
        conn.execute(s, ()).await?;
    }

    // Use a fixed, stable space_id so re-seeding is idempotent.
    // The app does NOT filter by space_id for remote spaces (sqld instance IS the boundary).
    let space_id = "default";
    let owner_id = "seed_user_001";

    for page in pages {
        // Check if page already exists (idempotent re-seed)
        let mut existing = conn
            .query(
                "SELECT id FROM pages WHERE title = ?1 AND space_id = ?2 AND deleted_at IS NULL",
                libsql::params![page.title.to_string(), space_id.to_string()],
            )
            .await?;
        if existing.next().await?.is_some() {
            eprintln!("[seed]   skipping (already exists): {}", page.title);
            continue;
        }

        let page_id = nanoid!();
        let version_id = nanoid!();

        conn.execute(
            "INSERT INTO pages (id, title, space_id, creator_id, source, permission_level) \
             VALUES (?1, ?2, ?3, ?4, 'local', 'owner')",
            libsql::params![
                page_id.clone(),
                page.title.to_string(),
                space_id.to_string(),
                owner_id.to_string()
            ],
        )
        .await?;

        conn.execute(
            "INSERT INTO page_versions (id, page_id, owner_id, title, content, text_content, \
             is_published, version_num) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 1)",
            libsql::params![
                version_id,
                page_id,
                owner_id.to_string(),
                page.title.to_string(),
                page.content.to_string(),
                page.title.to_string()
            ],
        )
        .await?;

        eprintln!("[seed]   inserted page: {}", page.title);
    }

    eprintln!("[seed] namespace '{}' seeded.", namespace);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    // Two separate sqld instances: shared on 8093, alice-private on 8095
    let mut shared_url = "http://127.0.0.1:8093".to_string();
    let mut alice_url = "http://127.0.0.1:8095".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--shared-url" if i + 1 < args.len() => {
                shared_url = args[i + 1].clone();
                i += 2;
            }
            "--alice-url" if i + 1 < args.len() => {
                alice_url = args[i + 1].clone();
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }

    eprintln!("[seed] shared sqld:        {}", shared_url);
    eprintln!("[seed] alice-private sqld: {}", alice_url);

    seed_namespace(
        &shared_url,
        "shared",
        &[
            PageSeed {
                title: "Team Guidelines",
                content: "# Team Guidelines\n\nPlease read before contributing.\n",
            },
            PageSeed {
                title: "Architecture Overview",
                content: "# Architecture\n\nBamako uses sqld for distributed sync.\n",
            },
            PageSeed {
                title: "Release Notes",
                content: "# Release Notes\n\nv0.1 — Initial release.\n",
            },
        ],
    )
    .await?;

    seed_namespace(
        &alice_url,
        "alice-private",
        &[
            PageSeed {
                title: "My Notes",
                content: "# Personal Notes\n\nJust for Alice.\n",
            },
            PageSeed {
                title: "Draft Ideas",
                content: "# Ideas\n\nWork in progress...\n",
            },
        ],
    )
    .await?;

    println!("\u{2705} Seeding complete!");
    println!();
    println!("\u{2500}\u{2500} Alice \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    println!("  Add in Bamako Settings \u{2192} Connect Server:");
    println!("  URL: {shared_url:<32}  Name: shared         Permission: write");
    println!("  URL: {alice_url:<32}  Name: alice-private  Permission: owner");
    println!();
    println!("\u{2500}\u{2500} Bob \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    println!("  Add in Bamako Settings \u{2192} Connect Server:");
    println!("  URL: {shared_url:<32}  Name: shared         Permission: read");
    println!();
    println!("\u{2500}\u{2500} Run two instances \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    println!("  Terminal 1:  BAMAKO_DATA=/tmp/bam-alice npm run tauri dev");
    println!("  Terminal 2:  BAMAKO_DATA=/tmp/bam-bob   VITE_PORT=5275 npm run tauri dev");

    Ok(())
}

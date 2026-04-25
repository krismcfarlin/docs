/// Vector search via VelesDB (local HTTP) with SQLite fallback.
///
/// Architecture:
///   - Documents are replicated via sqld (libsql embedded replica → sqld server)
///   - Embeddings are stored in VelesDB, one collection per space_id
///   - Permission model: only pages in the local DB (i.e. pages you can read)
///     are vectorized — no separate ACL needed
///   - When VelesDB is unreachable, falls back to SQLite cosine similarity

use crate::commands::db::get_or_open_space_db;
use crate::state::AppState;
use tauri::State;

const DIMS: usize = 64;
const VELESDB_URL: &str = "http://localhost:9000";

// ── Embedding (64-dim bag-of-words, L2-normalised) ────────────────────────────

fn embed(text: &str) -> [f32; DIMS] {
    let mut v = [0.0f32; DIMS];
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        let w = word.to_lowercase();
        if w.len() < 3 {
            continue;
        }
        let mut h: u64 = 14_695_981_039_346_656_037;
        for b in w.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(1_099_511_628_211);
        }
        v[(h % DIMS as u64) as usize] += 1.0;
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-9 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    v
}

fn cosine(a: &[f32; DIMS], b: &[f32; DIMS]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn vec_to_json(v: &[f32; DIMS]) -> String {
    let inner: Vec<String> = v.iter().map(|x| format!("{:.6}", x)).collect();
    format!("[{}]", inner.join(","))
}

fn json_to_vec(s: &str) -> Option<[f32; DIMS]> {
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    let nums: Vec<f32> = s
        .split(',')
        .filter_map(|t| t.trim().parse().ok())
        .collect();
    if nums.len() != DIMS {
        return None;
    }
    let mut arr = [0.0f32; DIMS];
    arr.copy_from_slice(&nums);
    Some(arr)
}

/// Extract a ~160-char snippet from `text` centred around the first occurrence
/// of any query word. Falls back to the start of the document.
fn make_snippet(text: &str, query: &str) -> String {
    let plain: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();

    let lower = plain.to_lowercase();
    let pos = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .filter_map(|w| lower.find(&w.to_lowercase()))
        .min()
        .unwrap_or(0);

    let start = pos.saturating_sub(40);
    let window: String = plain.chars().skip(start).take(160).collect();
    let trimmed = window.trim().to_string();
    if start > 0 {
        format!("…{trimmed}")
    } else {
        trimmed
    }
}

/// Deterministic u64 id from a string (for VelesDB integer point ids).
fn str_to_id(s: &str) -> u64 {
    let mut h: u64 = 14_695_981_039_346_656_037;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1_099_511_628_211);
    }
    h
}

// ── VelesDB HTTP helpers ──────────────────────────────────────────────────────

async fn veles_ensure_collection(client: &reqwest::Client, collection: &str) -> bool {
    let body = serde_json::json!({
        "name": collection,
        "dimension": DIMS as u64,
        "metric": "cosine"
    });
    let res = client
        .post(format!("{VELESDB_URL}/collections"))
        .json(&body)
        .send()
        .await;
    match res {
        Ok(r) => r.status().is_success() || r.status().as_u16() == 409,
        Err(_) => false,
    }
}

async fn veles_upsert(
    client: &reqwest::Client,
    collection: &str,
    version_id: &str,
    page_id: &str,
    title: &str,
    vector: &[f32; DIMS],
) -> bool {
    let point_id = str_to_id(version_id);
    let vec_list: Vec<f32> = vector.to_vec();
    let body = serde_json::json!({
        "points": [{
            "id": point_id,
            "vector": vec_list,
            "payload": {
                "version_id": version_id,
                "page_id": page_id,
                "title": title
            }
        }]
    });
    let res = client
        .post(format!("{VELESDB_URL}/collections/{collection}/points"))
        .json(&body)
        .send()
        .await;
    res.map(|r| r.status().is_success()).unwrap_or(false)
}

#[derive(serde::Deserialize)]
struct VelesHit {
    score: f32,
    payload: Option<serde_json::Value>,
}

async fn veles_search(
    client: &reqwest::Client,
    collection: &str,
    query_vec: &[f32; DIMS],
    top_k: usize,
) -> Option<Vec<SearchResult>> {
    let qv: Vec<f32> = query_vec.to_vec();
    let body = serde_json::json!({
        "vector": qv,
        "top_k": top_k as u64
    });
    let res = client
        .post(format!("{VELESDB_URL}/collections/{collection}/search"))
        .json(&body)
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        return None;
    }

    let hits: Vec<VelesHit> = res.json().await.ok()?;
    let results = hits
        .into_iter()
        .filter_map(|h| {
            let p = h.payload?;
            Some(SearchResult {
                version_id: p["version_id"].as_str()?.to_string(),
                page_id: p["page_id"].as_str()?.to_string(),
                title: p["title"].as_str().unwrap_or("Untitled").to_string(),
                score: h.score,
                snippet: String::new(),
            })
        })
        .collect();
    Some(results)
}

// ── Tauri Commands ────────────────────────────────────────────────────────────

/// Embed and store a page version in VelesDB (collection = space_id).
/// Also stores the embedding in the per-space SQLite DB for offline fallback.
/// Called automatically on freeze; can also be triggered manually.
#[tauri::command]
pub async fn vectorize_page(
    state: State<'_, AppState>,
    version_id: String,
    space_id: String,
) -> Result<(), String> {
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    let mut rows = conn
        .query(
            "SELECT pv.page_id, pv.title, pv.text_content, pv.content, p.space_id \
             FROM page_versions pv \
             JOIN pages p ON p.id = pv.page_id \
             WHERE pv.id = ?1",
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
    let text_content: Option<String> = row.get(2).map_err(|e| e.to_string())?;
    let content: Option<String> = row.get(3).map_err(|e| e.to_string())?;
    let sid: String = row.get(4).map_err(|e| e.to_string())?;

    let title_str = title.as_deref().unwrap_or("Untitled");
    let text = text_content
        .filter(|s| !s.is_empty())
        .or_else(|| content.filter(|s| !s.is_empty()))
        .unwrap_or_else(|| title_str.to_string());

    let embedding = embed(&text);

    // Store in per-space SQLite (always — works offline)
    let json = vec_to_json(&embedding);
    conn.execute(
        "INSERT OR REPLACE INTO page_embeddings (version_id, page_id, space_id, embedding) \
         VALUES (?1, ?2, ?3, ?4)",
        libsql::params![
            version_id.clone(),
            page_id.clone(),
            sid.clone(),
            json
        ],
    )
    .await
    .map_err(|e| e.to_string())?;

    // Also push to VelesDB if available (best-effort, non-fatal)
    let collection = format!("space_{}", sid);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|e| e.to_string())?;

    if veles_ensure_collection(&client, &collection).await {
        veles_upsert(&client, &collection, &version_id, &page_id, title_str, &embedding).await;
    }

    Ok(())
}

/// Search for pages similar to a query within a space.
/// Uses VelesDB if available, falls back to SQLite cosine similarity.
/// Returns a snippet of matching content alongside each result.
#[tauri::command]
pub async fn search_similar_pages(
    state: State<'_, AppState>,
    space_id: String,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<SearchResult>, String> {
    let k = limit.unwrap_or(10) as usize;
    let q_vec = embed(&query);
    let collection = format!("space_{}", space_id);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|e| e.to_string())?;

    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;

    // Helper: fetch snippet for a version_id from the space DB
    async fn fetch_snippet(conn: &libsql::Connection, version_id: &str, query: &str) -> String {
        let mut r = conn
            .query(
                "SELECT COALESCE(text_content, content, '') FROM page_versions WHERE id = ?1",
                libsql::params![version_id.to_string()],
            )
            .await
            .ok();
        if let Some(ref mut rows) = r {
            if let Ok(Some(row)) = rows.next().await {
                let text: String = row.get(0).unwrap_or_default();
                if !text.is_empty() {
                    return make_snippet(&text, query);
                }
            }
        }
        String::new()
    }

    // Try VelesDB first
    if let Some(mut results) = veles_search(&client, &collection, &q_vec, k).await {
        for r in &mut results {
            r.snippet = fetch_snippet(&conn, &r.version_id, &query).await;
        }
        return Ok(results);
    }

    // Fallback: load all embeddings from the space SQLite and rank in Rust
    let mut rows = conn
        .query(
            "SELECT pe.version_id, pe.page_id, pe.embedding, \
                    COALESCE(pv.title, p.title, 'Untitled'), \
                    COALESCE(pv.text_content, pv.content, '') \
             FROM page_embeddings pe \
             JOIN page_versions pv ON pv.id = pe.version_id \
             JOIN pages          p ON p.id  = pe.page_id \
             WHERE pe.space_id = ?1",
            libsql::params![space_id],
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut scored: Vec<(f32, SearchResult)> = vec![];
    while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
        let version_id: String = row.get(0).map_err(|e| e.to_string())?;
        let page_id: String = row.get(1).map_err(|e| e.to_string())?;
        let json: String = row.get(2).map_err(|e| e.to_string())?;
        let title: String = row.get(3).map_err(|e| e.to_string())?;
        let text: String = row.get(4).unwrap_or_default();

        if let Some(vec) = json_to_vec(&json) {
            let score = cosine(&q_vec, &vec);
            let snippet = make_snippet(&text, &query);
            scored.push((
                score,
                SearchResult {
                    version_id,
                    page_id,
                    title,
                    score,
                    snippet,
                },
            ));
        }
    }

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(scored.into_iter().take(k).map(|(_, r)| r).collect())
}

#[derive(serde::Serialize)]
pub struct SearchResult {
    pub version_id: String,
    pub page_id: String,
    pub title: String,
    pub score: f32,
    pub snippet: String,
}

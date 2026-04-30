# Bamako Modularization Plan: `bamako-synthesis` Crate

## 1. Goal

Extract every LLM-driven knowledge operation — page summarization, entity extraction, wiki stub generation, space overview, wiki Q&A, lint analysis, vector embedding — into a standalone Rust crate: **`bamako-synthesis`**.

The Tauri app (`src-tauri`) becomes a thin orchestration layer: it reads from SQLite, calls the crate, writes results back to SQLite, and emits Tauri events.

---

## 2. Current State

### Files involved

| File | LOC | Role |
|------|-----|------|
| `src-tauri/src/commands/synthesis.rs` | ~1600 | All Tauri commands for synthesis, entity management, wiki, graph data queries |
| `src-tauri/src/commands/graph_synthesis.rs` | ~643 | 4-task graph-flow pipeline (Profiler → NodeExtractor → EdgeExtractor → EntityResolver) |
| `src-tauri/src/commands/vector.rs` | ~367 | Local embedding (64-dim BoW), VelesDB HTTP client, SQLite fallback search |

### Key problems with the current structure

1. **`call_openrouter` is duplicated** — implemented independently in both `synthesis.rs` and `graph_synthesis.rs`.
2. **Zero separation of concerns** — DB reads, LLM calls, and DB writes are interleaved in single async functions.
3. **Untestable** — no way to unit-test synthesis logic without Tauri state and a real SQLite database.
4. **Tauri-locked** — the synthesis logic can't be used from a CLI, a background daemon, or a future web API.
5. **Single-crate complexity** — adding features to synthesis requires touching a 1600-line file that also owns Tauri command registration.

---

## 3. Target Architecture

```
bamako/
├── Cargo.toml                        ← NEW: workspace root
├── crates/
│   └── bamako-synthesis/             ← NEW crate
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs                ← public API surface
│           ├── error.rs              ← SynthesisError type
│           ├── llm.rs                ← unified OpenRouter client
│           ├── graph/
│           │   ├── mod.rs            ← run_graph_synthesis() entry point
│           │   ├── tasks.rs          ← 4 Task impls
│           │   └── types.rs          ← SourceProfile, ExtractedNode, ExtractedEdge, ResolvedGraph
│           ├── synthesis/
│           │   ├── mod.rs            ← page summarization, space overview, ask_wiki, lint
│           │   └── types.rs          ← PageSynthesis, SpaceConfig, SpaceOverview, WikiAnswer
│           ├── vector/
│           │   ├── mod.rs            ← embed(), cosine(), make_snippet()
│           │   └── veles.rs          ← VelesDB HTTP helpers (ensure_collection, upsert, search)
│           └── wiki/
│               └── mod.rs            ← update_mentioned_in_section(), build_wiki_stub(), content_hash()
│
├── src-tauri/
│   ├── Cargo.toml                    ← add bamako-synthesis path dep, remove duplicated deps
│   └── src/
│       ├── commands/
│       │   ├── synthesis.rs          ← THINNED: DB reads → crate call → DB writes → emit events
│       │   ├── graph_synthesis.rs    ← REPLACED: re-export or deleted
│       │   └── vector.rs             ← THINNED: thin wrapper around crate embedding functions
│       └── ...
└── src/                              ← Svelte frontend (no changes)
```

---

## 4. The New Crate in Detail

### 4.1 `Cargo.toml` for `bamako-synthesis`

```toml
[package]
name = "bamako-synthesis"
version = "0.1.0"
edition = "2021"

[dependencies]
reqwest = { version = "0.12", features = ["rustls-tls", "json"], default-features = false }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
hex = "0.4"
graph-flow = "0.5"
async-trait = "0.1"
nanoid = "0.4"
tokio = { version = "1", features = ["full"] }
thiserror = "1"

[dev-dependencies]
tokio = { version = "1", features = ["full", "test-util"] }
```

**No `tauri`, `libsql`, or `anyhow` dependencies.** This is the structural guarantee: the crate is framework-agnostic.

---

### 4.2 `error.rs` — Single Error Type

```rust
// crates/bamako-synthesis/src/error.rs

#[derive(Debug, thiserror::Error)]
pub enum SynthesisError {
    #[error("LLM API error: {0}")]
    LlmError(String),

    #[error("JSON parse error: {0} — raw: {1}")]
    ParseError(String, String),

    #[error("Graph pipeline error: {0}")]
    PipelineError(String),

    #[error("No API key configured. Set one in Settings → API Key.")]
    NoApiKey,

    #[error("Page has no content")]
    EmptyContent,
}

pub type Result<T> = std::result::Result<T, SynthesisError>;
```

---

### 4.3 `llm.rs` — Unified OpenRouter Client

Currently `synthesis.rs` has `call_openrouter()` and `graph_synthesis.rs` has `llm_call()` / `do_llm_request()`. These are effectively the same function with minor variations. Unified:

```rust
// crates/bamako-synthesis/src/llm.rs

pub struct LlmRequest<'a> {
    pub api_key: &'a str,
    pub model: &'a str,
    pub system: &'a str,
    pub user: &'a str,
    pub json_mode: bool,
    pub temperature: f32,
}

/// Call OpenRouter and return the response text (code fences stripped).
/// Retries once with stricter "no markdown" instruction on JSON parse failure
/// when json_mode is true.
pub async fn call(req: LlmRequest<'_>) -> Result<String>;

/// Like call() but parses and returns serde_json::Value directly.
pub async fn call_json(req: LlmRequest<'_>) -> Result<serde_json::Value>;

fn strip_code_fences(s: &str) -> String;
fn extract_content(json: &serde_json::Value) -> Option<String>;
```

All LLM calls across graph and synthesis flow through this single `call()` function. The duplicate implementations in both files vanish.

---

### 4.4 `graph/types.rs` — Data Types for the Pipeline

Moved from `graph_synthesis.rs` verbatim:

```rust
pub struct SourceProfile { ... }   // domain, entity_types, relationship_types, themes
pub struct ExtractedNode { ... }   // id, name, entity_type, description, confidence, aliases
pub struct ExtractedEdge { ... }   // from_id, to_id, relationship, description, confidence, inferred
pub struct ResolvedGraph {         // output of the full pipeline
    pub nodes: Vec<ExtractedNode>,
    pub edges: Vec<ExtractedEdge>,
}
```

All derive `Serialize, Deserialize, Clone, Debug` — no Tauri-specific derives needed.

---

### 4.5 `graph/tasks.rs` — The 4-Task Pipeline

Moved from `graph_synthesis.rs` with one change: all direct `reqwest::Client::new()` calls replaced with calls to `crate::llm::call_json()`.

```rust
pub struct SourceProfilerTask   { progress: Arc<dyn Fn(String) + Send + Sync> }
pub struct NodeExtractorTask    { progress: Arc<dyn Fn(String) + Send + Sync> }
pub struct EdgeExtractorTask    { progress: Arc<dyn Fn(String) + Send + Sync> }
pub struct EntityResolverTask   { progress: Arc<dyn Fn(String) + Send + Sync> }

// Each implements graph_flow::Task (no changes to logic)
```

The `#[async_trait]` impl for each task is moved verbatim. The tasks become testable because they no longer need Tauri state.

---

### 4.6 `graph/mod.rs` — Pipeline Entry Point

```rust
// crates/bamako-synthesis/src/graph/mod.rs

pub mod tasks;
pub mod types;
pub use types::*;

pub struct GraphInput<'a> {
    pub title: &'a str,
    pub content: &'a str,
    pub api_key: &'a str,
    pub model: &'a str,
}

/// Run the 4-task graph-flow pipeline.
/// `on_progress` receives human-readable stage strings for UI display.
pub async fn run_graph_synthesis(
    input: &GraphInput<'_>,
    on_progress: impl Fn(String) + Send + Sync + 'static,
) -> Result<ResolvedGraph>;
```

The `run_graph_synthesis` function body is moved from `graph_synthesis.rs` with minimal changes (use `GraphInput` struct instead of individual params).

---

### 4.7 `synthesis/types.rs` — Core Data Types

Moved from `synthesis.rs`:

```rust
pub struct SpaceConfig {
    pub api_key: String,
    pub model: String,
    pub synthesizer_role: String,
}

pub struct PageSummaryInput {
    pub page_id: String,
    pub title: String,
    pub summary: String,
    pub key_points: Vec<String>,
}

pub struct EntityInput {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    pub description: String,
    pub mention_count: i64,
}

pub struct MentionInput {
    pub excerpt: String,
    pub page_title: String,
}

pub struct PageSynthesis { ... }   // page_id, summary, key_points, topics, synthesized_at
pub struct SpaceOverview { ... }   // overview, topics, synthesized_at
pub struct WikiAnswer    { ... }   // answer, sources, confidence
pub struct LintQuestions { ... }   // investigation_questions, suggested_sources
```

Note: `EntitySuggestion`, `PageLink`, `GraphNode`, `GraphEdge`, `GraphData`, `LintResult` stay in the Tauri app since they're DB-query result shapes, not synthesis outputs.

---

### 4.8 `synthesis/mod.rs` — LLM Operations (No DB)

Each function takes pre-loaded data and returns pure output. The Tauri command layer handles all DB I/O before and after.

```rust
// crates/bamako-synthesis/src/synthesis/mod.rs

/// Summarize a single page and extract initial entities.
/// Returns the summary/key_points/topics AND kicks off the graph pipeline.
pub struct SynthesizePageOutput {
    pub summary: String,
    pub key_points: Vec<String>,
    pub topics: Vec<String>,
    pub graph: ResolvedGraph,
}

pub async fn summarize_page(
    title: &str,
    content: &str,           // raw markdown, up to 8000 chars used
    config: &SpaceConfig,
    on_progress: impl Fn(String) + Send + Sync + 'static,
) -> Result<SynthesizePageOutput>;


/// Generate or update a space overview from existing page summaries.
pub async fn generate_space_overview(
    existing: Option<&str>,
    summaries: &[PageSummaryInput],
    config: &SpaceConfig,
) -> Result<SpaceOverview>;


/// Answer a question using compiled knowledge from the space.
pub async fn ask_wiki(
    question: &str,
    summaries: &[PageSummaryInput],
    entities: &[EntityInput],
    config: &SpaceConfig,
) -> Result<WikiAnswer>;


/// Generate LLM-enriched content for a wiki entity page.
pub async fn generate_entity_page(
    entity: &EntityInput,
    mentions: &[MentionInput],
    config: &SpaceConfig,
) -> Result<String>;   // returns markdown content


/// Generate investigation questions + suggested sources from summaries.
pub async fn generate_lint_questions(
    summaries: &[PageSummaryInput],
    orphan_page_names: &[String],
    config: &SpaceConfig,
) -> Result<LintQuestions>;
```

All of these are pure async functions: data in, data out, no I/O side-effects.

---

### 4.9 `wiki/mod.rs` — Wiki Content Helpers

Pure string-manipulation functions with no async or I/O:

```rust
// crates/bamako-synthesis/src/wiki/mod.rs

/// Insert or update the "## Mentioned In" section of a wiki page.
pub fn update_mentioned_in_section(content: &str, src_title: &str) -> String;

/// Build a rich markdown stub for a newly promoted entity.
pub fn build_wiki_stub(
    name: &str,
    entity_type: &str,
    description: &str,
    mention_count: i64,
    source_pages: &[String],
    related_entities: &[(String, String)],  // (name, relationship)
) -> String;

/// SHA-256 hash of content string (hex-encoded), for change detection.
pub fn content_hash(content: &str) -> String;
```

These are currently embedded inline in `synthesis.rs`. Making them public + tested is important because `build_wiki_stub` and `update_mentioned_in_section` affect user-visible wiki content.

---

### 4.10 `vector/mod.rs` — Embedding Functions

```rust
// crates/bamako-synthesis/src/vector/mod.rs

pub const DIMS: usize = 64;

/// 64-dim bag-of-words embedding, L2-normalized.
pub fn embed(text: &str) -> [f32; DIMS];

/// Cosine similarity between two embeddings.
pub fn cosine(a: &[f32; DIMS], b: &[f32; DIMS]) -> f32;

/// Serialize embedding to JSON string for SQLite storage.
pub fn to_json(v: &[f32; DIMS]) -> String;

/// Deserialize embedding from SQLite JSON string.
pub fn from_json(s: &str) -> Option<[f32; DIMS]>;

/// Extract a ~160-char snippet from text centered around query terms.
pub fn make_snippet(text: &str, query: &str) -> String;
```

---

### 4.11 `vector/veles.rs` — VelesDB HTTP Client

```rust
// crates/bamako-synthesis/src/vector/veles.rs

pub struct VelesClient {
    client: reqwest::Client,
    base_url: String,         // defaults to "http://localhost:9000"
}

impl VelesClient {
    pub fn new(base_url: &str) -> Self;

    pub async fn ensure_collection(&self, name: &str, dims: usize) -> bool;

    pub async fn upsert(
        &self,
        collection: &str,
        version_id: &str,
        page_id: &str,
        title: &str,
        vector: &[f32; DIMS],
    ) -> bool;

    pub async fn search(
        &self,
        collection: &str,
        query_vec: &[f32; DIMS],
        top_k: usize,
    ) -> Option<Vec<VelesHit>>;
}

pub struct VelesHit {
    pub score: f32,
    pub version_id: String,
    pub page_id: String,
    pub title: String,
}
```

The Tauri `vector.rs` command file becomes a thin wrapper: get the VelesClient from the crate, do the DB I/O around it.

---

### 4.12 `lib.rs` — Public API Surface

```rust
// crates/bamako-synthesis/src/lib.rs

pub mod error;
pub mod graph;
pub mod llm;
pub mod synthesis;
pub mod vector;
pub mod wiki;

// Flatten the most-used types to crate root
pub use error::{Result, SynthesisError};
pub use graph::{run_graph_synthesis, GraphInput, ResolvedGraph, ExtractedNode, ExtractedEdge};
pub use synthesis::types::{
    SpaceConfig, PageSyntharyInput, EntityInput, MentionInput,
    PageSynthesis, SpaceOverview, WikiAnswer, LintQuestions,
    SynthesizePageOutput,
};
pub use synthesis::{
    summarize_page, generate_space_overview, ask_wiki,
    generate_entity_page, generate_lint_questions,
};
pub use wiki::{update_mentioned_in_section, build_wiki_stub, content_hash};
pub use vector::{embed, cosine, make_snippet, DIMS};
pub use vector::veles::VelesClient;
```

---

## 5. What the Tauri App Becomes

### 5.1 `synthesis.rs` after refactor

The file is split into two concerns:
- **Read phase**: load config + content from DB (same as today but cleaner)
- **Call crate**: `bamako_synthesis::summarize_page()`
- **Write phase**: persist results to DB and emit Tauri events

Example — `synthesize_page` after refactor:

```rust
// src-tauri/src/commands/synthesis.rs

#[tauri::command]
pub async fn synthesize_page(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
) -> Result<PageSynthesis, String> {
    // ── 1. Read phase ────────────────────────────────────────────────────────
    let (content, title, hash, cfg) = {
        let db = get_or_open_space_db(&state, &space_id).await?;
        let conn = db.connect().map_err(|e| e.to_string())?;
        ensure_synthesis_tables(&conn).await?;
        let cfg = load_space_config(&conn).await?;
        // ... read content, check hash cache ...
        (content, title, hash, cfg)
        // conn dropped here
    };

    // ── 2. Synthesis (no DB connection held) ─────────────────────────────────
    let page_id_clone = page_id.clone();
    let app_clone = app.clone();

    let output = bamako_synthesis::summarize_page(
        &title, &content,
        &bamako_synthesis::SpaceConfig {
            api_key: cfg.api_key,
            model: cfg.model,
            synthesizer_role: cfg.synthesizer_role,
        },
        move |msg| {
            app_clone.emit("synthesis:stage", serde_json::json!({
                "page_id": page_id_clone,
                "stage": "progress",
                "label": msg,
            })).ok();
        },
    ).await.map_err(|e| e.to_string())?;

    // ── 3. Write phase ────────────────────────────────────────────────────────
    let db = get_or_open_space_db(&state, &space_id).await?;
    let conn = db.connect().map_err(|e| e.to_string())?;
    // persist summary, graph nodes, edges, wiki stubs...
    // (same SQL as today, just separated from the LLM call)
    ...
}
```

The Tauri `synthesis.rs` shrinks from ~1600 LOC to ~600-700 LOC. The remaining code is entirely DB I/O and Tauri event plumbing.

### 5.2 `graph_synthesis.rs` after refactor

This file **is deleted entirely**. Its content moves to `crates/bamako-synthesis/src/graph/`. The only reference in the Tauri app was:

```rust
// in synthesis.rs:
let resolved = crate::commands::graph_synthesis::run_graph_synthesis(...).await;
// becomes:
let resolved = bamako_synthesis::run_graph_synthesis(&GraphInput { ... }, on_progress).await;
```

### 5.3 `vector.rs` after refactor

The file shrinks to ~150 LOC. All math and HTTP client code moves to the crate:

```rust
#[tauri::command]
pub async fn vectorize_page(...) -> Result<(), String> {
    // DB read (same as today)
    let text = ...;

    // Embedding (from crate, not local function)
    let embedding = bamako_synthesis::embed(&text);

    // DB write (same as today)
    conn.execute("INSERT OR REPLACE INTO page_embeddings ...", ...)?;

    // VelesDB (from crate client)
    let veles = bamako_synthesis::VelesClient::new("http://localhost:9000");
    if veles.ensure_collection(&collection, bamako_synthesis::DIMS).await {
        veles.upsert(&collection, &version_id, &page_id, title_str, &embedding).await;
    }
    Ok(())
}
```

---

## 6. Workspace Setup

### 6.1 New root `Cargo.toml`

```toml
[workspace]
members = [
    "src-tauri",
    "crates/bamako-synthesis",
]
resolver = "2"
```

Place at `bamako/Cargo.toml` (project root, alongside `src/` and `src-tauri/`).

### 6.2 Updated `src-tauri/Cargo.toml`

```toml
[dependencies]
# ...existing deps...
bamako-synthesis = { path = "../crates/bamako-synthesis" }

# Remove from src-tauri (now owned by bamako-synthesis):
# graph-flow = "0.5"     ← remove
# sha2 = "0.10"          ← remove (unless used elsewhere)
# hex = "0.4"            ← remove (unless used elsewhere)
# async-trait = "0.1"    ← remove
```

### 6.3 Tauri build still works

Tauri reads `src-tauri/Cargo.toml` and `tauri.conf.json` — neither changes path. The workspace `Cargo.toml` at the root doesn't affect Tauri's build process because Tauri's build script operates from `src-tauri/`.

---

## 7. Migration Sequence

Execute in this order to avoid broken intermediate states:

### Step 1 — Create workspace skeleton

```bash
# Create crate directory
mkdir -p crates/bamako-synthesis/src/{graph,synthesis,vector,wiki}

# Create root workspace Cargo.toml (content above)
touch Cargo.toml

# Create crate Cargo.toml (content above)
touch crates/bamako-synthesis/Cargo.toml

# Stub lib.rs so it compiles
echo 'pub mod error;' > crates/bamako-synthesis/src/lib.rs
touch crates/bamako-synthesis/src/error.rs
```

Verify: `cargo check --workspace` passes (empty crate is valid).

### Step 2 — Move `error.rs`

Create `crates/bamako-synthesis/src/error.rs` with the `SynthesisError` enum above. This has no dependencies, compiles immediately.

### Step 3 — Move `llm.rs`

Create `crates/bamako-synthesis/src/llm.rs`. Extract from both `synthesis.rs` and `graph_synthesis.rs`:
- `do_llm_request` → `llm::do_request` (private)
- `call_openrouter` → `llm::call` (public, accepts `LlmRequest`)
- `strip_code_fences` → `llm::strip_code_fences` (private)
- `call_json` → new public wrapper that calls `call()` and parses JSON

Verify the new `llm.rs` compiles standalone.

### Step 4 — Move `graph/`

Create `crates/bamako-synthesis/src/graph/types.rs` (move types from `graph_synthesis.rs`).

Create `crates/bamako-synthesis/src/graph/tasks.rs` — move the 4 Task structs from `graph_synthesis.rs`, updating all `reqwest::Client::new()` + HTTP calls to use `crate::llm::call_json()`.

Create `crates/bamako-synthesis/src/graph/mod.rs` — move `run_graph_synthesis()`, changing signature to accept `&GraphInput`.

At this point: `cargo check --workspace` passes, `graph_synthesis.rs` in Tauri still exists unchanged. The crate is not yet used.

### Step 5 — Move `wiki/`

Create `crates/bamako-synthesis/src/wiki/mod.rs`. Move:
- `update_mentioned_in_section()` from `synthesis.rs:125-139`
- `content_hash()` from `synthesis.rs:155-160`
- Extract `build_wiki_stub()` — new function that encapsulates the stub markdown construction currently inline in `create_wiki_stubs` and `synthesize_page`

Write unit tests for all three. These are pure string functions — tests are trivial and high-value.

### Step 6 — Move `vector/`

Create `crates/bamako-synthesis/src/vector/veles.rs` — extract `VelesClient` from `vector.rs`.

Create `crates/bamako-synthesis/src/vector/mod.rs` — move `embed()`, `cosine()`, `make_snippet()`, `to_json()`, `from_json()`, `str_to_id()`.

Write unit tests for `embed()` + `cosine()` (known input → known output).

### Step 7 — Move `synthesis/`

Create `crates/bamako-synthesis/src/synthesis/types.rs` — move the pure data types from `synthesis.rs`.

Create `crates/bamako-synthesis/src/synthesis/mod.rs` — implement the public functions (`summarize_page`, `generate_space_overview`, `ask_wiki`, `generate_entity_page`, `generate_lint_questions`). Each function takes pre-loaded data and calls `crate::llm::call_json()`.

This is the most significant step — it requires carefully separating the LLM logic from the DB I/O in `synthesis.rs`.

### Step 8 — Wire crate into Tauri app

Add `bamako-synthesis = { path = "../crates/bamako-synthesis" }` to `src-tauri/Cargo.toml`.

Update `src-tauri/src/commands/synthesis.rs`:
- Replace inline LLM calls with calls to `bamako_synthesis::summarize_page()` etc.
- Replace `crate::commands::graph_synthesis::run_graph_synthesis()` with `bamako_synthesis::run_graph_synthesis()`
- Replace `update_mentioned_in_section()` with `bamako_synthesis::update_mentioned_in_section()`
- Replace `content_hash()` with `bamako_synthesis::content_hash()`

Update `src-tauri/src/commands/vector.rs`:
- Replace `embed()` with `bamako_synthesis::embed()`
- Replace VelesDB helpers with `bamako_synthesis::VelesClient`
- Remove all the inlined math and HTTP code

### Step 9 — Delete dead code

```bash
# graph_synthesis.rs is now empty of substance
rm src-tauri/src/commands/graph_synthesis.rs

# Remove from mod.rs:
# pub mod graph_synthesis;

# Remove from Cargo.toml:
# graph-flow = "0.5"
# async-trait = "0.1"
# sha2 = "0.10" (if not used elsewhere)
# hex = "0.4" (if not used elsewhere)
```

Verify: `cargo build --workspace` passes. `cargo test --workspace` passes.

### Step 10 — Add tests to bamako-synthesis

Now that the crate is wired in and stable, add integration tests:

```
crates/bamako-synthesis/
└── tests/
    ├── wiki_helpers_test.rs      # update_mentioned_in_section, build_wiki_stub, content_hash
    ├── vector_test.rs            # embed, cosine, make_snippet
    └── llm_mock_test.rs          # if a mock HTTP server (wiremock) is added later
```

---

## 8. What Does NOT Move

These stay in the Tauri app because they are inherently Tauri/DB concerns:

| What | Why it stays |
|------|-------------|
| `ensure_synthesis_tables()` | SQLite schema migration — DB concern |
| `load_space_config()` | Reads from `space_config` SQLite table |
| `ensure_wiki_root()` | Creates pages in SQLite |
| `create_wiki_stubs` command | DB read + `build_wiki_stub()` + DB write |
| `EntitySuggestion`, `PageLink`, `GraphNode`, `GraphEdge`, `GraphData`, `LintResult` | DB query result shapes, not synthesis outputs |
| All `get_*` commands (`get_page_synthesis`, `get_space_overview`, etc.) | Pure DB reads |
| `clear_synthesis_data`, `force_resynthesize`, `demote_entity_page` | Pure DB mutations |
| Event emission (`app.emit("synthesis:stage", ...)`) | Tauri-specific |
| `state: State<'_, AppState>` | Tauri-specific |

---

## 9. Interface Stability Contract

The `bamako-synthesis` crate's public API is stable once Step 8 is complete. Changes to how results are persisted in SQLite, how wiki pages are structured, or how Tauri events are emitted do NOT require changing the crate — only `synthesis.rs` in the Tauri app changes.

Changes that DO require crate changes:
- Changing the LLM prompt structure
- Adding new synthesis pipeline stages
- Changing the graph-flow task chain
- Switching from OpenRouter to another LLM provider
- Changing the vector embedding algorithm

This is a clean boundary: LLM logic is the crate's concern, persistence + UI is the Tauri app's concern.

---

## 10. Future Extensions Enabled by This Split

### 10.1 CLI tool for batch synthesis

```bash
cargo run -p bamako-cli -- synthesize --space ~/my-space --all
```

A `bamako-cli` crate could import `bamako-synthesis` and run synthesis from the command line without a Tauri UI — useful for bulk imports, CI pipelines, or server-side processing.

### 10.2 Server-side synthesis service

A future `bamako-server` binary could expose synthesis as an HTTP API, using `bamako-synthesis` directly. The crate has no `tauri` dependency so it can run in a plain `tokio` environment.

### 10.3 Swappable LLM providers

The `llm.rs` module can be extended with a `Provider` trait:
```rust
pub trait LlmProvider: Send + Sync {
    async fn call(&self, req: LlmRequest<'_>) -> Result<String>;
}
```
Today: `OpenRouterProvider`. Tomorrow: `AnthropicProvider`, `OllamaProvider`, mock for tests.

### 10.4 True unit testing

All synthesis logic is now testable without a SQLite database or Tauri runtime. With a mock LLM provider:
```rust
#[tokio::test]
async fn test_summarize_page_extracts_topics() {
    let mock = MockLlmProvider::with_response(r#"{"summary":"...","topics":["ai","rust"],...}"#);
    let output = summarize_page("My Doc", "content...", &config_with(mock), |_| {}).await.unwrap();
    assert_eq!(output.topics, vec!["ai", "rust"]);
}
```

---

## 11. Estimated Effort

| Step | Effort | Risk |
|------|--------|------|
| Workspace setup | 30 min | Low |
| `error.rs` + `llm.rs` | 2 hr | Low — deduplicate existing code |
| `graph/` move | 2 hr | Low — mostly mechanical |
| `wiki/` move + tests | 1 hr | Low |
| `vector/` move + tests | 2 hr | Low |
| `synthesis/` separation | 4-6 hr | Medium — DB/LLM separation requires care |
| Wire into Tauri app | 3 hr | Medium — verify all commands still work |
| Delete dead code + verify build | 1 hr | Low |
| **Total** | **~16-20 hr** | |

Biggest risk: Step 7 (synthesis separation) — the current code has DB reads, LLM calls, and DB writes deeply interleaved. The "read phase / LLM phase / write phase" pattern is already partially established (the locked-DB fix from this session), which reduces the risk considerably.

---

## 12. Self-Contained Deployment

The crate must work as a **standalone dependency** in any Rust project — Tauri, Axum, CLI, or otherwise.

### 12.1 Repository / Distribution

The crate lives in `crates/bamako-synthesis/` within the Bamako monorepo but is structured to be extracted to its own repo at any time:

- No workspace-level path deps
- No internal `crate::` references to other Bamako modules
- All types, errors, and config are defined within the crate itself
- Can be added as a `git` dep today, published to crates.io later

```toml
# Adding to any other project — git dependency
[dependencies]
bamako-synthesis = { git = "https://github.com/your-org/bamako-synthesis", branch = "main" }

# Or local path dep while developing
bamako-synthesis = { path = "../bamako-synthesis" }
```

---

### 12.2 Config File Format

The crate reads no config files itself. All configuration is passed in via the `SpaceConfig` struct. The **host app** owns loading config from wherever (TOML, env vars, database, etc.).

#### `bamako-synthesis.toml` (reference format for host apps)

```toml
# Recommended config file format for host apps integrating bamako-synthesis.
# The host app reads this and constructs SpaceConfig — the crate never reads this file.

[llm]
# OpenRouter API key (required)
api_key = "sk-or-v1-..."

# Model to use for all synthesis operations
# Recommended: "minimax/minimax-m2.5" (fast + cheap), "anthropic/claude-sonnet-4-5" (quality)
model = "minimax/minimax-m2.5"

# Role context injected into synthesis prompts
# "owner" = first-person knowledge base, "curator" = third-person editorial
synthesizer_role = "owner"

[vector]
# VelesDB URL (optional — omit to use SQLite-only fallback)
veles_url = "http://localhost:9000"

# Embedding dimensions (do not change unless you clear all stored embeddings)
dims = 64
```

#### Environment variable alternative

```bash
BAMAKO_API_KEY=sk-or-v1-...
BAMAKO_MODEL=minimax/minimax-m2.5
BAMAKO_SYNTHESIZER_ROLE=owner
BAMAKO_VELES_URL=http://localhost:9000
```

#### Config loading helper (ships in crate as optional)

```rust
// In bamako-synthesis, feature = "config-loader"
impl SpaceConfig {
    /// Load from environment variables with BAMAKO_ prefix.
    pub fn from_env() -> Result<Self>;

    /// Load from a TOML file at the given path.
    pub fn from_file(path: &std::path::Path) -> Result<Self>;
}
```

The config loader is behind a `config-loader` feature flag so projects that load config their own way don't pull in `toml`/`figment` deps.

---

### 12.3 Feature Flags

```toml
# crates/bamako-synthesis/Cargo.toml

[features]
default = ["veles"]

# VelesDB HTTP client — disable if you only want SQLite embedding fallback
veles = ["dep:reqwest"]

# Config loading from env/TOML
config-loader = ["dep:toml", "dep:figment"]

# Expose mock LLM provider for testing in host apps
test-utils = []
```

A minimal integration (no VelesDB, no HTTP) compiles with:
```toml
bamako-synthesis = { path = "...", default-features = false }
```

---

### 12.4 Deployment API Reference

Create `crates/bamako-synthesis/DEPLOYMENT.md`:

```markdown
# bamako-synthesis — Integration Guide

## What this crate provides

LLM-driven knowledge synthesis: page summarization, entity extraction,
knowledge graph construction, wiki stub generation, vector search, Q&A.

No database, no framework, no Tauri — pure async Rust.

## Minimum integration (5 lines)

\```rust
use bamako_synthesis::{SpaceConfig, summarize_page};

let config = SpaceConfig {
    api_key: "sk-or-v1-...".into(),
    model: "minimax/minimax-m2.5".into(),
    synthesizer_role: "owner".into(),
};

let output = summarize_page("My Doc", content, &config, |msg| println!("{msg}")).await?;
println!("Summary: {}", output.summary);
println!("Entities: {}", output.graph.nodes.len());
\```

## What you provide

The crate takes **pre-loaded text** and returns **structured output**.
You are responsible for:

| Concern | Your code |
|---------|-----------|
| Reading documents from storage | Your DB/filesystem |
| Persisting summaries | Your DB (`output.summary`, `output.key_points`, etc.) |
| Persisting entities | Your DB (`output.graph.nodes`) |
| Change-detection / caching | Use `content_hash(text)` before calling |
| Emitting progress events | Pass a closure to `on_progress` |
| Vector storage | Write `embed(text)` result to your DB |

## Full example: integrate with a Postgres-backed app

See `examples/postgres_integration.rs`.

## Error handling

All fallible functions return `Result<T, SynthesisError>`.
`SynthesisError` is non-exhaustive — match on the variants you care about,
use a wildcard for the rest.

\```rust
match synthesize_page(...).await {
    Ok(output) => persist(output),
    Err(SynthesisError::NoApiKey) => prompt_user_for_key(),
    Err(SynthesisError::LlmError(msg)) => log::error!("LLM: {msg}"),
    Err(e) => return Err(e.into()),
}
\```

## LLM provider

All calls go to **OpenRouter** (`https://openrouter.ai/api/v1`).
Set `SpaceConfig.model` to any OpenRouter-supported model ID.
The provider abstraction (`LlmProvider` trait) is planned for v0.2.

## Vector search

Embedding is a 64-dim bag-of-words (no neural model, runs offline).
For richer semantic search, substitute your own embeddings — the crate's
`embed()` function is a sensible default, not a requirement.

VelesDB integration is optional (feature = "veles").
If VelesDB is unavailable, implement fallback search using stored embeddings
and `cosine_similarity()`.
```

---

### 12.5 `examples/` Directory

```
crates/bamako-synthesis/
└── examples/
    ├── basic_summarize.rs       # minimal: summarize one page, print output
    ├── full_pipeline.rs         # summarize → build wiki stubs → ask question
    ├── vector_search.rs         # embed pages, rank by query
    └── postgres_integration.rs  # shows read-from-pg → synthesize → write-back-to-pg pattern
```

`basic_summarize.rs`:
```rust
//! Run with: BAMAKO_API_KEY=... cargo run --example basic_summarize
use bamako_synthesis::{SpaceConfig, summarize_page};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = SpaceConfig {
        api_key: std::env::var("BAMAKO_API_KEY")?,
        model: std::env::var("BAMAKO_MODEL")
            .unwrap_or_else(|_| "minimax/minimax-m2.5".into()),
        synthesizer_role: "owner".into(),
    };

    let content = std::fs::read_to_string(std::env::args().nth(1).unwrap_or("doc.md".into()))?;
    let output = summarize_page("My Document", &content, &config, |msg| {
        eprintln!("[progress] {msg}");
    }).await?;

    println!("Summary: {}", output.summary);
    println!("Topics: {}", output.topics.join(", "));
    println!("Entities: {}", output.graph.nodes.len());
    for node in &output.graph.nodes {
        println!("  - {} ({}): {}", node.name, node.entity_type, node.description);
    }
    Ok(())
}
```

---

### 12.6 Crate `README.md`

```markdown
# bamako-synthesis

LLM-driven knowledge synthesis for Rust applications.

- **Page summarization** — summary, key points, topic tags
- **Entity extraction** — 4-stage graph-flow pipeline (profile → extract → relate → resolve)
- **Knowledge graph** — entities, relationships, confidence scores
- **Wiki stub generation** — markdown pages from entity data
- **Space overview** — rolling summary of all pages in a knowledge base
- **Wiki Q&A** — question answering grounded in compiled summaries
- **Space linting** — orphan detection, staleness, gap analysis
- **Vector search** — 64-dim embedding, VelesDB or SQLite fallback

## Install

\```toml
[dependencies]
bamako-synthesis = "0.1"
\```

## Quick start

\```rust
use bamako_synthesis::{SpaceConfig, summarize_page};

let output = summarize_page(title, content, &config, |msg| println!("{msg}")).await?;
\```

See [DEPLOYMENT.md](./DEPLOYMENT.md) for full integration guide.

## License

MIT OR Apache-2.0
```

---

### 12.7 `src-tauri` as Reference Integration

The Bamako Tauri app (`src-tauri/src/commands/synthesis.rs`) serves as the **reference implementation** showing how to wire `bamako-synthesis` into a real app with a database. Any host app should use it as a template for:

- How to do the read/synthesize/write three-phase pattern
- How to pass `on_progress` closures that emit events to a UI
- How to handle change-detection with `content_hash()`
- How to build wiki stubs from `ResolvedGraph` output

---

## 13. File-by-file Change Summary

| File | Action | Resulting LOC |
|------|--------|---------------|
| `src-tauri/src/commands/synthesis.rs` | Thin to DB + event orchestration | ~650 |
| `src-tauri/src/commands/graph_synthesis.rs` | **Delete** | 0 |
| `src-tauri/src/commands/vector.rs` | Thin to DB + VelesClient wrapper | ~150 |
| `src-tauri/src/commands/mod.rs` | Remove `pub mod graph_synthesis` | ~7 |
| `src-tauri/Cargo.toml` | Add path dep, remove 4 crate deps | — |
| `Cargo.toml` (new) | Workspace root | ~6 |
| `crates/bamako-synthesis/src/lib.rs` | Public re-exports | ~30 |
| `crates/bamako-synthesis/src/error.rs` | Error type | ~20 |
| `crates/bamako-synthesis/src/llm.rs` | Unified OpenRouter client | ~120 |
| `crates/bamako-synthesis/src/graph/types.rs` | Data types | ~60 |
| `crates/bamako-synthesis/src/graph/tasks.rs` | 4 Task impls | ~480 |
| `crates/bamako-synthesis/src/graph/mod.rs` | Pipeline entry point | ~80 |
| `crates/bamako-synthesis/src/synthesis/types.rs` | Data types | ~60 |
| `crates/bamako-synthesis/src/synthesis/mod.rs` | Pure LLM functions | ~350 |
| `crates/bamako-synthesis/src/wiki/mod.rs` | String helpers | ~80 |
| `crates/bamako-synthesis/src/vector/mod.rs` | Embedding math | ~120 |
| `crates/bamako-synthesis/src/vector/veles.rs` | VelesDB HTTP | ~120 |

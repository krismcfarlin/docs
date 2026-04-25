/// Test 2 — Vector Search (cosine similarity)
///
/// Verifies that:
///   1. sqld accepts a table with an F32_BLOB(4) embedding column.
///   2. A cosine-metric vector index can be created.
///   3. vector_top_k with cosine similarity returns the closest document.
///
/// Cosine similarity measures the angle between vectors, ignoring magnitude.
/// Vectors should be unit-normalised so cosine distance = 1 - cosine_similarity.
/// We use 4-dimensional unit vectors — no real embedding model needed.
/// The test documents are cat-vs-dog themed; the query is closest to the cat
/// documents, so doc 1 or 3 must rank above doc 2.
///
/// Prerequisites:
///   sqld must be running on http://127.0.0.1:8093
///   Start it with:  docker compose up -d

use bamako_lib::db;
use uuid::Uuid;

#[tokio::test]
async fn test_vector_similarity_search() {
    let run_id = Uuid::new_v4().to_string().replace('-', "");
    let table = format!("vec_test_{}", &run_id[..8]);
    let index = format!("{table}_idx");

    // ── 1. connect to sqld ───────────────────────────────────────────────────
    let db = db::open_remote()
        .await
        .expect("❌  sqld not reachable at http://127.0.0.1:8093 — run: docker compose up -d");
    let conn = db.connect().unwrap();

    // ── 2. create vector table ───────────────────────────────────────────────
    conn.execute(
        &format!(
            "CREATE TABLE IF NOT EXISTS {table} \
             (id INTEGER PRIMARY KEY, label TEXT NOT NULL, embedding F32_BLOB(4))"
        ),
        (),
    )
    .await
    .expect("create vector table");

    // 'metric=cosine' → index uses cosine distance (1 - cosine_similarity).
    // Nearest neighbours have the smallest cosine distance.
    conn.execute(
        &format!(
            "CREATE INDEX IF NOT EXISTS {index} \
             ON {table} (libsql_vector_idx(embedding, 'metric=cosine'))"
        ),
        (),
    )
    .await
    .expect("create vector index with cosine metric");

    // ── 3. insert documents with unit-normalised toy embeddings ─────────────
    //
    //  Cosine similarity is angle-based so magnitude must be 1.
    //  Each vector below has been normalised to ||v|| = 1.0.
    //
    //  dim 0 → "cat-ness"   dim 1 → "dog-ness"
    //  dim 2 → "outdoor"    dim 3 → "softness"
    //
    //  doc 1 — cat on mat:       [0.9623, 0.0962, 0.0481, 0.2406]  ||v||≈1
    //  doc 2 — loyal dog:        [0.0985, 0.9853, 0.1477, 0.0246]  ||v||≈1
    //  doc 3 — feline on rug:    [0.7217, 0.1203, 0.0401, 0.6815]  ||v||≈1
    let docs = [
        (1i64, "the cat sat on the mat",       "[0.9623, 0.0962, 0.0481, 0.2406]"),
        (2i64, "dogs are loyal companions",    "[0.0985, 0.9853, 0.1477, 0.0246]"),
        (3i64, "the feline rested on the rug", "[0.7217, 0.1203, 0.0401, 0.6815]"),
    ];

    for (id, label, vec) in &docs {
        conn.execute(
            &format!(
                "INSERT OR IGNORE INTO {table} (id, label, embedding) \
                 VALUES ({id}, '{label}', vector('{vec}'))"
            ),
            (),
        )
        .await
        .unwrap_or_else(|e| panic!("insert doc {id}: {e}"));
    }

    // ── 4. similarity query ──────────────────────────────────────────────────
    //
    //  query ~ "cats resting indoors": unit-normalised toward cat + softness
    //  [0.88, 0.05, 0.02, 0.47] normalised → ||v||≈1
    //  expected top results: doc 1 and/or doc 3 (both cat-themed)
    let query_vec = "[0.8789, 0.0499, 0.0200, 0.4690]";

    // vector_top_k returns (id) — join back to the table for label.
    // The TVF orders results by distance ascending (nearest first).
    let mut rows = conn
        .query(
            &format!(
                "SELECT {table}.id, {table}.label \
                 FROM vector_top_k('{index}', vector('{query_vec}'), 3) AS k \
                 JOIN {table} ON {table}.id = k.id"
            ),
            (),
        )
        .await
        .expect("vector_top_k query failed");

    let first = rows
        .next()
        .await
        .expect("row iteration failed")
        .expect("❌  no results returned from vector_top_k");

    let top_id: i64 = first.get(0).unwrap();
    let top_label: String = first.get(1).unwrap();

    println!("✅  vector search top result: id={top_id} label='{top_label}'");

    assert!(
        top_id == 1 || top_id == 3,
        "❌  expected a cat doc (id 1 or 3) as top result, got id={top_id} ('{top_label}')"
    );

    // ── 5. cleanup ───────────────────────────────────────────────────────────
    conn.execute(&format!("DROP INDEX IF EXISTS {index}"), ()).await.ok();
    conn.execute(&format!("DROP TABLE IF EXISTS {table}"), ()).await.ok();
}

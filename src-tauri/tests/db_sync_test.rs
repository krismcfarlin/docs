/// Test 1 — DB Sync
///
/// Verifies that:
///   1. A local embedded replica can be created.
///   2. A row written locally is visible after sync to sqld.
///   3. A second client connecting directly to sqld can read that row.
///
/// Prerequisites:
///   sqld must be running on http://127.0.0.1:8093
///   Start it with:  docker compose up -d
///   (or)            cargo install sqld && sqld

use bamako_lib::db;
use uuid::Uuid;

fn temp_db_path(name: &str) -> String {
    format!("/tmp/bamako_test_{}.db", name)
}

#[tokio::test]
async fn test_local_write_syncs_to_remote() {
    let run_id = Uuid::new_v4().to_string().replace('-', "");
    let local_path = temp_db_path(&run_id);
    let table = format!("sync_test_{}", &run_id[..8]);

    // ── 1. open local embedded replica ───────────────────────────────────────
    let local_db = db::open_replica(&local_path)
        .await
        .expect("❌  sqld not reachable at http://127.0.0.1:8093 — run: docker compose up -d");

    let local = local_db.connect().unwrap();

    // ── 2. create table & write a row locally ────────────────────────────────
    local
        .execute(
            &format!(
                "CREATE TABLE IF NOT EXISTS {table} \
                 (id INTEGER PRIMARY KEY AUTOINCREMENT, value TEXT NOT NULL)"
            ),
            (),
        )
        .await
        .expect("create table");

    local
        .execute(
            &format!("INSERT INTO {table} (value) VALUES (?1)"),
            libsql::params!["hello_from_local"],
        )
        .await
        .expect("local insert");

    // ── 3. push local changes to sqld ────────────────────────────────────────
    local_db.sync().await.expect("sync to sqld failed");

    // ── 4. read back directly from sqld (no local cache) ────────────────────
    let remote_db = db::open_remote()
        .await
        .expect("remote open failed");
    let remote = remote_db.connect().unwrap();

    let mut rows = remote
        .query(
            &format!("SELECT value FROM {table} WHERE value = ?1"),
            libsql::params!["hello_from_local"],
        )
        .await
        .expect("remote query failed");

    let row = rows
        .next()
        .await
        .expect("row iteration failed")
        .expect("❌  no row found on remote — sync may have not pushed");

    let value: String = row.get(0).unwrap();
    assert_eq!(value, "hello_from_local", "round-trip value mismatch");

    // ── 5. cleanup ───────────────────────────────────────────────────────────
    remote
        .execute(&format!("DROP TABLE IF EXISTS {table}"), ())
        .await
        .ok();
    let _ = std::fs::remove_file(&local_path);

    println!("✅  sync test passed — value '{value}' round-tripped local → sqld");
}

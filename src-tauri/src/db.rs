use anyhow::Result;
use libsql::Database;

pub const SQLD_URL: &str = "http://127.0.0.1:8093";
pub const SQLD_TOKEN: &str = ""; // no auth for local sqld

/// Embedded replica: writes locally, syncs to sqld.
pub async fn open_replica(path: &str) -> Result<Database> {
    let db = libsql::Builder::new_remote_replica(path, SQLD_URL.to_string(), SQLD_TOKEN.to_string())
        .build()
        .await?;
    Ok(db)
}

/// Direct connection to sqld (no local file).
pub async fn open_remote() -> Result<Database> {
    let db = libsql::Builder::new_remote(SQLD_URL.to_string(), SQLD_TOKEN.to_string())
        .build()
        .await?;
    Ok(db)
}

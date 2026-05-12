use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Space {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub parent_space_id: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    /// "local" or "remote"
    pub source: String,
    /// sqld namespace name (remote spaces only)
    pub namespace: Option<String>,
    /// sqld server base URL — NO token stored here for security
    pub server_url: Option<String>,
    /// "owner" | "write" | "read"
    pub permission_level: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Page {
    pub id: String,
    pub title: String,
    pub space_id: String,
    pub creator_id: String,
    pub parent_page_id: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
    /// "local" or a remote server URL
    pub source: String,
    /// The ID of this page on the remote server, if synced
    pub remote_id: Option<String>,
    /// "owner" | "write" | "read"
    pub permission_level: String,
    /// When this page was last pulled from remote
    pub last_synced_at: Option<String>,
    /// 1 = entity wiki page, 0 = normal page
    pub is_entity_page: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PageVersion {
    pub id: String,
    pub page_id: String,
    pub owner_id: String,
    pub based_on_version_id: Option<String>,
    pub title: Option<String>,
    pub content: Option<String>,
    pub text_content: Option<String>,
    pub is_published: bool,
    pub is_frozen: bool,
    pub version_num: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RecentPage {
    pub id: String,
    pub title: String,
    pub space_id: String,
    pub space_name: String,
    pub last_accessed_at: String,
    pub source: String,
    pub permission_level: String,
}

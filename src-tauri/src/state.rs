use libsql::Database;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub const DEMO_USER_ID: &str = "user_demo_001";
pub const DEMO_USER_NAME: &str = "Demo User";
pub const DEMO_USER_EMAIL: &str = "demo@bamako.local";

pub struct AppState {
    pub registry: Arc<Mutex<Option<Database>>>,
    pub space_dbs: Arc<Mutex<HashMap<String, Arc<Database>>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(Mutex::new(None)),
            space_dbs: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

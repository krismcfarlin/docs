use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceProfile {
    pub entity_types: Vec<String>,
    pub relationship_types: Vec<String>,
    pub themes: Vec<String>,
    pub domain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedNode {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    pub description: String,
    pub confidence: f32,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEdge {
    pub from_id: String,
    pub to_id: String,
    pub relationship: String,
    pub description: String,
    pub confidence: f32,
    pub inferred: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedGraph {
    pub nodes: Vec<ExtractedNode>,
    pub edges: Vec<ExtractedEdge>,
}

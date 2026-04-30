use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceConfig {
    pub api_key: String,
    pub model: String,
    pub synthesizer_role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageSummaryInput {
    pub page_id: String,
    pub title: String,
    pub summary: String,
    pub key_points: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityInput {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    pub description: String,
    pub mention_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MentionInput {
    pub excerpt: String,
    pub page_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesizePageOutput {
    pub summary: String,
    pub key_points: Vec<String>,
    pub topics: Vec<String>,
    pub graph: crate::graph::ResolvedGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceOverview {
    pub overview: String,
    pub topics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiAnswer {
    pub answer: String,
    pub sources: Vec<String>,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintQuestions {
    pub investigation_questions: Vec<String>,
    pub suggested_sources: Vec<String>,
}

use async_trait::async_trait;
use graph_flow::{Context, NextAction, Task, TaskResult};
use std::sync::Arc;

use crate::prompts;
use super::types::{ExtractedEdge, ExtractedNode, SourceProfile};

const KEY_TITLE:          &str = "title";
const KEY_CONTENT:        &str = "content";
const KEY_API_KEY:        &str = "api_key";
const KEY_MODEL:          &str = "model";
const KEY_PROFILE:        &str = "source_profile";
const KEY_NODES:          &str = "extracted_nodes";
const KEY_EDGES:          &str = "extracted_edges";
pub const KEY_RESOLVED_NODES: &str = "resolved_nodes";
pub const KEY_RESOLVED_EDGES: &str = "resolved_edges";

// ── Task 1: SourceProfiler ────────────────────────────────────────────────────

pub struct SourceProfilerTask {
    pub progress: Arc<dyn Fn(String) + Send + Sync + 'static>,
}

#[async_trait]
impl Task for SourceProfilerTask {
    fn id(&self) -> &str { "SourceProfilerTask" }

    async fn run(&self, context: Context) -> graph_flow::Result<TaskResult> {
        let title:   String = context.get(KEY_TITLE).await.unwrap_or_default();
        let content: String = context.get(KEY_CONTENT).await.unwrap_or_default();
        let api_key: String = context.get(KEY_API_KEY).await.unwrap_or_default();
        let model:   String = context.get(KEY_MODEL).await.unwrap_or_default();

        (self.progress)(format!("Profiling: {}…", title));

        let content_preview = &content[..content.len().min(3000)];
        let user = format!("Title: {}\n\nContent:\n{}", title, content_preview);

        let parsed = crate::llm::call_json(&api_key, &model, prompts::GRAPH_SOURCE_PROFILER, &user)
            .await
            .map_err(|e| graph_flow::GraphError::TaskExecutionFailed(e.to_string()))?;

        let profile = SourceProfile {
            entity_types: parsed["entity_types"].as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_else(|| vec!["Person".to_string(), "Concept".to_string()]),
            relationship_types: parsed["relationship_types"].as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_else(|| vec!["RELATES_TO".to_string()]),
            themes: parsed["themes"].as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            domain: parsed["domain"].as_str().unwrap_or("general").to_string(),
        };

        (self.progress)(format!("Schema: {} types found", profile.entity_types.join(", ")));
        context.set(KEY_PROFILE, profile).await;
        Ok(TaskResult::move_to_next_direct())
    }
}

// ── Task 2: NodeExtractor ─────────────────────────────────────────────────────

pub struct NodeExtractorTask {
    pub progress: Arc<dyn Fn(String) + Send + Sync + 'static>,
}

#[async_trait]
impl Task for NodeExtractorTask {
    fn id(&self) -> &str { "NodeExtractorTask" }

    async fn run(&self, context: Context) -> graph_flow::Result<TaskResult> {
        let title:   String = context.get(KEY_TITLE).await.unwrap_or_default();
        let content: String = context.get(KEY_CONTENT).await.unwrap_or_default();
        let api_key: String = context.get(KEY_API_KEY).await.unwrap_or_default();
        let model:   String = context.get(KEY_MODEL).await.unwrap_or_default();
        let profile: SourceProfile = context.get(KEY_PROFILE).await.unwrap_or_else(|| SourceProfile {
            entity_types: vec!["Person".to_string(), "Concept".to_string()],
            relationship_types: vec!["RELATES_TO".to_string()],
            themes: vec![],
            domain: "general".to_string(),
        });

        let entity_types_str = profile.entity_types.join(", ");
        let system = prompts::graph_node_extractor(&entity_types_str);

        let chunks = chunk_content(&content, 600);
        let mut all_nodes: Vec<ExtractedNode> = Vec::new();
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        (self.progress)(format!("Extracting nodes ({} chunks): {}…", chunks.len(), title));

        let total = chunks.len();
        for (i, chunk) in chunks.iter().enumerate() {
            let parsed = match crate::llm::call_json(&api_key, &model, &system, chunk).await {
                Ok(v) => v,
                Err(_) => continue,
            };

            if let Some(nodes_arr) = parsed["nodes"].as_array() {
                for node_val in nodes_arr {
                    let id = node_val["id"].as_str().unwrap_or("").to_string();
                    if id.is_empty() || seen_ids.contains(&id) { continue; }
                    let confidence = node_val["confidence"].as_f64().unwrap_or(0.0) as f32;
                    if confidence < 0.7 { continue; }
                    seen_ids.insert(id.clone());
                    all_nodes.push(ExtractedNode {
                        id,
                        name: node_val["name"].as_str().unwrap_or("").to_string(),
                        entity_type: node_val["entity_type"].as_str().unwrap_or("Concept").to_string(),
                        description: node_val["description"].as_str().unwrap_or("").to_string(),
                        confidence,
                        aliases: node_val["aliases"].as_array()
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                            .unwrap_or_default(),
                    });
                }
            }
            (self.progress)(format!("  chunk {}/{} — {} nodes so far", i + 1, total, all_nodes.len()));
        }

        (self.progress)(format!("Nodes done: {} entities found", all_nodes.len()));
        context.set(KEY_NODES, all_nodes).await;
        Ok(TaskResult::move_to_next_direct())
    }
}

// ── Task 3: EdgeExtractor ─────────────────────────────────────────────────────

pub struct EdgeExtractorTask {
    pub progress: Arc<dyn Fn(String) + Send + Sync + 'static>,
}

#[async_trait]
impl Task for EdgeExtractorTask {
    fn id(&self) -> &str { "EdgeExtractorTask" }

    async fn run(&self, context: Context) -> graph_flow::Result<TaskResult> {
        let content: String = context.get(KEY_CONTENT).await.unwrap_or_default();
        let api_key: String = context.get(KEY_API_KEY).await.unwrap_or_default();
        let model:   String = context.get(KEY_MODEL).await.unwrap_or_default();
        let profile: SourceProfile = context.get(KEY_PROFILE).await.unwrap_or_else(|| SourceProfile {
            entity_types: vec![],
            relationship_types: vec!["RELATES_TO".to_string()],
            themes: vec![],
            domain: "general".to_string(),
        });
        let nodes: Vec<ExtractedNode> = context.get(KEY_NODES).await.unwrap_or_default();

        if nodes.is_empty() {
            context.set(KEY_EDGES, Vec::<ExtractedEdge>::new()).await;
            return Ok(TaskResult::move_to_next_direct());
        }

        (self.progress)(format!("Mapping relationships: {} nodes → edges…", nodes.len()));

        let node_ids: Vec<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
        let node_ids_str = node_ids.join(", ");
        let rel_types_str = profile.relationship_types.join(", ");
        let system = prompts::graph_edge_extractor(&node_ids_str, &rel_types_str);

        let nodes_summary: Vec<serde_json::Value> = nodes.iter()
            .map(|n| serde_json::json!({"id": n.id, "name": n.name, "type": n.entity_type}))
            .collect();
        let content_preview = &content[..content.len().min(6000)];
        let user = format!(
            "Content:\n{}\n\nKnown nodes:\n{}",
            content_preview,
            serde_json::to_string_pretty(&nodes_summary).unwrap_or_default()
        );

        let parsed = match crate::llm::call_json(&api_key, &model, &system, &user).await {
            Ok(v) => v,
            Err(_) => {
                context.set(KEY_EDGES, Vec::<ExtractedEdge>::new()).await;
                return Ok(TaskResult::move_to_next_direct());
            }
        };

        let node_id_set: std::collections::HashSet<&str> = node_ids.iter().copied().collect();
        let mut edges: Vec<ExtractedEdge> = Vec::new();

        if let Some(edges_arr) = parsed["edges"].as_array() {
            for edge_val in edges_arr {
                let from_id = edge_val["from_id"].as_str().unwrap_or("").to_string();
                let to_id   = edge_val["to_id"].as_str().unwrap_or("").to_string();
                if from_id.is_empty() || to_id.is_empty() { continue; }
                if !node_id_set.contains(from_id.as_str()) || !node_id_set.contains(to_id.as_str()) { continue; }
                let confidence = edge_val["confidence"].as_f64().unwrap_or(0.0) as f32;
                if confidence < 0.7 { continue; }
                edges.push(ExtractedEdge {
                    from_id,
                    to_id,
                    relationship: edge_val["relationship"].as_str().unwrap_or("RELATES_TO").to_string(),
                    description: edge_val["description"].as_str().unwrap_or("").to_string(),
                    confidence,
                    inferred: edge_val["inferred"].as_bool().unwrap_or(false),
                });
            }
        }

        (self.progress)(format!("Edges done: {} relationships found", edges.len()));
        context.set(KEY_EDGES, edges).await;
        Ok(TaskResult::move_to_next_direct())
    }
}

// ── Task 4: EntityResolver ────────────────────────────────────────────────────

pub struct EntityResolverTask {
    pub progress: Arc<dyn Fn(String) + Send + Sync + 'static>,
}

#[async_trait]
impl Task for EntityResolverTask {
    fn id(&self) -> &str { "EntityResolverTask" }

    async fn run(&self, context: Context) -> graph_flow::Result<TaskResult> {
        let api_key: String = context.get(KEY_API_KEY).await.unwrap_or_default();
        let model:   String = context.get(KEY_MODEL).await.unwrap_or_default();
        let nodes: Vec<ExtractedNode> = context.get(KEY_NODES).await.unwrap_or_default();
        let edges: Vec<ExtractedEdge> = context.get(KEY_EDGES).await.unwrap_or_default();

        if nodes.is_empty() {
            context.set(KEY_RESOLVED_NODES, Vec::<ExtractedNode>::new()).await;
            context.set(KEY_RESOLVED_EDGES, Vec::<ExtractedEdge>::new()).await;
            return Ok(TaskResult::new(None, NextAction::End));
        }

        (self.progress)(format!("Resolving duplicates: {} entities, {} edges…", nodes.len(), edges.len()));

        let nodes_json = serde_json::to_string_pretty(&nodes).unwrap_or_default();
        let user = format!("Entities to resolve:\n{}", nodes_json);

        let parsed = match crate::llm::call_json(&api_key, &model, prompts::GRAPH_ENTITY_RESOLVER, &user).await {
            Ok(v) => v,
            Err(_) => {
                context.set(KEY_RESOLVED_NODES, nodes.clone()).await;
                context.set(KEY_RESOLVED_EDGES, edges.clone()).await;
                return Ok(TaskResult::new(None, NextAction::End));
            }
        };

        let mut merge_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        if let Some(mm) = parsed["merge_map"].as_object() {
            for (alias, canonical_val) in mm {
                if let Some(canonical) = canonical_val.as_str() {
                    merge_map.insert(alias.clone(), canonical.to_string());
                }
            }
        }

        let mut resolved_nodes: Vec<ExtractedNode> = Vec::new();
        let mut resolved_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        if let Some(canonical_arr) = parsed["canonical_nodes"].as_array() {
            for node_val in canonical_arr {
                let confidence = node_val["confidence"].as_f64().unwrap_or(0.0) as f32;
                if confidence < 0.7 { continue; }
                let id = node_val["id"].as_str().unwrap_or("").to_string();
                if id.is_empty() { continue; }
                resolved_ids.insert(id.clone());
                resolved_nodes.push(ExtractedNode {
                    id,
                    name: node_val["name"].as_str().unwrap_or("").to_string(),
                    entity_type: node_val["entity_type"].as_str().unwrap_or("Concept").to_string(),
                    description: node_val["description"].as_str().unwrap_or("").to_string(),
                    confidence,
                    aliases: node_val["aliases"].as_array()
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                        .unwrap_or_default(),
                });
            }
        }

        if resolved_nodes.is_empty() {
            for node in &nodes {
                if node.confidence >= 0.7 {
                    resolved_ids.insert(node.id.clone());
                    resolved_nodes.push(node.clone());
                }
            }
        }

        let resolved_edges: Vec<ExtractedEdge> = edges.into_iter()
            .filter(|e| e.confidence >= 0.7)
            .map(|mut e| {
                if let Some(c) = merge_map.get(&e.from_id) { e.from_id = c.clone(); }
                if let Some(c) = merge_map.get(&e.to_id)   { e.to_id   = c.clone(); }
                e
            })
            .filter(|e| resolved_ids.contains(&e.from_id) && resolved_ids.contains(&e.to_id))
            .collect();

        (self.progress)(format!("Resolved: {} canonical entities, {} edges", resolved_nodes.len(), resolved_edges.len()));

        context.set(KEY_RESOLVED_NODES, resolved_nodes).await;
        context.set(KEY_RESOLVED_EDGES, resolved_edges).await;
        Ok(TaskResult::new(None, NextAction::End))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn chunk_content(content: &str, target_words: usize) -> Vec<String> {
    let words: Vec<&str> = content.split_whitespace().collect();
    if words.is_empty() { return vec![]; }
    words.chunks(target_words).map(|c| c.join(" ")).collect()
}

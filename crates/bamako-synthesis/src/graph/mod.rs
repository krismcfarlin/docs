pub mod tasks;
pub mod types;

pub use types::{ExtractedEdge, ExtractedNode, ResolvedGraph, SourceProfile};

use graph_flow::{ExecutionStatus, GraphBuilder, InMemorySessionStorage, Session, SessionStorage, Task};
use std::sync::Arc;
use tasks::{
    EdgeExtractorTask, EntityResolverTask, NodeExtractorTask, SourceProfilerTask,
    KEY_RESOLVED_EDGES, KEY_RESOLVED_NODES,
};

pub struct GraphInput<'a> {
    pub title:   &'a str,
    pub content: &'a str,
    pub api_key: &'a str,
    pub model:   &'a str,
}

/// Run the 4-task entity extraction pipeline and return the resolved graph.
/// `on_progress` receives human-readable stage strings for UI display.
pub async fn run_graph_synthesis(
    input: &GraphInput<'_>,
    on_progress: impl Fn(String) + Send + Sync + 'static,
) -> crate::error::Result<ResolvedGraph> {
    let progress = Arc::new(on_progress) as Arc<dyn Fn(String) + Send + Sync + 'static>;

    let profiler       = Arc::new(SourceProfilerTask { progress: progress.clone() });
    let node_extractor = Arc::new(NodeExtractorTask  { progress: progress.clone() });
    let edge_extractor = Arc::new(EdgeExtractorTask  { progress: progress.clone() });
    let resolver       = Arc::new(EntityResolverTask { progress: progress.clone() });

    let graph = Arc::new(
        GraphBuilder::new("entity_synthesis")
            .add_task(profiler.clone())
            .add_task(node_extractor.clone())
            .add_task(edge_extractor.clone())
            .add_task(resolver.clone())
            .add_edge(profiler.id(), node_extractor.id())
            .add_edge(node_extractor.id(), edge_extractor.id())
            .add_edge(edge_extractor.id(), resolver.id())
            .set_start_task(profiler.id())
            .build(),
    );

    let session_storage = Arc::new(InMemorySessionStorage::new());
    let session_id = nanoid::nanoid!();
    let session = Session::new_from_task(session_id.clone(), profiler.id());
    session.context.set("title",   input.title).await;
    session.context.set("content", input.content).await;
    session.context.set("api_key", input.api_key).await;
    session.context.set("model",   input.model).await;

    let context = session.context.clone();
    session_storage.save(session).await.map_err(|e| crate::error::SynthesisError::PipelineError(e.to_string()))?;

    let runner = graph_flow::FlowRunner::new(graph.clone(), session_storage.clone());
    let result = runner.run(&session_id).await.map_err(|e| crate::error::SynthesisError::PipelineError(e.to_string()))?;

    if let ExecutionStatus::Error(e) = &result.status {
        return Err(crate::error::SynthesisError::PipelineError(format!("pipeline error: {}", e)));
    }

    let nodes: Vec<ExtractedNode> = context.get(KEY_RESOLVED_NODES).await.unwrap_or_default();
    let edges: Vec<ExtractedEdge> = context.get(KEY_RESOLVED_EDGES).await.unwrap_or_default();

    Ok(ResolvedGraph { nodes, edges })
}

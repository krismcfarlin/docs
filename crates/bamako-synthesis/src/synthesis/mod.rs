pub mod types;
pub use types::*;

use crate::error::{Result, SynthesisError};
use crate::graph::{GraphInput, run_graph_synthesis};
use crate::llm::{LlmRequest, call, call_json};
use crate::prompts;

/// Summarize a page and run the entity extraction pipeline.
/// Returns summary data plus the full resolved entity graph.
pub async fn summarize_page(
    title: &str,
    content: &str,
    config: &SpaceConfig,
    on_progress: impl Fn(String) + Send + Sync + 'static,
) -> Result<SynthesizePageOutput> {
    if content.trim().is_empty() {
        return Err(SynthesisError::EmptyContent);
    }

    let user_msg = format!("Title: {}\n\n{}", title, &content[..content.len().min(8000)]);
    let raw = call(LlmRequest {
        api_key: &config.api_key,
        model: &config.model,
        system: prompts::PAGE_SUMMARIZER,
        user: &user_msg,
        json_mode: true,
        temperature: 0.3,
    }).await?;

    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| SynthesisError::ParseError(e.to_string(), raw[..raw.len().min(200)].to_string()))?;

    let summary = parsed["summary"].as_str().unwrap_or("").to_string();
    let key_points: Vec<String> = parsed["key_points"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let topics: Vec<String> = parsed["topics"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    let graph = run_graph_synthesis(
        &GraphInput { title, content, api_key: &config.api_key, model: &config.model },
        on_progress,
    ).await.unwrap_or_else(|_| crate::graph::ResolvedGraph { nodes: vec![], edges: vec![] });

    Ok(SynthesizePageOutput { summary, key_points, topics, graph })
}

/// Generate or update a space-level overview from existing page summaries.
pub async fn generate_space_overview(
    existing: Option<&str>,
    summaries: &[PageSummaryInput],
    config: &SpaceConfig,
) -> Result<SpaceOverview> {
    if summaries.is_empty() {
        return Err(SynthesisError::EmptyContent);
    }

    let mut summaries_text = String::new();
    for s in summaries {
        summaries_text.push_str(&format!("- \"{}\": {}\n", s.title, s.summary));
    }

    let user_msg = format!(
        "Current overview: {}\n\nNew/updated summaries:\n{}",
        existing.unwrap_or("(none yet)"),
        summaries_text
    );

    let raw = call(LlmRequest {
        api_key: &config.api_key,
        model: &config.model,
        system: prompts::SPACE_OVERVIEW_UPDATER,
        user: &user_msg,
        json_mode: true,
        temperature: 0.3,
    }).await?;

    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| SynthesisError::ParseError(e.to_string(), raw[..raw.len().min(300)].to_string()))?;

    Ok(SpaceOverview {
        overview: parsed["overview"].as_str().unwrap_or("").to_string(),
        topics: parsed["topics"].as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
    })
}

/// Answer a question grounded strictly in compiled knowledge from the space.
pub async fn ask_wiki(
    question: &str,
    summaries: &[PageSummaryInput],
    entities: &[EntityInput],
    config: &SpaceConfig,
) -> Result<WikiAnswer> {
    let mut context_parts: Vec<String> = Vec::new();
    let mut source_titles: Vec<String> = Vec::new();

    for s in summaries {
        source_titles.push(s.title.clone());
        let kp_str = if s.key_points.is_empty() { String::new() } else {
            format!("\nKey points:\n{}", s.key_points.iter().map(|k| format!("- {}", k)).collect::<Vec<_>>().join("\n"))
        };
        context_parts.push(format!("### {}\n{}{}", s.title, s.summary, kp_str));
    }

    let mut entity_context = String::new();
    for e in entities {
        entity_context.push_str(&format!("- {} ({}): {}\n", e.name, e.entity_type, e.description));
    }

    let user_msg = format!(
        "Question: {}\n\n## Compiled Knowledge\n\n{}\n\n## Known Entities\n\n{}",
        question,
        context_parts.join("\n\n---\n\n"),
        entity_context
    );

    let raw = call(LlmRequest {
        api_key: &config.api_key,
        model: &config.model,
        system: prompts::WIKI_QA,
        user: &user_msg,
        json_mode: true,
        temperature: 0.3,
    }).await?;

    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| SynthesisError::ParseError(e.to_string(), raw[..raw.len().min(200)].to_string()))?;

    Ok(WikiAnswer {
        answer: parsed["answer"].as_str().unwrap_or("No answer generated.").to_string(),
        sources: source_titles,
        confidence: parsed["confidence"].as_str().unwrap_or("medium").to_string(),
    })
}

/// Generate LLM-enriched markdown content for an entity wiki page.
pub async fn generate_entity_page(
    entity: &EntityInput,
    mentions: &[MentionInput],
    config: &SpaceConfig,
) -> Result<String> {
    let mut mentions_list = String::new();
    for m in mentions {
        mentions_list.push_str(&format!("- Document \"{}\": {}\n", m.page_title, m.excerpt));
    }

    let user_msg = format!(
        "Entity: {} (type: {})\nDescription: {}\n\nMentioned in these documents:\n{}",
        entity.name, entity.entity_type, entity.description, mentions_list
    );

    call(LlmRequest {
        api_key: &config.api_key,
        model: &config.model,
        system: prompts::ENTITY_PAGE_WRITER,
        user: &user_msg,
        json_mode: false,
        temperature: 0.3,
    }).await
}

/// Generate investigation questions and suggested sources from page summaries.
pub async fn generate_lint_questions(
    summaries: &[PageSummaryInput],
    orphan_page_names: &[String],
    config: &SpaceConfig,
) -> Result<LintQuestions> {
    if summaries.is_empty() {
        return Ok(LintQuestions { investigation_questions: vec![], suggested_sources: vec![] });
    }

    let mut summaries_text = String::new();
    for s in summaries {
        summaries_text.push_str(&format!("- \"{}\": {}\n", s.title, s.summary));
    }
    let orphan_info = if orphan_page_names.is_empty() { String::new() }
        else { format!("\nOrphan wiki pages: {}", orphan_page_names.join(", ")) };

    let user_msg = format!("Summaries:\n{}{}", summaries_text, orphan_info);

    let parsed = call_json(
        &config.api_key, &config.model,
        prompts::SPACE_LINTER, &user_msg,
    ).await?;

    Ok(LintQuestions {
        investigation_questions: parsed["questions"].as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        suggested_sources: parsed["sources"].as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
    })
}

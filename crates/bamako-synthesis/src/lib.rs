pub mod error;
pub mod graph;
pub mod llm;
pub mod prompts;
pub mod synthesis;
pub mod vector;
pub mod wiki;

// Flatten the most-used types to crate root
pub use error::{Result, SynthesisError};
pub use graph::{run_graph_synthesis, ExtractedEdge, ExtractedNode, GraphInput, ResolvedGraph};
pub use synthesis::{
    ask_wiki, generate_entity_page, generate_lint_questions, generate_space_overview,
    summarize_page,
    types::{
        EntityInput, LintQuestions, MentionInput, PageSummaryInput, SpaceConfig, SpaceOverview,
        SynthesizePageOutput, WikiAnswer,
    },
};
pub use vector::{cosine, embed, from_json as vec_from_json, make_snippet, to_json as vec_to_json, DIMS};
pub use wiki::{build_wiki_stub, content_hash, update_mentioned_in_section};

#[cfg(feature = "veles")]
pub use vector::VelesClient;

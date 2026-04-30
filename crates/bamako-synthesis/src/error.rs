#[derive(Debug, thiserror::Error)]
pub enum SynthesisError {
    #[error("LLM API error: {0}")]
    LlmError(String),

    #[error("JSON parse error: {0} — raw: {1}")]
    ParseError(String, String),

    #[error("Graph pipeline error: {0}")]
    PipelineError(String),

    #[error("No API key configured")]
    NoApiKey,

    #[error("Page has no content")]
    EmptyContent,
}

pub type Result<T> = std::result::Result<T, SynthesisError>;

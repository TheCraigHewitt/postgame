//! BYOK LLM access.

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("provider request failed: {0}")]
    Request(String),
}

/// An LLM provider returning JSON that matches a schema.
pub trait Provider {
    fn complete_json(&self, prompt: &str, schema: &str) -> Result<String, LlmError>;
}

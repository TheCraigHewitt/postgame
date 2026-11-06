//! Speech-to-text engines behind one trait.

use std::path::Path;

/// One recognized word. Times are milliseconds from the start of the audio.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub conf: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    #[error("ASR engine failed: {0}")]
    Engine(String),
}

/// An offline speech-to-text engine.
pub trait Asr {
    fn transcribe(&self, wav: &Path) -> Result<Vec<Word>, AsrError>;
}

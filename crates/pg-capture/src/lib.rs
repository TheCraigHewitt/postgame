//! Two-track capture: the mic and the meeting app's audio stay separate.

#[cfg(target_os = "macos")]
mod macos;
pub mod target;
pub mod track;

#[cfg(target_os = "macos")]
pub use macos::{MacCapture, audio_processes, shim_version};
pub use target::SystemTarget;
pub use track::TrackSummary;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("could not start capture: {0}")]
    Start(String),
    #[error("nothing is recording")]
    NotRecording,
    #[error("a track writer panicked")]
    WriterPanicked,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// What a finished recording contains.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaptureSummary {
    pub mic: TrackSummary,
    pub system: TrackSummary,
}

/// A platform capture engine.
pub trait CaptureBackend {
    /// Start recording. Stays active until [`CaptureBackend::stop`].
    fn start(&mut self, target: SystemTarget) -> Result<(), CaptureError>;
    /// Stop recording and finalize the output.
    fn stop(&mut self) -> Result<CaptureSummary, CaptureError>;
}

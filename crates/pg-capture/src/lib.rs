//! Two-track capture: the mic and the meeting app's audio stay separate.

#[cfg(target_os = "macos")]
mod macos;
pub mod target;
pub mod track;

#[cfg(target_os = "macos")]
pub use macos::shim_version;

pub use target::SystemTarget;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("capture is not implemented yet")]
    Unimplemented,
}

/// A platform capture engine.
pub trait CaptureBackend {
    /// Start recording. Stays active until [`CaptureBackend::stop`].
    fn start(&mut self, target: SystemTarget) -> Result<(), CaptureError>;
    /// Stop recording and finalize the output.
    fn stop(&mut self) -> Result<(), CaptureError>;
}

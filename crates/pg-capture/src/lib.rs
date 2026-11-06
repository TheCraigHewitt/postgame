//! Two-track capture: the mic and the meeting app's audio stay separate.

#[cfg(target_os = "macos")]
mod macos;
pub mod track;

#[cfg(target_os = "macos")]
pub use macos::shim_version;

/// What the system track records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemTarget {
    /// Every process whose bundle ID starts with this prefix, helpers included.
    BundlePrefix(String),
    /// All system audio except our own process.
    AllSystem,
}

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

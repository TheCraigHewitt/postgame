//! macOS capture: Core Audio process taps and AVAudioEngine through PGAudio.

mod backend;
mod shim;

pub use backend::MacCapture;
pub use shim::{audio_processes, shim_version};

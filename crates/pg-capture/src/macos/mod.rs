//! macOS capture: Core Audio process taps and AVAudioEngine through PGAudio.

pub mod shim;

pub use shim::{audio_processes, shim_version};

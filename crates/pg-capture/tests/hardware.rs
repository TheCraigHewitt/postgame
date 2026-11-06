//! Tests that need real audio hardware and macOS permissions. They play sound
//! through the speakers and open the mic.
//!
//! Run from a terminal that has Microphone and System Audio Recording access:
//!
//!     cargo test -p pg-capture --release -- --ignored --nocapture
//!
//! macOS attributes both permissions to the terminal app. A denied System Audio
//! permission shows up as a silent track, so this test then fails on its peak.

#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use pg_capture::{CaptureBackend, MacCapture, SystemTarget, audio_processes};
use pg_dsp::correlate::{best_ncc, decimate};

const SCRIPT: &str = "The quarterly renewal is due on the fifteenth of March. \
    Please send the updated security questionnaire to the procurement team.";
const MIN_CORRELATION: f32 = 0.9;
/// Both signals are compared at 4 kHz, plenty for speech, to keep the search fast.
const DECIMATE_TO_HZ: u32 = 4000;

fn run(command: &mut Command) {
    let status = command.status().unwrap();
    assert!(status.success(), "{command:?} failed");
}

fn read_mono(path: &Path) -> (Vec<f32>, u32) {
    let mut reader = hound::WavReader::open(path).unwrap();
    let rate = reader.spec().sample_rate;
    let samples = reader
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        .collect();
    (samples, rate)
}

#[test]
#[ignore = "plays sound and needs mic and system audio permissions"]
fn system_track_matches_the_tapped_process() {
    let dir = tempfile::tempdir().unwrap();
    let clip = dir.path().join("clip.aiff");
    run(Command::new("say").arg("-o").arg(&clip).arg(SCRIPT));

    let mut player = Command::new("afplay").arg(&clip).spawn().unwrap();
    let pid = player.id() as i32;
    let appeared = (0..100).any(|_| {
        sleep(Duration::from_millis(20));
        audio_processes()
            .iter()
            .any(|p| p.pid == pid && p.running_output)
    });
    assert!(appeared, "afplay never appeared as an audio process");

    let out = dir.path().join("recording");
    let mut capture = MacCapture::new(out.clone());
    capture.start(SystemTarget::Pid(pid)).unwrap();
    player.wait().unwrap();
    sleep(Duration::from_millis(500));
    let summary = capture.stop().unwrap();

    assert!(summary.mic.samples > 0, "the mic delivered nothing");
    assert!(
        summary.system.peak > 0.01,
        "system track is silent (peak {}): is System Audio Recording granted?",
        summary.system.peak
    );

    let (captured, rate) = read_mono(&out.join("system.wav"));
    let source = dir.path().join("source.wav");
    run(Command::new("afconvert")
        .args(["-f", "WAVE", "-d", &format!("LEI16@{rate}"), "-c", "1"])
        .arg(&clip)
        .arg(&source));
    let (source, _) = read_mono(&source);

    let factor = (rate / DECIMATE_TO_HZ) as usize;
    let score = best_ncc(
        &decimate(&source, factor),
        trim(&decimate(&captured, factor)),
    );
    println!("system track correlation with the source: {score:.3}");
    assert!(score >= MIN_CORRELATION, "correlation {score}");
}

/// Drops leading and trailing silence, which has no counterpart in the source.
fn trim(samples: &[f32]) -> &[f32] {
    let loud = |s: &f32| s.abs() > 0.001;
    let start = samples.iter().position(loud).unwrap_or(0);
    let end = samples.iter().rposition(loud).map_or(0, |i| i + 1);
    &samples[start..end.max(start)]
}

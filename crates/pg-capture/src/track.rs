//! Writes one captured track: a mono 16-bit WAV plus timing log entries.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use hound::{SampleFormat, WavSpec, WavWriter};
use pg_dsp::align::TimingPoint;
use serde::Serialize;

/// A jump in host time above this is treated as lost audio, and padded.
const GAP_TOLERANCE_NS: f64 = 50_000_000.0;

/// An append-only JSON-lines file, flushed per line so a crash loses nothing
/// already logged.
#[derive(Clone)]
pub struct JsonlLog(Arc<Mutex<BufWriter<File>>>);

impl JsonlLog {
    pub fn create(path: &Path) -> std::io::Result<Self> {
        Ok(Self(Arc::new(Mutex::new(BufWriter::new(File::create(
            path,
        )?)))))
    }

    pub fn append(&self, value: &impl Serialize) -> std::io::Result<()> {
        let line = serde_json::to_string(value)?;
        let mut file = self.0.lock().unwrap_or_else(|e| e.into_inner());
        writeln!(file, "{line}")?;
        file.flush()
    }
}

#[derive(Serialize)]
struct HealthEvent<'a> {
    t_ms: i64,
    kind: &'a str,
    detail: &'a str,
}

/// `health.jsonl`: everything that affected the capture.
#[derive(Clone)]
pub struct HealthLog {
    log: JsonlLog,
    started: Instant,
}

impl HealthLog {
    pub fn create(path: &Path) -> std::io::Result<Self> {
        Ok(Self {
            log: JsonlLog::create(path)?,
            started: Instant::now(),
        })
    }

    pub fn event(&self, kind: &str, detail: &str) {
        let t_ms = self.started.elapsed().as_millis() as i64;
        let event = HealthEvent { t_ms, kind, detail };
        if let Err(e) = self.log.append(&event) {
            eprintln!("could not write health log: {e}");
        }
    }
}

/// One buffer of mono audio. `host_time_ns` is the time of its first sample.
#[derive(Debug, Clone, PartialEq)]
pub struct Buffer {
    pub samples: Vec<f32>,
    pub host_time_ns: u64,
    pub sample_rate: f64,
}

/// What a finished track contains.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TrackSummary {
    pub samples: u64,
    pub padded_samples: u64,
    pub peak: f32,
}

/// Averages all channels of a capture callback into mono. Each slice in
/// `buffers` holds `channels_per_buffer` interleaved channels.
pub fn downmix(buffers: &[&[f32]], channels_per_buffer: usize, frames: usize) -> Vec<f32> {
    let channels = buffers.len() * channels_per_buffer;
    if channels == 0 {
        return Vec::new();
    }
    let mut mono = vec![0.0; frames];
    for buffer in buffers {
        for (out, frame) in mono.iter_mut().zip(buffer.chunks(channels_per_buffer)) {
            *out += frame.iter().sum::<f32>();
        }
    }
    mono.iter_mut().for_each(|s| *s /= channels as f32);
    mono
}

fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

type Wav = WavWriter<BufWriter<File>>;

/// Consumes buffers for one track.
pub struct TrackWriter {
    name: &'static str,
    path: PathBuf,
    timing: JsonlLog,
    health: HealthLog,
    wav: Option<Wav>,
    rate: f64,
    next_host_ns: f64,
    last_logged: Option<u64>,
    last_flushed: u64,
    summary: TrackSummary,
}

impl TrackWriter {
    pub fn new(name: &'static str, path: PathBuf, timing: JsonlLog, health: HealthLog) -> Self {
        Self {
            name,
            path,
            timing,
            health,
            wav: None,
            rate: 0.0,
            next_host_ns: 0.0,
            last_logged: None,
            last_flushed: 0,
            summary: TrackSummary::default(),
        }
    }

    /// Appends a buffer, padding with silence if audio was lost before it, so
    /// that the sample index stays linear in host time.
    pub fn write(&mut self, buffer: &Buffer) -> Result<(), hound::Error> {
        if self.wav.is_none() {
            self.rate = buffer.sample_rate;
            self.next_host_ns = buffer.host_time_ns as f64;
            self.wav = Some(WavWriter::create(
                &self.path,
                WavSpec {
                    channels: 1,
                    sample_rate: buffer.sample_rate.round() as u32,
                    bits_per_sample: 16,
                    sample_format: SampleFormat::Int,
                },
            )?);
        } else if buffer.sample_rate != self.rate {
            let detail = format!(
                "{}: {} Hz to {} Hz",
                self.name, self.rate, buffer.sample_rate
            );
            self.health.event("sample_rate_changed", &detail);
        }

        let gap_ns = buffer.host_time_ns as f64 - self.next_host_ns;
        if gap_ns > GAP_TOLERANCE_NS {
            self.pad(gap_ns)?;
        }

        self.log_timing(buffer.host_time_ns);
        let wav = self.wav.as_mut().expect("created above");
        for sample in &buffer.samples {
            wav.write_sample(to_i16(*sample))?;
            self.summary.peak = self.summary.peak.max(sample.abs());
        }
        self.summary.samples += buffer.samples.len() as u64;
        self.next_host_ns =
            buffer.host_time_ns as f64 + buffer.samples.len() as f64 * 1e9 / self.rate;

        if (self.summary.samples - self.last_flushed) as f64 >= self.rate {
            self.wav.as_mut().expect("created above").flush()?;
            self.last_flushed = self.summary.samples;
        }
        Ok(())
    }

    fn pad(&mut self, gap_ns: f64) -> Result<(), hound::Error> {
        let frames = (gap_ns * self.rate / 1e9).round() as u64;
        let wav = self.wav.as_mut().expect("created before any padding");
        for _ in 0..frames {
            wav.write_sample(0i16)?;
        }
        self.summary.samples += frames;
        self.summary.padded_samples += frames;
        let detail = format!("{}: {:.0} ms of silence padded", self.name, gap_ns / 1e6);
        self.health.event("gap", &detail);
        Ok(())
    }

    fn log_timing(&mut self, host_time_ns: u64) {
        let due = self
            .last_logged
            .is_none_or(|last| (self.summary.samples - last) as f64 >= self.rate);
        if due {
            self.append_timing(host_time_ns);
        }
    }

    fn append_timing(&mut self, host_time_ns: u64) {
        self.last_logged = Some(self.summary.samples);
        let point = TimingPoint {
            track: self.name.to_string(),
            host_time_ns,
            sample_index: self.summary.samples,
            sample_rate: self.rate,
        };
        if let Err(e) = self.timing.append(&point) {
            self.health.event("timing_log_failed", &e.to_string());
        }
    }

    /// Finalizes the WAV header and returns what was written.
    pub fn finish(mut self) -> Result<TrackSummary, hound::Error> {
        if self.wav.is_none() {
            self.health
                .event("no_audio", &format!("{}: no buffers arrived", self.name));
        }
        if let Some(wav) = self.wav.take() {
            // A last point lets short recordings fit a line.
            self.append_timing(self.next_host_ns as u64);
            wav.finalize()?;
        }
        Ok(self.summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pg_dsp::align::parse_timing;

    struct Fixture {
        dir: tempfile::TempDir,
        writer: TrackWriter,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let timing = JsonlLog::create(&dir.path().join("timing.jsonl")).unwrap();
        let health = HealthLog::create(&dir.path().join("health.jsonl")).unwrap();
        let writer = TrackWriter::new("mic", dir.path().join("mic.wav"), timing, health);
        Fixture { dir, writer }
    }

    fn buffer(value: f32, frames: usize, host_ms: u64) -> Buffer {
        Buffer {
            samples: vec![value; frames],
            host_time_ns: host_ms * 1_000_000,
            sample_rate: 1000.0,
        }
    }

    fn read_wav(path: &Path) -> Vec<i16> {
        hound::WavReader::open(path)
            .unwrap()
            .into_samples::<i16>()
            .map(Result::unwrap)
            .collect()
    }

    fn read(dir: &tempfile::TempDir, name: &str) -> String {
        std::fs::read_to_string(dir.path().join(name)).unwrap()
    }

    #[test]
    fn downmix_averages_interleaved_stereo() {
        let mono = downmix(&[&[1.0, 0.0, 0.5, 0.5]], 2, 2);
        assert_eq!(mono, vec![0.5, 0.5]);
    }

    #[test]
    fn downmix_averages_planar_channels() {
        let mono = downmix(&[&[1.0, 0.0], &[0.0, 0.0]], 1, 2);
        assert_eq!(mono, vec![0.5, 0.0]);
    }

    #[test]
    fn downmix_of_no_channels_is_empty() {
        assert!(downmix(&[], 1, 10).is_empty());
    }

    #[test]
    fn samples_are_clamped_and_scaled() {
        assert_eq!(to_i16(2.0), i16::MAX);
        assert_eq!(to_i16(-2.0), -i16::MAX);
        assert_eq!(to_i16(0.0), 0);
    }

    #[test]
    fn writes_a_mono_16_bit_wav() {
        let mut f = fixture();
        f.writer.write(&buffer(0.5, 100, 0)).unwrap();
        let summary = f.writer.finish().unwrap();
        assert_eq!(summary.samples, 100);
        let reader = hound::WavReader::open(f.dir.path().join("mic.wav")).unwrap();
        let spec = reader.spec();
        assert_eq!((spec.channels, spec.bits_per_sample), (1, 16));
        assert_eq!(spec.sample_rate, 1000);
        assert_eq!(read_wav(&f.dir.path().join("mic.wav"))[0], 16384);
    }

    #[test]
    fn timing_is_logged_about_every_second() {
        let mut f = fixture();
        for i in 0..35 {
            f.writer.write(&buffer(0.1, 100, i * 100)).unwrap();
        }
        f.writer.finish().unwrap();
        let points = parse_timing(&read(&f.dir, "timing.jsonl")).unwrap();
        let indexes: Vec<u64> = points.iter().map(|p| p.sample_index).collect();
        assert_eq!(indexes, vec![0, 1000, 2000, 3000, 3500]);
        assert_eq!(points[1].host_time_ns, 1_000_000_000);
        assert_eq!(points[4].host_time_ns, 3_500_000_000);
    }

    #[test]
    fn lost_audio_is_padded_and_logged() {
        let mut f = fixture();
        f.writer.write(&buffer(0.5, 100, 0)).unwrap();
        f.writer.write(&buffer(0.5, 100, 400)).unwrap();
        let summary = f.writer.finish().unwrap();
        assert_eq!(summary.samples, 500);
        assert_eq!(summary.padded_samples, 300);
        let samples = read_wav(&f.dir.path().join("mic.wav"));
        assert_eq!((samples[99], samples[100], samples[399]), (16384, 0, 0));
        assert_eq!(samples[400], 16384);
        assert!(read(&f.dir, "health.jsonl").contains("\"kind\":\"gap\""));
    }

    #[test]
    fn buffer_timing_jitter_is_not_a_gap() {
        let mut f = fixture();
        f.writer.write(&buffer(0.5, 100, 0)).unwrap();
        f.writer.write(&buffer(0.5, 100, 130)).unwrap();
        assert_eq!(f.writer.finish().unwrap().padded_samples, 0);
        assert_eq!(read(&f.dir, "health.jsonl"), "");
    }

    #[test]
    fn a_track_with_no_audio_is_reported_and_leaves_no_file() {
        let f = fixture();
        assert_eq!(f.writer.finish().unwrap(), TrackSummary::default());
        assert!(read(&f.dir, "health.jsonl").contains("\"kind\":\"no_audio\""));
        assert!(!f.dir.path().join("mic.wav").exists());
    }

    #[test]
    fn audio_flushed_before_a_crash_is_readable() {
        let mut f = fixture();
        f.writer.write(&buffer(0.5, 1500, 0)).unwrap();
        // Dropping without `finish` stands in for the process dying. Copy the
        // file first, because WavWriter's Drop would finalize it.
        let crashed = f.dir.path().join("crashed.wav");
        std::fs::copy(f.dir.path().join("mic.wav"), &crashed).unwrap();
        assert_eq!(read_wav(&crashed).len(), 1500);
    }

    #[test]
    fn health_events_are_json_lines() {
        let f = fixture();
        f.writer.health.event("started", "a \"quoted\" detail");
        let line = read(&f.dir, "health.jsonl");
        let event: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(event["kind"], "started");
        assert_eq!(event["detail"], "a \"quoted\" detail");
    }
}

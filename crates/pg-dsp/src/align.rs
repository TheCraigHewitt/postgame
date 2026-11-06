//! Maps the system track onto the mic timeline from the capture timing log.

use serde::{Deserialize, Serialize};

/// One line of `timing.jsonl`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimingPoint {
    pub track: String,
    pub host_time_ns: u64,
    pub sample_index: u64,
    pub sample_rate: f64,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum AlignError {
    #[error("the {0} track has fewer than 2 distinct timing points")]
    NotEnoughPoints(&'static str),
    #[error("invalid timing log line {line}: {detail}")]
    BadLine { line: usize, detail: String },
}

/// Least-squares fit of host time against sample index for one track.
#[derive(Debug, Clone, PartialEq)]
struct ClockFit {
    mean_sample: f64,
    mean_host_ns: i128,
    ns_per_sample: f64,
    sample_rate: f64,
    max_residual_ns: f64,
}

impl ClockFit {
    fn new(points: &[&TimingPoint], track: &'static str) -> Result<Self, AlignError> {
        let first_sample = points.first().map_or(0, |p| p.sample_index);
        if !points.iter().any(|p| p.sample_index != first_sample) {
            return Err(AlignError::NotEnoughPoints(track));
        }
        let n = points.len() as f64;
        let mean_sample = points.iter().map(|p| p.sample_index as f64).sum::<f64>() / n;
        let mean_host_ns =
            points.iter().map(|p| p.host_time_ns as i128).sum::<i128>() / points.len() as i128;

        let (mut xy, mut xx) = (0.0, 0.0);
        for p in points {
            let x = p.sample_index as f64 - mean_sample;
            xy += x * (p.host_time_ns as i128 - mean_host_ns) as f64;
            xx += x * x;
        }
        let ns_per_sample = xy / xx;
        let max_residual_ns = points
            .iter()
            .map(|p| {
                let x = p.sample_index as f64 - mean_sample;
                ((p.host_time_ns as i128 - mean_host_ns) as f64 - x * ns_per_sample).abs()
            })
            .fold(0.0, f64::max);
        Ok(Self {
            mean_sample,
            mean_host_ns,
            ns_per_sample,
            sample_rate: points[0].sample_rate,
            max_residual_ns,
        })
    }

    /// Host time at `sample`, as nanoseconds relative to this fit's mean.
    fn host_ns_at(&self, sample: f64) -> f64 {
        (sample - self.mean_sample) * self.ns_per_sample
    }

    /// Real nanoseconds per nominal nanosecond of audio.
    fn clock_ratio(&self) -> f64 {
        self.ns_per_sample * self.sample_rate / 1e9
    }
}

/// `mic_ms = offset_ms + system_ms * drift_ratio`, where `system_ms` and
/// `mic_ms` are positions within each track at its nominal sample rate.
#[derive(Debug, Clone, PartialEq)]
pub struct Alignment {
    pub offset_ms: f64,
    pub drift_ratio: f64,
    /// The worst distance of any timing point from its track's fit. A large
    /// value means an unpadded gap or a bad timestamp.
    pub max_residual_ms: f64,
}

impl Alignment {
    pub fn fit(points: &[TimingPoint]) -> Result<Self, AlignError> {
        let track = |name: &str| -> Vec<&TimingPoint> {
            points.iter().filter(|p| p.track == name).collect()
        };
        let mic = ClockFit::new(&track("mic"), "mic")?;
        let system = ClockFit::new(&track("system"), "system")?;

        // Host time of system sample 0, relative to the mic fit's mean.
        let system_start = (system.mean_host_ns - mic.mean_host_ns) as f64 + system.host_ns_at(0.0);
        let mic_sample = mic.mean_sample + system_start / mic.ns_per_sample;
        Ok(Self {
            offset_ms: mic_sample / mic.sample_rate * 1000.0,
            drift_ratio: system.clock_ratio() / mic.clock_ratio(),
            max_residual_ms: mic.max_residual_ns.max(system.max_residual_ns) / 1e6,
        })
    }

    pub fn system_to_mic_ms(&self, system_ms: f64) -> f64 {
        self.offset_ms + system_ms * self.drift_ratio
    }
}

/// Parses the contents of a `timing.jsonl` file.
pub fn parse_timing(text: &str) -> Result<Vec<TimingPoint>, AlignError> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(i, line)| {
            serde_json::from_str(line).map_err(|e| AlignError::BadLine {
                line: i + 1,
                detail: e.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A track starting at `start_ns` whose clock runs `ratio` times real time
    /// slower or faster, with `jitter_ns` of alternating timestamp noise.
    fn track(name: &str, start_ns: u64, rate: f64, ratio: f64, jitter_ns: i64) -> Vec<TimingPoint> {
        (0..3600)
            .map(|second| {
                let sample_index = (second as f64 * rate) as u64;
                let ns = second as f64 * 1e9 * ratio;
                let jitter = if second % 2 == 0 {
                    jitter_ns
                } else {
                    -jitter_ns
                };
                TimingPoint {
                    track: name.into(),
                    host_time_ns: (start_ns as i64 + ns as i64 + jitter) as u64,
                    sample_index,
                    sample_rate: rate,
                }
            })
            .collect()
    }

    const START: u64 = 5_000_000_000_000;

    #[test]
    fn identical_clocks_only_differ_by_start_offset() {
        let mut points = track("mic", START, 48_000.0, 1.0, 0);
        points.extend(track("system", START + 250_000_000, 48_000.0, 1.0, 0));
        let a = Alignment::fit(&points).unwrap();
        assert!((a.offset_ms - 250.0).abs() < 1e-3, "{a:?}");
        assert!((a.drift_ratio - 1.0).abs() < 1e-9, "{a:?}");
    }

    #[test]
    fn system_clock_drift_is_recovered_at_hour_one() {
        // 40 ppm of drift is 144 ms over an hour.
        let mut points = track("mic", START, 48_000.0, 1.0, 0);
        points.extend(track("system", START, 44_100.0, 1.000_04, 0));
        let a = Alignment::fit(&points).unwrap();
        assert!((a.system_to_mic_ms(3_600_000.0) - 3_600_144.0).abs() < 0.01);
    }

    #[test]
    fn mic_clock_drift_is_recovered() {
        let mut points = track("mic", START, 48_000.0, 0.999_97, 0);
        points.extend(track("system", START, 48_000.0, 1.0, 0));
        let a = Alignment::fit(&points).unwrap();
        // The mic's nominal timeline runs ahead of real time by 30 ppm.
        assert!((a.system_to_mic_ms(1_000_000.0) - 1_000_030.0).abs() < 0.01);
    }

    #[test]
    fn timestamp_jitter_does_not_move_the_fit() {
        let mut points = track("mic", START, 48_000.0, 1.0, 500_000);
        points.extend(track("system", START, 48_000.0, 1.000_01, 500_000));
        let a = Alignment::fit(&points).unwrap();
        assert!((a.system_to_mic_ms(3_600_000.0) - 3_600_036.0).abs() < 0.1);
        assert!(a.max_residual_ms < 0.51);
    }

    #[test]
    fn a_jump_shows_up_in_the_residual() {
        let mut points = track("mic", START, 48_000.0, 1.0, 0);
        let mut system = track("system", START, 48_000.0, 1.0, 0);
        for p in &mut system[1800..] {
            p.host_time_ns += 200_000_000;
        }
        points.extend(system);
        assert!(Alignment::fit(&points).unwrap().max_residual_ms > 50.0);
    }

    #[test]
    fn a_track_without_two_points_is_rejected() {
        let points = track("mic", START, 48_000.0, 1.0, 0);
        assert_eq!(
            Alignment::fit(&points),
            Err(AlignError::NotEnoughPoints("system"))
        );
    }

    #[test]
    fn timing_lines_round_trip_and_bad_lines_report_their_number() {
        let p = TimingPoint {
            track: "mic".into(),
            host_time_ns: 7,
            sample_index: 9,
            sample_rate: 48_000.0,
        };
        let line = serde_json::to_string(&p).unwrap();
        assert_eq!(parse_timing(&format!("{line}\n\n")).unwrap(), vec![p]);
        let err = parse_timing(&format!("{line}\nnot json\n")).unwrap_err();
        assert!(matches!(err, AlignError::BadLine { line: 2, .. }));
    }
}

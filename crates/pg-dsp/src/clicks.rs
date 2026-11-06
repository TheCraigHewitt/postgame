//! Finds loud, isolated transients. M1's sync check uses them to measure the
//! real mic-to-system offset at two points in a recording.

const HOP_MS: f64 = 1.0;
const THRESHOLD_OF_PEAK: f32 = 0.5;
const REFRACTORY_MS: f64 = 1000.0;

/// Returns the start time in ms of each transient reaching half the track's
/// peak level, at most one per second.
pub fn find_clicks(samples: &[f32], sample_rate: f64) -> Vec<f64> {
    let hop = ((sample_rate * HOP_MS / 1000.0) as usize).max(1);
    let peaks: Vec<f32> = samples
        .chunks(hop)
        .map(|c| c.iter().fold(0.0, |max: f32, s| max.max(s.abs())))
        .collect();
    let threshold = peaks.iter().copied().fold(0.0, f32::max) * THRESHOLD_OF_PEAK;
    if threshold <= 0.0 {
        return Vec::new();
    }

    let mut clicks = Vec::new();
    let mut last = f64::NEG_INFINITY;
    for (i, peak) in peaks.iter().enumerate() {
        let t = (i * hop) as f64 / sample_rate * 1000.0;
        if *peak >= threshold && t - last >= REFRACTORY_MS {
            clicks.push(t);
            last = t;
        }
    }
    clicks
}

/// For each system click, finds the nearest mic click within `tolerance_ms` and
/// returns `mic - system`. Both inputs must be on the same timeline.
pub fn click_offsets(mic: &[f64], system: &[f64], tolerance_ms: f64) -> Vec<f64> {
    system
        .iter()
        .filter_map(|s| {
            mic.iter()
                .map(|m| m - s)
                .min_by(|a, b| a.abs().total_cmp(&b.abs()))
                .filter(|offset| offset.abs() <= tolerance_ms)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_clicks_and_skips_unmatched_ones() {
        let offsets = click_offsets(&[1012.0, 5000.0, 9000.0], &[1000.0, 9005.0, 20000.0], 500.0);
        assert_eq!(offsets, vec![12.0, -5.0]);
    }

    fn with_clicks(rate: f64, seconds: usize, at_ms: &[usize]) -> Vec<f32> {
        let mut samples = vec![0.001; rate as usize * seconds];
        for ms in at_ms {
            let start = (*ms as f64 * rate / 1000.0) as usize;
            samples[start..start + 480].fill(0.8);
        }
        samples
    }

    #[test]
    fn finds_each_click_to_the_millisecond() {
        let clicks = find_clicks(&with_clicks(48_000.0, 10, &[1500, 7250]), 48_000.0);
        assert_eq!(clicks.len(), 2);
        assert!((clicks[0] - 1500.0).abs() <= 1.0, "{clicks:?}");
        assert!((clicks[1] - 7250.0).abs() <= 1.0, "{clicks:?}");
    }

    #[test]
    fn a_click_with_a_tail_counts_once() {
        let mut samples = with_clicks(48_000.0, 3, &[1000]);
        samples[48_480..50_000].fill(0.6);
        assert_eq!(find_clicks(&samples, 48_000.0).len(), 1);
    }

    #[test]
    fn digital_silence_has_no_clicks() {
        assert!(find_clicks(&[0.0; 48_000], 48_000.0).is_empty());
    }

    #[test]
    fn non_integer_hop_rates_keep_time() {
        let clicks = find_clicks(&with_clicks(44_100.0, 10, &[9000]), 44_100.0);
        assert!((clicks[0] - 9000.0).abs() <= 1.0, "{clicks:?}");
    }
}

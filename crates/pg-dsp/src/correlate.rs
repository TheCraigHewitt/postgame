//! Normalized cross-correlation, used to confirm a capture matches its source.

/// Slides `needle` across `haystack` and returns the best normalized
/// cross-correlation, in -1 to 1. Gain differences have no effect.
pub fn best_ncc(haystack: &[f32], needle: &[f32]) -> f32 {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0.0;
    }
    let needle_energy: f64 = needle.iter().map(|s| (*s as f64).powi(2)).sum();
    if needle_energy == 0.0 {
        return 0.0;
    }
    let mut window_energy: f64 = haystack[..needle.len()]
        .iter()
        .map(|s| (*s as f64).powi(2))
        .sum();
    let mut best = 0.0_f64;
    for start in 0..=haystack.len() - needle.len() {
        let dot: f64 = haystack[start..]
            .iter()
            .zip(needle)
            .map(|(a, b)| *a as f64 * *b as f64)
            .sum();
        if window_energy > 0.0 {
            best = best.max(dot / (window_energy * needle_energy).sqrt());
        }
        if let Some(next) = haystack.get(start + needle.len()) {
            window_energy += (*next as f64).powi(2) - (haystack[start] as f64).powi(2);
        }
    }
    best as f32
}

/// Averages each run of `factor` samples into one.
pub fn decimate(samples: &[f32], factor: usize) -> Vec<f32> {
    samples
        .chunks_exact(factor)
        .map(|c| c.iter().sum::<f32>() / factor as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| ((i * i % 97) as f32 / 48.0 - 1.0) * 0.5)
            .collect()
    }

    #[test]
    fn finds_a_quiet_copy_inside_a_longer_signal() {
        let source = signal(2000);
        let copy: Vec<f32> = source[500..1200].iter().map(|s| s * 0.1).collect();
        assert!(best_ncc(&source, &copy) > 0.999);
    }

    #[test]
    fn unrelated_signals_score_low() {
        let a = signal(1000);
        let b: Vec<f32> = (0..300).map(|i| (i as f32 * 0.37).sin()).collect();
        assert!(best_ncc(&a, &b) < 0.5);
    }

    #[test]
    fn silence_scores_zero() {
        assert_eq!(best_ncc(&signal(100), &[0.0; 50]), 0.0);
        assert_eq!(best_ncc(&[0.0; 100], &signal(50)), 0.0);
    }

    #[test]
    fn a_needle_longer_than_the_haystack_scores_zero() {
        assert_eq!(best_ncc(&signal(10), &signal(20)), 0.0);
    }

    #[test]
    fn decimate_averages_and_drops_the_tail() {
        assert_eq!(decimate(&[1.0, 3.0, 5.0, 7.0, 9.0], 2), vec![2.0, 6.0]);
    }
}

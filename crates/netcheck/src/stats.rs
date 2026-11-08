//! CONCEPT: turn a list of RTT samples (milliseconds) into min / p50 / p95 / max, the RttStats shape of §6.9.
//! WHY: WP-10 R10.3. One summary function so connect-probe RTT and echo RTT report the same way, and so the
//!      percentile math has unit tests (it feeds the WP-5 speed estimate, R5.12).
//! DEPENDS ON: nothing (pure math).
//! AFFECTS: result.rs (RttStats), report.rs (the table), WP-5 (reads p50/p95 from the JSON).
//!
//! Percentiles use linear interpolation between closest ranks (numpy's default, "type 7"). So p50 of an even
//! count is the mean of the two middle samples, i.e. the textbook median.
//! Rejected: nearest-rank. With 20 samples its p95 is just the 19th value and jumps in big steps; interpolation
//! moves smoothly as samples change, which makes two runs easier to compare.

/// RTT summary in milliseconds. `samples` counts the successful samples the numbers come from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub min: f64,
    pub p50: f64,
    pub p95: f64,
    pub max: f64,
    pub samples: u32,
}

/// Percentile `p` (0..=100) of an already sorted, non-empty slice, linear interpolation.
/// Panics on an empty slice: callers go through [`summarize`], which handles that case.
pub fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    assert!(!sorted.is_empty(), "percentile of an empty slice");
    let p = p.clamp(0.0, 100.0);
    let h = (sorted.len() - 1) as f64 * p / 100.0;
    let lo = h.floor() as usize;
    let hi = h.ceil() as usize;
    sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo])
}

/// Summarize RTT samples. `None` when there are no samples (every attempt failed).
/// NaN samples are dropped: they can't come from a clock, so they would only be a bug upstream.
pub fn summarize(samples_ms: &[f64]) -> Option<Summary> {
    let mut v: Vec<f64> = samples_ms.iter().copied().filter(|x| !x.is_nan()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    Some(Summary {
        min: v[0],
        p50: percentile_sorted(&v, 50.0),
        p95: percentile_sorted(&v, 95.0),
        max: v[v.len() - 1],
        samples: v.len() as u32,
    })
}

/// Throughput in megabits per second (10^6 bits), the unit of §6.9 `throughput_mbps`.
pub fn mbit_per_s(bytes: u64, secs: f64) -> f64 {
    if secs <= 0.0 {
        0.0
    } else {
        bytes as f64 * 8.0 / 1e6 / secs
    }
}

/// Throughput in megabytes per second (10^6 bytes), the unit the table prints.
pub fn mbyte_per_s(bytes: u64, secs: f64) -> f64 {
    if secs <= 0.0 {
        0.0
    } else {
        bytes as f64 / 1e6 / secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_none() {
        assert_eq!(summarize(&[]), None);
        assert_eq!(summarize(&[f64::NAN]), None);
    }

    #[test]
    fn single_sample() {
        let s = summarize(&[3.5]).unwrap();
        assert_eq!(
            (s.min, s.p50, s.p95, s.max, s.samples),
            (3.5, 3.5, 3.5, 3.5, 1)
        );
    }

    #[test]
    fn odd_count_median_is_middle() {
        let s = summarize(&[5.0, 1.0, 3.0]).unwrap();
        assert_eq!(s.p50, 3.0);
        assert_eq!(s.min, 1.0);
        assert_eq!(s.max, 5.0);
    }

    #[test]
    fn even_count_median_is_mean_of_middle_two() {
        let s = summarize(&[4.0, 1.0, 3.0, 2.0]).unwrap();
        assert_eq!(s.p50, 2.5);
    }

    #[test]
    fn p95_of_1_to_100() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        let s = summarize(&v).unwrap();
        assert!((s.p95 - 95.05).abs() < 1e-9, "p95 = {}", s.p95);
        assert_eq!(s.samples, 100);
    }

    #[test]
    fn p95_picks_up_one_outlier_in_twenty() {
        // 19 fast samples and one 50 ms spike: p95 must sit between them, max must be the spike.
        let mut v = vec![1.0; 19];
        v.push(50.0);
        let s = summarize(&v).unwrap();
        assert_eq!(s.p50, 1.0);
        assert!(s.p95 > 1.0 && s.p95 < 50.0, "p95 = {}", s.p95);
        assert_eq!(s.max, 50.0);
    }

    #[test]
    fn nan_is_dropped() {
        let s = summarize(&[1.0, f64::NAN, 3.0]).unwrap();
        assert_eq!(s.samples, 2);
        assert_eq!(s.p50, 2.0);
    }

    #[test]
    fn throughput_units() {
        // 125 MB in 1 s = 125 MB/s = 1000 Mbit/s.
        assert!((mbyte_per_s(125_000_000, 1.0) - 125.0).abs() < 1e-9);
        assert!((mbit_per_s(125_000_000, 1.0) - 1000.0).abs() < 1e-9);
        assert_eq!(mbit_per_s(1, 0.0), 0.0);
    }
}

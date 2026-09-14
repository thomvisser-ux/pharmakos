// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(clippy::float_arithmetic)]
//! Integer percentiles. Shared by the in-engine run and `meshbench` so the two
//! report the same statistic under the same name.
//!
//! Nearest-rank on a sorted slice: `p99` of 100 samples is the 99th, not an
//! interpolation between the 99th and the 100th. No floats — a percentile
//! computed two different ways is a number nobody can compare across runs.

/// `p` in per-mille (500 = p50, 990 = p99). Returns 0 for an empty slice.
pub fn pct_sorted<T: Copy + Default>(sorted: &[T], p_permille: u64) -> T {
    if sorted.is_empty() {
        return T::default();
    }
    let n = sorted.len() as u64;
    // Nearest-rank: ceil(p * n / 1000), clamped to 1..=n, then 0-based.
    let rank = (p_permille * n).div_ceil(1000).clamp(1, n);
    sorted[(rank - 1) as usize]
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub n: u64,
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
    pub max: u64,
    pub mean: u64,
}

pub fn summarise(values: &mut [u64]) -> Summary {
    if values.is_empty() {
        return Summary::default();
    }
    values.sort_unstable();
    let sum: u128 = values.iter().map(|&v| u128::from(v)).sum();
    Summary {
        n: values.len() as u64,
        p50: pct_sorted(values, 500),
        p90: pct_sorted(values, 900),
        p99: pct_sorted(values, 990),
        max: *values.last().unwrap_or(&0),
        mean: u64::try_from(sum / (values.len() as u128)).unwrap_or(u64::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_percentiles() {
        let mut v: Vec<u64> = (1..=100).collect();
        let s = summarise(&mut v);
        assert_eq!(s.p50, 50);
        assert_eq!(s.p90, 90);
        assert_eq!(s.p99, 99);
        assert_eq!(s.max, 100);
        assert_eq!(s.mean, 50); // 5050/100 = 50.5 -> 50 by integer division
    }

    #[test]
    fn empty_and_single() {
        assert_eq!(summarise(&mut []), Summary::default());
        let s = summarise(&mut [7]);
        assert_eq!((s.p50, s.p99, s.max, s.n), (7, 7, 7, 1));
    }
}

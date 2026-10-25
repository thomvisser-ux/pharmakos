// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Percentiles, means and the way a duration is written in a notice.
//!
//! Integer nanoseconds throughout. Floats would be legal here (this crate is
//! walled), but nothing below needs one: a nearest-rank percentile is an index,
//! and a microsecond figure with three decimals is a division and a remainder.
//!
//! An empty sample has no percentile and no mean, and says so with `None`
//! rather than answering zero: a zero is a measurement, and an empty sample is
//! the absence of one (S2's plan, E7).

use crate::error::Error;

/// The value at `percent` (at most 100) of a sorted sample, by nearest rank,
/// or `None` for an empty sample or a percent above 100.
///
/// Nearest rank and not interpolation: the answer is always a duration that was
/// actually measured, which is what a reader comparing two runs expects.
#[must_use]
pub fn percentile(sorted: &[u128], percent: u8) -> Option<u128> {
    if percent > 100 {
        return None;
    }
    // ceil(len * percent / 100), at least 1, as a one-based rank.
    let rank = sorted
        .len()
        .checked_mul(usize::from(percent))?
        .checked_add(99)?
        .div_euclid(100)
        .max(1);
    sorted.get(rank.checked_sub(1)?).copied()
}

/// The mean of a sample, rounded down, or `None` for an empty one.
#[must_use]
pub fn mean(sample: &[u128]) -> Option<u128> {
    // A `usize` fits a `u128` on every target Rust builds for.
    let count = u128::try_from(sample.len()).ok()?;
    let sum = sample
        .iter()
        .try_fold(0_u128, |sum, value| sum.checked_add(*value))?;
    sum.checked_div(count)
}

/// The two figures P1 publishes for a sample of durations, and the mean and
/// maximum G3′ adds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Summary {
    /// The median, in nanoseconds.
    pub p50_ns: u128,
    /// The 99th percentile, in nanoseconds.
    pub p99_ns: u128,
    /// The mean, in nanoseconds, rounded down.
    pub mean_ns: u128,
    /// The largest, in nanoseconds.
    pub max_ns: u128,
    /// How many durations the figures were taken over.
    pub samples: usize,
}

impl Summary {
    /// Sort `durations` and summarise them.
    ///
    /// # Errors
    ///
    /// [`Error::Empty`] for an empty sample, which has no figures.
    pub fn of(mut durations: Vec<u128>) -> Result<Summary, Error> {
        durations.sort_unstable();
        let empty = || Error::Empty(format!("a summary of {} durations", durations.len()));
        Ok(Summary {
            p50_ns: percentile(&durations, 50).ok_or_else(empty)?,
            p99_ns: percentile(&durations, 99).ok_or_else(empty)?,
            mean_ns: mean(&durations).ok_or_else(empty)?,
            max_ns: durations.last().copied().ok_or_else(empty)?,
            samples: durations.len(),
        })
    }
}

/// A duration in nanoseconds, written as microseconds with three decimals:
/// `1234567` is `1234.567 us`.
#[must_use]
pub fn micros(ns: u128) -> String {
    let whole = ns.div_euclid(1_000);
    let fraction = ns.rem_euclid(1_000);
    format!("{whole}.{fraction:03} us")
}

/// A duration in nanoseconds, written as milliseconds with three decimals:
/// `12345678` is `12.345 ms`.
#[must_use]
pub fn millis(ns: u128) -> String {
    let micros = ns.div_euclid(1_000);
    let whole = micros.div_euclid(1_000);
    let fraction = micros.rem_euclid(1_000);
    format!("{whole}.{fraction:03} ms")
}

/// `part` as per mille of `whole`, rounded down, or `None` when `whole` is
/// zero (a share of nothing is not a share).
#[must_use]
pub fn per_mille(part: u128, whole: u128) -> Option<u128> {
    part.checked_mul(1_000)?.checked_div(whole)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_answers_a_measured_value() {
        let sorted: Vec<u128> = (1..=100).collect();
        assert_eq!(percentile(&sorted, 50), Some(50));
        assert_eq!(percentile(&sorted, 99), Some(99));
        assert_eq!(percentile(&sorted, 100), Some(100));
        assert_eq!(percentile(&sorted, 0), Some(1));
        assert_eq!(percentile(&[7], 99), Some(7));
        assert_eq!(percentile(&[], 99), None);
        // Ten values: the 99th percentile is the largest, not an interpolation.
        let ten: Vec<u128> = (1..=10).collect();
        assert_eq!(percentile(&ten, 99), Some(10));
        assert_eq!(percentile(&ten, 50), Some(5));
        assert_eq!(percentile(&ten, 250), None, "a percent past 100 is refused");
    }

    #[test]
    fn a_summary_sorts_before_it_ranks() {
        let summary = Summary::of(vec![30, 10, 20]).expect("three samples");
        assert_eq!(summary.p50_ns, 20);
        assert_eq!(summary.p99_ns, 30);
        assert_eq!(summary.mean_ns, 20);
        assert_eq!(summary.max_ns, 30);
        assert_eq!(summary.samples, 3);
        assert!(matches!(Summary::of(Vec::new()), Err(Error::Empty(_))));
    }

    #[test]
    fn a_mean_of_nothing_is_none() {
        assert_eq!(mean(&[]), None);
        assert_eq!(mean(&[1, 2]), Some(1));
        assert_eq!(per_mille(1, 4), Some(250));
        assert_eq!(per_mille(1, 0), None);
    }

    #[test]
    fn durations_keep_three_decimals() {
        assert_eq!(micros(1_234_567), "1234.567 us");
        assert_eq!(micros(5), "0.005 us");
        assert_eq!(micros(0), "0.000 us");
        assert_eq!(millis(12_345_678), "12.345 ms");
        assert_eq!(millis(999), "0.000 ms");
    }
}

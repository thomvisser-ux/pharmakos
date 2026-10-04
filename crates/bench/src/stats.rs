// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Percentiles and the way a duration is written in a notice.
//!
//! Integer nanoseconds throughout. Floats would be legal here (this crate is
//! walled), but nothing below needs one: a nearest-rank percentile is an index,
//! and a microsecond figure with three decimals is a division and a remainder.

/// The value at `percent` of a sorted sample, by nearest rank.
///
/// Nearest rank and not interpolation: the answer is always a duration that was
/// actually measured, which is what a reader comparing two runs expects. An
/// empty sample answers zero rather than panicking, and a percent above 100 is
/// read as 100.
#[must_use]
pub fn percentile(sorted: &[u128], percent: u32) -> u128 {
    let Some(last) = sorted.len().checked_sub(1) else {
        return 0;
    };
    let percent = usize::try_from(percent.min(100)).unwrap_or(100);
    // ceil(len * percent / 100) - 1, clamped into the sample.
    let rank = sorted
        .len()
        .saturating_mul(percent)
        .saturating_add(99)
        .checked_div(100)
        .unwrap_or(0)
        .saturating_sub(1)
        .min(last);
    sorted.get(rank).copied().unwrap_or(0)
}

/// The two figures P1 publishes for a sample of durations.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Summary {
    /// The median, in nanoseconds.
    pub p50_ns: u128,
    /// The 99th percentile, in nanoseconds.
    pub p99_ns: u128,
    /// How many durations the two were taken over.
    pub samples: usize,
}

impl Summary {
    /// Sort `durations` and summarise them.
    #[must_use]
    pub fn of(mut durations: Vec<u128>) -> Summary {
        durations.sort_unstable();
        Summary {
            p50_ns: percentile(&durations, 50),
            p99_ns: percentile(&durations, 99),
            samples: durations.len(),
        }
    }
}

/// A duration in nanoseconds, written as microseconds with three decimals:
/// `1234567` is `1234.567 us`.
#[must_use]
pub fn micros(ns: u128) -> String {
    let whole = ns.checked_div(1_000).unwrap_or(0);
    let fraction = ns.checked_rem(1_000).unwrap_or(0);
    format!("{whole}.{fraction:03} us")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_answers_a_measured_value() {
        let sorted: Vec<u128> = (1..=100).collect();
        assert_eq!(percentile(&sorted, 50), 50);
        assert_eq!(percentile(&sorted, 99), 99);
        assert_eq!(percentile(&sorted, 100), 100);
        assert_eq!(percentile(&sorted, 0), 1);
        assert_eq!(percentile(&[7], 99), 7);
        assert_eq!(percentile(&[], 99), 0);
        // Ten values: the 99th percentile is the largest, not an interpolation.
        let ten: Vec<u128> = (1..=10).collect();
        assert_eq!(percentile(&ten, 99), 10);
        assert_eq!(percentile(&ten, 50), 5);
        assert_eq!(percentile(&ten, 250), 10);
    }

    #[test]
    fn a_summary_sorts_before_it_ranks() {
        let summary = Summary::of(vec![30, 10, 20]);
        assert_eq!(summary.p50_ns, 20);
        assert_eq!(summary.p99_ns, 30);
        assert_eq!(summary.samples, 3);
    }

    #[test]
    fn micros_keep_three_decimals() {
        assert_eq!(micros(1_234_567), "1234.567 us");
        assert_eq!(micros(5), "0.005 us");
        assert_eq!(micros(0), "0.000 us");
    }
}

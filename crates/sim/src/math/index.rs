// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Row indexes: the one audited widening from a table's `u32` row or count to
//! the `usize` a column is indexed by (S2's plan, E7).
//!
//! Every table in the sim counts its rows in `u32`, because the canonical
//! encoding and the snapshot write fixed-width counts, and every column is a
//! `Vec` indexed by `usize`. The conversion between the two was written
//! `usize::try_from(row).unwrap_or(usize::MAX)` at about two hundred sites: a
//! fallback that can never be taken on any target this crate builds for, and
//! that would, if it ever were, read "no such row" rather than say what went
//! wrong. So the failure is made unrepresentable instead: the assertion below
//! refuses to compile the crate for a target whose `usize` is narrower than
//! `u32`, and [`row`] is then a widening that cannot truncate.
//!
//! This is AGENTS.md section 4.3's rule, "the handful of legitimate widening
//! casts live in the sim's math module behind named constructors", applied to
//! the commonest conversion in the crate.

/// A crate whose rows are `u32` needs a `usize` at least as wide.
const _: () = assert!(usize::BITS >= u32::BITS);

/// A `u32` row, id or count as a column index.
///
/// Infallible: the module's assertion keeps the crate from building where the
/// widening could truncate.
#[must_use]
#[allow(
    clippy::as_conversions,
    reason = "the audited widening of AGENTS.md section 4.3: u32 -> usize cannot truncate where the module's assertion lets the crate build, and a const fn cannot call usize::from"
)]
pub const fn row(value: u32) -> usize {
    value as usize
}

#[cfg(test)]
mod tests {
    use super::row;

    #[test]
    fn a_row_widens_without_loss() {
        assert_eq!(row(0), 0);
        assert_eq!(row(4_095), 4_095);
        assert_eq!(
            Some(row(u32::MAX)),
            usize::try_from(u32::MAX).ok(),
            "the largest row is itself"
        );
    }
}

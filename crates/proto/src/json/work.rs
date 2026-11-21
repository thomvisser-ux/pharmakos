// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A meter of the bytes the readers and writers of a player's text touch
//! (S3's `vfyq2`, fix pass 1).
//!
//! The readers are parsers of untrusted input, so their work must be bounded
//! by a constant per byte of input: a scan that starts again from some early
//! byte for every token is quadratic, and a 256 KiB frame then costs billions
//! of byte reads (the security review found one, in the comment blanker).
//! A clock cannot hold that in a deterministic crate, and a count of calls
//! cannot see a call that reads too far. So every scanner reads through the
//! grammar's `Scan`, which adds what it reads here, and the canonical writers
//! add what they write. Tests read the meter around one call and hold its
//! total to a constant times the input's length, at sizes far apart
//! (`plan-core`'s `verify::linearity`).
//!
//! The meter is per thread and only ever added to; it reads nothing back into
//! any answer. It cannot fail to count, and it cannot overflow by its type
//! (S3's `vfyq2`, fix pass 3, item L8): the count is a `u128`, an add is at
//! most `usize::MAX`, which is at most 2^64 - 1 (asserted at compile time
//! below), so the count passes `u128::MAX` only after more than 2^64 adds
//! since the last [`take`], and an add is one read of the text: 2^64 of them,
//! at one a nanosecond, take over five hundred years. Every test takes the
//! meter around the one call it measures. So the add wraps explicitly
//! (`wrapping_add`, which by the bound never does), and [`take`] answers the
//! count with no error to report.

use std::cell::Cell;

// The bound the doc above rests on: an add is at most 2^64 - 1.
const _: () = assert!(
    usize::BITS <= 64,
    "an add must fit 64 bits for the meter's bound"
);

thread_local! {
    /// The bytes examined on this thread since the last [`take`].
    static EXAMINED: Cell<u128> = const { Cell::new(0) };
}

/// `bytes`, widened to the count's width: its little-endian bytes in the low
/// half of a `u128`'s (no cast; `usize::BITS <= 64` is asserted above).
fn wide(bytes: usize) -> u128 {
    let mut low = [0_u8; 16];
    let (head, _) = low.split_at_mut(core::mem::size_of::<usize>());
    head.copy_from_slice(&bytes.to_le_bytes());
    u128::from_le_bytes(low)
}

/// Count `bytes` examined.
pub fn add(bytes: usize) {
    EXAMINED.with(|examined| examined.set(examined.get().wrapping_add(wide(bytes))));
}

/// The bytes examined on this thread since the last call, and the meter set
/// back to zero.
#[must_use]
pub fn take() -> u128 {
    EXAMINED.with(|examined| examined.replace(0))
}

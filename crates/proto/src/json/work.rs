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
//! cannot see a call that reads too far. So every scanner adds the bytes it
//! actually examined here: the grammar's tokens and whitespace, the comment
//! scanner, the blanker and the canonical writers. Tests read the meter
//! around one call and hold its total to a constant times the input's length,
//! at two sizes far apart (`plan-core`'s `verify::linearity`).
//!
//! The meter is per thread and only ever added to; it reads nothing back into
//! any answer. Adding is one thread-local addition per token or run, not per
//! byte.

use std::cell::Cell;

thread_local! {
    /// The bytes examined on this thread since the last [`take`].
    static EXAMINED: Cell<u64> = const { Cell::new(0) };
}

/// Count `bytes` examined. Saturates, because the meter is a measure and
/// never an answer: a count past `u64::MAX` reads as `u64::MAX`, which every
/// bound refuses.
pub fn add(bytes: usize) {
    let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
    let _ = EXAMINED.try_with(|examined| examined.set(examined.get().saturating_add(bytes)));
}

/// The bytes examined on this thread since the last call, and the meter set
/// back to zero.
#[must_use]
pub fn take() -> u64 {
    EXAMINED
        .try_with(|examined| examined.replace(0))
        .unwrap_or(0)
}

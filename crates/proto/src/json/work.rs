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
//! any answer. It cannot fail to count (fix pass 2, item D2): the key is
//! `const`-initialised and holds a type with no destructor, so reading it
//! never fails, and a count past `usize::MAX` is not saturated but remembered
//! as overflowed, which [`take`] answers as a typed error.

use std::cell::Cell;

thread_local! {
    /// The bytes examined on this thread since the last [`take`], or `None`
    /// once the count has passed `usize::MAX`.
    static EXAMINED: Cell<Option<usize>> = const { Cell::new(Some(0)) };
}

/// The meter's count passed `usize::MAX` since the last [`take`]: a measure
/// that large is no measure, and a test reads it as a failure.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Overflowed;

/// Count `bytes` examined.
pub fn add(bytes: usize) {
    EXAMINED.with(|examined| {
        examined.set(examined.get().and_then(|count| count.checked_add(bytes)));
    });
}

/// The bytes examined on this thread since the last call, and the meter set
/// back to zero.
///
/// # Errors
///
/// [`Overflowed`] when the count passed `usize::MAX`.
pub fn take() -> Result<usize, Overflowed> {
    EXAMINED.with(|examined| examined.replace(Some(0)).ok_or(Overflowed))
}

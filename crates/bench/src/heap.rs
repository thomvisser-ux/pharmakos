// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a counting global allocator tells the G3′ segment (item 68; spike
//! step 6's drift check).
//!
//! The allocator itself lives in the test binary that measures
//! (`tests/g3_real.rs`): a `#[global_allocator]` is a property of a binary,
//! and writing one needs `unsafe`, which the workspace denies in library code.
//! This crate reads it through [`HeapCounter`], and a run without one says the
//! heap was not counted rather than printing a zero.

/// A counting allocator, as the segment reads it.
pub trait HeapCounter {
    /// Bytes allocated and not yet freed, process-wide.
    fn live_bytes(&self) -> u64;
    /// Allocations made by the calling thread since it started.
    fn allocations(&self) -> u64;
}

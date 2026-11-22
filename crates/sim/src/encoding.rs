// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The canonical byte encoding and the one hash seed (decisions log item 48).
//!
//! The hasher never sees a struct. It sees a byte string built by walking each
//! table in declared order and appending every field as fixed-width
//! little-endian bytes. Padding, `#[repr]`, field order in memory and pointer
//! width are irrelevant **by construction**, which is what makes the hash a
//! cross-OS contract rather than a same-binary artefact.
//!
//! Lengths are `u32`; `usize` never enters the hash.
//!
//! # Contract values
//!
//! | Constant | Value | What moves if it moves |
//! |---|---|---|
//! | [`STATE_HASH_SEED`] | `0x5048_4152_4D4B_4F53` (`b"PHARMKOS"`) | every golden file in the project |
//! | [`ENCODING_VERSION`] | `1`, pushed as the first byte of every encoder | every golden file in the project |
//!
//! Pinned regression values, asserted in `tests/vectors.rs` as literals:
//! `digest(b"") == 0xFE1A_F732_B028_10AD` and
//! `digest(b"pharmakos/g4") == 0xE170_26C8_A5EA_4D30`.
//!
//! The one hash function has two shapes, and only this module names either:
//! [`digest`] over bytes held whole, and [`StreamingDigest`] over bytes handed
//! over in pieces, which answers what [`digest`] answers for the pieces
//! joined, whatever the cut (`tests/vectors.rs` holds the two equal at every
//! length to 300, across xxh3's block boundaries and under every chunking it
//! tries). The streaming shape exists for the plan fingerprint (S3's
//! `fpstream`), whose canonical text is hashed as it is written and never
//! held whole.

use xxhash_rust::xxh3::{Xxh3, xxh3_64_with_seed};

/// The one compiled-in hash seed: the ASCII bytes `PHARMKOS`, big-endian in
/// the literal so the constant reads as the word.
///
/// There is exactly one seed constant in the crate and this is it. Changing it
/// invalidates every golden file that was ever stamped with it, so it is a
/// contract change (AGENTS.md §5).
pub const STATE_HASH_SEED: u64 = 0x5048_4152_4D4B_4F53;

/// Version byte of the canonical encoding, pushed first by every construction
/// path. A bump moves every hash in the project and needs owner approval.
pub const ENCODING_VERSION: u8 = 1;

/// An append-only canonical encoder over a reusable buffer.
///
/// Item 66 decided the per-tick hash is **full**: every table is re-encoded
/// every tick, in declared order, into a buffer that is allocated once and
/// reused. [`Enc::clear`] is what makes that allocation-free, and it is also
/// why `Default` is written by hand below rather than derived — a derived
/// `Default` would hand back an empty buffer, and an encoder missing its
/// version byte hashes the same world to a different value than one built the
/// documented way, a divergence that is invisible at the call site.
#[derive(Debug)]
pub struct Enc {
    buf: Vec<u8>,
}

impl Default for Enc {
    fn default() -> Enc {
        Enc::with_capacity(0)
    }
}

impl Enc {
    /// A new encoder with room for `capacity` bytes, already carrying its
    /// version byte.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Enc {
        let mut enc = Enc {
            buf: Vec::with_capacity(capacity),
        };
        enc.buf.push(ENCODING_VERSION);
        enc
    }

    /// Reset to just the version byte, keeping the allocation.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.buf.push(ENCODING_VERSION);
    }

    /// Append one byte.
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    /// Append a boolean as one byte, `0` or `1`.
    pub fn bool(&mut self, v: bool) {
        self.buf.push(u8::from(v));
    }

    /// Append a little-endian `u16`.
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Append a little-endian `u32`.
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Append a little-endian `u64`.
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Append a little-endian `i32`.
    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Append a little-endian `i64`.
    pub fn i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Append an `Option<u32>` as a one-byte tag followed by four bytes.
    ///
    /// The absent arm still writes four zero bytes, so the encoding is
    /// **fixed-stride** and a field can never be confused with its neighbour.
    pub fn opt_u32(&mut self, v: Option<u32>) {
        if let Some(x) = v {
            self.buf.push(1);
            self.buf.extend_from_slice(&x.to_le_bytes());
        } else {
            self.buf.push(0);
            self.buf.extend_from_slice(&0u32.to_le_bytes());
        }
    }

    /// Append a collection length.
    ///
    /// Counts in this crate are `u32` at rest (the `SoA` tables fix them at
    /// construction), so nothing has to convert a `usize` to reach this.
    pub fn len(&mut self, n: u32) {
        self.buf.extend_from_slice(&n.to_le_bytes());
    }

    /// Append raw bytes. Used only by callers that have already fixed the
    /// stride themselves — a per-chunk digest block, for instance.
    pub fn bytes(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }

    /// The canonical bytes built so far.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    /// How many bytes the encoding currently occupies, version byte included.
    #[must_use]
    pub fn encoded_len(&self) -> usize {
        self.buf.len()
    }

    /// Finish: xxh3-64 over the canonical bytes at [`STATE_HASH_SEED`].
    #[must_use]
    pub fn finish(&self) -> u64 {
        xxh3_64_with_seed(&self.buf, STATE_HASH_SEED)
    }
}

/// xxh3-64 of an arbitrary byte slice at [`STATE_HASH_SEED`].
///
/// Used for the per-chunk digests, the rules hash and file identity. There is
/// no second hash function in this project (AGENTS.md §5).
#[must_use]
pub fn digest(bytes: &[u8]) -> u64 {
    xxh3_64_with_seed(bytes, STATE_HASH_SEED)
}

/// [`digest`], fed in pieces: xxh3-64 at [`STATE_HASH_SEED`] over every byte
/// handed to [`StreamingDigest::update`], in order, as if they had been one
/// slice.
///
/// The same function as [`digest`], not a second one: xxh3's own streaming
/// state, seeded with the one seed, which answers the one-shot value for any
/// cut of the same bytes. It holds a fixed state of a few hundred bytes and
/// allocates nothing, so a text hashed through it is never held whole.
#[derive(Clone)]
pub struct StreamingDigest {
    state: Xxh3,
}

impl Default for StreamingDigest {
    fn default() -> StreamingDigest {
        StreamingDigest::new()
    }
}

impl core::fmt::Debug for StreamingDigest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // xxh3's state has no `Debug`, and its bytes say nothing a reader
        // could use; the value so far is what the state means.
        f.debug_struct("StreamingDigest")
            .field("so_far", &hex(self.finish()))
            .finish()
    }
}

impl StreamingDigest {
    /// A digest over no bytes yet, at [`STATE_HASH_SEED`].
    #[must_use]
    pub fn new() -> StreamingDigest {
        StreamingDigest {
            state: Xxh3::with_seed(STATE_HASH_SEED),
        }
    }

    /// Hand over the next bytes.
    pub fn update(&mut self, bytes: &[u8]) {
        self.state.update(bytes);
    }

    /// [`digest`] of every byte handed over so far, joined. The state is kept,
    /// so more bytes may follow.
    #[must_use]
    pub fn finish(&self) -> u64 {
        self.state.digest()
    }
}

/// Lowercase, zero-padded, 16 hex digits — the only rendering of a hash in the
/// project, and the format `tests/golden/determinism/expected.hashes.txt` and
/// `cargo xtask ci`'s `determinism` step both read.
#[must_use]
pub fn hex(h: u64) -> String {
    format!("{h:016x}")
}

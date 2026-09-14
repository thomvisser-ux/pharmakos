// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Canonical byte encoding and the per-tick state hash.
//!
//! The hasher never sees a struct. It sees a byte string built by walking each
//! SoA table in id order and appending every field as fixed-width little-endian
//! bytes. Padding, `#[repr]`, field order in memory and pointer width are
//! irrelevant *by construction*, which is what makes the hash a candidate for a
//! cross-OS contract rather than a same-binary artefact.
//!
//! Lengths are hashed as `u32` after a checked conversion; `usize` never enters
//! the hash.

use xxhash_rust::xxh3::xxh3_64_with_seed;

/// The one compiled-in hash seed.
///
/// Value: the ASCII bytes `PHARMKOS`, big-endian in the literal so the constant
/// reads as the word. Fixed at day 1 of the spike and never changed — changing
/// it invalidates every golden file that was ever stamped with it. There is
/// exactly one seed constant in the crate, and this is it.
pub const STATE_HASH_SEED: u64 = 0x5048_4152_4D4B_4F53;

/// Version byte of the canonical encoding. Bump only with the owner's approval;
/// a bump moves every hash in the project.
pub const ENCODING_VERSION: u8 = 1;

/// Append-only canonical encoder.
#[derive(Debug, Default)]
pub struct Enc {
    buf: Vec<u8>,
}

impl Enc {
    #[must_use]
    pub fn with_capacity(n: usize) -> Enc {
        let mut e = Enc { buf: Vec::with_capacity(n) };
        e.buf.push(ENCODING_VERSION);
        e
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.buf.push(ENCODING_VERSION);
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn bool(&mut self, v: bool) {
        self.buf.push(u8::from(v));
    }

    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// `Option<u32>`: a one-byte tag then, when present, the value. The absent
    /// arm still writes four zero bytes so the encoding is fixed-stride and a
    /// field can never be confused with its neighbour.
    pub fn opt_u32(&mut self, v: Option<u32>) {
        match v {
            Some(x) => {
                self.buf.push(1);
                self.buf.extend_from_slice(&x.to_le_bytes());
            }
            None => {
                self.buf.push(0);
                self.buf.extend_from_slice(&0u32.to_le_bytes());
            }
        }
    }

    /// A collection length. `usize` is converted, checked, and hashed as `u32`.
    pub fn len(&mut self, n: usize) {
        let v = u32::try_from(n).expect("length exceeds u32 in canonical encoding");
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Finish: xxh3-64 over the canonical bytes at the project seed.
    #[must_use]
    pub fn finish(&self) -> u64 {
        xxh3_64_with_seed(&self.buf, STATE_HASH_SEED)
    }
}

/// xxh3-64 of an arbitrary byte slice at the project seed. Used for the trace
/// digest and for snapshot-file identity.
#[must_use]
pub fn digest(bytes: &[u8]) -> u64 {
    xxh3_64_with_seed(bytes, STATE_HASH_SEED)
}

/// Lowercase, zero-padded, 16 hex digits. The only hash rendering in the spike.
#[must_use]
pub fn hex(h: u64) -> String {
    format!("{h:016x}")
}

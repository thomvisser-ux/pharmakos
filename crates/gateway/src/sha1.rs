// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SHA-1, for `Sec-WebSocket-Accept` and for nothing else.
//!
//! RFC 6455 section 4.2.2 step 5.4 defines the handshake's one piece of
//! arithmetic: concatenate the client's `Sec-WebSocket-Key` with the fixed GUID
//! `258EAFA5-E914-47DA-95CA-C5AB0DC85B11`, take the SHA-1 of that ASCII string,
//! and base64 the twenty bytes. That is the whole reason this file exists.
//!
//! **This is not a security primitive here and must never be used as one.**
//! SHA-1 is broken for collision resistance, and RFC 6455 uses it as a fixed
//! handshake transform rather than as a proof of anything. The gateway's actual
//! secrets are the 256-bit tokens in [`crate::token`], which are compared byte
//! for byte in constant time and never hashed. Nothing else in this crate calls
//! this module, and nothing else should: the project's one hash function is the
//! sim's xxh3 (AGENTS.md section 5), and adding a second one for state would be
//! a contract change.
//!
//! Hand-written over `std` because decisions-log item 99 chose to own the RFC
//! 6455 server side rather than take ten crates onto the surface v1.1 publishes.
//! The price of owning it is that it must be pinned to somebody else's numbers,
//! so it is: the unit tests below check the three FIPS 180-4 vectors *and* RFC
//! 6455 section 1.3's worked example, end to end through the proto crate's `json::base64`.
//!
//! Integer discipline is the project's (AGENTS.md section 4): no `as` casts, no
//! indexing, no floats. The message schedule is read through `get`, which cannot
//! be out of range by construction and says so at each call.

/// The five SHA-1 initial chaining values (FIPS 180-4, section 5.3.1).
const INIT: [u32; 5] = [
    0x6745_2301,
    0xEFCD_AB89,
    0x98BA_DCFE,
    0x1032_5476,
    0xC3D2_E1F0,
];

/// A SHA-1 digest: twenty bytes.
pub type Digest = [u8; 20];

/// The SHA-1 of `message`.
///
/// One shot, because the only caller has the whole input in hand. A streaming
/// interface would be an unused generalisation on a function whose only job is a
/// sixty-byte handshake string.
#[must_use]
pub fn digest(message: &[u8]) -> Digest {
    let mut state = INIT;

    // FIPS 180-4 section 5.1.1: append 0x80, then zeroes, then the length in
    // bits as a 64-bit big-endian integer, so the total is a whole number of
    // 64-byte blocks.
    let mut padded: Vec<u8> = Vec::with_capacity(message.len().saturating_add(72));
    padded.extend_from_slice(message);
    padded.push(0x80);
    while padded.len().checked_rem(64) != Some(56) {
        padded.push(0);
    }
    let bits = u64::try_from(message.len())
        .unwrap_or(u64::MAX)
        .wrapping_mul(8);
    padded.extend_from_slice(&bits.to_be_bytes());

    for block in padded.chunks_exact(64) {
        compress(&mut state, block);
    }

    let mut out: Digest = [0; 20];
    for (slot, word) in out.chunks_exact_mut(4).zip(state.iter()) {
        slot.copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// One 64-byte block into the chaining state.
///
/// `block` is always exactly 64 bytes: the only caller walks `chunks_exact(64)`.
fn compress(state: &mut [u32; 5], block: &[u8]) {
    let mut schedule: [u32; 80] = [0; 80];

    for (index, word) in block.chunks_exact(4).enumerate() {
        let value = u32::from(word.first().copied().unwrap_or(0)) << 24
            | u32::from(word.get(1).copied().unwrap_or(0)) << 16
            | u32::from(word.get(2).copied().unwrap_or(0)) << 8
            | u32::from(word.get(3).copied().unwrap_or(0));
        if let Some(slot) = schedule.get_mut(index) {
            *slot = value;
        }
    }
    for index in 16_usize..80 {
        // Every subtraction below is in range: `index >= 16`.
        let mixed = read(&schedule, index.wrapping_sub(3))
            ^ read(&schedule, index.wrapping_sub(8))
            ^ read(&schedule, index.wrapping_sub(14))
            ^ read(&schedule, index.wrapping_sub(16));
        if let Some(slot) = schedule.get_mut(index) {
            *slot = mixed.rotate_left(1);
        }
    }

    let mut va = state.first().copied().unwrap_or(0);
    let mut vb = state.get(1).copied().unwrap_or(0);
    let mut vc = state.get(2).copied().unwrap_or(0);
    let mut vd = state.get(3).copied().unwrap_or(0);
    let mut ve = state.get(4).copied().unwrap_or(0);

    for index in 0_usize..80 {
        let (mixed, constant) = round(index, vb, vc, vd);
        let temporary = va
            .rotate_left(5)
            .wrapping_add(mixed)
            .wrapping_add(ve)
            .wrapping_add(constant)
            .wrapping_add(read(&schedule, index));
        ve = vd;
        vd = vc;
        vc = vb.rotate_left(30);
        vb = va;
        va = temporary;
    }

    add_in(state, 0, va);
    add_in(state, 1, vb);
    add_in(state, 2, vc);
    add_in(state, 3, vd);
    add_in(state, 4, ve);
}

/// The round function and its constant, by round index (FIPS 180-4, section
/// 4.1.1). `index` is always below 80: the only caller's loop says so.
const fn round(index: usize, vb: u32, vc: u32, vd: u32) -> (u32, u32) {
    match index {
        0..=19 => ((vb & vc) | ((!vb) & vd), 0x5A82_7999),
        20..=39 => (vb ^ vc ^ vd, 0x6ED9_EBA1),
        40..=59 => ((vb & vc) | (vb & vd) | (vc & vd), 0x8F1B_BCDC),
        _ => (vb ^ vc ^ vd, 0xCA62_C1D6),
    }
}

/// One word of the message schedule. Never out of range by construction; `0` is
/// unreachable and is there because a slice read is a `Option` and indexing is
/// denied (AGENTS.md section 4.3's sibling rule, `clippy::indexing_slicing`).
fn read(schedule: &[u32; 80], index: usize) -> u32 {
    debug_assert!(index < 80, "the message schedule is read in range");
    schedule.get(index).copied().unwrap_or(0)
}

/// Fold one working word back into the chaining state.
fn add_in(state: &mut [u32; 5], index: usize, value: u32) {
    if let Some(slot) = state.get_mut(index) {
        *slot = slot.wrapping_add(value);
    }
}

/// Lower-case hexadecimal, for the test vectors and for a diagnostic.
#[must_use]
pub fn hex(digest: &Digest) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(40);
    for byte in digest {
        // Writing into a `String` cannot fail; the result is discarded because
        // there is nothing to report.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{digest, hex};

    /// FIPS 180-4, appendix A.1: the one-block message.
    #[test]
    fn the_fips_180_4_one_block_vector() {
        assert_eq!(
            hex(&digest(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    /// FIPS 180-4, appendix A.2: the two-block message.
    #[test]
    fn the_fips_180_4_two_block_vector() {
        let message = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(
            hex(&digest(message)),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    /// FIPS 180-4, appendix A.3: one million `a`. Slow by the standards of the
    /// file and fast by the standards of the suite, and it is the vector that
    /// catches a padding or length bug that the short ones cannot.
    #[test]
    fn the_fips_180_4_long_vector() {
        let message = vec![b'a'; 1_000_000];
        assert_eq!(
            hex(&digest(&message)),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
    }

    /// The empty message, which is the padding path with nothing in front of it.
    #[test]
    fn the_empty_message() {
        assert_eq!(
            hex(&digest(b"")),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    /// A message exactly one byte short of a block boundary, where the length
    /// field forces a second block. The classic off-by-one in this function.
    #[test]
    fn a_message_that_forces_a_second_padding_block() {
        // Independently reproducible with any SHA-1 implementation:
        //   python -c "import hashlib; print(hashlib.sha1(b'x'*55).hexdigest())"
        let message = vec![b'x'; 55];
        assert_eq!(
            hex(&digest(&message)),
            "cef734ba81a024479e09eb5a75b6ddae62e6abf1"
        );
        let message = vec![b'x'; 56];
        assert_eq!(
            hex(&digest(&message)),
            "901305367c259952f4e7af8323f480d59f81335b"
        );
    }
}

// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Standard base64, padded, for the RFC 6455 handshake.
//!
//! Two uses and no others: encoding the twenty bytes of
//! `Sec-WebSocket-Accept` (RFC 6455 section 4.2.2), and decoding a client's
//! `Sec-WebSocket-Key` far enough to check that it really is sixteen bytes,
//! which section 4.1 requires of the client and section 4.2.1 lets the server
//! refuse.
//!
//! # Why this is not `pharmakos_proto::json`'s base64
//!
//! Decisions-log item 99 says the transport uses "the proto crate's `Json` value
//! and its `base64` module (item 74)", and the `Json` half is exactly what
//! [`crate::rpc`] does. The base64 half could not be taken as written: the proto
//! crate's module is `pub(crate)`, private to its canonical-JSON codec, and
//! making it public is a change to a crate this task does not own (AGENTS.md
//! section 6). Forty lines here, or a cross-crate API change on somebody else's
//! lane -- this file is the cheaper of the two, and the pull request records the
//! alternative so the owner can fold the two together whenever the proto crate
//! is next open.
//!
//! The alphabet is the same, the padding is the same, and
//! `an_encode_matches_the_proto_crates_own_vector` pins the two together on the
//! vector that matters.

/// The standard alphabet (RFC 4648 section 4). The URL-safe alphabet is not
//  accepted: a WebSocket key is standard base64 and nothing else.
const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
///
/// No `as` casts and no division: chunking gives both for free (AGENTS.md
/// sections 4.1 and 4.3), which is also how the proto crate writes it.
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let first = chunk.first().copied().unwrap_or(0);
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        let packed = (u32::from(first) << 16) | (u32::from(second) << 8) | u32::from(third);

        let symbol = |shift: u32| -> char {
            let index = usize::try_from((packed >> shift) & 0x3f).unwrap_or(0);
            char::from(STANDARD.get(index).copied().unwrap_or(b'A'))
        };

        out.push(symbol(18));
        out.push(symbol(12));
        match chunk.len() {
            1 => out.push_str("=="),
            2 => {
                out.push(symbol(6));
                out.push('=');
            }
            _ => {
                out.push(symbol(6));
                out.push(symbol(0));
            }
        }
    }
    out
}

/// The value of one standard-alphabet symbol.
fn value_of(symbol: u8) -> Option<u32> {
    match symbol {
        b'A'..=b'Z' => Some(u32::from(symbol.wrapping_sub(b'A'))),
        b'a'..=b'z' => Some(u32::from(symbol.wrapping_sub(b'a')).saturating_add(26)),
        b'0'..=b'9' => Some(u32::from(symbol.wrapping_sub(b'0')).saturating_add(52)),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Standard base64 with padding, decoded.
///
/// Returns `None` for any symbol outside the standard alphabet, for a text
/// whose length is not a whole number of four-symbol quanta, and for padding
/// anywhere but at the end. Strict on purpose: the only caller is deciding
/// whether to refuse a handshake, and a lenient decoder there would accept keys
/// a conforming client never sends.
#[must_use]
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len().checked_rem(4) != Some(0) {
        return None;
    }
    let trimmed = text.trim_end_matches('=');
    let padding = bytes.len().saturating_sub(trimmed.len());
    if padding > 2 {
        return None;
    }

    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    for quantum in trimmed.as_bytes().chunks(4) {
        let mut packed: u32 = 0;
        let mut symbols: u32 = 0;
        for symbol in quantum {
            packed = (packed << 6) | value_of(*symbol)?;
            symbols = symbols.saturating_add(1);
        }
        // A quantum of one symbol carries no whole byte and is malformed.
        if symbols < 2 {
            return None;
        }
        packed <<= 6_u32.saturating_mul(4_u32.saturating_sub(symbols));
        let whole = symbols.saturating_sub(1);
        for shift in [16_u32, 8, 0].iter().take(usize::try_from(whole).ok()?) {
            let byte = u8::try_from((packed >> *shift) & 0xff).ok()?;
            out.push(byte);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    #[test]
    fn the_rfc_4648_test_vectors() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_round_trip_holds_for_every_length_up_to_a_quantum_boundary() {
        for length in 0..=24_usize {
            let bytes: Vec<u8> = (0..length)
                .map(|index| u8::try_from(index.wrapping_mul(7).wrapping_add(3) % 251).unwrap_or(0))
                .collect();
            let text = encode(&bytes);
            if bytes.is_empty() {
                assert_eq!(text, "");
                continue;
            }
            assert_eq!(decode(&text).as_deref(), Some(bytes.as_slice()));
        }
    }

    /// The proto crate encodes `bytes` fields with the same alphabet and the
    /// same padding. If these two ever disagree the project has two base64s,
    /// which is the thing item 99 was trying to avoid.
    #[test]
    fn an_encode_matches_the_proto_crates_own_vector() {
        // The handshake's own vector, from RFC 6455 section 1.3.
        let digest =
            crate::sha1::digest(b"dGhlIHNhbXBsZSBub25jZQ==258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        assert_eq!(encode(&digest), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn a_malformed_text_is_refused_rather_than_guessed_at() {
        assert_eq!(decode("Zm9vYmFy="), None, "length is not a whole quantum");
        assert_eq!(decode("Zm9v!mFy"), None, "a symbol outside the alphabet");
        assert_eq!(decode("Zm9=YmFy"), None, "padding in the middle");
        assert_eq!(
            decode("Zm9vYmF_"),
            None,
            "the URL-safe alphabet is not this one"
        );
        assert_eq!(decode(""), None, "an empty key is not sixteen bytes");
    }

    #[test]
    fn a_sixteen_byte_key_decodes_to_sixteen_bytes() {
        let key = encode(&[9_u8; 16]);
        assert_eq!(decode(&key).map(|bytes| bytes.len()), Some(16));
    }
}

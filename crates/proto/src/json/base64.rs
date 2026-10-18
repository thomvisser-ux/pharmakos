// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Base64, for `bytes` fields — and for anybody else in the workspace who
//! needs standard base64.
//!
//! The proto3 JSON mapping writes a `bytes` field as standard base64 with
//! padding and accepts the URL-safe alphabet as well, so that is exactly what
//! is here and nothing more. No `as` casts and no division: chunking gives
//! both for free (AGENTS.md sections 4.1 and 4.3).
//!
//! # Why this module is public
//!
//! Decisions-log item 99 says the Seat Gateway's transport is built on "the
//! proto crate's `Json` value and its `base64` module (item 74)", and item 100
//! (10) makes that literally true: this module was `pub(crate)` when T9 was
//! written, so the gateway carried a forty-line copy of it for the RFC 6455
//! handshake, and the copy is deleted now that the original is reachable. The
//! project has one base64, as it has one hash function and one JSON value.
//!
//! [`decode`] is **lenient**, because the proto3 JSON mapping requires it to
//! be: the mapping says a reader accepts both alphabets and both padding
//! conventions. A caller that needs to refuse a text a conforming *writer*
//! would never produce — the gateway's handshake, which must reject a
//! `Sec-WebSocket-Key` that is not sixteen bytes of standard, padded base64 —
//! checks the text's shape itself before calling this, because that strictness
//! is its rule and not the mapping's.

/// Standard base64, padded.
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (quanta, rest) = bytes.as_chunks::<3>();
    for &[first, second, third] in quanta {
        let [a, b, c, d] = sextets(first, second, third);
        out.extend([symbol(a), symbol(b), symbol(c), symbol(d)]);
    }
    // `as_chunks::<3>` leaves no byte, one or two over; nothing but the
    // empty remainder reaches the last arm, and it writes no quantum.
    match *rest {
        [first] => {
            let [a, b, _, _] = sextets(first, 0, 0);
            out.extend([symbol(a), symbol(b), '=', '=']);
        }
        [first, second] => {
            let [a, b, c, _] = sextets(first, second, 0);
            out.extend([symbol(a), symbol(b), symbol(c), '=']);
        }
        _ => {}
    }
    out
}

/// Three bytes as four six-bit values, each below 64 by its mask.
const fn sextets(first: u8, second: u8, third: u8) -> [u8; 4] {
    [
        first >> 2,
        ((first & 0x03) << 4) | (second >> 4),
        ((second & 0x0f) << 2) | (third >> 6),
        third & 0x3f,
    ]
}

/// The standard alphabet's symbol for a six-bit value, by arithmetic rather
/// than a table lookup, so no index can miss: every value a mask leaves,
/// 0 to 63, has its arm, and the last arm is `/`'s.
fn symbol(sextet: u8) -> char {
    char::from(match sextet & 0x3f {
        value @ 0..=25 => b'A' + value,
        value @ 26..=51 => b'a' + (value - 26),
        value @ 52..=61 => b'0' + (value - 52),
        62 => b'+',
        _ => b'/',
    })
}

fn value_of(symbol: u8) -> Option<u32> {
    match symbol {
        b'A'..=b'Z' => Some(u32::from(symbol.wrapping_sub(b'A'))),
        b'a'..=b'z' => Some(u32::from(symbol.wrapping_sub(b'a')).saturating_add(26)),
        b'0'..=b'9' => Some(u32::from(symbol.wrapping_sub(b'0')).saturating_add(52)),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

/// Standard or URL-safe base64, with or without padding.
///
/// Returns `None` for any symbol outside both alphabets, for a text whose
/// length is not a whole number of quanta, and for padding in the middle.
///
/// Lenient by design — see the module docs. A caller that wants to refuse a
/// spelling a conforming writer never produces checks the text's shape first.
#[must_use]
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let trimmed = text.trim_end_matches('=');
    let symbols: Vec<u8> = trimmed.bytes().collect();
    let mut out: Vec<u8> = Vec::new();
    for chunk in symbols.chunks(4) {
        let mut packed: u32 = 0;
        for index in 0..4 {
            let value = match chunk.get(index) {
                Some(symbol) => value_of(*symbol)?,
                // A trailing partial quantum contributes zero bits, which the
                // length check below then discards.
                None => 0,
            };
            packed = (packed << 6) | value;
        }
        // Four sextets fill the low 24 bits; the top byte is always 0.
        let [_, high, middle, low] = packed.to_be_bytes();
        let bytes = [high, middle, low];
        match chunk.len() {
            2 => out.extend_from_slice(bytes.get(..1)?),
            3 => out.extend_from_slice(bytes.get(..2)?),
            4 => out.extend_from_slice(&bytes),
            // A lone leftover symbol carries six bits, which is not a byte,
            // and `chunks` never yields zero or more than four.
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    /// RFC 4648 section 10's test vectors, both ways.
    #[test]
    fn the_rfc_4648_vectors_round_trip() {
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(plain.as_bytes()), coded, "{plain:?}");
            assert_eq!(
                decode(coded).as_deref(),
                Some(plain.as_bytes()),
                "{coded:?}"
            );
        }
    }

    /// Every symbol of the alphabet, in order, and every byte value.
    #[test]
    fn every_byte_round_trips_and_the_alphabet_is_standard() {
        let all: Vec<u8> = (0..=u8::MAX).collect();
        assert_eq!(decode(&encode(&all)), Some(all));
        let ramp = [
            0x00, 0x10, 0x83, 0x10, 0x51, 0x87, 0x20, 0x92, 0x8b, 0x30, 0xd3, 0x8f, 0x41, 0x14,
            0x93, 0x51, 0x55, 0x97, 0x61, 0x96, 0x9b, 0x71, 0xd7, 0x9f, 0x82, 0x18, 0xa3, 0x92,
            0x59, 0xa7, 0xa2, 0x9a, 0xab, 0xb2, 0xdb, 0xaf, 0xc3, 0x1c, 0xb3, 0xd3, 0x5d, 0xb7,
            0xe3, 0x9e, 0xbb, 0xf3, 0xdf, 0xbf,
        ];
        assert_eq!(
            encode(&ramp),
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        );
    }
}

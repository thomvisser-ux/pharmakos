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

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64, padded.
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
        let bytes = [
            u8::try_from((packed >> 16) & 0xff).ok()?,
            u8::try_from((packed >> 8) & 0xff).ok()?,
            u8::try_from(packed & 0xff).ok()?,
        ];
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

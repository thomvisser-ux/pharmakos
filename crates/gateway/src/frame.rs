// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! RFC 6455 framing, server side.
//!
//! The narrowest subset that carries JSON-RPC, and no more of the standard than
//! that (decisions-log item 99):
//!
//! * **Masked client frames are required.** RFC 6455 section 5.1: a client that
//!   sends an unmasked frame is a protocol error and the connection closes.
//!   There is no lenient mode.
//! * **Text, close, ping and pong only.** A binary frame is refused rather than
//!   decoded: every message on this transport is JSON-RPC text, so a binary
//!   frame is either a bug or somebody probing.
//! * **No extensions.** The handshake offers none, so any reserved bit set in a
//!   frame is a protocol error (RFC 6455 section 5.2, `RSV1..3`).
//! * **A size cap on every message**, fragments summed, because the gateway is
//!   the one process that must not fall over when a client misbehaves.
//! * Fragmentation is understood, because a conforming client may fragment a
//!   long playbook, and a control frame may be interleaved with the fragments
//!   but may never itself be fragmented.
//!
//! Nothing here reads a clock or allocates unboundedly, and no `as` cast appears
//! in a length computation: a payload length arrives as a `u64` on the wire and
//! becomes a `usize` through `try_from`, so a 2^63-byte claim on a 32-bit build
//! is a refusal rather than a truncation.

/// The largest single message the gateway will assemble, fragments summed.
///
/// PLACEHOLDER: 256 KiB is a guess sized to hold any playbook the size budget
/// allows (`rules/rules.v1.json`, `verifier.size_budget_units`) with room to
/// spare. OWNER sets the real number at hardening, alongside the rate limits;
/// it is a security parameter rather than a tuning value, so it is deliberately
/// not a rules-table row today -- a rules row is stamped into `rules_hash` and
/// a transport cap has no business moving a hash chain.
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;

/// The largest single frame the gateway will read.
///
/// PLACEHOLDER: as [`MAX_MESSAGE_BYTES`] (OWNER, hardening). Equal to it today,
/// so an unfragmented message of the maximum size is legal.
pub const MAX_FRAME_BYTES: usize = MAX_MESSAGE_BYTES;

/// The frame opcodes v1 speaks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opcode {
    /// A continuation of the message the previous frame began.
    Continuation,
    /// UTF-8 text: the only data frame this transport carries.
    Text,
    /// Closing handshake.
    Close,
    /// Keep-alive probe.
    Ping,
    /// Reply to a probe.
    Pong,
}

impl Opcode {
    /// The wire value.
    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            Opcode::Continuation => 0x0,
            Opcode::Text => 0x1,
            Opcode::Close => 0x8,
            Opcode::Ping => 0x9,
            Opcode::Pong => 0xA,
        }
    }

    /// True for close, ping and pong. A control frame is never fragmented and
    /// never carries more than 125 bytes (RFC 6455 section 5.5).
    #[must_use]
    pub const fn is_control(self) -> bool {
        matches!(self, Opcode::Close | Opcode::Ping | Opcode::Pong)
    }
}

/// What a frame or a message broke.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Error {
    /// A reserved bit was set, and no extension was negotiated.
    ReservedBit,
    /// An opcode this transport does not speak -- a binary frame, or one of the
    /// reserved numbers.
    Opcode(u8),
    /// A client frame with the mask bit clear.
    Unmasked,
    /// A control frame longer than 125 bytes.
    ControlTooLong,
    /// A control frame with `FIN` clear.
    FragmentedControl,
    /// A continuation with no message to continue, or a new data frame in the
    /// middle of one.
    Fragmentation,
    /// The frame or the assembled message ran past the cap.
    TooLarge,
    /// A text message that is not UTF-8.
    NotUtf8,
    /// A close frame with a one-byte payload, which RFC 6455 section 5.5.1 does
    /// not allow.
    MalformedClose,
}

impl Error {
    /// The RFC 6455 section 7.4.1 close code to answer with.
    #[must_use]
    pub const fn close_code(&self) -> u16 {
        match self {
            Error::TooLarge => 1009,
            Error::NotUtf8 => 1007,
            _ => 1002,
        }
    }

    /// English, for the close frame's reason and the audit log.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Error::ReservedBit => "no extension was negotiated",
            Error::Opcode(_) => "this transport carries text frames only",
            Error::Unmasked => "client frames must be masked",
            Error::ControlTooLong => "a control frame carries at most 125 bytes",
            Error::FragmentedControl => "a control frame is never fragmented",
            Error::Fragmentation => "fragments out of order",
            Error::TooLarge => "the message is over the size cap",
            Error::NotUtf8 => "a text frame must be UTF-8",
            Error::MalformedClose => "a malformed close payload",
        }
    }
}

/// One decoded frame, unmasked.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Frame {
    /// The last frame of its message.
    pub fin: bool,
    /// What kind of frame it is.
    pub opcode: Opcode,
    /// The payload, already unmasked.
    pub payload: Vec<u8>,
}

/// Decode one frame from the front of `bytes`.
///
/// Returns the frame and how many bytes it consumed, or `Ok(None)` when the
/// buffer does not yet hold a whole frame.
///
/// # Errors
///
/// An [`Error`] for anything the subset above refuses. The caller closes the
/// connection with [`Error::close_code`]; there is no recovery, because a peer
/// that has broken the framing cannot be resynchronised.
pub fn decode(bytes: &[u8]) -> Result<Option<(Frame, usize)>, Error> {
    let Some(first) = bytes.first().copied() else {
        return Ok(None);
    };
    let Some(second) = bytes.get(1).copied() else {
        return Ok(None);
    };

    if first & 0x70 != 0 {
        return Err(Error::ReservedBit);
    }
    let fin = first & 0x80 != 0;
    let opcode = match first & 0x0f {
        0x0 => Opcode::Continuation,
        0x1 => Opcode::Text,
        0x8 => Opcode::Close,
        0x9 => Opcode::Ping,
        0xA => Opcode::Pong,
        other => return Err(Error::Opcode(other)),
    };

    if second & 0x80 == 0 {
        return Err(Error::Unmasked);
    }
    let short = second & 0x7f;
    let (length, mut cursor): (u64, usize) = match short {
        126 => {
            let Some(raw) = bytes.get(2..4) else {
                return Ok(None);
            };
            let mut wide = [0_u8; 2];
            wide.copy_from_slice(raw);
            (u64::from(u16::from_be_bytes(wide)), 4)
        }
        127 => {
            let Some(raw) = bytes.get(2..10) else {
                return Ok(None);
            };
            let mut wide = [0_u8; 8];
            wide.copy_from_slice(raw);
            (u64::from_be_bytes(wide), 10)
        }
        other => (u64::from(other), 2),
    };

    if opcode.is_control() {
        if length > 125 {
            return Err(Error::ControlTooLong);
        }
        if !fin {
            return Err(Error::FragmentedControl);
        }
    }
    // A length that does not fit `usize` cannot fit the cap either, so it is
    // over the cap rather than an arithmetic surprise.
    let length = usize::try_from(length).map_err(|_| Error::TooLarge)?;
    if length > MAX_FRAME_BYTES {
        return Err(Error::TooLarge);
    }

    let Some(mask) = bytes.get(cursor..cursor.saturating_add(4)) else {
        return Ok(None);
    };
    let mut key = [0_u8; 4];
    key.copy_from_slice(mask);
    cursor = cursor.saturating_add(4);

    let Some(masked) = bytes.get(cursor..cursor.saturating_add(length)) else {
        return Ok(None);
    };
    let payload: Vec<u8> = masked
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ key.get(index.wrapping_rem(4)).copied().unwrap_or(0))
        .collect();

    Ok(Some((
        Frame {
            fin,
            opcode,
            payload,
        },
        cursor.saturating_add(length),
    )))
}

/// Encode one server frame. Server frames are never masked (RFC 6455 section
/// 5.1), and the gateway never fragments: every message it sends fits a frame.
#[must_use]
pub fn encode(opcode: Opcode, payload: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(payload.len().saturating_add(10));
    out.push(0x80 | opcode.bits());
    let length = payload.len();
    if length < 126 {
        out.push(u8::try_from(length).unwrap_or(125));
    } else if let Ok(short) = u16::try_from(length) {
        out.push(126);
        out.extend_from_slice(&short.to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&u64::try_from(length).unwrap_or(u64::MAX).to_be_bytes());
    }
    out.extend_from_slice(payload);
    out
}

/// A close frame with a status code and a reason.
///
/// The reason is truncated on a character boundary so the frame stays inside the
/// 125-byte control limit and stays valid UTF-8.
#[must_use]
pub fn close(code: u16, reason: &str) -> Vec<u8> {
    let mut payload: Vec<u8> = Vec::with_capacity(125);
    payload.extend_from_slice(&code.to_be_bytes());
    let room = 123_usize;
    let mut end = reason.len().min(room);
    while end > 0 && !reason.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    if let Some(text) = reason.get(..end) {
        payload.extend_from_slice(text.as_bytes());
    }
    encode(Opcode::Close, &payload)
}

/// A whole message, once its fragments are joined.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Message {
    /// A complete text message.
    Text(String),
    /// The peer is closing: the code it gave, and its reason.
    Close(u16, String),
    /// A ping, to be ponged back with the same payload.
    Ping(Vec<u8>),
    /// A pong. The gateway never pings, so it never expects one; a stray pong is
    /// legal and ignored (RFC 6455 section 5.5.3).
    Pong(Vec<u8>),
}

/// Joins fragments into messages, one connection's worth.
#[derive(Debug, Default)]
pub struct Assembler {
    partial: Option<Vec<u8>>,
}

impl Assembler {
    /// An assembler with no message in progress.
    #[must_use]
    pub const fn new() -> Assembler {
        Assembler { partial: None }
    }

    /// Feed one frame. `Ok(None)` means the message is not finished yet.
    ///
    /// # Errors
    ///
    /// An [`Error`] for a fragmentation sequence RFC 6455 does not allow, for a
    /// message over [`MAX_MESSAGE_BYTES`], or for text that is not UTF-8.
    pub fn accept(&mut self, frame: Frame) -> Result<Option<Message>, Error> {
        match frame.opcode {
            Opcode::Ping => return Ok(Some(Message::Ping(frame.payload))),
            Opcode::Pong => return Ok(Some(Message::Pong(frame.payload))),
            Opcode::Close => {
                let code = match frame.payload.len() {
                    0 => 1005,
                    1 => return Err(Error::MalformedClose),
                    _ => {
                        let mut raw = [0_u8; 2];
                        raw.copy_from_slice(frame.payload.get(..2).unwrap_or(&[0, 0]));
                        u16::from_be_bytes(raw)
                    }
                };
                // RFC 6455 section 5.5.1: the reason is UTF-8, and section 7.4.1
                // gives 1007 for text that is not. Decoding it lossily would
                // answer a protocol violation with 1000 "goodbye", and the rule
                // reads the same for a close frame as for a text frame.
                let reason = match frame.payload.get(2..) {
                    Some(rest) => String::from_utf8(rest.to_vec()).map_err(|_| Error::NotUtf8)?,
                    None => String::new(),
                };
                return Ok(Some(Message::Close(code, reason)));
            }
            Opcode::Text => {
                if self.partial.is_some() {
                    return Err(Error::Fragmentation);
                }
                self.partial = Some(Vec::new());
            }
            Opcode::Continuation => {
                if self.partial.is_none() {
                    return Err(Error::Fragmentation);
                }
            }
        }

        let Some(buffer) = self.partial.as_mut() else {
            return Err(Error::Fragmentation);
        };
        if buffer.len().saturating_add(frame.payload.len()) > MAX_MESSAGE_BYTES {
            self.partial = None;
            return Err(Error::TooLarge);
        }
        buffer.extend_from_slice(&frame.payload);
        if !frame.fin {
            return Ok(None);
        }
        let joined = self.partial.take().unwrap_or_default();
        let text = String::from_utf8(joined).map_err(|_| Error::NotUtf8)?;
        Ok(Some(Message::Text(text)))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Assembler, Error, Frame, MAX_MESSAGE_BYTES, Message, Opcode, close, decode, encode,
    };

    /// A client frame: masked, as RFC 6455 section 5.1 requires of every one.
    fn client(fin: bool, opcode: Opcode, payload: &[u8]) -> Vec<u8> {
        let key = [0xA1_u8, 0xB2, 0xC3, 0xD4];
        let mut out: Vec<u8> = Vec::new();
        out.push(if fin { 0x80 } else { 0x00 } | opcode.bits());
        let length = payload.len();
        if length < 126 {
            out.push(0x80 | u8::try_from(length).expect("short"));
        } else if let Ok(short) = u16::try_from(length) {
            out.push(0x80 | 0x7e);
            out.extend_from_slice(&short.to_be_bytes());
        } else {
            out.push(0x80 | 127);
            out.extend_from_slice(&u64::try_from(length).expect("fits").to_be_bytes());
        }
        out.extend_from_slice(&key);
        for (index, byte) in payload.iter().enumerate() {
            out.push(byte ^ key.get(index % 4).copied().unwrap_or(0));
        }
        out
    }

    #[test]
    fn a_masked_text_frame_round_trips() {
        let bytes = client(true, Opcode::Text, b"{\"jsonrpc\":\"2.0\"}");
        let (frame, used) = decode(&bytes).expect("well formed").expect("complete");
        assert_eq!(used, bytes.len());
        assert_eq!(frame.opcode, Opcode::Text);
        assert!(frame.fin);
        assert_eq!(frame.payload, b"{\"jsonrpc\":\"2.0\"}");
    }

    #[test]
    fn an_unmasked_client_frame_is_a_protocol_error() {
        // The same frame with the mask bit cleared and the payload in clear.
        let bytes = [0x81_u8, 0x03, b'a', b'b', b'c'];
        assert_eq!(decode(&bytes).expect_err("refused"), Error::Unmasked);
        assert_eq!(Error::Unmasked.close_code(), 1002);
    }

    #[test]
    fn a_binary_frame_is_refused_rather_than_decoded() {
        let mut bytes = client(true, Opcode::Text, b"x");
        if let Some(first) = bytes.first_mut() {
            *first = 0x82; // FIN + binary
        }
        assert_eq!(decode(&bytes).expect_err("refused"), Error::Opcode(2));
    }

    #[test]
    fn a_reserved_bit_is_refused_because_no_extension_was_negotiated() {
        let mut bytes = client(true, Opcode::Text, b"x");
        if let Some(first) = bytes.first_mut() {
            *first = 0xC1; // FIN + RSV1 + text
        }
        assert_eq!(decode(&bytes).expect_err("refused"), Error::ReservedBit);
    }

    #[test]
    fn a_partial_frame_asks_for_more_rather_than_failing() {
        let bytes = client(true, Opcode::Text, b"hello world");
        for cut in 0..bytes.len() {
            assert_eq!(
                decode(bytes.get(..cut).expect("in range")).expect("not an error"),
                None,
                "a {cut}-byte prefix is incomplete, not malformed"
            );
        }
    }

    #[test]
    fn a_fragmented_text_message_is_joined() {
        let mut assembler = Assembler::new();
        let (first, _) = decode(&client(false, Opcode::Text, b"{\"a\":"))
            .expect("well formed")
            .expect("complete");
        assert_eq!(assembler.accept(first).expect("legal"), None);
        let (rest, _) = decode(&client(true, Opcode::Continuation, b"1}"))
            .expect("well formed")
            .expect("complete");
        assert_eq!(
            assembler.accept(rest).expect("legal"),
            Some(Message::Text(String::from("{\"a\":1}")))
        );
    }

    #[test]
    fn a_control_frame_may_not_be_fragmented() {
        let bytes = client(false, Opcode::Ping, b"x");
        assert_eq!(
            decode(&bytes).expect_err("refused"),
            Error::FragmentedControl
        );
    }

    #[test]
    fn a_continuation_with_nothing_to_continue_is_refused() {
        let mut assembler = Assembler::new();
        let frame = Frame {
            fin: true,
            opcode: Opcode::Continuation,
            payload: Vec::new(),
        };
        assert_eq!(
            assembler.accept(frame).expect_err("refused"),
            Error::Fragmentation
        );
    }

    #[test]
    fn a_message_over_the_cap_is_refused_and_the_partial_is_dropped() {
        let mut assembler = Assembler::new();
        let half = vec![b'x'; MAX_MESSAGE_BYTES];
        let first = Frame {
            fin: false,
            opcode: Opcode::Text,
            payload: half,
        };
        assert_eq!(assembler.accept(first).expect("under the cap"), None);
        let second = Frame {
            fin: true,
            opcode: Opcode::Continuation,
            payload: vec![b'x'],
        };
        assert_eq!(assembler.accept(second).expect_err("over"), Error::TooLarge);
        assert_eq!(Error::TooLarge.close_code(), 1009);
        // And the connection is not left holding a quarter of a megabyte.
        let after = Frame {
            fin: true,
            opcode: Opcode::Continuation,
            payload: vec![b'x'],
        };
        assert_eq!(
            assembler.accept(after).expect_err("nothing to continue"),
            Error::Fragmentation
        );
    }

    #[test]
    fn a_text_frame_that_is_not_utf8_is_refused() {
        let mut assembler = Assembler::new();
        let frame = Frame {
            fin: true,
            opcode: Opcode::Text,
            payload: vec![0xff, 0xfe],
        };
        assert_eq!(
            assembler.accept(frame).expect_err("refused"),
            Error::NotUtf8
        );
    }

    /// RFC 6455 section 5.5.1: the close reason is UTF-8 too, and section 7.4.1
    /// gives 1007 for text that is not. Answering a protocol violation with
    /// 1000 "goodbye" would be the one place the rule read differently for a
    /// close frame than for a text frame.
    #[test]
    fn a_close_reason_that_is_not_utf8_is_refused() {
        let mut assembler = Assembler::new();
        let frame = Frame {
            fin: true,
            opcode: Opcode::Close,
            payload: vec![0x03, 0xe8, 0xff, 0xfe],
        };
        assert_eq!(
            assembler.accept(frame).expect_err("refused"),
            Error::NotUtf8
        );
        assert_eq!(Error::NotUtf8.close_code(), 1007);

        // A well formed one still reads, reason and all.
        let mut assembler = Assembler::new();
        let frame = Frame {
            fin: true,
            opcode: Opcode::Close,
            payload: vec![0x03, 0xe8, b'b', b'y', b'e'],
        };
        assert_eq!(
            assembler.accept(frame).expect("accepted"),
            Some(Message::Close(1000, String::from("bye")))
        );
    }

    #[test]
    fn a_server_frame_is_never_masked() {
        let bytes = encode(Opcode::Text, b"hello");
        assert_eq!(bytes.first().copied(), Some(0x81));
        assert_eq!(bytes.get(1).copied(), Some(0x05), "the mask bit is clear");
    }

    #[test]
    fn a_close_reason_is_truncated_inside_the_control_limit() {
        let bytes = close(1009, &"e".repeat(400));
        assert!(bytes.len() <= 127, "{} bytes", bytes.len());
        assert_eq!(bytes.get(1).copied(), Some(125));
    }

    #[test]
    fn a_close_reason_is_truncated_on_a_character_boundary() {
        // Two header bytes, then the two-byte close code, then the reason.
        let bytes = close(1002, &"na\u{00ef}ve ".repeat(40));
        let reason = bytes.get(4..).expect("reason");
        assert!(
            std::str::from_utf8(reason).is_ok(),
            "a truncated reason is still UTF-8"
        );
    }

    #[test]
    fn a_long_payload_uses_the_sixteen_bit_length() {
        let payload = vec![b'y'; 1000];
        let bytes = client(true, Opcode::Text, &payload);
        let (frame, used) = decode(&bytes).expect("well formed").expect("complete");
        assert_eq!(used, bytes.len());
        assert_eq!(frame.payload.len(), 1000);
    }
}

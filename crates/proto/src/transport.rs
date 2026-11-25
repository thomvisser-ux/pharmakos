// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Seat Gateway's transport limits, the numbers each side of the socket reads.
//!
//! - **Inbound** (S3's `fixq-c`, fix pass 1): the gateway assembles no message past
//!   [`MAX_MESSAGE_BYTES`] and closes the connection with code 1009 on one, and a client refuses
//!   to send one, so a call too large to read is said where the player looks instead of taking
//!   the connection down.
//! - **Outbound** (S3's `outcap`): the gateway sends no answer whose text is past
//!   [`MAX_ANSWER_BYTES`]. Every answer it makes, on a socket or to a seat this process plays or
//!   advises, is measured once, as the wire spells it, before it leaves the surface; one past the
//!   cap is never sent and never cut, and is answered with the typed `RESPONSE_TOO_LARGE`
//!   instead. A client sizes its inbound buffer from [`MAX_ANSWER_FRAME_BYTES`], the cap and a
//!   server frame's largest header, so every answer the gateway sends is one it can take whole.
//!
//! The values, and the PLACEHOLDERs that mark them as guesses for the owner at hardening, are
//! described beside the gateway's own constants (`pharmakos_gateway::frame::MAX_MESSAGE_BYTES`
//! for the inbound cap; [`MAX_ANSWER_BYTES`] here for the outbound one).

/// The largest single message the gateway assembles, its fragments summed: 256 KiB.
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;

/// The largest answer text the gateway sends, in UTF-8 bytes as the wire spells it (the
/// canonical writer, `pharmakos_proto::json::write`): 4 MiB.
///
/// Chosen so that every answer a playbook inside the size budget and a map at the gate's load
/// can produce fits with room: the worst verifier report is held to three quarters of it
/// (`pharmakos_verifier`'s `the_cap_is_the_top_of_its_range` reads this constant), a view page
/// is cut at `pharmakos_gateway::viewfeed::VIEW_PAGE_BYTES` of chunks and entities, an id at
/// [`MAX_ID_BYTES`], and `crates/gateway/tests/outgoing.rs` measures each method's worst answer
/// against it. A larger answer is a typed refusal, never a cut text.
///
/// PLACEHOLDER: the largest answer, 4 MiB — owner, at hardening, alongside the rate limits.
pub const MAX_ANSWER_BYTES: usize = 4 * 1024 * 1024;

/// The most bytes a server frame's header takes: RFC 6455 section 5.2's first two bytes and an
/// eight-byte extended length. Server frames are never masked, so there is no masking key.
pub const SERVER_FRAME_HEADER_BYTES: usize = 10;

/// The largest frame the gateway writes: an answer at [`MAX_ANSWER_BYTES`] in one frame with the
/// largest header. What a client's inbound buffer must hold so that every answer arrives whole.
pub const MAX_ANSWER_FRAME_BYTES: usize = MAX_ANSWER_BYTES + SERVER_FRAME_HEADER_BYTES;

/// The longest request `id` the gateway reads, in bytes of its JSON lexeme (a string's text
/// without its quotes, or a number's digits). Every answer echoes its request's id, so an
/// unbounded id would be an answer the size of the request, sent back: the amplification this
/// cap refuses. A request with a longer id is an `Invalid Request`, answered under no id.
///
/// PLACEHOLDER: the longest request id, 128 bytes — owner, at hardening, alongside the rate limits.
pub const MAX_ID_BYTES: usize = 128;

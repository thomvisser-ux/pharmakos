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
/// canonical writer, `pharmakos_proto::json::write`): 12 MiB.
///
/// The rule it is set by (S3's `outcap`, fix pass 2): **the worst in-budget answer is at most
/// three quarters of the cap.** The worst found is a `render_plan` of 7 723 838 bytes: 128
/// committed `place_beacon` steps (a covering seam, a raid's settings), each under a guard of 23
/// `enemy_structure_near` leaves near the safest place, labels to the inbound cap; a seeded search
/// over every condition kind, step kind and handler slot found it
/// (`crates/gateway/tests/outgoing.rs`, `the_worst_in_budget_render_is_searched`), and
/// `an_in_budget_playbook_at_the_inbound_cap_answers_whole` holds it to three quarters of this
/// cap. The cap is sized to today's render, not a design for it: the render's own cut is
/// `outcap` part 2's. The worst verifier report as `verify_plan` sends it, and eight more of the
/// catalogue's largest codes, fit it (`pharmakos_verifier`'s
/// `the_worst_report_fits_the_answer_with_room_to_grow`); a view page is cut at
/// `pharmakos_gateway::viewfeed::VIEW_PAGE_BYTES` of chunks and entities, an id at
/// [`MAX_ID_BYTES`]. A larger answer is a typed refusal, never a cut text.
///
/// PLACEHOLDER: the largest answer, 12 MiB by the rule that the worst in-budget answer is at most three quarters of it — owner, at hardening, alongside the rate limits.
pub const MAX_ANSWER_BYTES: usize = 12 * 1024 * 1024;

/// The most bytes a server frame's header takes: RFC 6455 section 5.2's first two bytes and an
/// eight-byte extended length. Server frames are never masked, so there is no masking key.
pub const SERVER_FRAME_HEADER_BYTES: usize = 10;

/// The largest frame the gateway writes: an answer at [`MAX_ANSWER_BYTES`] in one frame with the
/// largest header. What a client's inbound buffer must hold so that every answer arrives whole.
pub const MAX_ANSWER_FRAME_BYTES: usize = MAX_ANSWER_BYTES + SERVER_FRAME_HEADER_BYTES;

/// The longest request `id` the gateway reads, in UTF-8 bytes of its value as read: a string's
/// text once its escapes are read (an escaped `é` is two bytes, a four-byte character four),
/// or a number's digits: what `pharmakos_gateway::rpc::parse` measures. Every answer echoes its
/// request's id, so an
/// unbounded id would be an answer the size of the request, sent back: the amplification this
/// cap refuses. A request with a longer id is an `Invalid Request`, answered under no id.
///
/// PLACEHOLDER: the longest request id, 128 bytes — owner, at hardening, alongside the rate limits.
pub const MAX_ID_BYTES: usize = 128;

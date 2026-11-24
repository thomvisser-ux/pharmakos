// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the client's WebSocket peers must carry, from the one number the gateway reads (S3's
//! `fixq-c`, fix pass 2).
//!
//! The rig never sends a frame past [`pharmakos_proto::transport::MAX_MESSAGE_BYTES`], and a peer
//! must be able to queue every frame under it: Godot's `WebSocketPeer` refuses a `send_text` whose
//! payload, added to what is queued on it already, is past its `outbound_buffer_size` (64 KiB by
//! default; `wsl_peer.cpp`'s `_send`, `ERR_OUT_OF_MEMORY`). The review's 100 KB paste fell in that
//! gap: under the cap, so the rig sent it, and over the peer's buffer, so Godot dropped it and
//! the link timed out. `host_link.gd` sets each peer's buffers from these numbers, which the
//! bridge passes through (`watch_transport_limits`), and a refused send is reported back
//! (`watch_send_failed`), so the rig settles that call at once.

use pharmakos_proto::transport::MAX_MESSAGE_BYTES;

/// A peer's outbound buffer: one frame at the cap. Godot counts a queued message's payload, not
/// its WebSocket header, and the rig queues at most one frame per link (a link with a call in
/// flight sends nothing more until it is answered), so the cap itself is what a peer must hold;
/// the watch check sends a frame of exactly the cap to a real gateway on a peer sized so.
pub const OUTBOUND_BUFFER_BYTES: usize = MAX_MESSAGE_BYTES;

/// A peer's inbound buffer: the largest message the gateway sends a client, with room. The
/// gateway bounds no outgoing message by a cap of its own; the largest it makes is a view page,
/// whose runs it cuts at 256 KiB (about 350 KB once base64 and JSON grow them), plus a chunk origin
/// each and the entity list, which nothing bounds: `tests/transport.rs` builds a page at the cut
/// with every chunk of the committed map and 2 000 entities, 1 230 868 bytes as this crate's JSON
/// writer spells it, past the 1 MiB `host_link.gd` used to give a peer. Four MiB holds that page
/// with more than half to spare; the committed keyframe golden is 286 411 bytes. (Reported to the
/// main session for the gateway's next lane: a bound on a page's entities, or on any message the
/// gateway writes, belongs at the gateway.)
/// Four MiB, written as a literal: `crates/verifier/tests/report_size.rs` reads this line.
pub const INBOUND_BUFFER_BYTES: usize = 4_194_304;

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
/// the watch check sends a frame of exactly the cap to a real gateway on a peer sized so. (Godot
/// accepted two frames of exactly the cap back to back on a peer sized so, measured by review 3:
/// its buffer drains as it sends. The rig never queues the second.)
pub const OUTBOUND_BUFFER_BYTES: usize = MAX_MESSAGE_BYTES;

/// A peer's inbound buffer: what the client takes of one message the gateway sends. The gateway
/// bounds no outgoing message by a cap of its own. A view page cuts its runs at 256 KiB (about
/// 350 KB once base64 and JSON grow them), but adds a chunk origin each and the entity list,
/// which nothing bounds: `tests/transport.rs` builds a page at the cut with every chunk of the
/// committed map and 2 000 entities, 1 230 868 bytes as the gateway's wire spells it (the
/// canonical writer, `rpc::render`), past the 1 MiB `host_link.gd` used to give a peer. Four MiB
/// holds that page with more than half to spare; the committed keyframe golden is 286 411 bytes.
/// Other answers are unbounded too (a `render_plan` of a long file of bare steps, by reading,
/// may pass 4 MiB): lane `outcap` bounds every message the gateway writes, at the gateway.
///
/// An answer past it is never handed over cut: Godot closes the peer with 1009 and hands over
/// nothing (`client_check.gd` measures it), and the rig says that close to the player
/// (`Rig::closed_on_message`).
///
/// Four MiB, written as a literal: `crates/verifier/tests/report_size.rs` reads this line.
/// PLACEHOLDER: the client's inbound buffer, 4 MiB against a gateway that bounds no outgoing message — outcap, S3, when the gateway caps every message it sends
pub const INBOUND_BUFFER_BYTES: usize = 4_194_304;

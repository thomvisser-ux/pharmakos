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

use pharmakos_proto::transport::{MAX_ANSWER_FRAME_BYTES, MAX_MESSAGE_BYTES};

/// A peer's outbound buffer: one frame at the cap. Godot counts a queued message's payload, not
/// its WebSocket header, and the rig queues at most one frame per link (a link with a call in
/// flight sends nothing more until it is answered), so the cap itself is what a peer must hold;
/// the watch check sends a frame of exactly the cap to a real gateway on a peer sized so. (Godot
/// accepted two frames of exactly the cap back to back on a peer sized so, measured by review 3:
/// its buffer drains as it sends. The rig never queues the second.)
pub const OUTBOUND_BUFFER_BYTES: usize = MAX_MESSAGE_BYTES;

/// A peer's inbound buffer: the largest frame the gateway writes (S3's `outcap`), the one number
/// the gateway's sender reads, passed through. The gateway sends no answer whose text is past
/// [`pharmakos_proto::transport::MAX_ANSWER_BYTES`]: one past it is answered with the typed
/// `RESPONSE_TOO_LARGE` instead, never cut. The buffer is that cap and a server frame's largest
/// header ([`MAX_ANSWER_FRAME_BYTES`]), so every answer arrives whole; the client decides
/// nothing about the size.
///
/// A message past it is never handed over cut: Godot closes the peer with 1009 and hands over
/// nothing (`client_check.gd` measures it at this buffer and one byte more), and the rig says
/// that close to the player (`Rig::closed_on_message`).
pub const INBOUND_BUFFER_BYTES: usize = MAX_ANSWER_FRAME_BYTES;

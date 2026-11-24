// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Seat Gateway's transport limits, the one number each side of the socket reads (S3's
//! `fixq-c`, fix pass 1): the gateway assembles no message past [`MAX_MESSAGE_BYTES`] and
//! closes the connection with code 1009 on one, and a client refuses to send one, so a call
//! too large to read is said where the player looks instead of taking the connection down.
//!
//! Its value, and the PLACEHOLDER that marks it as a guess for the owner at hardening, are
//! described beside the gateway's own constant (`pharmakos_gateway::frame::MAX_MESSAGE_BYTES`),
//! which reads this one.

/// The largest single message the gateway assembles, its fragments summed: 256 KiB.
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;

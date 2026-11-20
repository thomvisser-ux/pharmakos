// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The stack a player's text is read on (S3's `vfyq2`, fix pass 2, item E).
//!
//! Every reader, writer, verifier stage and renderer of a playbook recurses on
//! its nesting, and the grammar bounds that nesting at
//! [`pharmakos_proto::json::grammar::MAX_DEPTH`] containers. A bound on depth
//! is a bound on the stack only once the stack itself is a stated number, so
//! no entry point that reads player text runs it on a thread whose size the
//! platform chose: not the main thread (1 MiB on Windows, the MSVC linker's
//! default; 8 MiB on Linux and macOS), and not `std::thread::spawn`'s
//! default, which `RUST_MIN_STACK` moves. Each one starts its work on a
//! thread made by [`player_text_thread`]:
//!
//! * `gamectl`'s whole command (`verify`, `host`, `scenario run`,
//!   `seat doctor`), from its `main`;
//! * the gateway's surface thread, which answers every method, and its
//!   connection threads, which parse every frame (`serve.rs`), and its
//!   accept threads (`net.rs`), which read no text and are held to the same
//!   door so that no thread in the gateway is made any other way.
//!
//! `tests/stack.rs` in the gateway and in `gamectl` run the deepest accepted
//! text, and the deepest refused one, through those real entry points on a
//! thread of **half** this size in a debug build, where frames are largest: a
//! factor of two over the worst build, not a pass.

/// The stack, in bytes, of every thread that reads player text.
///
/// Measured when it was set (2026-11-20, Windows, MSVC): the deepest
/// playbook through every planning method needed under 1 MiB in a debug
/// build and about 96 KiB in a release one, so 4 MiB is more than twice the
/// debug need and about forty times the release one. A thread's stack is
/// reserved address space, committed only as it is touched, so the figure
/// costs no memory a reader does not use.
pub const PLAYER_TEXT_STACK: usize = 4 << 20;

/// A thread builder for work that reads player text: named, and on
/// [`PLAYER_TEXT_STACK`] bytes of stack.
pub fn player_text_thread(name: impl Into<String>) -> std::thread::Builder {
    std::thread::Builder::new()
        .name(name.into())
        .stack_size(PLAYER_TEXT_STACK)
}

// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The self-counting caught-panic guard.
//!
//! **gdext's panic catch at the `#[func]` boundary is silent** (G1 section 10.12). A panic
//! in a call Godot made becomes a Godot error line and a default return value, and the
//! frame carries on looking healthy; in G1 the instrument that made "0 panics across all
//! 25 runs" a fact rather than a hope was the extension counting its own catches and the
//! analysis failing on a non-zero count. The skeleton's bridge does the same, and T12's
//! acceptance is that CI asserts the count is **0** after a headless run.
//!
//! So every `#[func]` body goes through [`PanicCounter::guard`]. That is a second
//! `catch_unwind` inside gdext's own, which is the point: gdext's catch keeps the panic
//! out of C++, and this one keeps it out of the shadows.
//!
//! # One caveat, stated rather than discovered later
//!
//! `catch_unwind` catches nothing under `panic = "abort"`, and the workspace's
//! `[profile.release]` sets exactly that. A debug or `release-checked` build unwinds and
//! the counter works; a release build would abort the whole engine process instead. That
//! is a **contract question for the owner**, not something this crate may decide
//! (`[profile.*]` is an AGENTS.md section 5 path) — it is raised in T12's pull request,
//! and until it is answered `cargo xtask stage-client` stages the debug library, where the
//! guard is real.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Counts panics caught at the bridge's own boundary, and remembers the last one.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PanicCounter {
    caught: u64,
    last: Option<String>,
}

impl PanicCounter {
    /// A counter that has caught nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            caught: 0,
            last: None,
        }
    }

    /// How many panics this bridge has caught. CI asserts this is zero.
    #[must_use]
    pub const fn caught(&self) -> u64 {
        self.caught
    }

    /// What the last caught panic said, if there was one.
    #[must_use]
    pub fn last(&self) -> Option<&str> {
        self.last.as_deref()
    }

    /// Runs `body`, counting a panic instead of letting it pass for a result.
    ///
    /// `what` names the call, so the report says which one failed rather than only that
    /// something did. Returns `None` when the body panicked; the caller decides what a
    /// missing result looks like on the Godot side, because a default return value that
    /// nobody flagged is the failure mode this whole module exists to prevent.
    pub fn guard<T>(&mut self, what: &str, body: impl FnOnce() -> T) -> Option<T> {
        match catch_unwind(AssertUnwindSafe(body)) {
            Ok(value) => Some(value),
            Err(payload) => {
                self.caught = self.caught.saturating_add(1);
                self.last = Some(format!("{what}: {}", describe(payload.as_ref())));
                None
            }
        }
    }

    /// A one-line report for the engine log and the CI leg.
    #[must_use]
    pub fn describe(&self) -> String {
        match self.last.as_deref() {
            None => format!("caught_panics={}", self.caught),
            Some(last) => format!("caught_panics={} last={last}", self.caught),
        }
    }
}

/// What a panic payload said, for the two shapes `panic!` actually produces.
fn describe(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        return (*message).to_owned();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "a panic with a payload that is neither a &str nor a String".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keeps the default hook from printing a backtrace for the panics these tests cause
    /// on purpose. Restored before the test ends, so a genuine failure still reports.
    fn quietly<T>(body: impl FnOnce() -> T) -> T {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let out = body();
        std::panic::set_hook(previous);
        out
    }

    #[test]
    fn a_call_that_returns_is_not_counted() {
        let mut counter = PanicCounter::new();
        assert_eq!(counter.guard("mesh_chunk", || 7_i32), Some(7));
        assert_eq!(counter.caught(), 0);
        assert_eq!(counter.last(), None);
        assert_eq!(counter.describe(), "caught_panics=0");
    }

    #[test]
    fn a_panic_is_counted_and_named_rather_than_passed_off_as_a_result() {
        let mut counter = PanicCounter::new();
        let result: Option<i32> =
            quietly(|| counter.guard("mesh_chunk", || panic!("the mesher fell over")));
        assert_eq!(result, None, "a panic must not look like a value");
        assert_eq!(counter.caught(), 1);
        let last = counter.last().unwrap_or_default();
        assert!(last.contains("mesh_chunk"), "{last}");
        assert!(last.contains("the mesher fell over"), "{last}");
        assert!(counter.describe().contains("caught_panics=1"));
    }

    #[test]
    fn a_formatted_panic_message_survives_as_a_string_payload() {
        let mut counter = PanicCounter::new();
        let chunk = 41_u32;
        let result: Option<()> =
            quietly(|| counter.guard("upload", || panic!("chunk {chunk} is not a chunk")));
        assert_eq!(result, None);
        assert!(
            counter.last().unwrap_or_default().contains("chunk 41"),
            "{:?}",
            counter.last()
        );
    }

    #[test]
    fn the_count_accumulates_across_calls() {
        let mut counter = PanicCounter::new();
        quietly(|| {
            for _ in 0..3 {
                let _: Option<()> = counter.guard("upload", || panic!("again"));
            }
        });
        assert_eq!(counter.caught(), 3);
    }
}

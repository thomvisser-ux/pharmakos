// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rate limiter, counted in ticks.
//!
//! Spec section 12 lists rate limits beside the audit log in the security
//! paragraph, and AGENTS.md section 7 says both are "part of the feature, not a
//! later hardening task". So the limiter is here at T9, before a single gameplay
//! method exists, and the numbers it enforces are the only part of it left open.
//!
//! # Why it counts ticks and not seconds
//!
//! Because the gateway reads no clock (AGENTS.md section 4.5). A wall-clock
//! limiter would also be the one part of the gateway whose behaviour differed
//! between a fast machine and a slow one, which would make a scenario run
//! unreproducible for a reason that has nothing to do with the sim. Counting the
//! host's ticks gives a limiter that is exactly as deterministic as the match it
//! protects: the same sequence of calls at the same ticks is admitted or refused
//! identically on every machine, which is what
//! `the_limiter_is_deterministic_and_reads_no_clock` asserts.
//!
//! The cost is honest and worth writing down: **during the Lull the tick still
//! advances**, so the limiter still works, but a client that hammers the gateway
//! while the host is paused is limited by the per-tick cap alone. That is the
//! right trade for a local, single-user process -- the limiter is there to keep
//! a runaway client from starving the host, not to price an API.
//!
//! # What it does not count, and why that is written here
//!
//! **A call that never authenticated is not counted.** The budget is per token
//! and [`crate::surface::Surface::call`] identifies the caller before it admits
//! one, deliberately: a rate-limited call should be logged against the token
//! that made it, and "somebody went too fast" would be the least useful line in
//! the audit log. The consequence is that a flood of bad tokens on the loopback
//! socket is refused one by one, unmetered, each refusal costing one bounded
//! audit entry. On a localhost-only binding that is a process on this machine
//! attacking its own user, and the answer to it is a per-connection budget the
//! session holds -- which needs a place to put per-connection state that T9 does
//! not have. Written down rather than left to be rediscovered: OWNER, at
//! hardening, with the numbers.

use crate::error::Error;
use pharmakos_sim::math::quantity::Tick;

/// Calls one token may make in a single tick.
///
/// PLACEHOLDER: 8 is a guess -- generous for an editor that verifies on every
/// keystroke, tight enough that a runaway loop is caught within one tick. OWNER
/// sets the real number at hardening (skeleton plan T9: "rate-limit numbers and
/// token lifetime"). It is deliberately not a rules-table row: a rules row is
/// stamped into `rules_hash`, and a transport limit has no business moving a
/// hash chain.
pub const CALLS_PER_TICK: u32 = 8;

/// Calls one token may make in a window of [`WINDOW_TICKS`].
///
/// PLACEHOLDER: as [`CALLS_PER_TICK`] (OWNER, hardening).
pub const CALLS_PER_WINDOW: u32 = 600;

/// The window, in ticks. 200 ticks is ten seconds of game time at 20 Hz.
///
/// PLACEHOLDER: as [`CALLS_PER_TICK`] (OWNER, hardening).
pub const WINDOW_TICKS: u32 = 200;

/// The caps for a token the host minted for **itself**: a built-in seat's and
/// an advisor's ([`crate::serve::BuiltInSeat`], [`crate::serve::Advisor`]).
///
/// Decisions-log item 111, decision C6, taken on the recommendation as the
/// owner's question D1: an in-process seat goes through the same door, the
/// same audit and the same fog as a socket, **with its own rate**. The reason
/// is H8: the host plans an in-process seat synchronously, right after the
/// call that opened the Lull, and a Lull's tick moves only when a client
/// reports its clock -- so every call an operator makes lands on one tick,
/// and at [`CALLS_PER_TICK`] its ninth would be refused. It is a rate
/// privilege, never a read privilege: every call is still counted against
/// these numbers, audited and fog-filtered, and a socket never gets them
/// (`an_in_process_seat_finishes_a_round_under_its_limits_and_a_socket_does_not_get_them`).
///
/// PLACEHOLDER: 128 calls in a tick and 1 200 in a window are working numbers,
/// sized well above the scripted seats' rounds and above the plan's estimate
/// of an Easy round (about fifty calls, and an advisor as much again). That an
/// in-process seat has a rate of its own at all was **taken on the
/// recommendation** (item 111 (4), the owner's question D1) and is open to the
/// **owner**'s overrule **now**; the **numbers** are the owner's at
/// **hardening**, against T18's derived `EASY_CALL_BUDGET`.
pub const IN_PROCESS_LIMITS: Limits = Limits {
    per_tick: 128,
    per_window: 1_200,
    window_ticks: WINDOW_TICKS,
};

/// The two caps, so a test or a host can set them without touching the
/// constants.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Limits {
    /// Calls in one tick.
    pub per_tick: u32,
    /// Calls in one window.
    pub per_window: u32,
    /// The window's length in ticks.
    pub window_ticks: u32,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            per_tick: CALLS_PER_TICK,
            per_window: CALLS_PER_WINDOW,
            window_ticks: WINDOW_TICKS,
        }
    }
}

/// One caller's budget.
///
/// A fixed-window counter rather than a leaky bucket: a bucket that drains "per
/// unit time" needs a unit of time, and the only one the gateway has is the
/// tick, at which point the bucket *is* this counter with more arithmetic.
#[derive(Clone, Debug)]
pub struct RateLimiter {
    limits: Limits,
    tick: Tick,
    in_tick: u32,
    window_start: Tick,
    in_window: u32,
}

impl RateLimiter {
    /// A limiter at the default caps, starting at tick zero.
    #[must_use]
    pub fn new() -> RateLimiter {
        RateLimiter::with_limits(Limits::default())
    }

    /// A limiter at `limits`.
    #[must_use]
    pub const fn with_limits(limits: Limits) -> RateLimiter {
        RateLimiter {
            limits,
            tick: Tick::ZERO,
            in_tick: 0,
            window_start: Tick::ZERO,
            in_window: 0,
        }
    }

    /// Count one call at `tick`.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::RateLimited`], with a message saying which cap was
    /// reached, so a client can tell "slow down within this tick" from "you have
    /// spent your window".
    pub fn admit(&mut self, tick: Tick) -> Result<(), Error> {
        // A tick earlier than the one already counted means the host went
        // backwards, which it does not do. Counting it against the current tick
        // is the conservative reading: it cannot be used to reset the budget.
        if tick > self.tick {
            self.tick = tick;
            self.in_tick = 0;
        }
        if self.tick.since(self.window_start) >= self.limits.window_ticks {
            self.window_start = self.tick;
            self.in_window = 0;
        }

        if self.in_tick >= self.limits.per_tick {
            return Err(Error::rate_limited(format!(
                "over {} calls in one tick",
                self.limits.per_tick
            )));
        }
        if self.in_window >= self.limits.per_window {
            return Err(Error::rate_limited(format!(
                "over {} calls in {} ticks",
                self.limits.per_window, self.limits.window_ticks
            )));
        }

        self.in_tick = self.in_tick.saturating_add(1);
        self.in_window = self.in_window.saturating_add(1);
        Ok(())
    }

    /// Calls counted against the current tick.
    #[must_use]
    pub const fn in_tick(&self) -> u32 {
        self.in_tick
    }

    /// Calls counted against the current window.
    #[must_use]
    pub const fn in_window(&self) -> u32 {
        self.in_window
    }
}

impl Default for RateLimiter {
    fn default() -> RateLimiter {
        RateLimiter::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Limits, RateLimiter};
    use crate::error::Code;
    use pharmakos_sim::math::quantity::Tick;

    fn limiter() -> RateLimiter {
        RateLimiter::with_limits(Limits {
            per_tick: 3,
            per_window: 5,
            window_ticks: 4,
        })
    }

    #[test]
    fn the_per_tick_cap_holds_and_the_next_tick_refills_it() {
        let mut limiter = limiter();
        for call in 0..3 {
            limiter.admit(Tick::new(10)).unwrap_or_else(|error| {
                panic!("call {call} should be admitted: {error}");
            });
        }
        let error = limiter.admit(Tick::new(10)).expect_err("over the tick cap");
        assert_eq!(error.code, Code::RateLimited);
        assert!(error.message.contains("in one tick"), "{}", error.message);
        limiter
            .admit(Tick::new(11))
            .expect("a new tick, a new budget");
    }

    #[test]
    fn the_window_cap_holds_across_ticks_and_resets_with_the_window() {
        let mut limiter = limiter();
        // Five calls spread over ticks 0..5 fill the window of five.
        for tick in 0..5_u32 {
            limiter
                .admit(Tick::new(tick))
                .unwrap_or_else(|error| panic!("tick {tick}: {error}"));
        }
        // Tick 4 is four ticks past the window start, so the window rolled at
        // tick 4 and the counter is inside a fresh one. Walk on to prove the cap
        // exists at all: fill the new window and watch it refuse.
        let mut limiter = RateLimiter::with_limits(Limits {
            per_tick: 10,
            per_window: 3,
            window_ticks: 100,
        });
        for call in 0..3 {
            limiter
                .admit(Tick::new(call))
                .unwrap_or_else(|error| panic!("call {call}: {error}"));
        }
        let error = limiter
            .admit(Tick::new(4))
            .expect_err("over the window cap");
        assert_eq!(error.code, Code::RateLimited);
        assert!(error.message.contains("ticks"), "{}", error.message);
        limiter
            .admit(Tick::new(100))
            .expect("a new window, a new budget");
    }

    /// The property the whole design is for: the same calls at the same ticks
    /// give the same answers, on every machine and in every run.
    #[test]
    fn the_limiter_is_deterministic_and_reads_no_clock() {
        let script: Vec<u32> = vec![0, 0, 0, 0, 1, 1, 5, 5, 5, 5, 400, 400];
        let run = || -> Vec<bool> {
            let mut limiter = limiter();
            script
                .iter()
                .map(|tick| limiter.admit(Tick::new(*tick)).is_ok())
                .collect()
        };
        let first = run();
        assert_eq!(first, run());
        assert_eq!(
            first,
            vec![
                true, true, true, false, true, true, true, true, true, false, true, true
            ]
        );
    }

    #[test]
    fn a_tick_that_goes_backwards_cannot_refill_the_budget() {
        let mut limiter = limiter();
        for _ in 0..3 {
            limiter.admit(Tick::new(50)).expect("in budget");
        }
        assert!(
            limiter.admit(Tick::new(1)).is_err(),
            "an earlier tick counts against the current one"
        );
    }
}

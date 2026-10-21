// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The pacer, the host clock and the keep-alive: the one wall clock in the client.
//!
//! **Presentation pacing on the walled side, like the Lull timer, and not game-rule time
//! arithmetic** (`docs/design/skeleton-plan-t16a-notes.md` section A (2)). The gateway reads
//! no clock (AGENTS.md section 4.5), so something has to decide how fast a watched Push
//! plays and tell the host how much time has passed while nothing is being played; that
//! something is here, in a walled crate, and nowhere in GDScript.
//!
//! # The pacer
//!
//! Elapsed wall time times the speed accumulates **game milliseconds** owed. At most one
//! `advance_push {ms}` is in flight; asks are no more frequent than [`PACER_PERIOD_US`];
//! the gateway answers `advanced_ms`, which is floored to what it could run, and what was
//! asked and not run is **carried** into the next ask. Owed time beyond [`BACKLOG_BOUND_MS`]
//! is dropped, so a stalled host or a slow frame is caught up by at most that much rather
//! than by a burst. The pacer never learns how long a step of the sim is, and never sees
//! one: the wire carries milliseconds in and milliseconds out (decisions-log item 107 (3)).
//!
//! **It accumulates from elapsed time, not from frames drawn** (item 108 (3)): a window that
//! is unfocused or minimised still plays its Push at speed, up to the backlog bound. There
//! is no pause control, because the spec names none.
//!
//! Skip is `advance_push {60 000}` asked again as soon as the last one is answered, with no
//! period in between, until the status footer leaves the Push.
//!
//! # The host clock
//!
//! Outside a Push the gateway's clock — and with it every token's rate budget — moves only
//! when `report_host_clock {elapsed_ms, remaining_ms}` says so. [`HostClock`] reports about
//! four times a second in a Lull, a recap and after match end: `elapsed_ms` is wall time
//! spent in the current phase, monotone, and reported in steps of at most 60 000 after a
//! stall because the gateway refuses a larger step; `remaining_ms` is the Lull's countdown,
//! which starts from the rules table's length for the round (`first_lull_ms` for round 1,
//! `lull_ms` after it) or from the countdown the footer already shows when this client
//! rejoins a Lull it had timed, and is left out of the report in every phase with no
//! countdown — an untimed Lull, a recap, the end of the match — because the gateway reads a
//! report with no `remaining_ms` as no countdown and one with 0 as a timer that ran out
//! (S1-11, decisions-log item 135).
//!
//! # The keep-alive
//!
//! The gateway drops a connection that has been silent for 30 seconds
//! (`session::READ_TIMEOUT`), so each connection that has sent nothing for
//! [`KEEPALIVE_US`] sends a cheap call. A cheap call rather than a WebSocket ping, because
//! Godot's own heartbeat closes a connection whose pong is late, and a pong is late behind
//! a long `advance_push` on the one thread that owns the match.

use std::num::NonZeroU64;
use std::time::Instant;

/// The speeds the watch rig offers: 1x, 2x and 4x.
///
/// PLACEHOLDER: the watch speeds 1x, 2x and 4x — OWNER, ruled at the demo to stand for now.
/// They are Tuning: the spec says "2-4x" and does not say whether 3x is a speed
/// (decisions-log item 126 (2) (b); register D-24; skeleton-plan-t16a-notes.md section D).
pub const SPEEDS: [u32; 3] = [1, 2, 4];

/// How often, at most, the pacer asks for game time: 100 ms of wall time.
///
/// PLACEHOLDER: the pacer's period — OWNER, at hardening, with the rate limits. At least
/// 50 ms, so the admin token's budget of 600 calls per 200 steps is never approached: at
/// 1x an ask of 100 ms runs two steps of the sim, so the pacer spends at most one call per
/// two. Tuning.
pub const PACER_PERIOD_US: u64 = 100_000;

/// The most one `advance_push` may ask for: the gateway's `MAX_ADVANCE_MS`, a wire bound
/// that the gateway refuses above rather than clamps.
pub const MAX_ADVANCE_MS: i32 = 60_000;

// An ask is a positive number of milliseconds, so the bound is one.
const _: () = assert!(MAX_ADVANCE_MS > 0);

/// How much owed game time the pacer carries before it drops the rest: 5 seconds.
///
/// PLACEHOLDER: the backlog bound — OWNER, at hardening, with the pacer's period. The
/// marker named no when before; this one is the register's M-04 proposal. A stalled host or
/// a long hitch is caught up by at most this much, so a Push never plays a burst of minutes
/// to make up for a frozen window.
pub const BACKLOG_BOUND_MS: u64 = 5_000;

/// How often the host clock is reported outside a Push: every 250 ms of wall time.
///
/// PLACEHOLDER: the host clock's report interval — OWNER, at hardening, with the rate
/// limits. "About four times a second" (skeleton-plan-t16a-notes.md section A (2)). The
/// rate budget of every token refills at this grain in a Lull, so the editor's
/// verify-on-every-edit (T19) feels it.
pub const CLOCK_REPORT_US: u64 = 250_000;

/// The largest step one host-clock report may take: the gateway's own bound.
pub const MAX_CLOCK_STEP_MS: u64 = 60_000;

/// How long a connection may send nothing before it sends a keep-alive call: 5 seconds.
///
/// PLACEHOLDER: the keep-alive interval — OWNER, at hardening, with `READ_TIMEOUT`. Well
/// inside the gateway's 30-second `READ_TIMEOUT` and the "at least every 10 s" the plan
/// asks for.
pub const KEEPALIVE_US: u64 = 5_000_000;

/// How long the editor must be left alone before FULL runs: 600 ms of wall time.
///
/// Spec section 13, "Validation: QUICK runs on every edit and FULL after 600 ms idle". The
/// number is the spec's, not a tuning guess, and it is a question of *when* to ask, which is
/// why it lives beside the other three timers rather than in the editor: the editor asks the
/// gateway for every verdict and measures no time of its own (AGENTS.md section 3 rule 4).
pub const FULL_AFTER_IDLE_US: u64 = 600_000;

/// Microseconds in a millisecond.
const US_PER_MS: NonZeroU64 = non_zero(1_000);

/// Milliseconds in a second, for [`clock_text`].
const MS_PER_SECOND: NonZeroU64 = non_zero(1_000);

/// Seconds in a minute, for [`clock_text`].
const SECONDS_PER_MINUTE: NonZeroU64 = non_zero(60);

/// A unit that is not zero, checked when the constant is built: a zero is a compile error,
/// never a division at run time.
const fn non_zero(value: u64) -> NonZeroU64 {
    match NonZeroU64::new(value) {
        Some(unit) => unit,
        None => panic!("a unit of time is not zero"),
    }
}

/// `value` in whole `unit`s, rounded down: the one division the timers make, by a unit
/// that cannot be zero, so it cannot fail. (Named rather than written `/`, because
/// `integer_division` asks for the rounding to be said.)
fn floor_div(value: u64, unit: NonZeroU64) -> u64 {
    value.div_euclid(unit.get())
}

/// The one wall clock the client reads.
#[derive(Clone, Copy, Debug)]
pub struct WallClock {
    origin: Instant,
}

impl WallClock {
    /// A clock whose zero is now.
    #[must_use]
    pub fn start() -> Self {
        Self {
            origin: Instant::now(),
        }
    }

    /// Wall microseconds since [`WallClock::start`].
    #[must_use]
    pub fn now_us(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_micros()).unwrap_or(u64::MAX)
    }
}

/// How much game time the watched Push asks the host for, and when.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pacer {
    active: bool,
    speed: u32,
    skipping: bool,
    /// Game microseconds owed and not yet asked for.
    owed_us: u64,
    /// Wall microseconds since the last ask.
    since_ask_us: u64,
    /// The ask in flight, in game milliseconds.
    in_flight: Option<i32>,
    /// Game milliseconds dropped at the backlog bound, for the report.
    dropped_ms: u64,
}

impl Default for Pacer {
    fn default() -> Self {
        Self::new()
    }
}

impl Pacer {
    /// An idle pacer at 1x.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            active: false,
            speed: 1,
            skipping: false,
            owed_us: 0,
            since_ask_us: PACER_PERIOD_US,
            in_flight: None,
            dropped_ms: 0,
        }
    }

    /// The Push began: start owing game time from nothing, with nothing in flight.
    pub fn start(&mut self) {
        self.active = true;
        self.skipping = false;
        self.owed_us = 0;
        self.since_ask_us = PACER_PERIOD_US;
        self.in_flight = None;
    }

    /// The Push is over: owe nothing and stop skipping. An answer still in flight is
    /// forgotten here, so it can never hold up the next Push, and ignored when it arrives.
    pub fn stop(&mut self) {
        self.active = false;
        self.skipping = false;
        self.owed_us = 0;
        self.in_flight = None;
    }

    /// Whether a Push is being paced.
    #[must_use]
    pub const fn active(&self) -> bool {
        self.active
    }

    /// The current speed.
    #[must_use]
    pub const fn speed(&self) -> u32 {
        self.speed
    }

    /// Chooses a speed from [`SPEEDS`]; anything else is refused and changes nothing.
    pub fn set_speed(&mut self, speed: u32) -> bool {
        if SPEEDS.contains(&speed) {
            self.speed = speed;
            true
        } else {
            false
        }
    }

    /// Skip to the end of the segment: free and instant, and only in a Push.
    pub fn skip(&mut self) {
        if self.active {
            self.skipping = true;
        }
    }

    /// Whether a skip is under way.
    #[must_use]
    pub const fn skipping(&self) -> bool {
        self.skipping
    }

    /// Whether an ask is waiting for its answer.
    #[must_use]
    pub const fn in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    /// Game milliseconds owed and not yet asked for.
    #[must_use]
    pub fn owed_ms(&self) -> u64 {
        floor_div(self.owed_us, US_PER_MS)
    }

    /// Game milliseconds dropped at the backlog bound so far.
    #[must_use]
    pub const fn dropped_ms(&self) -> u64 {
        self.dropped_ms
    }

    /// `wall_us` of wall time went by.
    pub fn elapse(&mut self, wall_us: u64) {
        self.since_ask_us = self.since_ask_us.saturating_add(wall_us);
        if !self.active || self.skipping {
            return;
        }
        let owed = self
            .owed_us
            .saturating_add(wall_us.saturating_mul(u64::from(self.speed)));
        self.owed_us = self.bounded(owed);
    }

    /// The next `advance_push`, in game milliseconds, when one is due.
    pub fn next_ask(&mut self) -> Option<i32> {
        if !self.active || self.in_flight.is_some() {
            return None;
        }
        if self.skipping {
            self.in_flight = Some(MAX_ADVANCE_MS);
            self.since_ask_us = 0;
            return Some(MAX_ADVANCE_MS);
        }
        if self.since_ask_us < PACER_PERIOD_US {
            return None;
        }
        let ask = match i32::try_from(self.owed_ms()) {
            Ok(owed) => owed.min(MAX_ADVANCE_MS),
            // More milliseconds owed than an `i32` holds is more than one ask may carry.
            Err(_) => MAX_ADVANCE_MS,
        };
        if ask == 0 {
            return None;
        }
        let due_us = u64::from(ask.unsigned_abs()).saturating_mul(US_PER_MS.get());
        self.owed_us = self.owed_us.saturating_sub(due_us);
        self.since_ask_us = 0;
        self.in_flight = Some(ask);
        Some(ask)
    }

    /// The gateway ran `advanced_ms` of the ask in flight. What was asked and not run is
    /// carried into the next ask; the gateway floors an advance to what it could run, so
    /// it never runs more than it was asked.
    ///
    /// # Errors
    ///
    /// [`crate::error::BridgeError::Rpc`] for an answer that says the gateway ran more
    /// than it was asked, which the gateway never does; the ask is settled with nothing
    /// carried, since the host is past it.
    pub fn answered(&mut self, advanced_ms: u32) -> Result<(), crate::error::BridgeError> {
        let Some(asked) = self.in_flight.take() else {
            return Ok(());
        };
        let Some(unrun) = asked.unsigned_abs().checked_sub(advanced_ms) else {
            return Err(crate::error::BridgeError::Rpc(format!(
                "advance_push ran {advanced_ms} ms where {asked} ms were asked"
            )));
        };
        if !self.active || self.skipping {
            return Ok(());
        }
        let owed = self
            .owed_us
            .saturating_add(u64::from(unrun).saturating_mul(US_PER_MS.get()));
        self.owed_us = self.bounded(owed);
        Ok(())
    }

    /// The ask in flight was refused or lost: carry all of it.
    pub fn refused(&mut self) {
        // An answer of 0 ms is never more than was asked, so it carries the whole ask and
        // cannot be refused.
        if self.answered(0).is_err() {
            self.in_flight = None;
        }
    }

    /// `owed` with everything above the backlog bound dropped, and counted.
    fn bounded(&mut self, owed: u64) -> u64 {
        let bound = BACKLOG_BOUND_MS.saturating_mul(US_PER_MS.get());
        if owed > bound {
            let over = floor_div(owed.saturating_sub(bound), US_PER_MS);
            self.dropped_ms = self.dropped_ms.saturating_add(over);
            bound
        } else {
            owed
        }
    }
}

/// Which clock the host is told about, by phase.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClockPhase {
    /// A Push, or a phase not yet known: nothing is reported, because in a Push the sim's
    /// own progress is the gateway's clock.
    Silent,
    /// A Lull. `countdown_ms` is the countdown it began at, or `None` for an untimed Lull,
    /// which is told to the gateway as no countdown at all rather than as a countdown at
    /// zero (`report_host_clock` with no `remaining_ms`; the register's S1-11). A countdown
    /// of zero is a timed Lull that has already run out.
    Lull {
        /// The countdown, in milliseconds, when the Lull opened; `None` when it has none.
        countdown_ms: Option<u64>,
    },
    /// A recap, or the end of the match: `remaining_ms` is zero.
    Open,
}

/// The host clock outside a Push.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HostClock {
    phase: ClockPhase,
    /// Wall microseconds spent in this phase.
    spent_us: u64,
    /// The last report the gateway accepted, in milliseconds.
    accepted_ms: u64,
    /// The report in flight.
    pending_ms: Option<u64>,
    /// Wall microseconds since the last report went out.
    since_report_us: u64,
    /// Whether a report was due that `report_host_clock`'s `int32` fields cannot carry: a
    /// phase longer than `i32::MAX` milliseconds (24.8 days of wall time in one Lull,
    /// recap or ended match). No report goes out from then on in that phase, rather than
    /// one held at the bound, and [`HostClock::unreportable`] says so.
    unreportable: bool,
}

impl Default for HostClock {
    fn default() -> Self {
        Self::new()
    }
}

impl HostClock {
    /// A clock in no phase.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            phase: ClockPhase::Silent,
            spent_us: 0,
            accepted_ms: 0,
            pending_ms: None,
            since_report_us: CLOCK_REPORT_US,
            unreportable: false,
        }
    }

    /// A new phase began: its time starts at zero, and its first report is due at once.
    pub fn enter(&mut self, phase: ClockPhase) {
        self.phase = phase;
        self.spent_us = 0;
        self.accepted_ms = 0;
        self.pending_ms = None;
        self.since_report_us = CLOCK_REPORT_US;
        self.unreportable = false;
    }

    /// Whether this phase ran past what `report_host_clock` can carry, so no report goes
    /// out until the next phase ([`HostClock::next_report`]).
    #[must_use]
    pub const fn unreportable(&self) -> bool {
        self.unreportable
    }

    /// The phase this clock reports for.
    #[must_use]
    pub const fn phase(&self) -> ClockPhase {
        self.phase
    }

    /// `wall_us` of wall time went by.
    pub fn elapse(&mut self, wall_us: u64) {
        self.spent_us = self.spent_us.saturating_add(wall_us);
        self.since_report_us = self.since_report_us.saturating_add(wall_us);
    }

    /// The next `report_host_clock`, as `(elapsed_ms, remaining_ms)`, when one is due.
    /// `remaining_ms` is `None` in every phase with no countdown — an untimed Lull, a recap,
    /// the end of the match — so the report carries no countdown there, which the gateway
    /// reads as none rather than as one that ran out (S1-11).
    ///
    /// After a stall the report is a step of at most [`MAX_CLOCK_STEP_MS`] beyond the last
    /// one the gateway accepted, so a long freeze is reported as several steps in a row.
    pub fn next_report(&mut self) -> Option<(i32, Option<i32>)> {
        if self.phase == ClockPhase::Silent || self.pending_ms.is_some() || self.unreportable {
            return None;
        }
        let behind = self.spent_ms().saturating_sub(self.accepted_ms) > MAX_CLOCK_STEP_MS;
        if self.since_report_us < CLOCK_REPORT_US && !behind {
            return None;
        }
        let report = self
            .spent_ms()
            .min(self.accepted_ms.saturating_add(MAX_CLOCK_STEP_MS));
        let countdown = match self.phase {
            ClockPhase::Lull {
                countdown_ms: Some(total_ms),
            } => Some(total_ms.saturating_sub(report)),
            ClockPhase::Lull { countdown_ms: None } | ClockPhase::Silent | ClockPhase::Open => None,
        };
        // A countdown is at most a Lull's length, a positive `int32` of the rules table, so
        // only the time spent can outgrow the wire.
        let (Ok(wired), Ok(countdown)) = (
            i32::try_from(report),
            countdown.map(i32::try_from).transpose(),
        ) else {
            self.unreportable = true;
            return None;
        };
        self.pending_ms = Some(report);
        self.since_report_us = 0;
        Some((wired, countdown))
    }

    /// The report in flight was accepted.
    pub fn answered(&mut self) {
        if let Some(report) = self.pending_ms.take() {
            self.accepted_ms = self.accepted_ms.max(report);
        }
    }

    /// The report in flight was refused; the next one steps from the last accepted.
    pub fn refused(&mut self) {
        self.pending_ms = None;
    }

    /// Whether a timed Lull's countdown has run out. An untimed Lull never runs out.
    #[must_use]
    pub fn run_out(&self) -> bool {
        match self.phase {
            ClockPhase::Lull {
                countdown_ms: Some(total_ms),
            } => self.spent_ms() >= total_ms,
            ClockPhase::Lull { countdown_ms: None } | ClockPhase::Silent | ClockPhase::Open => {
                false
            }
        }
    }

    /// The Lull timer as the lobby shows it, `m:ss`; empty outside a timed Lull.
    #[must_use]
    pub fn timer_text(&self) -> String {
        match self.phase {
            ClockPhase::Lull {
                countdown_ms: Some(total_ms),
            } => clock_text(total_ms.saturating_sub(self.spent_ms())),
            ClockPhase::Lull { countdown_ms: None } | ClockPhase::Silent | ClockPhase::Open => {
                String::new()
            }
        }
    }

    /// Whole milliseconds spent in this phase.
    fn spent_ms(&self) -> u64 {
        floor_div(self.spent_us, US_PER_MS)
    }
}

/// Game or wall milliseconds as `m:ss`, rounded down to the second.
#[must_use]
pub fn clock_text(ms: u64) -> String {
    let seconds = floor_div(ms, MS_PER_SECOND);
    let minutes = floor_div(seconds, SECONDS_PER_MINUTE);
    let rest = seconds % SECONDS_PER_MINUTE;
    format!("{minutes}:{rest:02}")
}

/// The editor's idle timer: when FULL is owed (spec section 13, "FULL after 600 ms idle").
///
/// An edit restarts it; once [`FULL_AFTER_IDLE_US`] of wall time has gone by with no further
/// edit, [`IdleTimer::due`] says so until the editor has asked for FULL and
/// [`IdleTimer::clear`] is called. A timer nobody restarted is never due, so a draft that has
/// not changed is not verified again for nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct IdleTimer {
    /// Wall microseconds since the last edit, or `None` when no FULL is owed.
    quiet_us: Option<u64>,
}

impl IdleTimer {
    /// An edit landed: FULL is owed once the editor has been left alone long enough.
    pub fn edited(&mut self) {
        self.quiet_us = Some(0);
    }

    /// `wall_us` of wall time went by.
    pub fn elapse(&mut self, wall_us: u64) {
        if let Some(quiet) = self.quiet_us.as_mut() {
            *quiet = quiet.saturating_add(wall_us);
        }
    }

    /// Whether FULL is owed now.
    #[must_use]
    pub fn due(&self) -> bool {
        self.quiet_us
            .is_some_and(|quiet| quiet >= FULL_AFTER_IDLE_US)
    }

    /// FULL was asked for (or is no longer wanted): nothing is owed until the next edit.
    pub fn clear(&mut self) {
        self.quiet_us = None;
    }
}

/// Everything the watch rig times, advanced together from one reading of [`WallClock`].
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Timing {
    /// Whether [`Timing::advance`] has been given a reading yet.
    started: bool,
    /// The latest reading; zero before the first.
    last_us: u64,
    /// The Push's pacer.
    pub pacer: Pacer,
    /// The host clock outside a Push.
    pub clock: HostClock,
    /// The editor's idle timer.
    pub idle: IdleTimer,
    /// Wall microseconds since each connection last sent anything: admin, then seat.
    quiet_us: [u64; 2],
}

impl Timing {
    /// Brings every timer up to `now_us`, a [`WallClock`] reading. A reading earlier than
    /// the last one moves nothing.
    pub fn advance(&mut self, now_us: u64) {
        // The first reading starts the timers, so no time went by before it.
        let gone = if self.started {
            now_us.saturating_sub(self.last_us)
        } else {
            0
        };
        self.started = true;
        self.last_us = now_us.max(self.last_us);
        self.pacer.elapse(gone);
        self.clock.elapse(gone);
        self.idle.elapse(gone);
        for quiet in &mut self.quiet_us {
            *quiet = quiet.saturating_add(gone);
        }
    }

    /// Connection `link` (0 admin, 1 seat) just sent something.
    pub fn sent(&mut self, link: usize) {
        if let Some(quiet) = self.quiet_us.get_mut(link) {
            *quiet = 0;
        }
    }

    /// Whether connection `link` has been quiet long enough to need a keep-alive.
    #[must_use]
    pub fn keepalive_due(&self, link: usize) -> bool {
        self.quiet_at_least(link, KEEPALIVE_US)
    }

    /// Whether connection `link` has sent nothing for at least `wall_us`.
    #[must_use]
    pub fn quiet_at_least(&self, link: usize, wall_us: u64) -> bool {
        self.quiet_us
            .get(link)
            .is_some_and(|quiet| *quiet >= wall_us)
    }

    /// The latest [`WallClock`] reading [`Timing::advance`] was given; zero before any.
    #[must_use]
    pub const fn now_us(&self) -> u64 {
        self.last_us
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pushing(speed: u32) -> Pacer {
        let mut pacer = Pacer::new();
        assert!(pacer.set_speed(speed));
        pacer.start();
        pacer
    }

    #[test]
    fn a_speed_outside_the_set_is_refused() {
        let mut pacer = Pacer::new();
        assert!(!pacer.set_speed(3), "3x is not in the PLACEHOLDER set");
        assert!(
            !pacer.set_speed(0),
            "there is no pause control (item 108 (3))"
        );
        assert_eq!(pacer.speed(), 1);
    }

    #[test]
    fn owed_time_beyond_the_backlog_bound_is_dropped() {
        let mut pacer = pushing(4);
        pacer.elapse(10_000_000);
        assert_eq!(pacer.owed_ms(), BACKLOG_BOUND_MS);
        assert_eq!(pacer.dropped_ms(), 40_000 - BACKLOG_BOUND_MS);
        assert_eq!(
            pacer.next_ask(),
            Some(i32::try_from(BACKLOG_BOUND_MS).expect("fits"))
        );
    }

    #[test]
    fn a_skip_asks_the_maximum_again_as_soon_as_it_is_answered_with_no_period() {
        let mut pacer = pushing(1);
        pacer.skip();
        assert_eq!(pacer.next_ask(), Some(MAX_ADVANCE_MS));
        assert_eq!(pacer.next_ask(), None, "one in flight");
        pacer
            .answered(MAX_ADVANCE_MS.unsigned_abs())
            .expect("no more than was asked");
        assert_eq!(pacer.next_ask(), Some(MAX_ADVANCE_MS), "no period between");
        pacer.answered(20_000).expect("no more than was asked");
        pacer.stop();
        assert_eq!(pacer.next_ask(), None, "the footer left the Push");
        assert!(!pacer.skipping());
    }

    #[test]
    fn an_ask_left_in_flight_at_the_end_of_a_push_never_holds_up_the_next() {
        let mut pacer = pushing(1);
        pacer.skip();
        assert_eq!(pacer.next_ask(), Some(MAX_ADVANCE_MS));
        pacer.stop();
        assert!(!pacer.in_flight());
        pacer.start();
        pacer.elapse(PACER_PERIOD_US);
        assert_eq!(pacer.next_ask(), Some(100), "the new Push asks at once");
    }

    #[test]
    fn nothing_is_asked_outside_a_push() {
        let mut pacer = Pacer::new();
        pacer.elapse(1_000_000);
        assert_eq!(pacer.next_ask(), None);
        pacer.skip();
        assert!(!pacer.skipping(), "a skip outside a Push is nothing");
    }

    #[test]
    fn the_host_clock_reports_a_countdown_in_a_lull_and_zero_after_it() {
        let mut clock = HostClock::new();
        clock.enter(ClockPhase::Lull {
            countdown_ms: Some(180_000),
        });
        assert_eq!(
            clock.next_report(),
            Some((0, Some(180_000))),
            "the first is at once"
        );
        clock.answered();
        clock.elapse(100_000);
        assert_eq!(clock.next_report(), None, "not before the cadence");
        clock.elapse(150_000);
        assert_eq!(clock.next_report(), Some((250, Some(179_750))));
        clock.answered();
        assert_eq!(clock.timer_text(), "2:59");

        clock.enter(ClockPhase::Open);
        assert_eq!(
            clock.next_report(),
            Some((0, None)),
            "a recap has no countdown, so its report carries none (S1-11)"
        );
        assert_eq!(clock.timer_text(), "");
    }

    /// An untimed Lull reports the time it spends and no countdown, so the gateway's footer
    /// keeps it untimed rather than showing a timer at zero that ran out (S1-11's client
    /// half, decisions-log item 135).
    #[test]
    fn an_untimed_lull_reports_no_countdown_and_shows_none() {
        let mut clock = HostClock::new();
        clock.enter(ClockPhase::Lull { countdown_ms: None });
        assert_eq!(clock.next_report(), Some((0, None)));
        clock.answered();
        clock.elapse(250_000);
        assert_eq!(clock.next_report(), Some((250, None)));
        assert_eq!(clock.timer_text(), "");
        assert!(!clock.run_out());
    }

    #[test]
    fn a_stall_is_reported_in_steps_the_gateway_accepts() {
        let mut clock = HostClock::new();
        clock.enter(ClockPhase::Open);
        clock.elapse(150_000_000);
        assert_eq!(clock.next_report(), Some((60_000, None)));
        clock.answered();
        assert_eq!(
            clock.next_report(),
            Some((120_000, None)),
            "behind by more than a step, so the next is due at once"
        );
        clock.refused();
        assert_eq!(
            clock.next_report(),
            Some((120_000, None)),
            "a refused step is taken again from the last accepted"
        );
        clock.answered();
        clock.elapse(250_000);
        assert_eq!(clock.next_report(), Some((150_250, None)));
    }

    /// A phase that runs past what `report_host_clock`'s `int32` carries stops reporting
    /// and says so, rather than reporting the bound for ever; the next phase reports again.
    #[test]
    fn a_phase_past_the_wire_stops_reporting_and_says_so() {
        let mut clock = HostClock::new();
        clock.enter(ClockPhase::Open);
        clock.elapse(3_000_000_000_000);
        let mut last = 0;
        while let Some((report, _)) = clock.next_report() {
            assert!(report >= last, "the reports never run backwards");
            last = report;
            clock.answered();
        }
        assert!(clock.unreportable(), "it says why it stopped");
        assert!(
            last > i32::MAX - 60_000,
            "it reported up to the wire's bound: {last}"
        );
        clock.enter(ClockPhase::Open);
        assert!(!clock.unreportable());
        assert_eq!(clock.next_report(), Some((0, None)));
    }

    /// An answer that says the gateway ran more than it was asked is refused, and the ask
    /// is settled with nothing carried.
    #[test]
    fn an_advance_past_the_ask_is_refused() {
        let mut pacer = pushing(1);
        pacer.elapse(130_000);
        assert_eq!(pacer.next_ask(), Some(130));
        assert!(pacer.answered(131).is_err());
        assert_eq!(pacer.owed_ms(), 0, "nothing carried");
        pacer.elapse(PACER_PERIOD_US);
        assert_eq!(
            pacer.next_ask(),
            Some(100),
            "the next ask is the next period's"
        );
    }

    #[test]
    fn a_timed_lull_runs_out_and_an_untimed_one_never_does() {
        let mut clock = HostClock::new();
        clock.enter(ClockPhase::Lull {
            countdown_ms: Some(1_000),
        });
        clock.elapse(999_000);
        assert!(!clock.run_out());
        clock.elapse(1_000);
        assert!(clock.run_out());
        clock.enter(ClockPhase::Lull { countdown_ms: None });
        clock.elapse(10_000_000_000);
        assert!(!clock.run_out());
        // A timed Lull rejoined at zero has run out, and is not mistaken for an untimed one.
        clock.enter(ClockPhase::Lull {
            countdown_ms: Some(0),
        });
        assert!(clock.run_out());
        assert_eq!(clock.timer_text(), "0:00");
    }

    #[test]
    fn a_quiet_connection_is_due_a_keepalive() {
        let mut timing = Timing::default();
        timing.advance(0);
        timing.advance(KEEPALIVE_US - 1);
        assert!(!timing.keepalive_due(0));
        timing.advance(KEEPALIVE_US);
        assert!(timing.keepalive_due(0) && timing.keepalive_due(1));
        timing.sent(1);
        assert!(timing.keepalive_due(0) && !timing.keepalive_due(1));
    }

    #[test]
    fn full_is_owed_only_after_600_ms_with_no_edit() {
        let mut timing = Timing::default();
        timing.advance(0);
        assert!(!timing.idle.due(), "nothing was edited");
        timing.idle.edited();
        timing.advance(FULL_AFTER_IDLE_US - 1);
        assert!(!timing.idle.due());
        timing.idle.edited();
        timing.advance(FULL_AFTER_IDLE_US + 10);
        assert!(!timing.idle.due(), "an edit restarts the wait");
        timing.advance(2 * FULL_AFTER_IDLE_US);
        assert!(timing.idle.due());
        timing.idle.clear();
        timing.advance(4 * FULL_AFTER_IDLE_US);
        assert!(
            !timing.idle.due(),
            "asked for once, not again until the next edit"
        );
    }

    #[test]
    fn the_clock_text_is_minutes_and_seconds() {
        assert_eq!(clock_text(0), "0:00");
        assert_eq!(clock_text(59_999), "0:59");
        assert_eq!(clock_text(180_000), "3:00");
    }
}

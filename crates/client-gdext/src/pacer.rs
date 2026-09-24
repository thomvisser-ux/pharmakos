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
//! read from the gateway's own footer when the Lull opened, and zero in every other phase.
//!
//! # The keep-alive
//!
//! The gateway drops a connection that has been silent for 30 seconds
//! (`session::READ_TIMEOUT`), so each connection that has sent nothing for
//! [`KEEPALIVE_US`] sends a cheap call. A cheap call rather than a WebSocket ping, because
//! Godot's own heartbeat closes a connection whose pong is late, and a pong is late behind
//! a long `advance_push` on the one thread that owns the match.

use std::time::Instant;

/// The speeds the watch rig offers: 1x, 2x and 4x.
///
/// PLACEHOLDER: the spec says "2-4x" and does not say whether 3x is a speed. Tuning,
/// OWNER, settled at T16's review with the demo (skeleton-plan-t16a-notes.md section D).
pub const SPEEDS: [u32; 3] = [1, 2, 4];

/// How often, at most, the pacer asks for game time: 100 ms of wall time.
///
/// PLACEHOLDER: at least 50 ms, so the admin token's budget of 600 calls per 200 steps is
/// never approached — at 1x an ask of 100 ms runs two steps of the sim, so the pacer spends
/// at most one call per two. OWNER, Tuning, with the rate limits at hardening.
pub const PACER_PERIOD_US: u64 = 100_000;

/// The most one `advance_push` may ask for: the gateway's `MAX_ADVANCE_MS`, a wire bound
/// that the gateway refuses above rather than clamps.
pub const MAX_ADVANCE_MS: i32 = 60_000;

/// How much owed game time the pacer carries before it drops the rest: 5 seconds.
///
/// PLACEHOLDER: a stalled host or a long hitch is caught up by at most this much, so a
/// Push never plays a burst of minutes to make up for a frozen window. Tuning, OWNER.
pub const BACKLOG_BOUND_MS: u64 = 5_000;

/// How often the host clock is reported outside a Push: every 250 ms of wall time.
///
/// PLACEHOLDER: "about four times a second" (skeleton-plan-t16a-notes.md section A (2)).
/// The rate budget of every token refills at this grain in a Lull, so the editor's
/// verify-on-every-edit (T19) feels it. OWNER, with the rate limits at hardening.
pub const CLOCK_REPORT_US: u64 = 250_000;

/// The largest step one host-clock report may take: the gateway's own bound.
pub const MAX_CLOCK_STEP_MS: u64 = 60_000;

/// How long a connection may send nothing before it sends a keep-alive call: 5 seconds.
///
/// PLACEHOLDER: well inside the gateway's 30-second `READ_TIMEOUT` and the "at least every
/// 10 s" the plan asks for. OWNER, at hardening, together with `READ_TIMEOUT`.
pub const KEEPALIVE_US: u64 = 5_000_000;

/// How long the editor must be left alone before FULL runs: 600 ms of wall time.
///
/// Spec section 13, "Validation: QUICK runs on every edit and FULL after 600 ms idle". The
/// number is the spec's, not a tuning guess, and it is a question of *when* to ask, which is
/// why it lives beside the other three timers rather than in the editor: the editor asks the
/// gateway for every verdict and measures no time of its own (AGENTS.md section 3 rule 4).
pub const FULL_AFTER_IDLE_US: u64 = 600_000;

/// Microseconds in a millisecond.
const US_PER_MS: u64 = 1_000;

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
        self.owed_us.checked_div(US_PER_MS).unwrap_or(0)
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
        let limit = u64::try_from(MAX_ADVANCE_MS).unwrap_or(0);
        let whole = self.owed_ms().min(limit);
        if whole == 0 {
            return None;
        }
        let ask = i32::try_from(whole).unwrap_or(MAX_ADVANCE_MS);
        self.owed_us = self.owed_us.saturating_sub(whole.saturating_mul(US_PER_MS));
        self.since_ask_us = 0;
        self.in_flight = Some(ask);
        Some(ask)
    }

    /// The gateway ran `advanced_ms` of the ask in flight. What was asked and not run is
    /// carried into the next ask.
    pub fn answered(&mut self, advanced_ms: i32) {
        let Some(asked) = self.in_flight.take() else {
            return;
        };
        if !self.active || self.skipping {
            return;
        }
        let unrun = u64::try_from(asked.saturating_sub(advanced_ms).max(0)).unwrap_or(0);
        let owed = self.owed_us.saturating_add(unrun.saturating_mul(US_PER_MS));
        self.owed_us = self.bounded(owed);
    }

    /// The ask in flight was refused or lost: carry all of it.
    pub fn refused(&mut self) {
        self.answered(0);
    }

    /// `owed` with everything above the backlog bound dropped, and counted.
    fn bounded(&mut self, owed: u64) -> u64 {
        let bound = BACKLOG_BOUND_MS.saturating_mul(US_PER_MS);
        if owed > bound {
            let over = owed
                .saturating_sub(bound)
                .checked_div(US_PER_MS)
                .unwrap_or(0);
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
    /// A Lull whose countdown began at `total_ms`. Zero means untimed.
    Lull {
        /// The countdown the gateway's footer showed when the Lull opened.
        total_ms: u64,
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
        }
    }

    /// A new phase began: its time starts at zero, and its first report is due at once.
    pub fn enter(&mut self, phase: ClockPhase) {
        self.phase = phase;
        self.spent_us = 0;
        self.accepted_ms = 0;
        self.pending_ms = None;
        self.since_report_us = CLOCK_REPORT_US;
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
    ///
    /// After a stall the report is a step of at most [`MAX_CLOCK_STEP_MS`] beyond the last
    /// one the gateway accepted, so a long freeze is reported as several steps in a row.
    pub fn next_report(&mut self) -> Option<(i32, i32)> {
        if self.phase == ClockPhase::Silent || self.pending_ms.is_some() {
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
            ClockPhase::Lull { total_ms } => total_ms.saturating_sub(report),
            ClockPhase::Silent | ClockPhase::Open => 0,
        };
        self.pending_ms = Some(report);
        self.since_report_us = 0;
        Some((
            i32::try_from(report).unwrap_or(i32::MAX),
            i32::try_from(countdown).unwrap_or(i32::MAX),
        ))
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
            ClockPhase::Lull { total_ms } => total_ms > 0 && self.spent_ms() >= total_ms,
            ClockPhase::Silent | ClockPhase::Open => false,
        }
    }

    /// The Lull timer as the lobby shows it, `m:ss`; empty outside a timed Lull.
    #[must_use]
    pub fn timer_text(&self) -> String {
        match self.phase {
            ClockPhase::Lull { total_ms } if total_ms > 0 => {
                clock_text(total_ms.saturating_sub(self.spent_ms()))
            }
            ClockPhase::Lull { .. } | ClockPhase::Silent | ClockPhase::Open => String::new(),
        }
    }

    /// Whole milliseconds spent in this phase.
    fn spent_ms(&self) -> u64 {
        self.spent_us.checked_div(US_PER_MS).unwrap_or(0)
    }
}

/// Game or wall milliseconds as `m:ss`, rounded down to the second.
#[must_use]
pub fn clock_text(ms: u64) -> String {
    let seconds = ms.checked_div(1_000).unwrap_or(0);
    let minutes = seconds.checked_div(60).unwrap_or(0);
    let rest = seconds.checked_rem(60).unwrap_or(0);
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
    last_us: Option<u64>,
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
        let gone = self.last_us.map_or(0, |last| now_us.saturating_sub(last));
        self.last_us = Some(now_us.max(self.last_us.unwrap_or(0)));
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
    pub fn now_us(&self) -> u64 {
        self.last_us.unwrap_or(0)
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
        pacer.answered(MAX_ADVANCE_MS);
        assert_eq!(pacer.next_ask(), Some(MAX_ADVANCE_MS), "no period between");
        pacer.answered(20_000);
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
        clock.enter(ClockPhase::Lull { total_ms: 180_000 });
        assert_eq!(
            clock.next_report(),
            Some((0, 180_000)),
            "the first is at once"
        );
        clock.answered();
        clock.elapse(100_000);
        assert_eq!(clock.next_report(), None, "not before the cadence");
        clock.elapse(150_000);
        assert_eq!(clock.next_report(), Some((250, 179_750)));
        clock.answered();
        assert_eq!(clock.timer_text(), "2:59");

        clock.enter(ClockPhase::Open);
        assert_eq!(clock.next_report(), Some((0, 0)));
        assert_eq!(clock.timer_text(), "");
    }

    #[test]
    fn a_stall_is_reported_in_steps_the_gateway_accepts() {
        let mut clock = HostClock::new();
        clock.enter(ClockPhase::Open);
        clock.elapse(150_000_000);
        assert_eq!(clock.next_report(), Some((60_000, 0)));
        clock.answered();
        assert_eq!(
            clock.next_report(),
            Some((120_000, 0)),
            "behind by more than a step, so the next is due at once"
        );
        clock.refused();
        assert_eq!(
            clock.next_report(),
            Some((120_000, 0)),
            "a refused step is taken again from the last accepted"
        );
        clock.answered();
        clock.elapse(250_000);
        assert_eq!(clock.next_report(), Some((150_250, 0)));
    }

    #[test]
    fn a_timed_lull_runs_out_and_an_untimed_one_never_does() {
        let mut clock = HostClock::new();
        clock.enter(ClockPhase::Lull { total_ms: 1_000 });
        clock.elapse(999_000);
        assert!(!clock.run_out());
        clock.elapse(1_000);
        assert!(clock.run_out());
        clock.enter(ClockPhase::Lull { total_ms: 0 });
        clock.elapse(10_000_000_000);
        assert!(!clock.run_out());
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

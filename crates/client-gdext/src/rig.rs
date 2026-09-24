// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The watch rig's two connections: which call goes out next, and what each answer means.
//!
//! The lobby (GDScript) spawns `gamectl host`, reads its announce line, holds the two
//! tokens and the two WebSocket connections — the **admin** connection, which controls the
//! match, and the **seat** connection, which watches it (decisions-log item 107 (2) and
//! (3); `docs/design/skeleton-plan-t16a-notes.md` section A (2)). What goes over them is
//! decided here, because every one of those decisions is either a JSON-RPC frame to build
//! or read, which is marshalling, or a question of *when*, which is the pacer's and the
//! host clock's ([`crate::pacer`]) — and GDScript holds neither.
//!
//! Tokens never reach this module: they ride the WebSocket upgrade, which GDScript owns.
//!
//! # One request in flight per connection
//!
//! The gateway answers one request per connection at a time and in order, so the rig
//! keeps exactly one in flight on each and matches every answer to it by id. An answer
//! whose id is not the one in flight is dropped as stale — it can only be from before a
//! reconnect.
//!
//! # Whose footer is believed
//!
//! Every answer carries the `_status` footer, and the two connections' answers can arrive
//! in either order relative to each other. Only the **admin** connection changes the
//! phase — `end_lull`, `advance_push` and `end_recap` are admin methods — so only its
//! answers move the rig's phase. A seat answer served just before an `end_lull` and read
//! just after it would otherwise flip the rig back into the Lull and restart the clock.
//!
//! # What each connection sends, in priority order
//!
//! | admin | seat |
//! |---|---|
//! | `get_status` until the phase is known | a keyframe, or the next page of one |
//! | `end_lull` once the Lull is ready to end | `set_ready`, once the human says so |
//! | `end_recap` once the human continues | `get_view` from the cursor after the match moved |
//! | `advance_push`, when the pacer says | `get_segment_feed` after the match moved |
//! | `report_host_clock`, outside a Push | the editor's planning calls, in a Lull |
//! | a keep-alive `get_status` | a keep-alive `get_status` |
//!
//! "Ready ends the Lull": the host clock's answer carries `all_ready`, and when it is true
//! or the Lull's countdown has run out, the admin connection calls `end_lull`. That is the
//! one gameplay-shaped decision in this file, and it is the spec's, not the client's.
//! `set_ready` waits behind anything the editor still owes the gateway, so a Ready pressed
//! after Submit reaches the gateway after the submission it is ready with.
//!
//! # The seat connection's rate budget
//!
//! The editor shares the seat connection with the vista's polls
//! (`docs/design/skeleton-plan-t16a-notes.md` section B, "T19" (5)), and the seat token's
//! budget is the gateway's: `pharmakos_gateway::limit::CALLS_PER_TICK` calls per gateway
//! tick. That tick moves only when the match's clock does: in a Lull, when the admin
//! connection's `report_host_clock` is answered, about four times a second; in a Push, when
//! an `advance_push` ran any game time (section B, "T19" (3)). So the rig spends at most
//! [`SEAT_CALLS_PER_REFILL`] seat calls between two such answers. With one call in flight per
//! connection, at most one more can land in the same gateway tick, the one that was already
//! on its way when the clock moved, which keeps the seat under the gateway's limit however
//! fast the player edits (`tests/rate_budget.rs`).

use pharmakos_proto::gp::api::v1::{GetSegmentFeedResponse, status};
use pharmakos_proto::json::{self, Json};

use crate::editor::Editor;
use crate::enums;
use crate::error::BridgeError;
use crate::pacer::{CLOCK_REPORT_US, ClockPhase, KEEPALIVE_US, Timing, clock_text};
use crate::view::{read_status, split_footer};

/// Seat calls the rig sends between two moves of the gateway's clock.
///
/// The gateway admits a token eight calls per tick (`pharmakos_gateway::limit::
/// CALLS_PER_TICK`, itself a PLACEHOLDER for hardening). Six here plus the one call that can
/// straddle a clock move is seven, one under the limit, so a keep-alive or a late answer
/// never tips a burst of edits into `RATE_LIMITED`. `tests/rate_budget.rs` reads the
/// gateway's constants out of its source and drives the rig against them.
///
/// PLACEHOLDER: the margin is the client's; the numbers are OWNER's at hardening with the
/// gateway's rate limits, and this follows them.
pub const SEAT_CALLS_PER_REFILL: u32 = 6;

/// The admin connection's index.
pub const ADMIN: usize = 0;

/// The seat connection's index.
pub const SEAT: usize = 1;

/// The match phase, as the admin connection's latest footer says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// No admin answer has been read yet.
    Unknown,
    /// Before the match: not a phase this host serves, carried for completeness.
    Lobby,
    /// Planning.
    Lull,
    /// The segment plays.
    Push,
    /// The Ledger settles.
    Recap,
    /// The match is over.
    Ended,
}

impl Phase {
    /// The name GDScript and the logs print.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Lobby => "lobby",
            Self::Lull => "lull",
            Self::Push => "push",
            Self::Recap => "recap",
            Self::Ended => "ended",
        }
    }

    fn of(wire: i32) -> Self {
        match status::Phase::try_from(wire) {
            Ok(status::Phase::Lobby) => Self::Lobby,
            Ok(status::Phase::Lull) => Self::Lull,
            Ok(status::Phase::Push) => Self::Push,
            Ok(status::Phase::Recap) => Self::Recap,
            Ok(status::Phase::Ended) => Self::Ended,
            _ => Self::Unknown,
        }
    }
}

/// What a request was for, so its answer can be routed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Purpose {
    Status,
    View,
    Feed,
    Advance,
    Clock,
    EndLull,
    EndRecap,
    Ready,
    /// One of the editor's planning calls ([`crate::editor`]).
    Plan,
}

/// One connection's state.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Link {
    open: bool,
    next_id: i64,
    in_flight: Option<(i64, Purpose)>,
    drops: u32,
    calls: u64,
}

/// One frame to send: which connection, and the JSON-RPC text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Outgoing {
    /// [`ADMIN`] or [`SEAT`].
    pub link: usize,
    /// The request, as the gateway reads it.
    pub text: String,
}

/// What one answer meant.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Answer {
    /// A `get_view` result, footer included, for [`crate::view`] to decode and apply.
    pub view: Option<Json>,
    /// New event-list rows from `get_segment_feed`, in the feed's order.
    pub events: Vec<String>,
    /// The gateway's refusal, as `CODE: message`, when the call was refused.
    pub error: Option<String>,
    /// Whether this answer moved the phase.
    pub phase_changed: bool,
}

/// What the seat connection owes the view and the feed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Due {
    /// A keyframe: at the start, after a reconnect, and after a refused cursor.
    keyframe: bool,
    /// A `get_view` from the cursor, because the match moved.
    view: bool,
    /// A `get_segment_feed` from the cursor, because the match moved.
    feed: bool,
}

/// What the human, the timer or the host clock asked for and the rig has not yet sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Wants {
    end_lull: bool,
    end_recap: bool,
    ready: bool,
}

/// The watch rig: the two connections' state, the view and feed cursors, the timing, and
/// the editor that shares the seat connection.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rig {
    links: [Link; 2],
    phase: Phase,
    round: u32,
    timing: Timing,
    view_cursor: String,
    view_paging: Option<String>,
    feed_cursor: String,
    due: Due,
    wants: Wants,
    all_ready: bool,
    last_error: Option<String>,
    /// The Lull's length, from the rules table's `match.lull_ms`; zero is untimed.
    lull_length: u64,
    /// How many `get_view` pages this client refused after the gateway served them.
    view_refusals: u32,
    /// After a refused page: the wall time before which no keyframe is asked again.
    keyframe_not_before_us: u64,
    /// Seat calls left before the gateway's clock next moves.
    seat_budget: u32,
    /// Whether `set_ready` has gone out this Lull: the seat's orders are final, and the
    /// editor sends nothing more until the next Lull.
    ///
    /// PLACEHOLDER: Ready is final for the rest of the Lull; an un-ready toggle that reopens
    /// the editor (`set_ready {ready: false}`) is the real lobby's. OWNER, S6.
    ready_sent: bool,
    /// The playbook editor.
    editor: Editor,
}

impl Default for Rig {
    fn default() -> Self {
        Self::new()
    }
}

impl Rig {
    /// A rig with both connections closed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            links: [Link::default(), Link::default()],
            phase: Phase::Unknown,
            round: 0,
            timing: Timing::default(),
            view_cursor: String::new(),
            view_paging: None,
            feed_cursor: String::new(),
            due: Due {
                keyframe: true,
                view: false,
                feed: false,
            },
            wants: Wants::default(),
            all_ready: false,
            last_error: None,
            lull_length: 0,
            view_refusals: 0,
            keyframe_not_before_us: 0,
            seat_budget: SEAT_CALLS_PER_REFILL,
            ready_sent: false,
            editor: Editor::default(),
        }
    }

    /// The editor, to act on.
    pub fn editor_mut(&mut self) -> &mut Editor {
        &mut self.editor
    }

    /// The editor, to read.
    #[must_use]
    pub const fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Times every Lull at `length`, the rules table's `match.lull_ms`; zero is untimed.
    ///
    /// The gateway's footer shows the countdown this client reports and has none of its
    /// own before the first report, so the Lull's length is the client's to know
    /// (skeleton-plan-t16a-notes.md section D: "the client counts, the gateway is told").
    pub fn set_lull_length(&mut self, length: u64) {
        self.lull_length = length;
    }

    /// Connection `link` is open (again). A seat connection starts with a keyframe,
    /// because the feed keeps its state per viewer and asks nothing of a new connection.
    pub fn opened(&mut self, link: usize) {
        if let Some(state) = self.links.get_mut(link) {
            state.open = true;
            state.in_flight = None;
        }
        if link == SEAT {
            self.due.keyframe = true;
            self.view_paging = None;
            self.due.feed = true;
        }
    }

    /// The editor's edits restart the idle timer that owes FULL.
    fn sync_editor(&mut self) {
        if self.editor.take_edit_mark() {
            self.timing.idle.edited();
        }
    }

    /// Connection `link` dropped. Whatever it had in flight is carried or forgotten, and
    /// the drop is counted: the watch check asserts there were none.
    pub fn dropped(&mut self, link: usize) {
        let Some(state) = self.links.get_mut(link) else {
            return;
        };
        if state.open {
            state.drops = state.drops.saturating_add(1);
        }
        state.open = false;
        match state.in_flight.take() {
            Some((_, Purpose::Advance)) => self.timing.pacer.refused(),
            Some((_, Purpose::Clock)) => self.timing.clock.refused(),
            Some((_, Purpose::Plan)) => self.editor.dropped(),
            _ => {}
        }
    }

    /// The frames to send now, given a [`crate::pacer::WallClock`] reading.
    pub fn poll(&mut self, now_us: u64) -> Vec<Outgoing> {
        self.sync_editor();
        self.timing.advance(now_us);
        if self.phase == Phase::Lull && self.timing.clock.run_out() {
            self.wants.end_lull = true;
        }
        let mut out = Vec::new();
        if let Some((purpose, method, params)) = self.admin_next() {
            if let Some(frame) = self.send(ADMIN, purpose, method, params) {
                out.push(frame);
            }
        }
        if let Some((purpose, method, params)) = self.seat_next() {
            if let Some(frame) = self.send(SEAT, purpose, method, params) {
                out.push(frame);
            }
        }
        out
    }

    /// Whether `link` is open with nothing in flight.
    fn idle(&self, link: usize) -> bool {
        self.links
            .get(link)
            .is_some_and(|state| state.open && state.in_flight.is_none())
    }

    fn admin_next(&mut self) -> Option<(Purpose, &'static str, Json)> {
        if !self.idle(ADMIN) {
            return None;
        }
        if self.phase == Phase::Unknown {
            // At once the first time; after that at the clock's cadence, so a footer this
            // build cannot place (an unreadable one, or an unspecified phase) is asked about
            // again without spinning against the rate limiter.
            let first = self.links.get(ADMIN).is_some_and(|state| state.calls == 0);
            if first || self.timing.quiet_at_least(ADMIN, CLOCK_REPORT_US) {
                return Some((Purpose::Status, "get_status", object(Vec::new())));
            }
            return None;
        }
        if self.wants.end_lull && self.phase == Phase::Lull {
            return Some((Purpose::EndLull, "end_lull", object(Vec::new())));
        }
        if self.wants.end_recap && self.phase == Phase::Recap {
            return Some((Purpose::EndRecap, "end_recap", object(Vec::new())));
        }
        if self.phase == Phase::Push {
            if let Some(ask) = self.timing.pacer.next_ask() {
                return Some((
                    Purpose::Advance,
                    "advance_push",
                    object(vec![("ms", number(i64::from(ask)))]),
                ));
            }
        }
        if let Some((spent, countdown)) = self.timing.clock.next_report() {
            return Some((
                Purpose::Clock,
                "report_host_clock",
                object(vec![
                    ("elapsed_ms", number(i64::from(spent))),
                    ("remaining_ms", number(i64::from(countdown))),
                ]),
            ));
        }
        if self.timing.keepalive_due(ADMIN) {
            return Some((Purpose::Status, "get_status", object(Vec::new())));
        }
        None
    }

    fn seat_next(&mut self) -> Option<(Purpose, &'static str, Json)> {
        if !self.idle(SEAT) || self.seat_budget == 0 {
            return None;
        }
        if let Some(cursor) = self.view_paging.clone() {
            return Some((Purpose::View, "get_view", cursor_params(&cursor)));
        }
        if self.due.keyframe && self.timing.now_us() >= self.keyframe_not_before_us {
            return Some((Purpose::View, "get_view", cursor_params("")));
        }
        if self.wants.ready && self.phase == Phase::Lull && !self.editor.has_pending_jobs() {
            self.ready_sent = true;
            return Some((
                Purpose::Ready,
                "set_ready",
                object(vec![("ready", Json::Bool(true))]),
            ));
        }
        // Planning is open in a Lull and closed once this seat is ready or the Lull is
        // ending: a call sent into the end of a Lull would come back PHASE_CLOSED.
        if self.phase == Phase::Lull && !self.ready_sent && !self.wants.end_lull && !self.all_ready
        {
            let full_due = self.timing.idle.due();
            if let Some(call) = self.editor.next_call(full_due) {
                if call.full {
                    self.timing.idle.clear();
                }
                return Some((Purpose::Plan, call.method, call.params));
            }
        }
        if self.due.view {
            let cursor = self.view_cursor.clone();
            return Some((Purpose::View, "get_view", cursor_params(&cursor)));
        }
        if self.due.feed {
            let cursor = self.feed_cursor.clone();
            return Some((Purpose::Feed, "get_segment_feed", cursor_params(&cursor)));
        }
        if self.timing.keepalive_due(SEAT) {
            return Some((Purpose::Status, "get_status", object(Vec::new())));
        }
        None
    }

    /// Records the request as in flight and renders it.
    fn send(
        &mut self,
        link: usize,
        purpose: Purpose,
        method: &str,
        params: Json,
    ) -> Option<Outgoing> {
        let state = self.links.get_mut(link)?;
        state.next_id = state.next_id.saturating_add(1);
        let id = state.next_id;
        state.in_flight = Some((id, purpose));
        state.calls = state.calls.saturating_add(1);
        if link == SEAT {
            self.seat_budget = self.seat_budget.saturating_sub(1);
        }
        self.timing.sent(link);
        match purpose {
            Purpose::View => self.due.view = false,
            Purpose::Feed => self.due.feed = false,
            _ => {}
        }
        let text = json::write(&object(vec![
            ("jsonrpc", Json::String("2.0".to_owned())),
            ("id", number(id)),
            ("method", Json::String(method.to_owned())),
            ("params", params),
        ]));
        Some(Outgoing { link, text })
    }

    /// Reads one answer from connection `link`.
    ///
    /// # Errors
    ///
    /// [`BridgeError::Rpc`] for a frame that is not a JSON-RPC answer at all, and
    /// [`BridgeError::Schema`] for a result or footer the schema refuses. A refusal by the
    /// gateway is not an error here: it is an [`Answer`] whose `error` says what it was.
    pub fn receive(&mut self, link: usize, text: &str) -> Result<Answer, BridgeError> {
        let frame = json::read(text)?;
        let id = frame
            .get("id")
            .and_then(json_integer)
            .ok_or_else(|| BridgeError::Rpc("an answer with no numeric id".to_owned()))?;
        let Some(state) = self.links.get_mut(link) else {
            return Err(BridgeError::Rpc(format!("there is no connection {link}")));
        };
        let purpose = match state.in_flight {
            Some((waiting, purpose)) if waiting == id => {
                state.in_flight = None;
                purpose
            }
            // From before a reconnect: nobody is waiting for it.
            _ => return Ok(Answer::default()),
        };

        let mut answer = Answer::default();
        if let Some(error) = frame.get("error") {
            let code = error
                .get("data")
                .and_then(|data| data.get("code"))
                .and_then(json_text)
                .unwrap_or("ERROR")
                .to_owned();
            let message = error
                .get("message")
                .and_then(json_text)
                .unwrap_or("")
                .to_owned();
            self.refused(purpose, &code);
            if purpose == Purpose::Plan {
                // A refusal is an answer: the editor settles the call and says what it was.
                let _ = self.editor.answered(Err((code.as_str(), message.as_str())));
                self.sync_editor();
            }
            let said = format!("{code}: {message}");
            self.last_error = Some(said.clone());
            answer.error = Some(said);
            return Ok(answer);
        }
        let Some(result) = frame.get("result") else {
            // The call is settled either way: nothing is left waiting on it.
            self.refused(purpose, "RPC");
            return Err(BridgeError::Rpc(
                "an answer with neither result nor error".to_owned(),
            ));
        };
        // What the call was for is settled BEFORE the footer is read, so a footer this
        // build cannot read never leaves the pacer or the host clock waiting on an answer
        // that has already arrived.
        let settled = self.settle(purpose, result, &mut answer);
        self.sync_editor();
        settled?;
        if link == ADMIN {
            if let (_, Some(footer)) = split_footer(result) {
                answer.phase_changed = self.observe(&read_status(&footer)?);
            }
        }
        Ok(answer)
    }

    /// Routes one successful answer by what its call was for.
    fn settle(
        &mut self,
        purpose: Purpose,
        result: &Json,
        answer: &mut Answer,
    ) -> Result<(), BridgeError> {
        match purpose {
            Purpose::View => {
                let complete = matches!(result.get("complete"), Some(Json::Bool(true)));
                let next = result
                    .get("next_cursor")
                    .and_then(json_text)
                    .unwrap_or("")
                    .to_owned();
                if complete {
                    self.due.keyframe = false;
                    self.view_paging = None;
                    self.view_cursor = next;
                } else {
                    self.view_paging = Some(next);
                }
                answer.view = Some(result.clone());
            }
            Purpose::Feed => {
                let (body, _) = split_footer(result);
                let feed: GetSegmentFeedResponse = json::decode_json(&enums::canonical(
                    "gp.api.v1.GetSegmentFeedResponse",
                    &body,
                ))?;
                answer.events = event_rows(&feed);
                if !feed.next_cursor.is_empty() {
                    self.feed_cursor = feed.next_cursor;
                }
            }
            Purpose::Advance => {
                let ran = result
                    .get("advanced_ms")
                    .and_then(json_integer)
                    .and_then(|value| i32::try_from(value).ok())
                    .unwrap_or(0);
                self.timing.pacer.answered(ran);
                if ran > 0 {
                    self.due.view = true;
                    self.due.feed = true;
                    // The Push moved the gateway's clock, and with it every token's budget.
                    self.seat_budget = SEAT_CALLS_PER_REFILL;
                }
            }
            Purpose::Clock => {
                self.timing.clock.answered();
                // The report moved the gateway's clock, and with it every token's budget.
                self.seat_budget = SEAT_CALLS_PER_REFILL;
                self.all_ready = matches!(result.get("all_ready"), Some(Json::Bool(true)));
                if self.all_ready && self.phase == Phase::Lull {
                    self.wants.end_lull = true;
                }
            }
            Purpose::EndLull => self.wants.end_lull = false,
            Purpose::EndRecap => self.wants.end_recap = false,
            Purpose::Ready => self.wants.ready = false,
            Purpose::Plan => self.editor.answered(Ok(result))?,
            Purpose::Status => {}
        }
        Ok(())
    }

    /// The bridge could not decode or apply a `get_view` page the rig had already read.
    ///
    /// The cursor the page carried is not trusted: the page is lost, and a view with a hole
    /// in it cannot be told from a map with one, so the seat asks for a keyframe again —
    /// no sooner than [`KEEPALIVE_US`] from now, so a page this build can never read is
    /// not asked for in a loop.
    pub fn view_refused(&mut self) {
        self.view_refusals = self.view_refusals.saturating_add(1);
        self.due.keyframe = true;
        self.view_paging = None;
        self.keyframe_not_before_us = self.timing.now_us().saturating_add(KEEPALIVE_US);
    }

    /// Undoes what a refused call had counted on.
    fn refused(&mut self, purpose: Purpose, code: &str) {
        match purpose {
            Purpose::Advance => self.timing.pacer.refused(),
            Purpose::Clock => self.timing.clock.refused(),
            Purpose::View => {
                // A cursor from before an attach, or any other refusal: start again from
                // a keyframe, which is what the feed asks of a client that lost its place.
                self.due.keyframe = true;
                self.view_paging = None;
            }
            Purpose::Feed => {
                if code == "STALE_SNAPSHOT" {
                    self.feed_cursor.clear();
                }
            }
            Purpose::EndLull => self.wants.end_lull = false,
            Purpose::EndRecap => self.wants.end_recap = false,
            Purpose::Ready => {
                self.wants.ready = false;
                self.ready_sent = false;
            }
            Purpose::Plan | Purpose::Status => {}
        }
    }

    /// Takes in an admin footer. Returns whether the phase moved.
    fn observe(&mut self, footer: &pharmakos_proto::gp::api::v1::Status) -> bool {
        let phase = Phase::of(footer.phase);
        self.round = footer.round;
        if phase == self.phase {
            return false;
        }
        self.phase = phase;
        self.wants.end_lull = false;
        self.wants.end_recap = false;
        self.all_ready = false;
        // Something about the view may have changed at the boundary: the unlock at match
        // end is a policy change, and a new segment makes the feed cursor stale.
        self.due.view = true;
        self.due.feed = true;
        match phase {
            Phase::Push => {
                self.feed_cursor.clear();
                self.timing.pacer.start();
                self.timing.clock.enter(ClockPhase::Silent);
            }
            Phase::Lull => {
                self.ready_sent = false;
                self.editor.lull_opened(footer.round);
                self.timing.pacer.stop();
                // A footer that already shows a countdown is one this client reported
                // before a reconnect; otherwise the Lull is as long as the rules say.
                let shown = u64::try_from(footer.phase_remaining_ms).unwrap_or(0);
                let total = if shown > 0 { shown } else { self.lull_length };
                self.timing
                    .clock
                    .enter(ClockPhase::Lull { total_ms: total });
            }
            Phase::Recap | Phase::Ended => {
                self.timing.pacer.stop();
                self.timing.clock.enter(ClockPhase::Open);
            }
            Phase::Unknown | Phase::Lobby => {
                self.timing.pacer.stop();
                self.timing.clock.enter(ClockPhase::Silent);
            }
        }
        true
    }

    /// Chooses a speed; refused when it is not one of [`crate::pacer::SPEEDS`].
    pub fn set_speed(&mut self, speed: u32) -> bool {
        self.timing.pacer.set_speed(speed)
    }

    /// Skip to the end of the segment. Only in a Push.
    pub fn skip(&mut self) {
        self.timing.pacer.skip();
    }

    /// The human pressed Ready.
    pub fn ready(&mut self) {
        if self.phase == Phase::Lull {
            self.wants.ready = true;
        }
    }

    /// End the Lull now: what the lobby does when every seat is ready or the timer runs
    /// out, and what the headless watch check does after its idle stretch.
    pub fn end_lull(&mut self) {
        if self.phase == Phase::Lull {
            self.wants.end_lull = true;
        }
    }

    /// Leave the recap for the next Lull.
    pub fn end_recap(&mut self) {
        if self.phase == Phase::Recap {
            self.wants.end_recap = true;
        }
    }

    /// The phase, as the admin connection last heard it.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// The round, as the admin connection last heard it.
    #[must_use]
    pub const fn round(&self) -> u32 {
        self.round
    }

    /// The pacer's speed.
    #[must_use]
    pub const fn speed(&self) -> u32 {
        self.timing.pacer.speed()
    }

    /// Whether a skip is under way.
    #[must_use]
    pub const fn skipping(&self) -> bool {
        self.timing.pacer.skipping()
    }

    /// Whether the last host-clock answer said every seat is ready.
    #[must_use]
    pub const fn all_ready(&self) -> bool {
        self.all_ready
    }

    /// The Lull timer as the lobby shows it; empty outside a timed Lull.
    #[must_use]
    pub fn timer_text(&self) -> String {
        self.timing.clock.timer_text()
    }

    /// How many times connection `link` dropped while open.
    #[must_use]
    pub fn drops(&self, link: usize) -> u32 {
        self.links.get(link).map_or(0, |state| state.drops)
    }

    /// How many calls connection `link` has sent.
    #[must_use]
    pub fn calls(&self, link: usize) -> u64 {
        self.links.get(link).map_or(0, |state| state.calls)
    }

    /// The last refusal the gateway answered with, if any.
    #[must_use]
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Whether the seat's view is complete and current: no keyframe or page waiting.
    #[must_use]
    pub const fn view_settled(&self) -> bool {
        !self.due.keyframe && self.view_paging.is_none()
    }

    /// Whether the seat connection has caught up: its view settled, no view or feed poll
    /// owed, and nothing in flight.
    #[must_use]
    pub fn seat_settled(&self) -> bool {
        self.view_settled() && !self.due.view && !self.due.feed && self.idle(SEAT)
    }

    /// How many `get_view` pages the bridge refused after the gateway served them.
    #[must_use]
    pub const fn view_refusals(&self) -> u32 {
        self.view_refusals
    }
}

/// The event list's rows for one `get_segment_feed` page, in the feed's order.
///
/// Each row is the event's time in the segment, its kind and its text, exactly as the
/// gateway wrote them. Nothing is added, merged, reworded or filtered: the event list shows
/// what the feed said, which is what its golden
/// (`tests/golden/vista/expected.events.txt`) holds the client to.
#[must_use]
pub fn event_rows(feed: &GetSegmentFeedResponse) -> Vec<String> {
    feed.events
        .iter()
        .map(|event| {
            let when = clock_text(u64::try_from(event.at_ms).unwrap_or(0));
            format!("{when}  {}  {}", event.kind, event.text)
        })
        .collect()
}

fn object(entries: Vec<(&str, Json)>) -> Json {
    Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn number(value: i64) -> Json {
    Json::Number(value.to_string())
}

fn cursor_params(cursor: &str) -> Json {
    if cursor.is_empty() {
        object(Vec::new())
    } else {
        object(vec![("cursor", Json::String(cursor.to_owned()))])
    }
}

fn json_integer(value: &Json) -> Option<i64> {
    match value {
        Json::Number(lexeme) => lexeme.parse::<i64>().ok(),
        _ => None,
    }
}

fn json_text(value: &Json) -> Option<&str> {
    match value {
        Json::String(text) => Some(text.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pacer::PACER_PERIOD_US;

    fn answer(id: i64, result: &str, phase: &str) -> String {
        let body = result.strip_suffix('}').expect("a result is an object");
        let sep = if body.len() > 1 { "," } else { "" };
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{body}{sep}"_status":{{"phase":"{phase}","phase_remaining_ms":180000,"round":1}}}}}}"#
        )
    }

    fn id_of(frame: &Outgoing) -> i64 {
        json::read(&frame.text)
            .expect("json")
            .get("id")
            .and_then(json_integer)
            .expect("an id")
    }

    fn method_of(frame: &Outgoing) -> String {
        json::read(&frame.text)
            .expect("json")
            .get("method")
            .and_then(json_text)
            .expect("a method")
            .to_owned()
    }

    /// A rig with both connections open, the phase learnt, and the keyframe complete.
    fn in_lull() -> Rig {
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        rig.opened(SEAT);
        let sent = rig.poll(0);
        assert_eq!(sent.len(), 2);
        let status = sent.first().expect("admin first");
        assert_eq!(method_of(status), "get_status");
        let view = sent.get(1).expect("seat second");
        assert_eq!(method_of(view), "get_view");
        let got = rig
            .receive(ADMIN, &answer(id_of(status), "{}", "lull"))
            .expect("reads");
        assert!(got.phase_changed);
        assert_eq!(rig.phase(), Phase::Lull);
        let got = rig
            .receive(
                SEAT,
                &answer(
                    id_of(view),
                    r#"{"next_cursor":"c1","complete":true}"#,
                    "lull",
                ),
            )
            .expect("reads");
        assert!(got.view.is_some());
        assert!(rig.view_settled());
        settle(&mut rig, 0);
        rig
    }

    /// Answers every call the rig makes at `now` with an unremarkable result, until it
    /// has nothing left to send.
    fn settle(rig: &mut Rig, now: u64) {
        for _ in 0..16 {
            let sent = rig.poll(now);
            if sent.is_empty() {
                return;
            }
            for frame in sent {
                let result = match method_of(&frame).as_str() {
                    "get_view" => r#"{"next_cursor":"c9","complete":true}"#,
                    "get_segment_feed" => r#"{"events":[],"next_cursor":"f1"}"#,
                    "report_host_clock" => r#"{"all_ready":false}"#,
                    _ => "{}",
                };
                let phase = rig.phase().name();
                rig.receive(frame.link, &answer(id_of(&frame), result, phase))
                    .expect("reads");
            }
        }
        panic!("the rig never went quiet");
    }

    #[test]
    fn a_lull_reports_the_clock_and_ends_when_every_seat_is_ready() {
        let mut rig = in_lull();
        let sent = rig.poll(CLOCK_REPORT_US);
        let clock = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("an admin call");
        assert_eq!(method_of(clock), "report_host_clock");
        assert!(clock.text.contains("\"remaining_ms\""), "{}", clock.text);
        rig.receive(
            ADMIN,
            &answer(id_of(clock), r#"{"all_ready":true}"#, "lull"),
        )
        .expect("reads");
        let sent = rig.poll(CLOCK_REPORT_US + 1);
        let end = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("an admin call");
        assert_eq!(method_of(end), "end_lull");
    }

    #[test]
    fn a_push_is_paced_and_the_seat_follows_each_advance() {
        let mut rig = in_lull();
        rig.end_lull();
        let sent = rig.poll(1);
        let end = sent.first().expect("end_lull");
        assert_eq!(method_of(end), "end_lull");
        rig.receive(ADMIN, &answer(id_of(end), "{}", "push"))
            .expect("reads");
        assert_eq!(rig.phase(), Phase::Push);
        assert!(rig.set_speed(4));
        settle(&mut rig, 1);

        let sent = rig.poll(1 + PACER_PERIOD_US);
        let advance = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("an advance");
        assert_eq!(method_of(advance), "advance_push");
        assert!(advance.text.contains("400"), "{}", advance.text);
        rig.receive(
            ADMIN,
            &answer(id_of(advance), r#"{"advanced_ms":400}"#, "push"),
        )
        .expect("reads");
        let sent = rig.poll(2 + PACER_PERIOD_US);
        assert!(
            sent.iter()
                .any(|frame| frame.link == SEAT && method_of(frame) == "get_view"),
            "the seat looks again after the match moved: {sent:?}"
        );
    }

    #[test]
    fn a_seat_answer_never_moves_the_phase() {
        let mut rig = in_lull();
        rig.due.view = true;
        let sent = rig.poll(1);
        let view = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a view poll");
        let got = rig
            .receive(
                SEAT,
                &answer(
                    id_of(view),
                    r#"{"next_cursor":"c2","complete":true}"#,
                    "push",
                ),
            )
            .expect("reads");
        assert!(!got.phase_changed);
        assert_eq!(rig.phase(), Phase::Lull);
    }

    #[test]
    fn a_stale_cursor_asks_for_a_keyframe_again() {
        let mut rig = in_lull();
        rig.due.view = true;
        let sent = rig.poll(1);
        let view = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a view poll");
        let refusal = format!(
            r#"{{"jsonrpc":"2.0","id":{},"error":{{"code":-32000,"message":"stale","data":{{"code":"STALE_SNAPSHOT"}}}}}}"#,
            id_of(view)
        );
        let got = rig.receive(SEAT, &refusal).expect("reads");
        assert!(got.error.is_some());
        let sent = rig.poll(2);
        let again = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a keyframe");
        assert!(again.text.contains("\"params\": {}"), "{}", again.text);
    }

    #[test]
    fn a_quiet_connection_sends_a_keepalive_and_a_drop_is_counted() {
        let mut rig = in_lull();
        let sent = rig.poll(KEEPALIVE_US + 10);
        assert!(
            sent.iter()
                .any(|frame| frame.link == SEAT && method_of(frame) == "get_status"),
            "{sent:?}"
        );
        rig.dropped(SEAT);
        assert_eq!(rig.drops(SEAT), 1);
        rig.opened(SEAT);
        assert!(!rig.view_settled(), "a reconnect starts from a keyframe");
    }

    /// A rig in a Push at 4x, with nothing owed yet.
    fn in_push() -> Rig {
        let mut rig = in_lull();
        rig.end_lull();
        let sent = rig.poll(1);
        let end = sent.first().expect("end_lull");
        rig.receive(ADMIN, &answer(id_of(end), "{}", "push"))
            .expect("reads");
        assert!(rig.set_speed(4));
        settle(&mut rig, 1);
        rig
    }

    #[test]
    fn the_host_clock_is_reported_in_a_recap_and_after_the_match_ends() {
        for phase in ["recap", "ended"] {
            let mut rig = in_push();
            let sent = rig.poll(1 + PACER_PERIOD_US);
            let advance = sent
                .iter()
                .find(|frame| frame.link == ADMIN)
                .expect("an advance");
            rig.receive(
                ADMIN,
                &answer(id_of(advance), r#"{"advanced_ms":400}"#, phase),
            )
            .expect("reads");
            assert_eq!(rig.phase().name(), phase);
            let sent = rig.poll(2 + PACER_PERIOD_US);
            let clock = sent
                .iter()
                .find(|frame| frame.link == ADMIN)
                .expect("an admin call");
            assert_eq!(method_of(clock), "report_host_clock", "{phase}");
            assert!(
                clock.text.contains(r#""remaining_ms": 0"#),
                "{phase}: {}",
                clock.text
            );
            rig.receive(ADMIN, &answer(id_of(clock), "{}", phase))
                .expect("reads");
            let sent = rig.poll(2 + PACER_PERIOD_US + CLOCK_REPORT_US);
            assert!(
                sent.iter()
                    .any(|frame| frame.link == ADMIN && method_of(frame) == "report_host_clock"),
                "{phase}: reported again at the cadence: {sent:?}"
            );
        }
    }

    #[test]
    fn an_unreadable_footer_leaves_nothing_waiting() {
        let mut rig = in_push();
        let sent = rig.poll(1 + PACER_PERIOD_US);
        let advance = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("an advance");
        let broken = format!(
            r#"{{"jsonrpc":"2.0","id":{},"result":{{"advanced_ms":400,"_status":{{"phase":"push","not_a_field":1}}}}}}"#,
            id_of(advance)
        );
        rig.receive(ADMIN, &broken)
            .expect_err("the footer is not a Status");
        let sent = rig.poll(1 + 2 * PACER_PERIOD_US);
        assert!(
            sent.iter()
                .any(|frame| frame.link == ADMIN && method_of(frame) == "advance_push"),
            "the pacer asks again: {sent:?}"
        );
    }

    #[test]
    fn an_unknown_phase_is_asked_about_at_the_clocks_cadence() {
        let mut rig = Rig::new();
        rig.opened(ADMIN);
        let sent = rig.poll(0);
        let status = sent.first().expect("the first get_status");
        let no_phase = format!(
            r#"{{"jsonrpc":"2.0","id":{},"result":{{"_status":{{"round":1}}}}}}"#,
            id_of(status)
        );
        rig.receive(ADMIN, &no_phase).expect("reads");
        assert_eq!(rig.phase(), Phase::Unknown);
        assert!(rig.poll(1).is_empty(), "not again at once");
        let sent = rig.poll(CLOCK_REPORT_US);
        assert_eq!(sent.len(), 1, "{sent:?}");
    }

    #[test]
    fn a_page_the_client_refused_is_asked_for_again_as_a_keyframe() {
        let mut rig = in_lull();
        rig.due.view = true;
        let sent = rig.poll(1);
        let view = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a view poll");
        rig.receive(
            SEAT,
            &answer(
                id_of(view),
                r#"{"next_cursor":"c2","complete":true}"#,
                "lull",
            ),
        )
        .expect("reads");
        rig.view_refused();
        assert_eq!(rig.view_refusals(), 1);
        assert!(!rig.view_settled());
        assert!(
            !rig.poll(2)
                .iter()
                .any(|frame| frame.link == SEAT && method_of(frame) == "get_view"),
            "not in a loop"
        );
        let sent = rig.poll(2 + KEEPALIVE_US);
        let again = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a keyframe");
        assert_eq!(method_of(again), "get_view");
        assert!(again.text.contains("\"params\": {}"), "{}", again.text);
    }

    #[test]
    fn an_answer_nobody_is_waiting_for_is_dropped() {
        let mut rig = in_lull();
        let got = rig
            .receive(ADMIN, &answer(999, "{}", "push"))
            .expect("reads");
        assert_eq!(got, Answer::default());
        assert_eq!(rig.phase(), Phase::Lull);
    }

    /// A rig in a Lull whose editor holds a playbook it has checked.
    fn editing() -> Rig {
        let mut rig = in_lull();
        assert!(rig.editor_mut().load(b"{}"));
        let sent = rig.poll(2);
        let check = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("the load check");
        assert_eq!(method_of(check), "verify_plan");
        rig.receive(
            SEAT,
            &answer(
                id_of(check),
                r#"{"report":{"qualifies":true,"depth":"quick"}}"#,
                "lull",
            ),
        )
        .expect("reads");
        assert!(rig.editor().has_text());
        rig
    }

    #[test]
    fn ready_waits_behind_the_submission_it_is_ready_with() {
        let mut rig = editing();
        assert!(rig.editor_mut().submit());
        rig.ready();
        let sent = rig.poll(3);
        let first = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a seat call");
        assert_eq!(method_of(first), "submit_plan", "the submission goes first");
        rig.receive(
            SEAT,
            &answer(
                id_of(first),
                r#"{"report":{"qualifies":true,"depth":"full"},"accepted":true}"#,
                "lull",
            ),
        )
        .expect("reads");
        let sent = rig.poll(4);
        let next = sent
            .iter()
            .find(|frame| frame.link == SEAT)
            .expect("a seat call");
        assert_eq!(method_of(next), "set_ready");
        rig.receive(SEAT, &answer(id_of(next), "{}", "lull"))
            .expect("reads");
        // Ready means the orders are final: the editor sends nothing more this Lull.
        assert!(rig.editor_mut().act(
            crate::editor::Action::Go,
            &crate::editor::Target::Beacon("b_00".to_owned())
        ));
        assert!(
            !rig.poll(5)
                .iter()
                .any(|frame| frame.link == SEAT && method_of(frame) == "patch_plan"),
            "no planning call after Ready"
        );
    }

    #[test]
    fn planning_calls_wait_for_a_lull_and_for_budget() {
        let mut rig = editing();
        rig.end_lull();
        let sent = rig.poll(3);
        let end = sent
            .iter()
            .find(|frame| frame.link == ADMIN)
            .expect("end_lull");
        rig.receive(ADMIN, &answer(id_of(end), "{}", "push"))
            .expect("reads");
        assert_eq!(rig.phase(), Phase::Push);
        assert!(rig.editor_mut().submit());
        settle(&mut rig, 4);
        assert!(
            rig.editor().has_pending_jobs(),
            "planning is closed in a Push; the submission waits for the next Lull"
        );
    }

    #[test]
    fn the_seat_spends_no_more_than_its_budget_between_clock_reports() {
        let mut rig = editing();
        for _ in 0..12 {
            assert!(rig.editor_mut().act(
                crate::editor::Action::Go,
                &crate::editor::Target::Beacon("b_00".to_owned())
            ));
        }
        let mut seat_calls = 0_u32;
        // Answer every seat call at once, with no clock report in between.
        for step in 0..64_u64 {
            let sent = rig.poll(3 + step);
            for frame in sent.iter().filter(|frame| frame.link == SEAT) {
                seat_calls += 1;
                let result = match method_of(frame).as_str() {
                    "patch_plan" => {
                        r#"{"playbook_jsonc":"{ }","inverse_json_patch":"[]"}"#.to_owned()
                    }
                    _ => r#"{"report":{"qualifies":true,"depth":"quick"}}"#.to_owned(),
                };
                rig.receive(SEAT, &answer(id_of(frame), &result, "lull"))
                    .expect("reads");
            }
            // The admin connection's calls are left unanswered: the clock never moves.
        }
        assert!(
            seat_calls <= SEAT_CALLS_PER_REFILL,
            "{seat_calls} seat calls with the gateway's clock standing still"
        );
    }
}

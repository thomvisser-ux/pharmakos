// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The runner: Lull, Push and recap as phases in their fixed order, the
//! segment ladder, the frozen segment-end snapshot, the one-tick match-end rule
//! and the round limit.
//!
//! Spec section 3 (Match structure), section 4 (Commander, death), section 15;
//! decisions log items 16, 19, 20, 21, 30, 40 and 68.
//!
//! # Two objects, and the line between them
//!
//! * [`MatchState`] is **hashed state** and lives inside [`World`]: the phase,
//!   the round, the round limit, the effective per-round length list, the
//!   segment's start tick and length, the coming segment's length, and the
//!   outcome. Every one of those decides what a tick does, so every one of them
//!   is in the state hash, in the snapshot round-trip and in the goldens
//!   (AGENTS.md §4.8). [`crate::world::Phase::Match`] is the tick phase that
//!   moves it.
//! * [`Runner`] is the **driver**: it owns a world and an encoder, it steps the
//!   world only while the phase is a Push, and it takes the frozen snapshot at
//!   segment end. It carries no rule of its own that the world does not.
//!
//! `World::step` is the tick and runs whatever phase it is in; `Runner` is what
//! decides whether a tick happens at all. That split is what lets a test, a
//! bench or `fork` drive the world directly without walking into the match
//! machinery, and it is why a world stepped by hand stays in its opening Lull
//! for ever: a Push is something a host opens.
//!
//! # The Lull and the recap have no clock in them
//!
//! AGENTS.md §4.5: the Lull's timer is a UI and host concern the sim never
//! reads, and `rules.match.lull_ms` is read by the host and the editor and by
//! nothing here. The spec says the same of the recap — "timer not running". So
//! in the sim the Lull and the recap are **states, not durations**: they consume
//! no tick, and they end on the host's word ([`Runner::begin_push`] when every
//! seat is ready or the timer ran out, [`Runner::end_recap`] when the recap is
//! watched or skipped). The consequence is worth stating plainly, because it
//! looks like an omission and is not: the per-tick hash chain contains Push
//! ticks and nothing else, and a phase change shows up in the chain at the tick
//! that caused it or at the next tick that runs.
//!
//! # The segment ladder, and why the coming length rides in the snapshot
//!
//! Item 30 fixed a ladder by round number (3/4/5/6/7/8 minutes) and item 40
//! amended the host's override into a **per-round length list**: empty means the
//! ladder, one value means that length every round, N values mean round *i*
//! takes the *i*-th entry and rounds past the end take the last entry. That list
//! is resolved **once**, at [`World::new`], into [`MatchState`], and from then
//! on the rules table is not consulted about segment length again.
//!
//! The coming segment's length is written at segment end, into
//! [`MatchState::coming_segment_ms`], and travels in the snapshot — which is
//! what `get_status`, `get_briefing`, the editor's clock, the fits pill and
//! `render_plan` all read (item 30, and `plan-core`'s `context::segment_length_ms`,
//! whose PLACEHOLDER named this task). It is emphatically *not* re-derived from
//! `rules.match.segment_lengths_ms` when the next Push opens:
//! `the_coming_segments_length_comes_from_the_snapshot_not_a_constant` restores
//! a frozen snapshot into a world built under a **different** ladder and shows
//! the Push runs the frozen length.
//!
//! # Match end is checked every tick
//!
//! Item 16, the one-tick rule: end conditions are evaluated every tick, the
//! Push halts at the **first tick** where fewer than two seats remain, standings
//! freeze and the recap plays. Not at the segment boundary — which is what
//! `the_match_end_rule_is_checked_at_the_tick_not_at_the_segment_boundary`
//! pins. If no seat survives that tick the final audit at that tick decides, so
//! the outcome carries [`MatchEndReason::NoSurvivor`] and no winner rather than
//! a draw state. The match also ends at the host-set round limit, which is
//! decided when the last round's recap closes.

use crate::encoding::Enc;
use crate::events::{Emission, Event, EventKind};
use crate::math::quantity::{MS_PER_TICK, Ms, Tick};
use crate::rules::RulesTable;
use crate::snapshot::{Snapshot, SnapshotError};
use crate::tables::SeatId;
use crate::world::World;

/// The round limit a host gets when it names none (spec section 3: "default 6,
/// about 75 min").
///
/// PLACEHOLDER: the round limit is a lobby setting and has no rules-table row,
/// because it is not tuning — the host picks it per match. The *default* is the
/// spec's, and the owner ratifies it with the lobby at T22.
pub const DEFAULT_ROUND_LIMIT: u32 = 6;

/// The three phases of a round, in the order they run.
///
/// [`MatchPhase::Ended`] is terminal and outside the cycle: a match that has
/// ended plays no further round.
pub const PHASE_CYCLE: [MatchPhase; 3] = [MatchPhase::Lull, MatchPhase::Push, MatchPhase::Recap];

/// Where a match is.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum MatchPhase {
    /// Planning. Seats seal playbooks against the frozen snapshot; nothing
    /// moves and no tick runs.
    #[default]
    Lull,
    /// Play. The only phase in which the sim ticks.
    Push,
    /// Settlement, awards and highlights. No tick runs; the frozen snapshot has
    /// been taken.
    Recap,
    /// The match is over. Terminal.
    Ended,
}

impl MatchPhase {
    /// The wire id. Additive only: never reuse, never renumber.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            MatchPhase::Lull => 1,
            MatchPhase::Push => 2,
            MatchPhase::Recap => 3,
            MatchPhase::Ended => 4,
        }
    }

    /// The phase an id names, or `None` for an id this build does not define.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<MatchPhase> {
        match id {
            1 => Some(MatchPhase::Lull),
            2 => Some(MatchPhase::Push),
            3 => Some(MatchPhase::Recap),
            4 => Some(MatchPhase::Ended),
            _ => None,
        }
    }

    /// The phase's name, in the spelling a scenario file and the gateway both
    /// use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            MatchPhase::Lull => "lull",
            MatchPhase::Push => "push",
            MatchPhase::Recap => "recap",
            MatchPhase::Ended => "ended",
        }
    }
}

/// Why a match ended (item 16).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum MatchEndReason {
    /// Fewer than two seats remained at a tick, and one of them was alive: the
    /// last seat standing wins.
    LastSeatStanding,
    /// Fewer than two seats remained and none survived that tick. The final
    /// audit at that tick decides; there is no draw state.
    NoSurvivor,
    /// The host-set round limit was reached.
    RoundLimit,
}

impl MatchEndReason {
    /// The wire id. Additive only; `0` is reserved for "the match has not
    /// ended", which is why no variant carries it.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            MatchEndReason::LastSeatStanding => 1,
            MatchEndReason::NoSurvivor => 2,
            MatchEndReason::RoundLimit => 3,
        }
    }

    /// The reason an id names, or `None` for `0` and for anything this build
    /// does not define.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<MatchEndReason> {
        match id {
            1 => Some(MatchEndReason::LastSeatStanding),
            2 => Some(MatchEndReason::NoSurvivor),
            3 => Some(MatchEndReason::RoundLimit),
            _ => None,
        }
    }

    /// The name a scenario file and the recap both use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            MatchEndReason::LastSeatStanding => "last_seat_standing",
            MatchEndReason::NoSurvivor => "no_survivor",
            MatchEndReason::RoundLimit => "round_limit",
        }
    }
}

/// How a match ended.
///
/// PLACEHOLDER: the winner of a [`MatchEndReason::RoundLimit`] or
/// [`MatchEndReason::NoSurvivor`] end is decided by the **final audit** — value
/// held after the final settlement plus enemy value destroyed, with item 4's
/// tie-break order — and the audit is the economy's, which is T14's. Until then
/// both carry `winner: None` and the runner records the reason and the tick
/// honestly rather than inventing a standing (owner, at T14).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatchOutcome {
    /// Why it ended.
    pub reason: MatchEndReason,
    /// Who won, when the rule that ended it names somebody.
    pub winner: Option<SeatId>,
    /// The tick it ended on. For the one-tick rule this is the tick the
    /// condition first held, not the segment's last tick.
    pub at: Tick,
}

/// The host's match settings: what the lobby decides and the rules table does
/// not.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MatchSettings {
    /// The per-round segment length list, in game milliseconds (item 40).
    ///
    /// Empty means the rules table's ladder (`match.segment_lengths_ms`), which
    /// is item 68's 3 / 5 / 8 at the skeleton. One value means that length
    /// every round; N values mean round *i* takes the *i*-th entry and rounds
    /// past the end take the last entry. The gate's "3 rounds at 3 / 5 / 8 min"
    /// is the list `[180000, 300000, 480000]`.
    pub segment_lengths_ms: Vec<i32>,
    /// How many rounds the match plays before the round limit ends it.
    pub round_limit: u32,
}

impl Default for MatchSettings {
    fn default() -> MatchSettings {
        MatchSettings {
            segment_lengths_ms: Vec::new(),
            round_limit: DEFAULT_ROUND_LIMIT,
        }
    }
}

/// The match's own hashed state, carried inside [`World`].
///
/// Built once by [`MatchState::new`] and moved only by
/// [`crate::world::Phase::Match`] and by the two host transitions on
/// [`Runner`]. Read-only accessors rather than public fields, for the reason
/// [`RulesTable`]'s view has them: a field that could be written from outside
/// could disagree with the hash that was taken over it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MatchState {
    phase: MatchPhase,
    round: u32,
    round_limit: u32,
    segment_lengths_ms: Vec<i32>,
    segment_started: Tick,
    segment_length_ms: i32,
    coming_segment_ms: i32,
    outcome: Option<MatchOutcome>,
}

impl MatchState {
    /// The opening state: the first Lull of round one, with the coming
    /// segment's length already resolved.
    ///
    /// The effective ladder is the host's list when it has one and the rules
    /// table's otherwise (item 40). The rules loader refuses an empty
    /// `match.segment_lengths_ms`, so the effective list is never empty.
    ///
    /// Both the host's list and the round limit are **clamped, not trusted**:
    /// `round_limit` to at least one round, and every ladder entry to at least
    /// one tick ([`clamp_ladder`]). A lobby that hands the sim a zero-length
    /// round would otherwise get a Push that closes on its first tick while the
    /// editor's clock showed zero, which is not a match anybody can play.
    #[must_use]
    pub fn new(settings: &MatchSettings, rules: &RulesTable) -> MatchState {
        let segment_lengths_ms = if settings.segment_lengths_ms.is_empty() {
            clamp_ladder(rules.segment_lengths_ms().to_vec())
        } else {
            clamp_ladder(settings.segment_lengths_ms.clone())
        };
        let coming_segment_ms = length_for_round(&segment_lengths_ms, 1);
        MatchState {
            phase: MatchPhase::Lull,
            round: 1,
            round_limit: settings.round_limit.max(1),
            segment_lengths_ms,
            segment_started: Tick::ZERO,
            segment_length_ms: 0,
            coming_segment_ms,
            outcome: None,
        }
    }

    /// Where the match is.
    #[must_use]
    pub const fn phase(&self) -> MatchPhase {
        self.phase
    }

    /// Which round is being played, from one.
    #[must_use]
    pub const fn round(&self) -> u32 {
        self.round
    }

    /// How many rounds the host set.
    #[must_use]
    pub const fn round_limit(&self) -> u32 {
        self.round_limit
    }

    /// The effective per-round length list (item 40).
    #[must_use]
    pub fn segment_lengths_ms(&self) -> &[i32] {
        &self.segment_lengths_ms
    }

    /// The tick the running segment opened on. Its first played tick is the
    /// one after this.
    #[must_use]
    pub const fn segment_started(&self) -> Tick {
        self.segment_started
    }

    /// The running segment's length. Zero outside a Push.
    #[must_use]
    pub const fn segment_length_ms(&self) -> Ms {
        Ms::new(self.segment_length_ms)
    }

    /// How many ticks the running segment plays.
    ///
    /// Rounded **up**, so a segment is never cut short by the conversion — the
    /// same rounding a timeout takes ([`Ms::to_ticks_ceil`]).
    #[must_use]
    pub fn segment_ticks(&self) -> u32 {
        Ms::new(self.segment_length_ms).to_ticks_ceil()
    }

    /// The coming segment's length — the number the frozen snapshot carries and
    /// the editor's clock reads.
    #[must_use]
    pub const fn coming_segment_ms(&self) -> Ms {
        Ms::new(self.coming_segment_ms)
    }

    /// How the match ended, when it has.
    #[must_use]
    pub const fn outcome(&self) -> Option<MatchOutcome> {
        self.outcome
    }

    /// The length round `round` plays, from the effective list.
    #[must_use]
    pub fn length_for_round(&self, round: u32) -> Ms {
        Ms::new(length_for_round(&self.segment_lengths_ms, round))
    }

    /// Append the match state to the canonical encoding.
    ///
    /// Determinism code: this is the ninth and last block of
    /// [`World::encode`]'s declared order, and every field below decides what a
    /// tick does.
    pub fn encode(&self, enc: &mut Enc) {
        enc.u8(self.phase.id());
        enc.u32(self.round);
        enc.u32(self.round_limit);
        enc.len(u32::try_from(self.segment_lengths_ms.len()).unwrap_or(u32::MAX));
        for length in &self.segment_lengths_ms {
            enc.i32(*length);
        }
        enc.u32(self.segment_started.raw());
        enc.i32(self.segment_length_ms);
        enc.i32(self.coming_segment_ms);
        // Fixed stride, absent arm included, for the reason `Enc::opt_u32` has
        // one: a field must never be confusable with its neighbour. Written
        // out rather than routed through `to_parts`, which clones the ladder —
        // this runs every tick, and a tick allocates nothing (G3' §9.17).
        let (reason, winner, at) = self
            .outcome
            .map_or((0, SeatId::NEUTRAL.raw(), 0), |outcome| {
                (
                    outcome.reason.id(),
                    outcome.winner.map_or(SeatId::NEUTRAL.raw(), SeatId::raw),
                    outcome.at.raw(),
                )
            });
        enc.u8(reason);
        enc.u8(winner);
        enc.u32(at);
    }

    // --- the transitions, all crate-internal -----------------------------

    /// Open a Push at `tick`, running the coming segment's length.
    ///
    /// The length is floored at one tick. This is the single point where a
    /// number becomes a Push's duration, so it is the one place that has to
    /// hold the floor: the ladder is clamped on both the way in
    /// ([`MatchState::new`]) and the way back ([`MatchState::from_parts`]), but
    /// [`MatchState::coming_segment_ms`] also travels in a save file, and a
    /// hand-edited one must not be able to describe a Push that closes before
    /// anything in it runs.
    pub(crate) fn open_push(&mut self, tick: Tick) {
        self.phase = MatchPhase::Push;
        self.segment_started = tick;
        self.segment_length_ms = self.coming_segment_ms.max(MS_PER_TICK);
    }

    /// Close the segment at `tick` and open the recap, writing the coming
    /// segment's length for the round after this one.
    ///
    /// The **single writer** of [`MatchState::coming_segment_ms`]. Writing it
    /// here rather than when the next Push opens is what makes the frozen
    /// snapshot the source of the number instead of a copy of it.
    pub(crate) fn close_segment(&mut self, tick: Tick) {
        self.phase = MatchPhase::Recap;
        self.segment_length_ms = 0;
        self.segment_started = tick;
        self.coming_segment_ms = length_for_round(
            &self.segment_lengths_ms,
            self.round.saturating_add(1).min(self.round_limit),
        );
    }

    /// Close the recap: the next round's Lull, or the end of the match.
    ///
    /// `false` when the match is over — either because the one-tick rule
    /// already decided it during the Push, or because this was the last round
    /// the host set.
    pub(crate) fn close_recap(&mut self, tick: Tick) -> bool {
        if self.outcome.is_some() {
            self.phase = MatchPhase::Ended;
            return false;
        }
        if self.round >= self.round_limit {
            self.decide(MatchEndReason::RoundLimit, None, tick);
            self.phase = MatchPhase::Ended;
            return false;
        }
        self.round = self.round.saturating_add(1);
        self.phase = MatchPhase::Lull;
        true
    }

    /// Decide the match and open the recap.
    ///
    /// Item 16: the Push halts, **standings freeze, the recap plays**, and the
    /// last seat standing wins. So a decided match is in its recap, not
    /// already over: [`MatchPhase::Ended`] is where [`MatchState::close_recap`]
    /// puts it once that recap has been watched or skipped.
    pub(crate) fn decide(&mut self, reason: MatchEndReason, winner: Option<SeatId>, at: Tick) {
        self.phase = MatchPhase::Recap;
        self.segment_length_ms = 0;
        self.outcome = Some(MatchOutcome { reason, winner, at });
    }

    /// Replace the whole state from a restored snapshot.
    pub(crate) fn restore(&mut self, restored: MatchState) {
        *self = restored;
    }

    /// Rebuild from a snapshot's flat columns. `None` when the phase or the end
    /// reason is not one this build defines — a file from another version, or
    /// an edited one, refused rather than restored into a phase that does not
    /// exist.
    ///
    /// **Structure is refused; values are normalised.** A phase id nothing
    /// names is a file this build cannot read, so it is turned away at the
    /// door. A ladder entry shorter than a tick is a *value*, and it takes the
    /// same clamp the host's list takes in [`MatchState::new`] — refusing it
    /// instead would make `Snapshot::default()` unrestorable, which is the one
    /// thing that default is hand-written to guarantee
    /// (`impl Default for Snapshot`, and it carries an empty ladder).
    #[must_use]
    pub(crate) fn from_parts(parts: MatchParts) -> Option<MatchState> {
        let phase = MatchPhase::from_id(parts.phase)?;
        let outcome = if parts.end_reason == 0 {
            None
        } else {
            let reason = MatchEndReason::from_id(parts.end_reason)?;
            let winner = if parts.winner == SeatId::NEUTRAL.raw() {
                None
            } else {
                Some(SeatId::new(parts.winner))
            };
            Some(MatchOutcome {
                reason,
                winner,
                at: Tick::new(parts.ended_at),
            })
        };
        Some(MatchState {
            phase,
            round: parts.round,
            round_limit: parts.round_limit,
            segment_lengths_ms: clamp_ladder(parts.segment_lengths_ms),
            segment_started: Tick::new(parts.segment_started),
            segment_length_ms: parts.segment_length_ms,
            coming_segment_ms: parts.coming_segment_ms,
            outcome,
        })
    }

    /// The flat columns a snapshot carries.
    #[must_use]
    pub(crate) fn to_parts(&self) -> MatchParts {
        let (end_reason, winner, ended_at) = match self.outcome {
            Some(outcome) => (
                outcome.reason.id(),
                outcome.winner.map_or(SeatId::NEUTRAL.raw(), SeatId::raw),
                outcome.at.raw(),
            ),
            None => (0, SeatId::NEUTRAL.raw(), 0),
        };
        MatchParts {
            phase: self.phase.id(),
            round: self.round,
            round_limit: self.round_limit,
            segment_lengths_ms: self.segment_lengths_ms.clone(),
            segment_started: self.segment_started.raw(),
            segment_length_ms: self.segment_length_ms,
            coming_segment_ms: self.coming_segment_ms,
            end_reason,
            winner,
            ended_at,
        }
    }
}

/// The match state as fixed-width columns, the shape [`Snapshot`] carries.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct MatchParts {
    pub(crate) phase: u8,
    pub(crate) round: u32,
    pub(crate) round_limit: u32,
    pub(crate) segment_lengths_ms: Vec<i32>,
    pub(crate) segment_started: u32,
    pub(crate) segment_length_ms: i32,
    pub(crate) coming_segment_ms: i32,
    pub(crate) end_reason: u8,
    pub(crate) winner: u8,
    pub(crate) ended_at: u32,
}

/// Every entry floored at one tick, because a round shorter than a tick is a
/// round that closes before anything in it runs.
///
/// The ladder's companion to `round_limit.max(1)`: a list arrives either from a
/// lobby ([`MatchSettings`]) or from a save file ([`MatchState::from_parts`]),
/// and neither is trusted. Idempotent, so the rules table's own ladder — every
/// entry minutes long — passes through unchanged and no committed hash moves.
fn clamp_ladder(mut lengths: Vec<i32>) -> Vec<i32> {
    for ms in &mut lengths {
        *ms = (*ms).max(MS_PER_TICK);
    }
    lengths
}

/// Item 40's lookup: round *i* takes the *i*-th entry, rounds past the end take
/// the last entry, and an empty list — which the rules loader refuses — gives
/// zero rather than panicking inside a tick. A zero from here still cannot
/// produce a zero-tick Push, because [`MatchState::open_push`] floors the
/// length it consumes.
fn length_for_round(lengths: &[i32], round: u32) -> i32 {
    let Some(last) = lengths.len().checked_sub(1) else {
        return 0;
    };
    let index = usize::try_from(round.saturating_sub(1)).unwrap_or(last);
    lengths.get(index.min(last)).copied().unwrap_or(0)
}

/// The planning snapshot, frozen at segment end (spec section 3).
///
/// What the Lull plans against: `get_status`, `get_briefing` and the editor read
/// it, and it is the file a host saves from ("the host can save during any
/// Lull, from the frozen segment-end snapshot"). It is frozen rather than live
/// precisely so that two seats planning at once see the same world.
///
/// It is taken at a segment end and, on a restore, from the restored world —
/// so after a mid-Push restore it is a mid-Push capture rather than a planning
/// snapshot (see [`Runner::restore`]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FrozenSnapshot {
    snapshot: Snapshot,
    state_hash: u64,
}

impl FrozenSnapshot {
    /// Freeze `world` as it stands.
    #[must_use]
    pub fn of(world: &World) -> FrozenSnapshot {
        FrozenSnapshot {
            snapshot: Snapshot::capture(world),
            state_hash: world.state_hash(),
        }
    }

    /// The snapshot itself — what `plan-core` and the verifier read.
    #[must_use]
    pub const fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// The per-tick state hash of the world it was taken from.
    ///
    /// The identity of a frozen snapshot, and the value a gateway cursor is
    /// tied to: the project already has exactly one hash function, so the
    /// snapshot does not get a second one (AGENTS.md §5).
    #[must_use]
    pub const fn state_hash(&self) -> u64 {
        self.state_hash
    }

    /// The tick it was taken at.
    #[must_use]
    pub const fn taken_at(&self) -> Tick {
        Tick::new(self.snapshot.tick)
    }

    /// The round that has just been played, from one.
    #[must_use]
    pub const fn round(&self) -> u32 {
        self.snapshot.match_round
    }

    /// The coming segment's length — the number item 30 puts in this snapshot
    /// and nowhere else.
    #[must_use]
    pub const fn coming_segment_ms(&self) -> Ms {
        Ms::new(self.snapshot.coming_segment_ms)
    }
}

/// What one Push tick did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TickReport {
    /// The tick that ran.
    pub tick: Tick,
    /// Its state hash — the line that goes in the chain.
    pub hash: u64,
    /// The phase the match is in **after** the tick.
    pub phase: MatchPhase,
    /// Whether this tick closed the segment.
    pub segment_ended: bool,
    /// Whether the match has been decided. The recap still plays: item 16 puts
    /// [`MatchPhase::Ended`] after the recap, not instead of it.
    pub match_ended: bool,
}

/// The match host's driver: phases in, ticks and events out.
///
/// The shape the gateway's match host (T13) and the scenario runner (T15) both
/// drive. It owns its world and its encoder, so a tick costs no allocation and
/// a caller has one object to hold.
#[derive(Debug)]
pub struct Runner {
    world: World,
    enc: Enc,
    frozen: FrozenSnapshot,
}

impl Runner {
    /// Open a match on `world`.
    ///
    /// The world arrives in its opening Lull (see [`MatchState::new`]), so this
    /// takes the first frozen snapshot — the one the first Lull plans against —
    /// and emits `match_started` and `lull_opened`.
    #[must_use]
    pub fn new(mut world: World) -> Runner {
        let tick = world.tick();
        let round = world.match_state().round();
        let coming = world.match_state().coming_segment_ms().raw();
        world.emit(
            tick,
            Emission::of(EventKind::MatchStarted).value(coming.into()),
        );
        world.emit(
            tick,
            Emission::of(EventKind::LullOpened).value(i64::from(round)),
        );
        let frozen = FrozenSnapshot::of(&world);
        Runner {
            world,
            // One encoder for the whole match: a tick allocates nothing
            // (G3′ §9.17), and `tests/allocations.rs` asserts it.
            enc: Enc::with_capacity(64 * 1024),
            frozen,
        }
    }

    /// The world, to read.
    #[must_use]
    pub const fn world(&self) -> &World {
        &self.world
    }

    /// The world, to write.
    ///
    /// What a host files this tick's orders through — a voxel edit, a damage
    /// order — and what T11's interpreter and T14's economy reach the tables
    /// with. Stepping is deliberately **not** here: a tick runs through
    /// [`Runner::step`], so the phase machine cannot be walked around.
    pub const fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// Give the world back.
    #[must_use]
    pub fn into_world(self) -> World {
        self.world
    }

    /// Where the match is.
    #[must_use]
    pub fn phase(&self) -> MatchPhase {
        self.world.match_state().phase()
    }

    /// Which round is being played, from one.
    #[must_use]
    pub fn round(&self) -> u32 {
        self.world.match_state().round()
    }

    /// The tick the world stands at.
    #[must_use]
    pub fn tick(&self) -> Tick {
        self.world.tick()
    }

    /// How the match ended, when it has.
    #[must_use]
    pub fn outcome(&self) -> Option<MatchOutcome> {
        self.world.match_state().outcome()
    }

    /// The planning snapshot the current Lull plans against.
    #[must_use]
    pub const fn frozen(&self) -> &FrozenSnapshot {
        &self.frozen
    }

    /// Everything emitted since the last drain, in `(tick, seq)` order.
    #[must_use]
    pub fn events(&self) -> &[Event] {
        self.world.events()
    }

    /// Drain the event bus. A host that reads the feed calls this every tick.
    pub fn clear_events(&mut self) {
        self.world.clear_events();
    }

    /// Open the Push: every seat is ready, or the Lull's timer ran out.
    ///
    /// `false` when the match is not in a Lull, which is the honest answer to a
    /// host that asked twice. Resets the per-Push state item 21 makes per-Push:
    /// the commander death counters and any pending respawn.
    pub fn begin_push(&mut self) -> bool {
        if self.phase() != MatchPhase::Lull {
            return false;
        }
        let tick = self.world.tick();
        self.world.open_push(tick);
        let length = self.world.match_state().segment_length_ms().raw();
        self.world.emit(
            tick,
            Emission::of(EventKind::PushStarted).value(length.into()),
        );
        true
    }

    /// Advance one tick, and return what it did.
    ///
    /// `None` outside a Push: the Lull and the recap consume no tick and end on
    /// the host's word (see the module docs). A tick that closed the segment
    /// takes the frozen snapshot before returning, so a host that reads
    /// [`Runner::frozen`] on a `segment_ended` report gets the world as the
    /// segment left it.
    pub fn step(&mut self) -> Option<TickReport> {
        if self.phase() != MatchPhase::Push {
            return None;
        }
        let hash = self.world.step(&mut self.enc);
        let phase = self.phase();
        let segment_ended = phase != MatchPhase::Push;
        if segment_ended {
            // Outside `World::step`, deliberately: a capture allocates, and a
            // tick must not. It happens once a segment.
            self.frozen = FrozenSnapshot::of(&self.world);
        }
        Some(TickReport {
            tick: self.world.tick(),
            hash,
            phase,
            segment_ended,
            match_ended: self.outcome().is_some(),
        })
    }

    /// Close the recap: the next round's Lull, or the end of the match.
    ///
    /// `false` when the match is not in a recap.
    ///
    /// PLACEHOLDER: the recap's **settlement** is not done here. Item 19: at
    /// each recap the Ledger credits BMI plus the fixed award fund to the
    /// treasury and shows it as a recap line, and during a Push the only income
    /// is mining and salvage. All of that is the economy's, which is T14's; T10
    /// opens the phase it happens in and says so rather than crediting a number
    /// nobody has decided (owner, at T14).
    pub fn end_recap(&mut self) -> bool {
        if self.phase() != MatchPhase::Recap {
            return false;
        }
        let tick = self.world.tick();
        // A match the one-tick rule already decided announced itself at the
        // tick it was decided on; closing its recap must not say so twice.
        let announced = self.outcome().is_some();
        if self.world.close_recap(tick) {
            let round = self.world.match_state().round();
            self.world.emit(
                tick,
                Emission::of(EventKind::LullOpened).value(i64::from(round)),
            );
            // The snapshot the new Lull plans against is the one the segment
            // froze; re-freezing here would hand the Lull a world one recap
            // older or newer than the one the seats were shown.
        } else if !announced {
            let reason = self
                .outcome()
                .map_or(MatchEndReason::RoundLimit, |outcome| outcome.reason);
            self.world.emit(
                tick,
                Emission::of(EventKind::MatchEnded).value(i64::from(reason.id())),
            );
        }
        true
    }

    /// Capture the world as it stands.
    #[must_use]
    pub fn capture(&self) -> Snapshot {
        Snapshot::capture(&self.world)
    }

    /// Restore a snapshot into the runner's world.
    ///
    /// The frozen snapshot is re-taken from the restored world, because it is
    /// derived from it: a restore lands at a segment boundary (the only place a
    /// save is written — spec section 3, "no saving mid-Push"), where the frozen
    /// snapshot and the world are the same thing.
    ///
    /// **After a mid-Push restore, [`Runner::frozen`] is that mid-Push world**,
    /// not a segment-end freeze. Nothing a host saves lands there, but a test
    /// and a scenario file can (`a_restored_match_keeps_its_phase_round_and_ladder`
    /// does), so it is said here rather than left to be discovered: the
    /// alternative — keeping the receiving runner's own frozen snapshot — would
    /// hand back a planning snapshot of a *different world*, which is worse than
    /// one of this world at the wrong tick. The next segment end replaces it.
    ///
    /// # Errors
    ///
    /// Whatever [`Snapshot::restore_into`] returns. The world is left exactly as
    /// it was on any error.
    pub fn restore(&mut self, snapshot: &Snapshot) -> Result<(), SnapshotError> {
        snapshot.restore_into(&mut self.world)?;
        self.frozen = FrozenSnapshot::of(&self.world);
        Ok(())
    }
}

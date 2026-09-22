// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The event bus: what a tick reports, in a total order, under a name a
//! scenario file can assert on.
//!
//! The gateway's `get_segment_feed` and the watch rig's live event list both
//! read this bus; `crates/gateway/src/feed.rs` fog-filters it, pages it behind
//! opaque cursors and counts it per kind in each 60 s digest. That module's
//! own doc says the catalogue of kinds "belongs to the sim's event bus (T10)",
//! and this module is it.
//!
//! # A kind is a name, and the name is the contract
//!
//! Decisions log item 97: a scenario file asserts `"event": "beacon_placed"`,
//! and T15's extension counts the same names. So a kind is **not a free
//! string** — it is an [`EventKind`] whose [`EventKind::name`] is lower-case
//! ASCII letters, digits and underscores, starting with a letter, exactly what
//! the gateway's `Kind::new` accepts. `every_kind_is_assertable` asserts the
//! spelling of every variant rather than trusting it, and
//! [`EventKind::id`] is written out so that reordering the enum cannot change a
//! wire value by accident (the same rule the unit kinds and the RNG streams
//! follow).
//!
//! Adding a kind is additive: a new variant, a new id at the end, a new name.
//! Renaming one breaks every scenario file that asserts on it, so a name is
//! treated as append-only from the day it ships.
//!
//! # The bus is derived output, not hashed state — and why that is safe
//!
//! Nothing in a tick reads an event. Events are produced by the phases, drained
//! by the host, and never consulted again by the sim, so dropping one cannot
//! change what the world does next. That is the whole test AGENTS.md §4.8 sets:
//! *a field that affects behaviour must be hashed*. This one affects nothing,
//! so it is deliberately **outside** the state hash, outside the snapshot and
//! outside the golden files, and `the_event_bus_is_not_in_the_state_encoding`
//! asserts it rather than leaving it as a claim.
//!
//! That is a decision with a price, and the price is written down: a resumed
//! match starts with an empty bus. It costs nothing in practice because the
//! host drains the bus every tick and a segment-end freeze happens with the bus
//! already emptied, but a stage that wants the feed to survive a save has to
//! make it state rather than assume it already is.
//!
//! What the bus *is* is **deterministic**: the same seed and the same playbooks
//! produce the same events in the same order on every platform, because every
//! emitter walks its table in id order and the order below is a total one.
//! `the_same_seed_produces_the_same_events` asserts that.
//!
//! # The total order
//!
//! An event is ordered by `(tick, seq)`. `seq` counts from zero within a tick
//! and is assigned in emission order; emission order is fixed because the tick
//! phases run in [`crate::world::PHASE_ORDER`] and every phase walks its table
//! in ascending id. **Never arrival order**: nothing here is fed by a channel,
//! a task or a thread, and nothing may be (AGENTS.md §4.6).
//!
//! # A full bus drops and says so
//!
//! [`EVENT_BUS_CAPACITY`] is fixed at construction, because nothing in a tick
//! allocates (G3′ §9.17). A bus with no room **drops the event and counts the
//! drop** ([`EventBus::dropped`]); it never grows and never blocks. A drop is
//! deterministic — it depends only on how many events the tick produced — so it
//! cannot desync a match, but it can silence an assertion, which is why the
//! count is part of the public surface rather than an internal detail.

use crate::knowledge::{AssetId, Position};
use crate::math::quantity::Tick;
use crate::tables::SeatId;

/// How many events one bus holds between drains.
///
/// Sized above the gateway's `MAX_PAGE_EVENTS` (256), so a host draining once
/// per tick and paging at the gateway's ceiling never meets a full bus at the
/// skeleton's event rate. The tick that produces the most events is a seat's
/// elimination, which emits one line per beacon of that seat plus one per
/// structure homed to it.
///
/// PLACEHOLDER: 512 is a working number against a world total of 300 units and
/// 40 beacons (item 63). The owner settles it with the read-method detail
/// budgets at hardening, alongside `MAX_PAGE_EVENTS`.
pub const EVENT_BUS_CAPACITY: usize = 512;

/// What happened. The name is what a scenario file asserts on (item 97).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum EventKind {
    /// The match opened. Emitted once, at tick zero, before the first Lull.
    MatchStarted,
    /// A Lull opened: seats may plan against the frozen snapshot.
    LullOpened,
    /// A Push began. `value` carries the segment's length in game
    /// milliseconds.
    PushStarted,
    /// A Push ended, either by running its length or by the one-tick rule.
    /// `value` carries the ticks the segment actually ran.
    SegmentEnded,
    /// The recap opened: the Ledger settles and the frozen snapshot is taken.
    RecapOpened,
    /// The match ended. `seat` is the winner when there is one.
    MatchEnded,
    /// A beacon reached zero hit points. Its structures become neutral ruins
    /// in the same tick (item 20, local elimination).
    BeaconDestroyed,
    /// A structure became a neutral ruin: inert, no supply, no draw, no
    /// capability, unrepairable, zero audit value (item 20).
    StructureRuined,
    /// A seat lost its last beacon: its units disband and its structures
    /// become ruins (spec section 3, Elimination).
    SeatEliminated,
    /// A commander died. `value` carries how many times it has died in this
    /// Push, counting this one.
    CommanderDied,
    /// A commander came back. `value` carries the ticks the respawn took.
    CommanderRespawned,
    /// A unit was parked as sealed in: no route to its destination exists
    /// (item 60, "park and report").
    UnitSealedIn,
    /// A seat sealed a playbook. `value` carries the route's length in steps.
    PlanSealed,
    /// A route or rule-body step started. `value` carries its index, and `at`
    /// the target its late-bound selector pinned, when it has one.
    ///
    /// **The index is a route index or a body index and the line does not say
    /// which** — a body's step 0 and route step 0 write the same four fields.
    /// A reader tells them apart by the `rule_fired` / `rule_ended` pair a body
    /// runs between, which is enough for a transcript and for item 97's "event
    /// count in range". It is not enough for an assertion that wants to name
    /// one step, so a second field, or a `rule_step_started` kind beside this
    /// one, is the additive answer when T15's scenario assertions ask for it
    /// (owner, at T15). The same holds of `step_completed`, `step_skipped` and
    /// `step_failed`.
    StepStarted,
    /// A step completed. `value` carries its index.
    StepCompleted,
    /// A step's `skip_if` guard was true as it would have started, so it was
    /// skipped. `value` carries its index.
    StepSkipped,
    /// A step failed and `on_fail` decided what happened next. `value` carries
    /// [`crate::interpreter::state::StepFailure::id`] — never a silent skip.
    StepFailed,
    /// A handler's body started. `value` carries the handler's index.
    RuleFired,
    /// A handler's body ended. `value` carries
    /// [`crate::interpreter::Resume::id`].
    RuleEnded,
    /// The fixed 20 % self-preservation reflex fired: it aborted any visit and
    /// walked the commander to the safest own beacon. `value` carries the
    /// commander's hit points as a whole percentage.
    ReflexFired,
    /// The reflex stopped holding the seat's decision, and the route continues
    /// from the step it was on. `value` carries the commander's hit points as a
    /// whole percentage, exactly as `reflex_fired` does.
    ///
    /// One kind covers **all four** of its exits, because what a reader wants
    /// from it is "the playbook is running again" and the interpreter's state
    /// carries which exit it was: the commander reached the safest own beacon;
    /// there was no living own beacon to walk to (the line then lands on the
    /// same tick as its `reflex_fired`); the beacon it was walking to died on
    /// the way; or no route to it exists and the walker was parked (item 60,
    /// with its own `unit_sealed_in` beside this line). Splitting them is
    /// additive and costs a wire id, so it waits for a reader that needs it.
    ReflexCleared,
    /// A visit began: the commander is on site and the handshake is being paid.
    /// `value` carries how many rows the visit will commit.
    VisitStarted,
    /// One interface row committed, at the end of its own duration. `value`
    /// carries the row's index within the visit.
    RowCommitted,
    /// A visit ended. `value` carries how many rows committed — which is how a
    /// transcript shows that an aborted visit kept the rows it had already
    /// committed and pays the handshake again when it resumes.
    VisitEnded,
    /// A beacon was deployed. `value` carries
    /// [`crate::seams::MandateKind::id`].
    BeaconPlaced,
    /// The route ended and the fallback posture took over. `value` carries
    /// [`crate::interpreter::Posture::id`].
    FallbackEngaged,
    /// A beacon went dormant: draw outran supply and the brownout order
    /// reached it. Everything homed to it powers down and parks (item 10).
    /// `value` is unused.
    BeaconBrownedOut,
    /// A dormant beacon came back: supply exceeded draw by
    /// `power.revive_margin_kw` with this beacon's own load added back.
    BeaconRevived,
    /// A beacon's fabricator produced a unit. `value` carries
    /// [`crate::tables::UnitKind::id`].
    UnitFabricated,
    /// The Quartermaster paid for a Build target and the structure went into
    /// the ground at one hit point. `value` carries
    /// [`crate::tables::StructureKind::id`]. "Paid means yours": the `$` has
    /// already left the treasury (item 23).
    StructureQueued,
    /// A structure reached full hit points and started supplying or drawing.
    /// `value` carries [`crate::tables::StructureKind::id`].
    StructureCompleted,
    /// A mining drone delivered its load to its home beacon and the `$` were
    /// credited. `value` carries the amount (spec section 6: "delivered ore is
    /// credited to the seat treasury immediately").
    OreDelivered,
    /// A reclaim drone delivered salvage. `value` carries the amount.
    SalvageDelivered,
    /// A beacon was recycled on site. `value` carries the refund, which is
    /// `economy.recycle_refund_percent` of its remaining value (item 18). The
    /// beacon then dies down the ordinary path, so a `beacon_destroyed` line
    /// follows it on the same tick.
    BeaconRecycled,
    /// The Ledger settled at a recap and credited a seat. `value` carries the
    /// Basic Minimum Income it was paid, after the standing adjustment
    /// (item 19).
    Settled,
    /// One seat's share of a destruction, apportioned by largest remainder
    /// with ties to the lowest seat id (item 17). `value` carries the share, in
    /// `$` of the destroyed thing's full build cost.
    KillCredited,
}

impl EventKind {
    /// Every kind, in ascending [`EventKind::id`] order.
    pub const ALL: [EventKind; 36] = [
        EventKind::MatchStarted,
        EventKind::LullOpened,
        EventKind::PushStarted,
        EventKind::SegmentEnded,
        EventKind::RecapOpened,
        EventKind::MatchEnded,
        EventKind::BeaconDestroyed,
        EventKind::StructureRuined,
        EventKind::SeatEliminated,
        EventKind::CommanderDied,
        EventKind::CommanderRespawned,
        EventKind::UnitSealedIn,
        EventKind::PlanSealed,
        EventKind::StepStarted,
        EventKind::StepCompleted,
        EventKind::StepSkipped,
        EventKind::StepFailed,
        EventKind::RuleFired,
        EventKind::RuleEnded,
        EventKind::ReflexFired,
        EventKind::ReflexCleared,
        EventKind::VisitStarted,
        EventKind::RowCommitted,
        EventKind::VisitEnded,
        EventKind::BeaconPlaced,
        EventKind::FallbackEngaged,
        EventKind::BeaconBrownedOut,
        EventKind::BeaconRevived,
        EventKind::UnitFabricated,
        EventKind::StructureQueued,
        EventKind::StructureCompleted,
        EventKind::OreDelivered,
        EventKind::SalvageDelivered,
        EventKind::BeaconRecycled,
        EventKind::Settled,
        EventKind::KillCredited,
    ];

    /// The wire id. Additive only: never reuse, never renumber.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            EventKind::MatchStarted => 1,
            EventKind::LullOpened => 2,
            EventKind::PushStarted => 3,
            EventKind::SegmentEnded => 4,
            EventKind::RecapOpened => 5,
            EventKind::MatchEnded => 6,
            EventKind::BeaconDestroyed => 7,
            EventKind::StructureRuined => 8,
            EventKind::SeatEliminated => 9,
            EventKind::CommanderDied => 10,
            EventKind::CommanderRespawned => 11,
            EventKind::UnitSealedIn => 12,
            EventKind::PlanSealed => 13,
            EventKind::StepStarted => 14,
            EventKind::StepCompleted => 15,
            EventKind::StepSkipped => 16,
            EventKind::StepFailed => 17,
            EventKind::RuleFired => 18,
            EventKind::RuleEnded => 19,
            EventKind::ReflexFired => 20,
            EventKind::ReflexCleared => 21,
            EventKind::VisitStarted => 22,
            EventKind::RowCommitted => 23,
            EventKind::VisitEnded => 24,
            EventKind::BeaconPlaced => 25,
            EventKind::FallbackEngaged => 26,
            EventKind::BeaconBrownedOut => 27,
            EventKind::BeaconRevived => 28,
            EventKind::UnitFabricated => 29,
            EventKind::StructureQueued => 30,
            EventKind::StructureCompleted => 31,
            EventKind::OreDelivered => 32,
            EventKind::SalvageDelivered => 33,
            EventKind::BeaconRecycled => 34,
            EventKind::Settled => 35,
            EventKind::KillCredited => 36,
        }
    }

    /// The kind an id names, or `None` for an id this build does not define.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<EventKind> {
        match id {
            1 => Some(EventKind::MatchStarted),
            2 => Some(EventKind::LullOpened),
            3 => Some(EventKind::PushStarted),
            4 => Some(EventKind::SegmentEnded),
            5 => Some(EventKind::RecapOpened),
            6 => Some(EventKind::MatchEnded),
            7 => Some(EventKind::BeaconDestroyed),
            8 => Some(EventKind::StructureRuined),
            9 => Some(EventKind::SeatEliminated),
            10 => Some(EventKind::CommanderDied),
            11 => Some(EventKind::CommanderRespawned),
            12 => Some(EventKind::UnitSealedIn),
            13 => Some(EventKind::PlanSealed),
            14 => Some(EventKind::StepStarted),
            15 => Some(EventKind::StepCompleted),
            16 => Some(EventKind::StepSkipped),
            17 => Some(EventKind::StepFailed),
            18 => Some(EventKind::RuleFired),
            19 => Some(EventKind::RuleEnded),
            20 => Some(EventKind::ReflexFired),
            21 => Some(EventKind::ReflexCleared),
            22 => Some(EventKind::VisitStarted),
            23 => Some(EventKind::RowCommitted),
            24 => Some(EventKind::VisitEnded),
            25 => Some(EventKind::BeaconPlaced),
            26 => Some(EventKind::FallbackEngaged),
            27 => Some(EventKind::BeaconBrownedOut),
            28 => Some(EventKind::BeaconRevived),
            29 => Some(EventKind::UnitFabricated),
            30 => Some(EventKind::StructureQueued),
            31 => Some(EventKind::StructureCompleted),
            32 => Some(EventKind::OreDelivered),
            33 => Some(EventKind::SalvageDelivered),
            34 => Some(EventKind::BeaconRecycled),
            35 => Some(EventKind::Settled),
            36 => Some(EventKind::KillCredited),
            _ => None,
        }
    }

    /// The name a scenario file asserts on: `lower_snake_case`, ASCII, starting
    /// with a letter (item 97; the gateway's `Kind::new` enforces the same
    /// shape on the way out).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            EventKind::MatchStarted => "match_started",
            EventKind::LullOpened => "lull_opened",
            EventKind::PushStarted => "push_started",
            EventKind::SegmentEnded => "segment_ended",
            EventKind::RecapOpened => "recap_opened",
            EventKind::MatchEnded => "match_ended",
            EventKind::BeaconDestroyed => "beacon_destroyed",
            EventKind::StructureRuined => "structure_ruined",
            EventKind::SeatEliminated => "seat_eliminated",
            EventKind::CommanderDied => "commander_died",
            EventKind::CommanderRespawned => "commander_respawned",
            EventKind::UnitSealedIn => "unit_sealed_in",
            EventKind::PlanSealed => "plan_sealed",
            EventKind::StepStarted => "step_started",
            EventKind::StepCompleted => "step_completed",
            EventKind::StepSkipped => "step_skipped",
            EventKind::StepFailed => "step_failed",
            EventKind::RuleFired => "rule_fired",
            EventKind::RuleEnded => "rule_ended",
            EventKind::ReflexFired => "reflex_fired",
            EventKind::ReflexCleared => "reflex_cleared",
            EventKind::VisitStarted => "visit_started",
            EventKind::RowCommitted => "row_committed",
            EventKind::VisitEnded => "visit_ended",
            EventKind::BeaconPlaced => "beacon_placed",
            EventKind::FallbackEngaged => "fallback_engaged",
            EventKind::BeaconBrownedOut => "beacon_browned_out",
            EventKind::BeaconRevived => "beacon_revived",
            EventKind::UnitFabricated => "unit_fabricated",
            EventKind::StructureQueued => "structure_queued",
            EventKind::StructureCompleted => "structure_completed",
            EventKind::OreDelivered => "ore_delivered",
            EventKind::SalvageDelivered => "salvage_delivered",
            EventKind::BeaconRecycled => "beacon_recycled",
            EventKind::Settled => "settled",
            EventKind::KillCredited => "kill_credited",
        }
    }

    /// The kind a name spells, or `None` when nothing does.
    ///
    /// The other half of [`EventKind::name`], so a scenario file's string can
    /// be resolved once, at load, rather than compared per event.
    #[must_use]
    pub fn from_name(name: &str) -> Option<EventKind> {
        EventKind::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

/// One thing that happened, at one tick.
///
/// Ordered by `(tick, seq)`, which is a total order because `seq` is unique
/// within a tick (item 62's convention: every ordering key ends in something
/// unique).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    /// The tick it happened on.
    pub tick: Tick,
    /// Where it sits within that tick, from zero, in emission order.
    pub seq: u32,
    /// What happened.
    pub kind: EventKind,
    /// Whose it is, when it belongs to a seat. `None` for a match-wide event
    /// and for anything that has become neutral.
    pub seat: Option<SeatId>,
    /// What it is about, when it is about one thing.
    pub subject: Option<AssetId>,
    /// Where it happened, when it happened somewhere. The gateway's fog filter
    /// decides visibility from this, so an event about a place carries one.
    pub at: Option<Position>,
    /// A kind-specific number; each variant of [`EventKind`] documents its own.
    /// Zero when the kind carries none.
    pub value: i64,
}

/// One event under construction.
///
/// A builder rather than a seven-argument `emit`, because five of those seven
/// arguments are optional and adjacent, and a positional list that long is how
/// a seat id ends up in the subject slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Emission {
    kind: EventKind,
    seat: Option<SeatId>,
    subject: Option<AssetId>,
    at: Option<Position>,
    value: i64,
}

impl Emission {
    /// An event of `kind`, about nothing in particular.
    pub(crate) const fn of(kind: EventKind) -> Emission {
        Emission {
            kind,
            seat: None,
            subject: None,
            at: None,
            value: 0,
        }
    }

    /// Whose it is.
    pub(crate) const fn seat(mut self, seat: SeatId) -> Emission {
        self.seat = Some(seat);
        self
    }

    /// What it is about.
    pub(crate) const fn subject(mut self, subject: AssetId) -> Emission {
        self.subject = Some(subject);
        self
    }

    /// Where it happened.
    pub(crate) const fn at(mut self, at: Position) -> Emission {
        self.at = Some(at);
        self
    }

    /// The kind's own number.
    pub(crate) const fn value(mut self, value: i64) -> Emission {
        self.value = value;
        self
    }
}

/// A fixed-capacity, ordered sink for one tick's events.
///
/// Derived output: see the module docs for why it is not hashed, not
/// snapshotted and not in the goldens.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EventBus {
    events: Vec<Event>,
    capacity: usize,
    /// The tick `seq` is currently counting within.
    tick: Tick,
    /// The next sequence number for [`EventBus::tick`].
    seq: u32,
    emitted: u64,
    dropped: u64,
}

impl EventBus {
    /// A bus holding `capacity` events between drains, allocated once.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> EventBus {
        EventBus {
            events: Vec::with_capacity(capacity),
            capacity,
            tick: Tick::ZERO,
            seq: 0,
            emitted: 0,
            dropped: 0,
        }
    }

    /// Everything emitted since the last drain, in `(tick, seq)` order.
    #[must_use]
    pub fn as_slice(&self) -> &[Event] {
        &self.events
    }

    /// How many events are waiting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether none are waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// How many the bus will hold before it starts dropping.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Take the waiting events out, keeping the allocation.
    ///
    /// The sequence counter is **not** reset: it belongs to the tick, not to
    /// the drain, so a host that drains twice within one tick still gets a
    /// strictly increasing `seq`.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Empty the bus and re-anchor it on `tick`, with `seq` back at zero.
    ///
    /// What a **restore** does, as against a drain. A drain leaves the anchor
    /// alone because the tick has not changed; a restore replaces the world, so
    /// leaving the anchor alone would carry the *receiving* world's history into
    /// the resumed feed — the same snapshot resumed in two runners would emit
    /// the same event under two different `seq` values whenever the receiving
    /// bus happened to be anchored on the restored tick. The feed after a
    /// restore is then a function of the restored state and of nothing else,
    /// which is what `a_restore_re_anchors_the_feed_so_two_resumes_agree`
    /// asserts.
    ///
    /// It does not make the feed survive a save — see the module docs: a
    /// resumed match still starts with an empty feed, and that price is the
    /// decision, not an oversight.
    pub(crate) fn reset(&mut self, tick: Tick) {
        self.events.clear();
        self.tick = tick;
        self.seq = 0;
    }

    /// How many events have ever been emitted, drops included.
    #[must_use]
    pub const fn emitted(&self) -> u64 {
        self.emitted
    }

    /// How many events have ever been dropped for want of room.
    ///
    /// Non-zero means a scenario assertion may be looking at an incomplete
    /// feed. It never means the sim diverged.
    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Record `emission` as having happened at `tick`. `false` when the bus was
    /// full and the event was dropped.
    ///
    /// Crate-internal: the sim's phases and its runner are the only emitters,
    /// so nothing outside can put a line in a seat's feed.
    pub(crate) fn emit(&mut self, tick: Tick, emission: Emission) -> bool {
        if tick != self.tick {
            self.tick = tick;
            self.seq = 0;
        }
        self.emitted = self.emitted.saturating_add(1);
        if self.events.len() >= self.capacity {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        self.events.push(Event {
            tick,
            seq: self.seq,
            kind: emission.kind,
            seat: emission.seat,
            subject: emission.subject,
            at: emission.at,
            value: emission.value,
        });
        self.seq = self.seq.saturating_add(1);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{Emission, EventBus, EventKind};
    use crate::math::quantity::Tick;

    #[test]
    fn a_sequence_counts_within_a_tick_and_restarts_at_the_next() {
        let mut bus = EventBus::with_capacity(8);
        bus.emit(Tick::new(3), Emission::of(EventKind::PushStarted));
        bus.emit(Tick::new(3), Emission::of(EventKind::UnitSealedIn));
        bus.emit(Tick::new(4), Emission::of(EventKind::UnitSealedIn));

        let seen: Vec<(u32, u32)> = bus
            .as_slice()
            .iter()
            .map(|event| (event.tick.raw(), event.seq))
            .collect();
        assert_eq!(seen, [(3, 0), (3, 1), (4, 0)]);
        for pair in seen.windows(2) {
            let (Some(first), Some(second)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            assert!(first < second, "(tick, seq) is a total order");
        }
    }

    #[test]
    fn a_drain_does_not_restart_the_sequence_within_a_tick() {
        // A host may drain twice in one tick; `seq` belongs to the tick, not to
        // the drain, so the second page continues where the first left off.
        let mut bus = EventBus::with_capacity(8);
        bus.emit(Tick::new(1), Emission::of(EventKind::LullOpened));
        bus.clear();
        bus.emit(Tick::new(1), Emission::of(EventKind::PushStarted));
        assert_eq!(bus.as_slice().first().map(|event| event.seq), Some(1));
    }

    #[test]
    fn a_full_bus_drops_and_counts_rather_than_growing() {
        let mut bus = EventBus::with_capacity(2);
        assert!(bus.emit(Tick::new(1), Emission::of(EventKind::LullOpened)));
        assert!(bus.emit(Tick::new(1), Emission::of(EventKind::PushStarted)));
        assert!(
            !bus.emit(Tick::new(1), Emission::of(EventKind::UnitSealedIn)),
            "the third does not fit"
        );

        assert_eq!(bus.len(), 2, "and the bus did not grow to hold it");
        assert_eq!(bus.capacity(), 2);
        assert_eq!(bus.dropped(), 1, "the drop is counted, never silent");
        assert_eq!(bus.emitted(), 3, "drops included");

        // Draining makes room again, and the counters keep the history.
        bus.clear();
        assert!(bus.is_empty());
        assert!(bus.emit(Tick::new(2), Emission::of(EventKind::SegmentEnded)));
        assert_eq!(bus.dropped(), 1);
        assert_eq!(bus.emitted(), 4);
    }

    #[test]
    fn an_emission_carries_what_it_was_given_and_nothing_else() {
        use crate::knowledge::AssetId;
        use crate::tables::{SeatId, UnitId};

        let mut bus = EventBus::with_capacity(4);
        bus.emit(
            Tick::new(7),
            Emission::of(EventKind::CommanderDied)
                .seat(SeatId::new(2))
                .subject(AssetId::of_unit(UnitId::new(5)))
                .value(3),
        );
        // `.expect()` is banned in this crate's source text by
        // `tests/confinement.rs`, unit-test modules included, so a test in
        // `src/` reads its subject the way the sim does.
        let Some(event) = bus.as_slice().first().copied() else {
            panic!("the bus holds the event it was given");
        };
        assert_eq!(event.kind, EventKind::CommanderDied);
        assert_eq!(event.seat, Some(SeatId::new(2)));
        assert_eq!(event.subject, Some(AssetId::of_unit(UnitId::new(5))));
        assert_eq!(event.at, None, "no place was given, so none is claimed");
        assert_eq!(event.value, 3);
    }
}

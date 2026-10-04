// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The interpreter's **hashed state**: where each seat's playbook has got to.
//!
//! Every field here decides what a tick does, so every field is in
//! [`crate::world::World::encode`], in the snapshot round-trip and in the
//! golden files, in the pull request that added it (AGENTS.md §4.8). The
//! compiled [`crate::interpreter::Plan`] is **not** here and is not hashed: a
//! playbook is an input, exactly like the rules table, and the sim is a pure
//! function of `(map seed, playbooks, rules hash)`.
//!
//! # What resets when, and why
//!
//! Spec section 10: *"Segment end: the playbook stops, and nothing carries over
//! except the world itself."* [`Interpreter::open_push`] resets every
//! per-segment field — the cursor, the stage, the visit, every handler's
//! cooldown and fire count, the reflex — and keeps exactly one thing:
//! [`PlanState::deaths_match`], because `gp.v1.CmdrDeaths` asks how many times
//! the commander has died **this match**, not this Push. (The per-Push count is
//! the seat table's `commander_deaths`, which item 21 resets at the same
//! boundary; the two are different questions and are counted in different
//! places rather than one being derived wrongly from the other.)
//!
//! # Sentinels rather than `Option`
//!
//! Every field is fixed-width, for the reason [`crate::snapshot`] gives: the
//! snapshot has no tag byte whose layout could differ between targets. A tick
//! index that means "none" is [`NO_TICK`], a route index that means "none" is
//! [`NO_INDEX`].

use crate::encoding::Enc;
use crate::interpreter::Plan;
use crate::math::quantity::Tick;
use crate::tables::BeaconId;

/// "No tick": a deadline that is not set, a respawn that is not due, damage
/// that has not been taken.
///
/// Not a tick any match reaches — `u32::MAX` ticks is over six years of game
/// time — and the same sentinel convention as [`crate::tables::NO_RESPAWN`].
pub const NO_TICK: u32 = u32::MAX;

/// "No index": a route step, handler or beacon slot that is not set.
pub const NO_INDEX: u32 = u32::MAX;

/// What the step in progress is doing.
///
/// The ids are written out rather than taken from the enum's order, for the
/// reason every other wire id in this crate is: the value reaches the canonical
/// encoding, so a reordering must not be able to change it by accident.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum VisitState {
    /// No step has started: the next decision starts one.
    #[default]
    NotStarted,
    /// The step is running — walking, holding, or waiting on a condition.
    Running,
    /// Paying the visit handshake (spec section 5: 1.5 s, once per visit).
    Handshake,
    /// Committing one interface row, which commits at the end of its own
    /// duration.
    Committing,
    /// Deploying a beacon (`interface_times.place_beacon_deploy_ms`), during
    /// which the commander must stay.
    Deploying,
}

impl VisitState {
    /// The wire id. Additive only: never reuse, never renumber.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            VisitState::NotStarted => 0,
            VisitState::Running => 1,
            VisitState::Handshake => 2,
            VisitState::Committing => 3,
            VisitState::Deploying => 4,
        }
    }

    /// The state an id names, or `None` for one this build does not define.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<VisitState> {
        match id {
            0 => Some(VisitState::NotStarted),
            1 => Some(VisitState::Running),
            2 => Some(VisitState::Handshake),
            3 => Some(VisitState::Committing),
            4 => Some(VisitState::Deploying),
            _ => None,
        }
    }

    /// Whether this state is inside a visit — the thing the reflex aborts and a
    /// resumed visit pays the handshake for again (item 24).
    #[must_use]
    pub const fn is_visiting(self) -> bool {
        matches!(self, VisitState::Handshake | VisitState::Committing)
    }
}

/// One target a step read when it started: an `on` anchor's feature and the
/// column it chose (`docs/design/targeting.md`, "Three reading rules", 1).
///
/// Hashed, because the step keeps it until it ends and commits it into a
/// Build target: a re-read at commit time would be a second reading of the
/// same description.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Binding {
    /// The feature's index in the map's feature table.
    pub feature: u32,
    /// The `on` column, in whole voxels; `z` is the column's standing height
    /// when the step started.
    pub at: [i32; 3],
}

/// One seat's execution state.
#[derive(Clone, PartialEq, Eq, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is one fixed-width hashed field with its own meaning (pinned, the reflex's two, a resumed placement); folding them into a state machine would change the canonical encoding for no reader's gain"
)]
pub struct PlanState {
    /// How many times this seat's commander has died **this match**
    /// (`gp.v1.CmdrDeaths`). The only field that survives a segment.
    pub deaths_match: u32,
    /// The route step in progress, or [`NO_INDEX`] once the route has ended and
    /// the fallback has taken over.
    pub cursor: u32,
    /// The highest route index ever entered, or [`NO_INDEX`].
    ///
    /// `step_reached` is `index <= reached`, which is exactly the predicate's
    /// meaning ("true from the moment the step starts and stays true for the
    /// rest of the segment") because the cursor never decreases — a jump only
    /// goes forward and a skip moves on.
    pub reached: u32,
    /// What the step in progress is doing.
    pub stage: VisitState,
    /// The tick the step in progress started on, or [`NO_TICK`].
    pub started: u32,
    /// The tick the step in progress times out at, or [`NO_TICK`].
    pub deadline: u32,
    /// The handler whose body is running, or [`NO_INDEX`]. At most one body
    /// runs at a time and no handler pre-empts another (item 24).
    pub rule: u32,
    /// How far into that body execution has got.
    pub rule_step: u32,
    /// Whether the step in progress has a pinned target. A late-bound selector
    /// resolves at step start and stays pinned for that step.
    pub pinned: bool,
    /// The pinned beacon, or [`NO_INDEX`] when the pinned target is a voxel.
    pub pinned_beacon: u32,
    /// The pinned place, in whole voxels.
    pub pinned_at: [i32; 3],
    /// The beacon a visit or deploy is at, or [`NO_INDEX`].
    pub visit_beacon: u32,
    /// Which row of the visit is committing.
    pub visit_row: u32,
    /// The tick the handshake, the row or the deploy completes at, or
    /// [`NO_TICK`].
    pub visit_due: u32,
    /// Whether the reflex may fire. Set by new damage, cleared by firing:
    /// *"it re-fires only after new damage"* (item 24).
    pub reflex_armed: bool,
    /// Whether the reflex is walking the commander to the safest own beacon.
    pub reflex_active: bool,
    /// The tick the commander last lost hit points, or [`NO_TICK`]. What
    /// `cmdr_took_damage_within` reads and what arms the reflex.
    pub last_damage: u32,
    /// The commander's hit points as of the last decision — the marker the
    /// damage edge is spotted against.
    pub last_hp: i32,
    /// Which patrol waypoint the fallback is walking to.
    pub fallback_leg: u32,
    /// Per handler, how many times it has fired this segment.
    pub fires: Vec<u32>,
    /// Per handler, the first tick it may fire again (its cooldown).
    pub ready: Vec<u32>,
    /// The feature a `covering` step chose, or [`NO_INDEX`]: what `covered {}`
    /// names, and a bound target the step checks at each decision.
    pub bound_feature: u32,
    /// Every `on` anchor the step in progress read when it started, in the
    /// order its rows write them. Reserved at the seal to the plan's
    /// [`crate::interpreter::Plan::max_bindings`], so binding allocates
    /// nothing inside a tick.
    pub bindings: Vec<Binding>,
    /// Whether the step in progress is a placement **resumed as a visit** to
    /// the beacon it placed (targeting.md, "Companion changes": restart keeps
    /// the placed beacon).
    pub resumed: bool,
    /// The beacon a placement placed and has not finished writing, or
    /// [`NO_INDEX`]: what a restart resumes at. Kept when death, the reflex or
    /// a rule clears the step; dropped when the step ends.
    pub restart_beacon: u32,
    /// The rule body that placement belongs to, or [`NO_INDEX`] for the route.
    pub restart_rule: u32,
    /// The placement's step index in the route or that body.
    pub restart_at: u32,
    /// The first of its initial rows not yet committed.
    pub restart_row: u32,
}

/// A seat that has sealed nothing: no route, no rule, no visit, no deadline.
///
/// Written by hand rather than derived, for the reason
/// [`crate::snapshot::Snapshot`]'s `Default` is: a derived one would give
/// `cursor = 0` and `rule = 0`, and neither of those means *none* — they mean
/// "on route step zero, with handler zero's body running". This state is
/// **hashed**, so a default that reads as a state the sim can be in is a trap
/// rather than a convenience.
impl Default for PlanState {
    fn default() -> PlanState {
        PlanState {
            deaths_match: 0,
            cursor: NO_INDEX,
            reached: NO_INDEX,
            stage: VisitState::NotStarted,
            started: NO_TICK,
            deadline: NO_TICK,
            rule: NO_INDEX,
            rule_step: 0,
            pinned: false,
            pinned_beacon: NO_INDEX,
            pinned_at: [0; 3],
            visit_beacon: NO_INDEX,
            visit_row: 0,
            visit_due: NO_TICK,
            reflex_armed: false,
            reflex_active: false,
            last_damage: NO_TICK,
            last_hp: 0,
            fallback_leg: 0,
            fires: Vec::new(),
            ready: Vec::new(),
            bound_feature: NO_INDEX,
            bindings: Vec::new(),
            resumed: false,
            restart_beacon: NO_INDEX,
            restart_rule: NO_INDEX,
            restart_at: NO_INDEX,
            restart_row: 0,
        }
    }
}

impl PlanState {
    /// The state a freshly sealed playbook starts in.
    ///
    /// `deaths_match` is carried over rather than zeroed: sealing a new
    /// playbook for the next segment does not un-kill a commander.
    #[must_use]
    pub fn sealed(plan: &Plan, deaths_match: u32, commander_hp: i32) -> PlanState {
        PlanState {
            deaths_match,
            cursor: 0,
            reached: NO_INDEX,
            stage: VisitState::NotStarted,
            started: NO_TICK,
            deadline: NO_TICK,
            rule: NO_INDEX,
            rule_step: 0,
            pinned: false,
            pinned_beacon: NO_INDEX,
            pinned_at: [0; 3],
            visit_beacon: NO_INDEX,
            visit_row: 0,
            visit_due: NO_TICK,
            // Armed from the first decision: a commander that starts a segment
            // already under 20 % — because the previous segment left it there —
            // is exactly the case the reflex exists for.
            reflex_armed: true,
            reflex_active: false,
            last_damage: NO_TICK,
            last_hp: commander_hp,
            fallback_leg: 0,
            fires: vec![0; plan.handlers().len()],
            ready: vec![0; plan.handlers().len()],
            bound_feature: NO_INDEX,
            bindings: Vec::with_capacity(plan.max_bindings()),
            resumed: false,
            restart_beacon: NO_INDEX,
            restart_rule: NO_INDEX,
            restart_at: NO_INDEX,
            restart_row: 0,
        }
    }

    /// Whether the route has ended and the fallback has taken over.
    #[must_use]
    pub const fn in_fallback(&self) -> bool {
        self.cursor == NO_INDEX
    }

    /// The pinned beacon, when the step pinned one.
    #[must_use]
    pub const fn pinned_beacon(&self) -> Option<BeaconId> {
        if self.pinned_beacon == NO_INDEX {
            None
        } else {
            Some(BeaconId::new(self.pinned_beacon))
        }
    }

    /// Forget the step in progress, so the next decision starts one afresh.
    ///
    /// What the reflex does to a visit, and what a resumed step does to its
    /// pinned selector: *"a resumed visit is a new visit and pays the handshake
    /// again"* (item 24).
    ///
    /// A placement that has already placed its beacon **keeps** its bindings
    /// and its restart point: it resumes as a visit to that beacon, with the
    /// rows it had left, and reads nothing again (targeting.md, "Companion
    /// changes"). [`PlanState::clear_restart`] is what forgets those, when the
    /// step ends.
    pub fn clear_step(&mut self) {
        self.stage = VisitState::NotStarted;
        self.started = NO_TICK;
        self.deadline = NO_TICK;
        self.pinned = false;
        self.pinned_beacon = NO_INDEX;
        self.pinned_at = [0; 3];
        self.visit_beacon = NO_INDEX;
        self.visit_row = 0;
        self.visit_due = NO_TICK;
        self.resumed = false;
        if self.restart_beacon == NO_INDEX {
            self.bound_feature = NO_INDEX;
            self.bindings.clear();
        }
    }

    /// Forget the step's bindings and its restart point: the step has ended,
    /// so nothing of it is resumed.
    pub fn clear_restart(&mut self) {
        self.restart_beacon = NO_INDEX;
        self.restart_rule = NO_INDEX;
        self.restart_at = NO_INDEX;
        self.restart_row = 0;
        self.bound_feature = NO_INDEX;
        self.bindings.clear();
    }

    /// Whether a placement at step `index` (of rule `rule`, or the route when
    /// [`NO_INDEX`]) is the one this state's restart point resumes.
    #[must_use]
    pub const fn resumes(&self, rule: u32, index: u32) -> bool {
        self.restart_beacon != NO_INDEX && self.restart_rule == rule && self.restart_at == index
    }

    /// Append this seat's block to the canonical encoding.
    fn encode(&self, enc: &mut Enc) {
        enc.u32(self.deaths_match);
        enc.u32(self.cursor);
        enc.u32(self.reached);
        enc.u8(self.stage.id());
        enc.u32(self.started);
        enc.u32(self.deadline);
        enc.u32(self.rule);
        enc.u32(self.rule_step);
        enc.bool(self.pinned);
        enc.u32(self.pinned_beacon);
        for axis in self.pinned_at {
            enc.i32(axis);
        }
        enc.u32(self.visit_beacon);
        enc.u32(self.visit_row);
        enc.u32(self.visit_due);
        enc.bool(self.reflex_armed);
        enc.bool(self.reflex_active);
        enc.u32(self.last_damage);
        enc.i32(self.last_hp);
        enc.u32(self.fallback_leg);
        enc.len(u32::try_from(self.fires.len()).unwrap_or(u32::MAX));
        for (index, fires) in self.fires.iter().enumerate() {
            enc.u32(*fires);
            enc.u32(self.ready.get(index).copied().unwrap_or(0));
        }
        // S1's targeting (snapshot version 7): the step's bindings and the
        // restarted placement's beacon and rows.
        enc.u32(self.bound_feature);
        enc.len(u32::try_from(self.bindings.len()).unwrap_or(u32::MAX));
        for binding in &self.bindings {
            enc.u32(binding.feature);
            for axis in binding.at {
                enc.i32(axis);
            }
        }
        enc.bool(self.resumed);
        enc.u32(self.restart_beacon);
        enc.u32(self.restart_rule);
        enc.u32(self.restart_at);
        enc.u32(self.restart_row);
    }
}

/// One [`PlanState`] per seat, and the sealed plans beside them.
///
/// The plans are an input and the states are hashed state, so they are two
/// columns of one object rather than one column of pairs: [`Interpreter::encode`]
/// walks the states and never touches the plans.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Interpreter {
    plans: Vec<Option<Plan>>,
    states: Vec<PlanState>,
}

impl Interpreter {
    /// Room for `seats` seats, none of which has sealed anything.
    #[must_use]
    pub fn with_seats(seats: u32) -> Interpreter {
        let count = usize::try_from(seats).unwrap_or(0);
        Interpreter {
            plans: vec![None; count],
            states: vec![PlanState::default(); count],
        }
    }

    /// How many seats it covers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// Whether it covers no seats at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// One seat's sealed plan, when it has one.
    #[must_use]
    pub fn plan(&self, seat: usize) -> Option<&Plan> {
        self.plans.get(seat)?.as_ref()
    }

    /// One seat's execution state.
    #[must_use]
    pub fn state(&self, seat: usize) -> Option<&PlanState> {
        self.states.get(seat)
    }

    /// One seat's execution state, to write.
    pub fn state_mut(&mut self, seat: usize) -> Option<&mut PlanState> {
        self.states.get_mut(seat)
    }

    /// One seat's sealed plan and its state, borrowed together.
    ///
    /// A decision needs the plan to read and the state to write, and the two
    /// live in different columns of this object — so they are handed out in one
    /// call rather than left to a caller to borrow twice.
    pub(crate) fn parts_mut(&mut self, seat: usize) -> Option<(&Plan, &mut PlanState)> {
        let plan = self.plans.get(seat)?.as_ref()?;
        let state = self.states.get_mut(seat)?;
        Some((plan, state))
    }

    /// Seal a playbook for one seat, resetting that seat's execution state.
    ///
    /// `commander_hp` is the commander's hit points now, which becomes the
    /// damage marker the reflex compares against — so sealing does not itself
    /// read as damage.
    pub fn seal(&mut self, seat: usize, plan: Plan, commander_hp: i32) {
        let deaths = self.states.get(seat).map_or(0, |state| state.deaths_match);
        let fresh = PlanState::sealed(&plan, deaths, commander_hp);
        if let Some(slot) = self.plans.get_mut(seat) {
            *slot = Some(plan);
        }
        if let Some(slot) = self.states.get_mut(seat) {
            *slot = fresh;
        }
    }

    /// Book one commander death against a seat's match-long counter.
    ///
    /// Called by the match phase, which is the one place a death is booked, so
    /// this counter and the seat table's per-Push one cannot drift apart.
    pub fn book_death(&mut self, seat: usize) {
        if let Some(state) = self.states.get_mut(seat) {
            state.deaths_match = state.deaths_match.saturating_add(1);
        }
    }

    /// Reset every per-segment field as a Push opens.
    ///
    /// The playbook stops at segment end and nothing carries over except the
    /// world itself (spec section 10) — and [`PlanState::deaths_match`], which
    /// is a match-long question.
    pub fn open_push(&mut self, commander_hp: &[i32]) {
        for (seat, state) in self.states.iter_mut().enumerate() {
            let hp = commander_hp.get(seat).copied().unwrap_or(0);
            match self.plans.get(seat).and_then(Option::as_ref) {
                Some(plan) => *state = PlanState::sealed(plan, state.deaths_match, hp),
                None => {
                    *state = PlanState {
                        deaths_match: state.deaths_match,
                        last_hp: hp,
                        ..PlanState::default()
                    };
                }
            }
        }
    }

    /// Append the interpreter to the canonical encoding, in seat order.
    ///
    /// Determinism code: the tenth block of [`crate::world::World::encode`]'s
    /// declared order.
    pub fn encode(&self, enc: &mut Enc) {
        enc.len(u32::try_from(self.states.len()).unwrap_or(u32::MAX));
        for state in &self.states {
            state.encode(enc);
        }
    }

    /// The flat columns a snapshot carries.
    #[must_use]
    pub fn to_parts(&self) -> PlanParts {
        let mut parts = PlanParts::default();
        for state in &self.states {
            parts.deaths_match.push(state.deaths_match);
            parts.cursor.push(state.cursor);
            parts.reached.push(state.reached);
            parts.stage.push(state.stage.id());
            parts.started.push(state.started);
            parts.deadline.push(state.deadline);
            parts.rule.push(state.rule);
            parts.rule_step.push(state.rule_step);
            parts.pinned.push(u8::from(state.pinned));
            parts.pinned_beacon.push(state.pinned_beacon);
            for axis in state.pinned_at {
                parts.pinned_at.push(axis);
            }
            parts.visit_beacon.push(state.visit_beacon);
            parts.visit_row.push(state.visit_row);
            parts.visit_due.push(state.visit_due);
            parts.reflex_armed.push(u8::from(state.reflex_armed));
            parts.reflex_active.push(u8::from(state.reflex_active));
            parts.last_damage.push(state.last_damage);
            parts.last_hp.push(state.last_hp);
            parts.fallback_leg.push(state.fallback_leg);
            parts
                .rule_count
                .push(u32::try_from(state.fires.len()).unwrap_or(u32::MAX));
            for (index, fires) in state.fires.iter().enumerate() {
                parts.fires.push(*fires);
                parts
                    .ready
                    .push(state.ready.get(index).copied().unwrap_or(0));
            }
            parts.bound_feature.push(state.bound_feature);
            parts
                .binding_count
                .push(u32::try_from(state.bindings.len()).unwrap_or(u32::MAX));
            for binding in &state.bindings {
                parts.binding_feature.push(binding.feature);
                for axis in binding.at {
                    parts.binding_at.push(axis);
                }
            }
            parts.resumed.push(u8::from(state.resumed));
            parts.restart_beacon.push(state.restart_beacon);
            parts.restart_rule.push(state.restart_rule);
            parts.restart_at.push(state.restart_at);
            parts.restart_row.push(state.restart_row);
        }
        parts
    }

    /// Rebuild the states from a snapshot's flat columns, **keeping the sealed
    /// plans**.
    ///
    /// A playbook is an input and is not in the file, exactly as the rules
    /// table is not (see [`crate::snapshot`]). So a restore writes the state
    /// back into whatever plans this world holds, and a host that resumes a
    /// save without re-sealing the playbooks resumes a match whose seats have
    /// none — which is a host mistake rather than a sim state.
    ///
    /// The plan fingerprint (item 77) this needs is carried by the **save**,
    /// beside the snapshot and never inside it (T17; the wave-6 notes,
    /// decision C10): the gateway's `save.json` holds each seat's sealed text
    /// and its fingerprint, and a resume recompiles the text, compares the
    /// fingerprint and re-seals before the first tick, so resuming into a
    /// playbook other than the saved one is refused at the door. The snapshot
    /// format did not move for it.
    ///
    /// Returns `false` and changes nothing when the columns disagree in length,
    /// or when they cover a different number of seats than `seats`.
    pub fn restore(&mut self, parts: &PlanParts, seats: usize) -> bool {
        if !parts.is_consistent(seats) {
            return false;
        }
        let mut states: Vec<PlanState> = Vec::with_capacity(seats);
        let mut at: usize = 0;
        let mut rule_at: usize = 0;
        let mut binding_at: usize = 0;
        while at < seats {
            let Some(stage) = parts.stage.get(at).copied().and_then(VisitState::from_id) else {
                return false;
            };
            let count =
                usize::try_from(parts.rule_count.get(at).copied().unwrap_or(0)).unwrap_or(0);
            let to = rule_at.saturating_add(count);
            let (Some(fires), Some(ready)) =
                (parts.fires.get(rule_at..to), parts.ready.get(rule_at..to))
            else {
                return false;
            };
            let bound = usize::try_from(parts.binding_count.get(at).copied().unwrap_or(0))
                .unwrap_or(usize::MAX);
            let bound_to = binding_at.saturating_add(bound);
            let Some(features) = parts.binding_feature.get(binding_at..bound_to) else {
                return false;
            };
            let mut bindings: Vec<Binding> = Vec::with_capacity(bound);
            for (slot, feature) in features.iter().enumerate() {
                let axis = |k: usize| -> i32 {
                    binding_at
                        .saturating_add(slot)
                        .checked_mul(3)
                        .and_then(|base| base.checked_add(k))
                        .and_then(|index| parts.binding_at.get(index).copied())
                        .unwrap_or(0)
                };
                bindings.push(Binding {
                    feature: *feature,
                    at: [axis(0), axis(1), axis(2)],
                });
            }
            let base = at.saturating_mul(3);
            states.push(PlanState {
                deaths_match: parts.deaths_match.get(at).copied().unwrap_or(0),
                cursor: parts.cursor.get(at).copied().unwrap_or(NO_INDEX),
                reached: parts.reached.get(at).copied().unwrap_or(NO_INDEX),
                stage,
                started: parts.started.get(at).copied().unwrap_or(NO_TICK),
                deadline: parts.deadline.get(at).copied().unwrap_or(NO_TICK),
                rule: parts.rule.get(at).copied().unwrap_or(NO_INDEX),
                rule_step: parts.rule_step.get(at).copied().unwrap_or(0),
                pinned: parts.pinned.get(at).copied().unwrap_or(0) != 0,
                pinned_beacon: parts.pinned_beacon.get(at).copied().unwrap_or(NO_INDEX),
                pinned_at: [
                    parts.pinned_at.get(base).copied().unwrap_or(0),
                    parts
                        .pinned_at
                        .get(base.saturating_add(1))
                        .copied()
                        .unwrap_or(0),
                    parts
                        .pinned_at
                        .get(base.saturating_add(2))
                        .copied()
                        .unwrap_or(0),
                ],
                visit_beacon: parts.visit_beacon.get(at).copied().unwrap_or(NO_INDEX),
                visit_row: parts.visit_row.get(at).copied().unwrap_or(0),
                visit_due: parts.visit_due.get(at).copied().unwrap_or(NO_TICK),
                reflex_armed: parts.reflex_armed.get(at).copied().unwrap_or(0) != 0,
                reflex_active: parts.reflex_active.get(at).copied().unwrap_or(0) != 0,
                last_damage: parts.last_damage.get(at).copied().unwrap_or(NO_TICK),
                last_hp: parts.last_hp.get(at).copied().unwrap_or(0),
                fallback_leg: parts.fallback_leg.get(at).copied().unwrap_or(0),
                fires: fires.to_vec(),
                ready: ready.to_vec(),
                bound_feature: parts.bound_feature.get(at).copied().unwrap_or(NO_INDEX),
                bindings,
                resumed: parts.resumed.get(at).copied().unwrap_or(0) != 0,
                restart_beacon: parts.restart_beacon.get(at).copied().unwrap_or(NO_INDEX),
                restart_rule: parts.restart_rule.get(at).copied().unwrap_or(NO_INDEX),
                restart_at: parts.restart_at.get(at).copied().unwrap_or(NO_INDEX),
                restart_row: parts.restart_row.get(at).copied().unwrap_or(0),
            });
            rule_at = to;
            binding_at = bound_to;
            at = at.saturating_add(1);
        }
        self.plans.resize(seats, None);
        // A restored seat's counters are resized to the plan it is resuming
        // into. A file whose `fires` column is shorter than the sealed plan's
        // handler list would otherwise give those handlers no fire count and no
        // cooldown at all — `try_fire` reads a missing counter as zero and
        // writes it nowhere — so the handler would fire every 250 ms for the
        // rest of the segment. Short columns degrade to "no fires recorded",
        // which is a resume that loses information rather than one that
        // silently removes a limit. (A host resuming a save never meets this:
        // the save carries the plan fingerprint and a resume re-seals the saved
        // playbook, which resets these counters -- see the doc above.)
        for (seat, state) in states.iter_mut().enumerate() {
            let Some(plan) = self.plans.get(seat).and_then(Option::as_ref) else {
                continue;
            };
            state.fires.resize(plan.handlers().len(), 0);
            state.ready.resize(plan.handlers().len(), 0);
            // The room a step's bindings need, reserved here for the reason
            // the seal reserves it: binding inside a tick allocates nothing.
            state
                .bindings
                .reserve(plan.max_bindings().saturating_sub(state.bindings.len()));
        }
        self.states = states;
        true
    }
}

/// The interpreter's state as fixed-width columns, the shape
/// [`crate::snapshot::Snapshot`] carries.
///
/// One column per field, seat by seat, with the per-handler counters **packed**
/// behind a per-seat count — the same shape the router's route nodes take, and
/// for the same reason: a stride wide enough for the largest playbook would be
/// almost all padding.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PlanParts {
    /// Commander deaths this match, per seat.
    pub deaths_match: Vec<u32>,
    /// The route cursor, per seat.
    pub cursor: Vec<u32>,
    /// The highest route index entered, per seat.
    pub reached: Vec<u32>,
    /// [`VisitState::id`], per seat.
    pub stage: Vec<u8>,
    /// The tick the step in progress started, per seat.
    pub started: Vec<u32>,
    /// The tick it times out at, per seat.
    pub deadline: Vec<u32>,
    /// The running handler, per seat.
    pub rule: Vec<u32>,
    /// How far into its body, per seat.
    pub rule_step: Vec<u32>,
    /// Whether the step has a pinned target, per seat.
    pub pinned: Vec<u8>,
    /// The pinned beacon, per seat.
    pub pinned_beacon: Vec<u32>,
    /// The pinned place, three whole voxels per seat.
    pub pinned_at: Vec<i32>,
    /// The beacon a visit is at, per seat.
    pub visit_beacon: Vec<u32>,
    /// The row committing, per seat.
    pub visit_row: Vec<u32>,
    /// The tick it commits at, per seat.
    pub visit_due: Vec<u32>,
    /// Whether the reflex may fire, per seat.
    pub reflex_armed: Vec<u8>,
    /// Whether the reflex is walking, per seat.
    pub reflex_active: Vec<u8>,
    /// The tick the commander last took damage, per seat.
    pub last_damage: Vec<u32>,
    /// The commander's hit points at the last decision, per seat.
    pub last_hp: Vec<i32>,
    /// The patrol waypoint the fallback is on, per seat.
    pub fallback_leg: Vec<u32>,
    /// How many handlers each seat's plan has.
    pub rule_count: Vec<u32>,
    /// Fire counts, packed in seat order.
    pub fires: Vec<u32>,
    /// Cooldown ticks, packed in seat order.
    pub ready: Vec<u32>,
    /// The feature a `covering` step chose, per seat.
    pub bound_feature: Vec<u32>,
    /// How many bindings each seat's step in progress holds.
    pub binding_count: Vec<u32>,
    /// Each binding's feature, packed in seat order.
    pub binding_feature: Vec<u32>,
    /// Each binding's column, three whole voxels per binding, packed.
    pub binding_at: Vec<i32>,
    /// Whether the step is a placement resumed as a visit, per seat.
    pub resumed: Vec<u8>,
    /// The beacon a restarted placement resumes at, per seat.
    pub restart_beacon: Vec<u32>,
    /// The rule body that placement is in, per seat.
    pub restart_rule: Vec<u32>,
    /// That placement's step index, per seat.
    pub restart_at: Vec<u32>,
    /// The first initial row it had not committed, per seat.
    pub restart_row: Vec<u32>,
}

impl PlanParts {
    /// Whether every column covers `seats` seats, every stage byte names a
    /// stage this build defines, and the packed per-handler columns are exactly
    /// as long as the counts say.
    ///
    /// One check in one place, called by both readers — the snapshot's door and
    /// [`Interpreter::restore`] — because two copies of a raggedness check are
    /// two copies that can drift.
    ///
    /// `seats` is the **seat table's** width, not the plan block's own: a block
    /// that is merely self-consistent can still cover a different number of
    /// seats than the file it travels in, and an empty one is self-consistent.
    /// A world restored from such a file would run with the interpreter
    /// silently switched off — `encode` writing `len 0` where an unrestored run
    /// of the same match writes `len 3` — which is a hash the chain has no way
    /// to notice.
    #[must_use]
    pub fn is_consistent(&self, seats: usize) -> bool {
        if self.deaths_match.len() != seats {
            return false;
        }
        if self.cursor.len() != seats
            || self.reached.len() != seats
            || self.stage.len() != seats
            || self.started.len() != seats
            || self.deadline.len() != seats
            || self.rule.len() != seats
            || self.rule_step.len() != seats
            || self.pinned.len() != seats
            || self.pinned_beacon.len() != seats
            || self.pinned_at.len() != seats.saturating_mul(3)
            || self.visit_beacon.len() != seats
            || self.visit_row.len() != seats
            || self.visit_due.len() != seats
            || self.reflex_armed.len() != seats
            || self.reflex_active.len() != seats
            || self.last_damage.len() != seats
            || self.last_hp.len() != seats
            || self.fallback_leg.len() != seats
            || self.rule_count.len() != seats
            || self.bound_feature.len() != seats
            || self.binding_count.len() != seats
            || self.resumed.len() != seats
            || self.restart_beacon.len() != seats
            || self.restart_rule.len() != seats
            || self.restart_at.len() != seats
            || self.restart_row.len() != seats
        {
            return false;
        }
        let mut bound: usize = 0;
        for count in &self.binding_count {
            bound = bound.saturating_add(usize::try_from(*count).unwrap_or(usize::MAX));
        }
        if bound != self.binding_feature.len() || bound.saturating_mul(3) != self.binding_at.len() {
            return false;
        }
        // Every stage byte must name a stage this build defines, for the reason
        // the match phase's byte must: a file that restores into a stage
        // nothing names is a world whose next decision does nothing and whose
        // hash says so (`SnapshotError::MatchState`'s argument, one field
        // along).
        if !self
            .stage
            .iter()
            .all(|id| VisitState::from_id(*id).is_some())
        {
            return false;
        }
        let mut packed: usize = 0;
        for count in &self.rule_count {
            packed = packed.saturating_add(usize::try_from(*count).unwrap_or(usize::MAX));
        }
        packed == self.fires.len() && packed == self.ready.len()
    }
}

/// Why a step failed. The value the `step_failed` event carries.
///
/// The ids are written out and additive only, for the reason every other wire
/// id in this crate is: a scenario file and a transcript golden both read them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum StepFailure {
    /// The step ran past its `timeout_ms`.
    Timeout,
    /// A late-bound selector resolved to nothing. Never a silent skip: it is a
    /// step failure and `on_fail` decides.
    NoTarget,
    /// No route exists to the target — the walker is sealed in (item 60).
    NoPath,
    /// The commander left the visit's range mid-visit (spec section 5).
    OutOfRange,
    /// The beacon being interfaced with was destroyed.
    BeaconGone,
    /// The site is not a legal one: not inside one of the seat's own spheres,
    /// on the column of one of the seat's own live beacons (no stacking), or,
    /// for a description, no candidate it matched has a legal site
    /// (`docs/design/targeting.md`, "Failure").
    IllegalSite,
    /// There is no room in the beacon table for another beacon.
    NoBeaconRoom,
    /// A `broadcast` with no Radio Mast. The verifier rejects one at plan time;
    /// if one reaches the interpreter it fails loudly rather than being a
    /// silent no-op (S4).
    NoMast,
    /// The commander died while the step was running.
    CommanderDead,
    /// The treasury cannot cover the order the step would commit (spec
    /// section 7, "an order the treasury cannot cover fails like any other
    /// step"). In S1 the deploy is the only step that spends, so this is a
    /// `place_beacon` whose `structures.beacon.cost_dollars` the seat's `$`
    /// does not reach when the deploy would start (decision 7 of S1's plan,
    /// ruled by item 128).
    Unaffordable,
    /// A feature the step bound when it started -- a vent or seam it is
    /// covering or building on -- is **lost**: no exposed vent material, or no
    /// ore, is left in its footprint (`docs/design/targeting.md`, "Three
    /// reading rules" and "Failure"). A step checks its bound features at each
    /// decision, and no row ever commits a lost target.
    FeatureLost,
}

impl StepFailure {
    /// Every reason, in ascending [`StepFailure::id`] order.
    ///
    /// `tests/interpreter.rs`'s `every_step_failure_has_a_unique_id` walks it,
    /// which is what makes the ordering claim checked rather than asserted —
    /// the array was out of order when it was only a doc comment.
    pub const ALL: [StepFailure; 11] = [
        StepFailure::Timeout,
        StepFailure::NoTarget,
        StepFailure::NoPath,
        StepFailure::OutOfRange,
        StepFailure::BeaconGone,
        StepFailure::IllegalSite,
        StepFailure::NoBeaconRoom,
        StepFailure::NoMast,
        StepFailure::CommanderDead,
        StepFailure::FeatureLost,
        StepFailure::Unaffordable,
    ];

    /// The id `not_own` carried until S1: **retired, never reused**. It told a
    /// seat whether an unseen enemy beacon was alive, and `no_target` answers
    /// instead (`docs/design/targeting.md`, "Failure").
    pub const RETIRED_NOT_OWN: u8 = 10;

    /// The wire id. Additive only: never reuse, never renumber. Id 10 is
    /// retired ([`StepFailure::RETIRED_NOT_OWN`]).
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            StepFailure::Timeout => 1,
            StepFailure::NoTarget => 2,
            StepFailure::NoPath => 3,
            StepFailure::OutOfRange => 4,
            StepFailure::BeaconGone => 5,
            StepFailure::IllegalSite => 6,
            StepFailure::NoBeaconRoom => 7,
            StepFailure::NoMast => 8,
            StepFailure::CommanderDead => 9,
            StepFailure::FeatureLost => 11,
            StepFailure::Unaffordable => 12,
        }
    }

    /// The name a transcript and a scenario file read.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            StepFailure::Timeout => "timeout",
            StepFailure::NoTarget => "no_target",
            StepFailure::NoPath => "no_path",
            StepFailure::OutOfRange => "out_of_range",
            StepFailure::BeaconGone => "beacon_gone",
            StepFailure::IllegalSite => "illegal_site",
            StepFailure::NoBeaconRoom => "no_beacon_room",
            StepFailure::NoMast => "no_mast",
            StepFailure::CommanderDead => "commander_dead",
            StepFailure::FeatureLost => "feature_lost",
            StepFailure::Unaffordable => "unaffordable",
        }
    }
}

/// The tick a deadline falls on, or [`NO_TICK`] when there is none.
#[must_use]
pub fn deadline_of(start: Tick, ms: i32) -> u32 {
    if ms <= 0 {
        return NO_TICK;
    }
    start
        .raw()
        .saturating_add(crate::interpreter::ticks_of(ms))
        .min(NO_TICK.saturating_sub(1))
}

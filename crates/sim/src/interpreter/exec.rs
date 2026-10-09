// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One decision, for one seat: the writing half of the interpreter.
//!
//! [`decide`] is what [`crate::world::Phase::Decision`] runs, once per seat,
//! once per `match.decision_tick_ms` of game time. The order below is the order
//! the rules bind, and each step is where it is because of the one before it:
//!
//! 1. **The damage marker.** The commander's hit points are compared against
//!    the marker from the last decision. A drop sets `last_damage` — which is
//!    what `cmdr_took_damage_within` reads — and **re-arms the reflex**, which
//!    is what "it re-fires only after new damage" means in code (item 24).
//! 2. **A dead commander drives nothing.** Death interrupts any visit in
//!    progress (spec section 4); the step is cleared, so the route resumes at
//!    the step the commander was on when it comes back
//!    (`on_death.on_respawn = CONTINUE`, the only value v1 defines).
//! 3. **The reflex**, the only interrupt. At commander HP ≤
//!    [`REFLEX_HP_PERCENT`] it aborts any visit, walks the commander to the
//!    safest own beacon, and nothing else runs until it arrives; then the route
//!    continues from the step it was on — which *restarts* that step, so a
//!    resumed visit is a new visit and pays the handshake again.
//! 4. **Handlers**, but only while no body is running *and* the current step is
//!    not inside a visit or a deploy. The second half is spec section 5's own
//!    sentence: a visit stops only when the commander dies, the reflex fires,
//!    the beacon is destroyed, or the commander leaves range — a rule firing is
//!    not on that list. Those are the only two conditions: a seat that has
//!    reached its guaranteed tail still evaluates its handlers, because spec
//!    section 10 lists rules and the tail as two separate things a playbook
//!    has and says nothing about one ending the other.
//! 5. **The step**: the running rule body's, or the route's, or — once the
//!    route has ended — the fallback posture's.

use crate::events::{Emission, EventKind};
use crate::interpreter::cond::{self, View, resolve_beacon};
use crate::interpreter::state::{
    Binding, NO_INDEX, NO_TICK, PlanState, StepFailed, StepFailure, VisitState, deadline_of,
};
use crate::interpreter::{
    Action, Anchor, FailAction, FeatureSpec, InterfaceTimes, Place, Plan, PlanStep, Posture,
    REFLEX_HP_PERCENT, Removal, Resume, Row, ticks_of,
};
use crate::knowledge::{AssetId, Position};
use crate::math::fixed::Fx;
use crate::math::quantity::Tick;
use crate::programs::within;
use crate::tables::{AreaKind, BeaconId, SeatId, TargetKind};
use crate::targeting::{DESCRIPTION_VOXEL, NO_FEATURE};
use crate::world::{BuildEntry, World};

/// What one pass over the step in progress concluded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
    /// Still going; the next decision looks again.
    Running,
    /// Done: move on.
    Complete,
    /// Skipped by its own `skip_if` guard.
    Skipped,
    /// Failed; `on_fail` decides what happens next.
    Failed(StepFailure),
}

/// Run one decision for one seat.
pub(crate) fn decide(world: &mut World, seat_index: usize, plan: &Plan, state: &mut PlanState) {
    let Some(raw) = world.seats().seats().get(seat_index).copied() else {
        return;
    };
    if !world.seats().is_alive(seat_index) {
        return;
    }
    let seat = SeatId::new(raw);
    let tick = world.tick();

    let hp = view(world, seat, seat_index, state).commander_hp();
    if hp < state.last_hp {
        state.last_damage = tick.raw();
        state.reflex_armed = true;
    }
    state.last_hp = hp;

    if hp <= 0 {
        // Spec section 4: death interrupts any visit in progress. The step is
        // forgotten rather than failed, because `on_respawn: CONTINUE` resumes
        // the route at the step the commander was on — and a step that starts
        // again re-pins its selector and pays the handshake again.
        if state.stage != VisitState::NotStarted {
            let subject = state.pinned_beacon();
            emit(
                world,
                tick,
                EventKind::StepFailed,
                seat,
                subject,
                None,
                StepFailed::bare(StepFailure::CommanderDead).value(),
            );
            drop_step(world, seat, state);
        }
        state.reflex_active = false;
        return;
    }

    if !state.in_fallback() && out_of_deaths(world, seat_index, plan) {
        enter_fallback(world, seat, state, plan);
    }

    if run_reflex(world, seat, seat_index, state) {
        return;
    }

    // Handlers are evaluated in the guaranteed tail too. Spec section 10 lists
    // "rules with explicit resume semantics" and "a guaranteed tail" as two
    // things a playbook has, and nowhere says the first stops when the second
    // starts — so stopping them would be an invented rule (AGENTS.md §12), and
    // an author's flee rule would quietly stop working the moment the route
    // ran out. A rule that fires in the tail runs its body and hands the seat
    // back to the tail afterwards, because every `resume` a body can name
    // leaves a cursor of [`NO_INDEX`] where it is.
    //
    // Nor while a placement has a beacon in the ground and rows left to write:
    // **handlers do not fire during a deploy or a commit**, and a placement
    // cleared by death or the reflex after its deploy is still that deploy's
    // commit, resumed (`docs/design/targeting.md`, "Companion changes").
    if state.rule == NO_INDEX
        && state.restart_beacon == NO_INDEX
        && !matches!(
            state.stage,
            VisitState::Handshake | VisitState::Committing | VisitState::Deploying
        )
    {
        try_fire(world, seat, seat_index, state, plan);
    }

    if state.rule != NO_INDEX {
        advance_body(world, seat, seat_index, state, plan);
    } else if state.in_fallback() {
        run_fallback(world, seat, seat_index, state, plan);
    } else {
        advance_route(world, seat, seat_index, state, plan);
    }
}

/// Whether the commander has died more often this segment than the playbook's
/// `on_death` block allows.
///
/// PLACEHOLDER: `max_deaths_before_fallback` is a `uint32` and proto3 has no
/// presence on one, so zero and unset are the same bytes. Zero reads as **no
/// limit** here, because the alternative — the fallback taking over before the
/// commander has died at all — makes the required `on_death` block impossible
/// to write without a number. Owner, at S3, with the rest of the guard
/// vocabulary.
fn out_of_deaths(world: &World, seat_index: usize, plan: &Plan) -> bool {
    let limit = plan.max_deaths_before_fallback();
    if limit == 0 {
        return false;
    }
    world
        .seats()
        .commander_deaths()
        .get(seat_index)
        .is_some_and(|deaths| *deaths >= limit)
}

/// The fixed 20 % reflex. Returns `true` when it is holding the decision.
fn run_reflex(world: &mut World, seat: SeatId, seat_index: usize, state: &mut PlanState) -> bool {
    let tick = world.tick();
    let pct = view(world, seat, seat_index, state).commander_hp_pct();
    if state.reflex_armed && pct <= i64::from(REFLEX_HP_PERCENT) {
        state.reflex_armed = false;
        if state.stage.is_visiting() {
            let subject = state.pinned_beacon();
            emit(
                world,
                tick,
                EventKind::VisitEnded,
                seat,
                subject,
                None,
                i64::from(state.visit_row),
            );
        }
        drop_step(world, seat, state);
        emit(world, tick, EventKind::ReflexFired, seat, None, None, pct);
        let target = {
            let seen = view(world, seat, seat_index, state);
            resolve_beacon(&seen, crate::interpreter::BeaconSpec::Safest)
                .and_then(|beacon| beacon_at(world, beacon).map(|at| (beacon, at)))
        };
        match target {
            Some((beacon, at)) => {
                state.reflex_active = true;
                state.pinned = true;
                state.pinned_beacon = beacon.raw();
                state.pinned_at = voxels_of(at);
                walk_commander(world, seat, at);
            }
            None => {
                // Nowhere safe to walk to: the seat has no living beacon, which
                // is the tick its elimination is being settled on. The reflex
                // has fired and is over in the same decision rather than
                // pretending to walk somewhere that does not exist.
                emit(world, tick, EventKind::ReflexCleared, seat, None, None, pct);
            }
        }
    }
    if !state.reflex_active {
        return false;
    }
    let arrived = {
        let seen = view(world, seat, seat_index, state);
        let radius = world.arrive_radius();
        match (seen.commander_at(), pinned_point(state.pinned_at)) {
            (Some(at), Some(target)) => within(at, target, radius),
            _ => false,
        }
    };
    // The reflex has the same two exits a `move` step has, and for the same
    // reason: while it holds the decision **nothing else in the playbook runs**,
    // so a reflex with only an arrival exit is a seat frozen for the rest of the
    // segment the first time the commander cannot get home. A commander walled
    // in by its own craters (item 60's "park and report") or walking to a beacon
    // that has since died is exactly that case, and it is not hypothetical:
    // `tests/pathing.rs` builds the walled-in walker on purpose.
    let gone = state.pinned_beacon().is_none_or(|beacon| {
        let seen = view(world, seat, seat_index, state);
        !seen
            .beacon_row(beacon)
            .is_some_and(|row| seen.beacon_alive(row))
    });
    // A pinned target no point can stand on is a place the reflex can never
    // reach, so it ends as a gone beacon does.
    let unreachable = pinned_point(state.pinned_at).is_none();
    if arrived || gone || unreachable || sealed_in(world, seat) {
        state.reflex_active = false;
        drop_step(world, seat, state);
        emit(world, tick, EventKind::ReflexCleared, seat, None, None, pct);
        return false;
    }
    true
}

/// Evaluate the handlers in priority order and start the first enabled one
/// whose condition is true.
fn try_fire(
    world: &mut World,
    seat: SeatId,
    seat_index: usize,
    state: &mut PlanState,
    plan: &Plan,
) {
    let tick = world.tick();
    let chosen = {
        let seen = view(world, seat, seat_index, state);
        let mut found: Option<u32> = None;
        for (index, rule) in plan.handlers().iter().enumerate() {
            let fires = state.fires.get(index).copied().unwrap_or(0);
            let ready = state.ready.get(index).copied().unwrap_or(0);
            if fires >= rule.max_fires || tick.raw() < ready {
                continue;
            }
            // P1's counted half: a handler tried is one evaluation unit, and
            // its condition's nodes are counted as they are evaluated.
            seen.world.charge_decision(1);
            if cond::evaluate(&seen, &rule.when) {
                found = u32::try_from(index).ok();
                break;
            }
        }
        found
    };
    let Some(index) = chosen else {
        return;
    };
    let Some(at) = slot_of(index) else {
        return;
    };
    let Some(rule) = plan.handlers().get(at) else {
        return;
    };
    if let Some(slot) = state.fires.get_mut(at) {
        *slot = slot.saturating_add(1);
    }
    if let Some(slot) = state.ready.get_mut(at) {
        *slot = tick.raw().saturating_add(ticks_of(rule.cooldown_ms));
    }
    // The route step in flight is forgotten, not suspended: a body that walks
    // the commander somewhere else makes the old step's pinned target and its
    // elapsed time meaningless, and `CONTINUE` then means "carry on with that
    // step", which restarts it.
    drop_step(world, seat, state);
    state.rule = index;
    state.rule_step = 0;
    emit(
        world,
        tick,
        EventKind::RuleFired,
        seat,
        None,
        None,
        i64::from(index),
    );
}

/// Advance the running rule body by one decision.
fn advance_body(
    world: &mut World,
    seat: SeatId,
    seat_index: usize,
    state: &mut PlanState,
    plan: &Plan,
) {
    let Some(rule) = slot_of(state.rule).and_then(|at| plan.handlers().get(at)) else {
        state.rule = NO_INDEX;
        return;
    };
    let index = state.rule_step;
    let Some(step) = slot_of(index).and_then(|at| rule.body.get(at)) else {
        let resume = rule.resume;
        end_body(world, seat, state, plan, resume);
        return;
    };
    let times = plan.interface_times();
    let outcome = run_step(world, seat, seat_index, state, step, index, times);
    let body_len = rule.body.len();
    let resume = rule.resume;
    if outcome != Outcome::Running {
        // The step has ended, so nothing of it is resumed.
        state.clear_restart();
    }
    match outcome {
        Outcome::Running => {}
        Outcome::Complete | Outcome::Skipped => {
            drop_step(world, seat, state);
            state.rule_step = state.rule_step.saturating_add(1);
            if !within_list(state.rule_step, body_len) {
                end_body(world, seat, state, plan, resume);
            }
        }
        Outcome::Failed(_) => {
            drop_step(world, seat, state);
            match step.on_fail {
                FailAction::Skip => {
                    state.rule_step = state.rule_step.saturating_add(1);
                    if !within_list(state.rule_step, body_len) {
                        end_body(world, seat, state, plan, resume);
                    }
                }
                // Inside a body, "abort route" ends the **body**: the route is
                // what `resume` decides about, and a step guard cannot reach
                // past its own body.
                FailAction::AbortRoute => end_body(world, seat, state, plan, resume),
                FailAction::JumpForward(target) => {
                    end_body(world, seat, state, plan, Resume::JumpForward(target));
                }
            }
        }
    }
}

/// End the running body and apply the handler's `resume`.
fn end_body(world: &mut World, seat: SeatId, state: &mut PlanState, plan: &Plan, resume: Resume) {
    let tick = world.tick();
    state.rule = NO_INDEX;
    state.rule_step = 0;
    state.clear_restart();
    drop_step(world, seat, state);
    let mut ended = false;
    match resume {
        Resume::Continue => {}
        Resume::SkipStep => {
            state.cursor = state.cursor.saturating_add(1);
        }
        Resume::JumpForward(target) => {
            // A jump only ever goes forward. The route's cursor is not known
            // until the rule fires, so the compile-time check a route step gets
            // is made here instead: a target at or behind the cursor leaves the
            // cursor alone rather than moving it backwards.
            if target > state.cursor {
                state.cursor = target;
            }
        }
        Resume::EndRoute => ended = true,
    }
    emit(
        world,
        tick,
        EventKind::RuleEnded,
        seat,
        None,
        None,
        i64::from(resume.id()),
    );
    if ended || !within_list(state.cursor, plan.route().len()) {
        enter_fallback(world, seat, state, plan);
    }
}

/// Advance the route by one decision.
fn advance_route(
    world: &mut World,
    seat: SeatId,
    seat_index: usize,
    state: &mut PlanState,
    plan: &Plan,
) {
    let index = state.cursor;
    let Some(step) = slot_of(index).and_then(|at| plan.route().get(at)) else {
        enter_fallback(world, seat, state, plan);
        return;
    };
    let outcome = run_step(
        world,
        seat,
        seat_index,
        state,
        step,
        index,
        plan.interface_times(),
    );
    // `None` means the route has ended and the fallback takes over. Every arm
    // below either leaves the cursor alone or moves it **forward**, which is
    // what bounds the number of steps a route can enter.
    let next: Option<u32> = match outcome {
        Outcome::Running => return,
        Outcome::Complete | Outcome::Skipped => Some(index.saturating_add(1)),
        Outcome::Failed(_) => match step.on_fail {
            FailAction::Skip => Some(index.saturating_add(1)),
            FailAction::AbortRoute => None,
            FailAction::JumpForward(target) => Some(target.max(index.saturating_add(1))),
        },
    };
    // The step has ended, so nothing of it is resumed.
    state.clear_restart();
    drop_step(world, seat, state);
    match next {
        Some(cursor) if within_list(cursor, plan.route().len()) => {
            state.cursor = cursor;
        }
        _ => enter_fallback(world, seat, state, plan),
    }
}

/// Run one step: start it if it has not started, then look at it.
fn run_step(
    world: &mut World,
    seat: SeatId,
    seat_index: usize,
    state: &mut PlanState,
    step: &PlanStep,
    index: u32,
    times: &InterfaceTimes,
) -> Outcome {
    let tick = world.tick();
    if state.stage == VisitState::NotStarted {
        if let Some(guard) = step.skip_if.as_ref() {
            let seen = view(world, seat, seat_index, state);
            if cond::evaluate(&seen, guard) {
                emit(
                    world,
                    tick,
                    EventKind::StepSkipped,
                    seat,
                    None,
                    None,
                    i64::from(index),
                );
                return Outcome::Skipped;
            }
        }
        if state.rule == NO_INDEX && (state.reached == NO_INDEX || index > state.reached) {
            state.reached = index;
        }
        match start_step(world, seat, seat_index, state, step, index, times) {
            Ok(()) => {}
            Err(failure) => {
                // A step that fails *while starting* — an unresolvable
                // selector, an illegal site, a commander out of interface
                // range — still announces itself first, so that every
                // `step_failed` in a transcript has a `step_started` above it
                // naming which step failed. The line carries no place, because
                // the step never got one.
                emit(
                    world,
                    tick,
                    EventKind::StepStarted,
                    seat,
                    None,
                    None,
                    i64::from(index),
                );
                return fail(world, seat, state, failure);
            }
        }
    }
    if state.deadline != NO_TICK && tick.raw() >= state.deadline {
        return fail(world, seat, state, StepFailure::Timeout.into());
    }
    match look(world, seat, seat_index, state, step, index, times) {
        Outcome::Failed(failure) => fail(world, seat, state, failure.into()),
        other => other,
    }
}

/// Report a failure and hand the decision back to `on_fail`.
///
/// The step's own index is not on the event: a `step_failed` follows the
/// `step_started` that named it, and the value slot carries the reason, which
/// is the thing a transcript and a scenario assertion want, and for a step that
/// read a feature how many candidates matched ([`StepFailed`]'s layout).
fn fail(world: &mut World, seat: SeatId, state: &PlanState, failure: StepFailed) -> Outcome {
    let tick = world.tick();
    emit(
        world,
        tick,
        EventKind::StepFailed,
        seat,
        state.pinned_beacon(),
        None,
        failure.value(),
    );
    Outcome::Failed(failure.reason)
}

/// Start a step: resolve and pin its target, read every target its rows write,
/// set its clocks, report it.
///
/// **A step reads all its targets when it starts**, including the ones it
/// writes into a new beacon's initial settings or an interface row, and keeps
/// them until it ends (`docs/design/targeting.md`, "Three reading rules", 1):
/// a `covering` site, and every `on` anchor of its rows, are resolved here and
/// bound on the state, never read again by a later decision.
///
/// **`place_beacon` and `interface` walk to their target first** (targeting.md,
/// "Companion changes"): a step whose commander is not yet within interface
/// range starts by walking there, and the step's `timeout_ms` bounds that walk
/// up to arrival; a target the commander cannot reach fails `no_path`. The
/// placement range no longer plays a part (amending item 11).
#[allow(
    clippy::too_many_lines,
    reason = "one arm per action, and the placement arm is two halves -- a fresh placement and a resumed one -- that share the walk in; splitting it would hand the state between functions as a parameter list as long as the arm"
)]
fn start_step(
    world: &mut World,
    seat: SeatId,
    seat_index: usize,
    state: &mut PlanState,
    step: &PlanStep,
    index: u32,
    times: &InterfaceTimes,
) -> Result<(), StepFailed> {
    let tick = world.tick();
    let resuming =
        matches!(step.action, Action::PlaceBeacon { .. }) && state.resumes(state.rule, index);
    if !resuming {
        state.clear_restart();
    }
    state.started = tick.raw();
    state.deadline = deadline_of(tick, step.timeout_ms);
    state.stage = VisitState::Running;
    state.visit_row = 0;
    state.visit_due = NO_TICK;

    let mut where_at: Option<[Fx; 3]> = None;
    // A visit's own line is emitted *after* the step's, so a transcript reads
    // in the order the events happened: the step started, and then the visit
    // inside it began.
    let mut visit: Option<(BeaconId, [Fx; 3], usize)> = None;
    match &step.action {
        Action::Move { to, .. } => {
            let at = pin_place(world, seat, seat_index, state, *to)?;
            walk_commander(world, seat, at);
            where_at = Some(at);
        }
        Action::Interface { beacon, rows, .. } => {
            let resolved = {
                let seen = view(world, seat, seat_index, state);
                // *Touch to change* is a rule about your **own** beacons:
                // beacon capture is out of v1 (AGENTS.md §11). Since S1 a
                // `b_NN` counts only the seat's own beacons and an `e_NN`
                // resolves to nothing, so "someone else's" is `no_target`, the
                // one answer hidden and absent get too; `not_own` (id 10),
                // which told a seat whether an unseen enemy beacon was alive,
                // is retired (`docs/design/targeting.md`, "Failure").
                resolve_beacon(&seen, *beacon).ok_or(StepFailure::NoTarget)?
            };
            let at = beacon_at(world, resolved).ok_or(StepFailure::NoTarget)?;
            let commander = commander_at(world, seat).ok_or(StepFailure::CommanderDead)?;
            bind_rows(world, seat, state, rows, at, Some(resolved))?;
            state.pinned = true;
            state.pinned_beacon = resolved.raw();
            state.pinned_at = voxels_of(at);
            state.visit_beacon = resolved.raw();
            if within(commander, at, world.interface_range()) {
                begin_visit(world, state, times);
                visit = Some((resolved, at, rows.len()));
            } else {
                walk_in(world, seat, commander, at)?;
            }
            where_at = Some(at);
        }
        Action::PlaceBeacon { at, rows, .. } => {
            let commander = commander_at(world, seat).ok_or(StepFailure::CommanderDead)?;
            if resuming {
                // **Restart keeps the placed beacon** (targeting.md,
                // "Companion changes"): the step resumes as a visit to the
                // beacon it placed, with the rows it had left, and reads
                // nothing again -- so it never pays for a second beacon.
                let placed = BeaconId::new(state.restart_beacon);
                let alive = world_beacon_row(world, placed).is_some_and(|row| {
                    world
                        .beacons()
                        .hit_points()
                        .get(row)
                        .is_some_and(|hp| hp.is_alive())
                });
                if !alive {
                    return Err(StepFailure::BeaconGone.into());
                }
                let site = beacon_at(world, placed).ok_or(StepFailure::BeaconGone)?;
                state.resumed = true;
                state.pinned = true;
                state.pinned_beacon = placed.raw();
                state.pinned_at = voxels_of(site);
                state.visit_beacon = placed.raw();
                state.visit_row = state.restart_row;
                if within(commander, site, world.interface_range()) {
                    begin_visit(world, state, times);
                    visit = Some((placed, site, rows_left(rows, state.restart_row)));
                } else {
                    walk_in(world, seat, commander, site)?;
                }
                where_at = Some(site);
            } else {
                let site = match at {
                    Place::Covering(spec) => {
                        let found = {
                            let (ground, scratch) = world.ground_and_scratch();
                            crate::targeting::cover_counted(
                                &ground, scratch, seat, commander, *spec,
                            )?
                        };
                        let [x, y] = found.site;
                        let point = world
                            .ground()
                            .standing(x, y)
                            .ok_or(StepFailure::IllegalSite)?;
                        // A feature index past `u32` names no feature the
                        // map's table can hold.
                        state.bound_feature =
                            u32::try_from(found.feature).map_err(|_| StepFailure::NoTarget)?;
                        state.pinned = true;
                        state.pinned_beacon = NO_INDEX;
                        state.pinned_at = voxels_of(point);
                        point
                    }
                    Place::Voxel(_) | Place::Beacon(_) => {
                        pin_place(world, seat, seat_index, state, *at)?
                    }
                };
                if !site_is_legal(world, seat, site) {
                    return Err(StepFailure::IllegalSite.into());
                }
                bind_rows(world, seat, state, rows, site, None)?;
                if within(commander, site, world.interface_range()) {
                    begin_deploy(world, seat, state, site, times)?;
                } else {
                    walk_in(world, seat, commander, site)?;
                }
                where_at = Some(site);
            }
        }
        Action::WaitUntil(_) | Action::Hold { .. } | Action::Broadcast => {}
    }
    let mut emission = Emission::of(EventKind::StepStarted)
        .seat(seat)
        .value(i64::from(index));
    if let Some(point) = where_at {
        emission = emission.at(Position::from_array(point));
    }
    world.emit(tick, emission);
    if let Some((beacon, at, rows)) = visit {
        emit(
            world,
            tick,
            EventKind::VisitStarted,
            seat,
            Some(beacon),
            Some(at),
            i64::try_from(rows).unwrap_or(i64::MAX),
        );
    }
    Ok(())
}

/// Read every `on` anchor `rows` write, in order, and bind each on the state
/// (`docs/design/targeting.md`, "Three reading rules", 1).
///
/// `centre` is the sphere the targets are written into -- the beacon a visit
/// is at, or, for a placement's initial settings, the site -- and `except` is
/// that beacon, whose own targets do not count against the `on` candidate
/// rule. `covered {}` names the feature this step's `covering` chose.
///
/// The bindings vector was reserved at the seal to the plan's largest step, so
/// nothing here allocates.
fn bind_rows(
    world: &mut World,
    seat: SeatId,
    state: &mut PlanState,
    rows: &[Row],
    centre: [Fx; 3],
    except: Option<BeaconId>,
) -> Result<(), StepFailed> {
    state.bindings.clear();
    let covered = (state.bound_feature != NO_INDEX)
        .then(|| usize::try_from(state.bound_feature).ok())
        .flatten();
    for row in rows {
        // A switch binds what the settings it carries bind.
        let row = match row {
            Row::Mandate {
                settings: Some(settings),
                ..
            } => settings.as_ref(),
            other => other,
        };
        let targets: &[crate::interpreter::Target] = match row {
            Row::AddTarget { target } => core::slice::from_ref(target),
            Row::Settings { targets, .. } => targets,
            Row::Mandate { .. }
            | Row::Priority { .. }
            | Row::Recycle
            | Row::RemoveTarget { .. } => {
                continue;
            }
        };
        for target in targets {
            if let Anchor::On(spec) = target.anchor {
                let binding = bind_one(world, seat, centre, except, covered, spec)?;
                state.bindings.push(binding);
            }
        }
    }
    Ok(())
}

/// Resolve one `on` anchor ([`crate::targeting::on_vent`]) into a binding.
fn bind_one(
    world: &mut World,
    seat: SeatId,
    centre: [Fx; 3],
    except: Option<BeaconId>,
    covered: Option<usize>,
    spec: FeatureSpec,
) -> Result<Binding, StepFailed> {
    let site = {
        let (ground, scratch) = world.ground_and_scratch();
        crate::targeting::on_vent_counted(&ground, scratch, seat, centre, except, covered, spec)?
    };
    let [x, y] = site.column;
    // The resolver chose a column of the surface, so it has a node; a column
    // with none is no ground to build on.
    let z = world
        .surface()
        .node_of(x, y)
        .map(|node| world.surface().standing_z(node))
        .ok_or(StepFailure::IllegalSite)?;
    Ok(Binding {
        // A feature index past `u32` names no feature the map's table holds.
        feature: u32::try_from(site.feature).map_err(|_| StepFailure::NoTarget)?,
        at: [x, y, z],
    })
}

/// Whether every feature the step in progress bound is still there. A step
/// checks its bound features at each decision, and a lost one fails it
/// `feature_lost` (targeting.md, "Three reading rules", 1).
fn bindings_live(world: &World, state: &PlanState) -> bool {
    let live = |feature: u32| -> bool { feature == NO_INDEX || feature_live(world, feature) };
    live(state.bound_feature) && state.bindings.iter().all(|binding| live(binding.feature))
}

/// Begin the visit handshake (spec section 5: 1.5 s, once per visit). The walk
/// in, if there was one, is over, so the step's timeout no longer runs: it
/// bounds the walk up to arrival (targeting.md, "Companion changes").
fn begin_visit(world: &World, state: &mut PlanState, times: &InterfaceTimes) {
    let tick = world.tick();
    state.deadline = NO_TICK;
    state.stage = VisitState::Handshake;
    state.visit_due = tick
        .raw()
        .saturating_add(ticks_of(times.visit_handshake_ms()));
}

/// Begin the deploy at `site`: the site must still be legal, and spec section
/// 7's "`$` leaves the treasury the moment an order commits: deploy starts"
/// charges the beacon's cost **here**, after the walk in and never at the
/// step's start (S1's targeting moved it; X-12's charge is otherwise
/// unchanged). A treasury that cannot cover it fails the step `unaffordable`
/// (decision 7 of S1's plan, id 12). From here until the beacon is in the
/// ground the stage is `Deploying`, and that stage is the only record of the
/// charge: every way out of it that is not the placement refunds it in full
/// ([`drop_step`], and [`abandon_deploys`] at segment end).
fn begin_deploy(
    world: &mut World,
    seat: SeatId,
    state: &mut PlanState,
    site: [Fx; 3],
    times: &InterfaceTimes,
) -> Result<(), StepFailure> {
    if !site_is_legal(world, seat, site) {
        return Err(StepFailure::IllegalSite);
    }
    if !world.charge_deploy(seat) {
        return Err(StepFailure::Unaffordable);
    }
    let tick = world.tick();
    state.deadline = NO_TICK;
    state.stage = VisitState::Deploying;
    state.visit_due = tick
        .raw()
        .saturating_add(ticks_of(times.place_beacon_deploy_ms()));
    Ok(())
}

/// Walk the commander to `to`, or fail `no_path` at once when no route can
/// exist: the connectivity oracle's answer, two array reads (item 60), so a
/// sealed-in commander does not spend its whole timeout learning it.
fn walk_in(
    world: &mut World,
    seat: SeatId,
    commander: [Fx; 3],
    to: [Fx; 3],
) -> Result<(), StepFailure> {
    let [fx, fy] = crate::targeting::column_of(commander);
    let [tx, ty] = crate::targeting::column_of(to);
    let surface = world.surface();
    let (Some(from), Some(goal)) = (surface.node_of(fx, fy), surface.node_of(tx, ty)) else {
        return Err(StepFailure::NoPath);
    };
    if !world.clusters().connected(surface, from, goal) {
        return Err(StepFailure::NoPath);
    }
    walk_commander(world, seat, to);
    Ok(())
}

/// Look at a started step and say whether it is done.
fn look(
    world: &mut World,
    seat: SeatId,
    seat_index: usize,
    state: &mut PlanState,
    step: &PlanStep,
    index: u32,
    times: &InterfaceTimes,
) -> Outcome {
    let tick = world.tick();
    if !bindings_live(world, state) {
        return Outcome::Failed(StepFailure::FeatureLost);
    }
    match &step.action {
        Action::Move { .. } => {
            let Some(at) = commander_at(world, seat) else {
                return Outcome::Failed(StepFailure::CommanderDead);
            };
            // The pinned place came off a point the step resolved, so it
            // stands on a point; one that does not is nowhere to walk to.
            let Some(target) = pinned_point(state.pinned_at) else {
                return Outcome::Failed(StepFailure::NoTarget);
            };
            if within(at, target, world.arrive_radius()) {
                complete(world, seat, index);
                return Outcome::Complete;
            }
            if sealed_in(world, seat) {
                return Outcome::Failed(StepFailure::NoPath);
            }
            Outcome::Running
        }
        Action::Hold { ms } => {
            if tick.raw() >= state.started.saturating_add(ticks_of(*ms)) {
                complete(world, seat, index);
                return Outcome::Complete;
            }
            Outcome::Running
        }
        Action::WaitUntil(condition) => {
            let seen = view(world, seat, seat_index, state);
            if cond::evaluate(&seen, condition) {
                complete(world, seat, index);
                return Outcome::Complete;
            }
            Outcome::Running
        }
        Action::Broadcast => Outcome::Failed(StepFailure::NoMast),
        Action::Interface {
            rows, durations, ..
        } => {
            if state.stage == VisitState::Running {
                return walk_to_visit(world, seat, state, rows.len(), times);
            }
            run_visit(world, seat, state, rows, durations, index)
        }
        Action::PlaceBeacon {
            mandate,
            rows,
            durations,
            tags,
            ..
        } => {
            let _ = tags;
            if state.resumed {
                if state.stage == VisitState::Running {
                    let left = rows_left(rows, state.visit_row);
                    return walk_to_visit(world, seat, state, left, times);
                }
                return run_visit(world, seat, state, rows, durations, index);
            }
            if state.stage == VisitState::Running {
                return walk_to_deploy(world, seat, state, times);
            }
            run_deploy(world, seat, state, *mandate, rows, durations, index)
        }
    }
}

/// One decision of a walk in to a visit: arrived within interface range, the
/// handshake begins; a beacon gone or a commander sealed in ends the step.
fn walk_to_visit(
    world: &mut World,
    seat: SeatId,
    state: &mut PlanState,
    rows: usize,
    times: &InterfaceTimes,
) -> Outcome {
    let tick = world.tick();
    let beacon = BeaconId::new(state.visit_beacon);
    let Some(row) = world_beacon_row(world, beacon) else {
        return Outcome::Failed(StepFailure::BeaconGone);
    };
    if !world
        .beacons()
        .hit_points()
        .get(row)
        .is_some_and(|hp| hp.is_alive())
    {
        return Outcome::Failed(StepFailure::BeaconGone);
    }
    let Some(commander) = commander_at(world, seat) else {
        return Outcome::Failed(StepFailure::CommanderDead);
    };
    let Some(at) = world.beacons().positions().get(row).copied() else {
        return Outcome::Failed(StepFailure::BeaconGone);
    };
    if within(commander, at, world.interface_range()) {
        begin_visit(world, state, times);
        emit(
            world,
            tick,
            EventKind::VisitStarted,
            seat,
            Some(beacon),
            Some(at),
            i64::try_from(rows).unwrap_or(i64::MAX),
        );
        return Outcome::Running;
    }
    if sealed_in(world, seat) {
        return Outcome::Failed(StepFailure::NoPath);
    }
    Outcome::Running
}

/// One decision of a walk in to a deploy site: arrived within interface range,
/// the deploy begins (and is charged); a commander sealed in ends the step.
fn walk_to_deploy(
    world: &mut World,
    seat: SeatId,
    state: &mut PlanState,
    times: &InterfaceTimes,
) -> Outcome {
    // The site was pinned off a point the step resolved; one no point stands
    // on is no site to deploy at.
    let Some(site) = pinned_point(state.pinned_at) else {
        return Outcome::Failed(StepFailure::IllegalSite);
    };
    let Some(commander) = commander_at(world, seat) else {
        return Outcome::Failed(StepFailure::CommanderDead);
    };
    if within(commander, site, world.interface_range()) {
        return match begin_deploy(world, seat, state, site, times) {
            Ok(()) => Outcome::Running,
            Err(failure) => Outcome::Failed(failure),
        };
    }
    if sealed_in(world, seat) {
        return Outcome::Failed(StepFailure::NoPath);
    }
    Outcome::Running
}

/// One decision's worth of a visit: the handshake, then one row at a time.
///
/// Also what a **resumed placement** runs: a visit to the beacon it placed,
/// from the first row it had not committed, which it keeps as its restart
/// point so a second interruption resumes later still.
fn run_visit(
    world: &mut World,
    seat: SeatId,
    state: &mut PlanState,
    rows: &[Row],
    durations: &[i32],
    index: u32,
) -> Outcome {
    let tick = world.tick();
    let beacon = BeaconId::new(state.visit_beacon);
    let Some(row_index) = world_beacon_row(world, beacon) else {
        return Outcome::Failed(StepFailure::BeaconGone);
    };
    if !world
        .beacons()
        .hit_points()
        .get(row_index)
        .is_some_and(|hp| hp.is_alive())
    {
        return Outcome::Failed(StepFailure::BeaconGone);
    }
    let Some(commander) = commander_at(world, seat) else {
        return Outcome::Failed(StepFailure::CommanderDead);
    };
    let Some(at) = world.beacons().positions().get(row_index).copied() else {
        return Outcome::Failed(StepFailure::BeaconGone);
    };
    if !within(commander, at, world.interface_range()) {
        return Outcome::Failed(StepFailure::OutOfRange);
    }
    if tick.raw() < state.visit_due {
        return Outcome::Running;
    }
    if state.stage == VisitState::Handshake {
        state.stage = VisitState::Committing;
        let Some((_, _, first_ms)) = row_at(rows, durations, state.visit_row) else {
            emit(
                world,
                tick,
                EventKind::VisitEnded,
                seat,
                Some(beacon),
                Some(at),
                i64::from(state.visit_row),
            );
            complete(world, seat, index);
            return Outcome::Complete;
        };
        state.visit_due = tick.raw().saturating_add(ticks_of(first_ms));
        return Outcome::Running;
    }
    let Some((done, row, _)) = row_at(rows, durations, state.visit_row) else {
        emit(
            world,
            tick,
            EventKind::VisitEnded,
            seat,
            Some(beacon),
            Some(at),
            i64::from(state.visit_row),
        );
        complete(world, seat, index);
        return Outcome::Complete;
    };
    let slots = crate::interpreter::binding_slots(done);
    if let Err(failure) = commit_row(world, row_index, seat, row, &state.bindings, slots) {
        return Outcome::Failed(failure);
    }
    emit(
        world,
        tick,
        EventKind::RowCommitted,
        seat,
        Some(beacon),
        Some(at),
        i64::from(state.visit_row),
    );
    state.visit_row = state.visit_row.saturating_add(1);
    if state.resumed {
        state.restart_row = state.visit_row;
    }
    if let Some((_, _, next_ms)) = row_at(rows, durations, state.visit_row) {
        state.visit_due = tick.raw().saturating_add(ticks_of(next_ms));
        return Outcome::Running;
    }
    emit(
        world,
        tick,
        EventKind::VisitEnded,
        seat,
        Some(beacon),
        Some(at),
        i64::from(state.visit_row),
    );
    complete(world, seat, index);
    Outcome::Complete
}

/// One decision's worth of a deploy: 12 s of standing still, then the beacon,
/// then its initial settings at interface rates.
#[allow(
    clippy::too_many_arguments,
    reason = "the deploy's step, its writ, its rows and their prices, each read here and nowhere else; a parameter struct would be built at the one caller only to be taken apart"
)]
fn run_deploy(
    world: &mut World,
    seat: SeatId,
    state: &mut PlanState,
    mandate: crate::seams::MandateKind,
    rows: &[Row],
    durations: &[i32],
    index: u32,
) -> Outcome {
    let tick = world.tick();
    let Some(site) = pinned_point(state.pinned_at) else {
        return Outcome::Failed(StepFailure::IllegalSite);
    };
    let Some(commander) = commander_at(world, seat) else {
        return Outcome::Failed(StepFailure::CommanderDead);
    };
    // "The commander must stay for all of it: death, leaving, or an illegal
    // site aborts the deploy" (spec section 5). Leaving is tested against the
    // interface range: the commander walked in to the site, and the deploy
    // requires the interface range (targeting.md: the placement range is
    // moot).
    //
    // Tested on **every** decision of the step, the initial settings rows
    // included, and not only while the deploy's clock runs: those rows are
    // interface rows at interface rates, and `run_visit` re-tests the range on
    // every decision of a visit for the same sentence of the same section. A
    // check that stopped once the beacon was in the ground would have let the
    // commander walk away and the rows commit behind it.
    if !within(commander, site, world.interface_range()) {
        return Outcome::Failed(StepFailure::OutOfRange);
    }
    if tick.raw() < state.visit_due {
        return Outcome::Running;
    }
    if state.stage == VisitState::Deploying {
        if !site_is_legal(world, seat, site) {
            return Outcome::Failed(StepFailure::IllegalSite);
        }
        let Some(placed) = world.place_beacon(seat, site, mandate) else {
            return Outcome::Failed(StepFailure::NoBeaconRoom);
        };
        state.visit_beacon = placed.raw();
        state.pinned_beacon = placed.raw();
        // The restart point: from here the step has a beacon to resume at,
        // and a step that death or the reflex clears resumes as a visit to it
        // rather than paying for another (targeting.md, "Companion changes").
        state.restart_beacon = placed.raw();
        state.restart_rule = state.rule;
        state.restart_at = index;
        state.restart_row = 0;
        emit(
            world,
            tick,
            EventKind::BeaconPlaced,
            seat,
            Some(placed),
            Some(site),
            i64::from(mandate.id()),
        );
        state.stage = VisitState::Committing;
        state.visit_row = 0;
        // No handshake: the visit handshake is charged once per visit to an
        // existing beacon (spec section 5's table), and a deploy is the
        // commander standing over the thing it is building.
        let Some((_, _, first_ms)) = row_at(rows, durations, 0) else {
            complete(world, seat, index);
            return Outcome::Complete;
        };
        state.visit_due = tick.raw().saturating_add(ticks_of(first_ms));
        return Outcome::Running;
    }
    let beacon = BeaconId::new(state.visit_beacon);
    let Some(row_index) = world_beacon_row(world, beacon) else {
        return Outcome::Failed(StepFailure::BeaconGone);
    };
    // The beacon a deploy's own settings are being written into can die between
    // the deploy and the last row, exactly as the beacon of a visit can.
    if !world
        .beacons()
        .hit_points()
        .get(row_index)
        .is_some_and(|hp| hp.is_alive())
    {
        return Outcome::Failed(StepFailure::BeaconGone);
    }
    let Some((done, row, _)) = row_at(rows, durations, state.visit_row) else {
        complete(world, seat, index);
        return Outcome::Complete;
    };
    let slots = crate::interpreter::binding_slots(done);
    if let Err(failure) = commit_row(world, row_index, seat, row, &state.bindings, slots) {
        return Outcome::Failed(failure);
    }
    emit(
        world,
        tick,
        EventKind::RowCommitted,
        seat,
        Some(beacon),
        Some(site),
        i64::from(state.visit_row),
    );
    state.visit_row = state.visit_row.saturating_add(1);
    state.restart_row = state.visit_row;
    if let Some((_, _, next_ms)) = row_at(rows, durations, state.visit_row) {
        state.visit_due = tick.raw().saturating_add(ticks_of(next_ms));
        return Outcome::Running;
    }
    complete(world, seat, index);
    Outcome::Complete
}

/// Apply one committed row.
///
/// **`set_mandate` is the only row that writes the writ.** A
/// `set_mandate_settings` row names *which* mandate's settings it edits — it
/// does not switch to that mandate — and the two are priced apart on purpose:
/// spec section 5 charges `switch_mandate_ms` (8 s) for the switch and
/// `edit_settings_base_ms` + per extra field, capped (2 s to 6 s), for the
/// edit. A settings row that wrote the writ would buy the 8 s change at the 2 s
/// price, which is the one reading the spec rules out.
///
/// **A row that cannot be applied still commits.** Spec section 5's table
/// prices a change by what it is, not by whether it landed, and item 23's
/// "an order the treasury cannot cover fails like any other step" is about a
/// *spend*, not about an edit. So a Build target added outside the beacon's own
/// sphere, or a removal naming an anchor nothing sits on, pays its time and
/// reports its commit and changes nothing — and the `row_committed` event names
/// the row, so a transcript shows it rather than hiding it.
///
/// **No row ever commits a lost target** (targeting.md, "Three reading
/// rules", 1): a row writing an `on` anchor whose bound feature is lost fails
/// the step `feature_lost` instead. `bindings` are the step's, and `slot` is
/// where this row's first one is.
///
/// The one row that does more than write a column is [`Row::Recycle`], which
/// takes the beacon out of the world; it is applied last of its visit because
/// the rows after it would have nothing to write to, which
/// [`run_visit`]'s `BeaconGone` test then catches on the next decision.
#[allow(
    clippy::too_many_lines,
    reason = "one arm per interface row, each the whole of what that row writes; the settings arm is the longest because a settings edit replaces three lists at once"
)]
fn commit_row(
    world: &mut World,
    row: usize,
    seat: SeatId,
    spec: &Row,
    bindings: &[Binding],
    slot: usize,
) -> Result<(), StepFailure> {
    let beacon = world
        .beacons()
        .ids()
        .get(row)
        .copied()
        .map(BeaconId::new)
        .ok_or(StepFailure::BeaconGone)?;
    let live = |world: &World, binding: &Binding| -> bool { feature_live(world, binding.feature) };
    match spec {
        Row::Mandate { kind, settings } => {
            // The switch first, which clears the old mandate's settings, then
            // the settings the row carries over the defaults it left: one row,
            // committing at the end of its own duration, all or nothing. The
            // one way the settings can fail -- a lost `on` target -- is tested
            // before the switch writes anything, so a failed row leaves the
            // old writ, its settings and its held seam exactly as they were.
            if let Some(settings) = settings {
                if !targets_live(world, settings, bindings, slot) {
                    return Err(StepFailure::FeatureLost);
                }
            }
            world.set_beacon_mandate(row, *kind);
            if let Some(settings) = settings {
                commit_row(world, row, seat, settings, bindings, slot)?;
            }
        }
        Row::Priority { priority } => world.set_beacon_priority(row, *priority),
        Row::Recycle => {
            world.recycle_beacon(seat, beacon);
        }
        Row::AddTarget { target } => {
            let (at, bound) = match target.anchor {
                Anchor::Voxel(voxel) => (point_of(voxel), (NO_FEATURE, DESCRIPTION_VOXEL)),
                Anchor::On(feature) => {
                    let binding = bindings.get(slot).ok_or(StepFailure::NoTarget)?;
                    if !live(world, binding) {
                        return Err(StepFailure::FeatureLost);
                    }
                    (
                        binding_point(binding).ok_or(StepFailure::IllegalSite)?,
                        (binding.feature, feature.description_id()),
                    )
                }
            };
            // One anchor, one building: a target on ground another target of
            // the seat's has already claimed would be charged for in its own
            // right and would stand a second structure in the same voxel
            // ([`World::anchor_is_claimed`]).
            if crate::mandate::inside_sphere(world, beacon, at)
                && !world.anchor_is_claimed(seat, at)
            {
                world.add_bound_target(beacon, target.blueprint, at, bound, target.build);
            }
        }
        Row::RemoveTarget { anchor } => match anchor {
            Removal::Voxel(voxel) => {
                world.remove_target_at(beacon, point_of(*voxel));
            }
            Removal::Feature { kind, anchor } => {
                // A feature index past `u32` is no feature a target is bound
                // to, so the removal names nothing and changes nothing.
                if let Some(feature) = world
                    .features()
                    .index_of(*kind, *anchor)
                    .and_then(|index| u32::try_from(index).ok())
                {
                    world.remove_bound_target(beacon, feature);
                }
            }
        },
        Row::Settings {
            targets,
            protected,
            probes,
            scouts,
            mine,
            ..
        } => {
            // A settings edit **replaces** the lists it names, because spec
            // section 5 makes a multi-field edit all-or-nothing and a list that
            // merged would have no way to remove an entry except the dedicated
            // `remove_build_target` row. A list the row does not name is left
            // alone, which is what "edits settings within the current mandate"
            // means for the fields it is silent about.
            if !targets.is_empty() {
                // All-or-nothing reaches the bindings too: a list with one lost
                // target commits none of it.
                if !targets_live(world, spec, bindings, slot) {
                    return Err(StepFailure::FeatureLost);
                }
                // The bindings this list's `on` anchors read when the step
                // started, in list order, and the point each one stands on;
                // every one is checked here, before anything is written, so
                // the iterator below reads them without a fallback.
                let bound = bound_slice(targets, bindings, slot).ok_or(StepFailure::NoTarget)?;
                if bound.iter().any(|binding| binding_point(binding).is_none()) {
                    return Err(StepFailure::IllegalSite);
                }
                // Replacing is not abandoning: an anchor that survives the
                // edit keeps the structure it has already been paid for, or
                // the mandate would buy it a second time
                // ([`World::replace_build_targets`]). The list goes over as an
                // iterator, so nothing here allocates inside a tick; each `on`
                // anchor takes the next binding, and the two checks above are
                // why there always is one, standing on a point.
                let entries = targets.iter().scan(bound.iter(), |read, target| {
                    let (anchor, feature, desc) = match target.anchor {
                        Anchor::Voxel(voxel) => (point_of(voxel), NO_FEATURE, DESCRIPTION_VOXEL),
                        Anchor::On(on) => {
                            let binding = read.next()?;
                            (
                                binding_point(binding)?,
                                binding.feature,
                                on.description_id(),
                            )
                        }
                    };
                    Some(BuildEntry {
                        blueprint: target.blueprint,
                        anchor,
                        feature,
                        desc,
                        build: target.build,
                    })
                });
                world.replace_build_targets(beacon, entries);
            }
            if !protected.is_empty() {
                world.clear_targets(beacon, TargetKind::Protected);
                for (centre, area) in protected {
                    world.add_area(beacon, AreaKind::Protected, point_of(*centre), *area);
                }
            }
            if !probes.is_empty() {
                world.clear_targets(beacon, TargetKind::Probe);
                for (centre, area) in probes {
                    world.add_area(beacon, AreaKind::Probe, point_of(*centre), *area);
                }
            }
            // A count past a byte is refused at the seal
            // (`PlanError::ScoutsOutOfRange`), so the row carries a byte.
            if *scouts > 0 {
                world.set_beacon_scouts(row, *scouts);
            }
            // The Mine settings the row names, and nothing it is silent about
            // (`crate::mining::MineEdit`).
            if let Some(edit) = mine {
                world.set_beacon_mine(row, *edit);
            }
        }
    }
    Ok(())
}

/// Whether every `on` target a [`Row::Settings`] row writes is still bound to
/// a live feature: the one test a settings edit can fail at commit time, kept
/// in one place so the switch that carries the edit can ask it before it
/// writes anything. Any other row has no targets and answers `true`.
fn targets_live(world: &World, spec: &Row, bindings: &[Binding], slot: usize) -> bool {
    let Row::Settings { targets, .. } = spec else {
        return true;
    };
    if targets.is_empty() {
        return true;
    }
    bound_slice(targets, bindings, slot).is_some_and(|bound| {
        bound
            .iter()
            .all(|binding| feature_live(world, binding.feature))
    })
}

/// The bindings a target list's `on` anchors read when the step started: the
/// `used` of them from `slot`, `None` when the step holds fewer.
fn bound_slice<'a>(
    targets: &[crate::interpreter::Target],
    bindings: &'a [Binding],
    slot: usize,
) -> Option<&'a [Binding]> {
    let used = targets
        .iter()
        .filter(|target| target.anchor.binds())
        .count();
    bindings.get(slot..slot.checked_add(used)?)
}

/// Whether the feature at index `feature` of the map's table is live. An
/// index past `usize` is no feature, so it is not.
fn feature_live(world: &World, feature: u32) -> bool {
    usize::try_from(feature).is_ok_and(|index| world.feature_is_live(index))
}

/// The point a binding's `on` column stands on: `None` for one no voxel
/// coordinate holds, which no column the resolver chose can be.
fn binding_point(binding: &Binding) -> Option<[Fx; 3]> {
    pinned_point(binding.at)
}

/// The guaranteed tail (spec section 10): hold, shadow or patrol, for ever.
fn run_fallback(
    world: &mut World,
    seat: SeatId,
    seat_index: usize,
    state: &mut PlanState,
    plan: &Plan,
) {
    let target = match plan.fallback() {
        Posture::Hold(place) => resolve_place(world, seat, seat_index, state, *place),
        Posture::Shadow(beacon) => {
            let seen = view(world, seat, seat_index, state);
            resolve_beacon(&seen, *beacon).and_then(|id| beacon_at(world, id))
        }
        Posture::Patrol(waypoints) => {
            // A leg past the patrol's end starts it over, the wrap the
            // arrival below applies; the compile refuses a patrol with no
            // waypoints, so the first is always there.
            if !within_list(state.fallback_leg, waypoints.len()) {
                state.fallback_leg = 0;
            }
            slot_of(state.fallback_leg)
                .and_then(|at| waypoints.get(at))
                .copied()
                .and_then(|place| resolve_place(world, seat, seat_index, state, place))
        }
    };
    let Some(target) = target else {
        return;
    };
    let Some(at) = commander_at(world, seat) else {
        return;
    };
    if within(at, target, world.arrive_radius()) {
        if let Posture::Patrol(waypoints) = plan.fallback() {
            let next = state.fallback_leg.saturating_add(1);
            state.fallback_leg = if within_list(next, waypoints.len()) {
                next
            } else {
                0
            };
            state.pinned = false;
        }
        return;
    }
    // The destination is written once per target rather than every decision, so
    // a fallback does not re-queue a repath twenty times a second.
    if !state.pinned || state.pinned_at != voxels_of(target) {
        state.pinned = true;
        state.pinned_at = voxels_of(target);
        walk_commander(world, seat, target);
    }
}

/// End the route and take up the fallback posture.
fn enter_fallback(world: &mut World, seat: SeatId, state: &mut PlanState, plan: &Plan) {
    if state.in_fallback() && state.stage == VisitState::NotStarted && state.pinned {
        return;
    }
    let tick = world.tick();
    let announce = !state.in_fallback();
    state.rule = NO_INDEX;
    state.rule_step = 0;
    state.cursor = NO_INDEX;
    state.clear_restart();
    drop_step(world, seat, state);
    if announce {
        emit(
            world,
            tick,
            EventKind::FallbackEngaged,
            seat,
            None,
            None,
            i64::from(plan.fallback().id()),
        );
    }
}

// ---------------------------------------------------------------------------
// The small helpers
// ---------------------------------------------------------------------------

/// Forget the step in progress, refunding a deploy that had not yet placed its
/// beacon.
///
/// Spec section 5, Placement: "death, leaving, or an illegal site aborts the
/// deploy and refunds it in full". Every way a step is forgotten goes through
/// here — a failure and its `on_fail`, the commander's death, the reflex
/// walking it away, a rule firing, the route ending — so the refund cannot be
/// missed by one of them. The stage is the whole of the bookkeeping:
/// [`VisitState::Deploying`] means "charged at deploy start and not yet
/// placed", because [`start_step`] charges on the way in and [`run_deploy`]
/// leaves the stage the moment the beacon is placed. Nothing new is hashed.
fn drop_step(world: &mut World, seat: SeatId, state: &mut PlanState) {
    if state.stage == VisitState::Deploying {
        world.refund_deploy(seat);
    }
    state.clear_step();
}

/// Refund every deploy still running as the segment closes, and forget that
/// step. Every other seat's state is left exactly as it was.
///
/// The playbook stops at segment end (spec section 10), so a deploy that has
/// not placed its beacon by the segment's last tick never will: the commander
/// cannot "stay for all of it". That is an aborted deploy, and spec section 5
/// refunds an aborted deploy in full. Done inside the tick that closes the
/// segment and before the Ledger settles, so the settlement, the frozen
/// snapshot and the final audit all read the refunded treasury.
pub(crate) fn abandon_deploys(
    world: &mut World,
    interpreter: &mut crate::interpreter::state::Interpreter,
) {
    let seats = interpreter.len();
    let mut index: usize = 0;
    while index < seats {
        let seat = world.seats().seats().get(index).copied().map(SeatId::new);
        if let (Some(seat), Some(state)) = (seat, interpreter.state_mut(index))
            && state.stage == VisitState::Deploying
        {
            drop_step(world, seat, state);
        }
        index = index.saturating_add(1);
    }
}

fn view<'a>(world: &'a World, seat: SeatId, seat_index: usize, state: &'a PlanState) -> View<'a> {
    View {
        world,
        seat,
        seat_index,
        state,
    }
}

/// Resolve a place, pin it on the state, and hand back the point.
fn pin_place(
    world: &mut World,
    seat: SeatId,
    seat_index: usize,
    state: &mut PlanState,
    place: Place,
) -> Result<[Fx; 3], StepFailure> {
    let beacon = match place {
        Place::Beacon(spec) => {
            let resolved = {
                let seen = view(world, seat, seat_index, state);
                resolve_beacon(&seen, spec)
            };
            match resolved {
                Some(id) => Some(id),
                None => return Err(StepFailure::NoTarget),
            }
        }
        Place::Voxel(_) => None,
        // `covering` is a placement's site and is resolved by the placement
        // itself; the compile refuses it anywhere else.
        Place::Covering(_) => return Err(StepFailure::NoTarget),
    };
    let at = match (place, beacon) {
        (Place::Voxel(voxel), _) => ground_point(world, voxel).ok_or(StepFailure::NoPath)?,
        (Place::Beacon(_), Some(id)) => beacon_at(world, id).ok_or(StepFailure::NoTarget)?,
        (Place::Beacon(_) | Place::Covering(_), None) | (Place::Covering(_), Some(_)) => {
            return Err(StepFailure::NoTarget);
        }
    };
    state.pinned = true;
    state.pinned_beacon = beacon.map_or(NO_INDEX, BeaconId::raw);
    state.pinned_at = voxels_of(at);
    Ok(at)
}

/// Resolve a place without pinning it — what the fallback uses.
fn resolve_place(
    world: &World,
    seat: SeatId,
    seat_index: usize,
    state: &PlanState,
    place: Place,
) -> Option<[Fx; 3]> {
    match place {
        Place::Voxel(voxel) => ground_point(world, voxel),
        Place::Beacon(spec) => {
            let seen = view(world, seat, seat_index, state);
            resolve_beacon(&seen, spec).and_then(|id| beacon_at(world, id))
        }
        // A fallback never covers anything: the compile refuses it there.
        Place::Covering(_) => None,
    }
}

/// Whether a site is legal: inside one of the seat's own spheres (spec section
/// 5, Placement; item 11) and **not on the column of one of the seat's own live
/// beacons** -- no stacking, a legality rule with no rules-table row
/// (`docs/design/targeting.md`, "Companion changes"; decision 10 of S1's plan).
///
/// There is no placement range any more: `place_beacon` walks to its site
/// first, so the commander's distance from the site is the walk's business,
/// and the deploy requires the interface range (targeting.md, amending item
/// 11; `commander.placement_range_voxels` stays in the schema, superseded).
fn site_is_legal(world: &World, seat: SeatId, site: [Fx; 3]) -> bool {
    let ground = world.ground();
    let [x, y] = crate::targeting::column_of(site);
    ground.inside_own_sphere(seat, site) && !ground.stacks_on_own(seat, x, y)
}

/// Send the commander somewhere. The route itself is the router's (T7): this
/// writes the destination and asks for a repath.
fn walk_commander(world: &mut World, seat: SeatId, to: [Fx; 3]) {
    let commander = world.commander_of(seat);
    if commander.is_some() {
        world.set_unit_destination(commander.raw(), to);
    }
}

/// Whether the commander has been parked as sealed in — no route exists (item
/// 60's "park and report").
fn sealed_in(world: &World, seat: SeatId) -> bool {
    let commander = world.commander_of(seat);
    commander.is_some()
        && world.router().state(commander.raw()) == crate::pathing::router::WalkState::Sealed
}

fn commander_at(world: &World, seat: SeatId) -> Option<[Fx; 3]> {
    let commander = world.commander_of(seat);
    if !commander.is_some() {
        return None;
    }
    let index = usize::try_from(commander.raw()).ok()?;
    let alive = world
        .units()
        .hit_points()
        .get(index)
        .is_some_and(|hp| hp.is_alive());
    if !alive {
        return None;
    }
    world.units().positions().get(index).copied()
}

fn beacon_at(world: &World, beacon: BeaconId) -> Option<[Fx; 3]> {
    let row = world_beacon_row(world, beacon)?;
    world.beacons().positions().get(row).copied()
}

fn world_beacon_row(world: &World, beacon: BeaconId) -> Option<usize> {
    let beacons = world.beacons();
    let index = usize::try_from(beacon.raw()).ok()?;
    if beacons.ids().get(index).copied() == Some(beacon.raw()) {
        return Some(index);
    }
    let count = usize::try_from(beacons.len()).unwrap_or(0);
    (0..count).find(|row| beacons.ids().get(*row).copied() == Some(beacon.raw()))
}

fn complete(world: &mut World, seat: SeatId, index: u32) {
    let tick = world.tick();
    emit(
        world,
        tick,
        EventKind::StepCompleted,
        seat,
        None,
        None,
        i64::from(index),
    );
}

fn emit(
    world: &mut World,
    tick: Tick,
    kind: EventKind,
    seat: SeatId,
    subject: Option<BeaconId>,
    at: Option<[Fx; 3]>,
    value: i64,
) {
    let mut emission = Emission::of(kind).seat(seat).value(value);
    if let Some(beacon) = subject {
        emission = emission.subject(AssetId::of_beacon(beacon));
    }
    if let Some(point) = at {
        emission = emission.at(Position::from_array(point));
    }
    world.emit(tick, emission);
}

/// A `u32` position of the plan's lists as an index: `None` for one no
/// `usize` holds, which is past the end of every list.
fn slot_of(at: u32) -> Option<usize> {
    usize::try_from(at).ok()
}

/// Whether the `u32` position `at` lies inside a list of `len` entries.
fn within_list(at: u32, len: usize) -> bool {
    slot_of(at).is_some_and(|at| at < len)
}

/// The row at `index` of a step's rows, with the rows before it and the price
/// it was compiled at: `None` past the end, which is where a visit ends.
/// `rows` and `durations` are one length, because the compile prices every row
/// it compiles ([`crate::interpreter::Row::duration_ms`]).
fn row_at<'a>(rows: &'a [Row], durations: &[i32], index: u32) -> Option<(&'a [Row], &'a Row, i32)> {
    let at = slot_of(index)?;
    let (done, rest) = rows.split_at_checked(at)?;
    Some((done, rest.first()?, durations.get(at).copied()?))
}

/// How many of a step's rows are left once `done` have committed: none once
/// `done` reaches the end, which a resumed placement's restart row can.
///
/// Exact, not a fallback: a `done` no `usize` holds is past the end of any
/// slice, so none are left -- the same answer the subtraction gives a `done`
/// at or past `rows.len()`.
fn rows_left(rows: &[Row], done: u32) -> usize {
    match slot_of(done) {
        Some(done) => rows.len().saturating_sub(done),
        None => 0,
    }
}

/// A playbook's voxel, **standing on the ground under it**.
///
/// The router puts a walker on a *column*: it routes to `node_at`, which
/// discards `z`, and stands the unit at `Surface::standing_z` of the column it
/// reaches. Arrival is a squared distance over all three axes, so a target kept
/// at the author's own `z` could only ever be reached when that `z` happened to
/// land within `arrive_radius_voxels` of the terrain height — the spec's own
/// worked example writes `z: 62` in a map 64 voxels tall, which no ground
/// column comes near, and a `move` to it could never complete. The author's
/// `x` and `y` are the question; the `z` is the ground's answer, and the same
/// projection is what makes a `patrol` leg advance.
///
/// A column off the map keeps the author's `z`: there is no ground to stand on,
/// and the step then fails on its own terms — by timing out, or by
/// `no_path` — rather than being silently moved somewhere legal.
///
/// `None` when the ground's standing height is one no voxel coordinate holds,
/// which a map this sim generates never has: the caller fails the step rather
/// than keep a height the ground did not give.
fn ground_point(world: &World, voxel: [i16; 3]) -> Option<[Fx; 3]> {
    let [x, y, z] = voxel;
    let standing = match world.surface().node_of(i32::from(x), i32::from(y)) {
        Some(node) => i16::try_from(world.surface().standing_z(node)).ok()?,
        None => z,
    };
    Some(point_of([x, y, standing]))
}

/// Whole voxels to a position, as a playbook writes them. Total: the compile
/// keeps only voxels a voxel coordinate holds
/// ([`crate::interpreter::PlanError::VoxelOutOfMap`] and
/// [`crate::interpreter::PlanError::VoxelOutOfRange`]).
fn point_of(voxel: [i16; 3]) -> [Fx; 3] {
    let [x, y, z] = voxel;
    [Fx::from_voxels(x), Fx::from_voxels(y), Fx::from_voxels(z)]
}

/// A place the state holds in whole voxels, as a position: `None` for one no
/// voxel coordinate holds. The state's places are written by [`voxels_of`]
/// from positions, so every one the run wrote converts; a restored state
/// that holds another is answered by the step's own failure, never by a
/// position at the origin.
fn pinned_point(voxel: [i32; 3]) -> Option<[Fx; 3]> {
    let [x, y, z] = voxel;
    Some(point_of([
        i16::try_from(x).ok()?,
        i16::try_from(y).ok()?,
        i16::try_from(z).ok()?,
    ]))
}

/// A position to whole voxels, for the pinned target the state carries.
fn voxels_of(at: [Fx; 3]) -> [i32; 3] {
    let [x, y, z] = at;
    [x.floor_voxels(), y.floor_voxels(), z.floor_voxels()]
}

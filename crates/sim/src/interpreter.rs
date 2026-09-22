// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The playbook interpreter at the v1 vocabulary (spec sections 4, 5 and 10).
//!
//! A playbook is **typed declarative data, not a script** (AGENTS.md §2). This
//! module is the Rust that reads it: [`Plan::compile`] turns one decoded
//! `gp.v1.Playbook` into a compiled [`Plan`], and [`crate::world::Phase::Decision`]
//! runs one decision per `match.decision_tick_ms` of game time against it.
//! Nothing here interprets user *text*: the vocabulary is closed, every
//! construct is a proto field, and a construct this build cannot execute is
//! **refused at the door** rather than skipped (spec section 10, "Load never
//! strips").
//!
//! # The three objects
//!
//! * [`Plan`] — the compiled playbook. An **input**, exactly like the rules
//!   table: the sim is a pure function of `(map seed, playbooks, rules hash)`,
//!   so a plan is never hashed and never travels in a snapshot.
//! * [`PlanState`] — one seat's execution state: the route step in progress,
//!   the rule body running, each handler's cooldown and fire count, the pinned
//!   selector target, the visit with its handshake and row timers, and the
//!   reflex's last-damage marker. **All of it is hashed state**, in
//!   [`crate::world::World::encode`], in the snapshot round-trip and in the
//!   goldens, in this pull request (AGENTS.md §4.8).
//! * [`Interpreter`] — one [`PlanState`] per seat, held by the world.
//!
//! # Execution, in the order the rules bind
//!
//! Item 24 and spec section 10, "Execution":
//!
//! 1. **One body at a time, no pre-emption.** On each decision, if no rule body
//!    is running, the first enabled handler whose condition is true starts its
//!    body; while a body runs, no handler is even evaluated. While none runs,
//!    the current route step continues.
//! 2. **Only the reflex interrupts.** It fires at commander HP ≤
//!    [`REFLEX_HP_PERCENT`] — fixed, not disableable, and there is no
//!    `reflex_hp_pct` option (the field number is reserved). It aborts any
//!    visit in progress, walks the commander to the safest own beacon, and then
//!    the route continues **from the step it was on**, which restarts that
//!    step — so a resumed visit is a new visit and pays the handshake again. It
//!    re-fires only after *new damage*.
//! 3. **Interface rows are atomic.** Each row is one change that commits at the
//!    end of its own duration, at spec section 5's rates, read from
//!    `interface_times.*` in the rules table. Committed rows stay. Damage does
//!    not interrupt; a visit stops only when the commander dies, the reflex
//!    fires, the beacon is destroyed, or the commander leaves range.
//! 4. **Segment end: the playbook stops** and nothing carries over except the
//!    world itself. [`Interpreter::open_push`] resets every per-segment field.
//!
//! # Why every playbook halts
//!
//! Three properties, each checked here rather than assumed:
//!
//! * **A jump only ever goes forward.** A route step's `on_fail` jump is
//!   resolved at compile time and refused unless its target is strictly later
//!   ([`PlanError::BackwardJump`]); a handler's `resume_at_label` is resolved at
//!   compile time and *clamped* forward at run time, because the route's cursor
//!   is not known until the rule fires.
//! * **Every wait has a timeout.** A `wait_until` with no positive
//!   `timeout_ms` is refused ([`PlanError::WaitWithoutTimeout`]).
//! * **The route cursor never decreases.** Every transition — complete, skip,
//!   fail, jump, resume — moves it forward or leaves it alone, so a route of
//!   *n* steps enters at most *n* steps.
//!
//! `crates/sim/tests/interpreter.rs`'s `every_playbook_halts` is the property
//! test over a generated corpus; `a_jump_only_goes_forward` and
//! `every_wait_has_a_timeout` are the two named cases.
//!
//! # Durations are game milliseconds, converted at one boundary
//!
//! Item 46: every duration in a playbook is an `int32` of **game
//! milliseconds**, never ticks. [`ticks_of`] is the only conversion in this
//! module, and it rounds **up**, so no wait is ever cut short. Nothing here
//! reads a clock (AGENTS.md §4.5), and every condition is an integer
//! comparison.
//!
//! # What this build refuses, and why that is the honest answer
//!
//! A construct in the v1 vocabulary whose *effect* needs a stage that has not
//! been built — a row that queues a capability structure, a selector that ranks
//! by incoming threat — is refused by [`Plan::compile`] with
//! [`PlanError::NotAtThisStage`], which names the construct and the stage that
//! fills it. It is refused at **seal** time, where there is a host to tell,
//! rather than halfway through a Push where there is nobody, and it is never
//! silently treated as true, false or a no-op. The list is in
//! [`PlanError::NotAtThisStage`]'s documentation.

pub(crate) mod cond;
pub(crate) mod exec;
pub mod state;

use crate::math::quantity::Ms;
use crate::rules::RulesTable;
use crate::seams::MandateKind;
use crate::tables::BeaconId;
use pharmakos_proto::gp;

pub use state::{Interpreter, PlanParts, PlanState, StepFailure, VisitState};

/// The self-preservation reflex's threshold, in whole percent of the
/// commander's maximum hit points.
///
/// **Fixed, not disableable, and not a tuning value** (item 24, spec section
/// 10): `gp.v1.Options` reserves the `reflex_hp_pct` field number precisely so
/// that no playbook can move it, and author-tuned bravery is an ordinary flee
/// handler. It is a constant here rather than a rules-table row for that
/// reason — a row is a number the owner may tune, and this one is a rule.
pub const REFLEX_HP_PERCENT: i32 = 20;

/// The deepest an `all` / `any` / `not` tree may nest (spec section 10,
/// "Conditions": depth ≤ 4).
pub const MAX_CONDITION_DEPTH: u32 = 4;

/// The most nodes one condition tree may hold (spec section 10: ≤ 24 nodes).
pub const MAX_CONDITION_NODES: u32 = 24;

/// PLACEHOLDER (harness): how many route steps one plan may hold.
///
/// A route step is one unit of the playbook size budget
/// (`verifier.size_budget_units`, item 94), so a plan can never hold more steps
/// than that budget — 128 today. This constant is the *interpreter's* own
/// ceiling, written down so that the state's route cursor has a bound
/// independent of a rules row the owner may raise; the compiler refuses a
/// longer route with [`PlanError::TooManySteps`]. Owner, at S3, with P1's real
/// size budget.
pub const MAX_ROUTE_STEPS: usize = 256;

/// PLACEHOLDER (harness): how many handlers one plan may hold, bounded for the
/// reason [`MAX_ROUTE_STEPS`] is. Owner, at S3.
pub const MAX_HANDLERS: usize = 64;

/// Game milliseconds to whole ticks, rounding **up**.
///
/// The one conversion boundary this module has (item 46). Rounding up is what
/// keeps a timeout from expiring early and a hold from ending a tick short.
#[must_use]
pub fn ticks_of(ms: i32) -> u32 {
    Ms::new(ms).to_ticks_ceil()
}

/// One integer comparison — the only kind a condition may make (spec section
/// 10).
///
/// `gp.v1` spells the six operators twice, on `CmdrHpPct.Cmp` and on
/// `IntCompare.Op`, for the reason `playbook.proto` gives: the spec's worked
/// example fixes `CmdrHpPct`'s field names. They mean the same six things, so
/// the interpreter has one type for them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Cmp {
    /// Less than.
    Lt,
    /// Less than or equal.
    Le,
    /// Equal.
    Eq,
    /// Not equal.
    Ne,
    /// Greater than or equal.
    Ge,
    /// Greater than.
    Gt,
}

impl Cmp {
    /// Apply the comparison to two integers of the predicate's own unit.
    #[must_use]
    pub const fn holds(self, left: i64, right: i64) -> bool {
        match self {
            Cmp::Lt => left < right,
            Cmp::Le => left <= right,
            Cmp::Eq => left == right,
            Cmp::Ne => left != right,
            Cmp::Ge => left >= right,
            Cmp::Gt => left > right,
        }
    }

    /// The comparison a `gp.v1.IntCompare.Op` names.
    fn of_op(op: i32) -> Option<Cmp> {
        match gp::v1::int_compare::Op::try_from(op).ok()? {
            gp::v1::int_compare::Op::Unspecified => None,
            gp::v1::int_compare::Op::Lt => Some(Cmp::Lt),
            gp::v1::int_compare::Op::Le => Some(Cmp::Le),
            gp::v1::int_compare::Op::Eq => Some(Cmp::Eq),
            gp::v1::int_compare::Op::Ne => Some(Cmp::Ne),
            gp::v1::int_compare::Op::Ge => Some(Cmp::Ge),
            gp::v1::int_compare::Op::Gt => Some(Cmp::Gt),
        }
    }

    /// The comparison a `gp.v1.CmdrHpPct.Cmp` names.
    fn of_cmp(cmp: i32) -> Option<Cmp> {
        match gp::v1::cmdr_hp_pct::Cmp::try_from(cmp).ok()? {
            gp::v1::cmdr_hp_pct::Cmp::Unspecified => None,
            gp::v1::cmdr_hp_pct::Cmp::Lt => Some(Cmp::Lt),
            gp::v1::cmdr_hp_pct::Cmp::Le => Some(Cmp::Le),
            gp::v1::cmdr_hp_pct::Cmp::Eq => Some(Cmp::Eq),
            gp::v1::cmdr_hp_pct::Cmp::Ne => Some(Cmp::Ne),
            gp::v1::cmdr_hp_pct::Cmp::Ge => Some(Cmp::Ge),
            gp::v1::cmdr_hp_pct::Cmp::Gt => Some(Cmp::Gt),
        }
    }
}

/// Which beacons a late-bound selector ranks.
///
/// Own beacons only at the skeleton: `ENEMY_KNOWN` needs the seat's knowledge
/// store, which is S2's and S3's, and a tag filter needs a per-beacon tag
/// column that does not exist. Both are refused by [`Plan::compile`] rather
/// than answered wrongly.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Filter {
    /// Only beacons carrying this writ, when the filter names one.
    pub mandate: Option<MandateKind>,
}

/// A beacon, fixed or late-bound (spec section 10, "Late-bound selectors").
///
/// A selector **resolves when the step starts and stays pinned for that step**.
/// Resolving to nothing is a step failure, so `on_fail` decides — never a
/// silent skip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BeaconSpec {
    /// A beacon named by its `b_NN` id in the frozen snapshot.
    Id(BeaconId),
    /// "the safest own beacon" — the target the fixed reflex walks to.
    Safest,
    /// Least travel from the commander first.
    Nearest(Filter),
    /// Lowest hit points first (the beacon's own, not its structures').
    Weakest(Filter),
}

/// A place, fixed or late-bound.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    /// Whole voxel coordinates, as a playbook writes them.
    Voxel([i32; 3]),
    /// The anchor of whichever beacon the selector picks.
    Beacon(BeaconSpec),
}

/// One on-site change (spec section 5's change list).
///
/// Each row is **one change that commits at the end of its own duration**, so a
/// multi-field edit is all-or-nothing. The durations are rules-table data, not
/// schema and not constants: [`Row::duration_ms`] reads them.
///
/// **Not `Copy`.** A settings row carries the lists it writes - Build targets,
/// protected areas and Survey probe areas - and those are `Vec`s. They are in
/// the compiled plan rather than looked up again at commit time for the reason
/// everything else in a `Plan` is: a playbook is an input, compiled once at the
/// seal, and a row that re-read the submission would be reading it during a
/// tick.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Row {
    /// Switch mandate type — `interface_times.switch_mandate_ms`. Clears the
    /// old mandate's settings and targets; the beacon's Quartermaster priority
    /// survives (item 20).
    Mandate {
        /// The writ the beacon takes.
        kind: MandateKind,
    },
    /// Edit mandate settings — base plus per extra field, capped.
    ///
    /// It edits *within the current mandate* (`playbook.proto`'s own wording)
    /// and never switches the writ: switching is [`Row::Mandate`], at
    /// `switch_mandate_ms`, which is four times this row's base rate.
    Settings {
        /// Which mandate's settings the row names. Read at `place_beacon`
        /// time, where choosing the mandate *type* is free (spec section 5) and
        /// only the settings cost time; **not** written by a committed row.
        kind: Option<MandateKind>,
        /// How many settings fields the row writes. The first costs the base
        /// time and each further one the per-field time, capped at the maximum.
        fields: u32,
        /// The Build mandate's target list: blueprint and anchor.
        targets: Vec<(u8, [i32; 3])>,
        /// The Build mandate's protected areas, as centre and radius in whole
        /// voxels.
        protected: Vec<([i32; 3], i32)>,
        /// The Survey mandate's probe areas, same shape.
        probes: Vec<([i32; 3], i32)>,
        /// The Survey mandate's scout count.
        scouts: u32,
    },
    /// Set the Quartermaster priority — `interface_times.set_priority_ms`.
    Priority {
        /// `gp.v1.InterfaceRow.QuartermasterPriority`'s wire value.
        priority: u8,
    },
    /// Recycle the beacon — `interface_times.recycle_ms`.
    ///
    /// `economy.recycle_refund_percent` of its remaining value is refunded when
    /// it commits, and the removal books destruction credit exactly as
    /// destruction does (item 18).
    Recycle,
    /// Add one target to the beacon's Build mandate —
    /// `interface_times.build_target_ms`.
    AddTarget {
        /// The blueprint, as a [`crate::tables::StructureKind::id`].
        blueprint: u8,
        /// Where it goes. Must lie inside one of your own spheres, which the
        /// interpreter tests when the row commits.
        anchor: [i32; 3],
    },
    /// Remove the Build target anchored at this place —
    /// `interface_times.build_target_ms`.
    ///
    /// Named by its anchor rather than by an index because a playbook is sealed
    /// before the round runs, and an index into a list the mandate may have
    /// changed is not a stable reference (`playbook.proto`'s own words).
    RemoveTarget {
        /// The anchor naming the target.
        anchor: [i32; 3],
    },
}

impl Row {
    /// How long this row takes to commit, in game milliseconds, from
    /// `interface_times.*`.
    ///
    /// Read from the rules table on every commit rather than cached in the
    /// compiled plan, because a tuning value is data and the table is the one
    /// place it lives (AGENTS.md §12).
    #[must_use]
    pub fn duration_ms(&self, rules: &RulesTable) -> i32 {
        let times = rules.message().interface_times.as_ref();
        let get = |pick: fn(&gp::v1::rules_table::InterfaceTimes) -> i32| -> i32 {
            times.map_or(0, pick)
        };
        match *self {
            Row::Mandate { .. } => get(|t| t.switch_mandate_ms),
            Row::Priority { .. } => get(|t| t.set_priority_ms),
            Row::Recycle => get(|t| t.recycle_ms),
            Row::AddTarget { .. } | Row::RemoveTarget { .. } => get(|t| t.build_target_ms),
            Row::Settings { fields, .. } => {
                let base = get(|t| t.edit_settings_base_ms);
                let per = get(|t| t.edit_settings_per_field_ms);
                let max = get(|t| t.edit_settings_max_ms);
                let extra = i32::try_from(fields.saturating_sub(1)).unwrap_or(0);
                let total = base.saturating_add(per.saturating_mul(extra));
                if max > 0 { total.min(max) } else { total }
            }
        }
    }
}

/// What a step does. Exactly one per step, and required.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Action {
    /// Walk somewhere. Completes within the arrive radius; fails on no path or
    /// timeout.
    Move {
        /// Where to.
        to: Place,
        /// The pace the author asked for.
        ///
        /// PLACEHOLDER: `AVOID_KNOWN_THREATS` routes exactly as `DIRECT` does
        /// at the skeleton, because there are no known threats to avoid — the
        /// knowledge store and the threat weighting are S2's and S3's. Kept in
        /// the compiled plan so that the day it means something, nothing about
        /// the playbook has to change (owner, at S2).
        avoid_known_threats: bool,
    },
    /// Interface with a beacon. Completes when all rows are committed.
    Interface {
        /// Which beacon.
        beacon: BeaconSpec,
        /// The rows, applied in order, each committing on its own.
        rows: Vec<Row>,
    },
    /// Place a beacon: `interface_times.place_beacon_deploy_ms` of deploy, then
    /// the initial settings at interface rates.
    PlaceBeacon {
        /// The site.
        at: Place,
        /// The writ the new beacon carries. Choosing the type is free.
        mandate: MandateKind,
        /// The initial settings, as the interface rows they are.
        rows: Vec<Row>,
        /// How many author tags the step carries.
        ///
        /// PLACEHOLDER: the tags themselves are not stored, because a beacon
        /// has no tag column and a `Vec<String>` per beacon is neither
        /// fixed-width hashed state nor allocation-free inside a tick. Writing
        /// tags is legal and round-trips; *reading* them — a selector's tag
        /// filter — is refused loudly by [`Plan::compile`], so nothing answers
        /// a tag question wrongly (owner, at S3, with the full selector work).
        tags: u32,
    },
    /// Wait for a condition. The timeout is required.
    WaitUntil(Cond),
    /// Hold still for a duration.
    Hold {
        /// Game milliseconds.
        ms: i32,
    },
    /// Send a radio message.
    ///
    /// There is no Radio Mast in v1's skeleton (S4), so this step **fails
    /// loudly** with [`StepFailure::NoMast`] and `on_fail` decides. It is never
    /// a silent no-op: the plan's own line for this step says so.
    Broadcast,
}

/// What to do when a step fails (spec section 10).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailAction {
    /// Skip the step and go on.
    Skip,
    /// End the route (or, inside a rule body, end the body).
    AbortRoute,
    /// Jump to a later route step, by index.
    JumpForward(u32),
}

/// One route step, or one step of a handler body.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PlanStep {
    /// Guard: skip the step when this is true as it would start.
    pub skip_if: Option<Cond>,
    /// Guard: game milliseconds, or zero for none.
    pub timeout_ms: i32,
    /// Guard: what a failure does.
    pub on_fail: FailAction,
    /// The step itself.
    pub action: Action,
}

/// What the route does once a rule body ends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resume {
    /// Carry on with the step the route was on.
    Continue,
    /// Skip that step.
    SkipStep,
    /// Jump to a later route step, by index. Clamped forward at run time.
    JumpForward(u32),
    /// End the route: the fallback takes over.
    EndRoute,
}

impl Resume {
    /// The wire id, for the `rule_ended` event's value.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Resume::Continue => 1,
            Resume::SkipStep => 2,
            Resume::JumpForward(_) => 3,
            Resume::EndRoute => 4,
        }
    }
}

/// One handler (rule).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rule {
    /// Required. Evaluated only while no body is running.
    pub when: Cond,
    /// Runs to its end; no handler pre-empts another.
    pub body: Vec<PlanStep>,
    /// What the route does afterwards.
    pub resume: Resume,
    /// Game milliseconds, at least `verifier.handler_cooldown_min_ms`.
    pub cooldown_ms: i32,
    /// 1 to `verifier.max_fires_max`.
    pub max_fires: u32,
}

/// The guaranteed tail (spec section 10, "A guaranteed tail").
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Posture {
    /// Hold a place.
    Hold(Place),
    /// Stay with a beacon.
    Shadow(BeaconSpec),
    /// Walk a list of waypoints, in order, for ever.
    Patrol(Vec<Place>),
}

impl Posture {
    /// The wire id, for the `fallback_engaged` event's value.
    #[must_use]
    pub const fn id(&self) -> u8 {
        match *self {
            Posture::Hold(_) => 1,
            Posture::Shadow(_) => 2,
            Posture::Patrol(_) => 3,
        }
    }
}

/// A condition tree. Integer comparisons only, over the seat's own knowledge.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Cond {
    /// Every item is true.
    All(Vec<Cond>),
    /// Some item is true.
    Any(Vec<Cond>),
    /// The item is false.
    Not(Box<Cond>),
    /// The commander's hit points as a whole percentage of its maximum.
    CmdrHpPct(Cmp, i64),
    /// Whether the commander took damage within a window of game milliseconds.
    CmdrTookDamageWithin(i32),
    /// How many times the commander has died this match.
    CmdrDeaths(Cmp, i64),
    /// How far into the current segment the Push has run, in game
    /// milliseconds.
    SegmentElapsed(Cmp, i64),
    /// A beacon's own hit points as a whole percentage of its maximum.
    BeaconHpPct(BeaconSpec, Cmp, i64),
    /// Whether a beacon is powered (`true`) or dormant (`false`).
    BeaconPowered(BeaconSpec, bool),
    /// The seat treasury in whole `$`.
    Treasury(Cmp, i64),
    /// Supply minus draw, in whole `kW`.
    KwHeadroom(Cmp, i64),
    /// Whether the route has reached a step, by route index. True from the
    /// moment the step starts and for the rest of the segment.
    StepReached(u32),
    /// Whether a handler has fired, by handler index.
    RuleFired(u32),
}

/// A compiled playbook: an **input** to the sim, never hashed state.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Plan {
    route: Vec<PlanStep>,
    handlers: Vec<Rule>,
    fallback: Posture,
    max_deaths_before_fallback: u32,
}

impl Plan {
    /// The route, in the order the commander walks it.
    #[must_use]
    pub fn route(&self) -> &[PlanStep] {
        &self.route
    }

    /// The handlers, in priority order.
    #[must_use]
    pub fn handlers(&self) -> &[Rule] {
        &self.handlers
    }

    /// The guaranteed tail.
    #[must_use]
    pub const fn fallback(&self) -> &Posture {
        &self.fallback
    }

    /// After this many commander deaths in the segment, the playbook stops and
    /// the fallback takes over (`on_death.max_deaths_before_fallback`).
    #[must_use]
    pub const fn max_deaths_before_fallback(&self) -> u32 {
        self.max_deaths_before_fallback
    }

    /// Compile a decoded playbook, or say exactly what is wrong with it.
    ///
    /// The verifier (`pharmakos-verifier`) is the seal inspection and rejects a
    /// playbook that does not qualify; this is the sim's own door, and it is
    /// deliberately not the same check. The verifier answers *"may this be
    /// sealed"*; this answers *"can this build execute it"*, and the two differ
    /// exactly by the constructs [`PlanError::NotAtThisStage`] lists.
    ///
    /// # Errors
    ///
    /// Returns the first [`PlanError`] the playbook trips, naming the route
    /// step or handler it was found in.
    pub fn compile(playbook: &gp::v1::Playbook, rules: &RulesTable) -> Result<Plan, PlanError> {
        let declarative = playbook
            .declarative
            .as_ref()
            .ok_or(PlanError::MissingBlock("declarative"))?;
        let on_death = playbook
            .on_death
            .as_ref()
            .ok_or(PlanError::MissingBlock("on_death"))?;
        let fallback_block = playbook
            .fallback
            .as_ref()
            .ok_or(PlanError::MissingBlock("fallback"))?;

        if declarative.route.len() > MAX_ROUTE_STEPS {
            return Err(PlanError::TooManySteps {
                found: declarative.route.len(),
                limit: MAX_ROUTE_STEPS,
            });
        }
        if declarative.handlers.len() > MAX_HANDLERS {
            return Err(PlanError::TooManyHandlers {
                found: declarative.handlers.len(),
                limit: MAX_HANDLERS,
            });
        }

        // Two rules-table blocks are read on every visit and every deploy, and
        // proto3 cannot tell an absent message from an empty one — so a table
        // with no `interface_times` would price every handshake, every row and
        // every deploy at zero milliseconds, and one with no `verifier` block
        // would refuse every handler ever written (`max_fires_max` of nought).
        // Refused here, for the reason `pharmakos-verifier`'s `limits.rs`
        // refuses the same gap: a missing block is a broken table, not a table
        // of zeroes.
        if rules.message().interface_times.is_none() {
            return Err(PlanError::MissingBlock("rules.interface_times"));
        }
        if rules.message().verifier.is_none() {
            return Err(PlanError::MissingBlock("rules.verifier"));
        }

        let names = Names {
            labels: &declarative.route,
            handlers: &declarative.handlers,
            extent: rules.map_size_voxels(),
        };

        let mut route: Vec<PlanStep> = Vec::with_capacity(declarative.route.len());
        for (index, step) in declarative.route.iter().enumerate() {
            let at = u32::try_from(index).unwrap_or(u32::MAX);
            route.push(compile_step(step, &names, Some(at))?);
        }

        let limits = Limits::from_rules(rules);
        let mut handlers: Vec<Rule> = Vec::with_capacity(declarative.handlers.len());
        for handler in &declarative.handlers {
            handlers.push(compile_handler(handler, &names, limits)?);
        }

        // `on_respawn` names one value in v1 and reserves the rest: CONTINUE,
        // "resume the route at the step the commander was on". An unset value
        // is an error rather than a default, like every other enum in the
        // package (the one exception is `BeaconFilter`, and it is named there).
        match gp::v1::on_death::OnRespawn::try_from(on_death.on_respawn) {
            Ok(gp::v1::on_death::OnRespawn::Continue) => {}
            Ok(_) => return Err(PlanError::UnsetEnum("on_death.on_respawn")),
            Err(_) => {
                return Err(PlanError::UnknownEnum {
                    field: "on_death.on_respawn",
                    value: on_death.on_respawn,
                });
            }
        }

        let fallback = compile_fallback(fallback_block, &names)?;

        Ok(Plan {
            route,
            handlers,
            fallback,
            max_deaths_before_fallback: on_death.max_deaths_before_fallback,
        })
    }
}

/// The two rules-table rows the compiler enforces.
///
/// Read from the table rather than written here, because they are tuning values
/// (item 90's "verifier and interpreter" rows) and AGENTS.md §12 says a tuning
/// value lives in the table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Limits {
    cooldown_min_ms: i32,
    max_fires: u32,
}

impl Limits {
    fn from_rules(rules: &RulesTable) -> Limits {
        let verifier = rules.message().verifier.as_ref();
        Limits {
            cooldown_min_ms: verifier.map_or(0, |block| block.handler_cooldown_min_ms),
            max_fires: verifier.map_or(0, |block| block.max_fires_max),
        }
    }
}

/// The label and id tables a compile resolves references against, and the map
/// a voxel has to fit inside.
struct Names<'a> {
    labels: &'a [gp::v1::Step],
    handlers: &'a [gp::v1::Handler],
    /// `map.size_*` in voxels, from the rules table.
    extent: [i32; 3],
}

impl Names<'_> {
    /// Refuse a voxel outside the map.
    ///
    /// A coordinate is a question about a place, and a place off the map is not
    /// a place. Without this check the executor's `i16` conversion would answer
    /// `x: 2147483647` with `x: 0` — a legal spot the commander then walks to
    /// and reports arriving at, which is the confident wrong answer AGENTS.md
    /// §12 exists to forbid. Clamping to the map edge would be the same wrong
    /// answer moved, so the playbook is refused at the door instead. The
    /// verifier says the same thing with a diagnostic and a JSON Pointer
    /// (`E0402`); this is the sim's own door, for a plan that reached it by
    /// another road.
    fn voxel(&self, voxel: &gp::v1::Voxel) -> Result<(), PlanError> {
        for (axis, value) in [(0_usize, voxel.x), (1, voxel.y), (2, voxel.z)] {
            let limit = self.extent.get(axis).copied().unwrap_or(0);
            if value < 0 || value >= limit {
                return Err(PlanError::VoxelOutOfMap {
                    axis: match axis {
                        0 => "x",
                        1 => "y",
                        _ => "z",
                    },
                    value,
                    limit,
                });
            }
        }
        Ok(())
    }

    /// The route index a label names.
    fn route_index(&self, label: &str) -> Option<u32> {
        if label.is_empty() {
            return None;
        }
        let at = self.labels.iter().position(|step| step.label == label)?;
        u32::try_from(at).ok()
    }

    /// The handler index an id names.
    fn handler_index(&self, id: &str) -> Option<u32> {
        if id.is_empty() {
            return None;
        }
        let at = self.handlers.iter().position(|handler| handler.id == id)?;
        u32::try_from(at).ok()
    }
}

/// Compile one step. `at` is its route index when it is a route step, and
/// `None` inside a handler body.
fn compile_step(
    step: &gp::v1::Step,
    names: &Names<'_>,
    at: Option<u32>,
) -> Result<PlanStep, PlanError> {
    if step.timeout_ms < 0 {
        return Err(PlanError::NegativeDuration("timeout_ms"));
    }
    let skip_if = match step.skip_if.as_ref() {
        Some(condition) => Some(compile_cond(condition, names, 0, &mut 0)?),
        None => None,
    };
    let on_fail = compile_on_fail(step.on_fail.as_ref(), names, at)?;
    let action = compile_action(step, names)?;
    if matches!(action, Action::WaitUntil(_)) && step.timeout_ms <= 0 {
        return Err(PlanError::WaitWithoutTimeout);
    }
    Ok(PlanStep {
        skip_if,
        timeout_ms: step.timeout_ms,
        on_fail,
        action,
    })
}

/// The default when a step names no `on_fail`.
///
/// PLACEHOLDER: the spec says "a step that times out fails, and `on_fail`
/// decides what happens next" and does not say what an absent `on_fail` means —
/// and proto3 has no presence on an enum, so an absent block and
/// `ON_FAIL_UNSPECIFIED` are the same bytes. `SKIP` is taken because spec
/// section 10's own reason for guards is that "steps degrade instead of
/// blocking", and a route that stops at its first failed step degrades into
/// nothing. Owner, at S3, with the rest of the guard vocabulary.
const DEFAULT_ON_FAIL: FailAction = FailAction::Skip;

fn compile_on_fail(
    on_fail: Option<&gp::v1::OnFail>,
    names: &Names<'_>,
    at: Option<u32>,
) -> Result<FailAction, PlanError> {
    let Some(block) = on_fail else {
        return Ok(DEFAULT_ON_FAIL);
    };
    match gp::v1::on_fail::Action::try_from(block.action) {
        Ok(gp::v1::on_fail::Action::Skip) => Ok(FailAction::Skip),
        Ok(gp::v1::on_fail::Action::AbortRoute) => Ok(FailAction::AbortRoute),
        Ok(gp::v1::on_fail::Action::JumpForward) => {
            let target = names
                .route_index(&block.jump_to_label)
                .ok_or_else(|| PlanError::UnknownLabel(block.jump_to_label.clone()))?;
            // A jump only ever goes forward. Inside a handler body `at` is
            // `None`: the body has no route index of its own, so the check that
            // can be made at compile time is made at run time instead, by
            // clamping the target forward of the cursor.
            if let Some(index) = at
                && target <= index
            {
                return Err(PlanError::BackwardJump {
                    from: index,
                    to: target,
                });
            }
            Ok(FailAction::JumpForward(target))
        }
        // `ON_FAIL_UNSPECIFIED` takes the documented default; a wire value this
        // build does not know is refused. The two are different questions —
        // "the author wrote nothing" and "the author wrote something newer than
        // this build" — and answering the second with the first is how Load
        // silently strips a construct (spec section 10).
        Ok(gp::v1::on_fail::Action::OnFailUnspecified) => Ok(DEFAULT_ON_FAIL),
        Err(_) => Err(PlanError::UnknownEnum {
            field: "on_fail.action",
            value: block.action,
        }),
    }
}

fn compile_action(step: &gp::v1::Step, names: &Names<'_>) -> Result<Action, PlanError> {
    let kind = step.kind.as_ref().ok_or(PlanError::StepWithoutAction)?;
    match kind {
        gp::v1::step::Kind::Move(move_step) => {
            let to = compile_place(move_step.to.as_ref(), names)?;
            let pace = gp::v1::move_step::Pace::try_from(move_step.pace).map_err(|_| {
                PlanError::UnknownEnum {
                    field: "move.pace",
                    value: move_step.pace,
                }
            })?;
            Ok(Action::Move {
                to,
                avoid_known_threats: matches!(pace, gp::v1::move_step::Pace::AvoidKnownThreats),
            })
        }
        gp::v1::step::Kind::Interface(interface) => {
            let beacon = compile_beacon_ref(interface.beacon.as_ref(), names)?;
            let mut rows: Vec<Row> = Vec::with_capacity(interface.rows.len());
            for row in &interface.rows {
                rows.push(compile_row(row, names)?);
            }
            Ok(Action::Interface { beacon, rows })
        }
        gp::v1::step::Kind::PlaceBeacon(place) => {
            let at = compile_place(place.at.as_ref(), names)?;
            let mut rows: Vec<Row> = Vec::new();
            let mut mandate = MandateKind::None;
            if let Some(initial) = place.initial.as_ref() {
                if let Some(settings) = initial.mandate.as_ref() {
                    let row = compile_settings_row(settings, names)?;
                    if let Row::Settings { kind, fields, .. } = &row {
                        mandate = kind.unwrap_or(MandateKind::None);
                        // Choosing the mandate type is free at place_beacon
                        // time (spec section 5); only the settings cost time,
                        // so a row with no settings fields is not filed at all.
                        if *fields > 0 {
                            rows.push(row);
                        }
                    }
                }
                if initial.priority != 0 {
                    let priority = compile_priority(initial.priority)?;
                    rows.push(Row::Priority { priority });
                }
            }
            let tags = u32::try_from(place.tags.len()).unwrap_or(u32::MAX);
            Ok(Action::PlaceBeacon {
                at,
                mandate,
                rows,
                tags,
            })
        }
        gp::v1::step::Kind::WaitUntil(wait) => {
            let condition = wait
                .condition
                .as_ref()
                .ok_or(PlanError::MissingBlock("wait_until.condition"))?;
            Ok(Action::WaitUntil(compile_cond(
                condition, names, 0, &mut 0,
            )?))
        }
        gp::v1::step::Kind::Hold(hold) => {
            if hold.ms < 0 {
                return Err(PlanError::NegativeDuration("hold.ms"));
            }
            Ok(Action::Hold { ms: hold.ms })
        }
        gp::v1::step::Kind::Broadcast(_) => Ok(Action::Broadcast),
    }
}

fn compile_row(row: &gp::v1::InterfaceRow, names: &Names<'_>) -> Result<Row, PlanError> {
    let inner = row.row.as_ref().ok_or(PlanError::MissingBlock("row"))?;
    match inner {
        gp::v1::interface_row::Row::SetMandate(settings) => {
            let kind = mandate_of(settings).ok_or(PlanError::UnsetEnum("set_mandate.mandate"))?;
            Ok(Row::Mandate { kind })
        }
        gp::v1::interface_row::Row::SetMandateSettings(settings) => {
            compile_settings_row(settings, names)
        }
        gp::v1::interface_row::Row::SetPriority(priority) => Ok(Row::Priority {
            priority: compile_priority(*priority)?,
        }),
        gp::v1::interface_row::Row::Recycle(_) => Ok(Row::Recycle),
        gp::v1::interface_row::Row::AddBuildTarget(add) => {
            let target = add
                .target
                .as_ref()
                .ok_or(PlanError::MissingBlock("add_build_target.target"))?;
            let (blueprint, anchor) = compile_build_target(target, names)?;
            Ok(Row::AddTarget { blueprint, anchor })
        }
        gp::v1::interface_row::Row::RemoveBuildTarget(remove) => Ok(Row::RemoveTarget {
            anchor: compile_voxel_place(remove.anchor.as_ref(), names)?,
        }),
        gp::v1::interface_row::Row::QueueStructure(_) => Err(PlanError::NotAtThisStage {
            construct: "interface row `queue_structure`",
            stage: "S4 (the capability catalogue and its licences)",
        }),
    }
}

/// A Quartermaster priority's wire value, refused when it is one this build
/// does not name.
///
/// An out-of-range value used to fall back to `0`, which is
/// `QUARTERMASTER_PRIORITY_UNSPECIFIED` — so a row this build could not read
/// still paid `set_priority_ms` and reported a commit, for a priority nobody
/// asked for.
fn compile_priority(priority: i32) -> Result<u8, PlanError> {
    match gp::v1::interface_row::QuartermasterPriority::try_from(priority) {
        Ok(known) => Ok(u8::try_from(i32::from(known)).unwrap_or(0)),
        Err(_) => Err(PlanError::UnknownEnum {
            field: "set_priority",
            value: priority,
        }),
    }
}

/// One `set_mandate_settings` row: the writ it names, and how many settings
/// fields it writes.
///
/// The field count is what spec section 5's "2 s + 0.5 s per extra field, max
/// 6 s" is over, so it is counted here, once, where the schema says which
/// fields there are.
fn compile_settings_row(
    settings: &gp::v1::MandateSettings,
    names: &Names<'_>,
) -> Result<Row, PlanError> {
    let mut fields: u32 = 0;
    if settings.roe != 0 {
        fields = fields.saturating_add(1);
    }
    if settings.retreat_hp_pct != 0 {
        fields = fields.saturating_add(1);
    }
    let mut targets: Vec<(u8, [i32; 3])> = Vec::new();
    let mut protected: Vec<([i32; 3], i32)> = Vec::new();
    let mut probes: Vec<([i32; 3], i32)> = Vec::new();
    let mut scouts: u32 = 0;
    if let Some(mandate) = settings.mandate.as_ref() {
        fields = fields.saturating_add(mandate_fields(mandate)?);
        match mandate {
            gp::v1::mandate_settings::Mandate::Build(build) => {
                for target in &build.targets {
                    targets.push(compile_build_target(target, names)?);
                }
                for area in &build.protected_areas {
                    protected.push(compile_area(area, names)?);
                }
            }
            gp::v1::mandate_settings::Mandate::Survey(survey) => {
                for area in &survey.probe_areas {
                    probes.push(compile_area(area, names)?);
                }
                scouts = survey.scout_count;
            }
            gp::v1::mandate_settings::Mandate::Mine(_)
            | gp::v1::mandate_settings::Mandate::Defend(_)
            | gp::v1::mandate_settings::Mandate::Attack(_) => {}
        }
    }
    Ok(Row::Settings {
        kind: mandate_of(settings),
        fields,
        targets,
        protected,
        probes,
        scouts,
    })
}

/// One Build target: its blueprint and its anchor.
///
/// PLACEHOLDER: the blueprint catalogue is spec section 8's contract and its
/// licences land at **S4**, so `blueprint_id` is a string in the schema and the
/// skeleton's minimal Build mandate ships exactly one blueprint, the Generator,
/// which is what the plan's wk-18.5 row names. Any other id is refused rather
/// than compiled to a structure this build cannot raise, so a playbook naming a
/// capability structure fails loudly at the seal instead of silently building
/// nothing (owner, at S4).
fn compile_build_target(
    target: &gp::v1::BuildTarget,
    names: &Names<'_>,
) -> Result<(u8, [i32; 3]), PlanError> {
    let blueprint = match target.blueprint_id.as_str() {
        "generator" => crate::tables::StructureKind::Generator.id(),
        _ => {
            return Err(PlanError::NotAtThisStage {
                construct: "a Build target naming a blueprint other than `generator`",
                stage: "S4 (the capability catalogue and its licences)",
            });
        }
    };
    Ok((
        blueprint,
        compile_voxel_place(target.anchor.as_ref(), names)?,
    ))
}

/// One area, as its centre and a radius in whole voxels.
///
/// The schema's `Area` is an axis-aligned box; the sim keeps a centre and a
/// radius because every test it does on an area is a range check, and a range
/// check in squared distance is the one form that needs no square root
/// (AGENTS.md section 4.2). The radius is the box's **half-diagonal in x and
/// y**, rounded up, so the circle covers the box rather than being covered by
/// it - over-covering a protected area protects a little more ground, which is
/// the safe direction.
fn compile_area(area: &gp::v1::Area, names: &Names<'_>) -> Result<([i32; 3], i32), PlanError> {
    let min = area
        .min
        .as_ref()
        .ok_or(PlanError::MissingBlock("area.min"))?;
    let max = area
        .max
        .as_ref()
        .ok_or(PlanError::MissingBlock("area.max"))?;
    names.voxel(min)?;
    names.voxel(max)?;
    let centre = [
        min.x.saturating_add(max.x).div_euclid(2),
        min.y.saturating_add(max.y).div_euclid(2),
        min.z.saturating_add(max.z).div_euclid(2),
    ];
    let half_x = max
        .x
        .saturating_sub(min.x)
        .abs()
        .saturating_add(1)
        .div_euclid(2);
    let half_y = max
        .y
        .saturating_sub(min.y)
        .abs()
        .saturating_add(1)
        .div_euclid(2);
    Ok((centre, half_x.max(half_y)))
}

/// A `Location` that has to be a fixed voxel.
///
/// A Build target's anchor and a removal's anchor are both written into a
/// beacon's settings and compared against later, so a late-bound selector would
/// have to resolve at commit time and then be compared against a selector that
/// resolved somewhere else. Refused rather than resolved: `playbook.proto` says
/// a removal names its anchor "rather than by an index because a playbook is
/// sealed before the round runs", and the same argument bars a target whose
/// place is not fixed either.
fn compile_voxel_place(
    place: Option<&gp::v1::Location>,
    names: &Names<'_>,
) -> Result<[i32; 3], PlanError> {
    match compile_place(place, names)? {
        Place::Voxel(at) => Ok(at),
        Place::Beacon(_) => Err(PlanError::NotAtThisStage {
            construct: "a Build target anchored on a selector rather than on a voxel",
            stage: "S3 (the full selector work, where a pinned anchor gets a meaning)",
        }),
    }
}

/// How many settings fields one mandate's own block writes.
///
/// Build's target list and Survey's probe areas are the two that need a stage
/// that does not exist, and both are refused rather than counted and dropped.
fn mandate_fields(mandate: &gp::v1::mandate_settings::Mandate) -> Result<u32, PlanError> {
    match mandate {
        gp::v1::mandate_settings::Mandate::Build(build) => {
            // Targets and protected areas are **lists**, and spec section 5
            // prices an edit by the number of fields it writes rather than by
            // the number of entries in a list: a target list is one field
            // however long it is, which is also what keeps a long list from
            // buying more interface time than the cap allows.
            let mut fields: u32 = 0;
            if !build.targets.is_empty() {
                fields = fields.saturating_add(1);
            }
            if !build.protected_areas.is_empty() {
                fields = fields.saturating_add(1);
            }
            if build.repair_threshold_pct != 0 {
                fields = fields.saturating_add(1);
            }
            if build.rebuild_destroyed {
                fields = fields.saturating_add(1);
            }
            if build.terraform != 0 {
                fields = fields.saturating_add(1);
            }
            Ok(fields)
        }
        gp::v1::mandate_settings::Mandate::Survey(survey) => {
            let mut fields = u32::from(survey.scout_count != 0);
            if !survey.probe_areas.is_empty() {
                fields = fields.saturating_add(1);
            }
            Ok(fields)
        }
        gp::v1::mandate_settings::Mandate::Mine(mine) => {
            let mut fields: u32 = 0;
            if mine.dig_max_depth != 0 {
                fields = fields.saturating_add(1);
            }
            if mine.flee_on_threat {
                fields = fields.saturating_add(1);
            }
            if mine.seam_choice != 0 {
                fields = fields.saturating_add(1);
            }
            if mine.pillar_spacing != 0 {
                fields = fields.saturating_add(1);
            }
            Ok(fields)
        }
        gp::v1::mandate_settings::Mandate::Defend(_) => Err(PlanError::NotAtThisStage {
            construct: "Defend mandate settings",
            stage: "S2 (destruction and combat)",
        }),
        gp::v1::mandate_settings::Mandate::Attack(_) => Err(PlanError::NotAtThisStage {
            construct: "Attack mandate settings",
            stage: "S2 (destruction and combat)",
        }),
    }
}

/// The writ a `MandateSettings` names, or `None` when it names none.
fn mandate_of(settings: &gp::v1::MandateSettings) -> Option<MandateKind> {
    match settings.mandate.as_ref()? {
        gp::v1::mandate_settings::Mandate::Build(_) => Some(MandateKind::Build),
        gp::v1::mandate_settings::Mandate::Defend(_) => Some(MandateKind::Defend),
        gp::v1::mandate_settings::Mandate::Attack(_) => Some(MandateKind::Attack),
        gp::v1::mandate_settings::Mandate::Survey(_) => Some(MandateKind::Survey),
        gp::v1::mandate_settings::Mandate::Mine(_) => Some(MandateKind::Mine),
    }
}

fn compile_place(place: Option<&gp::v1::Location>, names: &Names<'_>) -> Result<Place, PlanError> {
    let location = place.ok_or(PlanError::MissingBlock("location"))?;
    match location.place.as_ref() {
        Some(gp::v1::location::Place::Voxel(voxel)) => {
            names.voxel(voxel)?;
            Ok(Place::Voxel([voxel.x, voxel.y, voxel.z]))
        }
        Some(gp::v1::location::Place::BeaconAnchor(reference)) => {
            Ok(Place::Beacon(compile_beacon_ref(Some(reference), names)?))
        }
        Some(gp::v1::location::Place::Safest(_)) => Ok(Place::Beacon(BeaconSpec::Safest)),
        None => Err(PlanError::MissingBlock("location.place")),
    }
}

fn compile_beacon_ref(
    reference: Option<&gp::v1::BeaconRef>,
    _names: &Names<'_>,
) -> Result<BeaconSpec, PlanError> {
    let beacon = reference.ok_or(PlanError::MissingBlock("beacon"))?;
    match beacon.r#ref.as_ref() {
        Some(gp::v1::beacon_ref::Ref::BeaconId(id)) => Ok(BeaconSpec::Id(
            parse_beacon_id(id).ok_or_else(|| PlanError::UnknownBeacon(id.clone()))?,
        )),
        Some(gp::v1::beacon_ref::Ref::Safest(_)) => Ok(BeaconSpec::Safest),
        Some(gp::v1::beacon_ref::Ref::Nearest(nearest)) => Ok(BeaconSpec::Nearest(compile_filter(
            nearest.filter.as_ref(),
        )?)),
        Some(gp::v1::beacon_ref::Ref::Weakest(weakest)) => Ok(BeaconSpec::Weakest(compile_filter(
            weakest.filter.as_ref(),
        )?)),
        Some(gp::v1::beacon_ref::Ref::MostThreatened(_)) => Err(PlanError::NotAtThisStage {
            construct: "selector `most_threatened`",
            stage: "S2 (destruction and combat: incoming threat is what it ranks by)",
        }),
        None => Err(PlanError::MissingBlock("beacon.ref")),
    }
}

/// `b_NN` to a beacon id.
///
/// The spelling is [`BeaconId::playbook_id`]'s, and it is a contract between
/// three crates: the gateway builds the list, the verifier resolves against it,
/// and this parses it back.
fn parse_beacon_id(text: &str) -> Option<BeaconId> {
    let digits = text.strip_prefix("b_")?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u32>().ok().map(BeaconId::new)
}

fn compile_filter(filter: Option<&gp::v1::BeaconFilter>) -> Result<Filter, PlanError> {
    let Some(filter) = filter else {
        return Ok(Filter::default());
    };
    // `BeaconFilter` is the one message in `gp.v1` where an unset enum is not
    // an error: an unset `side` reads as OWN and an unset `mandate` means "any
    // writ" (the exception is named in playbook.proto's header).
    match gp::v1::beacon_filter::Side::try_from(filter.side) {
        Ok(gp::v1::beacon_filter::Side::Unspecified | gp::v1::beacon_filter::Side::Own) => {}
        Err(_) => {
            return Err(PlanError::UnknownEnum {
                field: "beacon_filter.side",
                value: filter.side,
            });
        }
        Ok(_) => {
            return Err(PlanError::NotAtThisStage {
                construct: "selector filter `side: ENEMY_KNOWN`",
                stage: "S3 (the seat's knowledge store, which is what an enemy beacon is known \
                        through)",
            });
        }
    }
    if !filter.tags.is_empty() {
        return Err(PlanError::NotAtThisStage {
            construct: "selector filter `tags`",
            stage: "S3 (a beacon has no author-tag column yet, so a tag filter cannot be answered \
                    — writing tags is legal, reading them is not)",
        });
    }
    let mandate = match gp::v1::beacon_filter::MandateKind::try_from(filter.mandate) {
        // Unspecified means "any writ" — the one documented default in the
        // package. A value this build does not know is **not** that: widening
        // the filter to "any" would answer a narrower question than the author
        // asked, so it is refused.
        Err(_) => {
            return Err(PlanError::UnknownEnum {
                field: "beacon_filter.mandate",
                value: filter.mandate,
            });
        }
        Ok(gp::v1::beacon_filter::MandateKind::Unspecified) => None,
        Ok(gp::v1::beacon_filter::MandateKind::Build) => Some(MandateKind::Build),
        Ok(gp::v1::beacon_filter::MandateKind::Defend) => Some(MandateKind::Defend),
        Ok(gp::v1::beacon_filter::MandateKind::Attack) => Some(MandateKind::Attack),
        Ok(gp::v1::beacon_filter::MandateKind::Survey) => Some(MandateKind::Survey),
        Ok(gp::v1::beacon_filter::MandateKind::Mine) => Some(MandateKind::Mine),
    };
    Ok(Filter { mandate })
}

fn compile_handler(
    handler: &gp::v1::Handler,
    names: &Names<'_>,
    limits: Limits,
) -> Result<Rule, PlanError> {
    let when = handler
        .when
        .as_ref()
        .ok_or(PlanError::MissingBlock("handler.when"))?;
    let when = compile_cond(when, names, 0, &mut 0)?;
    if handler.cooldown_ms < limits.cooldown_min_ms {
        return Err(PlanError::CooldownTooShort {
            found: handler.cooldown_ms,
            least: limits.cooldown_min_ms,
        });
    }
    if handler.max_fires == 0 || handler.max_fires > limits.max_fires {
        return Err(PlanError::FiresOutOfRange {
            found: handler.max_fires,
            most: limits.max_fires,
        });
    }
    let mut body: Vec<PlanStep> = Vec::with_capacity(handler.body.len());
    for step in &handler.body {
        body.push(compile_step(step, names, None)?);
    }
    let resume = match gp::v1::handler::Resume::try_from(handler.resume) {
        Ok(gp::v1::handler::Resume::Continue) => Resume::Continue,
        Ok(gp::v1::handler::Resume::SkipStep) => Resume::SkipStep,
        Ok(gp::v1::handler::Resume::EndRoute) => Resume::EndRoute,
        Ok(gp::v1::handler::Resume::JumpForward) => {
            let target = names
                .route_index(&handler.resume_at_label)
                .ok_or_else(|| PlanError::UnknownLabel(handler.resume_at_label.clone()))?;
            Resume::JumpForward(target)
        }
        Ok(gp::v1::handler::Resume::Unspecified) => {
            return Err(PlanError::UnsetEnum("handler.resume"));
        }
        Err(_) => {
            return Err(PlanError::UnknownEnum {
                field: "handler.resume",
                value: handler.resume,
            });
        }
    };
    Ok(Rule {
        when,
        body,
        resume,
        cooldown_ms: handler.cooldown_ms,
        max_fires: handler.max_fires,
    })
}

fn compile_fallback(fallback: &gp::v1::Fallback, names: &Names<'_>) -> Result<Posture, PlanError> {
    match fallback.posture.as_ref() {
        Some(gp::v1::fallback::Posture::Hold(hold)) => {
            Ok(Posture::Hold(compile_place(hold.at.as_ref(), names)?))
        }
        Some(gp::v1::fallback::Posture::Shadow(shadow)) => Ok(Posture::Shadow(compile_beacon_ref(
            shadow.beacon.as_ref(),
            names,
        )?)),
        Some(gp::v1::fallback::Posture::Patrol(patrol)) => {
            if patrol.waypoints.is_empty() {
                return Err(PlanError::MissingBlock("fallback.patrol.waypoints"));
            }
            let mut waypoints: Vec<Place> = Vec::with_capacity(patrol.waypoints.len());
            for waypoint in &patrol.waypoints {
                waypoints.push(compile_place(Some(waypoint), names)?);
            }
            Ok(Posture::Patrol(waypoints))
        }
        None => Err(PlanError::MissingBlock("fallback.posture")),
    }
}

/// Compile one condition, enforcing spec section 10's depth and node limits.
fn compile_cond(
    condition: &gp::v1::Condition,
    names: &Names<'_>,
    depth: u32,
    nodes: &mut u32,
) -> Result<Cond, PlanError> {
    if depth >= MAX_CONDITION_DEPTH {
        return Err(PlanError::ConditionTooDeep {
            limit: MAX_CONDITION_DEPTH,
        });
    }
    *nodes = nodes.saturating_add(1);
    if *nodes > MAX_CONDITION_NODES {
        return Err(PlanError::ConditionTooBig {
            limit: MAX_CONDITION_NODES,
        });
    }
    let node = condition
        .node
        .as_ref()
        .ok_or(PlanError::MissingBlock("condition.node"))?;
    match node {
        gp::v1::condition::Node::All(all) => {
            let mut items: Vec<Cond> = Vec::with_capacity(all.items.len());
            for item in &all.items {
                items.push(compile_cond(item, names, depth.saturating_add(1), nodes)?);
            }
            Ok(Cond::All(items))
        }
        gp::v1::condition::Node::Any(any) => {
            let mut items: Vec<Cond> = Vec::with_capacity(any.items.len());
            for item in &any.items {
                items.push(compile_cond(item, names, depth.saturating_add(1), nodes)?);
            }
            Ok(Cond::Any(items))
        }
        gp::v1::condition::Node::Not(not) => {
            let item = not
                .item
                .as_ref()
                .ok_or(PlanError::MissingBlock("not.item"))?;
            Ok(Cond::Not(Box::new(compile_cond(
                item,
                names,
                depth.saturating_add(1),
                nodes,
            )?)))
        }
        gp::v1::condition::Node::CmdrHpPct(predicate) => {
            let cmp = Cmp::of_cmp(predicate.cmp).ok_or(PlanError::UnsetEnum("cmdr_hp_pct.cmp"))?;
            Ok(Cond::CmdrHpPct(cmp, i64::from(predicate.pct)))
        }
        gp::v1::condition::Node::CmdrTookDamageWithin(predicate) => {
            if predicate.ms < 0 {
                return Err(PlanError::NegativeDuration("cmdr_took_damage_within.ms"));
            }
            Ok(Cond::CmdrTookDamageWithin(predicate.ms))
        }
        gp::v1::condition::Node::CmdrDeaths(predicate) => {
            let (cmp, value) = int_compare(predicate.deaths.as_ref(), "cmdr_deaths.deaths")?;
            Ok(Cond::CmdrDeaths(cmp, value))
        }
        gp::v1::condition::Node::SegmentElapsed(predicate) => {
            let (cmp, value) = int_compare(predicate.ms.as_ref(), "segment_elapsed.ms")?;
            Ok(Cond::SegmentElapsed(cmp, value))
        }
        gp::v1::condition::Node::BeaconHpPct(predicate) => {
            let beacon = compile_beacon_ref(predicate.beacon.as_ref(), names)?;
            let (cmp, value) = int_compare(predicate.pct.as_ref(), "beacon_hp_pct.pct")?;
            Ok(Cond::BeaconHpPct(beacon, cmp, value))
        }
        gp::v1::condition::Node::BeaconPowered(predicate) => {
            let beacon = compile_beacon_ref(predicate.beacon.as_ref(), names)?;
            Ok(Cond::BeaconPowered(beacon, predicate.powered))
        }
        gp::v1::condition::Node::BeaconUnderAttack(_) => Err(PlanError::NotAtThisStage {
            construct: "predicate `beacon_under_attack`",
            stage: "S2 (destruction and combat: a beacon has no damage history to ask about)",
        }),
        gp::v1::condition::Node::Treasury(predicate) => {
            let (cmp, value) = int_compare(predicate.dollars.as_ref(), "treasury.dollars")?;
            Ok(Cond::Treasury(cmp, value))
        }
        gp::v1::condition::Node::KwHeadroom(predicate) => {
            let (cmp, value) = int_compare(predicate.kw.as_ref(), "kw_headroom.kw")?;
            Ok(Cond::KwHeadroom(cmp, value))
        }
        gp::v1::condition::Node::StepReached(predicate) => {
            let index = names
                .route_index(&predicate.label)
                .ok_or_else(|| PlanError::UnknownLabel(predicate.label.clone()))?;
            Ok(Cond::StepReached(index))
        }
        gp::v1::condition::Node::RuleFired(predicate) => {
            let index = names
                .handler_index(&predicate.handler_id)
                .ok_or_else(|| PlanError::UnknownHandler(predicate.handler_id.clone()))?;
            Ok(Cond::RuleFired(index))
        }
    }
}

fn int_compare(
    compare: Option<&gp::v1::IntCompare>,
    field: &'static str,
) -> Result<(Cmp, i64), PlanError> {
    let compare = compare.ok_or(PlanError::MissingBlock(field))?;
    let cmp = Cmp::of_op(compare.op).ok_or(PlanError::UnsetEnum(field))?;
    Ok((cmp, i64::from(compare.value)))
}

/// Why a playbook cannot be executed by this build.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PlanError {
    /// A block the interpreter reads is absent. Proto3 cannot tell an absent
    /// message from an empty one, so the interpreter refuses rather than
    /// substituting a default.
    MissingBlock(&'static str),
    /// An enum the package says is a sentinel arrived unset. An unset enum is
    /// an error, never a default (`playbook.proto`'s header).
    UnsetEnum(&'static str),
    /// An enum carried a wire value this build does not name.
    ///
    /// Told apart from [`PlanError::UnsetEnum`] on purpose: "the author wrote
    /// nothing" and "the author wrote something this build is too old to read"
    /// are different failures, and answering the second with a default is
    /// exactly the silent strip spec section 10's "Load never strips" forbids.
    UnknownEnum {
        /// Which field.
        field: &'static str,
        /// The wire value that arrived.
        value: i32,
    },
    /// A step names no action at all.
    StepWithoutAction,
    /// A duration is negative. The verifier rejects one with a code and a JSON
    /// Pointer; this is the sim's own door (item 46).
    NegativeDuration(&'static str),
    /// A `wait_until` carries no positive `timeout_ms`. Every wait has a
    /// timeout: that is half of why every playbook halts.
    WaitWithoutTimeout,
    /// A jump names a step that is not strictly later in the route. A jump only
    /// ever goes forward: that is the other half.
    BackwardJump {
        /// The route index the jump is written on.
        from: u32,
        /// The route index it names.
        to: u32,
    },
    /// A voxel names a place outside the map (`map.size_*`).
    VoxelOutOfMap {
        /// `"x"`, `"y"` or `"z"`.
        axis: &'static str,
        /// What the playbook wrote.
        value: i32,
        /// The map's size on that axis; legal values are `0` to `limit - 1`.
        limit: i32,
    },
    /// A label names no step in this playbook's route.
    UnknownLabel(String),
    /// A handler id names no handler in this playbook.
    UnknownHandler(String),
    /// A `beacon_id` is not of the `b_NN` form the three crates agree on.
    UnknownBeacon(String),
    /// A handler's cooldown is below `verifier.handler_cooldown_min_ms`.
    CooldownTooShort {
        /// What the playbook said.
        found: i32,
        /// What the rules table requires.
        least: i32,
    },
    /// A handler's `max_fires` is outside 1 to `verifier.max_fires_max`.
    FiresOutOfRange {
        /// What the playbook said.
        found: u32,
        /// What the rules table allows.
        most: u32,
    },
    /// A condition tree nests deeper than spec section 10 allows.
    ConditionTooDeep {
        /// The limit.
        limit: u32,
    },
    /// A condition tree holds more nodes than spec section 10 allows.
    ConditionTooBig {
        /// The limit.
        limit: u32,
    },
    /// The route holds more steps than the interpreter's own ceiling.
    TooManySteps {
        /// What the playbook holds.
        found: usize,
        /// [`MAX_ROUTE_STEPS`].
        limit: usize,
    },
    /// The playbook holds more handlers than the interpreter's own ceiling.
    TooManyHandlers {
        /// What the playbook holds.
        found: usize,
        /// [`MAX_HANDLERS`].
        limit: usize,
    },
    /// A construct in the v1 vocabulary whose **effect** needs a stage this
    /// build does not have.
    ///
    /// Refused at seal time and never treated as true, false or a no-op
    /// (AGENTS.md §12: don't invent rules). The full list, each naming the task
    /// or stage that fills it:
    ///
    /// | Construct | Stage |
    /// |---|---|
    /// | `queue_structure` | S4 (capability licences) |
    /// | a Build target naming a blueprint other than `generator` | S4 |
    /// | a Build target anchored on a selector rather than on a voxel | S3 |
    /// | Defend and Attack mandate settings | S2 |
    /// | selector `most_threatened` | S2 |
    /// | selector filter `side: ENEMY_KNOWN` | S3 |
    /// | selector filter `tags` | S3 |
    /// | predicate `beacon_under_attack` | S2 |
    NotAtThisStage {
        /// What was written.
        construct: &'static str,
        /// The stage that gives it an effect.
        stage: &'static str,
    },
}

impl core::fmt::Display for PlanError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PlanError::MissingBlock(name) => write!(
                f,
                "the `{name}` block is missing; the interpreter reads it and will not substitute a \
                 default"
            ),
            PlanError::UnsetEnum(name) => {
                write!(f, "`{name}` is unset, and an unset enum is an error")
            }
            PlanError::UnknownEnum { field, value } => write!(
                f,
                "`{field}` carries {value}, which this build does not name; a value it cannot read \
                 is refused rather than defaulted"
            ),
            PlanError::StepWithoutAction => write!(f, "a step names no action"),
            PlanError::NegativeDuration(name) => {
                write!(f, "`{name}` is negative; durations are game milliseconds")
            }
            PlanError::WaitWithoutTimeout => write!(
                f,
                "a `wait_until` carries no timeout; every wait has one, which is half of why every \
                 playbook halts"
            ),
            PlanError::BackwardJump { from, to } => write!(
                f,
                "a jump from route step {from} names step {to}; a jump only ever goes forward"
            ),
            PlanError::VoxelOutOfMap { axis, value, limit } => write!(
                f,
                "a voxel's {axis} of {value} is outside the map, which runs 0 to {}",
                limit.saturating_sub(1)
            ),
            PlanError::UnknownLabel(label) => {
                write!(f, "`{label}` names no step in this playbook's route")
            }
            PlanError::UnknownHandler(id) => write!(f, "`{id}` names no handler in this playbook"),
            PlanError::UnknownBeacon(id) => {
                write!(f, "`{id}` is not a beacon reference of the `b_NN` form")
            }
            PlanError::CooldownTooShort { found, least } => write!(
                f,
                "a handler's cooldown of {found} ms is below the {least} ms the rules table sets"
            ),
            PlanError::FiresOutOfRange { found, most } => write!(
                f,
                "a handler's `max_fires` of {found} is outside 1 to {most}"
            ),
            PlanError::ConditionTooDeep { limit } => {
                write!(f, "a condition tree nests deeper than {limit}")
            }
            PlanError::ConditionTooBig { limit } => {
                write!(f, "a condition tree holds more than {limit} nodes")
            }
            PlanError::TooManySteps { found, limit } => {
                write!(f, "the route holds {found} steps; the ceiling is {limit}")
            }
            PlanError::TooManyHandlers { found, limit } => {
                write!(
                    f,
                    "the playbook holds {found} handlers; the ceiling is {limit}"
                )
            }
            PlanError::NotAtThisStage { construct, stage } => write!(
                f,
                "{construct} is in the v1 vocabulary but has no effect until {stage}; it is \
                 refused rather than silently ignored"
            ),
        }
    }
}

impl std::error::Error for PlanError {}

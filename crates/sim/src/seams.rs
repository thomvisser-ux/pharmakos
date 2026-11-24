// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The in-code seams spec section 15 ("Seams kept for later") names.
//!
//! Reserved, shaped, and **not read**. The v1 rule (AGENTS.md §2) is that
//! everything which executes is Rust and everything the player touches is data;
//! these seams are what lets v1.1 publish the gateway and v1.2 open the script
//! arm without reshaping the sim underneath them.
//!
//! What is here:
//!
//! * [`Operator`] — local view, mailbox and mandate in; intents out. The
//!   built-in operator (`pharmakos-operator`, T18) is *an ordinary client of
//!   this shape*, with no privileged reads (AGENTS.md §3 rule 3).
//! * [`WorkCounter`] — the abstract per-tick work counter that becomes fuel
//!   later.
//! * [`BeaconMandate`] — the `program_id` a beacon's mandate carries.
//!
//! What is deliberately **not** here: a `Quartermaster` trait (T14's, with the
//! single-treasury stub it exists to hold), and anything resembling a script
//! runtime (AGENTS.md §11).

use crate::math::quantity::Tick;
use crate::tables::{SeatId, UnitId};

/// The abstract unit of work a tick may spend.
///
/// Today it counts and nothing reads the count. Later it becomes fuel: a seat's
/// operator is budgeted in **evaluation units, never wall time** (AGENTS.md
/// §4.7), because a budget measured by a clock is a budget that differs between
/// a fast machine and a slow one, which is a desync with extra steps.
///
/// PLACEHOLDER: whether the work counter becomes hashed state — owner, at S5, with the operator's budget.
/// The stage that gives it teeth settles it. It is
/// **not** hashed today, and `tests/determinism.rs`'s
/// `the_work_counter_is_not_in_the_state_encoding` asserts that, because a
/// counter that affects nothing must not move a golden file.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WorkCounter {
    spent: u32,
    budget: u32,
}

/// What a [`WorkCounter`] says when the budget is gone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OutOfWork {
    /// The budget that was exhausted.
    pub budget: u32,
    /// What was already spent when the charge arrived.
    pub spent: u32,
    /// What the charge asked for.
    pub wanted: u32,
}

impl WorkCounter {
    /// A counter with `budget` units available this tick.
    #[must_use]
    pub const fn with_budget(budget: u32) -> WorkCounter {
        WorkCounter { spent: 0, budget }
    }

    /// Spend `units`, or report that the budget is gone.
    ///
    /// # Errors
    ///
    /// Returns [`OutOfWork`] when the charge would take the total past the
    /// budget. Nothing is spent in that case.
    pub const fn charge(&mut self, units: u32) -> Result<(), OutOfWork> {
        match self.spent.checked_add(units) {
            Some(total) if total <= self.budget => {
                self.spent = total;
                Ok(())
            }
            _ => Err(OutOfWork {
                budget: self.budget,
                spent: self.spent,
                wanted: units,
            }),
        }
    }

    /// What has been spent this tick.
    #[must_use]
    pub const fn spent(self) -> u32 {
        self.spent
    }

    /// What the budget is.
    #[must_use]
    pub const fn budget(self) -> u32 {
        self.budget
    }

    /// Start a fresh tick with the same budget.
    pub const fn reset(&mut self) {
        self.spent = 0;
    }
}

/// A running count of evaluation units that a **read-only** path can add to:
/// what a decision's counted work is measured in (P1's counted half, S1's
/// plan section 5; decisions-log item 33 (c)).
///
/// **One unit is one primitive step of counted work, charged where the step
/// happens** (lane `dunit` and its part 2, decisions-log item 175): **one a
/// visit of each loop a decision runs, at every level down to the
/// innermost** -- a handler tried, a condition node evaluated, a selector
/// candidate ranked, a feature row, a footprint column, a voxel of a seam's
/// depth or a column's fall, a cell of a pit-safe flood, a beacon, unit,
/// structure, target, sighting or edit row a scan reads (up to the row that
/// answers, for a scan that stops there), a Spire test's kind byte or
/// structure row, a ranker entry, a site column, a sphere an `h` folds
/// over -- and **a node a search pops**: the grid and endpoint pops and the
/// abstract pops of a served walk (`targeting::Ground::travel`) and the grid
/// optimum's pops of a `toward` (`targeting::stone_site`), each charged as
/// the change in the search scratch's own counters over the call. A call
/// that visits nothing charges nothing: there is no flat charge per call. A
/// read by index (an id's row, a column's node) is no loop and is not
/// charged, nor are the standard library's sorts and moves. A unit is a
/// count, not a cost model: no step is weighted against another, so what a
/// unit costs in nanoseconds per kind of step is the bench's to measure.
///
/// The tally behind [`WorkCounter`]'s per-decision figure. It exists because
/// a condition is evaluated over a `&World`, and counting there needs interior
/// mutability; it is atomics, with relaxed ordering, only so that a `World`
/// holding one stays `Sync`. Nothing reads it from another thread, and every
/// count is made in the tick's own fixed order.
///
/// It counts in `u64` by a checked add and never saturates: a sum that would
/// pass `u64::MAX`, or a charge whose source count could not be read
/// ([`UnitTally::add_measured`]), leaves the tally **unmeasured** under an
/// enumerated reason ([`Unmeasured`]), which [`UnitTally::read`] answers in
/// place of a number until the next reset. The first reason sticks.
///
/// **It is not state**: never hashed, never saved, and it compares equal to
/// any other tally, so two worlds that differ only in what was last measured
/// are the same world. `tests/determinism.rs` keeps it out of the encoding.
#[derive(Debug, Default)]
pub struct UnitTally {
    count: core::sync::atomic::AtomicU64,
    /// [`Unmeasured::id`] of the first reason the count stopped being
    /// measured, or [`MEASURED`] while it is a count.
    fault: core::sync::atomic::AtomicU8,
    /// Whether the next charge is to leave the tally unmeasured whatever it
    /// is ([`UnitTally::fault_next_charge`]). A reset keeps it.
    armed: core::sync::atomic::AtomicBool,
}

/// The [`UnitTally::fault`] byte of a tally that still holds a count.
const MEASURED: u8 = 0;

/// Why a [`UnitTally`] holds no count since its last reset.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unmeasured {
    /// The units counted passed `u64::MAX`.
    Overflowed,
    /// A charge's source count, a search scratch's pop counter, could not be
    /// read: it had itself passed `u64::MAX`, or it read lower after the
    /// call than before it.
    SourceUnreadable,
}

impl Unmeasured {
    /// The reason's byte in [`UnitTally`]; never [`MEASURED`].
    const fn id(self) -> u8 {
        match self {
            Unmeasured::Overflowed => 1,
            Unmeasured::SourceUnreadable => 2,
        }
    }

    /// The reason a fault byte names; `None` for [`MEASURED`].
    const fn from_id(id: u8) -> Option<Unmeasured> {
        match id {
            1 => Some(Unmeasured::Overflowed),
            2 => Some(Unmeasured::SourceUnreadable),
            _ => None,
        }
    }
}

impl core::fmt::Display for Unmeasured {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Unmeasured::Overflowed => "the decision units counted passed u64::MAX",
            Unmeasured::SourceUnreadable => {
                "a search's pop counter could not be read for a decision-unit charge"
            }
        })
    }
}

impl std::error::Error for Unmeasured {}

/// **A test seam: the sim never arms it.** Whether a test asked that the
/// next decision's tally be left unmeasured at its first charge
/// ([`crate::world::World::fault_next_decision_charge`]): the world keeps
/// this, and no tally, between decisions, so nothing outside a decision has a
/// tally it could charge. Compares equal to every other, as a measurement
/// does ([`UnitTally`]).
#[doc(hidden)]
#[derive(Debug, Default)]
pub struct FaultSeam(core::sync::atomic::AtomicBool);

impl FaultSeam {
    /// Arm it.
    pub fn arm(&self) {
        self.0.store(true, core::sync::atomic::Ordering::Relaxed);
    }

    /// Whether it was armed, disarming it.
    pub fn take(&self) -> bool {
        self.0.swap(false, core::sync::atomic::Ordering::Relaxed)
    }
}

impl Clone for FaultSeam {
    fn clone(&self) -> FaultSeam {
        FaultSeam(core::sync::atomic::AtomicBool::new(
            self.0.load(core::sync::atomic::Ordering::Relaxed),
        ))
    }
}

impl PartialEq for FaultSeam {
    fn eq(&self, _other: &FaultSeam) -> bool {
        true
    }
}

impl Eq for FaultSeam {}

/// Why a seat's published decision figure ([`crate::world::World::decision_work`])
/// holds no count (dunit part 2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoFigure {
    /// No decision has run since this world was made or restored: there is
    /// nothing to report yet, which is not a count of nothing.
    NoDecisionYet,
    /// The last decision ran, and its tally stopped being a count.
    Unmeasured(Unmeasured),
    /// The seat is out of the match: its decision returns before it decides
    /// anything, so it made none. Its seam upkeep still runs, as it always
    /// has, and what that reads is not a decision's figure.
    Eliminated,
}

impl core::fmt::Display for NoFigure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            NoFigure::NoDecisionYet => {
                f.write_str("no decision has run since the world was made or restored")
            }
            NoFigure::Unmeasured(reason) => reason.fmt(f),
            NoFigure::Eliminated => f.write_str("the seat is eliminated, so it made no decision"),
        }
    }
}

impl std::error::Error for NoFigure {}

impl UnitTally {
    /// A tally at zero.
    #[must_use]
    pub const fn new() -> UnitTally {
        UnitTally {
            count: core::sync::atomic::AtomicU64::new(0),
            fault: core::sync::atomic::AtomicU8::new(MEASURED),
            armed: core::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Add `units`.
    pub fn add(&self, units: u32) {
        self.add_count(u64::from(units));
    }

    /// Add `units`, a count of primitive steps a search reports in `u64`.
    /// A sum past `u64::MAX` leaves the tally [`Unmeasured::Overflowed`].
    pub fn add_count(&self, units: u64) {
        if self.fire_armed() {
            return;
        }
        let now = self.count.load(core::sync::atomic::Ordering::Relaxed);
        match now.checked_add(units) {
            Some(next) => self
                .count
                .store(next, core::sync::atomic::Ordering::Relaxed),
            None => self.fail(Unmeasured::Overflowed),
        }
    }

    /// Add a measured count, or record why it could not be measured.
    pub fn add_measured(&self, units: Result<u64, Unmeasured>) {
        if self.fire_armed() {
            return;
        }
        match units {
            Ok(units) => self.add_count(units),
            Err(reason) => self.fail(reason),
        }
    }

    /// Record that the count is no longer measured; the first reason sticks.
    fn fail(&self, reason: Unmeasured) {
        if self.fault.load(core::sync::atomic::Ordering::Relaxed) == MEASURED {
            self.fault
                .store(reason.id(), core::sync::atomic::Ordering::Relaxed);
        }
    }

    /// What has been counted since the last reset.
    ///
    /// # Errors
    ///
    /// The [`Unmeasured`] reason, once the count stopped being a count.
    pub fn read(&self) -> Result<u64, Unmeasured> {
        match Unmeasured::from_id(self.fault.load(core::sync::atomic::Ordering::Relaxed)) {
            Some(reason) => Err(reason),
            None => Ok(self.count.load(core::sync::atomic::Ordering::Relaxed)),
        }
    }

    /// **A test seam: the sim never calls it.** Leave the tally
    /// [`Unmeasured::Overflowed`] at its next charge, whatever that charge
    /// is, even after a [`UnitTally::reset`] in between, so a test can fault
    /// the tally inside a decision (after the decision's own reset) and show
    /// that the match plays the same (`tests/nearest.rs`'s
    /// `decision_units_never_reach_the_state_hash`).
    #[doc(hidden)]
    pub fn fault_next_charge(&self) {
        self.armed
            .store(true, core::sync::atomic::Ordering::Relaxed);
    }

    /// Whether an armed fault fired on this charge: it leaves the tally
    /// unmeasured and disarms.
    fn fire_armed(&self) -> bool {
        // A plain load on every charge; the swap only once a test armed it.
        if self.armed.load(core::sync::atomic::Ordering::Relaxed)
            && self
                .armed
                .swap(false, core::sync::atomic::Ordering::Relaxed)
        {
            self.fail(Unmeasured::Overflowed);
            return true;
        }
        false
    }

    /// Start again from zero, measured. An armed fault stays armed.
    pub fn reset(&self) {
        self.count.store(0, core::sync::atomic::Ordering::Relaxed);
        self.fault
            .store(MEASURED, core::sync::atomic::Ordering::Relaxed);
    }
}

/// Where a read-only scan reports the counted work it does, so that the
/// same scan is charged when a decision runs it and costs nothing to count
/// when anything else does: the rows it reads ([`Meter::rows`]), and units of
/// work measured another way, such as a search's pops, which can be
/// unreadable ([`Meter::measured`]).
///
/// A decision's caller passes the world's decision tally
/// ([`UnitTally`], one unit a row read, and each measured unit); every other
/// caller passes `()`, which counts nothing. A meter never changes what the
/// scan decides or returns: it only hears counts.
pub trait Meter: core::fmt::Debug {
    /// `rows` more rows read.
    fn rows(&self, rows: usize);

    /// `units` more units of counted work, or the reason they could not be
    /// counted: what a search's pops are charged as, the change in its
    /// scratch's counters over the call, which can be unreadable.
    fn measured(&self, units: Result<u64, Unmeasured>);
}

/// Counts nothing: a scan run outside a decision.
impl Meter for () {
    fn rows(&self, _rows: usize) {}

    fn measured(&self, _units: Result<u64, Unmeasured>) {}
}

/// One unit a row read; a row count past `u64` leaves the tally
/// [`Unmeasured::Overflowed`].
impl Meter for UnitTally {
    fn rows(&self, rows: usize) {
        self.add_measured(u64::try_from(rows).map_err(|_| Unmeasured::Overflowed));
    }

    fn measured(&self, units: Result<u64, Unmeasured>) {
        self.add_measured(units);
    }
}

impl Clone for UnitTally {
    fn clone(&self) -> UnitTally {
        UnitTally {
            count: core::sync::atomic::AtomicU64::new(
                self.count.load(core::sync::atomic::Ordering::Relaxed),
            ),
            fault: core::sync::atomic::AtomicU8::new(
                self.fault.load(core::sync::atomic::Ordering::Relaxed),
            ),
            armed: core::sync::atomic::AtomicBool::new(
                self.armed.load(core::sync::atomic::Ordering::Relaxed),
            ),
        }
    }
}

/// Every tally equals every other: a measurement is not state (see
/// [`UnitTally`]).
impl PartialEq for UnitTally {
    fn eq(&self, _other: &UnitTally) -> bool {
        true
    }
}

impl Eq for UnitTally {}

/// The identity of a built-in program.
///
/// `program_id` sits on a beacon's mandate so that the `program {builtin|script}`
/// oneof in `gp.v1` has somewhere to land when the script arm opens. Only the
/// `builtin` arm exists, and nothing reads the seam (AGENTS.md §2).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct ProgramId(u32);

impl ProgramId {
    /// No program: the mandate runs its own default behaviour.
    pub const NONE: ProgramId = ProgramId(0);

    /// Build from a raw id.
    #[must_use]
    pub const fn new(raw: u32) -> ProgramId {
        ProgramId(raw)
    }

    /// The raw id, for the canonical encoder.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// The five mandates a beacon can carry (spec section 6).
///
/// Build and Survey are the skeleton's (T14); Defend, Attack and Mine are S2's
/// and later. The enum is complete now because its discriminants reach the
/// canonical encoding, and adding a variant in the middle later would move
/// every golden file.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum MandateKind {
    /// No mandate set.
    #[default]
    None,
    /// Build.
    Build,
    /// Defend.
    Defend,
    /// Attack.
    Attack,
    /// Mine.
    Mine,
    /// Survey.
    Survey,
}

impl MandateKind {
    /// The kind a wire id names, or `None` for one this build does not define.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<MandateKind> {
        match id {
            0 => Some(MandateKind::None),
            1 => Some(MandateKind::Build),
            2 => Some(MandateKind::Defend),
            3 => Some(MandateKind::Attack),
            4 => Some(MandateKind::Mine),
            5 => Some(MandateKind::Survey),
            _ => None,
        }
    }

    /// The wire id. Additive only: never reuse, never renumber.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            MandateKind::None => 0,
            MandateKind::Build => 1,
            MandateKind::Defend => 2,
            MandateKind::Attack => 3,
            MandateKind::Mine => 4,
            MandateKind::Survey => 5,
        }
    }
}

/// A beacon's mandate, with the `program_id` seam attached.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BeaconMandate {
    /// Which of the five mandates the beacon carries.
    pub kind: MandateKind,
    /// The built-in program the mandate runs. Reserved seam; see [`ProgramId`].
    pub program_id: ProgramId,
}

/// One instruction a seat's operator hands back to the sim.
///
/// Deliberately thin, and not read: T11 built the playbook interpreter without
/// intents ([`crate::interpreter`]'s decision writes its own seat's commander,
/// beacons and state directly, in seat order), so this is the reserved shape of
/// the seam and nothing more.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Intent {
    /// Which seat issued it. Ties break to the lowest seat id.
    pub seat: SeatId,
    /// Which unit it addresses.
    pub unit: UnitId,
    /// The tick it was issued on.
    pub issued: Tick,
    /// Not read. T11 needed no intent vocabulary (see the type's doc), so an
    /// intent carries only its addressing; a vocabulary is the script arm's to
    /// define when it opens (AGENTS.md section 2; register X-10, reworded by
    /// S1's `fixs` lane).
    pub kind: u16,
}

/// A fixed-capacity sink for intents.
///
/// Fixed capacity because nothing in a tick allocates (G3′ §9.17). A sink that
/// is full drops nothing silently: [`IntentSink::push`] says so.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IntentSink {
    intents: Vec<Intent>,
    capacity: usize,
}

impl IntentSink {
    /// A sink that will hold `capacity` intents and never grow.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> IntentSink {
        IntentSink {
            intents: Vec::with_capacity(capacity),
            capacity,
        }
    }

    /// Offer an intent. Returns `false` when the sink is full.
    pub fn push(&mut self, intent: Intent) -> bool {
        if self.intents.len() >= self.capacity {
            return false;
        }
        self.intents.push(intent);
        true
    }

    /// Everything offered this tick, in the order it was offered. The caller
    /// sorts before applying — every sort key ends in a unique id (item 62).
    #[must_use]
    pub fn as_slice(&self) -> &[Intent] {
        &self.intents
    }

    /// Empty the sink, keeping the allocation.
    pub fn clear(&mut self) {
        self.intents.clear();
    }
}

/// The messages a seat may read. Radio carries data and never control, and is
/// treated as untrusted input (AGENTS.md §1).
///
/// PLACEHOLDER: the message vocabulary — owner, at S4, with the Radio Mast.
/// The message vocabulary arrives at S4 with the Radio Mast. The
/// type exists now because the trait below takes it, and adding a parameter to
/// a trait later is a bigger change than filling a struct.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mailbox {
    /// How many messages are waiting. Always zero in v1's skeleton.
    pub pending: u32,
}

/// What a seat's operator can see — its own fog-limited view.
///
/// The concrete knowledge types are [`crate::knowledge`]; this is the borrow
/// the trait hands out, so that an implementation cannot hold on to the world.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LocalView<'a> {
    /// The seat this view belongs to.
    pub seat: SeatId,
    /// The tick the view was taken at.
    pub tick: Tick,
    /// What the seat knows.
    pub knowledge: &'a crate::knowledge::SeatKnowledge,
}

/// The seam the **playbook interpreter** plugs into, and the one thing T10
/// leaves for T11.
///
/// [`crate::world::Phase::Decision`] is the tick phase that calls it, once per
/// `match.decision_tick_ms` of game time — 250 ms, which is exactly five ticks
/// at 20 Hz, so no decision lands between ticks. Everything about what a
/// decision *is* belongs to T11 (the v1 vocabulary: route steps, handlers,
/// selectors, the fixed 20 % reflex, `on_death` and `fallback`), and **none of
/// it is built here**: this trait exists so that the runner, the phases and the
/// event bus can be written against a stable shape, and so that the tick's
/// order does not move when the interpreter arrives.
///
/// Two rules bind it, both inherited from [`Operator`] and from AGENTS.md §2:
///
/// * **No script runtime.** A playbook is data with a closed vocabulary; an
///   implementation of this trait is Rust, compiled in, and reads that data.
/// * **No clock.** A budget is [`WorkCounter`] units, never wall time.
///
/// The parameter list is the shape [`Operator`] already has, because a
/// decision needs the same three things: a fog-limited view, the mandate in
/// force, and somewhere to put intents. **T11 did not use it**: the built-in
/// interpreter is [`crate::interpreter`], which the decision phase calls
/// directly, so nothing implements this trait and nothing calls it. It stays
/// as the reserved seam spec section 15 keeps for later; reshaping or deleting
/// it is the call of the stage that opens the script arm (AGENTS.md section 2;
/// register X-11, reworded by S1's `fixs` lane).
pub trait PlaybookInterpreter {
    /// Run one decision tick for one seat.
    ///
    /// # Errors
    ///
    /// Returns [`OutOfWork`] when the tick's work budget runs out mid-decision.
    /// The intents already pushed stand, for the reason [`Operator::decide`]'s
    /// do: a budget is a stop, not a rollback.
    fn decide(
        &mut self,
        view: &LocalView<'_>,
        mandate: BeaconMandate,
        work: &mut WorkCounter,
        out: &mut IntentSink,
    ) -> Result<(), OutOfWork>;
}

/// The seam the operator arm hangs off (spec section 15, "Seams kept for
/// later").
///
/// Local view, mailbox and mandate in; intents out. Two rules bind every
/// implementation and are what keep the seam honest:
///
/// * **No privileged reads.** An implementation sees the same snapshot every
///   other seat sees, and must not name a sim-internal type (AGENTS.md §3
///   rule 3, asserted by `pharmakos-operator`'s own source-text test at T18).
/// * **No clock.** A budget is measured in evaluation units through
///   [`WorkCounter`], never in wall time.
pub trait Operator {
    /// Decide what to do this tick.
    ///
    /// # Errors
    ///
    /// Returns [`OutOfWork`] when the tick's work budget runs out mid-decision.
    /// The intents already pushed stand: a budget is a stop, not a rollback.
    fn decide(
        &mut self,
        view: &LocalView<'_>,
        mailbox: Mailbox,
        mandate: BeaconMandate,
        work: &mut WorkCounter,
        out: &mut IntentSink,
    ) -> Result<(), OutOfWork>;
}

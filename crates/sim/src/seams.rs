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
/// PLACEHOLDER: whether the counter becomes hashed state is settled by the
/// stage that gives it teeth (owner, at S5 with the operator's budget). It is
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
/// Deliberately thin: the vocabulary is T11's, and filling it here would be
/// guessing at the interpreter's shape (AGENTS.md §12, "don't invent rules").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Intent {
    /// Which seat issued it. Ties break to the lowest seat id.
    pub seat: SeatId,
    /// Which unit it addresses.
    pub unit: UnitId,
    /// The tick it was issued on.
    pub issued: Tick,
    /// PLACEHOLDER: the intent vocabulary is T11's (the playbook interpreter at
    /// the v1 vocabulary). Until then an intent carries only its addressing, so
    /// that the trait below can be written against a stable shape.
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
/// PLACEHOLDER: the message vocabulary arrives at S4 with the Radio Mast. The
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

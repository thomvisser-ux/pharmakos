// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stage 5, **estimate**: the first half of FULL. **Present and empty.**
//!
//! Decisions-log item 82 settles why it exists before it does anything:
//! `verify_plan{depth}` and `report_hash` ship complete from day one, with
//! FULL's two stages present and empty, so **the API shape and the hash contract
//! never change** when S1 and S3 fill them. `submit_plan` runs FULL exactly as
//! spec section 12 says, and gets an honest, short report. The cost was written
//! down in advance: FULL is briefly a promise rather than a depth, and the
//! `report_hash` goldens move once when the stages fill — a movement to be
//! explained in the pull request that causes it, not a surprise.
//!
//! # What belongs here, and who brings it
//!
//! The allowed estimates of spec section 11, and nothing else:
//!
//! | Work | Brought by |
//! |---|---|
//! | Pathfinder travel over known terrain, fogged legs priced `cost * 3 / 2` | **T7**, whose `estimate(...) -> Option<Estimate>` signature item 61 has already frozen |
//! | Interface-time arithmetic at the section 5 rates | **T8** (`plan-core`), which owns the arithmetic; this stage calls it |
//! | `$` and `kW` projection — the Quartermaster's rules | **S1**, with the economy: `E0601`, `W0601`, `W0602` |
//! | Whether the route fits the coming segment | **S1**: `W0701`, `I0001` |
//! | Mast coverage for a broadcast | **S4**, with radio: `E0602` |
//!
//! # What may never happen here
//!
//! Stepping the sim. Running mandates, programs, combat or construction.
//! Modelling an enemy. Evaluating a rule condition over a projected future. The
//! verifier estimates; it does not rehearse (AGENTS.md section 3 rule 2).
//!
//! Every code above is already a row in [`crate::catalogue`] with no emitter, so
//! filling this stage adds behaviour and never a number.

use pharmakos_proto::gp::v1::Playbook;

use crate::limits::Limits;
use crate::report::Builder;
use crate::scope::Scope;

/// Run the stage. Nothing to run yet (item 82).
///
/// The signature is the one T7, T8 and S1 will write into, and it is deliberately
/// the same shape as every QUICK stage's: the playbook, the seat's view, the
/// rules-table numbers, and the shared report builder.
pub(crate) fn run(_playbook: &Playbook, _scope: &Scope, _limits: &Limits, _out: &mut Builder) {}

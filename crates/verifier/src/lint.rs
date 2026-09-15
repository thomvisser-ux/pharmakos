// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stage 6, **lint**: the second half of FULL. **Present and empty.**
//!
//! The same bargain as [`crate::estimate`], for the same reason (decisions-log
//! item 82): the stage exists from day one so the pipeline's shape and the hash
//! contract are fixed before anything depends on them, and it stays empty until
//! the stages that own its findings arrive.
//!
//! # What belongs here, and who brings it
//!
//! The `W07xx` family — schedule, conflicts and staleness — and the information
//! codes:
//!
//! | Work | Brought by |
//! |---|---|
//! | A handler that can never fire because an earlier one always wins | **S3**, with the rule list at full depth: `W0702` |
//! | A reference resting on a sighting older than the reach window | **S2/S3**, once there are sightings to be stale: `W0703` |
//! | Selector previews — "if this resolved now, it would pick…" | **S3**: `I0002` |
//!
//! A lint is **advice**, never a refusal: everything this stage can raise is a
//! warning or information, and spec section 11 is explicit that a playbook
//! qualifies on having zero **errors**. Nothing added here may change whether a
//! playbook qualifies without that being a deliberate, stated change.
//!
//! # Why the conflict lint is not a dry run
//!
//! "A handler that can never fire" is a question about the *text* — which
//! handler is earlier, and whether one condition implies another — and not about
//! a projected world. The moment it needs a future state to answer, it is out of
//! bounds and belongs nowhere in this crate (AGENTS.md section 3 rule 2).

use pharmakos_proto::gp::v1::Playbook;

use crate::limits::Limits;
use crate::report::Builder;
use crate::resolve::Symbols;
use crate::scope::Scope;

/// Run the stage. Nothing to run yet (item 82).
pub(crate) fn run(
    _playbook: &Playbook,
    _scope: &Scope,
    _limits: &Limits,
    _symbols: &Symbols,
    _out: &mut Builder,
) {
}

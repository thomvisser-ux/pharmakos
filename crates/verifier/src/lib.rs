// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The verifier — seal inspection. Role from spec section 11, sitting in the
//! gateway layer of spec section 15 (Architecture). Every seat passes through
//! it: the human's playbook and the built-in operator's are checked by the same
//! code on the same snapshot.
//!
//! # Pipeline
//!
//! `decode → structure → resolve → semantics` (together **QUICK**, run on every
//! edit) `→ estimate → lint` (**FULL**, always run on submit).
//!
//! | Stage | Module | State |
//! |---|---|---|
//! | decode | [`decode`] | complete |
//! | structure | [`structure`] | complete |
//! | resolve | [`resolve`] | complete |
//! | semantics | [`semantics`] | complete |
//! | estimate | [`estimate`] | **present and empty** — T7, T8, S1 |
//! | lint | [`lint`] | **present and empty** — S2, S3 |
//!
//! FULL's two stages exist and do nothing, deliberately (decisions-log item
//! 82): `verify_plan{depth}` and `report_hash` ship complete from day one, so
//! the API shape and the hash contract never change when S1 and S3 fill them,
//! and `submit_plan` runs FULL exactly as spec section 12 says. Today a FULL
//! report therefore carries exactly QUICK's diagnostics —
//! `full_finds_what_quick_finds` asserts it — and the two differ only in the
//! depth stamped on the report and hashed into it.
//!
//! # Determinism
//!
//! A report is a pure function of exactly five inputs — playbook bytes,
//! snapshot, rules hash, verifier version and depth — so a pre-check and the
//! check at submit are byte-identical, which is what `report_hash` proves. Two
//! reports compare only within one depth. [`hash`] writes down the encoding and
//! why the seat's [`Scope`] rides with the snapshot.
//!
//! No wall clock, no hash map, no float, no `as` cast: the verifier is
//! deterministic in the same way the sim is, and `tests/confinement.rs` asserts
//! it against this crate's own source text rather than trusting the lints alone.
//!
//! # Diagnostics
//!
//! Modelled on rustc's JSON output: a code, a severity, a JSON Pointer path,
//! related paths, map references, a precise message, a plain-language beginner
//! sentence, and JSON Patch suggestions labelled with how safely they apply.
//! Codes are language-neutral and live in [`catalogue`]; every user-facing
//! string in v1 is English and lives in [`strings`], the verifier's section of
//! the one string table.
//!
//! An out-of-vocabulary construct is **rejected** with a code and a JSON Pointer
//! to the offending node — never silently stripped, on Load as well as on
//! submit (spec sections 10 and 11).
//!
//! # Rules this crate is held to
//!
//! * **No dry runs**, and therefore **no compile-time feature of its own**: only
//!   `crates/sim` defines one, this crate may never enable or transitively reach
//!   it, and `cargo xtask ci` enforces that. Allowed work is pathfinder travel
//!   estimates, interface-time arithmetic, `$` and `kW` projection, placement
//!   legality, selector previews and mast coverage. Forbidden is stepping the
//!   sim, running mandates, programs, combat or construction, modelling enemy
//!   behaviour, or evaluating rule conditions over a projected future.
//! * **Tuning values are data.** Not one limit is a constant here: they are rows
//!   in `rules/rules.v1.json`, read through [`Limits`], and covered by
//!   `rules_hash` (decisions-log item 89).
//!
//! # Using it
//!
//! ```no_run
//! use pharmakos_proto::gp::api::v1::verify_plan::Depth;
//! use pharmakos_sim::knowledge::SeatEconomy;
//! use pharmakos_sim::rules::RulesTable;
//! use pharmakos_sim::tables::SeatId;
//! use pharmakos_verifier::{Input, Scope, verify};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let rules = RulesTable::load(std::path::Path::new("rules/rules.v1.json"))?;
//! let scope = Scope::new(SeatId::new(0), SeatEconomy::default());
//! let playbook = std::fs::read("plan.json")?;
//! let snapshot: Vec<u8> = Vec::new();
//!
//! let input = Input::new(&playbook, &snapshot, &scope, &rules)?;
//! let report = verify(&input, Depth::Full);
//! assert!(report.qualifies || !report.diagnostics.is_empty());
//! # Ok(())
//! # }
//! ```

pub mod catalogue;
pub mod hash;
pub mod limits;
pub mod scope;
pub mod strings;

mod decode;
mod estimate;
mod lint;
mod pointer;
mod report;
mod resolve;
mod semantics;
mod size;
mod structure;
mod walk;

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{Snapshot, SnapshotError};

pub use hash::REPORT_HASH_DOMAIN;
pub use limits::{CONDITION_MAX_DEPTH, CONDITION_MAX_NODES, Limits, RulesGap};
pub use scope::{KnownBeacon, Scope};
pub use size::size_units;

/// This build of the verifier, opaque and hashed.
///
/// One of the five inputs to `report_hash`, so **changing it moves every report
/// hash in the project** — which is the point: a report is only comparable with
/// another report from the same verifier. It is a string rather than a number so
/// that a build can say what it is rather than only that it differs.
///
/// PLACEHOLDER: how this string is derived once there are releases — a version,
/// a build identity, or both — is the **owner's** call at the wk-35.5 v0.1 gate,
/// with the rest of the versioning. Until then it names the stage that produced
/// it, and the report goldens move when it moves.
pub const VERIFIER_VERSION: &str = "0.1.0-skeleton";

/// Everything one verification is a function of, apart from the depth.
///
/// Built once per call. [`Input::new`] is fallible because the rules table may
/// not carry a block the checks read, and a verifier that substituted zeroes for
/// a missing size budget would reject every playbook ever written.
#[derive(Clone, Copy, Debug)]
pub struct Input<'a> {
    playbook: &'a [u8],
    snapshot: &'a [u8],
    scope: &'a Scope,
    rules: &'a RulesTable,
    rules_hash: u64,
    limits: Limits,
}

impl<'a> Input<'a> {
    /// Assemble the inputs.
    ///
    /// `playbook` is the **exact bytes** a report is wanted about: canonical or
    /// plain `gp.v1` proto JSON as UTF-8. The verifier decodes what it hashes
    /// and hashes what it decodes. JSONC comment stripping, canonicalisation and
    /// the byte-exact round trip are `plan-core`'s and happen before this call
    /// (decisions-log item 74).
    ///
    /// `snapshot` is the seat's frozen planning snapshot as
    /// `pharmakos_sim::snapshot::Snapshot` postcard bytes. The verifier treats
    /// it as opaque — it is one of the five hashed inputs, and the seat's view
    /// of it is `scope`.
    ///
    /// # Errors
    ///
    /// Returns [`RulesGap`] when the rules table does not carry a block the
    /// checks read.
    pub fn new(
        playbook: &'a [u8],
        snapshot: &'a [u8],
        scope: &'a Scope,
        rules: &'a RulesTable,
    ) -> Result<Input<'a>, RulesGap> {
        Ok(Input {
            playbook,
            snapshot,
            scope,
            rules,
            rules_hash: rules.rules_hash(),
            limits: Limits::from_rules(rules)?,
        })
    }

    /// The playbook bytes, exactly as handed in.
    #[must_use]
    pub const fn playbook(&self) -> &'a [u8] {
        self.playbook
    }

    /// The snapshot bytes, exactly as handed in.
    #[must_use]
    pub const fn snapshot(&self) -> &'a [u8] {
        self.snapshot
    }

    /// The seat's view of that snapshot.
    #[must_use]
    pub const fn scope(&self) -> &'a Scope {
        self.scope
    }

    /// The rules table this verification is against.
    #[must_use]
    pub const fn rules(&self) -> &'a RulesTable {
        self.rules
    }

    /// The numbers the checks compare against.
    #[must_use]
    pub const fn limits(&self) -> &Limits {
        &self.limits
    }

    /// `rules_hash`, computed once when the input was assembled.
    #[must_use]
    pub const fn rules_hash(&self) -> u64 {
        self.rules_hash
    }

    /// The snapshot, decoded.
    ///
    /// The QUICK stages read the [`Scope`] rather than the snapshot, so nothing
    /// in this crate calls this today. It is here because the snapshot is a
    /// named input rather than an opaque blob, and because the estimate stage
    /// will want the map out of it — the fact that the bytes are a snapshot
    /// should be checkable by the caller, and by a test, without this crate
    /// guessing.
    ///
    /// # Errors
    ///
    /// As [`Snapshot::from_bytes`]: the bytes are not a snapshot, or are one
    /// this build cannot read.
    pub fn decode_snapshot(&self) -> Result<Snapshot, SnapshotError> {
        Snapshot::from_bytes(self.snapshot)
    }
}

/// Inspect a seal.
///
/// Runs the pipeline to `depth` and returns the report the gateway hands back
/// unchanged. `DEPTH_UNSPECIFIED` is read as `FULL`, which is `verify_plan`'s
/// documented default and what `submit_plan` always runs (spec sections 11 and
/// 12); the depth the report carries and hashes is the one that actually ran,
/// never the one that was asked for.
///
/// Never fails and never panics: everything a playbook can be wrong about comes
/// back as a diagnostic, which is the difference between a verifier and a
/// parser.
#[must_use]
pub fn verify(input: &Input<'_>, depth: Depth) -> VerifyReport {
    let depth = match depth {
        Depth::Unspecified | Depth::Full => Depth::Full,
        Depth::Quick => Depth::Quick,
    };
    let limits = input.limits();
    let mut out = report::Builder::default();

    let mut units: u32 = 0;
    let mut fingerprint: Vec<u8> = Vec::new();

    if let Some(playbook) = decode::run(input.playbook, &mut out) {
        units = size::size_units(&playbook);
        if let Some(digest) = hash::plan_fingerprint(&playbook) {
            fingerprint = digest.to_be_bytes().to_vec();
        }
        structure::run(&playbook, limits, &mut out);
        let symbols = resolve::run(&playbook, input.scope, &mut out);
        semantics::run(&playbook, input.scope, limits, &symbols, &mut out);
        if depth == Depth::Full {
            estimate::run(&playbook, input.scope, limits, &mut out);
            lint::run(&playbook, input.scope, limits, &symbols, &mut out);
        }
    }

    let report_hash = hash::report_hash(
        input.playbook,
        input.snapshot,
        input.scope,
        input.rules_hash,
        VERIFIER_VERSION,
        depth,
    );

    VerifyReport {
        qualifies: !out.has_errors(),
        depth: i32::from(depth),
        diagnostics: out.into_diagnostics(),
        report_hash: report_hash.to_be_bytes().to_vec(),
        rules_hash: input.rules_hash.to_be_bytes().to_vec(),
        plan_fingerprint: fingerprint,
        verifier_version: VERIFIER_VERSION.to_owned(),
        size_units: units,
        size_budget: limits.size_budget_units(),
    }
}

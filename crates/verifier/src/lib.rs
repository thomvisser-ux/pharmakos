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
//! | estimate | [`estimate`] | the `$` and `kW` projection and the schedule (S1); mast coverage is S4's |
//! | lint | [`lint`] | targeting's three lints (S1); S2 and S3 add the rest |
//!
//! FULL's two stages shipped present and empty, deliberately (decisions-log
//! item 82): `verify_plan{depth}` and `report_hash` were complete from day one,
//! so the API shape and the hash contract never change as S1 and S3 fill them,
//! and `submit_plan` runs FULL exactly as spec section 12 says. A FULL report
//! carries QUICK's diagnostics first, in the same order, and then whatever the
//! two FULL stages add. Of that, two codes refuse: `E0601`, spec section 11's
//! "adding draw beyond supply is an error unless 'allow dormant beacons' is
//! set", and from S2 `E0603`, a seal the Ledger cannot price, which
//! `submit_plan` refuses after the verifier (decisions-log item 135 (2) (d)) —
//! `full_finds_what_quick_finds` asserts it.
//!
//! # What the sim refuses, refused here first (S2)
//!
//! From S2 every construct the sim's compile refuses as not at this stage has
//! an error of its own, raised with the structure stage's checks
//! ([`REFUSED_CONSTRUCTS`], keyed by the sim's name for each), and so does a
//! `scout_count` past a beacon's byte (`E0508`), so the editor's verdict and
//! the seal's agree on those (S2's plan, engineering call E8).
//!
//! # S2's constructs (task `vfy2`)
//!
//! Defend's and Attack's settings are checked for what they must write and
//! the ranges of what they write (the structure stage), for the side of the
//! beacons they name (`E0414`, `E0415`) and, for an Attack target, for its
//! freshness: a target named by its handle, or one seat's commander, must
//! rest on a sighting inside the reach window when the playbook is sealed
//! (`E0418`; spec section 6). The enemy commander with no seat named
//! qualifies without one, with `W0708`: the sim chooses it at launch and holds
//! the force until a fresh sighting exists (S2's `vfy4`). A `wall` target's
//! box is required, unturned, anchored inside itself, tall enough and inside
//! its beacon's sphere
//! (`E0101`, `E0511`, `E0417`, `E0512`, `E0416`). The sim's handler ceiling is
//! `E0113`. Defend's and Attack's are refused today by the arm's own refusal as
//! well (`E0012`, `E0013`), until the S2 lanes that give them an effect lift
//! those; the checks stand ready for that day. A wall's checks are live: S2's
//! `site` built the base kit and retired `E0019`, and the sim makes the
//! ground-dependent halves of the box's checks when the row commits.
//!
//! # The arithmetic the estimate stage shares
//!
//! [`interface`] (spec section 5's rates) and [`projection`] (the `$` and `kW`
//! the route orders) moved here from `plan-core`, which re-exports both, so the
//! estimate stage could use them without a dependency cycle and the prose, the
//! editor and the verifier price one playbook one way (S1's plan, task
//! `proj`). [`walking`] is the commander's walking as a lower bound, which is
//! all the verifier can say without the pathfinder it never names.
//!
//! # Targeting (S1)
//!
//! From S1 a playbook can name or describe a vent or a seam
//! (docs/design/targeting.md). The verifier checks which arm stands where,
//! the picks' ranks and filters, that a name exists and that `on` stands on a
//! vent, and it lints a description that matches nothing, one an earlier
//! placement may change, and a fixed site beside a vent the seat already
//! covers. It **never ranks**: "nearest" is the sim's and the gateway's, and
//! `tests/confinement.rs`'s `the_verifier_never_ranks` holds this crate to
//! it.
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
pub mod interface;
pub mod limits;
pub mod projection;
pub mod scope;
pub mod strings;
pub mod walking;

mod decode;
mod enums;
mod estimate;
mod lint;
mod pointer;
mod refusals;
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

pub use hash::{HashError, REPORT_HASH_DOMAIN};
pub use interface::Rates;
pub use limits::{CONDITION_MAX_DEPTH, CONDITION_MAX_NODES, Limits, RulesGap};
pub use projection::{Prices, Projection, ProjectionError};
pub use refusals::{REFUSED_CONSTRUCTS, code_for_construct};
pub use scope::{FeatureKind, KnownBeacon, KnownFeature, Ownership, Scope, Sighted, Sighting};
pub use size::size_units;
pub use walking::Walk;

/// This build of the verifier, opaque and hashed.
///
/// One of the five inputs to `report_hash`, so **changing it moves every report
/// hash in the project** — which is the point: a report is only comparable with
/// another report from the same verifier. It is a string rather than a number so
/// that a build can say what it is rather than only that it differs.
///
/// PLACEHOLDER: how this string is derived once there are releases — owner, the wk-35.5 v0.1 gate.
///
/// A version, a build identity, or both, decided with the rest of the
/// versioning. Until then it names the stage that produced
/// it, and the report goldens move when it moves. `0.1.0-s1` from S1's
/// estimate stage (task `proj`): a FULL report from that build found what the
/// skeleton's empty stage could not. `0.1.0-s2` from S2's first verifier lane
/// (task `vfy1`): this build refuses, by a code of its own, every construct
/// the sim refuses at the seal (`E0009` to `E0017`, the plan's E8), an
/// enum number this build does not name (`E0018`), a
/// `scout_count` past a beacon's byte (`E0508`) and a seal the Ledger cannot
/// price (`E0603`), so its reports are not comparable with S1's either.
/// `0.1.0-s2.2` from S2's second verifier lane (task `vfy2`): this build
/// checks Defend's and Attack's settings, a wall's box and an Attack target's
/// freshness, prices the new fields, refuses more handlers than the sim
/// compiles, and notes an omitted `roe` and a repair threshold of 0, so its
/// reports are not comparable with `0.1.0-s2`'s. `0.1.0-s2.3` from S2's
/// threat model's verifier lane (task `vfy3`): this build notes a move that
/// avoids known threats with no enemy beacon in sight (`I0005`), reads an
/// omitted `roe` by writ, and refuses a half-written area, so its reports are
/// not comparable with `0.1.0-s2.2`'s. `0.1.0-s2.4` from S2's `vfy4`: this
/// build qualifies an `enemy_commander` Attack target with no seat named and
/// no fresh sighting, with `W0708` in place of `E0418`, so its reports are not
/// comparable with `0.1.0-s2.3`'s.
pub const VERIFIER_VERSION: &str = "0.1.0-s2.4";

/// Everything one verification is a function of, apart from the depth.
///
/// Built once per call. [`Input::new`] is fallible because the rules table may
/// not carry a block the checks read, and a verifier that substituted zeroes for
/// a missing size budget would reject every playbook ever written; and because
/// an input with no encoding has no report hash ([`HashError`]), which is
/// found here rather than half-way through a report.
#[derive(Clone, Copy, Debug)]
pub struct Input<'a> {
    playbook: &'a [u8],
    snapshot: &'a [u8],
    scope: &'a Scope,
    rules: &'a RulesTable,
    rules_hash: u64,
    limits: Limits,
    rates: Rates,
    prices: Prices,
    walk: Walk,
    report_hashes: hash::ReportHashes,
}

/// Why an [`Input`] could not be assembled.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum InputError {
    /// The rules table does not carry a block the checks read, or carries one
    /// out of range.
    Rules(RulesGap),
    /// An input has no encoding for `report_hash`: a byte string or a list
    /// past the encoding's `u32` length prefix.
    Unhashable(HashError),
}

impl core::fmt::Display for InputError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            InputError::Rules(gap) => write!(formatter, "{gap}"),
            InputError::Unhashable(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for InputError {}

impl From<RulesGap> for InputError {
    fn from(gap: RulesGap) -> InputError {
        InputError::Rules(gap)
    }
}

impl From<HashError> for InputError {
    fn from(error: HashError) -> InputError {
        InputError::Unhashable(error)
    }
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
    /// [`InputError::Rules`] when the rules table does not carry a block the
    /// checks read — the verifier's limits, the interface rates, the prices
    /// and the commander's walking rows — or carries one out of range; and
    /// [`InputError::Unhashable`] when the playbook, the snapshot or a list or
    /// string in the view is past `report_hash`'s `u32` length prefix.
    pub fn new(
        playbook: &'a [u8],
        snapshot: &'a [u8],
        scope: &'a Scope,
        rules: &'a RulesTable,
    ) -> Result<Input<'a>, InputError> {
        let rules_hash = rules.rules_hash();
        Ok(Input {
            playbook,
            snapshot,
            scope,
            rules,
            rules_hash,
            limits: Limits::from_rules(rules)?,
            rates: Rates::from_rules(rules)?,
            prices: Prices::from_rules(rules)?,
            walk: Walk::from_rules(rules)?,
            report_hashes: hash::ReportHashes::new(
                playbook,
                snapshot,
                scope,
                rules_hash,
                VERIFIER_VERSION,
            )?,
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

    /// Spec section 5's interface rates, which the estimate stage prices a
    /// visit and a deploy at.
    #[must_use]
    pub const fn rates(&self) -> &Rates {
        &self.rates
    }

    /// The rules table's prices, which the estimate stage projects `$` and
    /// `kW` with.
    #[must_use]
    pub const fn prices(&self) -> &Prices {
        &self.prices
    }

    /// The commander's walking rows, which the estimate stage bounds a leg
    /// with.
    #[must_use]
    pub const fn walk(&self) -> &Walk {
        &self.walk
    }

    /// `rules_hash`, computed once when the input was assembled.
    #[must_use]
    pub const fn rules_hash(&self) -> u64 {
        self.rules_hash
    }

    /// The snapshot, decoded.
    ///
    /// The QUICK stages read the [`Scope`] rather than the snapshot and never
    /// call this. The estimate stage does, for the one fact the snapshot
    /// carries and the view does not: the coming segment's length (spec
    /// section 3). The fact that the bytes are a snapshot is checkable by the
    /// caller, and by a test, without this crate guessing.
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
            // `to_field` rather than `to_be_bytes` here: `crates/proto`'s
            // `fingerprint` module owns "eight big-endian bytes" (item 77), and
            // a rule with one home cannot drift between its callers.
            fingerprint = pharmakos_proto::fingerprint::to_field(digest);
        }
        structure::run(&playbook, limits, &mut out);
        refusals::run(&playbook, &mut out);
        let symbols = resolve::run(&playbook, input.scope, &mut out);
        semantics::run(&playbook, input.scope, limits, &symbols, &mut out);
        if depth == Depth::Full {
            estimate::run(&playbook, input, &mut out);
            lint::run(&playbook, input.scope, limits, &symbols, &mut out);
        }
    }

    let report_hash = input.report_hashes.at(depth);

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

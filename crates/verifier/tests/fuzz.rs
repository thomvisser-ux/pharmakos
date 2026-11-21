// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The 10 000-playbook fuzz target (AGENTS.md section 9, "Nightly").
//!
//! Gate P1's fourth clause is "10k fuzzed playbooks with no panic". The verifier
//! is the one place in the game that reads a file a human wrote by hand, so the
//! property it has to hold is blunt: **whatever the bytes are, a report comes
//! back**. A panic here would turn a typo into a crash, and a crash during the
//! Lull is a seat that cannot seal.
//!
//! # How to run it
//!
//! ```sh
//! cargo test -p pharmakos-verifier --test fuzz -- --ignored --nocapture
//! ```
//!
//! It is `#[ignore]`d because ten thousand verifications do not belong in the
//! inner loop. `.github/workflows/nightly-scenarios.yml`'s `fuzz` job runs it,
//! as above with `--locked`, behind that workflow's variable gate (decisions-log
//! item 116 (6)(j)). `the_generator_still_produces_shapes_the_verifier_reads`
//! runs the first [`SMOKE_RUNS`] of the same generator on every ordinary
//! `cargo test`, every invariant [`check`] holds a run to included, and holds
//! them to the nightly run's S3 construct check, so a generator that stops
//! producing qualifying runs, or stops reaching a construct the verifier no
//! longer refuses, goes red on the pull request. The nightly run's other
//! tallies (the codes it must reach) are its own.
//!
//! # Seeded, not random
//!
//! The generator is `pharmakos_sim::math::random::StreamRng` — the project's own
//! counter-based split streams (decisions-log item 50). A failing iteration is
//! therefore reproducible from its index alone: there is no global state, and
//! the value at every position is a pure function of
//! `(seed, stream, tick, seat, sub, counter)`. A fuzz failure nobody can
//! reproduce is a rumour.
//!
//! # Random but *shaped*
//!
//! Uniform noise would be rejected by the JSON reader almost every time and
//! would exercise one code. So the generator emits playbook-shaped documents —
//! real field names, real enum spellings, plausible nesting — and then damages
//! them: a missing block, a negative duration, a label that names nothing, a
//! held-back word, a value of the wrong type. That is the population a hand
//! editor actually produces.
//!
//! # The estimate stage's arithmetic (S1's `proj`)
//!
//! The estimate stage sums counts the author controls, so the generator writes
//! the orders it prices — Survey scout counts over the whole `u32` field, Build
//! targets, `set_mandate`, `set_mandate_settings`, `add_build_target` and
//! `queue_structure` rows, an unpriced blueprint on a damaged run, and the
//! `allow_dormant_beacons` option — against a seat with 8 `kW` of headroom and
//! a snapshot with a segment coming. Beyond "a report comes back", one property
//! is checked on every run: a route whose deploys order more draw than the
//! headroom carries `E0601` or `W0603`, however large the count, against a
//! reference sum written apart from the verifier's.

#[allow(
    dead_code,
    reason = "the table is shared with plan_errors.rs, which reads the rest of it"
)]
mod plan_error_table;

use pharmakos_proto::gp::api::v1::VerifyReport;
use pharmakos_proto::gp::api::v1::diagnostic::Severity;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::gp::v1::by_richness::Richness;
use pharmakos_proto::json::{self, Json};
use pharmakos_sim::interpreter::Plan;
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::math::random::{Stream, StreamRng};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::catalogue::CATALOGUE;
use pharmakos_verifier::{
    FeatureKind, Input, KnownBeacon, KnownFeature, Ownership, Scope, Sighted, Sighting,
    VERIFIER_VERSION, verify,
};

use pharmakos_proto::gp::v1::beacon_filter::MandateKind;
use pharmakos_proto::gp::v1::{
    BuildTarget, MandateSettings, Playbook, Voxel, interface_row, mandate_settings, step,
};
use pharmakos_sim::math::quantity::{Kw, Money};

/// The blueprint id a damaged run orders that the rules table does not price.
const UNPRICED: &str = "death_ray";

/// The blueprints the rules table prices (`projection::BLUEPRINTS`), restated
/// for the property's reference sum.
const PRICED: [&str; 5] = [
    "generator",
    "autocannon",
    "mortar",
    "survey_post",
    "resonance_spire",
];

/// The fixture seat's economy: `$` 500, and 8 `kW` of headroom.
const TREASURY: i64 = 500;
const SUPPLY: i32 = 10;
const DRAW: i32 = 2;

/// How many playbooks the nightly run generates.
const RUNS: u32 = 10_000;

/// How many the ordinary `cargo test` run generates, so the generator is never
/// dead code between nightly runs, and enough that every S3 construct the
/// verifier no longer refuses qualifies in many of them, so a pull request
/// that stops the generator reaching one goes red, and a benign generator edit
/// does not. At the fuzz seed, over nine disjoint blocks of this many runs
/// against the smoke test's scope, the rarest construct (`toward`) qualifies
/// 18.9 times on average and 11 at the least.
const SMOKE_RUNS: u32 = 1_024;

/// An arbitrary, pinned fuzz seed. It is a *harness* seed and not a game value.
const FUZZ_SEED: u64 = 0x5052_4F42_4C45_4D53;

// ---------------------------------------------------------------------------
// The generator
// ---------------------------------------------------------------------------

/// Names the generator draws from, chosen so some resolve and some do not.
const NAMES: &[&str] = &[
    "s0", "s1", "s2", "h1", "b_01", "b_02", "e_01", "nowhere", "", "e_02", "e_03", "e_04",
];

/// The reach window the committed rules table holds (`verifier.reach_memory_ms`),
/// which the sightings' ages straddle; the long run checks it is still the
/// table's.
const WINDOW: u32 = 180_000;

/// S2's `vfy2` codes, S2's `atk`'s E0419, S2's `vfy4`'s W0708 and S2's
/// `atk2`'s W0709, in code order: the ten thousand runs reach every one.
const S2_CODES: [&str; 15] = [
    "E0113", "E0414", "E0415", "E0416", "E0417", "E0418", "E0419", "E0508", "E0510", "E0511",
    "E0512", "W0502", "W0708", "W0709", "I0004",
];

/// S3's `vfy5` codes, in code order: the ten thousand runs reach every one.
/// `E0507` and `E0513` stand behind `E0009`, so a run reaches them on a
/// damaged playbook's queued Spire; `E0513`'s standing Spire is
/// [`licensed_scope`]'s.
const S3_CODES: [&str; 4] = ["E0507", "E0513", "W0702", "I0006"];

/// S3's `vfy6` codes, in code order: the checks behind S3's predicates and
/// `toward`, which the ten thousand runs reach on a damaged run's
/// predicates and sites.
const VFY6_CODES: [&str; 5] = ["E0209", "E0210", "E0420", "E0421", "E0506"];

/// The size the padded runs reach past: twice the committed budget, the most
/// a later stage can choose (S3's plan, E24: never above 256), so the ten
/// thousand cover any budget `tune3` sets.
const PADDED_PAST: u32 = 256;

/// The codes for what the sim refuses at the seal (E0009 to E0016; S2's base
/// kit's E0019 is retired, since S2's `site` built it, E0012, E0014 and
/// E0017, since S2's `dfnd` built Defend's settings, `most_threatened` and
/// `beacon_under_attack`, and E0013, since S2's `atk` built Attack's
/// settings; and E0020 to E0028, since S3's `pred` built the predicates and
/// `toward`), for
/// an enum number this build does not name (E0018) and for a `wall_box` on a
/// target that is not a wall (E0509), in code order: the ten thousand runs
/// reach every one.
const REFUSALS: [&str; 7] = [
    "E0009", "E0010", "E0011", "E0015", "E0016", "E0018", "E0509",
];

/// The coordinates at the ends of the field, so the walk and the semantics
/// stage meet the values no map holds (S2's wire, the reviews of con4).
const EXTREMES: [i32; 4] = [i32::MIN, -1, 0, i32::MAX];

/// Words `gp.v1` holds back, so the never-strip path is exercised.
const HELD: &[&str] = &["set_flag", "branch", "repeat", "dispatch", "team_id"];

struct Shape {
    rng: StreamRng,
    /// When set, the generator damages nothing: every required block is there,
    /// every name is unique, every duration is positive, every voxel is on the
    /// map, every reference resolves, and it writes no construct the sim
    /// refuses at the seal (E0009 to E0017: no queued structure, no Build
    /// target but a Generator on a voxel, no S2 settings and no S2 selector
    /// but `most_threatened`, legal since S2's `dfnd`) and no enum number this
    /// build does not name (E0018). It does write S3's predicates and `toward`,
    /// whole, since S3's `pred` built them, so between them the runs that
    /// qualify hold every S3 construct.
    ///
    /// A generator that only ever produces rejected files exercises the decode
    /// stage and nothing else, and a green run over it would mean nothing — the
    /// lesson spike G3-prime wrote down about a sweep with nothing in it. One
    /// run in four is clean, so the later stages are reached too.
    clean: bool,
    labels: u32,
    handlers: u32,
    /// When set, the route is padded with holds to between 240 and about
    /// 330 size units, so the run meets sizes at and past [`PADDED_PAST`]
    /// (S3's `vfy6`): one run in eight, clean and damaged alike.
    padded: bool,
    /// When set, no held-back word is written, so a run that orders more
    /// handlers than the sim compiles reaches the structure stage's `E0113`
    /// rather than stopping at decode on one of its sixty-five handlers' steps.
    quiet: bool,
}

impl Shape {
    fn new(index: u32) -> Shape {
        Shape {
            rng: StreamRng::new(FUZZ_SEED, Stream::Map, index, 0, 0),
            clean: index.is_multiple_of(4),
            padded: matches!(index % 16, 4 | 7),
            quiet: false,
            labels: 0,
            handlers: 0,
        }
    }

    /// A number in `0..n`.
    fn below(&mut self, n: i32) -> i32 {
        self.rng.range_i32(0, n.saturating_sub(1).max(0))
    }

    /// True about one time in `n`.
    fn rarely(&mut self, n: i32) -> bool {
        self.below(n) == 0
    }

    /// True about one time in `n`, and never on a clean run.
    ///
    /// Every place the generator bends a rule goes through this, so "clean"
    /// needs no second code path to keep in step with the first.
    fn damage(&mut self, n: i32) -> bool {
        !self.clean && self.rarely(n)
    }

    /// A fresh step label on a clean run, an arbitrary one otherwise.
    fn label(&mut self) -> String {
        if self.clean {
            self.labels = self.labels.saturating_add(1);
            return format!("s{:03}", self.labels);
        }
        self.name()
    }

    /// A fresh handler id on a clean run, an arbitrary one otherwise.
    fn handler_id(&mut self) -> String {
        if self.clean {
            self.handlers = self.handlers.saturating_add(1);
            return format!("h{:03}", self.handlers);
        }
        self.name()
    }

    fn name(&mut self) -> String {
        let index =
            usize::try_from(self.below(i32::try_from(NAMES.len()).unwrap_or(1))).unwrap_or(0);
        NAMES.get(index).copied().unwrap_or("s0").to_owned()
    }

    fn number(&mut self, low: i32, high: i32) -> Json {
        Json::Number(self.rng.range_i32(low, high).to_string())
    }

    /// A duration: positive on a clean run, sometimes negative otherwise.
    fn duration(&mut self, high: i32) -> Json {
        let low = if self.clean { 1 } else { -high };
        self.number(low, high)
    }

    /// A voxel. On a clean run it sits inside the core's own sphere, so a
    /// `place_beacon` on it is legal; otherwise it wanders off the map.
    fn voxel(&mut self) -> Json {
        if self.clean {
            return Json::Object(vec![
                ("x".to_owned(), self.number(72, 88)),
                ("y".to_owned(), self.number(5, 18)),
                ("z".to_owned(), self.number(48, 62)),
            ]);
        }
        Json::Object(vec![
            ("x".to_owned(), self.number(-40, 420)),
            ("y".to_owned(), self.number(-40, 420)),
            ("z".to_owned(), self.number(-8, 80)),
        ])
    }

    fn beacon_ref(&mut self) -> Json {
        if self.clean {
            return match self.below(3) {
                0 => Json::Object(vec![(
                    "beacon_id".to_owned(),
                    Json::String("b_01".to_owned()),
                )]),
                1 => Json::Object(vec![("safest".to_owned(), Json::Object(Vec::new()))]),
                _ => Json::Object(vec![(
                    "most_threatened".to_owned(),
                    Json::Object(Vec::new()),
                )]),
            };
        }
        // `most_threatened`, legal since S2's `dfnd` built the threat model.
        if self.rarely(8) {
            return Json::Object(vec![(
                "most_threatened".to_owned(),
                Json::Object(Vec::new()),
            )]);
        }
        match self.below(4) {
            0 => Json::Object(vec![("beacon_id".to_owned(), Json::String(self.name()))]),
            1 => Json::Object(vec![("safest".to_owned(), Json::Object(Vec::new()))]),
            2 => {
                let tags = if self.rarely(2) {
                    vec![Json::String("east".to_owned())]
                } else {
                    Vec::new()
                };
                let side = if self.rarely(2) { "ENEMY_KNOWN" } else { "OWN" };
                Json::Object(vec![(
                    "nearest".to_owned(),
                    Json::Object(vec![(
                        "filter".to_owned(),
                        Json::Object(vec![
                            ("side".to_owned(), Json::String(side.to_owned())),
                            ("tags".to_owned(), Json::Array(tags)),
                        ]),
                    )]),
                )])
            }
            _ => Json::Object(vec![(
                "weakest".to_owned(),
                Json::Object(vec![("filter".to_owned(), Json::Object(Vec::new()))]),
            )]),
        }
    }

    fn location(&mut self) -> Json {
        // Targeting's two arms (S1), never on a clean run: they are legal in
        // one slot each, so most places this lands are refusals.
        if self.damage(4) {
            return self.feature_site();
        }
        match self.below(3) {
            0 => Json::Object(vec![("voxel".to_owned(), self.voxel())]),
            1 => Json::Object(vec![("beacon_anchor".to_owned(), self.beacon_ref())]),
            _ => Json::Object(vec![("safest".to_owned(), Json::Object(Vec::new()))]),
        }
    }

    /// A `place_beacon` site that covers a feature, or walks toward one (S3's
    /// `toward`, one site in two, [`Shape::toward_site`]): a legal one on a
    /// clean run (an UNCOVERED vent pick, or a name the scope holds, lost or
    /// live), anything [`Shape::feature_site`] makes otherwise.
    fn covering_site(&mut self) -> Json {
        if !self.clean {
            return self.feature_site();
        }
        if self.rarely(2) {
            return self.toward_site();
        }
        let reference = if self.rarely(2) {
            (
                "vent",
                Json::Object(vec![
                    ("rank".to_owned(), Json::String("NEAREST".to_owned())),
                    ("coverage".to_owned(), Json::String("UNCOVERED".to_owned())),
                ]),
            )
        } else {
            let index = usize::try_from(self.below(4)).unwrap_or(0);
            let id = FEATURE_IDS.get(index).copied().unwrap_or("vent_150_25");
            ("feature_id", Json::String(id.to_owned()))
        };
        Json::Object(vec![(
            "covering".to_owned(),
            Json::Object(vec![(reference.0.to_owned(), reference.1)]),
        )])
    }

    /// A clean run's S3 `toward` site: a vent or a seam picked NEAREST under
    /// either coverage, or a name the scope holds, vent or seam, lost or live.
    fn toward_site(&mut self) -> Json {
        let feature = if self.rarely(3) {
            let id = pick(self, &FEATURE_IDS);
            Json::Object(vec![("feature_id".to_owned(), Json::String(id.to_owned()))])
        } else {
            let kind = pick(self, &["vent", "seam"]);
            let coverage = pick(self, &["ANY", "UNCOVERED"]);
            Json::Object(vec![(
                kind.to_owned(),
                Json::Object(vec![
                    ("rank".to_owned(), Json::String("NEAREST".to_owned())),
                    ("coverage".to_owned(), Json::String(coverage.to_owned())),
                ]),
            )])
        };
        Json::Object(vec![(
            "toward".to_owned(),
            Json::Object(vec![("feature".to_owned(), feature)]),
        )])
    }

    /// `on`, `covering` or S3's `toward`, with a name, a pick (sometimes with
    /// its rank or coverage left out), `covered {}`, or nothing chosen at all.
    fn feature_site(&mut self) -> Json {
        let arm = ["on", "covering", "toward"]
            .get(usize::try_from(self.below(3)).unwrap_or(0))
            .copied()
            .unwrap_or("on");
        let pick = |rank: bool, coverage: Option<&str>| {
            let mut fields: Vec<(String, Json)> = Vec::new();
            if rank {
                fields.push(("rank".to_owned(), Json::String("NEAREST".to_owned())));
            }
            if let Some(filter) = coverage {
                fields.push(("coverage".to_owned(), Json::String(filter.to_owned())));
            }
            Json::Object(fields)
        };
        let reference = match self.below(6) {
            0 => {
                let index = usize::try_from(self.below(4)).unwrap_or(0);
                let id = FEATURE_IDS.get(index).copied().unwrap_or("vent_150_25");
                ("feature_id", Json::String(id.to_owned()))
            }
            1 => ("feature_id", Json::String(self.name())),
            2 => ("vent", pick(true, Some("UNCOVERED"))),
            3 => ("seam", pick(self.rarely(2), Some("ANY"))),
            4 => ("vent", pick(true, None)),
            _ => ("covered", Json::Object(Vec::new())),
        };
        let inner = if self.rarely(8) {
            Json::Object(Vec::new())
        } else {
            Json::Object(vec![(reference.0.to_owned(), reference.1)])
        };
        // `toward` holds its feature one level down, and now and then none.
        let inner = if arm == "toward" && !self.rarely(8) {
            Json::Object(vec![("feature".to_owned(), inner)])
        } else {
            inner
        };
        Json::Object(vec![(arm.to_owned(), inner)])
    }

    fn condition(&mut self, depth: i32) -> Json {
        if depth > 0 && self.rarely(3) {
            let group = if self.rarely(2) { "all" } else { "any" };
            let count = self.below(3).saturating_add(1);
            let mut items: Vec<Json> = Vec::new();
            for _ in 0..count {
                items.push(self.condition(depth.saturating_sub(1)));
            }
            return Json::Object(vec![(
                group.to_owned(),
                Json::Object(vec![("items".to_owned(), Json::Array(items))]),
            )]);
        }
        // S3's eight predicates, one condition in three on a clean run and
        // written whole there, so the runs that qualify hold each of them and
        // the compile cross-check in [`check`] reaches every one (S2's E8). A
        // damaged run reaches them below, with what each holds damaged, and
        // almost never qualifies, so the clean runs are where they qualify.
        // The draw is a clean run's alone: a damaged run's draws are unchanged.
        if self.clean && self.rarely(3) {
            return self.s3_predicate();
        }
        // Otherwise a clean run keeps to the three predicates that need no
        // reference to resolve; the rest are what the resolve stage is for.
        let families = if self.clean { 3 } else { 9 };
        match self.below(families) {
            7 | 8 => self.s3_predicate(),
            0 => {
                let pct = if self.clean {
                    self.number(0, 100)
                } else {
                    self.number(-10, 140)
                };
                let mut fields = vec![("pct".to_owned(), pct)];
                if !self.damage(4) {
                    fields.insert(0, ("cmp".to_owned(), Json::String("LE".to_owned())));
                }
                Json::Object(vec![("cmdr_hp_pct".to_owned(), Json::Object(fields))])
            }
            1 => {
                let ms = self.duration(60_000);
                Json::Object(vec![(
                    "cmdr_took_damage_within".to_owned(),
                    Json::Object(vec![("ms".to_owned(), ms)]),
                )])
            }
            2 => {
                let compare = self.int_compare();
                Json::Object(vec![(
                    "treasury".to_owned(),
                    Json::Object(vec![("dollars".to_owned(), compare)]),
                )])
            }
            3 => Json::Object(vec![(
                "step_reached".to_owned(),
                Json::Object(vec![("label".to_owned(), Json::String(self.name()))]),
            )]),
            4 => Json::Object(vec![(
                "rule_fired".to_owned(),
                Json::Object(vec![("handler_id".to_owned(), Json::String(self.name()))]),
            )]),
            6 => {
                // Legal since S2's `dfnd` (E0017 retired).
                let beacon = self.beacon_ref();
                Json::Object(vec![(
                    "beacon_under_attack".to_owned(),
                    Json::Object(vec![
                        ("beacon".to_owned(), beacon),
                        ("within_ms".to_owned(), Json::Number("5000".to_owned())),
                    ]),
                )])
            }
            _ => {
                let beacon = self.beacon_ref();
                let compare = self.int_compare();
                Json::Object(vec![(
                    "beacon_hp_pct".to_owned(),
                    Json::Object(vec![
                        ("beacon".to_owned(), beacon),
                        ("pct".to_owned(), compare),
                    ]),
                )])
            }
        }
    }

    /// One of S3's `con6` predicates: written whole on a clean run, and on a
    /// damaged one each field now and then left out or out of range, as a hand
    /// editor writes them (S3's `vfy6`).
    fn s3_predicate(&mut self) -> Json {
        let (name, mut fields): (&str, Vec<(String, Json)>) = match self.below(8) {
            0 => {
                let role = self.wild_enum(&[
                    "BUILD_DRONE",
                    "MINING_DRONE",
                    "REPAIR_DRONE",
                    "RAIDER",
                    "SCOUT",
                ]);
                (
                    "beacon_units_by_role",
                    vec![
                        ("beacon".to_owned(), self.beacon_ref()),
                        ("role".to_owned(), role),
                        ("count".to_owned(), self.int_compare()),
                    ],
                )
            }
            1 => {
                let mandate = self.wild_enum(&["BUILD", "MINE", "DEFEND", "ATTACK", "SURVEY"]);
                (
                    "beacon_mandate",
                    vec![
                        ("beacon".to_owned(), self.beacon_ref()),
                        ("mandate".to_owned(), mandate),
                    ],
                )
            }
            2 => (
                "beacon_has_structure",
                vec![
                    ("beacon".to_owned(), self.beacon_ref()),
                    ("blueprint_id".to_owned(), self.structure_kind()),
                ],
            ),
            3 => (
                "enemy_units_near",
                vec![
                    ("near".to_owned(), self.near()),
                    ("radius_voxels".to_owned(), self.radius()),
                    ("count".to_owned(), self.int_compare()),
                    ("freshness".to_owned(), self.freshness()),
                ],
            ),
            4 => (
                "enemy_beacon_known",
                vec![("freshness".to_owned(), self.freshness())],
            ),
            5 => {
                let structure = match self.below(3) {
                    0 => ("any_kind".to_owned(), Json::Object(Vec::new())),
                    _ => ("blueprint_id".to_owned(), self.structure_kind()),
                };
                (
                    "enemy_structure_near",
                    vec![
                        ("near".to_owned(), self.near()),
                        ("radius_voxels".to_owned(), self.radius()),
                        structure,
                        ("freshness".to_owned(), self.freshness()),
                    ],
                )
            }
            6 => ("kw_supply", vec![("kw".to_owned(), self.int_compare())]),
            _ => ("kw_draw", vec![("kw".to_owned(), self.int_compare())]),
        };
        // One field in four left out on a damaged run, whichever it is.
        if !fields.is_empty() && self.damage(4) {
            let index =
                usize::try_from(self.below(i32::try_from(fields.len()).unwrap_or(1))).unwrap_or(0);
            if index < fields.len() {
                fields.remove(index);
            }
        }
        Json::Object(vec![(name.to_owned(), Json::Object(fields))])
    }

    /// An enemy predicate's place: a voxel, a beacon, the safest, or (on a
    /// damaged run) none; a damaged run's voxel ranges over the whole field.
    fn near(&mut self) -> Json {
        let arms = if self.clean { 3 } else { 4 };
        match self.below(arms) {
            0 => {
                let voxel = if self.clean {
                    self.voxel()
                } else {
                    self.wild_voxel()
                };
                Json::Object(vec![("voxel".to_owned(), voxel)])
            }
            1 => Json::Object(vec![("beacon_anchor".to_owned(), self.beacon_ref())]),
            2 => Json::Object(vec![("safest".to_owned(), Json::Object(Vec::new()))]),
            _ => Json::Object(Vec::new()),
        }
    }

    /// An enemy predicate's radius: positive on a clean run, sometimes 0 (no
    /// radius, `E0101`) otherwise.
    fn radius(&mut self) -> Json {
        let low = i32::from(self.clean);
        self.number(low, 64)
    }

    /// A freshness: on a clean run either kind, with a positive window under
    /// `KNOWN_WITHIN` and none under `VISIBLE_NOW`; otherwise either kind, a
    /// window under either (or none), an unset kind, or a number the enum does
    /// not name.
    fn freshness(&mut self) -> Json {
        if self.clean {
            if self.rarely(2) {
                return Json::Object(vec![(
                    "kind".to_owned(),
                    Json::String("VISIBLE_NOW".to_owned()),
                )]);
            }
            return Json::Object(vec![
                ("kind".to_owned(), Json::String("KNOWN_WITHIN".to_owned())),
                ("known_within_ms".to_owned(), self.number(1, 240_000)),
            ]);
        }
        let mut fields: Vec<(String, Json)> = Vec::new();
        if !self.rarely(5) {
            let kind = self.wild_enum(&["VISIBLE_NOW", "KNOWN_WITHIN"]);
            fields.push(("kind".to_owned(), kind));
        }
        if self.rarely(2) {
            fields.push(("known_within_ms".to_owned(), self.number(-1_000, 240_000)));
        }
        Json::Object(fields)
    }

    fn int_compare(&mut self) -> Json {
        let mut fields: Vec<(String, Json)> = Vec::new();
        // A clean run's operator is any of the six; a damaged run's is `GE`,
        // or now and then none.
        if self.clean {
            let op = pick(self, &["LT", "LE", "EQ", "NE", "GE", "GT"]);
            fields.push(("op".to_owned(), Json::String(op.to_owned())));
        } else if !self.damage(5) {
            fields.push(("op".to_owned(), Json::String("GE".to_owned())));
        }
        let value = if self.clean {
            self.number(0, 100)
        } else {
            self.number(-200, 200)
        };
        fields.push(("value".to_owned(), value));
        Json::Object(fields)
    }

    fn entry(&mut self) -> Json {
        let mut fields: Vec<(String, Json)> = Vec::new();
        self.guards(&mut fields);
        self.action(&mut fields);
        Json::Object(fields)
    }

    /// The guards every step supports, plus the two damage sites that hang off
    /// them: a jump that names nothing in particular, and a held-back word.
    fn guards(&mut self, fields: &mut Vec<(String, Json)>) {
        if !self.damage(8) {
            let label = self.label();
            fields.push(("label".to_owned(), Json::String(label)));
        }
        if self.rarely(4) {
            let timeout = self.duration(120_000);
            fields.push(("timeout_ms".to_owned(), timeout));
        }
        if self.rarely(6) {
            let guard = self.condition(2);
            fields.push(("skip_if".to_owned(), guard));
        }
        if self.damage(5) {
            let action = if self.rarely(2) {
                "JUMP_FORWARD"
            } else {
                "SKIP"
            };
            let label = self.name();
            fields.push((
                "on_fail".to_owned(),
                Json::Object(vec![
                    ("action".to_owned(), Json::String(action.to_owned())),
                    ("jump_to_label".to_owned(), Json::String(label)),
                ]),
            ));
        }
        if !self.quiet && self.damage(12) {
            let index = usize::try_from(self.below(5)).unwrap_or(0);
            let held = HELD.get(index).copied().unwrap_or("set_flag");
            fields.push((held.to_owned(), Json::Object(Vec::new())));
        }
    }

    /// The step itself.
    ///
    /// One step in six names no action at all on a damaged run: that is
    /// `E0103`, and a hand-edited file produces it constantly. A clean run
    /// always names one.
    fn action(&mut self, fields: &mut Vec<(String, Json)>) {
        let kinds = if self.clean { 5 } else { 6 };
        match self.below(kinds) {
            0 => {
                let place = self.location();
                let mut walk = vec![("to".to_owned(), place)];
                // A number `Pace` does not name (E0018), never read as unset.
                if self.damage(6) {
                    walk.push(("pace".to_owned(), Json::Number("9".to_owned())));
                }
                fields.push(("move".to_owned(), Json::Object(walk)));
            }
            1 => {
                // One site in three describes a feature instead (one in two
                // on a clean run, so its `toward` share is wide enough for the
                // smoke test), so the lint stage's targeting checks see sites
                // they have something to say about.
                let feature = if self.clean {
                    self.rarely(2)
                } else {
                    self.below(3) == 0
                };
                let site = if feature {
                    self.covering_site()
                } else {
                    Json::Object(vec![("voxel".to_owned(), self.voxel())])
                };
                let retreat = if self.clean {
                    self.number(0, 100)
                } else {
                    self.number(0, 150)
                };
                fields.push((
                    "place_beacon".to_owned(),
                    Json::Object(vec![
                        ("at".to_owned(), site),
                        (
                            "initial".to_owned(),
                            Json::Object(vec![(
                                "mandate".to_owned(),
                                Json::Object(vec![
                                    ("retreat_hp_pct".to_owned(), retreat),
                                    self.mandate(),
                                ]),
                            )]),
                        ),
                    ]),
                ));
            }
            2 => {
                let ms = self.duration(20_000);
                fields.push(("hold".to_owned(), Json::Object(vec![("ms".to_owned(), ms)])));
            }
            3 => {
                let guard = self.condition(2);
                // A clean wait is timed, once: `guards` may have timed the
                // step already, and a field written twice is E0001.
                if self.clean && !fields.iter().any(|(key, _)| key == "timeout_ms") {
                    fields.push(("timeout_ms".to_owned(), Json::Number("30000".to_owned())));
                }
                fields.push((
                    "wait_until".to_owned(),
                    Json::Object(vec![("condition".to_owned(), guard)]),
                ));
            }
            4 => {
                let beacon = self.beacon_ref();
                let row = self.row();
                fields.push((
                    "interface".to_owned(),
                    Json::Object(vec![
                        ("beacon".to_owned(), beacon),
                        ("rows".to_owned(), Json::Array(vec![row])),
                    ]),
                ));
            }
            _ => {}
        }
    }

    /// One interface row. Recycling is what a damaged run reaches for; the
    /// core cannot be recycled, so a clean run sets a priority instead. The
    /// other four are the rows the estimate stage prices (S1's `proj`): a
    /// settings edit and a switch, each with a mandate's settings, a Build
    /// target and a queued structure.
    fn row(&mut self) -> Json {
        match self.below(5) {
            0 if self.clean => Json::Object(vec![(
                "set_priority".to_owned(),
                Json::String("NORMAL".to_owned()),
            )]),
            0 => Json::Object(vec![("recycle".to_owned(), Json::Object(Vec::new()))]),
            1 => {
                let (key, value) = self.mandate();
                Json::Object(vec![(
                    "set_mandate_settings".to_owned(),
                    Json::Object(vec![(key, value)]),
                )])
            }
            2 => {
                let (key, value) = self.mandate();
                Json::Object(vec![(
                    "set_mandate".to_owned(),
                    Json::Object(vec![(key, value)]),
                )])
            }
            3 => {
                let target = self.build_target();
                Json::Object(vec![(
                    "add_build_target".to_owned(),
                    Json::Object(vec![("target".to_owned(), target)]),
                )])
            }
            // A queued structure is refused until S4 (E0009).
            _ if self.clean => Json::Object(vec![(
                "set_priority".to_owned(),
                Json::String("NORMAL".to_owned()),
            )]),
            _ => {
                // S3's licensed blueprint one row in two, so the Spire's own
                // checks (E0507, E0513) are reached behind E0009.
                let blueprint = if self.rarely(2) {
                    Json::String("resonance_spire".to_owned())
                } else {
                    self.blueprint()
                };
                Json::Object(vec![(
                    "queue_structure".to_owned(),
                    Json::Object(vec![("blueprint_id".to_owned(), blueprint)]),
                )])
            }
        }
    }

    /// A mandate's settings, as the `(arm, block)` pair a settings message
    /// holds: Mine with nothing set, Survey with a scout count, or Build with
    /// one to three targets.
    fn mandate(&mut self) -> (String, Json) {
        // Attack's settings are legal since S2's `atk`, and Defend's since
        // S2's `dfnd`.
        if self.damage(6) {
            let arm = if self.rarely(2) { "defend" } else { "attack" };
            // Empty, or filled with S2's fields, damaged as a hand editor would.
            let settings = if self.rarely(3) {
                Json::Object(Vec::new())
            } else if arm == "defend" {
                self.defend()
            } else {
                self.attack()
            };
            return (arm.to_owned(), settings);
        }
        match self.below(3) {
            0 => ("mine".to_owned(), Json::Object(Vec::new())),
            1 => {
                let count = self.scout_count();
                (
                    "survey".to_owned(),
                    Json::Object(vec![("scout_count".to_owned(), count)]),
                )
            }
            _ => {
                let mut targets = Vec::new();
                for _ in 0..=self.below(3) {
                    targets.push(self.build_target());
                }
                (
                    "build".to_owned(),
                    Json::Object(vec![("targets".to_owned(), Json::Array(targets))]),
                )
            }
        }
    }

    /// A voxel anywhere in the `i32` field, the extremes included.
    fn wild_voxel(&mut self) -> Json {
        let axis = |shape: &mut Shape| {
            if shape.rarely(2) {
                let index = usize::try_from(shape.below(4)).unwrap_or(0);
                Json::Number(EXTREMES.get(index).copied().unwrap_or(0).to_string())
            } else {
                shape.number(-40, 420)
            }
        };
        Json::Object(vec![
            ("x".to_owned(), axis(self)),
            ("y".to_owned(), axis(self)),
            ("z".to_owned(), axis(self)),
        ])
    }

    /// An area: upright, inside out, or missing a corner.
    fn wild_area(&mut self) -> Json {
        let mut fields = Vec::new();
        if !self.rarely(5) {
            fields.push(("min".to_owned(), self.wild_voxel()));
        }
        if !self.rarely(5) {
            fields.push(("max".to_owned(), self.wild_voxel()));
        }
        Json::Object(fields)
    }

    /// An enum value by one of its names, or on a damaged run by a number it
    /// may not name.
    fn wild_enum(&mut self, names: &[&str]) -> Json {
        if self.damage(3) {
            return self.number(-3, 40);
        }
        let index =
            usize::try_from(self.below(i32::try_from(names.len()).unwrap_or(1))).unwrap_or(0);
        Json::String(names.get(index).copied().unwrap_or("NEVER").to_owned())
    }

    /// A list of up to `most` items.
    fn list(&mut self, most: i32, item: fn(&mut Shape) -> Json) -> Json {
        let count = if self.rarely(4) { most } else { self.below(4) };
        Json::Array((0..count).map(|_| item(self)).collect())
    }

    /// S2's Defend settings, with every field the wire gave them.
    fn defend(&mut self) -> Json {
        let protect = self.list(64, |shape| {
            if shape.rarely(3) {
                Json::Object(vec![("my_commander".to_owned(), Json::Object(Vec::new()))])
            } else {
                Json::Object(vec![("beacon".to_owned(), shape.beacon_ref())])
            }
        });
        let pursue = self.wild_enum(&["NEVER", "SPHERE_EDGE", "WHILE_IN_REACH"]);
        Json::Object(vec![
            ("protect".to_owned(), protect),
            ("engagement_radius_pct".to_owned(), self.number(0, 200)),
            ("pursue".to_owned(), pursue),
            ("picket_scouts".to_owned(), self.number(0, 300)),
        ])
    }

    /// S2's Attack settings, with every field the wire gave them.
    fn attack(&mut self) -> Json {
        let target = match self.below(3) {
            0 => Json::Object(vec![("beacon".to_owned(), self.beacon_ref())]),
            1 => Json::Object(vec![("voxel".to_owned(), self.wild_voxel())]),
            _ => {
                // Unset, or a seat: the playbook's own, an enemy's, or past any
                // match's (S2's `atk`; E0419 and E0418, and W0708 unset).
                let seat = match self.below(4) {
                    0 => Vec::new(),
                    1 => vec![("seat".to_owned(), Json::Number("0".to_owned()))],
                    2 => vec![("seat".to_owned(), Json::Number("1".to_owned()))],
                    _ => vec![("seat".to_owned(), Json::Number("256".to_owned()))],
                };
                Json::Object(vec![("enemy_commander".to_owned(), Json::Object(seat))])
            }
        };
        let retarget = self.wild_enum(&["NONE", "NEAREST_KNOWN"]);
        let after = self.wild_enum(&["REGROUP_AT_STAGING", "RETURN_HOME"]);
        let launch = Json::Object(vec![
            ("force_size".to_owned(), self.number(0, 50)),
            ("segment_ms".to_owned(), self.duration(480_000)),
        ]);
        Json::Object(vec![
            ("target".to_owned(), target),
            ("retarget".to_owned(), retarget),
            ("staging".to_owned(), self.wild_voxel()),
            ("launch".to_owned(), launch),
            ("after_action".to_owned(), after),
            ("avoid_areas".to_owned(), self.list(64, Shape::wild_area)),
            ("breach_allowed".to_owned(), Json::Bool(self.rarely(2))),
            ("waves".to_owned(), self.number(0, 9)),
        ])
    }

    /// A scout count over the whole `u32` field: a small one, the very top,
    /// or anywhere between, so the projection meets the counts no `i32` holds.
    fn scout_count(&mut self) -> Json {
        let anywhere = u32::try_from(self.rng.range_i32(0, i32::MAX)).unwrap_or(0);
        let count = match self.below(4) {
            0 => u32::try_from(self.below(12)).unwrap_or(0),
            1 => u32::MAX,
            2 => anywhere.saturating_mul(2).saturating_add(1),
            _ => anywhere,
        };
        Json::Number(count.to_string())
    }

    /// A predicate's blueprint id: on a clean run any kind that stands as a
    /// structure, since which beacon has which is the Push's and a predicate
    /// may ask about one not yet buildable; [`Shape::blueprint`]'s otherwise.
    fn structure_kind(&mut self) -> Json {
        if !self.clean {
            return self.blueprint();
        }
        let id = pick(
            self,
            &["generator", "autocannon", "mortar", "resonance_spire"],
        );
        Json::String(id.to_owned())
    }

    /// A blueprint id: one the rules table prices, or on a damaged run one it
    /// does not, which ends the projection at its step.
    fn blueprint(&mut self) -> Json {
        if self.damage(4) {
            return Json::String(UNPRICED.to_owned());
        }
        let index = usize::try_from(self.below(3)).unwrap_or(0);
        // Only the Generator is buildable before S4 (E0010).
        if self.clean {
            return Json::String("generator".to_owned());
        }
        // S2's base kit among them, the wall included.
        let index = if self.rarely(4) {
            self.below(2).saturating_add(3)
        } else {
            i32::try_from(index).unwrap_or(0)
        };
        Json::String(
            ["generator", "mortar", "survey_post", "autocannon", "wall"]
                .get(usize::try_from(index).unwrap_or(0))
                .copied()
                .unwrap_or("generator")
                .to_owned(),
        )
    }

    fn build_target(&mut self) -> Json {
        let blueprint = self.blueprint();
        // A target anchored on a beacon selector (E0011) is refused until S3.
        let anchor = if self.damage(6) {
            Json::Object(vec![("beacon_anchor".to_owned(), self.beacon_ref())])
        } else {
            Json::Object(vec![("voxel".to_owned(), self.voxel())])
        };
        let mut fields = vec![
            ("blueprint_id".to_owned(), blueprint),
            ("anchor".to_owned(), anchor),
        ];
        // A box on whatever blueprint was drawn (E0509 unless it is a wall).
        if self.damage(5) {
            fields.push(("wall_box".to_owned(), self.wild_area()));
        }
        // A turn, which a wall may not take (E0511) and nothing past 3 may.
        if self.damage(5) {
            fields.push(("rotation_quarter_turns".to_owned(), self.number(0, 5)));
        }
        Json::Object(fields)
    }

    fn handler(&mut self) -> Json {
        let mut fields: Vec<(String, Json)> = Vec::new();
        let id = self.handler_id();
        fields.push(("id".to_owned(), Json::String(id)));
        if !self.damage(6) {
            let when = self.condition(3);
            fields.push(("when".to_owned(), when));
        }
        let body_count = self.below(3);
        let mut body: Vec<Json> = Vec::new();
        for _ in 0..body_count {
            body.push(self.entry());
        }
        fields.push(("body".to_owned(), Json::Array(body)));
        if !self.damage(6) {
            fields.push(("resume".to_owned(), Json::String("CONTINUE".to_owned())));
        }
        let cooldown = if self.clean {
            self.number(5_000, 40_000)
        } else {
            self.number(-1_000, 40_000)
        };
        fields.push(("cooldown_ms".to_owned(), cooldown));
        let fires = if self.clean {
            self.number(1, 8)
        } else {
            self.number(0, 12)
        };
        fields.push(("max_fires".to_owned(), fires));
        Json::Object(fields)
    }

    fn playbook(&mut self) -> Json {
        let mut fields: Vec<(String, Json)> = Vec::new();
        if !self.damage(20) {
            let major = if self.clean {
                Json::Number("1".to_owned())
            } else {
                self.number(0, 2)
            };
            fields.push((
                "schema_version".to_owned(),
                Json::Object(vec![("major".to_owned(), major)]),
            ));
        }
        if !self.damage(20) {
            let author = if self.damage(8) { "SCRIPT" } else { "HUMAN" };
            fields.push((
                "meta".to_owned(),
                Json::Object(vec![
                    ("title".to_owned(), Json::String("fuzz".to_owned())),
                    ("author_kind".to_owned(), Json::String(author.to_owned())),
                ]),
            ));
        }
        if !self.damage(12) {
            fields.push(("kind".to_owned(), Json::String("PLAYBOOK".to_owned())));
        }
        let route_count = self.below(5).saturating_add(1);
        let mut route: Vec<Json> = Vec::new();
        for _ in 0..route_count {
            route.push(self.entry());
        }
        // Sizes at and past [`PADDED_PAST`]: holds, one unit each.
        if self.padded {
            for _ in 0..self.below(90).saturating_add(240) {
                let label = self.label();
                route.push(Json::Object(vec![
                    ("label".to_owned(), Json::String(label)),
                    (
                        "hold".to_owned(),
                        Json::Object(vec![("ms".to_owned(), Json::Number("1000".to_owned()))]),
                    ),
                ]));
            }
        }
        // Now and then more handlers than the sim compiles (E0113).
        let handler_count = if self.damage(40) {
            self.quiet = true;
            65
        } else {
            self.below(3)
        };
        let mut handlers: Vec<Json> = Vec::new();
        for _ in 0..handler_count {
            handlers.push(self.handler());
        }
        let mut body = vec![
            ("route".to_owned(), Json::Array(route)),
            ("handlers".to_owned(), Json::Array(handlers)),
        ];
        // The one option (spec section 7), so `W0603` is reached as well as
        // `E0601`.
        if self.rarely(4) {
            body.push((
                "options".to_owned(),
                Json::Object(vec![("allow_dormant_beacons".to_owned(), Json::Bool(true))]),
            ));
        }
        fields.push(("declarative".to_owned(), Json::Object(body)));
        if !self.damage(10) {
            fields.push((
                "on_death".to_owned(),
                Json::Object(vec![
                    ("on_respawn".to_owned(), Json::String("CONTINUE".to_owned())),
                    ("max_deaths_before_fallback".to_owned(), self.number(0, 4)),
                ]),
            ));
        }
        if !self.damage(10) {
            let place = self.location();
            fields.push((
                "fallback".to_owned(),
                Json::Object(vec![(
                    "hold".to_owned(),
                    Json::Object(vec![("at".to_owned(), place)]),
                )]),
            ));
        }
        if !self.quiet && self.damage(15) {
            let nonsense = self.number(0, 9);
            fields.push(("nonsense".to_owned(), nonsense));
        }
        Json::Object(fields)
    }
}

/// One of `names`, drawn evenly.
fn pick<'a>(shape: &mut Shape, names: &[&'a str]) -> &'a str {
    let count = i32::try_from(names.len()).unwrap_or(1);
    let index = usize::try_from(shape.below(count)).unwrap_or(0);
    names.get(index).copied().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The fixture
// ---------------------------------------------------------------------------

fn rules() -> RulesTable {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/verifier sits two levels below the workspace root"))
        .to_path_buf();
    RulesTable::load(&root.join("rules").join("rules.v1.json"))
        .unwrap_or_else(|error| panic!("reading the rules table: {error}"))
}

/// The committed rules table with the size budget at [`PADDED_PAST`], the
/// most a later stage can choose (S3's plan, E24), so a padded run can
/// qualify and the compile cross-check in [`check`] reaches it (the review of
/// PR #179, item 9). Every other run is checked against the committed table.
fn rules_at_the_widest_budget(rules: &RulesTable) -> RulesTable {
    let mut message = rules.message().clone();
    let verifier = message
        .verifier
        .as_mut()
        .unwrap_or_else(|| panic!("the committed rules carry the verifier's block"));
    verifier.size_budget_units = PADDED_PAST;
    RulesTable::from_message(&message).unwrap_or_else(|error| panic!("the widened rules: {error}"))
}

/// The S3 constructs `pred` lifts, by the field a playbook writes and the
/// code that refused each until then (`pharmakos_verifier::REFUSED_CONSTRUCTS`
/// while it still lists the code).
const S3_CONSTRUCTS: [(&str, &str); 9] = [
    (r#""beacon_units_by_role""#, "E0020"),
    (r#""beacon_mandate""#, "E0021"),
    (r#""beacon_has_structure""#, "E0022"),
    (r#""enemy_units_near""#, "E0023"),
    (r#""enemy_beacon_known""#, "E0024"),
    (r#""enemy_structure_near""#, "E0025"),
    (r#""kw_supply""#, "E0026"),
    (r#""kw_draw""#, "E0027"),
    (r#""toward""#, "E0028"),
];

/// What one run came to: its FULL report, whether the draw property applied,
/// whether a QUICK pass qualified it, and its text.
struct Run {
    report: VerifyReport,
    applied: bool,
    quick: bool,
    padded: bool,
    text: String,
}

/// The names the scope's features carry, for a `feature_id` that names one.
const FEATURE_IDS: [&str; 4] = ["vent_150_25", "vent_84_12", "vent_300_300", "seam_200_60"];

fn feature(
    id: &str,
    kind: FeatureKind,
    at: (i32, i32),
    live: bool,
    by: Option<&str>,
) -> KnownFeature {
    KnownFeature {
        feature_id: id.to_owned(),
        kind,
        grade: Richness::Standard,
        x: at.0,
        y: at.1,
        live,
        covered_by: by.map(str::to_owned),
    }
}

/// Seat 0's core, and one feature of each state the lints read: a live vent
/// nobody covers, a live vent the core covers (inside the core's sphere, where
/// a clean run's voxels sit, so W0706 fires), a lost vent and a seam.
fn scope() -> Scope {
    scope_aged(0, WINDOW)
}

/// [`scope`] with no enemy beacon seen inside the window (S2's `atk2`): the
/// two fresh beacon sightings aged past it, so the nearest known enemy beacon
/// as an Attack target meets `W0709`, which [`scope`]'s fresh `e_01`
/// silences.
fn stale_scope() -> Scope {
    scope_aged(WINDOW.saturating_add(1), WINDOW.saturating_add(2))
}

/// [`scope`] with the Resonance licence held and a Spire standing at the
/// core, `b_01` (S3's `vfy5`), so a route's queued Spire there meets `E0513`
/// and none meets `E0507`.
fn licensed_scope() -> Scope {
    scope()
        .with_licence(pharmakos_proto::gp::v1::Licence::ResonanceSpire)
        .and_then(|scope| scope.with_spire("b_01"))
        .unwrap_or_else(|error| panic!("the licensed fuzz scope builds: {error}"))
}

/// The fuzz scope with `e_01` and `e_02` seen `first` and `second` ms ago.
fn scope_aged(first: u32, second: u32) -> Scope {
    Scope::new(
        SeatId::new(0),
        SeatEconomy {
            treasury: Money::new(TREASURY),
            supply: Kw::new(SUPPLY),
            draw: Kw::new(DRAW),
        },
    )
    .with_commander(Voxel {
        x: 82,
        y: 13,
        z: 55,
    })
    .with_beacon(KnownBeacon {
        beacon_id: "b_01".to_owned(),
        owner: SeatId::new(0),
        side: Ownership::Own,
        mandate: MandateKind::Build,
        tags: Vec::new(),
        at: Voxel {
            x: 80,
            y: 11,
            z: 55,
        },
        is_core: true,
    })
    .with_beacon(enemy("e_01", 0))
    .with_beacon(enemy("e_02", 1))
    .with_beacon(enemy("e_03", 2))
    .with_beacon(enemy("e_04", 3))
    .with_sighting(seen(Sighted::Beacon("e_01".to_owned()), first))
    .with_sighting(seen(Sighted::Beacon("e_02".to_owned()), second))
    .with_sighting(seen(
        Sighted::Beacon("e_03".to_owned()),
        WINDOW.saturating_add(1),
    ))
    .with_sighting(seen(Sighted::Beacon("e_04".to_owned()), u32::MAX))
    .with_sighting(seen(Sighted::Commander(SeatId::new(0)), 0))
    .with_sighting(seen(
        Sighted::Commander(SeatId::new(1)),
        WINDOW.saturating_add(1),
    ))
    .with_sighting(seen(Sighted::Commander(SeatId::new(2)), u32::MAX))
    .with_feature(feature(
        "vent_150_25",
        FeatureKind::Vent,
        (150, 25),
        true,
        None,
    ))
    .with_feature(feature(
        "vent_84_12",
        FeatureKind::Vent,
        (84, 12),
        true,
        Some("b_01"),
    ))
    .with_feature(feature(
        "vent_300_300",
        FeatureKind::Vent,
        (300, 300),
        false,
        None,
    ))
    .with_feature(feature(
        "seam_200_60",
        FeatureKind::Seam,
        (200, 60),
        true,
        None,
    ))
}

/// An enemy beacon the seat knows, the `offset`th along the far corner.
fn enemy(id: &str, offset: i32) -> KnownBeacon {
    KnownBeacon {
        beacon_id: id.to_owned(),
        owner: SeatId::new(1),
        side: Ownership::EnemyKnown,
        mandate: MandateKind::Unspecified,
        tags: Vec::new(),
        at: Voxel {
            x: 300_i32.saturating_add(offset),
            y: 300,
            z: 40,
        },
        is_core: false,
    }
}

/// A sighting of `what`, this many ms old: the ages straddle the window (0,
/// the window, one past it, and the top of `u32`), and the seat's own
/// commander is sighted too, which never stands in for an enemy one.
fn seen(what: Sighted, age_ms: u32) -> Sighting {
    Sighting {
        what,
        at: Voxel {
            x: 300,
            y: 300,
            z: 40,
        },
        age_ms,
    }
}

/// A snapshot with a first round's three minutes coming, so the schedule
/// check (`W0701`) runs on every generated route.
fn snapshot() -> Vec<u8> {
    Snapshot {
        version: SNAPSHOT_VERSION,
        coming_segment_ms: 180_000,
        ..Snapshot::default()
    }
    .to_bytes()
    .unwrap_or_else(|error| panic!("encoding the fixture snapshot: {error}"))
}

/// Verify one generated playbook and assert every invariant a report has.
///
/// The flag says whether the draw property below applied, so a run can show
/// that it was not vacuous.
#[allow(
    clippy::too_many_lines,
    reason = "one run's every invariant in order; split, an invariant could be checked on a different run"
)]
fn check(
    index: u32,
    committed: &RulesTable,
    widest: &RulesTable,
    scope: &Scope,
    snapshot: &[u8],
) -> Run {
    let mut shape = Shape::new(index);
    let padded = shape.padded;
    let rules = if padded { widest } else { committed };
    let text = json::write(&shape.playbook());

    // The decode stage reads a playbook through the direct reader and falls
    // back to the exact path only to diagnose one that does not decode. The
    // direct reader must answer whenever the exact path decodes, or the
    // fallback would quietly carry every playbook and only a timing would
    // show it (PR #177's review).
    if let Ok(exact) =
        json::read(&text).and_then(|value| json::decode_json_encoded::<Playbook>(&value))
    {
        assert_eq!(
            json::decode_direct::<Playbook>(&text),
            json::Direct::Wire(exact),
            "run {index}: the direct reader left a playbook the exact path decodes to the exact \
             path"
        );
    }

    let input = Input::new(text.as_bytes(), snapshot, scope, rules)
        .unwrap_or_else(|error| panic!("assembling the input: {error}"));
    let report = verify(&input, Depth::Full);
    // QUICK runs FULL's first four stages and nothing else, so it qualifies
    // at least what FULL qualifies; the compile cross-check below reads QUICK's
    // answer, the wider of the two (the review of PR #179, item 9).
    let quick = verify(&input, Depth::Quick).qualifies;
    assert!(
        quick || !report.qualifies,
        "run {index}: FULL qualifies what QUICK refuses"
    );

    assert_eq!(report.verifier_version, VERIFIER_VERSION, "run {index}");
    assert_eq!(report.report_hash.len(), 8, "run {index}");
    assert_eq!(report.rules_hash.len(), 8, "run {index}");
    assert!(report.size_budget > 0, "run {index}");

    let errors = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == i32::from(Severity::Error))
        .count();
    assert_eq!(
        report.qualifies,
        errors == 0,
        "run {index}: a playbook qualifies exactly when it has zero errors"
    );

    for diagnostic in &report.diagnostics {
        assert!(
            CATALOGUE.iter().any(|row| row.code == diagnostic.code),
            "run {index}: `{}` is not in the catalogue",
            diagnostic.code
        );
        assert!(
            !diagnostic.message.is_empty() && !diagnostic.beginner.is_empty(),
            "run {index}: `{}` came back without its words",
            diagnostic.code
        );
        assert!(
            diagnostic.path.is_empty() || diagnostic.path.starts_with('/'),
            "run {index}: `{}` has a path that is not a JSON Pointer: {}",
            diagnostic.code,
            diagnostic.path
        );
        for suggestion in &diagnostic.suggestions {
            assert!(
                json::read(&suggestion.json_patch).is_ok(),
                "run {index}: `{}` carries a suggestion that is not JSON",
                diagnostic.code
            );
        }
    }

    // The report is a message the gateway hands back unchanged, so it has to be
    // writable as canonical JSON whatever went into it.
    json::encode(&report).unwrap_or_else(|error| panic!("run {index}: {error}"));

    // S2's E8, over every generated playbook (S3's `vfy6`): one the verifier
    // calls qualifying, the sim's compile accepts, so no file passes the
    // editor and is refused at the seal. When S3's `pred` lifts its
    // refusals, the predicates and sites the generator writes are held to it.
    if quick {
        let decoded: Playbook = json::decode(&text)
            .unwrap_or_else(|error| panic!("run {index}: qualifies and does not decode: {error}"));
        if let Err(error) = Plan::compile(&decoded, rules) {
            panic!("run {index}: qualifies and the sim's compile refuses it: {error}\n{text}");
        }
    }

    // The estimate stage's one refusal can never be silenced: a route whose
    // deploys certainly order more draw than the grid has room for carries
    // `E0601`, or `W0603` where the playbook accepts the shortfall, however
    // large the count.
    let mut applied = false;
    // The codes after which no later stage runs: the file did not decode
    // (E0001), named a field the schema does not have (E0002) or a word held
    // back (E0003). The rest of the decode family (E0004 to E0008, and E0018,
    // an enum number this build does not name) let the stages run, so the
    // property below holds over them too; the constructs E0009 to E0017 are
    // structure's.
    let decoded = !report
        .diagnostics
        .iter()
        .any(|diagnostic| ["E0001", "E0002", "E0003"].contains(&diagnostic.code.as_str()));
    if decoded && let Ok(playbook) = json::decode::<Playbook>(&text) {
        let per_unit = i128::from(
            rules
                .message()
                .power
                .as_ref()
                .map_or(0, |power| power.kw_per_unit),
        );
        let room = i128::from(SUPPLY.saturating_sub(DRAW));
        if certain_scout_draw(&playbook, per_unit) > room {
            applied = true;
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == "E0601" || diagnostic.code == "W0603"),
                "run {index}: the route's deploys order draw beyond supply and nothing says so"
            );
        }
    }
    Run {
        report,
        applied,
        quick,
        padded,
        text,
    }
}

/// A reference figure for the property above, written apart from the
/// verifier's: the draw of the scouts the route's deploys order, up to the
/// first step that names a blueprint the rules table does not price, where the
/// projection stops. It counts less than the verifier does (no switch, no
/// structure), so it is a lower bound of what the verifier must see.
fn certain_scout_draw(playbook: &Playbook, per_unit: i128) -> i128 {
    let unpriced = |targets: &[BuildTarget]| {
        targets
            .iter()
            .any(|target| !PRICED.contains(&target.blueprint_id.as_str()))
    };
    let settings_unpriced = |settings: Option<&MandateSettings>| {
        matches!(
            settings.and_then(|settings| settings.mandate.as_ref()),
            Some(mandate_settings::Mandate::Build(build)) if unpriced(&build.targets)
        )
    };
    let mut draw: i128 = 0;
    for entry in playbook
        .declarative
        .as_ref()
        .map_or(&[][..], |body| body.route.as_slice())
    {
        match entry.kind.as_ref() {
            Some(step::Kind::PlaceBeacon(place)) => {
                let settings = place
                    .initial
                    .as_ref()
                    .and_then(|initial| initial.mandate.as_ref());
                if settings_unpriced(settings) {
                    break;
                }
                if let Some(mandate_settings::Mandate::Survey(survey)) =
                    settings.and_then(|settings| settings.mandate.as_ref())
                {
                    draw = draw
                        .saturating_add(per_unit.saturating_mul(i128::from(survey.scout_count)));
                }
            }
            Some(step::Kind::Interface(visit)) => {
                let stops = visit.rows.iter().any(|row| match row.row.as_ref() {
                    Some(
                        interface_row::Row::SetMandate(settings)
                        | interface_row::Row::SetMandateSettings(settings),
                    ) => settings_unpriced(Some(settings)),
                    Some(interface_row::Row::AddBuildTarget(add)) => add
                        .target
                        .as_ref()
                        .is_some_and(|target| !PRICED.contains(&target.blueprint_id.as_str())),
                    Some(interface_row::Row::QueueStructure(queue)) => {
                        !PRICED.contains(&queue.blueprint_id.as_str())
                    }
                    _ => false,
                });
                if stops {
                    break;
                }
            }
            _ => {}
        }
    }
    draw
}

/// Counts `run` against each of [`S3_CONSTRUCTS`] whose field its text holds,
/// if QUICK qualified it. A run QUICK qualifies is one [`check`] has already
/// put through the sim's compile, which panics on a refusal, so each count is
/// of runs the compile accepted with the construct in them.
fn count_s3_constructs(counts: &mut [u32], run: &Run) {
    if !run.quick {
        return;
    }
    for (count, (field, _)) in counts.iter_mut().zip(S3_CONSTRUCTS) {
        *count = count.saturating_add(u32::from(run.text.contains(field)));
    }
}

/// Each S3 construct qualifies in some run once `pred` lifts its code, and in
/// none while its code still refuses it (S3's `pred` turns each on): the
/// nightly run and the smoke test both hold their counts to this, so a pull
/// request sees a construct the generator stops reaching.
///
/// Every construct is checked before any failure is reported, and the
/// failures are named together, so one red run names every construct that
/// went wrong.
fn each_s3_construct_qualifies_once_lifted(counts: &[u32], runs: u32) {
    assert_eq!(
        counts.len(),
        S3_CONSTRUCTS.len(),
        "a count for each S3 construct"
    );
    let mut checked = 0_usize;
    let mut failures: Vec<String> = Vec::new();
    for ((field, code), count) in S3_CONSTRUCTS.iter().zip(counts) {
        checked = checked.saturating_add(1);
        let refused = pharmakos_verifier::REFUSED_CONSTRUCTS
            .iter()
            .any(|(_, refusing)| refusing == code);
        println!("{field}: {count} of {runs} runs qualify ({code} refusing: {refused})");
        if refused && *count != 0 {
            failures.push(format!("{field} qualified while {code} refuses it"));
        } else if !refused && *count == 0 {
            failures.push(format!(
                "{field} never qualified in {runs} runs once {code} stopped refusing it"
            ));
        }
    }
    assert_eq!(
        checked,
        S3_CONSTRUCTS.len(),
        "every S3 construct was checked"
    );
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

/// Hand-written bases for [`every_one_node_edit_that_qualifies_compiles`]:
/// each qualifies in [`scope`] and carries a construct the generator does not
/// write qualifying (a patrolling fallback; a Build target under each of its
/// three carriers; a removal), so the edits reach every block those
/// constructs require.
const EDIT_BASES: [&str; 2] = [
    concat!(
        r#"{"schema_version":{"major":1},"meta":{"title":"patrol","author_kind":"HUMAN"},"#,
        r#""kind":"PLAYBOOK","declarative":{"route":[{"label":"s1","hold":{"ms":1000}}]},"#,
        r#""on_death":{"on_respawn":"CONTINUE","max_deaths_before_fallback":1},"#,
        r#""fallback":{"patrol":{"waypoints":[{"voxel":{"x":80,"y":12,"z":55}},"#,
        r#"{"beacon_anchor":{"beacon_id":"b_01"}}]}}}"#
    ),
    concat!(
        r#"{"schema_version":{"major":1},"meta":{"title":"targets","author_kind":"HUMAN"},"#,
        r#""kind":"PLAYBOOK","declarative":{"route":[{"label":"s1","on_fail":{"action":"SKIP"},"#,
        r#""interface":{"beacon":{"beacon_id":"b_01"},"rows":["#,
        r#"{"set_mandate_settings":{"build":{"targets":[{"blueprint_id":"generator","#,
        r#""anchor":{"voxel":{"x":82,"y":12,"z":55}}}]}}},"#,
        r#"{"add_build_target":{"target":{"blueprint_id":"generator","#,
        r#""anchor":{"voxel":{"x":78,"y":10,"z":55}}}}},"#,
        r#"{"remove_build_target":{"anchor":{"voxel":{"x":82,"y":12,"z":55}}}}]}}],"#,
        r#""options":{"allow_dormant_beacons":true}},"#,
        r#""on_death":{"on_respawn":"CONTINUE","max_deaths_before_fallback":1},"#,
        r#""fallback":{"hold":{"at":{"beacon_anchor":{"safest":{}}}}}}"#
    ),
];

/// How many of the generator's runs [`every_one_node_edit_that_qualifies_compiles`]
/// edits, beside [`EDIT_BASES`].
const EDITED_RUNS: u32 = 96;

/// Every copy of `root` with one node replaced by `null`, `{}`, `[]`, `0` or
/// `""`, or removed: the population of "a block written empty or left out"
/// that the sim's compile reads as required, one node at a time.
fn one_node_edits(root: &Json) -> Vec<Json> {
    fn walk(node: &Json, path: &mut Vec<usize>, root: &Json, out: &mut Vec<Json>) {
        let children: Vec<&Json> = match node {
            Json::Object(members) => members.iter().map(|(_, value)| value).collect(),
            Json::Array(items) => items.iter().collect(),
            _ => Vec::new(),
        };
        for (index, child) in children.into_iter().enumerate() {
            path.push(index);
            for with in [
                Some(Json::Null),
                Some(Json::Object(Vec::new())),
                Some(Json::Array(Vec::new())),
                Some(Json::Number("0".to_owned())),
                Some(Json::String(String::new())),
                None,
            ] {
                out.push(edited(root, path, with));
            }
            walk(child, path, root, out);
            path.pop();
        }
    }
    fn edited(root: &Json, path: &[usize], with: Option<Json>) -> Json {
        let mut copy = root.clone();
        let Some((last, parents)) = path.split_last() else {
            return copy;
        };
        let mut node = &mut copy;
        for index in parents {
            node = match node {
                Json::Object(members) => members.get_mut(*index).map(|(_, value)| value),
                Json::Array(items) => items.get_mut(*index),
                _ => None,
            }
            .unwrap_or_else(|| panic!("the walk's path {path:?} is in the tree"));
        }
        match (node, with) {
            (Json::Object(members), Some(value)) => {
                if let Some(member) = members.get_mut(*last) {
                    member.1 = value;
                }
            }
            (Json::Object(members), None) => {
                members.remove(*last);
            }
            (Json::Array(items), Some(value)) => {
                if let Some(item) = items.get_mut(*last) {
                    *item = value;
                }
            }
            (Json::Array(items), None) => {
                items.remove(*last);
            }
            _ => panic!("the walk's path {path:?} ends under a container"),
        }
        copy
    }
    let mut out = Vec::new();
    walk(root, &mut Vec::new(), root, &mut out);
    out
}

/// What qualifies at FULL, the sim compiles (S3's `vfyq2`, fix pass 1, item
/// 6). The security review's generator found 16 of 240,000 playbooks the
/// verifier qualified and `Plan::compile` refused, every one a block the
/// compile requires and the structure stage never asked for: a patrol with no
/// waypoint, an `add_build_target` with no target, a Build target with no
/// anchor or an anchor naming no place. This edits every node of qualifying
/// playbooks, one at a time, and holds every edit that still qualifies to the
/// sim's compile, so a required block the verifier does not require goes red
/// here, by the error the compile names.
#[test]
fn every_one_node_edit_that_qualifies_compiles() {
    let rules = rules();
    let widest = rules_at_the_widest_budget(&rules);
    let scope = scope();
    let snapshot = snapshot();
    let qualifies = |text: &str, rules: &RulesTable| {
        let input = Input::new(text.as_bytes(), &snapshot, &scope, rules)
            .unwrap_or_else(|error| panic!("assembling the input: {error}"));
        verify(&input, Depth::Full).qualifies
    };
    let mut bases: Vec<(Json, bool)> = Vec::new();
    for base in EDIT_BASES {
        let tree = json::read(base).unwrap_or_else(|error| panic!("an edit base: {error}"));
        assert!(
            qualifies(base, &rules),
            "an edit base does not qualify: {base}"
        );
        bases.push((tree, false));
    }
    for index in 0..EDITED_RUNS {
        let mut shape = Shape::new(index);
        let padded = shape.padded;
        let tree = shape.playbook();
        if qualifies(&json::write(&tree), if padded { &widest } else { &rules }) {
            bases.push((tree, padded));
        }
    }
    let mut refused: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut checked = 0_u32;
    for (tree, padded) in &bases {
        let rules = if *padded { &widest } else { &rules };
        for edit in one_node_edits(tree) {
            let text = json::write(&edit);
            if !qualifies(&text, rules) {
                continue;
            }
            checked = checked.saturating_add(1);
            let playbook: Playbook = json::decode(&text)
                .unwrap_or_else(|error| panic!("qualifies and does not decode: {error}\n{text}"));
            if let Err(error) = Plan::compile(&playbook, rules) {
                let flat: String = text.split_whitespace().collect();
                let slot = refused
                    .entry(format!("{error:?}"))
                    .or_insert_with(|| flat.clone());
                if flat.len() < slot.len() {
                    *slot = flat;
                }
            }
        }
    }
    assert!(
        bases.len() > EDIT_BASES.len() && checked > 0,
        "the property was never tested: {} bases, {checked} qualifying edits",
        bases.len()
    );
    let report: Vec<String> = refused
        .iter()
        .map(|(error, text)| format!("{error}, the smallest: {text}"))
        .collect();
    assert!(
        report.is_empty(),
        "{} kinds of edit qualify at FULL and the sim's compile refuses them:\n{}",
        report.len(),
        report.join("\n")
    );
}

/// The replacements a seeded edit draws from: the structural ones of
/// [`one_node_edits`] (`None` removes) and the security review's rich values.
fn replacements() -> Vec<Option<Json>> {
    vec![
        Some(Json::Null),
        Some(Json::Object(Vec::new())),
        Some(Json::Array(Vec::new())),
        Some(Json::Number("0".to_owned())),
        Some(Json::String(String::new())),
        None,
        Some(Json::Number("1".to_owned())),
        Some(Json::Number("-1".to_owned())),
        Some(Json::Number("2147483647".to_owned())),
        Some(Json::Number("-2147483648".to_owned())),
        Some(Json::Number("4294967295".to_owned())),
        Some(Json::Bool(true)),
        Some(Json::String("b_01".to_owned())),
        Some(Json::String("x".to_owned())),
    ]
}

/// Every node's path in `root`, the root left out.
fn node_paths(node: &Json, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    let children: Vec<&Json> = match node {
        Json::Object(members) => members.iter().map(|(_, value)| value).collect(),
        Json::Array(items) => items.iter().collect(),
        _ => Vec::new(),
    };
    for (index, child) in children.into_iter().enumerate() {
        path.push(index);
        out.push(path.clone());
        node_paths(child, path, out);
        path.pop();
    }
}

/// `root` with the node at `path` replaced by `with`, or removed.
fn edited_at(root: &Json, path: &[usize], with: Option<Json>) -> Json {
    let mut copy = root.clone();
    let Some((last, parents)) = path.split_last() else {
        return copy;
    };
    let mut node = &mut copy;
    for index in parents {
        node = match node {
            Json::Object(members) => members.get_mut(*index).map(|(_, value)| value),
            Json::Array(items) => items.get_mut(*index),
            _ => None,
        }
        .unwrap_or_else(|| panic!("the path {path:?} is in the tree"));
    }
    match (node, with) {
        (Json::Object(members), Some(value)) => {
            if let Some(member) = members.get_mut(*last) {
                member.1 = value;
            }
        }
        (Json::Object(members), None) => {
            members.remove(*last);
        }
        (Json::Array(items), Some(value)) => {
            if let Some(item) = items.get_mut(*last) {
                *item = value;
            }
        }
        (Json::Array(items), None) => {
            items.remove(*last);
        }
        _ => panic!("the path {path:?} ends under a container"),
    }
    copy
}

/// One seeded edit of `root`: a node drawn from its paths, and a replacement
/// drawn from [`replacements`].
fn seeded_edit(root: &Json, rng: &mut StreamRng) -> Json {
    let mut paths = Vec::new();
    node_paths(root, &mut Vec::new(), &mut paths);
    let choices = replacements();
    let draw = |rng: &mut StreamRng, count: usize| {
        let most = i32::try_from(count.saturating_sub(1))
            .unwrap_or_else(|_| panic!("{count} choices do not fit a draw"));
        usize::try_from(rng.range_i32(0, most))
            .unwrap_or_else(|_| panic!("a draw below zero from 0..={most}"))
    };
    let Some(path) = paths.get(draw(rng, paths.len())) else {
        return root.clone();
    };
    let Some(with) = choices.get(draw(rng, choices.len())).cloned() else {
        panic!("a draw past the replacements");
    };
    edited_at(root, path, with)
}

/// What [`two_node_edits`] found.
struct TwoNode {
    /// How many cases qualified at FULL.
    qualified: u32,
    /// Every compile refusal of a qualifying case, by kind, with its smallest
    /// text: each one a defect.
    refused: std::collections::BTreeMap<String, String>,
    /// For every `PlanError` variant, how many generated cases the compile
    /// refused with it, the verifier bypassed (S3's `vfyq2`, fix pass 3, item
    /// M5): what the probe reaches at all.
    reached: std::collections::BTreeMap<&'static str, u32>,
}

/// Two seeded edits of qualifying playbooks, `cases` of them, each that still
/// qualifies at FULL held to the sim's compile (S3's `vfyq2`, fix pass 2,
/// item C2: the security review's two-node probe, committed). Counted, not
/// timed.
fn two_node_edits(cases: u32) -> TwoNode {
    two_node_edits_under(cases, &[])
}

/// [`two_node_edits`], each case judged and compiled under `tables` in turn
/// when there are any (fix pass 3, item M4), or under the committed rules
/// (and the widest budget for a padded base) when there are none.
fn two_node_edits_under(cases: u32, tables: &[RulesTable]) -> TwoNode {
    let rules = rules();
    let widest = rules_at_the_widest_budget(&rules);
    let scope = scope();
    let snapshot = snapshot();
    let qualifies = |text: &str, rules: &RulesTable| {
        let input = Input::new(text.as_bytes(), &snapshot, &scope, rules)
            .unwrap_or_else(|error| panic!("assembling the input: {error}"));
        verify(&input, Depth::Full).qualifies
    };
    let mut bases: Vec<(Json, bool)> = Vec::new();
    for base in EDIT_BASES {
        bases.push((
            json::read(base).unwrap_or_else(|error| panic!("an edit base: {error}")),
            false,
        ));
    }
    for index in 0..EDITED_RUNS {
        let mut shape = Shape::new(index);
        let padded = shape.padded;
        let tree = shape.playbook();
        if qualifies(&json::write(&tree), if padded { &widest } else { &rules }) {
            bases.push((tree, padded));
        }
    }
    // The PlanError table's texts, one per reachable variant (fix pass 3,
    // item M5): a probe seeded only with qualifying playbooks reached 16 of
    // the 38. Each is refused as it stands; an edit of it is a near miss of
    // that shape, which is where a gap between the verifier and the compile
    // would be.
    for (name, text) in plan_error_table::texts() {
        bases.push((
            json::read(&text).unwrap_or_else(|error| panic!("{name}: {error}")),
            false,
        ));
    }
    let mut rng = StreamRng::new(FUZZ_SEED, Stream::Map, u32::MAX, 2, 2);
    let mut checked = 0_u32;
    let mut refused: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut reached: std::collections::BTreeMap<&'static str, u32> =
        std::collections::BTreeMap::new();
    // Every base as it stands, under every table: no edit at all.
    for (tree, _) in &bases {
        let text = json::write(tree);
        for table in tables {
            if !qualifies(&text, table) {
                continue;
            }
            checked = checked.saturating_add(1);
            let playbook: Playbook = json::decode(&text)
                .unwrap_or_else(|error| panic!("qualifies and does not decode: {error}\n{text}"));
            if let Err(error) = Plan::compile(&playbook, table) {
                let flat: String = text.split_whitespace().collect();
                refused.entry(format!("{error:?}")).or_insert(flat);
            }
        }
    }
    for case in 0..cases {
        let Some((tree, padded)) = usize::try_from(case)
            .ok()
            .and_then(|case| case.checked_rem(bases.len()))
            .and_then(|index| bases.get(index))
        else {
            panic!("case {case} names no base of {}", bases.len());
        };
        // Each base meets every table in turn: the table turns once the
        // cases have been round every base.
        let table = usize::try_from(case)
            .ok()
            .and_then(|case| case.checked_div(bases.len()))
            .and_then(|round| round.checked_rem(tables.len()))
            .and_then(|index| tables.get(index));
        let rules = match table {
            Some(table) => table,
            None if *padded => &widest,
            None => &rules,
        };
        let first_edit = seeded_edit(tree, &mut rng);
        let second_edit = seeded_edit(&first_edit, &mut rng);
        let text = json::write(&second_edit);
        // The verifier bypassed: what the compile alone says of the case.
        if let Ok(playbook) = json::decode::<Playbook>(&text)
            && let Err(error) = Plan::compile(&playbook, rules)
        {
            let (variant, _) = plan_error_table::classify(&error);
            let count = reached.entry(variant).or_insert(0);
            *count = count.saturating_add(1);
        }
        if !qualifies(&text, rules) {
            continue;
        }
        checked = checked.saturating_add(1);
        let playbook: Playbook = json::decode(&text)
            .unwrap_or_else(|error| panic!("qualifies and does not decode: {error}\n{text}"));
        if let Err(error) = Plan::compile(&playbook, rules) {
            let flat: String = text.split_whitespace().collect();
            let slot = refused
                .entry(format!("{error:?}"))
                .or_insert_with(|| flat.clone());
            if flat.len() < slot.len() {
                *slot = flat;
            }
        }
    }
    TwoNode {
        qualified: checked,
        refused,
        reached,
    }
}

/// How many two-node cases every ordinary run tries; the nightly run tries
/// the review's 240,000.
const TWO_NODE_CASES: u32 = 12_000;

#[test]
fn every_two_node_edit_that_qualifies_compiles() {
    let TwoNode {
        qualified,
        refused,
        reached,
    } = two_node_edits(TWO_NODE_CASES);
    // Every variant the verifier must refuse first is reached by the probe,
    // the verifier bypassed, or the probe could not find the verifier missing
    // it (fix pass 3, item M5).
    let mut table: Vec<String> = Vec::new();
    let mut unreached: Vec<&'static str> = Vec::new();
    for sample in plan_error_table::every_variant() {
        let (variant, row) = plan_error_table::classify(&sample);
        let count = reached.get(variant).copied().unwrap_or(0);
        table.push(format!("{variant}: {count}"));
        if matches!(row, plan_error_table::Row::Refused(_)) && count == 0 {
            unreached.push(variant);
        }
    }
    println!(
        "two-node probe, cases reaching each PlanError with the verifier bypassed:\n{}",
        table.join("\n")
    );
    assert!(
        unreached.is_empty(),
        "the two-node probe never reaches {} reachable PlanError variant(s): {unreached:?}",
        unreached.len()
    );
    println!("{TWO_NODE_CASES} two-node edits: {qualified} qualified");
    assert!(
        qualified > 0,
        "no two-node edit qualified: the property is vacuous"
    );
    assert!(
        refused.is_empty(),
        "{} kinds of two-node edit qualify at FULL and the sim's compile refuses them:\n{:#?}",
        refused.len(),
        refused
    );
}

/// Rules tables a probe judges and compiles under (fix pass 3, item M4): the
/// committed table with its verifier block's size budget, handler cooldown
/// floor and `max_fires` ceiling moved across what `RulesTable::from_message`
/// takes, a budget past the sim's route and handler ceilings among them.
fn generated_rules_tables() -> Vec<RulesTable> {
    let committed = rules();
    let mut tables = Vec::new();
    for (budget, cooldown, fires) in [
        (1, 0, 1),
        (64, 1_000, 3),
        (128, 30_000, 50),
        (400, 0, 1),
        (1_024, 5_000, 10),
        (4_096, 60_000, 100),
        (65_536, 1, 1_000),
        (u32::MAX, 0, u32::MAX),
    ] {
        let mut message = committed.message().clone();
        let verifier = message
            .verifier
            .as_mut()
            .unwrap_or_else(|| panic!("the committed rules carry the verifier's block"));
        verifier.size_budget_units = budget;
        verifier.handler_cooldown_min_ms = cooldown;
        verifier.max_fires_max = fires;
        tables.push(
            RulesTable::from_message(&message)
                .unwrap_or_else(|error| panic!("budget {budget}: {error}")),
        );
    }
    tables
}

/// "Qualifies implies compiles" under every generated rules table, crossed
/// with the two-node probe (fix pass 3, item M4): a verifier refusal that
/// reads a rules value the compile does not read (the size budget against
/// the sim's route ceiling was one) fails here.
#[test]
fn every_two_node_edit_that_qualifies_compiles_under_every_rules_table() {
    let tables = generated_rules_tables();
    let TwoNode {
        qualified, refused, ..
    } = two_node_edits_under(TWO_NODE_CASES, &tables);
    println!(
        "{TWO_NODE_CASES} two-node edits across {} rules tables: {qualified} qualified",
        tables.len()
    );
    assert!(
        qualified > 0,
        "no two-node edit qualified: the property is vacuous"
    );
    assert!(
        refused.is_empty(),
        "{} kinds of two-node edit qualify at FULL under some rules table and the sim's \
         compile refuses them:\n{:#?}",
        refused.len(),
        refused
    );
}

#[test]
#[ignore = "the security review's 240,000 cases; nightly-scenarios.yml's fuzz job calls it"]
fn two_hundred_forty_thousand_two_node_edits_that_qualify_compile() {
    let TwoNode {
        qualified, refused, ..
    } = two_node_edits(240_000);
    println!("240,000 two-node edits: {qualified} qualified");
    assert!(
        qualified > 0,
        "no two-node edit qualified: the property is vacuous"
    );
    assert!(
        refused.is_empty(),
        "{} kinds of two-node edit qualify at FULL and the sim's compile refuses them:\n{:#?}",
        refused.len(),
        refused
    );
}

#[test]
fn the_generator_still_produces_shapes_the_verifier_reads() {
    let rules = rules();
    let widest = rules_at_the_widest_budget(&rules);
    let scope = scope();
    let snapshot = snapshot();
    let mut qualified = 0_u32;
    let mut past = 0_u32;
    let mut constructs: Vec<u32> = vec![0; S3_CONSTRUCTS.len()];
    for index in 0..SMOKE_RUNS {
        let run = check(index, &rules, &widest, &scope, &snapshot);
        past = past.saturating_add(u32::from(run.report.size_units >= PADDED_PAST));
        qualified = qualified.saturating_add(u32::from(run.quick));
        count_s3_constructs(&mut constructs, &run);
    }
    each_s3_construct_qualifies_once_lifted(&constructs, SMOKE_RUNS);
    // A generator that never produces a valid playbook is a generator that only
    // ever exercises the decode stage, and a green fuzz run over it would mean
    // nothing (the lesson G3-prime wrote down about a sweep with nothing in it).
    assert!(
        qualified > 0,
        "none of {SMOKE_RUNS} generated playbooks qualified; the generator only reaches the \
         rejection paths"
    );
    assert!(
        past > 0,
        "no generated playbook reached {PADDED_PAST} size units"
    );
}

#[test]
#[ignore = "ten thousand verifications; nightly-scenarios.yml's fuzz job calls it"]
#[allow(
    clippy::too_many_lines,
    reason = "one tally over the ten thousand runs and its assertions; split, a code could be counted and not asserted"
)]
fn ten_thousand_playbooks_come_back_as_reports() {
    let rules = rules();
    let scope = scope();
    let stale = stale_scope();
    let licensed = licensed_scope();
    let snapshot = snapshot();
    let widest = rules_at_the_widest_budget(&rules);
    let mut padded_qualified: u32 = 0;
    let mut constructs: Vec<u32> = vec![0; S3_CONSTRUCTS.len()];
    let mut lints: Vec<String> = Vec::new();
    let mut estimates: Vec<String> = Vec::new();
    let mut refusals: Vec<String> = Vec::new();
    let mut s2: Vec<String> = Vec::new();
    let mut s3: Vec<String> = Vec::new();
    let mut vfy6: Vec<String> = Vec::new();
    let mut applied: u32 = 0;
    let mut past: u32 = 0;
    let window = pharmakos_verifier::Limits::from_rules(&rules)
        .unwrap_or_else(|gap| panic!("the limits: {gap}"))
        .reach_memory_ms();
    assert_eq!(
        u32::try_from(window).ok(),
        Some(WINDOW),
        "the fuzz scope's ages straddle the committed reach window"
    );
    for index in 0..RUNS {
        // One run in four against the stale scope, so `W0709` is reachable,
        // and one in four against the licensed one, so `E0513`'s standing
        // Spire is (S3's `vfy5`).
        let view = match index % 4 {
            3 => &stale,
            1 => &licensed,
            _ => &scope,
        };
        let run = check(index, &rules, &widest, view, &snapshot);
        applied = applied.saturating_add(u32::from(run.applied));
        if run.quick {
            padded_qualified = padded_qualified.saturating_add(u32::from(run.padded));
        }
        count_s3_constructs(&mut constructs, &run);
        let report = run.report;
        past = past.saturating_add(u32::from(report.size_units >= PADDED_PAST));
        for diagnostic in report.diagnostics {
            let code = diagnostic.code;
            let list = if ["W0704", "W0705", "W0706"].contains(&code.as_str()) {
                &mut lints
            } else if [
                "E0601", "E0603", "W0602", "W0603", "W0604", "W0701", "I0001",
            ]
            .contains(&code.as_str())
            {
                &mut estimates
            } else if REFUSALS.contains(&code.as_str()) {
                &mut refusals
            } else if S2_CODES.contains(&code.as_str()) {
                &mut s2
            } else if S3_CODES.contains(&code.as_str()) {
                &mut s3
            } else if VFY6_CODES.contains(&code.as_str()) {
                &mut vfy6
            } else {
                continue;
            };
            if !list.contains(&code) {
                list.push(code);
            }
        }
    }
    // Every code the estimate stage can raise against this fixture is
    // reached: `W0601` needs a grid already short, and `W0707` bytes that are
    // not a snapshot, and both have tests of their own.
    assert!(
        applied > 0,
        "no generated route ordered draw beyond supply; the property was never tested"
    );
    estimates.sort();
    assert_eq!(
        estimates,
        [
            "E0601", "E0603", "I0001", "W0602", "W0603", "W0604", "W0701"
        ],
        "the estimate codes the run reached"
    );
    // Every construct the sim refuses at the seal, and an enum number this
    // build does not name, is reached (S2's `vfy1`).
    refusals.sort();
    assert_eq!(refusals, REFUSALS, "the refusals the run reached");
    // Every code S2's `vfy2` added, or gave a new shape (E0508's picket), is
    // reached, so a green run says something about each of them.
    s2.sort();
    let mut wanted = S2_CODES.to_vec();
    wanted.sort_unstable();
    assert_eq!(s2, wanted, "the vfy2 codes the run reached");
    // Every code S3's `vfy5` emits is reached too.
    s3.sort();
    let mut wanted = S3_CODES.to_vec();
    wanted.sort_unstable();
    assert_eq!(s3, wanted, "the vfy5 codes the run reached");
    // And every code S3's `vfy6` added or gave its emitter.
    vfy6.sort();
    let mut wanted = VFY6_CODES.to_vec();
    wanted.sort_unstable();
    assert_eq!(vfy6, wanted, "the vfy6 codes the run reached");
    // Sizes at and past any budget a later stage can choose.
    assert!(
        past > 0,
        "no generated playbook reached {PADDED_PAST} size units"
    );
    // And against the widest budget, some padded playbook qualifies, so the
    // compile cross-check reaches a file that long.
    assert!(
        padded_qualified > 0,
        "no padded playbook qualified against a budget of {PADDED_PAST}"
    );
    each_s3_construct_qualifies_once_lifted(&constructs, RUNS);
    // Every emitting branch of targeting's three lints is reached, so a green
    // run says something about each of them.
    lints.sort();
    assert_eq!(
        lints,
        ["W0704", "W0705", "W0706"],
        "the lints the run reached"
    );
}

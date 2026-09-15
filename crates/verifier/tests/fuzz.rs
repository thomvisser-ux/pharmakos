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
//! inner loop. **T20 wires it into `.github/workflows/nightly-scenarios.yml`**,
//! behind that workflow's existing variable gate: `.github/workflows/**` is a
//! contract path (AGENTS.md section 5) and belongs to no wave-2 agent, so this
//! task ships the target and T20 ships the call. `the_generator_still_produces_
//! shapes_the_verifier_reads` runs a small slice of the same generator on every
//! ordinary `cargo test`, so the generator cannot rot between now and then.
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

use pharmakos_proto::gp::api::v1::diagnostic::Severity;
use pharmakos_proto::gp::api::v1::verify_plan::Depth;
use pharmakos_proto::json::{self, Json};
use pharmakos_sim::knowledge::SeatEconomy;
use pharmakos_sim::math::random::{Stream, StreamRng};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot};
use pharmakos_sim::tables::SeatId;
use pharmakos_verifier::catalogue::CATALOGUE;
use pharmakos_verifier::{Input, KnownBeacon, Ownership, Scope, VERIFIER_VERSION, verify};

use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::gp::v1::beacon_filter::MandateKind;

/// How many playbooks the nightly run generates.
const RUNS: u32 = 10_000;

/// How many the ordinary `cargo test` run generates, so the generator is never
/// dead code between nightly runs.
const SMOKE_RUNS: u32 = 64;

/// An arbitrary, pinned fuzz seed. It is a *harness* seed and not a game value.
const FUZZ_SEED: u64 = 0x5052_4F42_4C45_4D53;

// ---------------------------------------------------------------------------
// The generator
// ---------------------------------------------------------------------------

/// Names the generator draws from, chosen so some resolve and some do not.
const NAMES: &[&str] = &[
    "s0", "s1", "s2", "h1", "b_01", "b_02", "e_01", "nowhere", "",
];

/// Words `gp.v1` holds back, so the never-strip path is exercised.
const HELD: &[&str] = &["set_flag", "branch", "repeat", "dispatch", "team_id"];

struct Shape {
    rng: StreamRng,
    /// When set, the generator damages nothing: every required block is there,
    /// every name is unique, every duration is positive, every voxel is on the
    /// map and every reference resolves.
    ///
    /// A generator that only ever produces rejected files exercises the decode
    /// stage and nothing else, and a green run over it would mean nothing — the
    /// lesson spike G3-prime wrote down about a sweep with nothing in it. One
    /// run in four is clean, so the later stages are reached too.
    clean: bool,
    labels: u32,
    handlers: u32,
}

impl Shape {
    fn new(index: u32) -> Shape {
        Shape {
            rng: StreamRng::new(FUZZ_SEED, Stream::Map, index, 0, 0),
            clean: index % 4 == 0,
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
        let index = usize::try_from(self.below(9)).unwrap_or(0);
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
            return if self.rarely(2) {
                Json::Object(vec![(
                    "beacon_id".to_owned(),
                    Json::String("b_01".to_owned()),
                )])
            } else {
                Json::Object(vec![("safest".to_owned(), Json::Object(Vec::new()))])
            };
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
        match self.below(3) {
            0 => Json::Object(vec![("voxel".to_owned(), self.voxel())]),
            1 => Json::Object(vec![("beacon_anchor".to_owned(), self.beacon_ref())]),
            _ => Json::Object(vec![("safest".to_owned(), Json::Object(Vec::new()))]),
        }
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
        // A clean run keeps to the three predicates that need no reference to
        // resolve; the rest are what the resolve stage is for.
        let families = if self.clean { 3 } else { 6 };
        match self.below(families) {
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

    fn int_compare(&mut self) -> Json {
        let mut fields: Vec<(String, Json)> = Vec::new();
        if !self.damage(5) {
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
        if self.damage(12) {
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
                fields.push((
                    "move".to_owned(),
                    Json::Object(vec![("to".to_owned(), place)]),
                ));
            }
            1 => {
                let site = Json::Object(vec![("voxel".to_owned(), self.voxel())]);
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
                                    ("mine".to_owned(), Json::Object(Vec::new())),
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
                if self.clean {
                    fields.push(("timeout_ms".to_owned(), Json::Number("30000".to_owned())));
                }
                fields.push((
                    "wait_until".to_owned(),
                    Json::Object(vec![("condition".to_owned(), guard)]),
                ));
            }
            4 => {
                let beacon = self.beacon_ref();
                // Recycling is what a damaged run reaches for; the core cannot
                // be recycled, so a clean run sets a priority instead.
                let row = if self.clean {
                    Json::Object(vec![(
                        "set_priority".to_owned(),
                        Json::String("NORMAL".to_owned()),
                    )])
                } else {
                    Json::Object(vec![("recycle".to_owned(), Json::Object(Vec::new()))])
                };
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
        let handler_count = self.below(3);
        let mut handlers: Vec<Json> = Vec::new();
        for _ in 0..handler_count {
            handlers.push(self.handler());
        }
        fields.push((
            "declarative".to_owned(),
            Json::Object(vec![
                ("route".to_owned(), Json::Array(route)),
                ("handlers".to_owned(), Json::Array(handlers)),
            ]),
        ));
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
        if self.damage(15) {
            let nonsense = self.number(0, 9);
            fields.push(("nonsense".to_owned(), nonsense));
        }
        Json::Object(fields)
    }
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

fn scope() -> Scope {
    Scope::new(SeatId::new(0), SeatEconomy::default()).with_beacon(KnownBeacon {
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
}

fn snapshot() -> Vec<u8> {
    Snapshot {
        version: SNAPSHOT_VERSION,
        ..Snapshot::default()
    }
    .to_bytes()
    .unwrap_or_else(|error| panic!("encoding the fixture snapshot: {error}"))
}

/// Verify one generated playbook and assert every invariant a report has.
fn check(index: u32, rules: &RulesTable, scope: &Scope, snapshot: &[u8]) {
    let text = json::write(&Shape::new(index).playbook());
    let input = Input::new(text.as_bytes(), snapshot, scope, rules)
        .unwrap_or_else(|error| panic!("assembling the input: {error}"));
    let report = verify(&input, Depth::Full);

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
}

#[test]
fn the_generator_still_produces_shapes_the_verifier_reads() {
    let rules = rules();
    let scope = scope();
    let snapshot = snapshot();
    let mut qualified = 0_u32;
    for index in 0..SMOKE_RUNS {
        check(index, &rules, &scope, &snapshot);
        let text = json::write(&Shape::new(index).playbook());
        let input =
            Input::new(text.as_bytes(), &snapshot, &scope, &rules).expect("the input assembles");
        if verify(&input, Depth::Full).qualifies {
            qualified = qualified.saturating_add(1);
        }
    }
    // A generator that never produces a valid playbook is a generator that only
    // ever exercises the decode stage, and a green fuzz run over it would mean
    // nothing (the lesson G3-prime wrote down about a sweep with nothing in it).
    assert!(
        qualified > 0,
        "none of {SMOKE_RUNS} generated playbooks qualified; the generator only reaches the \
         rejection paths"
    );
}

#[test]
#[ignore = "ten thousand verifications; the nightly workflow calls it at T20"]
fn ten_thousand_playbooks_come_back_as_reports() {
    let rules = rules();
    let scope = scope();
    let snapshot = snapshot();
    for index in 0..RUNS {
        check(index, &rules, &scope, &snapshot);
    }
}

// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The deterministic simulation: the authoritative world and the only thing
//! that advances time. Role from spec section 15 (Architecture), simulation
//! layer.
//!
//! # What lives here
//!
//! * **The world** — the `SoA` tables (seats, units, beacons, structures,
//!   wrecks), the 32³ copy-on-write chunk store ([`voxels`]) and the seeded
//!   deterministic map generator ([`mapgen`]) that fills all of them from
//!   `(match seed, rules table, occupied seats)`.
//! * **The determinism core** — the integer newtypes, the split seeded RNG
//!   streams, the canonical encoding, the per-tick state hash, the
//!   snapshot/restore round-trip, the `SoA` tables and the CSR broadphase.
//!   Decisions log item 70 settled that there is **no `math` crate**: the
//!   determinism contract (items 48–51, 62, 66) lives in one crate, which is
//!   what makes "the determinism code" a nameable contract path for
//!   AGENTS.md §5.
//! * **The tick** — twelve named phases in a fixed order ([`world::PHASE_ORDER`]),
//!   several of them still empty and each naming the task that fills it.
//! * **The playbook interpreter** — [`interpreter`]: the compiled plan, the
//!   decision every 250 ms of game time, one rule body at a time, the fixed
//!   20 % reflex, the route steps and their guards, late-bound selectors, the
//!   interface rows that commit at the end of their own durations, and the
//!   guaranteed tail. Its per-seat state is hashed; the plans are inputs.
//! * **The match** — [`runner`]: Lull, Push and recap in their fixed order, the
//!   segment ladder, the frozen segment-end snapshot, the one-tick match-end
//!   rule, elimination and the commander's respawn. [`runner::Runner`] is what
//!   a host drives; [`runner::MatchState`] is the hashed state it moves.
//! * **The event bus** — [`events`]: what a tick reports, in a total order,
//!   under a name a scenario file can assert on. Derived output, not hashed
//!   state, and that module says why.
//! * **The public type surface** — [`snapshot`], [`knowledge`] and [`rules`],
//!   which `plan-core`, `verifier`, `gateway` and `operator` compile against
//!   with `default-features = false`.
//! * **The seams** spec section 15 keeps for later: [`seams::Operator`], the
//!   abstract per-tick work counter, and the `program_id` on a beacon's
//!   mandate.
//! * **`fork`**, behind `feature = "research"` and nowhere else.
//!
//! # Rules this crate is held to
//!
//! The sim is a pure function of `(map seed, playbooks, rules hash)`. Two
//! machines running the same binary on Windows, Linux and macOS must produce
//! the **same per-tick xxh3 hash chain**, and every rule below exists because
//! breaking it produces a desync that shows up days later as an unreproducible
//! replay (AGENTS.md §4):
//!
//! * integer newtypes, never bare numbers; no floats; no `as` casts outside the
//!   audited widening conversions in [`math`];
//! * no `HashMap`/`HashSet`, no wall-clock time — `tests/confinement.rs`
//!   asserts both against the crate's own **source text**, which is the one
//!   lesson G2 and G3′ both wrote down: copy the test, do not merely rely on
//!   the lint;
//! * ordered iteration always, and **every sort key ends in a unique id**
//!   (item 62). The `(f, h, node_id)` binary min-heap that convention was fixed
//!   for arrives with T7's HPA\*; the convention binds every sort in the crate
//!   from today;
//! * split seeded RNG streams, never a global generator;
//! * overflow checks on in every profile, so a wrap is a panic rather than a
//!   silent platform difference — and nothing in a tick phase panics, because
//!   every arithmetic form here says what it does at the limit;
//! * **any field added to sim state must be added to three places in the same
//!   pull request**: the state hash, the snapshot/restore round-trip, and the
//!   golden files.
//!
//! # The determinism binary
//!
//! `cargo run -p pharmakos-sim --bin determinism -- --ticks N --out FILE` writes
//! the per-tick chain that `cargo xtask ci`'s `determinism` step compares
//! against `tests/golden/determinism/expected.hashes.txt`.

pub mod chunks;
pub mod credit;
pub mod economy;
pub mod encoding;
pub mod events;
pub mod interpreter;
pub mod knowledge;
pub mod mandate;
pub mod mapgen;
pub mod math;
pub mod pathing;
pub mod power;
pub mod programs;
pub mod rules;
pub mod runner;
pub mod seams;
pub mod snapshot;
pub mod survey;
pub mod tables;
pub mod voxels;
pub mod world;

// `fork` is behind the feature at the *module* level, not just the function, so
// a default build does not compile a line of it.
#[cfg(feature = "research")]
pub mod research;

pub use economy::{Purchase, Quartermaster, SingleTreasury, SpendRequest, Urgency};
pub use encoding::{ENCODING_VERSION, Enc, STATE_HASH_SEED, digest, hex};
pub use events::{EVENT_BUS_CAPACITY, Event, EventBus, EventKind};
pub use interpreter::{Interpreter, Plan, PlanError, PlanState, REFLEX_HP_PERCENT};
pub use mandate::{Mandate, mandate_for};
pub use mapgen::{GeneratedMap, MapError, MapFile, MapReport};
pub use pathing::{
    Clusters, Estimate, Fog, Node, Router, Scratch, Speed, Surface, WalkState, estimate,
};
pub use power::PowerRules;
pub use programs::{Program, program_for};
pub use rules::{RULES_PATH, RulesError, RulesTable};
pub use runner::{
    DEFAULT_ROUND_LIMIT, FrozenSnapshot, MatchEndReason, MatchOutcome, MatchPhase, MatchSettings,
    MatchState, PHASE_CYCLE, Runner, TickReport,
};
pub use snapshot::{SNAPSHOT_VERSION, Snapshot, SnapshotError};
pub use voxels::{CHUNK_EDGE, CHUNK_VOXELS, Material, Richness, VoxelEdit, VoxelStore};
pub use world::{DamageOrder, DamageTarget, PHASE_ORDER, Phase, World, WorldConfig};

use pharmakos_proto::gp;

/// The match seed the determinism harness runs on.
///
/// Arbitrary and pinned. It is a *harness* seed, not a game constant: T20
/// replaces the harness run with a real segment and re-baselines the chain,
/// explaining the movement in that pull request.
pub const DETERMINISM_MATCH_SEED: u64 = 0x0102_0304_0506_0708;

/// Seats the determinism harness runs.
///
/// Four, one more than v1's three, deliberately: the widest table the hash can
/// be asked to cover is the one worth pinning, and per-seat apportionment with
/// three possible damagers is what exercises the kill-credit cap when T14 adds
/// it (G4's own reasoning, and its reason for running four seats).
pub const DETERMINISM_SEATS: u32 = 4;

/// Units per seat in the determinism harness.
pub const DETERMINISM_UNITS_PER_SEAT: u32 = 50;

/// PLACEHOLDER (harness): the per-round segment length list the determinism
/// harness plays, in game milliseconds.
///
/// Item 40 makes the per-round length list a **host** setting, and the harness
/// is a host: it sets short segments so that `cargo xtask ci`'s 1 200-tick run
/// covers whole segments and the phase changes between them, instead of sitting
/// 1 200 ticks into the first three-minute Push of item 68's real ladder and
/// never reaching a boundary. 20 000 ms is 400 ticks and 15 000 ms is 300, so
/// the committed chain covers `push → recap → lull → push` three times over.
///
/// Deleted when `DETERMINISM_TICKS` is raised from 1 200 to a real segment
/// (owner, at T20 — the same PLACEHOLDER T2 left in `xtask`). Whoever raises it
/// should know what is waiting on it: the plan's T10 acceptance line asks for
/// "a golden hash chain over a full segment", and the committed chain covers
/// three whole segments of *this* list rather than one of item 68's real
/// ladder, whose first round alone is 3 600 ticks. Raising `DETERMINISM_TICKS`
/// and deleting this constant is what turns that substitution into the real
/// thing.
pub const DETERMINISM_SEGMENT_LENGTHS_MS: [i32; 2] = [20_000, 15_000];

/// Find `rules/rules.v1.json` from wherever the caller happens to stand.
///
/// Two probes, both relative: the repository root (where `cargo xtask` and the
/// determinism binary run) and two levels up from it (where `cargo test` puts a
/// crate's working directory). Relative rather than `CARGO_MANIFEST_DIR` so
/// nothing bakes a build machine's absolute path into a shipped binary.
#[must_use]
pub fn default_rules_path() -> Option<std::path::PathBuf> {
    let from_root = std::path::PathBuf::from(RULES_PATH);
    if from_root.is_file() {
        return Some(from_root);
    }
    let from_crate = std::path::Path::new("..").join("..").join(RULES_PATH);
    if from_crate.is_file() {
        return Some(from_crate);
    }
    None
}

/// What stopped a world being built.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WorldError {
    /// The rules table could not be read, or is not one the sim can use.
    Rules(RulesError),
    /// The rules table cannot describe a map, or a world on one.
    Map(MapError),
    /// A playbook could not be compiled for this build.
    Plan(PlanError),
}

impl std::fmt::Display for WorldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorldError::Rules(error) => write!(f, "{error}"),
            WorldError::Map(error) => write!(f, "{error}"),
            WorldError::Plan(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for WorldError {}

impl From<RulesError> for WorldError {
    fn from(error: RulesError) -> WorldError {
        WorldError::Rules(error)
    }
}

impl From<MapError> for WorldError {
    fn from(error: MapError) -> WorldError {
        WorldError::Map(error)
    }
}

impl From<PlanError> for WorldError {
    fn from(error: PlanError) -> WorldError {
        WorldError::Plan(error)
    }
}

/// Build the world the determinism harness and its goldens run on, with the
/// harness playbook sealed for every seat.
///
/// One constructor, used by the binary and by every test, so a golden can never
/// disagree with the run that produced it.
///
/// # Errors
///
/// Returns [`WorldError`] when the rules table cannot be read from `path`,
/// cannot describe a map, or cannot compile [`determinism_playbook`].
pub fn determinism_world(rules_path: &std::path::Path) -> Result<World, WorldError> {
    let rules = RulesTable::load(rules_path)?;
    let plan = interpreter::Plan::compile(&determinism_playbook(), &rules)?;
    let mut world = World::new(&WorldConfig {
        match_seed: DETERMINISM_MATCH_SEED,
        seats: DETERMINISM_SEATS,
        units_per_seat: DETERMINISM_UNITS_PER_SEAT,
        rules,
        match_settings: MatchSettings {
            segment_lengths_ms: DETERMINISM_SEGMENT_LENGTHS_MS.to_vec(),
            round_limit: DEFAULT_ROUND_LIMIT,
        },
    })?;
    let mut seat: u32 = 0;
    while seat < DETERMINISM_SEATS {
        let id = tables::SeatId::new(u8::try_from(seat).unwrap_or(u8::MAX));
        world.seal_playbook(id, plan.clone());
        seat = seat.saturating_add(1);
    }
    Ok(world)
}

/// PLACEHOLDER (harness): the playbook every seat seals in the determinism run.
///
/// It is a *harness* playbook, not a game one, and it is written in code rather
/// than read from a file for the reason the harness's seeds are constants: the
/// determinism binary must produce its chain from nothing but the rules table.
/// What it is for is coverage — the chain has to run the interpreter's state
/// through its shapes, so the harness plays a route that uses a **late-bound
/// selector**, a **visit with a committed row**, a **hold**, a **wait on a
/// clock predicate** and a **handler with a cooldown and a fire limit**, and
/// then falls through to its guaranteed tail. Every place is a selector rather
/// than a voxel, so the same file is legal for every seat on every spawn.
///
/// It is deliberately *not* `examples/playbooks/expand_east.jsonc`: that file
/// names voxels in one seat's corner of one map, and the harness runs four
/// seats. The worked example is exercised by the scenario file instead
/// (`scenarios/skeleton/expand-east-segment.scenario.jsonc`).
///
/// Deleted when `DETERMINISM_TICKS` is raised to a real segment and the chain
/// covers a real playbook (owner, at T20, with `DETERMINISM_SEGMENT_LENGTHS_MS`).
#[must_use]
pub fn determinism_playbook() -> gp::v1::Playbook {
    use gp::v1::{
        Declarative, Fallback, FallbackHold, Meta, OnDeath, Playbook, SchemaVersion, meta,
        on_death, playbook,
    };

    Playbook {
        schema_version: Some(SchemaVersion { major: 1, minor: 0 }),
        meta: Some(Meta {
            title: "Determinism harness".to_owned(),
            author_kind: i32::from(meta::AuthorKind::Builtin),
            ..Meta::default()
        }),
        kind: i32::from(playbook::Kind::Playbook),
        declarative: Some(Declarative {
            route: harness_route(),
            handlers: harness_handlers(),
            options: None,
        }),
        on_death: Some(OnDeath {
            on_respawn: i32::from(on_death::OnRespawn::Continue),
            max_deaths_before_fallback: 2,
        }),
        fallback: Some(Fallback {
            posture: Some(gp::v1::fallback::Posture::Hold(FallbackHold {
                at: Some(safest_place()),
            })),
        }),
    }
}

/// "the safest own beacon", as a place. Every target in the harness playbook is
/// a selector rather than a voxel, so the same file is legal for every seat on
/// every spawn.
fn safest_place() -> gp::v1::Location {
    gp::v1::Location {
        place: Some(gp::v1::location::Place::Safest(gp::v1::Safest {})),
    }
}

/// "the safest own beacon", as a beacon reference.
fn safest_beacon() -> gp::v1::BeaconRef {
    gp::v1::BeaconRef {
        r#ref: Some(gp::v1::beacon_ref::Ref::Safest(gp::v1::Safest {})),
    }
}

/// `on_fail: SKIP` — the harness never wants a failed step to end its route,
/// because the chain is about the interpreter running, not about it giving up.
fn skip_on_fail() -> gp::v1::OnFail {
    gp::v1::OnFail {
        action: i32::from(gp::v1::on_fail::Action::Skip),
        jump_to_label: String::new(),
    }
}

/// The harness playbook's route: a walk to a selector, a visit with a committed
/// row, a hold and a wait on a clock predicate.
fn harness_route() -> Vec<gp::v1::Step> {
    use gp::v1::{
        Condition, HoldStep, IntCompare, InterfaceRow, InterfaceStep, MoveStep, SegmentElapsed,
        Step, WaitUntilStep, condition, int_compare, interface_row, move_step, step,
    };
    vec![
        Step {
            label: "to_core".to_owned(),
            timeout_ms: 60_000,
            on_fail: Some(skip_on_fail()),
            kind: Some(step::Kind::Move(MoveStep {
                to: Some(safest_place()),
                pace: i32::from(move_step::Pace::Direct),
            })),
            ..Step::default()
        },
        // The one step that adds a row to a table **inside a tick**, which is
        // why it is in the harness playbook: `BeaconTable::reserve` is what
        // keeps that row from being an allocation (G3′ §9.17), and
        // `tests/allocations.rs` can only assert it over a deploy it actually
        // performs. The site is the `safest` selector, like every other place
        // here, so the same file is legal for every seat on every spawn — the
        // commander has just walked to that beacon, so the site is inside its
        // own sphere and within placement range.
        Step {
            label: "deploy".to_owned(),
            timeout_ms: 30_000,
            on_fail: Some(skip_on_fail()),
            kind: Some(step::Kind::PlaceBeacon(gp::v1::PlaceBeaconStep {
                at: Some(safest_place()),
                tags: Vec::new(),
                // A writ and no settings: choosing the mandate type is free at
                // deploy time (spec section 5), so this costs the deploy and
                // nothing more, and the new beacon carries Mine rather than no
                // writ at all.
                initial: Some(gp::v1::InitialSettings {
                    priority: 0,
                    mandate: Some(gp::v1::MandateSettings {
                        mandate: Some(gp::v1::mandate_settings::Mandate::Mine(
                            gp::v1::MineSettings::default(),
                        )),
                        ..gp::v1::MandateSettings::default()
                    }),
                }),
            })),
            ..Step::default()
        },
        Step {
            label: "touch".to_owned(),
            timeout_ms: 30_000,
            on_fail: Some(skip_on_fail()),
            kind: Some(step::Kind::Interface(InterfaceStep {
                beacon: Some(safest_beacon()),
                rows: vec![InterfaceRow {
                    row: Some(interface_row::Row::SetPriority(i32::from(
                        interface_row::QuartermasterPriority::Normal,
                    ))),
                }],
            })),
            ..Step::default()
        },
        Step {
            label: "settle".to_owned(),
            kind: Some(step::Kind::Hold(HoldStep { ms: 2_000 })),
            ..Step::default()
        },
        Step {
            label: "watch".to_owned(),
            timeout_ms: 20_000,
            on_fail: Some(skip_on_fail()),
            kind: Some(step::Kind::WaitUntil(WaitUntilStep {
                condition: Some(Condition {
                    node: Some(condition::Node::SegmentElapsed(SegmentElapsed {
                        ms: Some(IntCompare {
                            op: i32::from(int_compare::Op::Ge),
                            value: 12_000,
                        }),
                    })),
                }),
            })),
            ..Step::default()
        },
    ]
}

/// The harness playbook's one handler: a condition that is always true, a
/// cooldown and a fire limit, so the chain covers a rule body running and
/// stopping.
fn harness_handlers() -> Vec<gp::v1::Handler> {
    use gp::v1::{
        Condition, Handler, HoldStep, IntCompare, Step, Treasury, condition, handler, int_compare,
        step,
    };
    vec![Handler {
        id: "stocktake".to_owned(),
        when: Some(Condition {
            node: Some(condition::Node::Treasury(Treasury {
                dollars: Some(IntCompare {
                    op: i32::from(int_compare::Op::Ge),
                    value: 0,
                }),
            })),
        }),
        body: vec![Step {
            label: "pause".to_owned(),
            kind: Some(step::Kind::Hold(HoldStep { ms: 1_000 })),
            ..Step::default()
        }],
        resume: i32::from(handler::Resume::Continue),
        cooldown_ms: 5_000,
        max_fires: 2,
        ..Handler::default()
    }]
}

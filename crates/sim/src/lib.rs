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

pub mod audit;
pub mod chunks;
pub mod credit;
pub mod economy;
pub mod encoding;
pub mod events;
pub mod features;
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
pub mod sight;
pub mod snapshot;
pub mod survey;
pub mod tables;
pub mod targeting;
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
pub use sight::Spheres;
pub use snapshot::{SNAPSHOT_VERSION, Snapshot, SnapshotError};
pub use voxels::{CHUNK_EDGE, CHUNK_VOXELS, Material, Richness, VoxelEdit, VoxelStore};
pub use world::{DamageOrder, DamageTarget, PHASE_ORDER, Phase, World, WorldConfig};

use pharmakos_proto::gp;

/// The match seed the determinism harness runs on.
///
/// Arbitrary and pinned. It is a *harness* seed, not a game constant: the pull
/// request that replaces the harness run with a real segment re-baselines the
/// chain and explains the movement (owner, at S1: decisions-log item 116 (6)(e)
/// kept the harness run through T20, because T20's demo scenario, item 116
/// (6)(c), carries the full-segment chain the plan's T10 line asks for).
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
/// (owner, at S1; decisions-log item 116 (6)(e) kept the harness run through
/// T20, and `DETERMINISM_TICKS` in `xtask/src/main.rs` carries its own
/// PLACEHOLDER). The plan's T10 acceptance line asks for "a golden hash chain
/// over a full segment". This chain covers three whole segments of *this* list
/// rather than one of item 68's real ladder, whose first round alone is 3 600
/// ticks; item 116 (6)(e) takes T20's demo scenario chain (item 116 (6)(c), a
/// 180 s first round compared across the three operating systems) as that
/// full-segment chain, so the raise is not what T10's line waits on.
pub const DETERMINISM_SEGMENT_LENGTHS_MS: [i32; 2] = [20_000, 15_000];

/// PLACEHOLDER (harness): how far east of its core, in whole voxels, each
/// seat's harness deploy stands.
///
/// S1's no-stacking rule makes the harness's old site -- the `safest` beacon's
/// own anchor, the core it had just walked to -- `illegal_site`
/// (`docs/design/targeting.md`, "Companion changes"), so the deploy moved to a
/// legal site: three voxels east of the seat's core, inside the core's sphere,
/// within the interface range of the core itself so the walk in is nothing,
/// and never on the core's column. Short enough that the deploy still lands
/// inside the harness's 20-second first segment and inside
/// `tests/allocations.rs`'s counted window. It goes with S1-26's real
/// determinism segment (owner, at S1, the `tune` lane), when the harness
/// playbook is replaced by a committed template's instantiation.
pub const DETERMINISM_SITE_OFFSET_VOXELS: i32 = 3;

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
    // How many seats sealed is the tests' to assert (`tests/determinism.rs`,
    // `tests/allocations.rs`); the loop seals exactly the world's own seats.
    let _sealed = seal_determinism_playbooks(&mut world)?;
    Ok(world)
}

/// Seal every seat's harness playbook ([`determinism_playbook`]) into `world`,
/// each deploying at its own seat's harness site ([`determinism_site`]).
///
/// The one sealing path the determinism binary, `tests/determinism.rs` and
/// `tests/allocations.rs` all share, so the chain they produce is one run.
///
/// Answers how many seats took their seal ([`World::seal_playbook`] refuses a
/// seat the world does not have), so a caller asserts that every seat sealed
/// rather than inferring it from the interpreter's length, which counts seats
/// whether or not anything was sealed for them.
///
/// # Errors
///
/// Returns [`PlanError`] when this build cannot compile the harness playbook.
pub fn seal_determinism_playbooks(world: &mut World) -> Result<u32, PlanError> {
    let seats = world.seats().len();
    let mut seat: u32 = 0;
    let mut sealed: u32 = 0;
    while seat < seats {
        let id = tables::SeatId::new(u8::try_from(seat).unwrap_or(u8::MAX));
        let plan = interpreter::Plan::compile(
            &determinism_playbook(determinism_site(world, id)),
            world.rules(),
        )?;
        if world.seal_playbook(id, plan) {
            sealed = sealed.saturating_add(1);
        }
        seat = seat.saturating_add(1);
    }
    Ok(sealed)
}

/// Where `seat`'s harness deploy stands: [`DETERMINISM_SITE_OFFSET_VOXELS`]
/// east of its core, at the core's height (the interpreter stands a voxel site
/// on the ground under it). A seat with no core -- the harness's fourth seat,
/// seated but unplaced on a three-zone map -- gets the map's origin, which it
/// never walks to because it has no commander.
#[must_use]
pub fn determinism_site(world: &World, seat: tables::SeatId) -> [i32; 3] {
    let beacons = world.beacons();
    let Some(core) = beacons.row_of_ordinal(seat, 0) else {
        return [0, 0, 0];
    };
    let at = beacons
        .positions()
        .get(core)
        .copied()
        .unwrap_or([math::fixed::Fx::ZERO; 3]);
    let axis = |index: usize| at.get(index).map_or(0, |value| value.floor_voxels());
    [
        axis(0).saturating_add(DETERMINISM_SITE_OFFSET_VOXELS),
        axis(1),
        axis(2),
    ]
}

/// PLACEHOLDER (harness): the playbook every seat seals in the determinism run,
/// deploying at `site`.
///
/// It is a *harness* playbook, not a game one, and it is written in code rather
/// than read from a file for the reason the harness's seeds are constants: the
/// determinism binary must produce its chain from nothing but the rules table.
/// What it is for is coverage — the chain has to run the interpreter's state
/// through its shapes, so the harness plays a route that uses a **late-bound
/// selector**, a **deploy**, a **visit with a committed row**, a **hold**, a
/// **wait on a clock predicate**, a **`covering` site** (S1's resolver, its
/// "nearest" estimates and its spiral, inside the tick) and a **handler with a
/// cooldown and a fire limit**, and then falls through to its guaranteed
/// tail. Every place but the deploy's is a selector or a description; the
/// deploy's is the seat's own [`determinism_site`], because S1's no-stacking
/// rule refuses the selector it used to name (`safest`, a beacon's own
/// column), and that is the one thing the file now varies by seat.
///
/// It is deliberately *not* `examples/playbooks/expand_east.jsonc`: that file
/// names voxels in one seat's corner of one map, and the harness runs four
/// seats. The worked example is exercised by the scenario file instead
/// (`scenarios/skeleton/expand-east-segment.scenario.jsonc`).
///
/// Deleted when `DETERMINISM_TICKS` is raised to a real segment and the chain
/// covers a real playbook (owner, at S1, with `DETERMINISM_SEGMENT_LENGTHS_MS`;
/// decisions-log item 116 (6)(e)).
#[must_use]
pub fn determinism_playbook(site: [i32; 3]) -> gp::v1::Playbook {
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
            route: harness_route(site),
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

/// The harness playbook's route: a walk to a selector, a deploy at `site`, a
/// visit with a committed row, a hold, a wait on a clock predicate and a
/// `covering` placement that resolves and walks until the segment ends.
#[allow(
    clippy::too_many_lines,
    reason = "one block per route step of the harness playbook, each a proto literal; splitting it would scatter one route across six functions"
)]
fn harness_route(site: [i32; 3]) -> Vec<gp::v1::Step> {
    use gp::v1::{
        Condition, FeatureRef, HoldStep, IntCompare, InterfaceRow, InterfaceStep, MoveStep,
        SegmentElapsed, Step, VentPick, WaitUntilStep, condition, feature_ref, int_compare,
        interface_row, location, move_step, step,
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
        // performs. The site is the seat's own harness site, three voxels
        // from the core the commander has just walked to: inside its sphere,
        // within interface range, and -- since S1's no-stacking rule -- not
        // on the core's own column, which the `safest` selector named before.
        Step {
            label: "deploy".to_owned(),
            timeout_ms: 30_000,
            on_fail: Some(skip_on_fail()),
            kind: Some(step::Kind::PlaceBeacon(gp::v1::PlaceBeaconStep {
                at: Some(gp::v1::Location {
                    place: Some(location::Place::Voxel(gp::v1::Voxel {
                        x: site.first().copied().unwrap_or(0),
                        y: site.get(1).copied().unwrap_or(0),
                        z: site.get(2).copied().unwrap_or(0),
                    })),
                }),
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
        // S1's targeting inside the tick: the resolver ranks the uncovered
        // vents by the estimator's travel and walks the `covering` spiral, and
        // the commander walks toward the site it chose. In the committed
        // 1 200-tick chain the step starts at ticks 646 and 946 and the
        // segment ends (700, 1 000) before its five seconds are spent, so it
        // neither deploys nor times out -- what the chain needs is the
        // resolution and the walk, not another beacon. The timeout bounds
        // a longer run.
        Step {
            label: "reach".to_owned(),
            timeout_ms: 5_000,
            on_fail: Some(skip_on_fail()),
            kind: Some(step::Kind::PlaceBeacon(gp::v1::PlaceBeaconStep {
                at: Some(gp::v1::Location {
                    place: Some(location::Place::Covering(FeatureRef {
                        r#ref: Some(feature_ref::Ref::Vent(VentPick {
                            rank: i32::from(feature_ref::Rank::Nearest),
                            coverage: i32::from(feature_ref::Coverage::Uncovered),
                        })),
                    })),
                }),
                tags: Vec::new(),
                initial: None,
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

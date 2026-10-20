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

pub mod arms;
pub mod audit;
pub mod bore;
pub mod chunks;
pub mod credit;
pub mod economy;
pub mod encoding;
pub mod events;
pub mod features;
pub mod footing;
pub mod hash;
pub mod ids;
pub mod interpreter;
pub mod knowledge;
pub mod mandate;
pub mod mapgen;
pub mod math;
pub mod mining;
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
pub mod terrain;
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
pub use world::site;
pub use world::{DamageOrder, DamageTarget, PHASE_ORDER, Phase, World, WorldConfig};

use pharmakos_proto::gp;

/// The match seed the determinism harness runs on.
///
/// Arbitrary and pinned. It is a *harness* seed, not a game constant, and S1's
/// real determinism segment kept it (S1's plan, decision 16; register S1-26):
/// a new seed buys no coverage the committed scenarios lack.
pub const DETERMINISM_MATCH_SEED: u64 = 0x0102_0304_0506_0708;

/// Seats the determinism harness runs.
///
/// Four, one more than v1's three, deliberately: the widest table the hash can
/// be asked to cover is the one worth pinning, and per-seat apportionment with
/// three possible damagers is what exercises the kill-credit cap when T14 adds
/// it (G4's own reasoning, and its reason for running four seats). The fourth
/// seat is seated but unplaced on the three-zone map, so it has no core and no
/// commander, and its sealed playbook never takes a step.
pub const DETERMINISM_SEATS: u32 = 4;

/// The playbook every seat of the determinism harness seals, relative to the
/// repository root: a committed copy of the Hold & Build template's
/// instantiation (`library/hold_and_build.jsonc` with its own values), in
/// canonical `gp.v1` JSON without its comments.
///
/// S1's plan, decision 16 (register S1-26): the chain covers the first-round
/// Push of a real playbook, up to the tick before the one that closes it,
/// rather than a harness playbook written in code. It is a copy, not the template itself, so that a later edit to
/// the template does not move the chain; and it is canonical JSON rather than
/// JSONC because the comment layer is `plan-core`'s and the sim reads only the
/// codec's form ([`pharmakos_proto::json`]).
pub const DETERMINISM_PLAYBOOK_PATH: &str = "tests/golden/determinism/hold_and_build.playbook.json";

/// Find `rules/rules.v1.json` from wherever the caller happens to stand.
///
/// Two probes, both relative: the repository root (where `cargo xtask` and the
/// determinism binary run) and two levels up from it (where `cargo test` puts a
/// crate's working directory). Relative rather than `CARGO_MANIFEST_DIR` so
/// nothing bakes a build machine's absolute path into a shipped binary.
#[must_use]
pub fn default_rules_path() -> Option<std::path::PathBuf> {
    probe(RULES_PATH)
}

/// Find [`DETERMINISM_PLAYBOOK_PATH`] the way [`default_rules_path`] finds
/// the rules table: from the repository root, then from two levels below it.
#[must_use]
pub fn default_determinism_playbook_path() -> Option<std::path::PathBuf> {
    probe(DETERMINISM_PLAYBOOK_PATH)
}

/// `relative` from the repository root, or from a crate's working directory
/// two levels below it; `None` when neither is a file.
fn probe(relative: &str) -> Option<std::path::PathBuf> {
    let from_root = std::path::PathBuf::from(relative);
    if from_root.is_file() {
        return Some(from_root);
    }
    let from_crate = std::path::Path::new("..").join("..").join(relative);
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
    /// The determinism harness's playbook file could not be read.
    PlaybookUnreadable {
        /// The path, as given.
        path: String,
        /// What the file system said.
        reason: String,
    },
    /// The determinism harness's playbook file is not canonical `gp.v1` JSON
    /// for a playbook.
    PlaybookNotCanonical {
        /// The path, as given.
        path: String,
        /// What the codec said, with its JSON Pointer.
        reason: String,
    },
}

impl std::fmt::Display for WorldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorldError::Rules(error) => write!(f, "{error}"),
            WorldError::Map(error) => write!(f, "{error}"),
            WorldError::Plan(error) => write!(f, "{error}"),
            WorldError::PlaybookUnreadable { path, reason } => {
                write!(f, "reading the harness playbook {path}: {reason}")
            }
            WorldError::PlaybookNotCanonical { path, reason } => write!(
                f,
                "the harness playbook {path} is not canonical gp.v1 JSON: {reason}"
            ),
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

/// Read the determinism harness's playbook ([`DETERMINISM_PLAYBOOK_PATH`])
/// from `path`.
///
/// # Errors
///
/// [`WorldError::PlaybookUnreadable`] when the file cannot be read, and
/// [`WorldError::PlaybookNotCanonical`] when it is not a `gp.v1.Playbook` in
/// canonical JSON (the codec is strict: an unknown field is refused, never
/// stripped).
pub fn determinism_playbook(path: &std::path::Path) -> Result<gp::v1::Playbook, WorldError> {
    let text = std::fs::read_to_string(path).map_err(|error| WorldError::PlaybookUnreadable {
        path: path.display().to_string(),
        reason: error.to_string(),
    })?;
    pharmakos_proto::json::decode(&text).map_err(|error| WorldError::PlaybookNotCanonical {
        path: path.display().to_string(),
        reason: error.to_string(),
    })
}

/// Build the world the determinism harness and its goldens run on, on
/// [`DETERMINISM_MATCH_SEED`], with the harness playbook sealed for every
/// seat.
///
/// One constructor, used by the binary and by every test, so a golden can never
/// disagree with the run that produced it.
///
/// # Errors
///
/// As [`determinism_world_at`].
pub fn determinism_world(
    rules_path: &std::path::Path,
    playbook_path: &std::path::Path,
) -> Result<World, WorldError> {
    determinism_world_at(DETERMINISM_MATCH_SEED, rules_path, playbook_path)
}

/// [`determinism_world`] on another match seed: [`DETERMINISM_SEATS`] seats,
/// no harness walkers, the rules table's own segment ladder and round limit
/// ([`MatchSettings::default`]), and every seat sealing the playbook at
/// `playbook_path`.
///
/// # Errors
///
/// Returns [`WorldError`] when the rules table cannot be read from
/// `rules_path`, cannot describe a map, or the playbook cannot be read from
/// `playbook_path` or compiled.
pub fn determinism_world_at(
    seed: u64,
    rules_path: &std::path::Path,
    playbook_path: &std::path::Path,
) -> Result<World, WorldError> {
    let rules = RulesTable::load(rules_path)?;
    let playbook = determinism_playbook(playbook_path)?;
    let mut world = World::new(&WorldConfig {
        match_seed: seed,
        seats: DETERMINISM_SEATS,
        units_per_seat: 0,
        rules,
        match_settings: MatchSettings::default(),
    })?;
    // How many seats sealed is the tests' to assert (`tests/determinism.rs`,
    // `tests/allocations.rs`); the loop seals exactly the world's own seats.
    let _sealed = seal_determinism_playbooks(&mut world, &playbook)?;
    Ok(world)
}

/// Seal `playbook` for every seat of `world`.
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
/// Returns [`PlanError`] when this build cannot compile `playbook`.
pub fn seal_determinism_playbooks(
    world: &mut World,
    playbook: &gp::v1::Playbook,
) -> Result<u32, PlanError> {
    let seats = world.seats().len();
    let mut sealed: u32 = 0;
    // A seat id is a `u8`, so no world has a seat past `u8::MAX` to miss.
    for raw in 0..=u8::MAX {
        if u32::from(raw) >= seats {
            break;
        }
        let plan = interpreter::Plan::compile(playbook, world.rules())?;
        if world.seal_playbook(tables::SeatId::new(raw), plan) {
            sealed = sealed.saturating_add(1);
        }
    }
    Ok(sealed)
}

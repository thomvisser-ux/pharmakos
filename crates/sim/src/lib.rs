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
//! * **The tick** — eleven named phases in a fixed order ([`world::PHASE_ORDER`]),
//!   most of them still empty and each naming the task that fills it.
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
pub mod encoding;
pub mod knowledge;
pub mod mapgen;
pub mod math;
pub mod pathing;
pub mod rules;
pub mod seams;
pub mod snapshot;
pub mod tables;
pub mod voxels;
pub mod world;

// `fork` is behind the feature at the *module* level, not just the function, so
// a default build does not compile a line of it.
#[cfg(feature = "research")]
pub mod research;

pub use encoding::{ENCODING_VERSION, Enc, STATE_HASH_SEED, digest, hex};
pub use mapgen::{GeneratedMap, MapError, MapFile, MapReport};
pub use pathing::{
    Clusters, Estimate, Fog, Node, Router, Scratch, Speed, Surface, WalkState, estimate,
};
pub use rules::{RULES_PATH, RulesError, RulesTable};
pub use snapshot::{SNAPSHOT_VERSION, Snapshot, SnapshotError};
pub use voxels::{CHUNK_EDGE, CHUNK_VOXELS, Material, Richness, VoxelEdit, VoxelStore};
pub use world::{PHASE_ORDER, Phase, World, WorldConfig};

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
}

impl std::fmt::Display for WorldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorldError::Rules(error) => write!(f, "{error}"),
            WorldError::Map(error) => write!(f, "{error}"),
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

/// Build the world the determinism harness and its goldens run on.
///
/// One constructor, used by the binary and by every test, so a golden can never
/// disagree with the run that produced it.
///
/// # Errors
///
/// Returns [`WorldError`] when the rules table cannot be read from `path` or
/// cannot describe a map.
pub fn determinism_world(rules_path: &std::path::Path) -> Result<World, WorldError> {
    let rules = RulesTable::load(rules_path)?;
    Ok(World::new(&WorldConfig {
        match_seed: DETERMINISM_MATCH_SEED,
        seats: DETERMINISM_SEATS,
        units_per_seat: DETERMINISM_UNITS_PER_SEAT,
        rules,
    })?)
}

// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! HPA\* over the chunk store's surface, the travel estimator, and the router
//! that serves a tick's repaths (decisions log items 57–62, 69 and 90).
//!
//! # What this module is
//!
//! Spike G2+P3 measured the shape and the owner fixed it, item by item. What
//! lands here is that shape on the real world rather than on a toy:
//!
//! * [`surface`] — the 2.5D search graph: **one node per column**, its top
//!   solid voxel read from [`crate::voxels::VoxelStore`], with the one-voxel
//!   step rule and the integer cost model 10 / 14 / 4 from the rules table
//!   (item 59). The generated terrain is a heightmap, so a column has exactly
//!   one surface and the node id *is* the column index.
//! * [`search`] — a hand-written binary min-heap ordered `(f, h, node_id)` and
//!   a dense, generation-stamped scratch. **The order is determinism contract**
//!   (item 62): no two distinct nodes ever compare equal, so the pop sequence
//!   is a function of the graph and nothing else.
//! * [`clusters`] — the decomposition at `locomotion.hpa_cluster_voxels` = 32,
//!   one chunk footprint, a **single abstract level** (item 58): entrances,
//!   transition nodes, per-cluster intra edges, and the union-find
//!   connectivity oracle that answers "no path" in two array reads before any
//!   search runs.
//! * [`repair`] — eager chunk-footprint repair (item 60): the dirtied clusters'
//!   entrances plus the neighbours whose shared border actually changed, then
//!   their intra edges, then one connectivity rebuild per tick.
//! * [`route`] — the abstract search, the per-leg refinement and the smoother.
//! * [`estimate`] — item 61's query: abstract search alone, never a refinement,
//!   never a step, never a fork.
//! * [`router`] — per-unit routes, the integer speed accumulator of item 90,
//!   the per-tick repath cap of 16 served round-robin by `(seat, beacon, unit)`
//!   (items 60 and 69), and **"sealed in" as a designed state** — park and
//!   report, never a repath loop.
//!
//! # Decision 8, taken: the mechanism now, the certification at S3
//!
//! Skeleton plan §6 decision 8 asked whether the skeleton lands real HPA\* or
//! only item 61's frozen contract over a simpler search. The recommendation —
//! and what is built here — is the mechanism: cluster-32 HPA\*, the repair, the
//! oracle, the cap and the estimator, with the committed path-hash golden. S3
//! keeps the ±15 % certification on real code, the corner entrance, and the
//! endpoint-sweep work. A flat search could not produce `legs` or price fog per
//! abstract edge, so the contract it claimed to freeze would be partly
//! unimplementable.
//!
//! # Nothing here allocates inside a tick
//!
//! Every structure below fixes its size at construction from the map extent and
//! the cluster edge, and the bounds are arithmetic rather than hope. The
//! derivation lives once, in [`clusters`]'s module docs, and the short form is:
//! a border of `n` cells holds at most `n` maximal runs — `n`, not `n / 2`,
//! because two adjacent border cells can each be crossable without being
//! steppable to each other along the border — so a cluster carries at most
//! `4 * cluster_edge` transition nodes and at most
//! `max_trans * (max_trans - 1)` directed intra edges. `tests/allocations.rs`
//! is the check, and it craters on every measured tick.
//!
//! # What is hashed
//!
//! The graph is **derived state**: it is a pure function of the chunk store and
//! the rules table, so it is neither encoded nor snapshotted — restoring a
//! snapshot rebuilds it, and `tests/pathing.rs` asserts that the rebuild is
//! identical to the repaired graph it replaces. What *is* hashed is what a unit
//! does with it: its route (as a digest, the way the chunk store enters through
//! per-chunk digests — item 66), how far along it is, its speed accumulator,
//! its walk state and whether it is waiting for a repath. Those columns live in
//! [`crate::tables::UnitTable`], go through the canonical encoder in declared
//! order and round-trip through the snapshot with the route nodes beside them.

pub mod clusters;
pub mod estimate;
pub mod repair;
pub mod route;
pub mod router;
pub mod search;
pub mod surface;

pub use clusters::{Clusters, Trans};
pub use estimate::{Estimate, Fog, Speed, estimate, ticks_for_cost};
pub use repair::{RepairReport, repair};
pub use route::{AbstractResult, RouteOutcome, abstract_search, route};
pub use router::{Router, WalkState};
pub use search::Scratch;
pub use surface::{NEIGHBOURS, Node, StepCosts, Surface};

/// Voxels of clear air a walker needs above the column it stands on.
///
/// The spike's value, and it is the locomotion rule rather than a performance
/// knob: a column whose surface is within [`HEADROOM_VOXELS`] of the world
/// ceiling is not walkable.
///
/// PLACEHOLDER: no `rules/rules.v1.json` row exists for it, because nothing has
/// needed one — the generated terrain is clamped far below the ceiling, so the
/// test never fires on a pristine map. Proposing the row belongs to the stage
/// that gives unit height a meaning (owner, at S2).
pub const HEADROOM_VOXELS: i32 = 2;

/// A border run longer than this gets a transition node at each end instead of
/// one in the middle (spike G2's `LONG_RUN`).
///
/// PLACEHOLDER: tuning — one of the seven values G2 §9.9 lists as "to become
/// rules-table data at S3" (owner, at S3).
pub const LONG_RUN: i32 = 6;

/// How far ahead the smoother looks for a shortcut.
///
/// PLACEHOLDER: tuning, from the same G2 §9.9 list (owner, at S3).
pub const SMOOTH_LOOKAHEAD: usize = 16;

/// The most nodes one unit's route may hold.
///
/// A route longer than this is kept to its first [`MAX_ROUTE_NODES`] nodes and
/// marked partial: the unit walks what it has and asks for the rest when it
/// gets there, which is a bounded amount of work per unit rather than a route
/// buffer whose size depends on the map. The map's own diagonal is 543 cells at
/// 384 × 384, so a partial route is a detour-heavy route rather than an
/// ordinary one.
///
/// PLACEHOLDER: sized against the skeleton's 384-voxel map; the stage that
/// changes the map's footprint re-derives it (owner, at S2).
pub const MAX_ROUTE_NODES: u32 = 1024;

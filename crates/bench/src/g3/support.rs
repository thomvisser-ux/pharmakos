// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The support search's adversarial tick (item 138 (3); S2's plan, section 5
//! and R4): many seeds from several charges, the layout's digs whose Mine
//! checks run in Programs, a region of exactly `collapse.max_region_voxels`,
//! and two interlocking regions, all on one tick, with the fall's passes on
//! that tick; and the footing query at its worst case (item 148 (4)).
//!
//! # The construct
//!
//! Built from outside on the gate's match, through the host's filing seam
//! (`Host::file_voxel_edit`, E5), between ticks and bottom-up, so every tick
//! of the build ends with everything it placed held up (a placed voxel is a
//! seed, and one placed before its support would fall at once). On a site the
//! layout's beacons and the crater stream do not reach, above the ground:
//!
//! * **A**, a block of exactly `collapse.max_region_voxels` voxels (16 x 16
//!   by however many layers make the bound), standing on one stone stem at a
//!   corner, an air gap under the rest;
//! * **Y**, a small block on its own stem; and **X**, a cradle around it: two
//!   walls either side of Y, a slab over it one air layer above Y's top, each
//!   wall on its own stem, its feet lower than Y's.
//!
//! # The tick
//!
//! The stems' top [`CUT_DEPTH`] voxels are cut, and several charges (balls of
//! `combat.charge_radius_voxels`, as a demolition charge's cut is filed) land
//! on the ground around the site, on the same tick. A falls whole; X falls
//! first (its lowest voxel is lowest), comes to rest on Y one layer down, then
//! Y falls away under it and X falls again: the interlock the rule's passes
//! exist for. The tick is timed phase by phase as the segment's are, and the
//! support rule's own counters say what the fall did: regions, voxels, passes,
//! visits against `collapse.max_visits_per_tick`, and seeds labelled supported
//! because the budget ran out.
//!
//! **The footing query.** Since `site` (#133) a footing ignores every edit, so
//! Mine never undermines and its dig asks no support search of its own: its
//! support check is the seam's reach flood, which runs in Programs on the
//! tick above. What a dig and a blast ask of the footings is timed here, on
//! the gate's world once the construct stands, at the worst case `pharmakos_sim::footing`
//! names: a ball of the largest blast radius whose box meets a footing, every
//! voxel asked, each miss walking every structure and beacon row ([`FootingQuery`]).

use std::path::Path;
use std::time::Instant;

use pharmakos_gateway::host::Host;
use pharmakos_proto::gp::v1::rules_table::Collapse;
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::voxels::{Material, VoxelEdit};
use pharmakos_sim::world::support::CollapseSkip;
use pharmakos_sim::world::{FileRefused, PHASE_ORDER, Phase, World};

use super::segment::{self, Gate, Timed};
use crate::error::Error;
use crate::hosted;

/// The block's side, in voxels: A is `SIDE x SIDE x layers`.
const SIDE: i32 = 16;

/// How far the site's centre stands from every beacon, in voxels: a sphere's
/// 24 and a crater's reach, with room.
const CLEARANCE: i64 = 64;

/// Ticks the match plays before the build starts, so the opening tick's
/// decisions and fabrications are behind it.
const SETTLE_TICKS: u32 = 2;

/// How many voxels of each stem the adversarial tick cuts, from its top down:
/// more than the one air layer between X's slab and Y, so X's walls cannot
/// come to rest on their own stems before Y has fallen away from under the
/// slab (the interlock), and Y falls farther than X's first drop.
const CUT_DEPTH: i32 = 4;

/// The charges filed on the adversarial tick.
const CHARGES: i32 = 6;

/// How many times the footing check is timed, the fastest kept.
pub const FOOTING_RUNS: usize = 5;

/// What the adversarial tick measured.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SupportReport {
    /// The site's corner column.
    pub site: [i32; 2],
    /// `collapse.max_region_voxels`.
    pub max_region_voxels: u32,
    /// `collapse.max_visits_per_tick`.
    pub max_visits_per_tick: u32,
    /// Ticks the build took.
    pub build_ticks: u32,
    /// Voxels the build placed.
    pub built_voxels: u64,
    /// The adversarial tick, whole, in nanoseconds.
    pub tick_ns: u128,
    /// Its phases but the hash, in order, with their nanoseconds.
    pub phases: Vec<(Phase, u128)>,
    /// Regions that fell on the tick.
    pub regions: u64,
    /// Voxels the falls moved on the tick.
    pub voxels: u64,
    /// The passes the tick's fall took.
    pub passes: u64,
    /// The voxels the tick's searches visited.
    pub visits: usize,
    /// Seeds labelled supported on the tick because the visit budget ran out.
    pub budget_skips: u64,
    /// Seeds labelled supported on the tick because their search passed the
    /// region bound.
    pub bound_skips: u64,
    /// The footing query at its worst case, on the gate's world as the
    /// construct left it.
    pub footing: FootingQuery,
}

/// One column of the construct: its voxels from `low` to `high`, inclusive.
#[derive(Clone, Copy, Debug)]
struct Run {
    x: i32,
    y: i32,
    low: i32,
    high: i32,
}

/// The construct's geometry, from the site's corner and the ground under it.
#[derive(Clone, Debug)]
struct Construct {
    /// Every column run of stone to place.
    runs: Vec<Run>,
    /// The stems' top [`CUT_DEPTH`] voxels, which the adversarial tick cuts.
    cuts: Vec<[i32; 3]>,
    /// A voxel of A's top layer, which must stand once the build is done.
    probe: [i32; 3],
}

/// The highest ground over the box `[x0, y0]..=[x1, y1]`.
fn highest_ground(world: &World, x0: i32, y0: i32, x1: i32, y1: i32) -> Result<i32, Error> {
    let mut top: Option<i32> = None;
    for x in x0..=x1 {
        for y in y0..=y1 {
            let z = world.voxels().top_solid_z(x, y).ok_or_else(|| {
                Error::Layout(format!(
                    "the support site's column ({x}, {y}) has no ground"
                ))
            })?;
            top = Some(match top {
                Some(held) => held.max(z),
                None => z,
            });
        }
    }
    top.ok_or_else(|| Error::Layout("an empty support site".to_owned()))
}

/// The site: the column on a 32-voxel grid farthest from every beacon, ties by
/// the grid's order, at least [`CLEARANCE`] from each.
fn site(world: &World) -> Result<[i32; 2], Error> {
    let size = world.rules().map_size_voxels();
    let beacons: Vec<[i32; 2]> = world
        .beacons()
        .positions()
        .iter()
        .map(|at| [at[0].floor_voxels(), at[1].floor_voxels()])
        .collect();
    let mut best: Option<(i64, [i32; 2])> = None;
    let mut x = 32;
    while x + 48 < size[0] {
        let mut y = 32;
        while y + 32 < size[1] {
            let centre = [x + 17, y + 8];
            let nearest = beacons
                .iter()
                .map(|at| {
                    let dx = i64::from(at[0]) - i64::from(centre[0]);
                    let dy = i64::from(at[1]) - i64::from(centre[1]);
                    dx * dx + dy * dy
                })
                .min()
                .ok_or_else(|| Error::Layout("no beacon stands on the map".to_owned()))?;
            if best.is_none_or(|(held, _)| nearest > held) {
                best = Some((nearest, [x, y]));
            }
            y += 32;
        }
        x += 32;
    }
    let (distance, chosen) =
        best.ok_or_else(|| Error::Layout("the map has no room for the support site".to_owned()))?;
    if distance < CLEARANCE * CLEARANCE {
        return Err(Error::Layout(format!(
            "the support site is {} voxels from a beacon, under the {CLEARANCE} it needs",
            distance.isqrt()
        )));
    }
    Ok(chosen)
}

/// A stem from the ground under `(x, y)` up to `top`.
fn stem(world: &World, x: i32, y: i32, top: i32) -> Result<Run, Error> {
    let ground = world
        .voxels()
        .top_solid_z(x, y)
        .ok_or_else(|| Error::Layout(format!("the stem column ({x}, {y}) has no ground")))?;
    Ok(Run {
        x,
        y,
        low: ground + 1,
        high: top,
    })
}

fn construct(world: &World, corner: [i32; 2], layers: i32) -> Result<Construct, Error> {
    let [ox, oy] = corner;
    let top = highest_ground(world, ox - 2, oy - 2, ox + 36, oy + SIDE + 2)?;
    let height = world.rules().map_size_voxels()[2];
    let base = top + 2;
    if base + layers >= height {
        return Err(Error::Layout(format!(
            "the support site's ground stands at z {top}, and a block of {layers} layers on a \
             stem does not fit under the map's {height}"
        )));
    }
    let mut runs: Vec<Run> = Vec::new();
    // A: a stem at the corner up to `base`, the block from `base + 1`.
    runs.push(stem(world, ox, oy, base)?);
    for x in ox..ox + SIDE {
        for y in oy..oy + SIDE {
            runs.push(Run {
                x,
                y,
                low: base + 1,
                high: base + layers,
            });
        }
    }
    // Y and X, beside A: Y a 4 x 4 x 3 block over `b`, X's walls two voxels
    // either side of it from `b - 2` to `b + 5`, the slab at `b + 5`.
    let b = top + 6;
    let (yx, yy) = (ox + 28, oy + 2);
    runs.push(stem(world, yx, yy, b)?);
    for x in yx..yx + 4 {
        for y in yy..yy + 4 {
            runs.push(Run {
                x,
                y,
                low: b + 1,
                high: b + 3,
            });
        }
    }
    let walls = [yx - 2, yx + 5];
    for wall in walls {
        runs.push(stem(world, wall, yy - 2, b - 3)?);
    }
    for x in yx - 2..=yx + 5 {
        for y in yy - 2..=yy + 5 {
            let is_wall = walls.contains(&x);
            runs.push(Run {
                x,
                y,
                low: if is_wall { b - 2 } else { b + 5 },
                high: b + 5,
            });
        }
    }
    Ok(Construct {
        runs,
        cuts: [
            [ox, oy, base],
            [yx, yy, b],
            [yx - 2, yy - 2, b - 3],
            [yx + 5, yy - 2, b - 3],
        ]
        .into_iter()
        .flat_map(|[x, y, z]| (0..CUT_DEPTH).map(move |down| [x, y, z - down]))
        .collect(),
        probe: [ox + SIDE - 1, oy + SIDE - 1, base + layers],
    })
}

/// The construct's voxels, bottom-up: by height, then row, then column, so
/// every prefix of the list is held up (see the module docs).
fn bottom_up(construct: &Construct) -> Vec<[i32; 3]> {
    let mut voxels: Vec<[i32; 3]> = construct
        .runs
        .iter()
        .flat_map(|run| (run.low..=run.high).map(move |z| [run.x, run.y, z]))
        .collect();
    voxels.sort_unstable_by_key(|[x, y, z]| (*z, *y, *x));
    voxels.dedup();
    voxels
}

/// File one edit, refusing silently nothing.
fn file(host: &mut Host, edit: VoxelEdit) -> Result<bool, Error> {
    match host.file_voxel_edit(edit) {
        Ok(()) => Ok(true),
        Err(FileRefused::QueueFull { .. }) => Ok(false),
        Err(refused @ (FileRefused::MidTick | FileRefused::OffTheMap)) => Err(Error::Layout(
            format!("the host refused the support construct's edit {edit:?}: {refused}"),
        )),
    }
}

/// Play one tick, with the crater stream's due craters filed first.
fn tick(gate: &mut Gate, played: &mut u64) -> Result<Timed, Error> {
    let host = gate
        .surface
        .host_mut()
        .map_err(|error| Error::gateway("filing the crater stream", &error))?;
    gate.stream.file(host, *played)?;
    let timed = segment::timed_tick(&mut gate.surface, None)?
        .ok_or_else(|| Error::Empty("the support run's segment ended early".to_owned()))?;
    *played = played.saturating_add(1);
    Ok(timed)
}

fn host(gate: &mut Gate) -> Result<&mut Host, Error> {
    gate.surface
        .host_mut()
        .map_err(|error| Error::gateway("filing the support construct", &error))
}

/// The collapse rows, the site and the construct on the gate's world.
fn plan(world: &World) -> Result<(Collapse, [i32; 2], Construct), Error> {
    let rows = world
        .rules()
        .message()
        .collapse
        .ok_or_else(|| Error::Rules("no `collapse` block".to_owned()))?;
    let per_layer = u32::try_from(SIDE * SIDE).map_err(|_| Error::Overflow("a layer"))?;
    if rows.max_region_voxels == 0 || rows.max_region_voxels.rem_euclid(per_layer) != 0 {
        return Err(Error::Rules(format!(
            "collapse.max_region_voxels {} is not a whole number of {SIDE} x {SIDE} layers",
            rows.max_region_voxels
        )));
    }
    let layers = i32::try_from(rows.max_region_voxels.div_euclid(per_layer))
        .map_err(|_| Error::Overflow("the block's layers"))?;
    let corner = site(world)?;
    let construct = construct(world, corner, layers)?;
    Ok((rows, corner, construct))
}

/// File `voxels` as stone, as much a tick as the host's share of the queue
/// takes, a tick played after each batch. Answers the ticks it took.
fn build(gate: &mut Gate, voxels: &[[i32; 3]], played: &mut u64) -> Result<u32, Error> {
    let mut pending = voxels;
    let mut build_ticks: u32 = 0;
    while !pending.is_empty() {
        let mut taken = 0_usize;
        for at in pending {
            let edit = VoxelEdit::Set {
                at: *at,
                material: Material::STONE,
                by: None,
            };
            if !file(host(gate)?, edit)? {
                break;
            }
            taken = taken.saturating_add(1);
        }
        if taken == 0 {
            return Err(Error::Layout(
                "the host's share of the edit queue took no edit of the support construct"
                    .to_owned(),
            ));
        }
        pending = pending
            .get(taken..)
            .ok_or(Error::Overflow("the build's progress"))?;
        tick(gate, played)?;
        build_ticks = build_ticks.saturating_add(1);
    }
    Ok(build_ticks)
}

/// Check the construct stands once the build is done.
fn stands(world: &World, construct: &Construct) -> Result<(), Error> {
    if world
        .voxels()
        .get(construct.probe)
        .is_none_or(Material::is_air)
    {
        return Err(Error::Drift(format!(
            "the support construct did not stand: {:?} is not stone after the build",
            construct.probe
        )));
    }
    Ok(())
}

/// The footing query at its worst case (item 148 (4), #133's figure).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FootingQuery {
    /// The blast's radius the ball was asked at: the largest edit the rules
    /// file, `combat.charge_radius_voxels`.
    pub radius: i32,
    /// The voxels of the ball the query asked about, `(2r + 1)^3` less the
    /// corners outside the ball.
    pub ball_voxels: u64,
    /// The structure and beacon rows each miss walks, live or not.
    pub rows: u64,
    /// One `set`'s check (a Mine dig, a fill, a host's write) that names no
    /// footing, so it walks every row: best of its runs, in nanoseconds.
    pub set_ns: u128,
    /// A blast's whole ball beside a live beacon's column, so its box
    /// touches a footing and every voxel is asked, as `VoxelStore::blast`
    /// asks: best of its runs, in nanoseconds.
    pub blast_ns: u128,
}

/// The fastest of [`FOOTING_RUNS`] runs of `run`, in nanoseconds.
fn fastest(mut run: impl FnMut() -> u64) -> Result<u128, Error> {
    let mut best: Option<u128> = None;
    for _ in 0..FOOTING_RUNS {
        let started = Instant::now();
        let held = run();
        let ns = started.elapsed().as_nanos();
        std::hint::black_box(held);
        best = Some(match best {
            Some(kept) => kept.min(ns),
            None => ns,
        });
    }
    best.ok_or_else(|| Error::Empty("the footing query was never timed".to_owned()))
}

/// Time the footing query at its worst case on `world`, restating the two
/// callers in `crates/sim`: a `set`'s `Footings::contains` (world.rs's edit
/// filing) and `VoxelStore::blast`'s `touches` then `contains` per voxel of
/// the ball. The ball is centred `r` voxels east of the first live beacon's
/// column, so its box reaches the footing while every other voxel misses, and
/// a miss walks every row: the most rows asked the most times.
fn footing_query(world: &World) -> Result<FootingQuery, Error> {
    let radius = i32::try_from(
        world
            .rules()
            .message()
            .combat
            .as_ref()
            .map(|combat| combat.charge_radius_voxels)
            .ok_or_else(|| Error::Rules("no `combat` block".to_owned()))?,
    )
    .map_err(|_| Error::Overflow("the charge radius"))?;
    let r2 = radius
        .checked_mul(radius)
        .ok_or(Error::Overflow("the charge radius squared"))?;
    let beacons = world.beacons();
    let [bx, by] = beacons
        .hit_points()
        .iter()
        .zip(beacons.positions())
        .find(|(hp, _)| hp.is_alive())
        .map(|(_, at)| pharmakos_sim::targeting::column_of(*at))
        .ok_or_else(|| Error::Layout("no live beacon to stand a footing on".to_owned()))?;
    let cx = bx
        .checked_add(radius)
        .ok_or(Error::Overflow("the ball's centre"))?;
    let cz = world
        .voxels()
        .top_solid_z(cx, by)
        .ok_or_else(|| Error::Layout(format!("the ball's column ({cx}, {by}) has no ground")))?;
    let footings = world.footings();
    let (min, max) = ([cx - radius, by - radius], [cx + radius, by + radius]);
    if !footings.touches(min, max) {
        return Err(Error::Drift(
            "the ball beside a live beacon does not reach its footing".to_owned(),
        ));
    }
    let mut ball: Vec<[i32; 3]> = Vec::new();
    for dz in -radius..=radius {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy + dz * dz <= r2 {
                    ball.push([cx + dx, by + dy, cz + dz]);
                }
            }
        }
    }
    let miss = [cx, by, cz];
    if footings.holds(miss) {
        return Err(Error::Drift(format!("{miss:?} lies in a footing")));
    }
    let set_ns = fastest(|| u64::from(footings.contains(miss)))?;
    let blast_ns = fastest(|| {
        let near = footings.touches(min, max);
        ball.iter().fold(0_u64, |held, at| {
            held + u64::from(near && footings.contains(*at))
        })
    })?;
    let rows = u64::from(world.structures().len())
        .checked_add(u64::from(beacons.len()))
        .ok_or(Error::Overflow("the footing rows"))?;
    Ok(FootingQuery {
        radius,
        ball_voxels: u64::try_from(ball.len()).map_err(|_| Error::Overflow("the ball"))?,
        rows,
        set_ns,
        blast_ns,
    })
}

/// File the adversarial tick's cuts and charges.
fn arm(gate: &mut Gate, construct: &Construct, corner: [i32; 2]) -> Result<(), Error> {
    let charge = i32::try_from(
        hosted::world(&gate.surface)?
            .rules()
            .message()
            .combat
            .as_ref()
            .map(|combat| combat.charge_radius_voxels)
            .ok_or_else(|| Error::Rules("no `combat` block".to_owned()))?,
    )
    .map_err(|_| Error::Overflow("the charge radius"))?;
    for cut in &construct.cuts {
        let edit = VoxelEdit::Set {
            at: *cut,
            material: Material::AIR,
            by: Some(SeatId::new(0)),
        };
        if !file(host(gate)?, edit)? {
            return Err(Error::Layout("the queue refused a stem's cut".to_owned()));
        }
    }
    for n in 0..CHARGES {
        let x = corner[0] - 6 + n * 8;
        let y = corner[1] + SIDE + 6;
        let z = hosted::world(&gate.surface)?
            .voxels()
            .top_solid_z(x, y)
            .ok_or_else(|| Error::Layout(format!("a charge's column ({x}, {y}) has no ground")))?;
        let edit = VoxelEdit::Crater {
            centre: [x, y, z],
            radius: charge,
            by: Some(SeatId::new(1)),
        };
        if !file(host(gate)?, edit)? {
            return Err(Error::Layout("the queue refused a charge".to_owned()));
        }
    }
    Ok(())
}

/// The difference of two support counts, which never go down.
fn delta(now: u64, was: u64) -> Result<u64, Error> {
    now.checked_sub(was)
        .ok_or_else(|| Error::Drift("a support count went down".to_owned()))
}

/// Build the construct on the gate's match and play its adversarial tick.
///
/// # Errors
///
/// When the gate's match cannot be opened, the site or the construct does not
/// fit the map, the build does not stand, or the tick does not fall as built.
pub fn measure(root: &Path, seed: u64) -> Result<SupportReport, Error> {
    // A minute: room for the settling ticks, the build and the tick.
    let mut gate = segment::open_gate(root, seed, 60_000, super::stream::CRATERS_PER_SECOND)?;
    let mut played: u64 = 0;
    for _ in 0..SETTLE_TICKS {
        tick(&mut gate, &mut played)?;
    }
    let (rows, corner, construct) = plan(hosted::world(&gate.surface)?)?;
    let voxels = bottom_up(&construct);
    let build_ticks = build(&mut gate, &voxels, &mut played)?;
    stands(hosted::world(&gate.surface)?, &construct)?;
    let footing = footing_query(hosted::world(&gate.surface)?)?;
    arm(&mut gate, &construct, corner)?;
    let before = hosted::world(&gate.surface)?.collapse_counts();
    let timed = tick(&mut gate, &mut played)?;
    let after = hosted::world(&gate.surface)?.collapse_counts();
    let regions = delta(after.regions, before.regions)?;
    let moved = delta(after.voxels, before.voxels)?;
    let block = u64::from(rows.max_region_voxels);
    if regions < 3 || moved < block {
        return Err(Error::Drift(format!(
            "the adversarial tick dropped {regions} regions and {moved} voxels; the construct \
             was built to drop A ({block} voxels), X and Y"
        )));
    }
    let mut phases: Vec<(Phase, u128)> = Vec::new();
    for (index, ns) in &timed.phase_ns {
        let phase = *PHASE_ORDER
            .get(*index)
            .ok_or(Error::Overflow("a phase index"))?;
        phases.push((phase, *ns));
    }
    Ok(SupportReport {
        site: corner,
        max_region_voxels: rows.max_region_voxels,
        max_visits_per_tick: rows.max_visits_per_tick,
        build_ticks,
        built_voxels: u64::try_from(voxels.len()).map_err(|_| Error::Overflow("the build"))?,
        tick_ns: timed.total_ns()?,
        phases,
        regions,
        voxels: moved,
        passes: after.last_passes,
        visits: after.last_visits,
        budget_skips: delta(
            after.skipped(CollapseSkip::TickBudget),
            before.skipped(CollapseSkip::TickBudget),
        )?,
        bound_skips: delta(
            after.skipped(CollapseSkip::RegionBound),
            before.skipped(CollapseSkip::RegionBound),
        )?,
        footing,
    })
}

// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The spike's unit tests. `cargo test --release --locked` must pass.
//!
//! What each group is for:
//!
//! * **hash** — the incremental scheme must be *the same value* as a
//!   from-scratch fold of the same sub-hashes, on every tick, and must move
//!   when any single hashed field moves. That identity is what makes the
//!   full-versus-incremental question a performance decision rather than a
//!   choice between two different hashes, and the hash is a contract file
//!   (AGENTS.md §5), so it is pinned by a test and not by a comment.
//! * **cost-model independence** — the per-tick hash stream must not depend on
//!   the repath cap or on the clock calibration. Without that, the three-OS
//!   `cmp` in `ci/spike-g3.yml` would fail for a reason that has nothing to do
//!   with the sim.
//! * **kill credit** — largest-remainder apportionment, ties to the lowest seat
//!   id, shares summing to the bounty exactly.
//! * **broadphase** — the ring search agrees with a brute-force scan over
//!   1_000 random queries, including the `(d2, id)` tiebreak.
//! * **voxels** — the copy-on-write store shares untouched chunks and copies on
//!   write.
//! * **power** — the brownout order is total.

use std::sync::Arc;

use g3_tick_budget::broadphase::{
    Grid, NO_CELL, Population, cell_of, nearest_enemy, nearest_enemy_brute,
};
use g3_tick_budget::fixed::Fx;
use g3_tick_budget::hash::{HashMode, StateHasher, TABLE_COUNT, Table};
use g3_tick_budget::rng::{Stream, StreamRng};
use g3_tick_budget::tables::{CreditTable, MAX_CREDITS};
use g3_tick_budget::tick::{Config, CostConstants, Phase, World};
use g3_tick_budget::voxels::{VoxelStore, index_of};
use g3_tick_budget::{apportion, power};

fn small(hash_mode: HashMode) -> Config {
    Config {
        seats: 4,
        units: 60,
        beacons: 12,
        edits_per_s: 20,
        hash_mode,
        ..Config::default()
    }
}

// ---------------------------------------------------------------------------
// hash
// ---------------------------------------------------------------------------

/// The incremental value equals a full re-hash of every table, on every tick of
/// a run long enough to include a rebuild sweep, several decision ticks and a
/// few hundred craters.
#[test]
fn incremental_hash_equals_a_from_scratch_fold_every_tick() {
    let mut w = World::new(small(HashMode::Incremental));
    for t in 0..600u32 {
        w.step();
        let incremental = w.last_hash;
        let full = w.full_hash_now();
        assert_eq!(
            incremental, full,
            "tick {t}: the incremental hash and a from-scratch fold disagree"
        );
        // And the cached sub-hashes still fold to the same value afterwards,
        // i.e. `full_hash_now` left no residue.
        assert_eq!(
            w.last_hash, incremental,
            "tick {t}: the cache was disturbed"
        );
    }
}

/// The fold is a pure function of the sub-hashes in declared order, so a
/// from-scratch fold of the cache equals the cache's own fold.
#[test]
fn the_fold_is_a_pure_function_of_the_sub_hashes() {
    let mut w = World::new(small(HashMode::Incremental));
    for _ in 0..50 {
        w.step();
        let subs: [u64; TABLE_COUNT] = w.hasher.sub_hashes();
        assert_eq!(StateHasher::fold_of(&subs), w.last_hash);
    }
}

/// Full and incremental produce byte-identical hash streams.
#[test]
fn full_and_incremental_produce_the_same_stream() {
    let mut a = World::new(small(HashMode::Full));
    let mut b = World::new(small(HashMode::Incremental));
    let mut sa: Vec<u64> = Vec::new();
    let mut sb: Vec<u64> = Vec::new();
    for _ in 0..500 {
        a.step();
        b.step();
        sa.push(a.last_hash);
        sb.push(b.last_hash);
    }
    assert_eq!(sa, sb, "the two hash modes disagree");
    assert!(sa.iter().any(|h| *h != sa[0]), "the hash never changed");
}

/// **The incremental mode must actually skip a table somewhere**, or the
/// full-versus-incremental measurement plan §8 item (3) asks for measures
/// nothing.
///
/// This is the test the first version of the spike did not have, and its
/// absence hid a real defect: every phase marked its tables on entry ("I ran")
/// rather than on a write ("I changed bytes"), so `tables_rehashed` was 8 of 8
/// on every tick in both modes and the reported 1.3% difference between them
/// was the cost of eight dirty-flag tests, not of an incremental scheme.
///
/// With the marking fixed, the honest finding is still mostly negative and this
/// test records the shape of it: with the destruction stream off, `Voxels` goes
/// clean and stays clean, so at least one table is skipped on every tick. With
/// destruction on, a crater lands on every tick at 20/s and there is nothing
/// left to skip — which is the structural reason to take the full hash, and is
/// a different claim from "the two cost the same".
#[test]
fn the_incremental_mode_skips_tables_when_a_table_is_clean() {
    let quiet = Config {
        edits_per_s: 0,
        hash_mode: HashMode::Incremental,
        ..small(HashMode::Incremental)
    };
    let mut w = World::new(quiet);
    let ticks = 400u32;
    let mut rehashed: u64 = 0;
    for _ in 0..ticks {
        rehashed += u64::from(w.step().tables_rehashed);
    }
    let ceiling = u64::from(ticks) * u64::try_from(TABLE_COUNT).unwrap();
    assert!(
        rehashed < ceiling,
        "incremental re-hashed every table on every tick ({rehashed} of {ceiling}): \
         it has degenerated to full and cannot measure anything"
    );
    // Specifically: the voxel table is never dirtied, so it is skipped on all
    // but the first tick.
    assert!(
        rehashed <= ceiling - u64::from(ticks - 1),
        "the clean voxel table was still re-encoded: {rehashed} of {ceiling}"
    );
}

/// Full mode re-hashes all eight tables on every tick, by definition. Stated as
/// a test so that the denominator the incremental figure is quoted against is
/// not itself an assumption.
#[test]
fn the_full_mode_rehashes_every_table_every_tick() {
    let mut w = World::new(small(HashMode::Full));
    for _ in 0..200 {
        assert_eq!(
            usize::try_from(w.step().tables_rehashed).unwrap(),
            TABLE_COUNT
        );
    }
}

/// Two runs of the same configuration produce identical hash streams.
#[test]
fn the_hash_stream_is_identical_across_two_runs() {
    let run = || {
        let mut w = World::new(small(HashMode::Full));
        (0..400)
            .map(|_| {
                w.step();
                w.last_hash
            })
            .collect::<Vec<u64>>()
    };
    assert_eq!(run(), run());
}

/// Every single-field mutation moves the per-tick hash.
///
/// One mutation per hashed field of every table that the tick writes, applied
/// to a settled world; each is applied, hashed, and reverted. A field that can
/// be changed without moving the hash is a latent desync (AGENTS.md §4.8), and
/// in a benchmark it is also a field the optimiser may stop computing.
#[test]
fn every_single_field_mutation_changes_the_hash() {
    let mut w = World::new(small(HashMode::Full));
    for _ in 0..40 {
        w.step();
    }
    let base = w.full_hash_now();
    let settled = w.clone();

    // Each mutation is applied to a fresh clone of the settled world, so no
    // mutation has to be undone and none can mask another.
    macro_rules! mutate {
        ($name:expr, $apply:expr) => {{
            #[allow(clippy::redundant_closure_call)]
            {
                let mut m = settled.clone();
                ($apply)(&mut m);
                m.hasher.mark_all();
                let moved = m.full_hash_now();
                assert_ne!(moved, base, "mutating {} did not move the hash", $name);
            }
        }};
    }

    // Units.
    mutate!("units.id", |w: &mut World| w.units.id[3] ^= 1);
    mutate!("units.seat", |w: &mut World| w.units.seat[3] ^= 1);
    mutate!("units.hp", |w: &mut World| w.units.hp[3] += 1);
    mutate!("units.pos", |w: &mut World| w.units.pos[3][2] =
        Fx(w.units.pos[3][2].raw() + 1));
    mutate!("units.step", |w: &mut World| w.units.step[3][1] =
        Fx(w.units.step[3][1].raw() + 1));
    mutate!("units.heading", |w: &mut World| w.units.heading[3] ^= 1);
    mutate!("units.cooldown", |w: &mut World| w.units.cooldown[3] ^= 1);
    mutate!("units.target", |w: &mut World| w.units.target[3] =
        Some(4_242));
    mutate!("units.nearest", |w: &mut World| w.units.nearest[3] =
        Some(4_243));
    mutate!("units.nearest_d2", |w: &mut World| w.units.nearest_d2[3] +=
        1);
    mutate!("units.program", |w: &mut World| w.units.program[3] ^= 1);
    mutate!("units.stance", |w: &mut World| w.units.stance[3] ^= 1);
    mutate!("units.kw", |w: &mut World| w.units.kw[3] += 1);
    mutate!("units.route", |w: &mut World| w.units.route[3][7][1] =
        Fx(w.units.route[3][7][1].raw() + 1));
    mutate!("units.route_len", |w: &mut World| w.units.route_len[3] ^= 1);
    mutate!("units.route_idx", |w: &mut World| w.units.route_idx[3] ^= 1);

    // Beacons.
    mutate!("beacons.id", |w: &mut World| w.beacons.id[2] ^= 1);
    mutate!("beacons.seat", |w: &mut World| w.beacons.seat[2] ^= 1);
    mutate!("beacons.hp", |w: &mut World| w.beacons.hp[2] += 1);
    mutate!("beacons.pos", |w: &mut World| w.beacons.pos[2][0] =
        Fx(w.beacons.pos[2][0].raw() + 1));
    mutate!("beacons.mandate", |w: &mut World| w.beacons.mandate[2] ^= 1);
    mutate!("beacons.kw_supply", |w: &mut World| w.beacons.kw_supply
        [2] += 1);
    mutate!("beacons.kw_draw", |w: &mut World| w.beacons.kw_draw[2] += 1);
    mutate!("beacons.powered", |w: &mut World| w.beacons.powered[2] ^= 1);
    mutate!("beacons.priority", |w: &mut World| w.beacons.priority[2] ^=
        1);
    mutate!("beacons.treasury", |w: &mut World| w.beacons.treasury[2] +=
        1);
    mutate!("beacons.roster", |w: &mut World| w.beacons.roster[2] ^= 1);

    // Structures.
    mutate!("structures.id", |w: &mut World| w.structures.id[1] ^= 1);
    mutate!("structures.seat", |w: &mut World| w.structures.seat[1] ^= 1);
    mutate!("structures.hp", |w: &mut World| w.structures.hp[1] += 1);
    mutate!("structures.pos", |w: &mut World| w.structures.pos[1][1] =
        Fx(w.structures.pos[1][1].raw() + 1));
    mutate!("structures.kind", |w: &mut World| w.structures.kind[1] ^= 1);
    mutate!("structures.program", |w: &mut World| w
        .structures
        .program[1] ^= 1);
    mutate!("structures.kw_draw", |w: &mut World| w
        .structures
        .kw_draw[1] += 1);
    mutate!("structures.progress", |w: &mut World| w
        .structures
        .progress[1] += 1);
    mutate!("structures.powered", |w: &mut World| w
        .structures
        .powered[1] ^= 1);

    // Projectiles.
    mutate!("projectiles.id", |w: &mut World| w.projectiles.id[0] ^= 1);
    mutate!("projectiles.seat", |w: &mut World| w.projectiles.seat[0] ^=
        1);
    mutate!("projectiles.active", |w: &mut World| w
        .projectiles
        .active[0] ^= 1);
    mutate!("projectiles.pos", |w: &mut World| w.projectiles.pos[0][0] =
        Fx(w.projectiles.pos[0][0].raw() + 1));
    mutate!("projectiles.vel", |w: &mut World| w.projectiles.vel[0][2] =
        Fx(w.projectiles.vel[0][2].raw() + 1));
    mutate!("projectiles.dmg", |w: &mut World| w.projectiles.dmg[0] += 1);
    mutate!("projectiles.ttl", |w: &mut World| w.projectiles.ttl[0] ^= 1);
    mutate!("projectiles.target", |w: &mut World| w.projectiles.target
        [0] = Some(4_244));

    // Credits.
    mutate!("credits.n", |w: &mut World| w.credits.n[5] ^= 1);
    mutate!("credits.seat", |w: &mut World| w.credits.seat[5][0] ^= 1);
    mutate!("credits.dmg", |w: &mut World| w.credits.dmg[5][2] ^= 1);

    // Seats.
    mutate!("seats.cash", |w: &mut World| w.seats.rows[1].cash += 1);
    mutate!("seats.kw_supply", |w: &mut World| w.seats.rows[1]
        .kw_supply += 1);
    mutate!("seats.kw_draw", |w: &mut World| w.seats.rows[1].kw_draw +=
        1);
    mutate!("seats.kills", |w: &mut World| w.seats.rows[1].kills += 1);
    mutate!("seats.brownouts", |w: &mut World| w.seats.rows[1]
        .brownouts += 1);
    mutate!("seats.units_alive", |w: &mut World| w.seats.rows[1]
        .units_alive += 1);
    mutate!("seats.spent", |w: &mut World| w.seats.rows[1].spent += 1);

    // Voxels: the table's canonical encoding is the per-chunk digests, so a
    // change to a single voxel must reach the hash through one of them.
    mutate!(
        "voxels (one voxel, through its chunk digest)",
        |w: &mut World| {
            w.voxels.crater(40, 40, 4, 1);
            w.voxels.refresh_digests();
        }
    );

    // Header.
    mutate!("header.tick", |w: &mut World| w.tick += 1);
}

/// Every table is reached by the fold, and every table's tag is distinct. A
/// table added to the enum and forgotten in `rehash` would otherwise hash as a
/// constant for ever.
#[test]
fn the_table_list_is_consistent() {
    let mut seen = [false; TABLE_COUNT];
    for (i, t) in Table::ALL.iter().enumerate() {
        assert_eq!(t.index(), i, "Table::ALL and Table::index disagree");
        seen[t.index()] = true;
    }
    assert!(seen.iter().all(|s| *s));
    for a in Table::ALL {
        for b in Table::ALL {
            if a != b {
                assert_ne!(a.tag(), b.tag(), "two tables share a tag");
                assert_ne!(a.name(), b.name(), "two tables share a name");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// cost-model independence
// ---------------------------------------------------------------------------

/// The hash stream does not depend on the repath cap or on the clock
/// calibration. This is what makes the three-OS hash comparison in CI a
/// controlled experiment: all three machines calibrate differently.
#[test]
fn the_hash_stream_ignores_the_repath_cap_and_the_calibration() {
    let stream = |cap: u32, ps: u64| {
        let cfg = Config {
            repath_cap: cap,
            cost: CostConstants::from_calibration(ps),
            ..small(HashMode::Full)
        };
        let mut w = World::new(cfg);
        (0..300)
            .map(|_| {
                w.step();
                w.last_hash
            })
            .collect::<Vec<u64>>()
    };
    let baseline = stream(u32::MAX, 0);
    assert_eq!(
        baseline,
        stream(8, 0),
        "the repath cap reached hashed state"
    );
    assert_eq!(
        baseline,
        stream(32, 0),
        "the repath cap reached hashed state"
    );
    // A calibration of 4 ps/iter is absurd, which is the point: it makes the
    // charged workload 250x larger without touching a hashed field.
    assert_eq!(
        baseline,
        stream(16, 4_000),
        "the clock calibration reached hashed state"
    );
}

/// The repath burst rate — a `PLACEHOLDER` this spike sweeps — is outside hashed
/// state too, and it **preserves G2's measured mean of 9.45 repaths per
/// crater** whatever it is set to. Without that scaling, the 1-in-64 burst
/// inflated mean demand by 10.9%, i.e. about 1.1 ms of the 11.29 ms mean that
/// G3'-b is judged on.
#[test]
fn the_burst_rate_moves_the_tail_and_not_the_mean() {
    let run = |burst: u32| {
        let cfg = Config {
            units: 300,
            burst_one_in: burst,
            ..small(HashMode::Full)
        };
        let mut w = World::new(cfg);
        let mut hashes: Vec<u64> = Vec::new();
        let mut craters: u64 = 0;
        for _ in 0..4_000 {
            craters += u64::from(w.step().craters);
            hashes.push(w.last_hash);
        }
        // milli-repaths of demand per crater
        (w.path.demand_total / craters.max(1), hashes)
    };
    let (mean_off, h_off) = run(0);
    let (mean_64, h_64) = run(64);
    let (mean_128, h_128) = run(128);
    // G2's measured 9.45 per crater, in per-mille, at 300 units.
    for (label, m) in [
        ("off", mean_off),
        ("1-in-64", mean_64),
        ("1-in-128", mean_128),
    ] {
        assert!(
            m.abs_diff(9_450) * 100 <= 9_450 * 3,
            "burst {label}: mean demand {m} milli-repaths per crater is more than 3% off G2's 9450"
        );
    }
    assert_eq!(h_off, h_64, "the burst rate reached hashed state");
    assert_eq!(h_off, h_128, "the burst rate reached hashed state");
}

/// The eleven phases run in the declared order and each is reachable.
#[test]
fn every_phase_is_indexed_once_and_named_once() {
    for (i, p) in Phase::ALL.iter().enumerate() {
        assert_eq!(p.index(), i);
    }
    for a in Phase::ALL {
        for b in Phase::ALL {
            if a != b {
                assert_ne!(a.name(), b.name());
            }
        }
    }
    // Calling the phases individually is what the harness does; it must equal
    // calling `step`.
    let mut a = World::new(small(HashMode::Full));
    let mut b = World::new(small(HashMode::Full));
    for _ in 0..80 {
        a.step();
        for p in Phase::ALL {
            b.phase(p);
        }
        assert_eq!(a.last_hash, b.last_hash);
    }
}

// ---------------------------------------------------------------------------
// kill credit
// ---------------------------------------------------------------------------

#[test]
fn apportionment_sums_to_the_bounty_and_ties_go_to_the_lowest_seat() {
    let mut out: Vec<(u8, i64)> = Vec::new();

    // Three equal contributors and a bounty that does not divide: the two extra
    // units go to the two lowest seat ids.
    apportion(100, &[(0, 1), (1, 1), (2, 1)], &mut out);
    assert_eq!(out, vec![(0, 34), (1, 33), (2, 33)]);
    assert_eq!(out.iter().map(|e| e.1).sum::<i64>(), 100);

    // The same, with the entries presented in a different order: the shares
    // follow the seat, not the position.
    apportion(100, &[(2, 1), (1, 1), (0, 1)], &mut out);
    let mut sorted = out.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, vec![(0, 34), (1, 33), (2, 33)]);

    // Two contributors with equal remainders.
    apportion(7, &[(1, 1), (3, 1)], &mut out);
    assert_eq!(out, vec![(1, 4), (3, 3)]);

    // Proportionality is exact, not rounded through anything wider.
    apportion(1_200, &[(0, 10), (1, 20), (2, 70)], &mut out);
    assert_eq!(out, vec![(0, 120), (1, 240), (2, 840)]);

    // Degenerate inputs return nothing rather than panicking.
    apportion(0, &[(0, 1)], &mut out);
    assert!(out.is_empty());
    apportion(100, &[], &mut out);
    assert!(out.is_empty());
    apportion(100, &[(0, 0), (1, 0)], &mut out);
    assert!(out.is_empty());
}

/// The counter cap binds, and on a damage tie the **highest** seat id is the one
/// displaced — so the lowest seat keeps its slot, which is what "ties to the
/// lowest seat id" means everywhere else.
#[test]
fn the_credit_cap_evicts_the_highest_seat_on_a_tie() {
    let mut c = CreditTable::with_counts(4, 1, 1);
    c.record(0, 1, 50);
    c.record(0, 2, 50);
    c.record(0, 3, 50);
    assert_eq!(c.n[0], u8::try_from(MAX_CREDITS).unwrap());
    // A fourth damager with more damage than the smallest: seat 3 is displaced,
    // not seat 1, because on the (50, 50, 50) tie the highest seat id loses.
    c.record(0, 0, 60);
    let (e, n) = c.entries(0);
    assert_eq!(&e[..n], &[(0, 60), (1, 50), (2, 50)]);
    // A fourth damager with less damage than the smallest changes nothing.
    c.record(0, 7, 10);
    let (e, n) = c.entries(0);
    assert_eq!(&e[..n], &[(0, 60), (1, 50), (2, 50)]);
    // Rows stay sorted by the unique seat id.
    for row in [0usize, 4, 5] {
        let (en, n) = c.entries(row);
        for k in 1..n {
            assert!(en[k - 1].0 < en[k].0);
        }
    }
}

#[test]
fn the_credit_slot_map_covers_every_asset_and_nothing_else() {
    let c = CreditTable::with_counts(5, 3, 2);
    assert_eq!(c.slot(0), Some(0));
    assert_eq!(c.slot(4), Some(4));
    assert_eq!(c.slot(5), None);
    assert_eq!(c.slot(g3_tick_budget::BEACON_ID_BASE), Some(5));
    assert_eq!(c.slot(g3_tick_budget::BEACON_ID_BASE + 2), Some(7));
    assert_eq!(c.slot(g3_tick_budget::BEACON_ID_BASE + 3), None);
    assert_eq!(c.slot(g3_tick_budget::STRUCT_ID_BASE), Some(8));
    assert_eq!(c.slot(g3_tick_budget::STRUCT_ID_BASE + 1), Some(9));
    assert_eq!(c.slot(g3_tick_budget::STRUCT_ID_BASE + 2), None);
    assert_eq!(c.slot(g3_tick_budget::PROJ_ID_BASE), None);
    assert_eq!(c.len(), 10);
}

// ---------------------------------------------------------------------------
// broadphase
// ---------------------------------------------------------------------------

/// The ring search agrees with a brute-force scan on 1_000 random queries,
/// answer for answer — including which of two equidistant candidates wins.
#[test]
fn the_range_query_agrees_with_a_brute_force_scan() {
    let n = 400usize;
    let mut rng = StreamRng::new(0xC0FF_EE00_1234_5678, Stream::Map, 0, 0, 0);
    let mut pos: Vec<[Fx; 3]> = Vec::with_capacity(n);
    let mut seat: Vec<u8> = Vec::with_capacity(n);
    let mut id: Vec<u32> = Vec::with_capacity(n);
    let mut live: Vec<bool> = Vec::with_capacity(n);
    for i in 0..n {
        let x = rng.range_i32(0, 383 << 16);
        let y = rng.range_i32(0, 383 << 16);
        let z = rng.range_i32(0, 63 << 16);
        pos.push([Fx(x), Fx(y), Fx(z)]);
        seat.push(u8::try_from(i % 4).unwrap());
        id.push(u32::try_from(i).unwrap());
        // A fifth of the population is dead, so the grid's exclusion path is
        // exercised rather than assumed.
        live.push(!i.is_multiple_of(5));
    }
    let cells: Vec<u16> = (0..n)
        .map(|i| if live[i] { cell_of(pos[i]) } else { NO_CELL })
        .collect();
    let mut grid = Grid::with_capacity(n);
    grid.rebuild(&cells);
    assert_eq!(
        usize::try_from(grid.occupancy()).unwrap(),
        live.iter().filter(|l| **l).count()
    );

    let p = Population {
        grid: &grid,
        pos: &pos,
        seat: &seat,
        id: &id,
    };
    let mut queried = 0usize;
    let mut found = 0usize;
    for q in 0..1_000u32 {
        let mut qr = StreamRng::new(0xC0FF_EE00_1234_5678, Stream::Combat, q, 0, 0);
        let from = [
            Fx(qr.range_i32(0, 383 << 16)),
            Fx(qr.range_i32(0, 383 << 16)),
            Fx(qr.range_i32(0, 63 << 16)),
        ];
        let my_seat = u8::try_from(qr.range_i32(0, 3)).unwrap();
        let mut examined = 0u32;
        let a = nearest_enemy(p, from, my_seat, &mut examined);
        let b = nearest_enemy_brute(&pos, &seat, &id, &live, from, my_seat);
        assert_eq!(a, b, "query {q} disagreed with the brute-force scan");
        queried += 1;
        if a.is_some() {
            found += 1;
        }
    }
    assert_eq!(queried, 1_000);
    assert!(found > 900, "the query set found almost nothing: {found}");
}

/// Two candidates at exactly the same distance: the lower id must win, and the
/// ring search must not stop before it has seen both.
#[test]
fn the_range_query_tiebreak_is_total() {
    let pos = vec![
        [Fx::from_voxels(100), Fx::from_voxels(100), Fx::ZERO],
        [Fx::from_voxels(140), Fx::from_voxels(100), Fx::ZERO],
        [Fx::from_voxels(60), Fx::from_voxels(100), Fx::ZERO],
    ];
    let seat = vec![0u8, 1, 1];
    let id = vec![0u32, 77, 12];
    let live = vec![true, true, true];
    let cells: Vec<u16> = pos.iter().map(|p| cell_of(*p)).collect();
    let mut grid = Grid::with_capacity(pos.len());
    grid.rebuild(&cells);
    let p = Population {
        grid: &grid,
        pos: &pos,
        seat: &seat,
        id: &id,
    };
    let mut examined = 0u32;
    let a = nearest_enemy(p, pos[0], 0, &mut examined);
    let b = nearest_enemy_brute(&pos, &seat, &id, &live, pos[0], 0);
    assert_eq!(a, b);
    assert_eq!(a.map(|c| c.1), Some(12), "the lower id must win the tie");
}

// ---------------------------------------------------------------------------
// voxels
// ---------------------------------------------------------------------------

#[test]
fn the_chunk_store_shares_untouched_chunks_and_copies_on_write() {
    let before = VoxelStore::new();
    let mut after = before.clone();

    // A clone shares every chunk.
    for (a, b) in before.chunks().iter().zip(after.chunks()) {
        assert!(Arc::ptr_eq(a, b), "a clone copied a chunk");
    }

    // The crater is placed where there is certainly rock: below the minimum
    // ground height of 12.
    let removed = after.crater(100, 100, 5, 3);
    assert!(removed > 0, "the crater removed nothing");
    let (touched, _) = index_of(100, 100, 5);

    let mut copied = 0usize;
    for (i, (a, b)) in before.chunks().iter().zip(after.chunks()).enumerate() {
        if Arc::ptr_eq(a, b) {
            assert_ne!(i, touched, "the written chunk was not copied");
        } else {
            copied += 1;
        }
    }
    assert!(copied >= 1, "no chunk was copied on write");
    assert!(
        copied <= 4,
        "a radius-3 crater touched {copied} chunks; the store is not copy-on-write per chunk"
    );
    assert_eq!(before.get(100, 100, 5), g3_tick_budget::voxels::ROCK);
    assert_eq!(after.get(100, 100, 5), g3_tick_budget::voxels::AIR);
    assert!(after.copies >= 1);
}

#[test]
fn only_dirtied_chunk_digests_are_refreshed() {
    let mut s = VoxelStore::new();
    let before: Vec<u64> = s.digests().to_vec();
    assert_eq!(s.refresh_digests(), 0, "a settled store had dirty chunks");
    s.crater(200, 200, 6, 3);
    let n = s.refresh_digests();
    assert!((1..=4).contains(&n), "unexpected dirty chunk count {n}");
    let after: Vec<u64> = s.digests().to_vec();
    let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
    assert_eq!(
        usize::try_from(n).unwrap(),
        changed,
        "the refreshed chunks and the changed digests disagree"
    );
}

// ---------------------------------------------------------------------------
// power
// ---------------------------------------------------------------------------

/// The brownout order is total: no two beacons share a shed key, so no tie can
/// be broken by scan order.
#[test]
fn the_brownout_order_is_total() {
    let w = World::new(Config {
        seats: 4,
        units: 60,
        beacons: 40,
        ..Config::default()
    });
    let mut keys: Vec<(u8, u8, u32)> = (0..w.beacons.len())
        .map(|i| power::shed_key(w.beacons.priority[i], w.beacons.seat[i], w.beacons.id[i]))
        .collect();
    let n = keys.len();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), n, "two beacons share a brownout shed key");
    assert_eq!(
        w.shed_order.len(),
        n,
        "the shed order does not cover every beacon"
    );
    let mut seen: Vec<u32> = w.shed_order.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), n, "the shed order repeats a beacon");
}

/// The gate configuration actually browns out, so the phase measured is the
/// phase that will run.
#[test]
fn the_gate_configuration_exercises_the_brownout_path() {
    let mut w = World::new(Config::default());
    let mut brownouts = 0u32;
    for _ in 0..40 {
        brownouts += w.step().brownouts;
    }
    assert!(
        brownouts > 0,
        "no beacon was ever shed; the power phase is in the table but not in the measurement"
    );
}

// ---------------------------------------------------------------------------
// the run as a whole
// ---------------------------------------------------------------------------

/// A run at the gate configuration keeps a live world for a whole segment: the
/// rebuild sweep holds the population up, so the last minute is not measuring
/// an empty map.
#[test]
fn the_world_stays_alive_for_a_whole_segment() {
    let mut w = World::new(Config {
        units: 120,
        beacons: 16,
        ..Config::default()
    });
    for _ in 0..2_000 {
        w.step();
    }
    let alive = (0..w.units.len()).filter(|i| w.units.alive(*i)).count();
    assert!(
        alive * 2 >= w.units.len(),
        "only {alive} of {} units survived 2 000 ticks",
        w.units.len()
    );
    assert!(w.voxels.edits >= 1_900, "the destruction stream stalled");
}

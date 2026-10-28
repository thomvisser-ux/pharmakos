// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Zero allocations per tick, asserted from day one (G3′ §9.17).
//!
//! > *"Zero allocations per tick is achievable and worth defending. It took
//! > removing exactly one `Vec` return from the kill-credit settlement path.
//! > With it, plan §5's whole allocator risk row evaporates and the budget
//! > stops being allocator-dependent. The real sim should have an
//! > allocations-per-tick assertion in its perf gate from S1, not as a later
//! > hardening task."*
//!
//! It is here rather than at S1 because it is far cheaper to keep than to
//! restore: every table below fixes its count at construction, the canonical
//! encoder reuses one buffer, the broadphase's counting sort writes into arrays
//! allocated once, and the voxel phase's edit queue, pending set and crater
//! scratch are all sized to the map at construction. The measured loop craters
//! on every tick for that last reason: an empty edit queue would leave the
//! whole voxel phase outside the assertion. A pull request that adds a `Vec` to a tick phase
//! fails here, in itself, rather than in a budget measurement three stages
//! later.
//!
//! **One test in this file, on purpose**, and the counter is **per thread**.
//! Both halves matter. A process-wide counter charges this test for every
//! allocation any other thread makes, and one of those threads is libtest's
//! own: it prints "has been running for over 60 seconds" from the main thread,
//! which allocates, and a test long enough to earn that line then fails for a
//! reason that has nothing to do with the sim. Counting on the allocating
//! thread makes the assertion say what it means.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use pharmakos_sim::encoding::Enc;
use pharmakos_sim::runner::{DEFAULT_ROUND_LIMIT, Runner};
use pharmakos_sim::voxels::VoxelEdit;
use pharmakos_sim::{MatchSettings, RulesTable, World, WorldConfig};

thread_local! {
    /// How many times **this thread** has allocated.
    ///
    /// `const`-initialised and holding a type with no destructor, so the slot
    /// needs no lazy setup and registers no teardown hook — which is what makes
    /// it safe to touch from inside the global allocator, where anything that
    /// allocated would recurse for ever.
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

/// Count one allocation against the calling thread.
fn charge() {
    // `try_with` rather than `with`: during thread teardown the slot is gone,
    // and an allocation then is not this test's business.
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get().saturating_add(1)));
}

/// How many times this thread has allocated so far.
fn allocations() -> u64 {
    ALLOCATIONS.try_with(Cell::get).unwrap_or(0)
}

/// The system allocator, counting.
struct Counting;

// SAFETY: every method forwards to `System` unchanged; the only addition is a
// relaxed counter increment, which cannot affect the allocation itself.
//
// `unsafe_code` is denied workspace-wide (`[workspace.lints.rust]`). A
// `GlobalAlloc` implementation cannot be written without `unsafe`, and the
// alternative — not asserting the property — is what G3′ warned against. The
// allowance is scoped to this one item, in a test target, and is raised in the
// pull request rather than edited into the contract lint table (the same route
// skeleton plan decision 12 sets out for gdext's scoped allow).
#[allow(
    unsafe_code,
    reason = "a GlobalAlloc implementation cannot be written without unsafe; scoped to this test's counting allocator, which only forwards to System"
)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        charge();
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        charge();
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        charge();
        unsafe { System.alloc_zeroed(layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[test]
fn a_tick_allocates_nothing() {
    let rules = RulesTable::load(
        &std::path::Path::new("..")
            .join("..")
            .join("rules")
            .join("rules.v1.json"),
    )
    .expect("the committed rules table loads");
    let world = World::new(&WorldConfig {
        match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
        seats: pharmakos_sim::DETERMINISM_SEATS,
        // No walkers. The determinism harness fielded fifty a seat until S1's
        // real segment (decision 16), and this test with it. A probe with
        // fifty a seat on the committed playbook (S1's `tune` fix pass) held
        // both allocation assertions below, but not the fixed-stride one: with
        // walkers wandering through the steady window the encoding's length
        // changed between ticks, so the window is no longer steady. The
        // wide-table coverage lives in `tests/determinism.rs` (`TEST_MOVERS`);
        // this test covers the real playbook's in-tick growth instead.
        rules,
        match_settings: MatchSettings {
            // Long enough that the measured loop never reaches the segment's
            // end: the segment-end freeze allocates by design (it captures a
            // whole world) and belongs outside the assertion, exactly like
            // `Runner::new` below.
            segment_lengths_ms: vec![600_000],
            round_limit: DEFAULT_ROUND_LIMIT,
        },
    })
    .expect("the rules table describes a map and a broadphase grid");
    // Driven through the `Runner`, and **in a Push**, because `World::phase_match`
    // returns at its first gate outside one: a world stepped by hand stays in
    // its opening Lull, and everything T10 put in that phase — the beacon
    // deaths, elimination, the respawns, the one-tick match-end rule, the
    // segment's end and every event they emit — would sit outside the counted
    // loop.
    let mut runner = Runner::new(world);
    // Every seat seals the harness playbook, for the same reason: the decision
    // phase returns immediately for a seat that sealed nothing, so without this
    // the whole interpreter — the `covering` resolver with its "nearest"
    // estimates and its spiral, the walk in, the deploy's charge, the initial
    // settings' interface rows, the Build target written `on` the covered
    // vent, the hold, the handler scan and the guaranteed tail — would sit
    // outside the one assertion that a tick allocates nothing (T11). Since
    // S1's real determinism segment (decision 16) the playbook is the
    // committed copy of Hold & Build's instantiation the chain runs.
    let playbook = pharmakos_sim::determinism_playbook(
        &std::path::Path::new("..")
            .join("..")
            .join(pharmakos_sim::DETERMINISM_PLAYBOOK_PATH),
    )
    .expect("the committed harness playbook reads");
    let sealed = pharmakos_sim::seal_determinism_playbooks(runner.world_mut(), &playbook)
        .expect("the harness playbook compiles");
    assert_eq!(sealed, pharmakos_sim::DETERMINISM_SEATS, "every seat seals");
    assert!(runner.begin_push().unwrap(), "a match opens in a Lull");
    let mut enc = Enc::with_capacity(64 * 1024);

    // Warm up: the first tick may still grow the encoder's buffer, and a
    // capacity that is right for tick 1 is right for tick 10 000 because every
    // table's count is fixed at construction.
    // The warm-up craters too, so the voxel phase's own scratch — the edit
    // queue, the store's pending and settled lists, the crater's touched list —
    // is paid for out here and the measured loop below sees it at its steady
    // capacity.
    for step in 0..50 {
        crater_at(runner.world_mut(), step);
        collapse_at(runner.world_mut(), step);
        runner
            .step()
            .unwrap()
            .expect("the segment outlasts this test");
        runner.clear_events();
        runner.world().encode(&mut enc);
    }
    // The growth window. The harness playbook adds **rows to tables** inside
    // a tick: its deploy a beacon (`BeaconTable::reserve`), the Build target it
    // writes a structure (the structure table's room), and the fabricator the
    // build drone that target asks for (`UNIT_TABLE_ROOM`). Each table makes
    // room at construction so the `push` cannot allocate, and that claim is
    // only worth anything over growth the test actually performs: at the
    // committed table the deploys land at ticks 631 to 741 and the Generators
    // are finished by tick 1 300. It is counted separately from the steady
    // loop below because the encoding legitimately *grows* when a row lands —
    // which is exactly what the fixed-stride assertion further down would
    // otherwise read as a bug.
    let before_growth = allocations();
    let beacons_before = runner.world().beacons().len();
    let structures_before = runner.world().structures().len();
    let units_before = runner.world().units().len();
    for step in 0..GROWTH_TICKS {
        crater_at(runner.world_mut(), step + 50);
        collapse_at(runner.world_mut(), step + 50);
        runner
            .step()
            .unwrap()
            .expect("the segment outlasts this test");
        runner.clear_events();
        runner.world().encode(&mut enc);
    }
    let growing = allocations() - before_growth;
    assert_eq!(
        growing, 0,
        "the {GROWTH_TICKS} ticks that deploy a beacon, queue a Generator and fabricate a drone per \
         seat allocated {growing} times. Each adds a table row inside a tick, and the room for it \
         is reserved at construction precisely so that it cannot allocate."
    );
    assert!(
        runner.world().beacons().len() > beacons_before
            && runner.world().structures().len() > structures_before
            && runner.world().units().len() > units_before,
        "the harness playbook's deploy, its Generator and the drone it asks for all landed inside \
         the counted window; without them this file never exercises the in-tick table growth \
         there is"
    );

    let capacity_after_warmup = enc.encoded_len();

    let regions_before = runner.world().collapse_counts().regions;
    let before = allocations();
    for step in 0..500 {
        // Every tick of the measured loop goes through the voxel phase with
        // work in it: a crater is the edit S2 will file by the thousand, and an
        // empty queue would leave `settle`, `mark` and `crater` outside the
        // property this file exists to defend.
        crater_at(runner.world_mut(), step + 50 + GROWTH_TICKS);
        // And a collapse every tick (S2's plan, `fall`): the support rule's
        // search, its fall and its labels' clearing inside the property too.
        collapse_at(runner.world_mut(), step + 50 + GROWTH_TICKS);
        runner
            .step()
            .unwrap()
            .expect("the segment outlasts this test");
        // A host drains every tick, and the bus is fixed-capacity: draining it
        // is what keeps an event emission inside the assertion instead of
        // silently overflowing past it.
        runner.clear_events();
        runner.world().encode(&mut enc);
    }
    let during = allocations() - before;
    let fell = runner.world().collapse_counts().regions - regions_before;
    assert!(
        fell >= 500,
        "a region fell on every measured tick, and a crater sometimes drops one too: {fell}"
    );

    assert_eq!(
        during, 0,
        "500 ticks allocated {during} times. A tick must allocate nothing: fix the count at \
         construction, reuse the caller's buffer, or hand the scratch space in."
    );
    assert_eq!(
        enc.encoded_len(),
        capacity_after_warmup,
        "the canonical encoding changed length between ticks, which means the encoder is not \
         fixed-stride after all"
    );

    crowd_allocates_nothing(&mut runner, &mut enc);
}

/// The attacking crowds' half of [`a_tick_allocates_nothing`].
fn crowd_allocates_nothing(runner: &mut Runner, enc: &mut Enc) {
    // **A moving crowd under combat** (S2's `atk`; decisions-log item 137 (2)
    // (j)): two Attack beacons' forces, launched at each other's staging
    // ground across open terrain, one round the other's avoid area, so the
    // Attack update, the surcharged routes, the raiders' fire, the damage,
    // the kill credit, the deaths and the wrecks all run inside the counted
    // window. Laid out between ticks, where a placement may allocate. The
    // fixed-stride check above is not repeated here: a damaged asset books a
    // credit row and a dead one leaves a wreck, rows of tables sized at
    // construction, so the encoding grows by design while nothing allocates.
    let lanes = crowd(runner.world_mut());
    for _ in 0..20 {
        runner
            .step()
            .unwrap()
            .expect("the segment outlasts this test");
        runner.clear_events();
        runner.world().encode(enc);
    }
    let before_crowd = allocations();
    let shots_before = runner.world().combat_counts().shots();
    for _ in 0..CROWD_TICKS {
        runner
            .step()
            .unwrap()
            .expect("the segment outlasts this test");
        runner.clear_events();
        runner.world().encode(enc);
    }
    let crowded = allocations() - before_crowd;
    let launched = lanes.iter().all(|beacon| {
        runner
            .world()
            .attack()
            .row_of(*beacon)
            .is_some_and(|row| row.waves_used > 0)
    });
    assert!(launched, "both forces launched");
    assert!(
        runner.world().combat_counts().shots() > shots_before,
        "the crowds fought inside the counted window"
    );
    assert_eq!(
        crowded, 0,
        "{CROWD_TICKS} ticks of two attacking crowds allocated {crowded} times"
    );
}

/// The ticks the attacking crowds are counted over.
const CROWD_TICKS: u32 = 400;

/// Raiders each crowd fields.
const CROWD: u32 = 10;

/// Lay out two Attack beacons, of seats 0 and 1, sixty voxels apart in open
/// ground, each with [`CROWD`] raiders and aimed at the other's beacon; seat
/// 0's force routes round an avoid area between them. Returns the two
/// beacons.
fn crowd(world: &mut World) -> [pharmakos_sim::tables::BeaconId; 2] {
    use pharmakos_sim::attack::{Aim, AttackSettings};
    use pharmakos_sim::seams::MandateKind;
    use pharmakos_sim::tables::{ColumnBox, PRIORITY_NORMAL, SeatId, UnitKind};
    let size = world.voxels().size();
    let mid_x = i32::try_from(size[0].div_euclid(2)).expect("a column");
    let mid_y = i32::try_from(size[1].div_euclid(2)).expect("a column");
    let west = world.standing_point([mid_x - 12, mid_y, 0]);
    let east = world.standing_point([mid_x + 12, mid_y, 0]);
    let voxel = |at: [pharmakos_sim::math::fixed::Fx; 3]| {
        at.map(|axis| i16::try_from(axis.floor_voxels()).expect("a voxel coordinate"))
    };
    let mut lanes = Vec::new();
    for (seat, here, there) in [(0_u8, west, east), (1_u8, east, west)] {
        let beacon = world
            .place_beacon_directly(
                SeatId::new(seat),
                here,
                MandateKind::Attack,
                PRIORITY_NORMAL,
            )
            .expect("an Attack beacon");
        let avoid = if seat == 0 {
            let x = i16::try_from(mid_x).expect("a column");
            let y = i16::try_from(mid_y).expect("a column");
            vec![ColumnBox::new([x - 4, y - 4], [x + 4, y + 4]).expect("a box")]
        } else {
            Vec::new()
        };
        assert!(world.set_attack(
            beacon,
            AttackSettings {
                aim: Aim::Voxel(voxel(there)),
                force_size: Some(CROWD),
                waves: 1,
                ..AttackSettings::default()
            },
            &avoid,
        ));
        for _ in 0..CROWD {
            world
                .field_unit(SeatId::new(seat), beacon, UnitKind::Raider, here)
                .expect("a raider");
        }
        lanes.push(beacon);
    }
    <[pharmakos_sim::tables::BeaconId; 2]>::try_from(lanes).expect("two lanes")
}

/// The ticks counted for table growth, after the 50-tick warm-up: long enough
/// that every placed seat's deploy, Generator and fabricated drone land inside
/// them (the last Generator is finished at tick 1 300 at the committed table).
const GROWTH_TICKS: u32 = 1_400;

/// Queue one crater on a fresh patch of ground, so the edit actually changes
/// voxels and the whole voxel phase runs.
///
/// The columns walk a coprime stride across the map, so a later tick never
/// craters air a former tick already removed; the `z` is the column's own
/// surface, so the ball always has rock to take.
fn crater_at(world: &mut World, step: u32) {
    let size = world.voxels().size();
    let width = i32::try_from(size[0]).expect("the map fits in an i32");
    let depth = i32::try_from(size[1]).expect("the map fits in an i32");
    let x = i32::try_from(step.wrapping_mul(37) % 4096).expect("a column index") % width;
    let y = i32::try_from(step.wrapping_mul(53) % 4096).expect("a column index") % depth;
    let z = world.voxels().top_solid_z(x, y).unwrap_or(0);
    assert!(
        world
            .request_voxel_edit(VoxelEdit::Crater {
                centre: [x, y, z],
                radius: 3,
                by: None,
            })
            .is_ok(),
        "the edit queue is full at step {step}"
    );
}

/// Queue a collapse on a fresh column: a voxel two above the column's top,
/// joined to it by one voxel that the same tick's last edit removes, so the
/// voxel above is a detached region of one that falls a voxel.
///
/// The columns walk a different coprime stride from [`crater_at`]'s, offset
/// across the map, so a collapse and a crater rarely share a column, and a
/// shared one still collapses: the edits run in the order they are filed.
fn collapse_at(world: &mut World, step: u32) {
    let size = world.voxels().size();
    let width = i32::try_from(size[0]).expect("the map fits in an i32");
    let depth = i32::try_from(size[1]).expect("the map fits in an i32");
    let x = (i32::try_from(step.wrapping_mul(41) % 4096).expect("a column index") + 101) % width;
    let y = (i32::try_from(step.wrapping_mul(59) % 4096).expect("a column index") + 211) % depth;
    let z = world.voxels().top_solid_z(x, y).unwrap_or(0);
    for (dz, material) in [
        (2, pharmakos_sim::voxels::Material::STONE),
        (1, pharmakos_sim::voxels::Material::STONE),
        (1, pharmakos_sim::voxels::Material::AIR),
    ] {
        assert!(
            world
                .request_voxel_edit(VoxelEdit::Set {
                    at: [x, y, z + dz],
                    material,
                    by: None,
                })
                .is_ok(),
            "the edit queue is full at step {step}"
        );
    }
}

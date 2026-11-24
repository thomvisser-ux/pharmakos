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
//! The counter is **per thread**. A process-wide counter charges a test for
//! every allocation any other thread makes, and one of those threads is
//! libtest's own: it prints "has been running for over 60 seconds" from the
//! main thread, which allocates, and a test long enough to earn that line then
//! fails for a reason that has nothing to do with the sim. Counting on the
//! allocating thread makes each assertion say what it means, so the file's
//! tests (the tick, the breach search at its most work and from a cloned
//! scratch, the near estimate and the `toward` tick), which libtest runs on
//! its threads side by side, each count only their own allocations.
//!
//! **The tick tests run in the default build only.** They sit in the `tick`
//! module, behind `not(feature = "research")`. The `research` feature gates
//! one item in the sim, `pub mod research;` (`fork`, a `World::clone`, and
//! `fork_and_step`), and enables nothing, so the research build's tick is the
//! default build's tick, function for function: running these tests again
//! under the feature counted the same code twice, at opt-level 0, for two to
//! four minutes a leg. The premise is checked rather than assumed, and in
//! both builds: [`the_research_feature_gates_the_fork_module_and_nothing_else`]
//! is outside the module, so a manifest or a crate that turns the feature on
//! still runs it, and it reads the sim's sources and manifest and goes red the
//! day the feature reaches anything else, when the tests must run under it as
//! well.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

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
    // This runs inside the global allocator, so it may not fail loudly: a
    // panic or a print here allocates, which re-enters the allocator and
    // recurses. `try_with` rather than `with` because during thread teardown
    // the slot is gone, and an allocation then is no test's business; the
    // `Err` of that case is dropped here and nowhere else, and no count is
    // read then (`allocations` refuses a missing slot). A `u64` of
    // allocations cannot reach its ceiling in a test, and `saturating_add`
    // keeps the overflow check's panic out of the allocator all the same.
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get().saturating_add(1)));
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

/// The default build's tests: the tick and the searches, each counted on its
/// own thread. Behind `not(feature = "research")` because the research build's
/// tick is this build's, function for function, which
/// [`the_research_feature_gates_the_fork_module_and_nothing_else`] checks in both
/// builds.
#[cfg(not(feature = "research"))]
mod tick {
    use super::ALLOCATIONS;
    use std::cell::Cell;

    use pharmakos_sim::encoding::Enc;
    use pharmakos_sim::runner::{DEFAULT_ROUND_LIMIT, Runner};
    use pharmakos_sim::voxels::VoxelEdit;
    use pharmakos_sim::{MatchSettings, RulesTable, World, WorldConfig};

    /// How many times this thread has allocated so far. Called only from a test's
    /// own body, where the slot exists; a missing slot is a failure, never a zero
    /// that would read as "nothing was allocated".
    fn allocations() -> u64 {
        ALLOCATIONS
            .try_with(Cell::get)
            .expect("the counting slot exists on a running test's thread")
    }

    /// Counts the allocations of a tick. The optimiser deletes an allocation whose
    /// value is never read (a `Vec::with_capacity` dropped at once, a `Box` freed
    /// where it was made), so what this counts depends on how the sim was built.
    ///
    /// `cargo xtask ci` always runs this target with the sim at opt-level 0
    /// (`--config 'profile.dev.package.pharmakos-sim.opt-level=0'`, xtask's
    /// `resolved` module), as an invocation of its own, and checks from cargo's
    /// report that it did, so there it counts the tick as written. A plain
    /// `cargo test` runs it against the optimised sim the dev profile builds, and
    /// there it counts only the allocations the optimiser kept.
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
        let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
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
        near_estimates_allocate_nothing(runner.world_mut());
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
        let z = world
            .voxels()
            .top_solid_z(x, y)
            .expect("every column of the determinism map has ground");
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
        let x =
            (i32::try_from(step.wrapping_mul(41) % 4096).expect("a column index") + 101) % width;
        let y =
            (i32::try_from(step.wrapping_mul(59) % 4096).expect("a column index") + 211) % depth;
        let z = world
            .voxels()
            .top_solid_z(x, y)
            .expect("every column of the determinism map has ground");
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

    /// The served walk "nearest" ranks by, and the estimator's clear queries
    /// (lane `p3fix`), drawn here on near pairs: both price the route the router
    /// serves, built in the scratch's
    /// own buffer, so neither allocates, inside a tick or out of one.
    ///
    /// The tick reaches the served walk: the harness playbook's `covering`
    /// resolver ranks "nearest" by it. It does so in the opening ticks, before the
    /// counted windows above (the first decision of each seat), so this half
    /// asserts the zero per call instead, on **the world's own scratch**, the
    /// buffer those ticks use, after checking that they did price walks; near
    /// pairs and far ones both, since the ranker prices either.
    fn near_estimates_allocate_nothing(world: &mut World) {
        use pharmakos_sim::pathing::estimate::{Fog, Speed, estimate, walk_cost};
        let in_ticks = world.scratch_mut().walks().expect("the count fits a u64");
        assert!(
            in_ticks > 0,
            "no served walk was priced inside a tick, so nothing here shows the tick ranks by it"
        );
        // The graph, copied out before counting so that the world's scratch can
        // be borrowed beside it; the copy is the only allocation and it is not
        // counted.
        let surface = world.surface().clone();
        let clusters = world.clusters().clone();
        let speed = Speed::commander(world.rules());
        // The pairs, drawn before counting: a column every 37 cells along the
        // diagonal, each paired with the column 20 cells east and 9 north.
        let [sx, sy, _] = surface.size();
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        let mut far: Vec<(u32, u32)> = Vec::new();
        let mut x = 5;
        while x + 20 < sx && x + 9 < sy {
            if let (Some(a), Some(b)) = (surface.node_of(x, x), surface.node_of(x + 20, x + 9))
                && clusters.touch(a, b) == Ok(true)
                && clusters.connected(&surface, a, b)
            {
                pairs.push((a, b));
            }
            if let (Some(a), Some(b)) = (surface.node_of(x, x), surface.node_of(sx - 1 - x, x))
                && clusters.touch(a, b) == Ok(false)
                && clusters.connected(&surface, a, b)
            {
                far.push((a, b));
            }
            x += 37;
        }
        assert!(far.len() >= 3, "too few far pairs drawn: {}", far.len());
        assert!(
            pairs.len() >= 3,
            "too few near pairs drawn: {}",
            pairs.len()
        );
        let scratch = world.scratch_mut();
        scratch.reset_counters();
        let before = allocations();
        for (a, b) in &pairs {
            let found = estimate(&surface, &clusters, scratch, *a, *b, Fog::Clear, speed);
            assert!(
                matches!(found, Ok(Some(_))),
                "a connected near pair has an estimate"
            );
        }
        for (a, b) in pairs.iter().chain(&far) {
            let walked = walk_cost(&surface, &clusters, scratch, (*a, *b));
            assert!(walked.is_ok(), "a connected pair has a served walk");
        }
        let during = allocations() - before;
        assert_eq!(
            during,
            0,
            "{} near estimates and {} served walks allocated {during} times; the route they price \
         is built in the scratch's own buffer so that none does",
            pairs.len(),
            pairs.len() + far.len()
        );
        assert_eq!(
            scratch.served_estimates().expect("the count fits a u64"),
            u64::try_from(pairs.len()).expect("fits"),
            "every pair drawn was priced at its served walk"
        );
    }

    /// File one `Set` of `material` at `at`, stepping the world to drain the edit
    /// queue when it is full. Outside any counted window.
    fn file_or_flush(world: &mut World, at: [i32; 3], material: pharmakos_sim::voxels::Material) {
        let edit = VoxelEdit::Set {
            at,
            material,
            by: None,
        };
        if world.request_voxel_edit(edit).is_err() {
            world.step(&mut Enc::default()).expect("the edits settle");
            world
                .request_voxel_edit(edit)
                .expect("an emptied queue takes the edit");
        }
    }

    /// A breach search allocates nothing, at its most work and on a cloned
    /// scratch (lane `settle`). The field round the start is the densest the
    /// search meets: a one-high enemy stub on three columns in four, so an
    /// uncrossed state on open ground pushes a walk and a crossing to each of its
    /// eight neighbours. The goal is inside a ring of the seat's own walls, which
    /// it may not demolish, so the search spends its whole budget of states
    /// (`Exhausted`). Its buffers are reserved with the scratch, before the
    /// count; a clone of the scratch (as a cloned or forked world's) keeps the
    /// heap's reservation, so its first search allocates nothing either.
    #[test]
    fn a_breach_search_allocates_nothing_at_its_most_work_or_from_a_clone() {
        use pharmakos_sim::attack::{BreachRefused, BreachScratch, SealedWay, breach_search};
        use pharmakos_sim::tables::{PRIORITY_NORMAL, SeatId};
        use pharmakos_sim::voxels::Material;
        let rules = RulesTable::load(
            &std::path::Path::new("..")
                .join("..")
                .join("rules")
                .join("rules.v1.json"),
        )
        .expect("the committed rules table loads");
        let mut world = World::new(&WorldConfig {
            match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
            seats: pharmakos_sim::DETERMINISM_SEATS,
            rules,
            match_settings: MatchSettings::default(),
        })
        .expect("the rules table describes a map and a broadphase grid");
        let (start, goal) = ([192_i32, 192_i32], [216_i32, 192_i32]);
        let goal_at = world.standing_point([goal[0], goal[1], 0]);
        world
            .place_beacon_directly(
                SeatId::new(0),
                goal_at,
                pharmakos_sim::seams::MandateKind::Defend,
                PRIORITY_NORMAL,
            )
            .expect("a beacon of the seat's own by the goal");
        let own = Material::wall(SeatId::new(0), false).expect("a wall id");
        let enemy = Material::wall(SeatId::new(1), false).expect("a wall id");
        for dy in -4..=4_i32 {
            for dx in -4..=4_i32 {
                if dx.abs() != 4 && dy.abs() != 4 {
                    continue;
                }
                let (x, y) = (goal[0] + dx, goal[1] + dy);
                let top = world.standing_point([x, y, 0])[2].floor_voxels();
                for z in top..top + 3 {
                    file_or_flush(&mut world, [x, y, z], own);
                }
            }
        }
        for dy in -60..=60_i32 {
            for dx in -60..=60_i32 {
                let (x, y) = (start[0] + dx, start[1] + dy);
                let ground = x % 2 == 0 && y % 2 == 0;
                let near_goal = (x - goal[0]).abs() <= 4 && (y - goal[1]).abs() <= 4;
                if ground || near_goal || (dx, dy) == (0, 0) {
                    continue;
                }
                let top = world.standing_point([x, y, 0])[2].floor_voxels();
                file_or_flush(&mut world, [x, y, top], enemy);
            }
        }
        world.step(&mut Enc::default()).expect("the walls settle");
        let surface = world.surface();
        let (from, to) = (
            surface.node_of(start[0], start[1]).expect("a column"),
            surface.node_of(goal[0], goal[1]).expect("a column"),
        );
        let way = SealedWay::of(&world, from, to).expect("the seat's own ring seals the goal");
        let mut scratch = BreachScratch::new(surface.node_count());
        let before = allocations();
        let found = breach_search(&world, &mut scratch, SeatId::new(0), way);
        let searching = allocations() - before;
        assert_eq!(
            found.map(|breach| breach.wall),
            Err(BreachRefused::Exhausted)
        );
        assert_eq!(
            searching, 0,
            "the search at its most work allocated {searching} times"
        );
        let mut cloned = scratch.clone();
        let before = allocations();
        let again = breach_search(&world, &mut cloned, SeatId::new(0), way);
        let from_clone = allocations() - before;
        assert_eq!(
            again.map(|breach| breach.wall),
            Err(BreachRefused::Exhausted)
        );
        assert_eq!(
            from_clone, 0,
            "the first search on a cloned scratch allocated {from_clone} times"
        );
    }

    /// A near estimate allocates nothing from a fresh world's scratch, on a route
    /// longer than any buffer an opening tick could have grown, and after a near
    /// query the router refuses (lane `p3fix`). The served route is built in the
    /// scratch's own buffer, reserved at `Scratch::for_map` for
    /// `MAX_ROUTE_NODES`, and the buffer is handed back on every path, the
    /// refusal's included: a buffer left behind on the error path would be grown
    /// again by the next query.
    #[test]
    fn a_near_estimate_allocates_nothing_from_a_fresh_scratch_or_after_a_refusal() {
        use pharmakos_sim::pathing::estimate::{Fog, Speed, estimate};
        let rules = RulesTable::load(
            &std::path::Path::new("..")
                .join("..")
                .join("rules")
                .join("rules.v1.json"),
        )
        .expect("the committed rules table loads");
        let mut world = World::new(&WorldConfig {
            match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
            seats: pharmakos_sim::DETERMINISM_SEATS,
            rules,
            match_settings: MatchSettings::default(),
        })
        .expect("the rules table describes a map and a broadphase grid");
        // A near pair whose served route is long, and a near pair the router
        // refuses: a column six east of the long pair's start, walled in by stone
        // five high, settled.
        let (a, b) = long_near_pair(&world);
        let (ax, ay) = world.surface().coord_of(a);
        let (cx, cy) = (ax + 6, ay);
        wall_in(&mut world, cx, cy);
        let surface = world.surface().clone();
        let clusters = world.clusters().clone();
        let speed = Speed::commander(world.rules());
        let (c, d) = (a, surface.node_of(cx, cy).expect("the walled column"));
        assert!(
            clusters.touch(c, d) == Ok(true) && !clusters.connected(&surface, c, d),
            "the walled column is a near pair no walker reaches"
        );
        let scratch = world.scratch_mut();
        let before = allocations();
        let first = estimate(&surface, &clusters, scratch, a, b, Fog::Clear, speed);
        let after_first = allocations() - before;
        assert!(
            matches!(first, Ok(Some(_))),
            "the long near pair has an estimate"
        );
        assert_eq!(
            after_first, 0,
            "a near estimate on a fresh scratch allocated {after_first} times: the served route's \
         buffer is not reserved for MAX_ROUTE_NODES where the scratch is made"
        );
        let before = allocations();
        let none = estimate(&surface, &clusters, scratch, c, d, Fog::Clear, speed);
        let again = estimate(&surface, &clusters, scratch, a, b, Fog::Clear, speed);
        let after_refusal = allocations() - before;
        assert_eq!(none, Ok(None), "the router refuses the pair");
        assert_eq!(again, first, "the long pair answers as before");
        assert_eq!(
            after_refusal, 0,
            "a refused near estimate and the next one allocated {after_refusal} times: the served \
         route's buffer was not handed back on the refusal's path"
        );
    }

    /// A near pair on `world` whose served route is whole and longer than 16
    /// columns.
    fn long_near_pair(world: &World) -> (u32, u32) {
        use pharmakos_sim::pathing::route::route;
        let surface = world.surface();
        let clusters = world.clusters();
        let [sx, sy, _] = surface.size();
        let mut buffer = Vec::with_capacity(4096);
        let mut scratch =
            pharmakos_sim::pathing::search::Scratch::for_map(surface, clusters.size())
                .expect("a scratch");
        for y in (8..sy - 8).step_by(11) {
            for x in (8..sx - 8).step_by(13) {
                let Some(a) = surface.node_of(x, y) else {
                    continue;
                };
                for (dx, dy) in [(28, 3), (3, 28), (20, 20)] {
                    let Some(b) = surface.node_of(x + dx, y + dy) else {
                        continue;
                    };
                    if clusters.touch(a, b) == Ok(true)
                        && clusters.connected(surface, a, b)
                        && route(surface, clusters, &mut scratch, a, b, &mut buffer)
                            .is_ok_and(|served| !served.partial())
                        && buffer.len() > 16
                    {
                        return (a, b);
                    }
                }
            }
        }
        panic!("no long near pair on the determinism map");
    }

    /// Walls the column `(cx, cy)` in with stone five high on its eight
    /// neighbours, and steps `world` once so the graph settles over it.
    fn wall_in(world: &mut World, cx: i32, cy: i32) {
        for dy in -1..=1_i32 {
            for dx in -1..=1_i32 {
                if (dx, dy) == (0, 0) {
                    continue;
                }
                let top = world
                    .voxels()
                    .top_solid_z(cx + dx, cy + dy)
                    .expect("a column");
                for z in top + 1..=top + 5 {
                    world
                        .request_voxel_edit(pharmakos_sim::voxels::VoxelEdit::Set {
                            at: [cx + dx, cy + dy, z],
                            material: pharmakos_sim::voxels::Material::STONE,
                            by: None,
                        })
                        .expect("the edit is filed");
                }
            }
        }
        world
            .step(&mut pharmakos_sim::encoding::Enc::with_capacity(64 * 1024))
            .expect("the wall settles");
    }

    /// A `toward` resolve allocates nothing at its walled worst case (lane
    /// `p3fix`, fix pass 2): the probation seed's second vent ringed by stone
    /// three high at radius 26, its gap facing away from seat 0's core, where
    /// the octile bound once priced every site. Seat 0 holds, then steps
    /// `toward` the vent; the ticks round the step's start are counted, after a
    /// warm-up that pays for everything else a tick grows. The grid optimum's
    /// buffers are the scratch's, each one entry per column, and the site list is
    /// the world's, reserved for every column.
    #[test]
    fn a_toward_tick_behind_a_ring_allocates_nothing() {
        let rules = RulesTable::load(
            &std::path::Path::new("..")
                .join("..")
                .join("rules")
                .join("rules.v1.json"),
        )
        .expect("the committed rules table loads");
        let mut world = World::new(&WorldConfig {
            match_seed: 20,
            seats: 2,
            rules: rules.clone(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![600_000],
                round_limit: DEFAULT_ROUND_LIMIT,
            },
        })
        .expect("the rules table describes a map");
        let vent = [232_i32, 96_i32];
        ring_round(&mut world, vent, 26);
        let mut enc = Enc::with_capacity(64 * 1024);
        for _ in 0..3 {
            world.step(&mut enc).expect("the ring settles");
        }
        let text = concat!(
            "{\"schema_version\":{\"major\":1},",
            "\"meta\":{\"title\":\"toward\",\"author_kind\":\"HUMAN\"},",
            "\"declarative\":{\"route\":[",
            "{\"label\":\"wait\",\"hold\":{\"ms\":2000}},",
            "{\"label\":\"stone\",\"place_beacon\":{\"at\":{\"toward\":{\"feature\":{\"feature_id\":\"vent_232_96\"}}}},",
            "\"timeout_ms\":120000,\"on_fail\":{\"action\":\"SKIP\"}}]},",
            "\"on_death\":{\"on_respawn\":\"CONTINUE\",\"max_deaths_before_fallback\":3},",
            "\"fallback\":{\"hold\":{\"at\":{\"safest\":{}}}},",
            "\"kind\":\"PLAYBOOK\"}"
        );
        let playbook: pharmakos_proto::gp::v1::Playbook =
            pharmakos_proto::json::decode(text).expect("the playbook decodes");
        let plan =
            pharmakos_sim::interpreter::Plan::compile(&playbook, &rules).expect("it compiles");
        let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
        runner
            .seal_playbook(pharmakos_sim::tables::SeatId::new(0), plan)
            .expect("a Lull takes a seal");
        assert!(runner.begin_push().unwrap(), "a match opens in a Lull");
        // The hold is 40 ticks: warm up for 30 of them.
        for _ in 0..30 {
            runner
                .step()
                .unwrap()
                .expect("the segment outlasts this test");
            runner.clear_events();
            runner.world().encode(&mut enc);
        }
        let commander = runner
            .world()
            .commander_of(pharmakos_sim::tables::SeatId::new(0))
            .expect("a commander");
        let unit = runner
            .world()
            .units()
            .row_of(commander)
            .expect("a live commander")
            .raw();
        assert!(
            runner.world().router().route(unit).len() < 2,
            "the commander holds through the warm-up"
        );
        let before = allocations();
        for _ in 0..30 {
            runner
                .step()
                .unwrap()
                .expect("the segment outlasts this test");
            runner.clear_events();
            runner.world().encode(&mut enc);
        }
        let during = allocations() - before;
        assert!(
            runner.world().router().route(unit).len() > 1,
            "the toward step resolved inside the counted ticks: the commander walks to its stone"
        );
        assert_eq!(
            during, 0,
            "the ticks round a toward resolve behind a ring allocated {during} times"
        );
    }

    /// Stone three high in a ring of `radius` round `vent`, its gap on the side
    /// away from seat 0's core (the cells within about 5.7 degrees of that
    /// direction, compared squared in integers), filed but not yet settled.
    fn ring_round(world: &mut World, vent: [i32; 2], radius: i32) {
        let core_row = world
            .beacons()
            .seats()
            .iter()
            .position(|owner| *owner == 0)
            .expect("seat 0's core");
        let core = pharmakos_sim::targeting::column_of(
            *world
                .beacons()
                .positions()
                .get(core_row)
                .expect("the core's row"),
        );
        let away = [vent[0] - core[0], vent[1] - core[1]];
        let norm = i128::from(away[0]).pow(2) + i128::from(away[1]).pow(2);
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let d2 = dx * dx + dy * dy;
                if d2 < (radius - 1) * (radius - 1) || d2 > radius * radius {
                    continue;
                }
                let dot =
                    i128::from(dx) * i128::from(away[0]) + i128::from(dy) * i128::from(away[1]);
                if dot > 0 && dot * dot * 1_000_000 > 990_025 * i128::from(d2) * norm {
                    continue;
                }
                let (x, y) = (vent[0] + dx, vent[1] + dy);
                let Some(top) = world.voxels().top_solid_z(x, y) else {
                    continue;
                };
                for z in top + 1..=top + 3 {
                    file_or_flush(world, [x, y, z], pharmakos_sim::voxels::Material::STONE);
                }
            }
        }
    }
}

/// One token of Rust source, as [`tokens`] reads it: comments are dropped,
/// and a literal is one token, so a word inside a comment or a string is
/// never read as code.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Token {
    /// An identifier or keyword (a raw identifier without its `r#`).
    Word(String),
    /// A string literal's text between its quotes, raw or not, unescaped
    /// only as far as the scan needs: `\"` stays two bytes.
    Str(String),
    /// A macro metavariable, `$name`.
    Meta(String),
    /// Punctuation: `==`, `!=`, `<=`, `>=`, `=>`, `->` and `::` as one token
    /// each, so a lone `=` is an assignment or an attribute's `=`.
    Punct(String),
    /// A number, a character or byte literal, or a lifetime.
    Other,
}

/// Rust source as [`Token`]s. Line comments, nested block comments and doc
/// comments are dropped; string, raw string, byte string, character and
/// byte literals are read whole, so their contents are never tokens.
fn tokens(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let at = |index: usize| chars.get(index).copied();
    let mut out = Vec::new();
    let mut i = 0_usize;
    while let Some(c) = at(i) {
        let next = at(i + 1);
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && next == Some('/') {
            while at(i).is_some_and(|c| c != '\n') {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            i = past_block_comment(&chars, i);
        } else if let Some((text, end)) = string_at(&chars, i) {
            out.push(Token::Str(text));
            i = end;
        } else if c == '\'' {
            out.push(Token::Other);
            i = past_quote(&chars, i);
        } else if c == '$' || c.is_alphabetic() || c == '_' {
            let meta = c == '$';
            if meta || (c == 'r' && next == Some('#')) {
                i += if meta { 1 } else { 2 };
            }
            let start = i;
            while at(i).is_some_and(|c| c.is_alphanumeric() || c == '_') {
                i += 1;
            }
            let word = collect(&chars, start, i);
            out.push(if meta {
                Token::Meta(word)
            } else {
                Token::Word(word)
            });
        } else if c.is_ascii_digit() {
            while at(i).is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                i += 1;
            }
            out.push(Token::Other);
        } else {
            let pair: String = [Some(c), next].into_iter().flatten().collect();
            if ["==", "!=", "<=", ">=", "=>", "->", "::"].contains(&pair.as_str()) {
                out.push(Token::Punct(pair));
                i += 2;
            } else {
                out.push(Token::Punct(c.to_string()));
                i += 1;
            }
        }
    }
    out
}

/// `chars[from..to]` as a string, empty where the range is not inside it.
fn collect(chars: &[char], from: usize, to: usize) -> String {
    chars.get(from..to).unwrap_or(&[]).iter().collect()
}

/// The index just past the (nested) block comment that opens at `i`.
fn past_block_comment(chars: &[char], mut i: usize) -> usize {
    let mut depth = 0_usize;
    loop {
        match (chars.get(i), chars.get(i + 1)) {
            (Some('/'), Some('*')) => {
                depth += 1;
                i += 2;
            }
            (Some('*'), Some('/')) => {
                depth = depth.saturating_sub(1);
                i += 2;
                if depth == 0 {
                    return i;
                }
            }
            (Some(_), _) => i += 1,
            (None, _) => return i,
        }
    }
}

/// The index just past the character literal (`'x'`, `'\n'`, `'\u{1F600}'`)
/// or the lifetime that starts at `i`.
fn past_quote(chars: &[char], mut i: usize) -> usize {
    if chars.get(i + 1) == Some(&'\\') {
        i += 2;
        while chars.get(i).is_some_and(|c| *c != '\'') {
            i += 1;
        }
        i + 1
    } else if chars.get(i + 2) == Some(&'\'') {
        i + 3
    } else {
        i += 1;
        while chars
            .get(i)
            .is_some_and(|c| c.is_alphanumeric() || *c == '_')
        {
            i += 1;
        }
        i
    }
}

/// The string literal that starts at `i`, plain, byte, C or raw: its text
/// between the quotes and the index just past it; `None` when no string
/// starts there.
fn string_at(chars: &[char], i: usize) -> Option<(String, usize)> {
    let c = *chars.get(i)?;
    let prefixed = c == 'b' || c == 'c';
    let quote = if prefixed { i + 1 } else { i };
    if chars.get(quote) == Some(&'"') && (c == '"' || prefixed) {
        let start = quote + 1;
        let mut end = start;
        while let Some(c) = chars.get(end) {
            match c {
                '\\' => end += 2,
                '"' => break,
                _ => end += 1,
            }
        }
        return Some((collect(chars, start, end), end + 1));
    }
    let r = if prefixed { i + 1 } else { i };
    if chars.get(r) != Some(&'r') || !(c == 'r' || prefixed) {
        return None;
    }
    let hashes = chars
        .get(r + 1..)?
        .iter()
        .take_while(|c| **c == '#')
        .count();
    if chars.get(r + 1 + hashes) != Some(&'"') {
        return None;
    }
    let start = r + 2 + hashes;
    let mut end = start;
    while end < chars.len() {
        let closes = chars.get(end) == Some(&'"')
            && chars
                .get(end + 1..end + 1 + hashes)
                .is_some_and(|tail| tail.iter().all(|c| *c == '#'));
        if closes {
            return Some((collect(chars, start, end), end + 1 + hashes));
        }
        end += 1;
    }
    Some((collect(chars, start, end), end))
}

/// Every `.rs` file under `dir`, sorted, as a path from the crate root with
/// `/` separators.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml's own reason string sanctions read_dir when the listing is collected and sorted before use, which is what happens below"
)]
fn rust_sources(dir: &std::path::Path, out: &mut Vec<String>) {
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .expect("the sim's source folder reads")
        .map(|entry| entry.expect("a folder entry reads").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path.display().to_string().replace('\\', "/"));
        }
    }
}

fn word(text: &str) -> Token {
    Token::Word(text.to_owned())
}

fn punct(text: &str) -> Token {
    Token::Punct(text.to_owned())
}

/// The research feature's whole reach, read from the sim's own sources and
/// manifest, in **both** builds: this test is outside the default build's
/// `tick` module, so a manifest that turned the feature on by default, or a
/// crate that unified it on, still runs it, and it is red there.
///
/// It holds that the research build's tick is the default build's, function
/// for function, which is the one premise that lets the `tick` module run in
/// the default build only:
///
/// * the only `cfg` predicate on a feature anywhere under `src/` (a `cfg`,
///   `cfg!` or `cfg_attr`, split over lines or not, a raw string or a macro's
///   `$` parameter in the feature's place) is lib.rs's
///   `#[cfg(feature = "research")] pub mod research;`;
/// * nothing under `src/` reaches a file outside it: the one `#[path]` is
///   `world.rs`'s `"support.rs"`, beside it, and nothing calls `include!`;
/// * the manifest declares exactly `default = []` and `research = []`, names
///   no build script (and there is no `build.rs`), and holds no
///   `cfg(...)`-keyed target table;
/// * `research.rs` is what it is today: two `use` lines and two free
///   functions, `fork` (`#[must_use]`) and `fork_and_step`, with no `impl`
///   (an `impl Drop` or an inherent `impl World` there would change the
///   tick's code with no `cfg` outside the module), no `static`, no
///   `unsafe`, no macro and no attribute but `#[must_use]`.
///
/// The scan drops comments and reads literals whole, so a mention in a
/// comment, a doc or a string is not a gate.
#[test]
fn the_research_feature_gates_the_fork_module_and_nothing_else() {
    let (gates, reaches) = gates_and_reaches();
    let lib_gate: Vec<Token> = vec![
        punct("#"),
        punct("["),
        word("cfg"),
        punct("("),
        word("feature"),
        punct("="),
        Token::Str("research".to_owned()),
        punct(")"),
        punct("]"),
        word("pub"),
        word("mod"),
        word("research"),
    ];
    assert_eq!(
        gates,
        [("src/lib.rs".to_owned(), lib_gate)],
        "a cfg predicate on a feature other than lib.rs's `#[cfg(feature = \"research\")] pub \
         mod research`: the research build's tick is no longer the default build's, so the \
         tick tests must run under the feature as well"
    );
    assert_eq!(
        reaches,
        [(
            "src/world.rs".to_owned(),
            format!("#[path = {:?}]", Some(Token::Str("support.rs".to_owned())))
        )],
        "the sim's sources reach a file this scan does not read"
    );
    assert!(
        std::path::Path::new("src/support.rs").is_file(),
        "world.rs's #[path] names src/support.rs, which this scan reads"
    );

    let research =
        tokens(&std::fs::read_to_string("src/research.rs").expect("the research module reads"));
    assert_eq!(
        item_heads(&research),
        [
            "use crate :: encoding :: Enc ;",
            "use crate :: world :: World ;",
            "# [ must_use ] pub fn fork (",
            "pub fn fork_and_step (",
        ],
        "research.rs holds an item beside its two uses and two functions"
    );
    for refused in [
        "impl",
        "static",
        "unsafe",
        "macro_rules",
        "mod",
        "extern",
        "trait",
    ] {
        assert!(
            !research.contains(&word(refused)),
            "research.rs holds `{refused}`: code there can change the tick's with no cfg \
             outside the module"
        );
    }
    let attributes: Vec<String> = research
        .windows(3)
        .filter(|window| window.first() == Some(&punct("#")))
        .map(spelled)
        .collect();
    assert_eq!(
        attributes,
        ["# [ must_use"],
        "research.rs carries an attribute but fork's #[must_use]"
    );
    assert!(!research.contains(&punct("!")), "research.rs calls a macro");

    assert!(
        !std::path::Path::new("build.rs").exists(),
        "the sim has a build script, which can set a cfg from the feature"
    );
    assert_eq!(
        manifest_features(),
        ["default = []", "research = []"],
        "the manifest's features are no longer `default = []` and a `research` that enables \
         nothing"
    );
}

/// A `cfg` predicate on a feature: the file, and the tokens round it.
type Gate = (String, Vec<Token>);

/// A reach outside a file: the file, and the `include!` or `#[path]` spelled.
type Reach = (String, String);

/// Every `cfg` predicate on a feature under `src/` (a `feature =` after a
/// `(` or a `,`), with the tokens round it, and every reach outside the file
/// it is in (`include!`, and an attribute's `path =` with its value).
fn gates_and_reaches() -> (Vec<Gate>, Vec<Reach>) {
    let mut files = Vec::new();
    rust_sources(std::path::Path::new("src"), &mut files);
    assert!(files.len() > 10, "the sim's sources were found: {files:?}");
    let mut gates = Vec::new();
    let mut reaches = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("a source file reads");
        let code = tokens(&text);
        for (at, window) in code.windows(3).enumerate() {
            let [before, name, equals] = window else {
                continue;
            };
            if *name == word("feature")
                && *equals == punct("=")
                && (*before == punct("(") || *before == punct(","))
            {
                let around: Vec<Token> = code
                    .iter()
                    .skip(at.saturating_sub(3))
                    .take(12)
                    .cloned()
                    .collect();
                gates.push((file.clone(), around));
            }
        }
        for (at, window) in code.windows(2).enumerate() {
            let after_bracket = at
                .checked_sub(1)
                .and_then(|before| code.get(before))
                .is_some_and(|before| *before == punct("[") || *before == punct(","));
            match window {
                [Token::Word(name), bang] if name == "include" && *bang == punct("!") => {
                    reaches.push((file.clone(), "include!".to_owned()));
                }
                [Token::Word(name), equals]
                    if name == "path" && *equals == punct("=") && after_bracket =>
                {
                    let value = code.get(at + 2).cloned();
                    reaches.push((file.clone(), format!("#[path = {value:?}]")));
                }
                _ => {}
            }
        }
    }
    (gates, reaches)
}

/// Tokens spelled out, a space between each.
fn spelled(tokens: &[Token]) -> String {
    tokens
        .iter()
        .map(|token| match token {
            Token::Word(w) | Token::Punct(w) => w.clone(),
            Token::Meta(m) => format!("${m}"),
            Token::Str(s) => format!("{s:?}"),
            Token::Other => "_".to_owned(),
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// Each top-level item's head: its tokens up to its first `(` or `{`, or
/// through its `;`, [`spelled`].
fn item_heads(code: &[Token]) -> Vec<String> {
    let mut heads = Vec::new();
    let mut head = Vec::new();
    let mut depth = 0_usize;
    for token in code {
        if depth == 0 && head.last() != Some(&punct("(")) && head.last() != Some(&punct("{")) {
            head.push(token.clone());
        }
        match token {
            Token::Punct(p) if p == "{" || p == "(" => depth += 1,
            Token::Punct(p) if p == "}" || p == ")" => {
                depth = depth.saturating_sub(1);
                if depth == 0 && p == "}" {
                    heads.push(spelled(&core::mem::take(&mut head)));
                }
            }
            Token::Punct(p) if p == ";" && depth == 0 => {
                heads.push(spelled(&core::mem::take(&mut head)));
            }
            _ => {}
        }
    }
    heads
}

/// The sim manifest's `[features]` lines, after checking that no table is
/// keyed by a `cfg(...)` and that `[package]` names no build script.
fn manifest_features() -> Vec<String> {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("the sim's manifest reads");
    let mut section = String::new();
    let mut features = Vec::new();
    for line in manifest
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        if line.starts_with('[') {
            assert!(
                !line.contains("cfg("),
                "the manifest holds a cfg-keyed table: {line}"
            );
            line.clone_into(&mut section);
            continue;
        }
        if section == "[package]" {
            assert!(
                !line.starts_with("build"),
                "the manifest names a build script: {line}"
            );
        }
        if section == "[features]" {
            features.push(line.to_owned());
        }
    }
    features
}

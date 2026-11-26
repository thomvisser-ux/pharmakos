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
//!
//! **What the research build adds, `fork`, is counted in the research build
//! alone**, in the `fork` module: a forked world's tick allocates exactly the
//! chunks it un-shares with its parent (copy-on-write, counted by the chunks'
//! addresses), its parent's tick likewise while the fork lives, and nothing
//! once the fork is dropped. The match, the warm-up and the edits both builds'
//! tests file are the shared helpers below.

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

/// **The counter is itself tested**, in both builds: every assertion in this
/// file reads [`allocations`], so a hook that stopped counting would turn them
/// all green at once. A `realloc` matters most: a `Vec` grown past its room
/// while it holds elements is a `realloc`, not an `alloc`, so a counter blind
/// to it is blind to exactly the bug this file exists for. One call of each
/// hook, through the global allocator, on this thread: `alloc`,
/// `alloc_zeroed` and a growing and a shrinking `realloc` each count one, a
/// `dealloc` counts none; and a `Vec` pushed past its capacity is seen.
#[test]
fn the_counting_allocator_counts_every_hook_it_says_it_counts() {
    use std::alloc::Layout;
    let small = Layout::from_size_align(64, 8).expect("a layout");
    let large = Layout::from_size_align(4096, 8).expect("a layout");
    let mut figures = [0_u64; 5];
    #[allow(
        unsafe_code,
        reason = "the hooks are reached only through the global allocator's unsafe entry points; each pointer is checked non-null before it is used, used with the layout it was made with, and freed once"
    )]
    unsafe {
        let before = allocations();
        let block = std::alloc::alloc(small);
        figures[0] = allocations() - before;
        assert!(!block.is_null(), "the system allocator answered");
        let before = allocations();
        let zeroed = std::alloc::alloc_zeroed(small);
        figures[1] = allocations() - before;
        assert!(!zeroed.is_null(), "the system allocator answered");
        let before = allocations();
        let block = std::alloc::realloc(block, small, large.size());
        figures[2] = allocations() - before;
        assert!(!block.is_null(), "the system allocator answered");
        let before = allocations();
        let block = std::alloc::realloc(block, large, small.size());
        figures[3] = allocations() - before;
        assert!(!block.is_null(), "the system allocator answered");
        let before = allocations();
        std::alloc::dealloc(block, small);
        std::alloc::dealloc(zeroed, small);
        figures[4] = allocations() - before;
    }
    assert_eq!(
        figures,
        [1, 1, 1, 1, 0],
        "alloc, alloc_zeroed, a growing realloc and a shrinking realloc each count one; dealloc none"
    );

    let mut vector: Vec<u64> = Vec::with_capacity(4);
    vector.extend([1, 2, 3, 4]);
    let before = allocations();
    vector.push(std::hint::black_box(5));
    let grown = allocations() - before;
    assert_eq!(
        grown, 1,
        "a Vec pushed past its capacity while it holds elements grows by a realloc, and the \
         counter sees it"
    );
    assert_eq!(std::hint::black_box(vector).len(), 5);
}

/// How many boxes [`the_counter_counts_each_thread_alone`]'s allocating
/// thread makes while the other thread watches.
const OTHER_THREADS_BOXES: u64 = 16;

/// **The counter counts each thread alone**, in both builds: a test that
/// steps two worlds on two threads side by side
/// (`tick::a_restored_worlds_tick_allocates_nothing`) holds one of them to
/// zero allocations while the other allocates as it likes, which is exact
/// only if an allocation is charged to the thread that made it and to no
/// other.
///
/// Two scoped threads, held in step by atomics alone: the watching thread
/// reads its count, raises `started` and spins until `finished`; the
/// allocating thread spins until `started`, makes [`OTHER_THREADS_BOXES`]
/// boxes, counting each in `boxes_done`, and raises `finished`; the watching
/// thread reads its count again. The watcher also reads `boxes_done` when its
/// span opens (none yet) and when it closes (all of them), so a span that did
/// not really cover the boxes fails here rather than passing vacuously.
///
/// **No scaffolding allocates inside a span, on any OS.** The flags are
/// atomics waited on by spinning, never a `Barrier`, `Mutex` or `Condvar`:
/// on macOS std's lock and condition variable allocate their OS object on
/// first use, which charged the watcher two allocations in CI. The counter's
/// thread-local is `const`-initialised with no destructor, so touching it
/// never allocates. The spawns and joins run on the test's thread, outside
/// both spans.
///
/// Orderings: each flag is stored `Release` and loaded `Acquire`, so the
/// allocator's boxes happen-before the watcher's second read; `boxes_done`
/// is `Relaxed`, its last value ordered by `finished`'s store and load.
#[test]
fn the_counter_counts_each_thread_alone() {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    let started = AtomicBool::new(false);
    let finished = AtomicBool::new(false);
    let boxes_done = AtomicU64::new(0);
    let wait = |flag: &AtomicBool| {
        while !flag.load(Ordering::Acquire) {
            std::hint::spin_loop();
        }
    };
    let (made, watched) = std::thread::scope(|scope| {
        let allocating = scope.spawn(|| {
            wait(&started);
            let before = allocations();
            for index in 0..OTHER_THREADS_BOXES {
                drop(std::hint::black_box(Box::new(std::hint::black_box(index))));
                boxes_done.fetch_add(1, Ordering::Relaxed);
            }
            let made = allocations() - before;
            finished.store(true, Ordering::Release);
            made
        });
        let watching = scope.spawn(|| {
            let before = allocations();
            let at_open = boxes_done.load(Ordering::Relaxed);
            started.store(true, Ordering::Release);
            wait(&finished);
            let at_close = boxes_done.load(Ordering::Relaxed);
            (allocations() - before, at_open, at_close)
        });
        (
            allocating
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
            watching
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
        )
    });
    let (watched, at_open, at_close) = watched;
    assert_eq!(
        (at_open, at_close),
        (0, OTHER_THREADS_BOXES),
        "the watcher's span opened before the first box and closed after the last"
    );
    assert_eq!(
        (made, watched),
        (OTHER_THREADS_BOXES, 0),
        "the allocating thread is charged its own boxes and the watching thread, which made \
         none in the same span, is charged nothing"
    );
}

/// The ticks [`warm_up`] runs: every buffer a tick fills is at its steady
/// capacity after them.
const WARM_UP_TICKS: u32 = 50;

/// The ticks counted for table growth, after the warm-up: long enough that
/// every placed seat's deploy, Generator and fabricated drone land inside them
/// (the last Generator is finished at tick 1 300 at the committed table).
const GROWTH_TICKS: u32 = 1_400;

/// The determinism harness's match, in its first Push, every seat sealed: the
/// world both builds' tick tests count. Built outside every counted window.
fn harness_runner() -> Runner {
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
    runner
}

/// Warm `runner` up for [`WARM_UP_TICKS`] ticks, cratering and collapsing on
/// each, outside every counted window.
fn warm_up(runner: &mut Runner, enc: &mut Enc) {
    // Warm up: the first tick may still grow the encoder's buffer, and a
    // capacity that is right for tick 1 is right for tick 10 000 because every
    // table's count is fixed at construction.
    // The warm-up craters too, so the voxel phase's own scratch — the edit
    // queue, the store's pending and settled lists, the crater's touched list —
    // is paid for out here and the measured loop below sees it at its steady
    // capacity.
    for step in 0..WARM_UP_TICKS {
        crater_at(runner.world_mut(), step);
        collapse_at(runner.world_mut(), step);
        runner
            .step()
            .unwrap()
            .expect("the segment outlasts this test");
        runner.clear_events();
        runner.world().encode(enc);
    }
}

/// The harness match warmed up ([`warm_up`]), and a second runner restored
/// from its save at that tick, through the file's bytes: the shipped resume
/// path (`Host::resume` restores a save into a fresh runner). The restored
/// world hashes as the original. Built outside every counted window.
fn warmed_and_restored(enc: &mut Enc) -> (Runner, Runner) {
    let mut straight = harness_runner();
    warm_up(&mut straight, enc);
    let bytes = pharmakos_sim::snapshot::Snapshot::capture(straight.world())
        .expect("a world between ticks")
        .to_bytes()
        .expect("the save encodes");
    let snapshot = pharmakos_sim::snapshot::Snapshot::from_bytes(&bytes).expect("the save decodes");
    let mut restored = harness_runner();
    restored.restore(&snapshot).expect("the save restores");
    assert_eq!(
        restored.world().state_hash(),
        straight.world().state_hash(),
        "a restored world hashes as the world it was saved from"
    );
    (straight, restored)
}

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
    let x = (i32::try_from(step.wrapping_mul(41) % 4096).expect("a column index") + 101) % width;
    let y = (i32::try_from(step.wrapping_mul(59) % 4096).expect("a column index") + 211) % depth;
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

/// The `research` build's test: a forked world's tick, and its parent's while
/// the fork lives. Behind `feature = "research"` because `fork` exists only
/// there; the default build's tick is counted by [`tick`]'s tests.
#[cfg(feature = "research")]
mod fork {
    use super::{
        CROWD_TICKS, GROWTH_TICKS, WARM_UP_TICKS, allocations, collapse_at, crater_at, crowd,
        harness_runner, warm_up,
    };

    use pharmakos_sim::World;
    use pharmakos_sim::encoding::Enc;
    use pharmakos_sim::runner::Runner;

    /// The ticks each counted window runs: the child's, the parent's while
    /// the child lives, and the parent's after the child is dropped.
    const FORK_TICKS: u32 = 50;

    /// The ticks the parent steps the crowds before the combat test forks it:
    /// at the committed rules and harness the two forces first fire between
    /// 1 300 and 1 400 ticks after they are laid out at the end of the
    /// warm-up (measured), so a fork here holds every shot in its counted
    /// window; the test asserts both halves, so a tuning change that moves the
    /// first shot fails it rather than leaving the combat phase uncounted.
    const COMBAT_LEAD: u32 = 1_250;

    /// Each of `world`'s chunks' bytes, by address, into `into`: what the
    /// counting compares to find the chunks a tick re-pointed. `into` is
    /// reserved for every chunk before any window, so this allocates nothing.
    fn chunk_addresses(world: &World, into: &mut Vec<*const u8>) {
        into.clear();
        for chunk in 0..world.voxels().chunk_count() {
            let bytes = world
                .voxels()
                .chunk_bytes(chunk)
                .expect("every chunk below the count has bytes");
            into.push(bytes.as_ptr());
        }
    }

    /// How many of `world`'s chunks no longer sit at the address `before`
    /// recorded: the chunks re-pointed since. A chunk shared with another
    /// world is re-pointed by its first write (copy-on-write,
    /// `Arc::make_mut`), and that copy is one allocation; a chunk this world
    /// alone holds is written in place.
    fn repointed(world: &World, before: &[*const u8]) -> u64 {
        let mut moved = 0_u64;
        for (chunk, was) in (0..world.voxels().chunk_count()).zip(before) {
            let now = world
                .voxels()
                .chunk_bytes(chunk)
                .expect("every chunk below the count has bytes")
                .as_ptr();
            if !core::ptr::eq(now, *was) {
                moved += 1;
            }
        }
        moved
    }

    /// One window's figures: what the thread allocated, and the chunks the
    /// stepped world re-pointed, counted tick by tick. Each tick is held to
    /// the rule on its own as well ([`tick_by_tick`]), so a tick that
    /// allocated one more than it re-pointed and another one fewer cannot
    /// cancel into a window that looks right.
    struct Window {
        allocated: u64,
        repointed: u64,
    }

    /// One counted tick held to the rule: the allocations of tick `step` of
    /// a window, from the edits filed before it to its encoding, equal the
    /// chunks it re-pointed. A failure stops the window at that tick; a pass
    /// allocates nothing.
    fn tick_by_tick(step: u32, allocated: u64, repointed: u64) {
        assert_eq!(
            allocated, repointed,
            "tick {step} of the window allocated {allocated} times and re-pointed {repointed} chunks"
        );
    }

    /// Step the child `child` for `ticks` ticks, cratering and
    /// collapsing on each from `first`, through the host's door
    /// (`request_voxel_edit`, between ticks, so the edit queue is counted
    /// too).
    fn count_child(
        child: &mut World,
        enc: &mut Enc,
        first: u32,
        ticks: u32,
        addresses: &mut Vec<*const u8>,
    ) -> Window {
        let mut repointed_total = 0_u64;
        let before = allocations();
        for step in 0..ticks {
            chunk_addresses(child, addresses);
            let tick_before = allocations();
            crater_at(child, step + first);
            collapse_at(child, step + first);
            child.step(enc).expect("the forked world steps");
            child.clear_events();
            child.encode(enc);
            let moved = repointed(child, addresses);
            tick_by_tick(step, allocations() - tick_before, moved);
            repointed_total += moved;
        }
        Window {
            allocated: allocations() - before,
            repointed: repointed_total,
        }
    }

    /// As [`count_child`], for the parent, driven through its `Runner`.
    fn count_parent(
        runner: &mut Runner,
        enc: &mut Enc,
        first: u32,
        addresses: &mut Vec<*const u8>,
    ) -> Window {
        let mut repointed_total = 0_u64;
        let before = allocations();
        for step in 0..FORK_TICKS {
            chunk_addresses(runner.world(), addresses);
            let tick_before = allocations();
            crater_at(runner.world_mut(), step + first);
            collapse_at(runner.world_mut(), step + first);
            runner
                .step()
                .expect("the parent steps")
                .expect("the segment outlasts this test");
            runner.clear_events();
            runner.world().encode(enc);
            let moved = repointed(runner.world(), addresses);
            tick_by_tick(step, allocations() - tick_before, moved);
            repointed_total += moved;
        }
        Window {
            allocated: allocations() - before,
            repointed: repointed_total,
        }
    }

    /// What the `research` build adds to the tick, it adds through `fork`
    /// alone, and a fork costs exactly the chunks it un-shares.
    ///
    /// A fork is a `World::clone`. It shares every chunk with its parent
    /// (copy-on-write, by design), so the first write to a shared chunk, in
    /// either world, copies it: one allocation, which this test counts by the
    /// chunk's address changing, never by a counter in the code under test.
    /// Everything else a clone must keep as the original built it: a clone that
    /// copied a buffer's length and not its reserved room (as a derived `Clone`
    /// of a `Vec` does) would allocate on the first tick that filled it, in
    /// the clone rather than in the original, which is exactly where nobody
    /// looks.
    ///
    /// The parent is warmed as the default build's tick test's match is, so
    /// every buffer is at its steady capacity when it is forked. Then: the
    /// child's first [`FORK_TICKS`] ticks, the parent's next ones while the
    /// child lives (on columns the child did not crater, so it meets chunks
    /// the two still share), and the parent's after the child is dropped,
    /// when it holds every chunk alone and must allocate nothing, as the
    /// default build's tick does. Every counted tick craters and collapses,
    /// through the host's `request_voxel_edit` between ticks.
    #[test]
    fn a_forked_worlds_tick_allocates_only_the_chunks_it_re_points() {
        let mut runner = harness_runner();
        let mut enc = Enc::with_capacity(64 * 1024);
        warm_up(&mut runner, &mut enc);
        let chunks = usize::try_from(runner.world().voxels().chunk_count())
            .expect("a chunk count fits a usize");
        let mut addresses: Vec<*const u8> = Vec::with_capacity(chunks);

        let mut child = pharmakos_sim::research::fork(runner.world());
        let regions_before = child.collapse_counts().regions;
        let forked = count_child(
            &mut child,
            &mut enc,
            WARM_UP_TICKS,
            FORK_TICKS,
            &mut addresses,
        );
        let fell = child.collapse_counts().regions - regions_before;
        assert!(
            fell >= u64::from(FORK_TICKS),
            "a region fell on every counted tick of the fork: {fell}"
        );
        assert!(
            forked.repointed > 0,
            "the child's craters met chunks it shares with its parent, so the count is not vacuous"
        );
        assert_eq!(
            forked.allocated, forked.repointed,
            "the first {FORK_TICKS} ticks of a forked world allocated {} times and re-pointed {} \
             chunks: a fork's clone must keep every buffer's reserved room, so the chunks it \
             un-shares are all it allocates",
            forked.allocated, forked.repointed
        );

        // The parent, on columns of its own, while the child still shares
        // every chunk neither has written since the fork.
        let parent = count_parent(
            &mut runner,
            &mut enc,
            WARM_UP_TICKS + 4 * FORK_TICKS,
            &mut addresses,
        );
        assert!(
            parent.repointed > 0,
            "the parent's craters met chunks it shares with the live child"
        );
        assert_eq!(
            parent.allocated, parent.repointed,
            "while a fork lives, its parent's {FORK_TICKS} ticks allocated {} times and \
             re-pointed {} chunks",
            parent.allocated, parent.repointed
        );

        drop(child);
        let alone = count_parent(
            &mut runner,
            &mut enc,
            WARM_UP_TICKS + 8 * FORK_TICKS,
            &mut addresses,
        );
        assert_eq!(
            (alone.allocated, alone.repointed),
            (0, 0),
            "once the fork is dropped the parent holds every chunk alone, and its tick \
             allocates nothing, as the default build's does"
        );
    }

    /// A fork keeps every table's reserved room, as it keeps every scratch
    /// buffer's: the child of a world forked before its growth window adds a
    /// beacon, a structure and a fabricated drone per seat (the rows the default
    /// build's tick test counts in that window) and allocates only the chunks it
    /// re-points. A table whose room a derived `Clone` dropped would allocate
    /// at its first row in the child.
    #[test]
    fn a_forked_worlds_table_growth_allocates_only_the_chunks_it_re_points() {
        let mut runner = harness_runner();
        let mut enc = Enc::with_capacity(64 * 1024);
        warm_up(&mut runner, &mut enc);
        let chunks = usize::try_from(runner.world().voxels().chunk_count())
            .expect("a chunk count fits a usize");
        let mut addresses: Vec<*const u8> = Vec::with_capacity(chunks);

        let mut child = pharmakos_sim::research::fork(runner.world());
        drop(runner);
        let beacons_before = child.beacons().len();
        let structures_before = child.structures().len();
        let units_before = child.units().len();
        let grown = count_child(
            &mut child,
            &mut enc,
            WARM_UP_TICKS,
            GROWTH_TICKS,
            &mut addresses,
        );
        assert!(
            child.beacons().len() > beacons_before
                && child.structures().len() > structures_before
                && child.units().len() > units_before,
            "the child's deploy, its Generator and the drone it asks for all landed inside the \
             counted window"
        );
        assert_eq!(
            grown.allocated, grown.repointed,
            "the {GROWTH_TICKS} ticks in which a forked world grows its tables allocated {} times \
             and re-pointed {} chunks",
            grown.allocated, grown.repointed
        );
    }

    /// A fork of a **restored** world keeps the room the restore gave back:
    /// the harness match is saved after its warm-up and restored into a
    /// second runner (the shipped resume path), the restored world is forked,
    /// and the child's growth window allocates only the chunks it re-points.
    #[test]
    fn a_fork_of_a_restored_world_allocates_only_the_chunks_it_re_points() {
        let mut enc = Enc::with_capacity(64 * 1024);
        let (straight, restored) = super::warmed_and_restored(&mut enc);
        drop(straight);
        let chunks = usize::try_from(restored.world().voxels().chunk_count())
            .expect("a chunk count fits a usize");
        let mut addresses: Vec<*const u8> = Vec::with_capacity(chunks);
        let mut child = pharmakos_sim::research::fork(restored.world());
        let beacons_before = child.beacons().len();
        let units_before = child.units().len();
        let grown = count_child(
            &mut child,
            &mut enc,
            WARM_UP_TICKS,
            GROWTH_TICKS,
            &mut addresses,
        );
        assert!(
            child.beacons().len() > beacons_before && child.units().len() > units_before,
            "the fork of the restored world added table rows inside the counted window"
        );
        assert!(
            grown.repointed > 0,
            "the child met chunks it shares with the restored world, so the count is not vacuous"
        );
        assert_eq!(
            grown.allocated, grown.repointed,
            "the {GROWTH_TICKS} ticks of a fork of a restored world allocated {} times and re-pointed {} chunks",
            grown.allocated, grown.repointed
        );
    }

    /// A fork keeps the combat phase's room too: the two attacking crowds of
    /// the default build's tick test are laid out in the parent after its
    /// warm-up and stepped [`COMBAT_LEAD`] ticks, when neither has fired yet,
    /// then forked; the child's [`CROWD_TICKS`] ticks hold the first shots
    /// and their damage, kill credit and deaths, every tick cratering and
    /// collapsing as well, and allocate only the chunks the child re-points.
    /// The parent stays alive, so the child meets shared chunks.
    #[test]
    fn a_forked_worlds_combat_allocates_only_the_chunks_it_re_points() {
        let mut runner = harness_runner();
        let mut enc = Enc::with_capacity(64 * 1024);
        warm_up(&mut runner, &mut enc);
        crowd(runner.world_mut());
        for _ in 0..COMBAT_LEAD {
            runner
                .step()
                .expect("the parent steps")
                .expect("the segment outlasts this test");
            runner.clear_events();
            runner.world().encode(&mut enc);
        }
        let chunks = usize::try_from(runner.world().voxels().chunk_count())
            .expect("a chunk count fits a usize");
        let mut addresses: Vec<*const u8> = Vec::with_capacity(chunks);

        let mut child = pharmakos_sim::research::fork(runner.world());
        let shots_before = child.combat_counts().shots();
        assert_eq!(shots_before, 0, "no crowd has fired before the fork");
        let fought = count_child(
            &mut child,
            &mut enc,
            WARM_UP_TICKS + 40 * FORK_TICKS,
            CROWD_TICKS,
            &mut addresses,
        );
        // The fight ends inside the window: seat 0's Attack beacon falls and
        // its row closes, so the shots, not the rows, say that it happened.
        assert!(
            child.combat_counts().shots() > shots_before,
            "the crowds fought inside the child's counted window"
        );
        assert_eq!(
            fought.allocated, fought.repointed,
            "the {CROWD_TICKS} ticks of a forked world's crowds allocated {} times and re-pointed \
             {} chunks",
            fought.allocated, fought.repointed
        );
    }
}

/// The default build's tests: the tick and the searches, each counted on its
/// own thread. Behind `not(feature = "research")` because the research build's
/// tick is this build's, function for function, which
/// [`the_research_feature_gates_the_fork_module_and_nothing_else`] checks in both
/// builds.
#[cfg(not(feature = "research"))]
mod tick {
    use super::{
        CROWD_TICKS, GROWTH_TICKS, WARM_UP_TICKS, allocations, collapse_at, crater_at, crowd,
        harness_runner, warm_up,
    };

    use pharmakos_sim::encoding::Enc;
    use pharmakos_sim::runner::{DEFAULT_ROUND_LIMIT, Runner};
    use pharmakos_sim::voxels::VoxelEdit;
    use pharmakos_sim::{MatchSettings, RulesTable, World, WorldConfig};

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
        let mut runner = harness_runner();
        let mut enc = Enc::with_capacity(64 * 1024);
        warm_up(&mut runner, &mut enc);
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
            crater_at(runner.world_mut(), step + WARM_UP_TICKS);
            collapse_at(runner.world_mut(), step + WARM_UP_TICKS);
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
            crater_at(runner.world_mut(), step + WARM_UP_TICKS + GROWTH_TICKS);
            // And a collapse every tick (S2's plan, `fall`): the support rule's
            // search, its fall and its labels' clearing inside the property too.
            collapse_at(runner.world_mut(), step + WARM_UP_TICKS + GROWTH_TICKS);
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

    /// One window of a runner's ticks: cratering and collapsing on each from
    /// `first` when `edit` holds, each tick's hash pushed to `chain`, which
    /// is reserved for every tick before any window, so this allocates
    /// nothing of its own.
    fn run_window(
        runner: &mut Runner,
        enc: &mut Enc,
        first: u32,
        ticks: u32,
        edit: bool,
        chain: &mut Vec<u64>,
    ) {
        for step in 0..ticks {
            if edit {
                crater_at(runner.world_mut(), step + first);
                collapse_at(runner.world_mut(), step + first);
            }
            let report = runner
                .step()
                .expect("the runner steps")
                .expect("the segment outlasts this test");
            chain.push(report.hash);
            runner.clear_events();
            runner.world().encode(enc);
        }
    }

    /// The ticks a restored world's crowds are stepped before their counted
    /// window, as [`crowd_allocates_nothing`] steps them.
    const RESTORED_CROWD_LEAD: u32 = 20;

    /// **A restored world's tick allocates nothing**, as a fresh one's does:
    /// the shipped resume path (`Host::resume`) restores a save into a fresh
    /// runner, and a table assigned from the file holds its rows and no room
    /// for more unless the restore gives its room back
    /// (`World::reserve_table_rooms`). The harness match is saved after its
    /// warm-up and restored into a second runner, which then runs the growth
    /// window (a deploy, a Generator and a drone per seat: rows in the beacon,
    /// structure, unit and target tables) and the attacking crowds' fight,
    /// both counted. Capacity is not state: the restored-then-run chain
    /// equals the run-straight-through chain over every tick of both windows.
    ///
    /// **The two runners step side by side**, each on a scoped thread of its
    /// own (lane `simroom`): once the restored runner is built from the
    /// straight one's save, neither reads the other, so only the comparison
    /// of their chains waits for both. The counter is per thread
    /// ([`super::the_counter_counts_each_thread_alone`] proves it), so the
    /// restored runner's thread counts its own ticks exactly, whatever the
    /// straight runner's thread allocates meanwhile. Each thread's panic is
    /// resumed on the test's thread, and each hands back its chain, so a
    /// thread whose result was dropped leaves a chain short and fails the
    /// comparison.
    #[test]
    fn a_restored_worlds_tick_allocates_nothing() {
        let mut enc = Enc::with_capacity(64 * 1024);
        let (straight, restored) = super::warmed_and_restored(&mut enc);
        let ticks = GROWTH_TICKS + RESTORED_CROWD_LEAD + CROWD_TICKS;
        let rows = usize::try_from(ticks).expect("a tick count fits a usize");
        let (chain, expected) = std::thread::scope(|scope| {
            let restored = scope.spawn(move || restored_windows(restored, enc, rows));
            let straight = scope.spawn(move || straight_windows(straight, rows));
            (
                restored
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
                straight
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
            )
        });
        assert_eq!(
            (chain.len(), expected.len()),
            (rows, rows),
            "each runner hands back a hash for every tick it stepped"
        );
        let parted = chain
            .iter()
            .zip(&expected)
            .position(|(restored, straight)| restored != straight);
        assert_eq!(
            parted, None,
            "the restored-then-run chain parts from the run-straight-through chain: capacity \
             is not state, so a restore that gives its room back moves no hash"
        );
    }

    /// The restored runner's half of [`a_restored_worlds_tick_allocates_nothing`],
    /// on its own thread: the growth window and the crowds' fight, each
    /// counted on this thread, and the chain of every tick it stepped.
    fn restored_windows(mut restored: Runner, mut enc: Enc, rows: usize) -> Vec<u64> {
        let mut chain: Vec<u64> = Vec::with_capacity(rows);
        let beacons_before = restored.world().beacons().len();
        let structures_before = restored.world().structures().len();
        let units_before = restored.world().units().len();
        let targets_before = restored.world().targets().len();
        let before = allocations();
        run_window(
            &mut restored,
            &mut enc,
            WARM_UP_TICKS,
            GROWTH_TICKS,
            true,
            &mut chain,
        );
        let growing = allocations() - before;
        assert!(
            restored.world().beacons().len() > beacons_before
                && restored.world().structures().len() > structures_before
                && restored.world().units().len() > units_before
                && restored.world().targets().len() > targets_before,
            "the restored world's deploys, Build targets, Generators and drones landed inside \
             the counted window"
        );
        assert_eq!(
            growing, 0,
            "the {GROWTH_TICKS} ticks in which a restored world grows its tables allocated \
             {growing} times: a restore gives every table its room again"
        );

        crowd(restored.world_mut());
        run_window(
            &mut restored,
            &mut enc,
            0,
            RESTORED_CROWD_LEAD,
            false,
            &mut chain,
        );
        let shots_before = restored.world().combat_counts().shots();
        let before = allocations();
        run_window(&mut restored, &mut enc, 0, CROWD_TICKS, false, &mut chain);
        let crowded = allocations() - before;
        assert!(
            restored.world().combat_counts().shots() > shots_before,
            "the restored world's crowds fought inside the counted window"
        );
        assert_eq!(
            crowded, 0,
            "{CROWD_TICKS} ticks of a restored world's crowds allocated {crowded} times"
        );
        chain
    }

    /// The straight runner's half of [`a_restored_worlds_tick_allocates_nothing`],
    /// on its own thread: the same windows, uncounted, and the chain the
    /// restored runner's must equal. Its encoder is its own; what it
    /// allocates is this thread's and reaches no count.
    fn straight_windows(mut straight: Runner, rows: usize) -> Vec<u64> {
        let mut enc = Enc::with_capacity(64 * 1024);
        let mut expected: Vec<u64> = Vec::with_capacity(rows);
        run_window(
            &mut straight,
            &mut enc,
            WARM_UP_TICKS,
            GROWTH_TICKS,
            true,
            &mut expected,
        );
        crowd(straight.world_mut());
        run_window(
            &mut straight,
            &mut enc,
            0,
            RESTORED_CROWD_LEAD + CROWD_TICKS,
            false,
            &mut expected,
        );
        expected
    }

    /// The seats of the protect-list match: the fewest a Push runs with (one
    /// seat alone ends its match at once, the last seat standing), and so
    /// the largest share of the beacon room a seat can hold.
    const PROTECT_SEATS: u32 = 2;

    /// The beacon rows each of the [`PROTECT_SEATS`] seats holds at most: its
    /// core and its share of the beacon room (`seat_beacon_limit_of(2)`,
    /// 1 + 20).
    const SEAT_BEACONS: usize = 21;

    /// The protect entries the Defend table can hold at most in a
    /// [`PROTECT_SEATS`]-seat match: every seat's [`SEAT_BEACONS`] Defend
    /// beacons, each protecting every one of its seat's beacons
    /// (`world::protect_room_of(2)`).
    const FULL_PROTECT: usize = 2 * SEAT_BEACONS * SEAT_BEACONS;

    /// The ticks a list-room Push runs before its counted window: the
    /// commander starts its walk to the beacon it will interface with.
    const LIST_LEAD: u32 = 20;

    /// The most ticks a list-room Push's counted window runs: the rest of the
    /// walk, every visit's handshake and rows (a settings edit at most six
    /// seconds, 120 ticks), and room to spare. The window ends early once the
    /// playbook's last row has committed or failed.
    const LIST_TICKS: u32 = 1_200;

    /// The committed rules table.
    fn committed_rules() -> RulesTable {
        RulesTable::load(
            &std::path::Path::new("..")
                .join("..")
                .join("rules")
                .join("rules.v1.json"),
        )
        .expect("the committed rules table loads")
    }

    /// A [`PROTECT_SEATS`]-seat match, in its opening Lull.
    fn list_world(rules: &RulesTable) -> World {
        World::new(&WorldConfig {
            match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
            seats: PROTECT_SEATS,
            rules: rules.clone(),
            match_settings: MatchSettings {
                segment_lengths_ms: vec![600_000],
                round_limit: DEFAULT_ROUND_LIMIT,
            },
        })
        .expect("the rules table describes a two-seat map")
    }

    /// `seat`'s core: its id and its column.
    fn core_of(world: &World, seat: u8) -> (pharmakos_sim::tables::BeaconId, [i32; 2]) {
        let row = world
            .beacons()
            .seats()
            .iter()
            .position(|owner| *owner == seat)
            .expect("the seat's core");
        let id = *world.beacons().ids().get(row).expect("the core's id");
        let at = *world
            .beacons()
            .positions()
            .get(row)
            .expect("the core's position");
        (
            pharmakos_sim::tables::BeaconId::new(id),
            pharmakos_sim::targeting::column_of(at),
        )
    }

    /// A playbook whose route is one interface visit per `(beacon, rows)`
    /// pair, each skipped on a failure, then a hold.
    fn visits(steps: &[(String, Vec<String>)]) -> pharmakos_proto::gp::v1::Playbook {
        let route: Vec<String> = steps
            .iter()
            .enumerate()
            .map(|(index, (beacon, rows))| {
                format!(
                    concat!(
                        "{{\"label\":\"visit_{index}\",\"interface\":{{\"beacon\":",
                        "{{\"beacon_id\":\"{beacon}\"}},\"rows\":[{rows}]}},",
                        "\"timeout_ms\":60000,\"on_fail\":{{\"action\":\"SKIP\"}}}}"
                    ),
                    index = index,
                    beacon = beacon,
                    rows = rows.join(","),
                )
            })
            .collect();
        let text = format!(
            concat!(
                "{{\"schema_version\":{{\"major\":1}},",
                "\"meta\":{{\"title\":\"lists\",\"author_kind\":\"HUMAN\"}},",
                "\"declarative\":{{\"route\":[{route},",
                "{{\"label\":\"stand\",\"hold\":{{\"ms\":60000}}}}]}},",
                "\"on_death\":{{\"on_respawn\":\"CONTINUE\",\"max_deaths_before_fallback\":3}},",
                "\"fallback\":{{\"hold\":{{\"at\":{{\"safest\":{{}}}}}}}},",
                "\"kind\":\"PLAYBOOK\"}}"
            ),
            route = route.join(","),
        );
        pharmakos_proto::json::decode(&text).expect("the playbook decodes")
    }

    /// Seal `playbook` for seat 0 of `world` and open the Push.
    fn pushing(
        world: World,
        rules: &RulesTable,
        playbook: &pharmakos_proto::gp::v1::Playbook,
    ) -> Runner {
        let plan = pharmakos_sim::interpreter::Plan::compile(playbook, rules).expect("it compiles");
        let mut runner = Runner::new(world).unwrap_or_else(|error| panic!("{error}"));
        runner
            .seal_playbook(pharmakos_sim::tables::SeatId::new(0), plan)
            .expect("a Lull takes a seal");
        assert!(runner.begin_push().unwrap(), "a match opens in a Lull");
        runner
    }

    /// What a list-room Push's counted window saw: what it allocated, and
    /// seat 0's committed rows and failed steps, by reason.
    struct ListWindow {
        allocated: u64,
        committed: u32,
        failed: Vec<pharmakos_sim::interpreter::StepFailure>,
        /// Every counted tick's hash.
        chain: Vec<u64>,
    }

    /// Step `runner` [`LIST_LEAD`] ticks uncounted, then count up to
    /// [`LIST_TICKS`] more, until seat 0 has committed or failed `outcomes`
    /// rows and steps in all. A refused step is read from its `step_failed`
    /// event; the events are read before they are drained, outside nothing
    /// (reading a slice allocates nothing).
    fn list_window(runner: &mut Runner, outcomes: u32) -> ListWindow {
        use pharmakos_sim::events::EventKind;
        let seat = Some(pharmakos_sim::tables::SeatId::new(0));
        let mut enc = Enc::with_capacity(64 * 1024);
        for _ in 0..LIST_LEAD {
            runner
                .step()
                .unwrap()
                .expect("the segment outlasts this test");
            runner.clear_events();
            runner.world().encode(&mut enc);
        }
        let mut committed: u32 = 0;
        let mut failed = Vec::with_capacity(8);
        let mut chain = Vec::with_capacity(usize::try_from(LIST_TICKS).expect("a count"));
        let before = allocations();
        for _ in 0..LIST_TICKS {
            let report = runner
                .step()
                .unwrap()
                .expect("the segment outlasts this test");
            chain.push(report.hash);
            for event in runner.events() {
                if event.seat != seat {
                    continue;
                }
                if event.kind == EventKind::RowCommitted {
                    committed += 1;
                } else if event.kind == EventKind::StepFailed {
                    failed.push(
                        pharmakos_sim::interpreter::StepFailed::decode(event.value)
                            .expect("a step_failed value decodes")
                            .reason,
                    );
                }
            }
            runner.clear_events();
            runner.world().encode(&mut enc);
            if committed + u32::try_from(failed.len()).expect("a count") >= outcomes {
                break;
            }
        }
        ListWindow {
            allocated: allocations() - before,
            committed,
            failed,
            chain,
        }
    }

    /// A [`PROTECT_SEATS`]-seat match whose Defend table ends the Push
    /// holding [`FULL_PROTECT`] entries, the most it can hold: each seat holds
    /// its whole share of the beacon rows, every beacon on the Defend writ
    /// protecting every beacon of its seat (the lists of earlier rounds:
    /// lists are kept from round to round), but for seat 0's last beacon,
    /// `b_20`, whose list names one. Seat 0's playbook visits `b_20` with two
    /// rows: the first clears its list, the second writes it whole, so the
    /// table falls by one entry and grows to its room inside the Push.
    fn protect_runner() -> Runner {
        use pharmakos_sim::seams::MandateKind;
        use pharmakos_sim::tables::{BeaconId, DefendSettings, PRIORITY_NORMAL, SeatId};
        let rules = committed_rules();
        let mut world = list_world(&rules);
        for raw in 0..u8::try_from(PROTECT_SEATS).expect("a seat id") {
            let seat = SeatId::new(raw);
            let (core, [cx, cy]) = core_of(&world, raw);
            world.set_writ(core, MandateKind::Defend);
            let mut beacons = vec![core];
            for index in 0..i32::try_from(SEAT_BEACONS - 1).expect("a count") {
                let at = world.standing_point([
                    cx - 12 + (index % 7) * 4,
                    cy + 6 + index.div_euclid(7) * 4,
                    0,
                ]);
                beacons.push(
                    world
                        .place_beacon_directly(seat, at, MandateKind::Defend, PRIORITY_NORMAL)
                        .expect("a beacon inside the seat's share"),
                );
            }
            let spare = world.standing_point([cx - 12, cy - 6, 0]);
            assert_eq!(
                world.place_beacon_directly(seat, spare, MandateKind::Defend, PRIORITY_NORMAL),
                None,
                "the seat holds its whole share of the beacon rows: no list can name more beacons"
            );
            for beacon in &beacons {
                // Seat 0's last beacon holds one entry, which its visit clears
                // and writes whole: the table grows by all but one entry inside
                // the Push, past what a table restored from the save holds.
                let list: &[BeaconId] = if raw == 0 && Some(beacon) == beacons.last() {
                    beacons.get(..1).expect("the core")
                } else {
                    &beacons
                };
                assert!(
                    world.set_defend(*beacon, DefendSettings::default(), list),
                    "a Defend row"
                );
            }
        }
        assert_eq!(
            world.defend().protected().len(),
            FULL_PROTECT - (SEAT_BEACONS - 1),
            "every list but seat 0's last is written whole before the match opens"
        );
        let entries: Vec<String> = (0..SEAT_BEACONS)
            .map(|ordinal| format!("{{\"beacon\":{{\"beacon_id\":\"b_{ordinal:02}\"}}}}"))
            .collect();
        let playbook = visits(&[(
            format!("b_{:02}", SEAT_BEACONS - 1),
            vec![
                "{\"set_mandate_settings\":{\"defend\":{\"clear_protect\":true}}}".to_owned(),
                format!(
                    "{{\"set_mandate_settings\":{{\"defend\":{{\"protect\":[{}]}}}}}}",
                    entries.join(",")
                ),
            ],
        )]);
        pushing(world, &rules, &playbook)
    }

    /// The protect-list Push's counted window, held to what it must show:
    /// both rows committed, nothing failed, the table back at
    /// [`FULL_PROTECT`]. Answers what the window allocated.
    fn protect_commit_allocations(runner: &mut Runner) -> u64 {
        let window = list_window(runner, 2);
        assert_eq!(
            (window.committed, window.failed.as_slice()),
            (2, &[][..]),
            "the visit's clear and its refill both committed inside the counted window"
        );
        assert_eq!(
            runner.world().defend().protected().len(),
            FULL_PROTECT,
            "b_20's list is written whole again, so each seat's {SEAT_BEACONS} Defend beacons \
             protect all {SEAT_BEACONS} of its beacons: the most entries the table can hold"
        );
        window.allocated
    }

    /// **A protect list committed inside a tick allocates nothing**, up to
    /// the most entries the Defend table can hold (lane `simroom`): the
    /// table's room is stated, `world::protect_room_of` the seat count,
    /// reserved with every other table's (`World::reserve_table_rooms`); the
    /// commit resolves the list into the world's protect scratch, which the
    /// world lends and takes back (`World::resolve_into_protect_scratch`);
    /// and [`protect_runner`]'s match clears a list and writes it whole, so
    /// the table ends its counted window holding exactly its room, and a room
    /// one entry short grows the list inside the tick. Then the same Push
    /// restored from its save, taken at the Push's first tick: a table
    /// restored from the file holds exactly its rows, and no room for more
    /// unless the restore states the room again.
    #[test]
    fn a_protect_list_commit_allocates_nothing_up_to_the_tables_room() {
        let mut fresh = protect_runner();
        let saved = fresh.capture().expect("a world between ticks");
        let bytes = saved.to_bytes().expect("the save encodes");
        let snapshot =
            pharmakos_sim::snapshot::Snapshot::from_bytes(&bytes).expect("the save decodes");
        let fresh_allocations = protect_commit_allocations(&mut fresh);
        assert_eq!(
            fresh_allocations, 0,
            "the ticks in which a commander's interface clears a protect list and writes it \
             again to fill the Defend table allocated {fresh_allocations} times"
        );

        let mut restored = protect_runner();
        restored.restore(&snapshot).expect("the save restores");
        let restored_allocations = protect_commit_allocations(&mut restored);
        assert_eq!(
            restored_allocations, 0,
            "the same ticks in a restored world allocated {restored_allocations} times: a \
             restore states the table's room again"
        );
    }

    /// The avoid areas the Attack table can hold in a [`PROTECT_SEATS`]-seat
    /// match: every seat's room together, the stated room
    /// `World::reserve_table_rooms` gives it.
    const AVOID_ROOM: usize = 2 * SEAT_AVOID_ROOM;

    /// The avoid areas one seat's Attack beacons can hold in all in a
    /// [`PROTECT_SEATS`]-seat match (`world::seat_avoid_room_of(2)`: its
    /// [`SEAT_BEACONS`] beacon rows times the per-beacon figure, 16). The
    /// room a commit is tested against is its own seat's (S3's `tune3`, E17).
    const SEAT_AVOID_ROOM: usize = SEAT_BEACONS * 16;

    /// The avoid areas seat 0's playbook writes on its core, `b_00`.
    const AVOID_WRITTEN: usize = 4;

    // The save [`avoid_runner`] starts from holds more than half the table's
    // room, so a restored column's capacity passes the room (P3 of the
    // review).
    const _: () = assert!(2 * (AVOID_ROOM - AVOID_WRITTEN) > AVOID_ROOM);

    /// A [`PROTECT_SEATS`]-seat match in which **seat 1 holds its whole
    /// avoid-area room** (its core on the Attack writ with
    /// [`SEAT_AVOID_ROOM`] areas) and seat 0 holds its own less
    /// [`AVOID_WRITTEN`]: a third beacon of its, `b_02`, on the Attack writ
    /// with them, its core and `b_01` on the Attack writ with none. Seat 0's
    /// playbook visits `b_00` and writes [`AVOID_WRITTEN`] areas, which fills
    /// seat 0's room exactly (seat 1's full room is no part of it), then
    /// `b_01` and writes one, which would pass it.
    fn avoid_runner() -> Runner {
        use pharmakos_sim::attack::AttackSettings;
        use pharmakos_sim::seams::MandateKind;
        use pharmakos_sim::tables::{ColumnBox, PRIORITY_NORMAL, SeatId};
        let rules = committed_rules();
        assert_eq!(
            usize::try_from(pharmakos_sim::world::seat_avoid_room_of(PROTECT_SEATS))
                .expect("a count"),
            SEAT_AVOID_ROOM,
            "the test's room is the world's"
        );
        let mut world = list_world(&rules);
        let (core, [cx, cy]) = core_of(&world, 0);
        world.set_writ(core, MandateKind::Attack);
        let at = world.standing_point([cx + 6, cy + 6, 0]);
        world
            .place_beacon_directly(SeatId::new(0), at, MandateKind::Attack, PRIORITY_NORMAL)
            .expect("a second Attack beacon");
        let at = world.standing_point([cx - 6, cy + 6, 0]);
        let third = world
            .place_beacon_directly(SeatId::new(0), at, MandateKind::Attack, PRIORITY_NORMAL)
            .expect("a third Attack beacon");
        let (other, [ox, oy]) = core_of(&world, 1);
        world.set_writ(other, MandateKind::Attack);
        let boxes = |origin: [i32; 2], count: usize| -> Vec<ColumnBox> {
            (0..count)
                .map(|index| {
                    let x = i16::try_from(origin[0]).expect("a column")
                        + i16::try_from(index % 32).expect("a column");
                    let y = i16::try_from(origin[1]).expect("a column")
                        + i16::try_from(index.div_euclid(32)).expect("a column");
                    ColumnBox::new([x, y], [x, y]).expect("a box")
                })
                .collect()
        };
        assert!(world.set_attack(
            other,
            AttackSettings::default(),
            &boxes([ox, oy], SEAT_AVOID_ROOM)
        ));
        assert!(world.set_attack(
            third,
            AttackSettings::default(),
            &boxes([cx, cy + 40], SEAT_AVOID_ROOM - AVOID_WRITTEN)
        ));
        assert_eq!(world.attack().avoid().len(), AVOID_ROOM - AVOID_WRITTEN);
        let z = 0;
        let area = |dx: i32| {
            format!(
                "{{\"min\":{{\"x\":{x},\"y\":{cy},\"z\":{z}}},\"max\":{{\"x\":{x},\"y\":{cy},\"z\":{z}}}}}",
                x = cx + dx,
            )
        };
        let written: Vec<String> = (0..i32::try_from(AVOID_WRITTEN).expect("a count"))
            .map(|index| area(index * 2 + 3))
            .collect();
        let playbook = visits(&[
            (
                "b_00".to_owned(),
                vec![format!(
                    "{{\"set_mandate_settings\":{{\"attack\":{{\"avoid_areas\":[{}]}}}}}}",
                    written.join(",")
                )],
            ),
            (
                "b_01".to_owned(),
                vec![format!(
                    "{{\"set_mandate_settings\":{{\"roe\":\"HOLD_FIRE\",\"attack\":{{\"avoid_areas\":[{}]}}}}}}",
                    area(-3)
                )],
            ),
        ]);
        pushing(world, &rules, &playbook)
    }

    /// **An avoid list committed inside a tick allocates nothing, and one that
    /// would pass its seat's stated room writes nothing and fails its
    /// step** `targets_unwritten`, as a Build list past its room does (lane
    /// `simroom`; the room a seat's own since S3's `tune3`): [`avoid_runner`]'s
    /// first visit fills seat 0's room exactly and commits, though seat 1
    /// holds its whole room beside it; its second would pass it by one area,
    /// and fails: nothing of
    /// its row is written, the `roe` it carries beside the list included
    /// (P2 of the review: every check before the first write); and neither
    /// allocates. A room one area short fails the first commit instead.
    /// Seat 0's `b_01`'s Common settings: what the refused row's `roe` would
    /// have written.
    fn b_01_common(runner: &Runner) -> pharmakos_sim::tables::Common {
        let row = runner
            .world()
            .beacons()
            .row_of_ordinal(pharmakos_sim::tables::SeatId::new(0), 1)
            .expect("b_01");
        *runner
            .world()
            .beacons()
            .commons()
            .get(row)
            .expect("b_01's settings")
    }

    #[test]
    fn an_avoid_list_commit_allocates_nothing_and_past_the_room_is_refused() {
        let mut runner = avoid_runner();
        let common_before = b_01_common(&runner);
        let window = list_window(&mut runner, 2);
        assert_eq!(
            (window.committed, window.failed.as_slice()),
            (
                1,
                &[pharmakos_sim::interpreter::StepFailure::TargetsUnwritten][..]
            ),
            "the commit at the room commits, and the one past it fails `targets_unwritten`"
        );
        assert_eq!(
            runner.world().attack().avoid().len(),
            AVOID_ROOM,
            "the table holds its room, and the refused list wrote nothing"
        );
        assert_eq!(
            b_01_common(&runner),
            common_before,
            "the refused row wrote nothing: not its avoid list, and not the `roe` beside it"
        );
        assert_eq!(
            window.allocated, 0,
            "the ticks of a commit at the avoid-area room and a refused one past it allocated {} \
             times",
            window.allocated
        );
    }

    /// **The avoid refusal decides on the stated room, never on capacity**
    /// (P3 of `simroom`'s review; #234's rule: capacity is never state). The
    /// same Push as [`an_avoid_list_commit_allocates_nothing_and_past_the_room_is_refused`],
    /// restored from its save at the Push's first tick: the file holds more
    /// than half the room (668 of 672 areas), so the restore's
    /// `reserve_total` grows the column to about twice its length, past the
    /// room. The restored run refuses the same row, commits the same one,
    /// allocates nothing, and its chain is the fresh run's, tick for tick.
    #[test]
    fn an_avoid_refusal_decides_on_the_stated_room_and_never_on_capacity() {
        let mut fresh = avoid_runner();
        let saved = fresh.capture().expect("a world between ticks");
        let bytes = saved.to_bytes().expect("the save encodes");
        let snapshot =
            pharmakos_sim::snapshot::Snapshot::from_bytes(&bytes).expect("the save decodes");
        let fresh_window = list_window(&mut fresh, 2);
        let mut restored = avoid_runner();
        restored.restore(&snapshot).expect("the save restores");
        let restored_window = list_window(&mut restored, 2);
        assert_eq!(
            (
                restored_window.committed,
                restored_window.failed.as_slice(),
                restored.world().attack().avoid().len(),
            ),
            (
                fresh_window.committed,
                fresh_window.failed.as_slice(),
                fresh.world().attack().avoid().len(),
            ),
            "the restored world commits and refuses as the fresh one does"
        );
        assert_eq!(restored_window.allocated, 0);
        assert_eq!(
            restored_window.chain, fresh_window.chain,
            "the restored run's chain is the fresh run's"
        );
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

/// **The source guard's walker.** The types the world owns are not a hand
/// list: they are found by walking field types from `World` through the
/// sim's sources, every struct and enum reachable by value (through
/// `Option`, `Box`, tuples, arrays and every other wrapper), and every `type`
/// alias expanded. A name is resolved as the compiler would find it, as far
/// as a reading of the source can: a path names its module (`crate::a::B`,
/// or `a::B` relative), a bare name is the file's own item or what the
/// file's `use` declarations bring in; where that still leaves more than one
/// item, every one of them is walked, so the walk errs towards reading too
/// much. A field is *plain* when `Vec`, `VecDeque` or `String` names a type
/// anywhere in it, qualified paths included, or when it holds a `Box<[T]>`;
/// a type name the walker cannot resolve is an error naming the field, never
/// a skip.
mod owned {
    use super::{Token, tokens};

    /// One field of a struct, or of an enum's variant: its name (a tuple
    /// field's index) and its type's tokens.
    #[derive(Clone, Debug)]
    struct Field {
        name: String,
        ty: Vec<Token>,
    }

    /// A struct or enum defined at module level.
    #[derive(Clone, Debug)]
    struct Item {
        file: String,
        module: Vec<String>,
        name: String,
        generics: Vec<String>,
        fields: Vec<Field>,
    }

    /// A module-level `type` alias.
    #[derive(Clone, Debug)]
    struct Alias {
        file: String,
        name: String,
        generics: Vec<String>,
        rhs: Vec<Token>,
    }

    /// A `use` declaration's binding: in `file`, `name` means `path`; a glob
    /// binds no name and brings in every item of `path`.
    #[derive(Clone, Debug)]
    struct Use {
        file: String,
        name: Option<String>,
        path: Vec<String>,
    }

    /// Every item, alias and `use` of a set of sources.
    #[derive(Default, Debug)]
    pub(super) struct Sources {
        items: Vec<Item>,
        aliases: Vec<Alias>,
        uses: Vec<Use>,
        /// Every file's tokens, for [`room_unstated`]'s search of its impls.
        texts: Vec<(String, Vec<Token>)>,
    }

    /// What the walk found: every plain field `(file, type, field)`, every
    /// field naming a type it could not resolve `(file, type, field, name)`,
    /// and the types it walked `(file, type)`.
    #[derive(Default, Debug)]
    pub(super) struct Walked {
        pub(super) plain: Vec<(String, String, String)>,
        pub(super) unresolved: Vec<(String, String, String, String)>,
        pub(super) types: Vec<(String, String)>,
        /// Every field whose type names [`RESERVED`] `(file, type, field)`:
        /// a vector that keeps its room, whose room [`room_unstated`] looks for.
        pub(super) reserved: Vec<(String, String, String)>,
    }

    /// The names that mark a field plain.
    const PLAIN: &[&str] = &["Vec", "VecDeque", "String"];

    /// Types the walker knows and does not descend into: the language's
    /// scalars and the standard library's wrappers, whose arguments it walks.
    const KNOWN: &[&str] = &[
        "u8",
        "u16",
        "u32",
        "u64",
        "u128",
        "usize",
        "i8",
        "i16",
        "i32",
        "i64",
        "i128",
        "isize",
        "bool",
        "char",
        "str",
        "Option",
        "Box",
        "Arc",
        "Rc",
        "Cell",
        "RefCell",
        "Result",
        "PhantomData",
        "BTreeMap",
        "BTreeSet",
        "AtomicBool",
        "AtomicU8",
        "AtomicU32",
        "AtomicU64",
        "NonZeroU8",
        "NonZeroU16",
        "NonZeroU32",
        "NonZeroU64",
        "NonZeroUsize",
        "NonZeroI32",
        "NonZeroI64",
        "Vec",
        "VecDeque",
        "String",
    ];

    fn word(token: Option<&Token>) -> Option<&str> {
        match token {
            Some(Token::Word(word)) => Some(word),
            _ => None,
        }
    }

    fn is_punct(token: Option<&Token>, text: &str) -> bool {
        matches!(token, Some(Token::Punct(p)) if p == text)
    }

    /// The module path of a source file: `src/a/b.rs` is `a::b`, `src/a.rs`
    /// is `a`, and `src/lib.rs` the crate root.
    fn module_of(file: &str) -> Vec<String> {
        let inner = file.strip_prefix("src/").unwrap_or(file);
        let inner = inner.strip_suffix(".rs").unwrap_or(inner);
        let inner = inner.strip_suffix("/mod").unwrap_or(inner);
        if inner == "lib" {
            return Vec::new();
        }
        inner.split('/').map(str::to_owned).collect()
    }

    /// The index past the bracketed group that opens at `i` (`(`, `[`,
    /// `{` or `<`), or the end.
    fn past_group(tokens: &[Token], i: usize) -> usize {
        let mut depth = 0_i64;
        let mut j = i;
        while let Some(token) = tokens.get(j) {
            if let Token::Punct(p) = token {
                match p.as_str() {
                    "(" | "[" | "{" | "<" => depth += 1,
                    ")" | "]" | "}" | ">" => depth -= 1,
                    _ => {}
                }
            }
            j += 1;
            if depth == 0 {
                return j;
            }
        }
        j
    }

    /// The index past an attribute (`#[..]` or `#![..]`) at `i`, if one opens
    /// there.
    fn past_attribute(tokens: &[Token], i: usize) -> Option<usize> {
        if !is_punct(tokens.get(i), "#") {
            return None;
        }
        let open = if is_punct(tokens.get(i + 1), "!") {
            i + 2
        } else {
            i + 1
        };
        is_punct(tokens.get(open), "[").then(|| past_group(tokens, open))
    }

    /// The index past a visibility at `i` (`pub`, `pub(crate)`,
    /// `pub(in ..)`), or `i`.
    fn past_visibility(tokens: &[Token], i: usize) -> usize {
        if word(tokens.get(i)) != Some("pub") {
            return i;
        }
        if is_punct(tokens.get(i + 1), "(") {
            past_group(tokens, i + 1)
        } else {
            i + 1
        }
    }

    /// The generic parameters' names of a `<..>` group at `i`, and the index
    /// past it.
    fn generics(tokens: &[Token], i: usize) -> (Vec<String>, usize) {
        if !is_punct(tokens.get(i), "<") {
            return (Vec::new(), i);
        }
        let end = past_group(tokens, i);
        let mut names = Vec::new();
        let mut expect = true;
        let mut depth = 0_i64;
        for token in tokens.get(i + 1..end.saturating_sub(1)).unwrap_or(&[]) {
            match token {
                Token::Punct(p) if p == "<" || p == "(" || p == "[" => depth += 1,
                Token::Punct(p) if p == ">" || p == ")" || p == "]" => depth -= 1,
                Token::Punct(p) if p == "," && depth == 0 => expect = true,
                Token::Word(w) if expect && depth == 0 && w != "const" => {
                    names.push(w.clone());
                    expect = false;
                }
                Token::Other if depth == 0 => expect = false,
                _ => {}
            }
        }
        (names, end)
    }

    /// Split `tokens[from..to]` at its top-level commas.
    fn split_commas(tokens: &[Token], from: usize, to: usize) -> Vec<Vec<Token>> {
        let mut parts = Vec::new();
        let mut part = Vec::new();
        let mut depth = 0_i64;
        for token in tokens.get(from..to).unwrap_or(&[]) {
            if let Token::Punct(p) = token {
                match p.as_str() {
                    "(" | "[" | "{" | "<" => depth += 1,
                    ")" | "]" | "}" | ">" => depth -= 1,
                    "," if depth == 0 => {
                        parts.push(core::mem::take(&mut part));
                        continue;
                    }
                    _ => {}
                }
            }
            part.push(token.clone());
        }
        if !part.is_empty() {
            parts.push(part);
        }
        parts
    }

    /// A part's tokens with its leading attributes and visibility removed.
    fn bare(part: &[Token]) -> Vec<Token> {
        let mut i = 0;
        while let Some(end) = past_attribute(part, i) {
            i = end;
        }
        let i = past_visibility(part, i);
        part.get(i..).unwrap_or(&[]).to_vec()
    }

    /// The named fields inside braces, `tokens[from..to]`.
    fn named_fields(tokens: &[Token], from: usize, to: usize, prefix: &str) -> Vec<Field> {
        split_commas(tokens, from, to)
            .into_iter()
            .filter_map(|part| {
                let part = bare(&part);
                let name = word(part.first())?;
                is_punct(part.get(1), ":").then(|| Field {
                    name: format!("{prefix}{name}"),
                    ty: part.get(2..).unwrap_or(&[]).to_vec(),
                })
            })
            .collect()
    }

    /// The tuple fields inside parentheses, `tokens[from..to]`, named by
    /// index.
    fn tuple_fields(tokens: &[Token], from: usize, to: usize, prefix: &str) -> Vec<Field> {
        split_commas(tokens, from, to)
            .into_iter()
            .enumerate()
            .map(|(index, part)| Field {
                name: format!("{prefix}{index}"),
                ty: bare(&part),
            })
            .collect()
    }

    /// The fields of an enum's variants inside braces, `tokens[from..to]`,
    /// named `Variant.field` (`Variant.0` for a tuple variant's).
    fn variant_fields(tokens: &[Token], from: usize, to: usize) -> Vec<Field> {
        let mut fields = Vec::new();
        for variant in split_commas(tokens, from, to) {
            let variant = bare(&variant);
            let Some(name) = word(variant.first()) else {
                continue;
            };
            let prefix = format!("{name}.");
            let close = past_group(&variant, 1);
            if is_punct(variant.get(1), "{") {
                fields.extend(named_fields(&variant, 2, close.saturating_sub(1), &prefix));
            } else if is_punct(variant.get(1), "(") {
                fields.extend(tuple_fields(&variant, 2, close.saturating_sub(1), &prefix));
            }
        }
        fields
    }

    /// Expand a `use` tree, `tokens`, under `base`, into bindings.
    fn use_tree(tokens: &[Token], base: &[String], file: &str, out: &mut Vec<Use>) {
        let mut path = base.to_vec();
        let mut i = 0;
        while let Some(token) = tokens.get(i) {
            match token {
                Token::Word(w) if w == "as" => {
                    if let Some(alias) = word(tokens.get(i + 1))
                        && alias != "_"
                    {
                        out.push(Use {
                            file: file.to_owned(),
                            name: Some(alias.to_owned()),
                            path: path.clone(),
                        });
                    }
                    return;
                }
                Token::Word(w) => path.push(w.clone()),
                Token::Punct(p) if p == "*" => {
                    out.push(Use {
                        file: file.to_owned(),
                        name: None,
                        path,
                    });
                    return;
                }
                Token::Punct(p) if p == "{" => {
                    let end = past_group(tokens, i);
                    for part in split_commas(tokens, i + 1, end.saturating_sub(1)) {
                        use_tree(&part, &path, file, out);
                    }
                    return;
                }
                _ => {}
            }
            i += 1;
        }
        if let Some(last) = path.last() {
            let (name, path) = if last == "self" {
                let module: Vec<String> = path.iter().take(path.len() - 1).cloned().collect();
                (module.last().cloned(), module)
            } else {
                (Some(last.clone()), path.clone())
            };
            if let Some(name) = name {
                out.push(Use {
                    file: file.to_owned(),
                    name: Some(name),
                    path,
                });
            }
        }
    }

    /// The struct or enum whose keyword is at `i`, and the index past it.
    fn read_item(
        tokens: &[Token],
        i: usize,
        is_enum: bool,
        file: &str,
        module: &[String],
    ) -> Option<(Item, usize)> {
        let name = word(tokens.get(i + 1))?.to_owned();
        let (generics, mut j) = generics(tokens, i + 2);
        while let Some(t) = tokens.get(j) {
            if ["{", "(", ";"].iter().any(|p| is_punct(Some(t), p)) {
                break;
            }
            j += 1;
        }
        let end = past_group(tokens, j);
        let inner = end.saturating_sub(1);
        let fields = if is_punct(tokens.get(j), "{") {
            if is_enum {
                variant_fields(tokens, j + 1, inner)
            } else {
                named_fields(tokens, j + 1, inner, "")
            }
        } else if is_punct(tokens.get(j), "(") {
            tuple_fields(tokens, j + 1, inner, "")
        } else {
            Vec::new()
        };
        Some((
            Item {
                file: file.to_owned(),
                module: module.to_vec(),
                name,
                generics,
                fields,
            },
            end,
        ))
    }

    /// Read every module-level struct, enum, `type` alias and `use` of
    /// `text`, the source of `file`, into `out`. Items inside a function, an
    /// `impl` or a `trait` are not the module's and are left out (an
    /// associated `type` is not an alias).
    pub(super) fn read(file: &str, text: &str, out: &mut Sources) {
        let tokens = tokens(text);
        out.texts.push((file.to_owned(), tokens.clone()));
        let module = module_of(file);
        // The kind of each open brace: true for a module's.
        let mut blocks: Vec<bool> = Vec::new();
        let mut last_keyword = "";
        let mut i = 0;
        while let Some(token) = tokens.get(i) {
            let at_module = blocks.iter().all(|module| *module);
            match token {
                Token::Punct(p) if p == "{" => {
                    blocks.push(last_keyword == "mod");
                    last_keyword = "";
                }
                Token::Punct(p) if p == "}" => {
                    blocks.pop();
                }
                Token::Punct(p) if p == ";" => last_keyword = "",
                Token::Word(w) if at_module && w == "use" => {
                    let mut k = i + 1;
                    while tokens.get(k).is_some_and(|t| !is_punct(Some(t), ";")) {
                        k += 1;
                    }
                    let tree = tokens.get(i + 1..k).unwrap_or(&[]);
                    use_tree(tree, &[], file, &mut out.uses);
                    i = k;
                }
                Token::Word(w) if at_module && (w == "struct" || w == "enum") => {
                    if let Some((item, end)) = read_item(&tokens, i, w == "enum", file, &module) {
                        out.items.push(item);
                        i = end;
                        last_keyword = "";
                        continue;
                    }
                }
                Token::Word(w) if at_module && w == "type" => {
                    if let Some(name) = word(tokens.get(i + 1)) {
                        let name = name.to_owned();
                        let (generics, j) = generics(&tokens, i + 2);
                        if is_punct(tokens.get(j), "=") {
                            let mut k = j + 1;
                            let mut rhs = Vec::new();
                            while let Some(t) = tokens.get(k) {
                                if is_punct(Some(t), ";") {
                                    break;
                                }
                                rhs.push(t.clone());
                                k += 1;
                            }
                            out.aliases.push(Alias {
                                file: file.to_owned(),
                                name,
                                generics,
                                rhs,
                            });
                            i = k;
                        }
                    }
                }
                Token::Word(w)
                    if [
                        "mod",
                        "impl",
                        "trait",
                        "fn",
                        "struct",
                        "enum",
                        "union",
                        "macro_rules",
                    ]
                    .contains(&w.as_str()) =>
                {
                    last_keyword = if w == "mod" { "mod" } else { "other" };
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// The type paths a type's tokens name (`a::b::C` is one path), leaving
    /// out constants (an array's length) and keywords; and whether it holds
    /// a `Box<[..]>`.
    fn paths(ty: &[Token]) -> (Vec<Vec<String>>, bool) {
        let mut paths = Vec::new();
        let mut boxed_slice = false;
        let mut current: Vec<String> = Vec::new();
        for (index, token) in ty.iter().enumerate() {
            if let Token::Word(w) = token {
                if is_punct(ty.get(index + 1), "::") {
                    current.push(w.clone());
                    continue;
                }
                let keyword = ["dyn", "mut", "const", "for", "impl", "Self"].contains(&w.as_str());
                let constant = w.len() > 1
                    && w.chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
                if w == "Box"
                    && is_punct(ty.get(index + 1), "<")
                    && is_punct(ty.get(index + 2), "[")
                {
                    boxed_slice = true;
                }
                if !keyword && !constant {
                    current.push(w.clone());
                    paths.push(core::mem::take(&mut current));
                }
                current.clear();
            } else if !is_punct(Some(token), "::") {
                current.clear();
            }
        }
        (paths, boxed_slice)
    }

    /// Whether `module` is the module a path's prefix names, from a file in
    /// `from`: the same module (`crate::a::b`), one of its children (an item
    /// re-exported from a child), or the prefix read relative to `from` or
    /// to any parent of it.
    fn names_module(module: &[String], prefix: &[String], from: &[String]) -> bool {
        let prefix: Vec<String> = prefix
            .iter()
            .filter(|s| !["crate", "self", "super"].contains(&s.as_str()))
            .cloned()
            .collect();
        if module.starts_with(&prefix) || module.ends_with(&prefix) {
            return true;
        }
        let mut relative = from.to_vec();
        relative.extend(prefix.iter().cloned());
        module.starts_with(&relative)
    }

    /// The items a type path names, read from the file `from`.
    fn resolve<'s>(sources: &'s Sources, from: &str, path: &[String]) -> Vec<&'s Item> {
        let Some(name) = path.last() else {
            return Vec::new();
        };
        // A name a `use .. as` binds in this file is the path it renames.
        if let [only] = path
            && let Some(renamed) = sources.uses.iter().find(|u| {
                u.file == from && u.name.as_ref() == Some(only) && u.path.last() != Some(only)
            })
        {
            return resolve(sources, from, &renamed.path);
        }
        let all: Vec<&Item> = sources.items.iter().filter(|i| &i.name == name).collect();
        if all.len() <= 1 {
            return all;
        }
        let from_module = module_of(from);
        let narrowed: Vec<&Item> = if path.len() > 1 {
            let prefix = path.get(..path.len() - 1).unwrap_or(&[]);
            all.iter()
                .copied()
                .filter(|item| names_module(&item.module, prefix, &from_module))
                .collect()
        } else {
            let own: Vec<&Item> = all.iter().copied().filter(|i| i.file == from).collect();
            if own.is_empty() {
                let mut found = Vec::new();
                for binding in sources.uses.iter().filter(|u| u.file == from) {
                    match &binding.name {
                        Some(bound) if bound == name => {
                            let prefix = binding.path.get(..binding.path.len().saturating_sub(1));
                            found.extend(all.iter().copied().filter(|item| {
                                Some(&item.name) == binding.path.last()
                                    && names_module(
                                        &item.module,
                                        prefix.unwrap_or(&[]),
                                        &from_module,
                                    )
                            }));
                        }
                        None => found.extend(all.iter().copied().filter(|item| {
                            names_module(&item.module, &binding.path, &from_module)
                        })),
                        _ => {}
                    }
                }
                found
            } else {
                own
            }
        };
        if narrowed.is_empty() { all } else { narrowed }
    }

    /// Walk from `root` (`(file, name)`), descending into every type named
    /// in a field, except `terminal` types, which are reached and not read.
    pub(super) fn walk(sources: &Sources, root: (&str, &str), terminal: &[&str]) -> Walked {
        let mut walked = Walked::default();
        let mut queue: Vec<&Item> = sources
            .items
            .iter()
            .filter(|item| item.file == root.0 && item.name == root.1)
            .collect();
        while let Some(item) = queue.pop() {
            let key = (item.file.clone(), item.name.clone());
            if walked.types.contains(&key) {
                continue;
            }
            walked.types.push(key);
            for field in &item.fields {
                let mut plain = false;
                let mut reserved = false;
                let mut pending: Vec<(Vec<Token>, Vec<String>, String)> =
                    vec![(field.ty.clone(), item.generics.clone(), item.file.clone())];
                let mut expanded: Vec<String> = Vec::new();
                while let Some((ty, generics, from)) = pending.pop() {
                    let (named, boxed_slice) = paths(&ty);
                    plain |= boxed_slice;
                    for path in named {
                        let Some(name) = path.last() else { continue };
                        if PLAIN.contains(&name.as_str()) {
                            plain = true;
                        }
                        if name == RESERVED {
                            reserved = true;
                        }
                        if KNOWN.contains(&name.as_str())
                            || generics.contains(name)
                            || terminal.contains(&name.as_str())
                        {
                            continue;
                        }
                        let aliases: Vec<&Alias> =
                            sources.aliases.iter().filter(|a| &a.name == name).collect();
                        let items = resolve(sources, &from, &path);
                        if aliases.is_empty() && items.is_empty() {
                            walked.unresolved.push((
                                item.file.clone(),
                                item.name.clone(),
                                field.name.clone(),
                                name.clone(),
                            ));
                            continue;
                        }
                        for alias in aliases {
                            if !expanded.contains(&alias.name) {
                                expanded.push(alias.name.clone());
                                let mut scope = generics.clone();
                                scope.extend(alias.generics.iter().cloned());
                                pending.push((alias.rhs.clone(), scope, alias.file.clone()));
                            }
                        }
                        queue.extend(items);
                    }
                }
                if plain {
                    walked
                        .plain
                        .push((item.file.clone(), item.name.clone(), field.name.clone()));
                }
                if reserved {
                    walked.reserved.push((
                        item.file.clone(),
                        item.name.clone(),
                        field.name.clone(),
                    ));
                }
            }
        }
        walked.plain.sort();
        walked.plain.dedup();
        walked.reserved.sort();
        walked.reserved.dedup();
        walked.unresolved.sort();
        walked.types.sort();
        walked
    }

    /// The type whose fields keep their room in a clone ([`Walked::reserved`]).
    pub(super) const RESERVED: &str = "Reserved";

    /// The methods that state a vector's room on it: `field.reserve(..)` or
    /// `field.reserve_total(..)` inside an impl of the field's type.
    const ROOM_METHODS: &[&str] = &["reserve", "reserve_total"];

    /// Each `{ .. }` body of an `impl` whose self type is named `ty`, in
    /// every source, as token ranges: `impl Ty`, `impl<T> Ty<T>` and
    /// `impl Trait for Ty`. Braces alone are counted, so a `<` in a body is
    /// never read as a bracket.
    fn impl_bodies<'s>(sources: &'s Sources, ty: &str) -> Vec<&'s [Token]> {
        let mut bodies = Vec::new();
        for (_, tokens) in &sources.texts {
            let mut i = 0;
            while let Some(token) = tokens.get(i) {
                if word(Some(token)) != Some("impl") {
                    i += 1;
                    continue;
                }
                // The header: every word up to the body's brace, the self
                // type being the last path's head after `for`, if any.
                let mut j = i + 1;
                let mut angle = 0_i64;
                let mut self_name: Option<&str> = None;
                while let Some(t) = tokens.get(j) {
                    match t {
                        Token::Punct(p) if p == "{" && angle == 0 => break,
                        Token::Punct(p) if p == ";" => break,
                        Token::Punct(p) if p == "<" => angle += 1,
                        Token::Punct(p) if p == ">" => angle -= 1,
                        Token::Word(w) if angle == 0 && w == "for" => self_name = None,
                        Token::Word(w) if angle == 0 && w != "where" => {
                            if self_name.is_none() || is_punct(tokens.get(j - 1), "::") {
                                self_name = Some(w.as_str());
                            }
                        }
                        Token::Word(w) if w == "where" => break,
                        _ => {}
                    }
                    j += 1;
                }
                // Past a `where` clause, to the body's brace.
                while tokens.get(j).is_some_and(|t| !is_punct(Some(t), "{")) {
                    j += 1;
                }
                let start = j;
                let mut depth = 0_i64;
                while let Some(t) = tokens.get(j) {
                    if is_punct(Some(t), "{") {
                        depth += 1;
                    } else if is_punct(Some(t), "}") {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
                if self_name == Some(ty)
                    && let Some(body) = tokens.get(start..=j.min(tokens.len().saturating_sub(1)))
                {
                    bodies.push(body);
                }
                i = j.max(i + 1);
            }
        }
        bodies
    }

    /// Whether `body` states `field`'s room: a struct literal's
    /// `field: ..::with_capacity(..)` or a `field: vec![value; n]` buffer
    /// (its whole length is its room, written by index), or
    /// `.field.reserve(..)` / `.field.reserve_total(..)`.
    fn states_room(body: &[Token], field: &str) -> bool {
        body.iter().enumerate().any(|(i, token)| {
            if word(Some(token)) != Some(field) {
                return false;
            }
            let literal = is_punct(body.get(i + 1), ":") && {
                let mut k = i + 2;
                while body
                    .get(k)
                    .is_some_and(|t| matches!(t, Token::Word(_)) || is_punct(Some(t), "::"))
                {
                    k += 1;
                }
                word(body.get(k.wrapping_sub(1))) == Some("with_capacity")
                    && is_punct(body.get(k), "(")
            };
            // A `vec![value; n]` literal: a buffer of its whole length, written
            // by index and never pushed past it.
            let sized = is_punct(body.get(i + 1), ":")
                && word(body.get(i + 2)) == Some("vec")
                && is_punct(body.get(i + 3), "!");
            let method = is_punct(body.get(i.wrapping_sub(1)), ".")
                && is_punct(body.get(i + 1), ".")
                && word(body.get(i + 2)).is_some_and(|m| ROOM_METHODS.contains(&m))
                && is_punct(body.get(i + 3), "(");
            literal || sized || method
        })
    }

    /// The `Reserved` fields the walk found whose room no impl of their type
    /// states ([`states_room`]): a vector that keeps a room it was never
    /// given, so its first `push` inside a tick allocates.
    pub(super) fn room_unstated(
        sources: &Sources,
        walked: &Walked,
    ) -> Vec<(String, String, String)> {
        walked
            .reserved
            .iter()
            .filter(|(_, ty, field)| {
                !impl_bodies(sources, ty)
                    .iter()
                    .any(|body| states_room(body, field))
            })
            .cloned()
            .collect()
    }

    /// The room check on sources given as `(file, text)` pairs, from `World`.
    fn unstated_texts(files: &[(&str, &str)]) -> Vec<String> {
        let mut sources = Sources::default();
        for (file, text) in files {
            read(file, text, &mut sources);
        }
        let walked = walk(&sources, ("src/world.rs", "World"), &[RESERVED]);
        room_unstated(&sources, &walked)
            .iter()
            .map(|(file, ty, field)| format!("{file} {ty}.{field}"))
            .collect()
    }

    /// The room check's own cases: a `Reserved` field given room by a
    /// struct literal's `with_capacity` or `vec!` buffer, by `reserve` or `reserve_total` in
    /// an inherent or a trait impl, passes; one built from an empty `Vec` or
    /// a default, or reserved only in another type's impl, or by a method
    /// of the same name on another field, fails, naming the field.
    #[test]
    fn the_room_check_finds_every_reserved_vector_given_no_room() {
        let world = r"
            pub struct World { given: Reserved<u8>, sized: Reserved<u8>, grown: Reserved<u8>, table: Table, bare: Reserved<u8> }
            impl World {
                fn new() -> World {
                    World {
                        given: crate::reserved::Reserved::with_capacity(4),
                        sized: vec![0; 4].into(),
                        grown: Reserved::default(),
                        table: Table::default(),
                        bare: Vec::new().into(),
                    }
                }
                fn rooms(&mut self) { if 1 < 2 { self.grown.reserve_total(8); } self.table.reserve_rows(3); }
            }
            pub struct Table { rows: Reserved<u32>, protected: Reserved<u32>, other: Other }
            impl Default for Table {
                fn default() -> Table { Table { rows: Vec::new().into(), protected: Vec::new().into(), other: Other::default() } }
            }
            impl Table { pub fn reserve_rows(&mut self, n: usize) { self.rows.reserve(n); } }
            pub struct Other { protected: Reserved<u8> }
            impl<T> Fill<T> for Other where T: Copy { fn fill(&mut self) { self.protected.reserve(2); } }
        ";
        assert_eq!(
            unstated_texts(&[("src/world.rs", world)]),
            ["src/world.rs Table.protected", "src/world.rs World.bare"],
            "the fields no impl of their own type gives room"
        );
    }

    /// The walker on sources given as `(file, text)` pairs, from `World`.
    fn walk_texts(files: &[(&str, &str)], terminal: &[&str]) -> Walked {
        let mut sources = Sources::default();
        for (file, text) in files {
            read(file, text, &mut sources);
        }
        walk(&sources, ("src/world.rs", "World"), terminal)
    }

    /// The walker's own cases: every way a review found to hide a vector
    /// from a hand list, a name two modules define, and an unresolved type,
    /// which must fail.
    #[test]
    fn the_walker_finds_every_vector_the_world_holds() {
        let world = r"
            use crate::ids::Row;
            use crate::other::{self, Deep as Deeper};
            pub struct World {
                plain: Vec<u32>,
                qualified: std::vec::Vec<u32>,
                aliased: Rows,
                optional: Option<Vec<u8>>,
                pair: (u8, std::collections::VecDeque<u8>),
                pub(in crate::world) scoped: String,
                regrown: Box<[u32]>,
                held: Holder,
                elsewhere: Deeper,
                generic: Wrap<Leaf>,
                #[allow(dead_code)]
                array: [Leaf; CHUNK_VOXELS],
                kinds: Kinds,
                tuple: TupleStruct,
                reserved: Reserved<Leaf>,
                rows: Reserved<Row>,
                fine: u32,
            }
            pub(crate) struct Holder { buf: Vec<u32> }
            type Rows = Vec<Leaf>;
            struct Wrap<T> { inner: T, count: u32 }
            pub struct Leaf { x: u8 }
            pub enum Kinds { A, B(Vec<u8>), C { list: Vec<u8> } }
            pub struct TupleStruct(pub u8, String);
            impl World { type Hidden = Vec<u8>; fn f() { struct Local { v: Vec<u8> } } }
        ";
        let other = "pub struct Deep { pub(crate) items: Vec<super::world::Leaf> }";
        let ids = "pub struct Row(u32);";
        let settings = "pub enum Row { Settings { list: Vec<u8> } }";
        let walked = walk_texts(
            &[
                ("src/world.rs", world),
                ("src/other.rs", other),
                ("src/ids.rs", ids),
                ("src/interpreter.rs", settings),
            ],
            &["Reserved"],
        );
        let plain: Vec<String> = walked
            .plain
            .iter()
            .map(|(file, ty, field)| format!("{file} {ty}.{field}"))
            .collect();
        assert_eq!(
            plain,
            [
                "src/other.rs Deep.items",
                "src/world.rs Holder.buf",
                "src/world.rs Kinds.B.0",
                "src/world.rs Kinds.C.list",
                "src/world.rs TupleStruct.1",
                "src/world.rs World.aliased",
                "src/world.rs World.optional",
                "src/world.rs World.pair",
                "src/world.rs World.plain",
                "src/world.rs World.qualified",
                "src/world.rs World.regrown",
                "src/world.rs World.scoped",
            ],
            "every vector the world holds, however it is spelled or wherever it lives, and \
             not the other module's `Row`"
        );
        assert!(walked.unresolved.is_empty(), "{:?}", walked.unresolved);
        assert!(
            !walked
                .types
                .iter()
                .any(|(_, ty)| ty == "Local" || ty == "Reserved"),
            "a function's local struct and a terminal type are not walked"
        );

        let unknown = walk_texts(
            &[("src/world.rs", "pub struct World { mystery: Mystery<u8> }")],
            &[],
        );
        assert_eq!(
            unknown.unresolved,
            [(
                "src/world.rs".to_owned(),
                "World".to_owned(),
                "mystery".to_owned(),
                "Mystery".to_owned()
            )],
            "a type the walker cannot resolve is an error naming the field"
        );
    }
}

/// Types the walker reaches and does not read, each with why a vector inside
/// it may drop its room in a clone.
const TERMINAL: &[(&str, &str)] = &[
    (
        "Reserved",
        "the type this guard holds the world to: its clone keeps the room (src/reserved.rs)",
    ),
    (
        "RulesTable",
        "an input: loaded once and never written by a tick, so no vector of it grows",
    ),
    (
        "Plan",
        "a sealed playbook, an input as the rules table is: compiled at the seal, then read, \
         never written by a tick",
    ),
];

/// A field sized once to one row a seat (`config.seats`), at construction and
/// again at a restore, and written by index.
const PER_SEAT: &str = "one row a seat: sized to the seat count at construction and at a restore, \
                        written by index, never pushed in a tick";

/// A field sized once to the map: one entry a chunk, column or voxel of it.
const PER_MAP: &str = "sized once to the map (one a chunk, column or voxel of the rules \
                       table's map size) where it is built, never pushed in a tick";

/// A field of the pathing graph's search scratch, `vec![..; n]` in its
/// constructor.
const PER_NODE: &str = "`vec![..; n]` in its constructor, `n` the surface graph's node count \
                        (or the abstract node count plus two), and written by index";

/// A field of the cluster decomposition, `vec![..]` in `Clusters::new`.
const PER_CLUSTER: &str = "`vec![..]` in `Clusters::new`, sized from the cluster count, the \
                           transitions a border holds (`max_trans`), the intra edges \
                           (`max_intra`) and the node count; written by index";

/// Why the support rule's lists may be plain: the only clone of the rule a
/// world makes is the support scratch's written-out one, which builds a fresh
/// rule with its room.
const SUPPORT_RULE: &str = "cloned only by the support scratch's written-out Clone, which builds \
                            a fresh rule with its room (`SupportRule::with_room` of its `Room`)";

/// Every plain field the walker finds, with what bounds it: a field reserved
/// past its length must be a `Reserved`; a new plain field fails the guard
/// until it is one or is listed here with its bound.
const PLAIN_VECTORS: &[(&str, &str, &str, &str)] = &[
    (
        "src/world.rs",
        "World",
        "spiral",
        "built once in World::new by `targeting::spiral_offsets` of the sphere radius",
    ),
    ("src/world.rs", "World", "decision_work", PER_SEAT),
    (
        "src/world.rs",
        "World",
        "damage_dealt",
        "`vec![0; seats * seats]` in World::new and at a restore, written by index",
    ),
    (
        "src/world.rs",
        "World",
        "route_spans",
        "`vec![(0, 0); unit_limit]` in World::new, one a unit row the table may ever hold, \
         written by index",
    ),
    (
        "src/world.rs",
        "World",
        "commanders",
        "`commander_index(units, seats)`: one a seat, at construction and at a restore",
    ),
    ("src/world.rs", "World", "unit_start_of", PER_SEAT),
    ("src/voxels.rs", "VoxelStore", "chunks", PER_MAP),
    ("src/voxels.rs", "VoxelStore", "modified", PER_MAP),
    ("src/voxels.rs", "VoxelStore", "pending_flag", PER_MAP),
    ("src/voxels.rs", "VoxelStore", "touch_flag", PER_MAP),
    ("src/chunks.rs", "ChunkDigests", "digests", PER_MAP),
    ("src/terrain.rs", "GeneratedSurface", "tops", PER_MAP),
    ("src/pathing/surface.rs", "Surface", "top", PER_MAP),
    ("src/pathing/surface.rs", "Surface", "walk", PER_MAP),
    (
        "src/bore.rs",
        "Bore",
        "columns",
        "the core's bore columns, generated with the map and regenerated at a restore",
    ),
    (
        "src/features.rs",
        "FeatureTable",
        "features",
        "every vent and seam, generated with the map and regenerated at a restore",
    ),
    (
        "src/features.rs",
        "Feature",
        "footprint",
        "the columns the generator stamped the feature into, at map generation",
    ),
    (
        "src/runner.rs",
        "MatchState",
        "segment_lengths_ms",
        "the match settings' segment ladder, set at construction and at a restore",
    ),
    (
        "src/interpreter/state.rs",
        "Interpreter",
        "plans",
        "one a seat (`Interpreter::with_seats`)",
    ),
    (
        "src/interpreter/state.rs",
        "Interpreter",
        "states",
        "one a seat (`Interpreter::with_seats`)",
    ),
    (
        "src/interpreter/state.rs",
        "PlanState",
        "fires",
        "one a handler of the sealed plan, sized at the seal and at a restore",
    ),
    (
        "src/interpreter/state.rs",
        "PlanState",
        "ready",
        "one a handler of the sealed plan, sized at the seal and at a restore",
    ),
    (
        "src/attack.rs",
        "BreachScratch",
        "mark",
        "`vec![0; 2 * nodes]` in BreachScratch::new, two states a surface node",
    ),
    (
        "src/attack.rs",
        "BreachScratch",
        "g",
        "`vec![0; 2 * nodes]` in BreachScratch::new, two states a surface node",
    ),
    (
        "src/attack.rs",
        "BreachScratch",
        "came",
        "`vec![0; 2 * nodes]` in BreachScratch::new, two states a surface node",
    ),
    (
        "src/support.rs",
        "Scratch",
        "victims",
        "the support scratch's written-out Clone reserves it afresh at its `victims_room`",
    ),
    ("src/support.rs", "SupportRule", "list", SUPPORT_RULE),
    ("src/support.rs", "SupportRule", "materials", SUPPORT_RULE),
    ("src/support.rs", "SupportRule", "regions", SUPPORT_RULE),
    ("src/support.rs", "SupportRule", "order", SUPPORT_RULE),
    ("src/support.rs", "SupportRule", "changed", SUPPORT_RULE),
    ("src/support.rs", "SupportRule", "seeds", SUPPORT_RULE),
    ("src/support.rs", "SupportRule", "members", SUPPORT_RULE),
    ("src/support.rs", "SupportRule", "landed", SUPPORT_RULE),
    (
        "src/support.rs",
        "SupportRule",
        "column_stamp",
        SUPPORT_RULE,
    ),
    (
        "src/support.rs",
        "SupportRule",
        "column_height",
        SUPPORT_RULE,
    ),
    (
        "src/support.rs",
        "Bits",
        "0",
        "`vec![0; room.words]` in SupportRule::with_room: a bit a voxel of the map",
    ),
    ("src/pathing/search.rs", "Scratch", "stamp", PER_NODE),
    ("src/pathing/search.rs", "Scratch", "closed", PER_NODE),
    ("src/pathing/search.rs", "Scratch", "g", PER_NODE),
    ("src/pathing/search.rs", "Scratch", "came", PER_NODE),
    (
        "src/pathing/search.rs",
        "Scratch",
        "abstract_stamp",
        PER_NODE,
    ),
    (
        "src/pathing/search.rs",
        "Scratch",
        "abstract_closed",
        PER_NODE,
    ),
    ("src/pathing/search.rs", "Scratch", "abstract_g", PER_NODE),
    (
        "src/pathing/search.rs",
        "Scratch",
        "abstract_came",
        PER_NODE,
    ),
    ("src/pathing/reach.rs", "Reach", "stamp", PER_NODE),
    ("src/pathing/reach.rs", "Reach", "settled", PER_NODE),
    ("src/pathing/reach.rs", "Reach", "cost", PER_NODE),
    ("src/pathing/reach.rs", "Reach", "key", PER_NODE),
    ("src/pathing/reach.rs", "Reach", "listed", PER_NODE),
    ("src/pathing/reach.rs", "Reach", "next", PER_NODE),
    ("src/pathing/reach.rs", "Reach", "prev", PER_NODE),
    ("src/pathing/reach.rs", "Reach", "sites", PER_NODE),
    ("src/pathing/reach.rs", "Reach", "heur", PER_NODE),
    (
        "src/pathing/reach.rs",
        "Reach",
        "heads",
        "`vec![NONE; buckets]` in Reach::new, the bucket ring's length from the dearest step",
    ),
    (
        "src/pathing/repair.rs",
        "Repairer",
        "dirty_flag",
        "`vec![false; clusters]` in Repairer::new, one a cluster",
    ),
    (
        "src/pathing/repair.rs",
        "Repairer",
        "transitions_flag",
        "`vec![false; clusters]` in Repairer::new, one a cluster",
    ),
    (
        "src/pathing/repair.rs",
        "Repairer",
        "inter_flag",
        "`vec![false; clusters]` in Repairer::new, one a cluster",
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "of_node",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "local_comp",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "comp_count",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "vborder_len",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "vborder",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "hborder_len",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "hborder",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "trans_len",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "trans_cell",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "intra_row",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "intra_to",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "intra_cost",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "inter_row",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "inter_to",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "inter_cost",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "comp_offset",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "comp_label",
        PER_CLUSTER,
    ),
    (
        "src/pathing/clusters.rs",
        "Clusters",
        "union_find",
        PER_CLUSTER,
    ),
];

/// **The type is the whole fix, so every vector the world owns is held to
/// it.** A cloned world (a `research` fork) or a restored one keeps a
/// vector's room only if the vector is a `Reserved`; the fork and restore
/// tests catch a plain one only where their scenarios fill it past its
/// length. This guard walks every type the world owns, from `World` through
/// the sim's sources ([`owned`]), and fails on a plain `Vec`, `VecDeque`,
/// `String` or `Box<[T]>` field that [`PLAIN_VECTORS`] does not list with its
/// bound, on a listed one that is gone, and on a field whose type it cannot
/// resolve. It runs in both builds.
#[test]
fn every_vector_the_world_owns_keeps_its_room_or_says_why_not() {
    let mut files = Vec::new();
    rust_sources(std::path::Path::new("src"), &mut files);
    let mut sources = owned::Sources::default();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("a sim source reads");
        owned::read(file, &text, &mut sources);
    }
    let terminal: Vec<&str> = TERMINAL.iter().map(|(name, _)| *name).collect();
    let walked = owned::walk(&sources, ("src/world.rs", "World"), &terminal);
    assert!(
        walked
            .types
            .iter()
            .any(|(file, name)| file == "src/world.rs" && name == "World"),
        "the walk starts at src/world.rs's World"
    );
    for (file, name) in [
        ("src/tables.rs", "UnitTable"),
        ("src/tables.rs", "ShellTable"),
        ("src/events.rs", "EventBus"),
        ("src/voxels.rs", "VoxelStore"),
        ("src/support.rs", "SupportRule"),
        ("src/pathing/router.rs", "Router"),
        ("src/pathing/search.rs", "Heap"),
        ("src/pathing/reach.rs", "Reach"),
        ("src/survey.rs", "Memory"),
        ("src/interpreter/state.rs", "PlanState"),
        ("src/licence.rs", "LicenceTable"),
    ] {
        assert!(
            walked.types.iter().any(|(f, n)| f == file && n == name),
            "the walk from World reaches {file}'s {name}"
        );
    }
    assert!(
        walked.unresolved.is_empty(),
        "fields naming a type the walker cannot resolve, (file, type, field, name): {:?}",
        walked.unresolved
    );
    let mut listed: Vec<(String, String, String)> = PLAIN_VECTORS
        .iter()
        .map(|(f, s, n, _)| ((*f).to_owned(), (*s).to_owned(), (*n).to_owned()))
        .collect();
    listed.sort();
    let unlisted: Vec<_> = walked
        .plain
        .iter()
        .filter(|p| !listed.contains(p))
        .collect();
    let stale: Vec<_> = listed
        .iter()
        .filter(|l| !walked.plain.contains(l))
        .collect();
    assert!(
        unlisted.is_empty(),
        "plain vectors the world owns, neither `Reserved` nor listed with their bound in \
         PLAIN_VECTORS: {unlisted:?}. A vector a tick fills past its length must be a \
         `Reserved`, or a cloned or restored world allocates on its first tick that fills it"
    );
    assert!(
        stale.is_empty(),
        "PLAIN_VECTORS lists fields the walk no longer finds: {stale:?}"
    );
    for (file, name, field, reason) in PLAIN_VECTORS {
        assert!(
            !reason.is_empty(),
            "{file} {name}.{field} says what bounds it"
        );
    }
    for (name, reason) in TERMINAL {
        assert!(!reason.is_empty(), "{name} says why it is not read");
    }
    // And a `Reserved` keeps only the room it was given (lane `simroom`):
    // the Defend protect list was a `Reserved` built from an empty `Vec`,
    // never reserved, so its clone and its restore kept a room of nothing
    // and a commit grew it inside a tick. Every `Reserved` field the walk
    // finds has its room stated in an impl of its own type.
    let unstated = owned::room_unstated(&sources, &walked);
    assert!(
        unstated.is_empty(),
        "`Reserved` fields the world owns whose room no impl of their type states (a struct \
         literal's `with_capacity`, or `reserve` or `reserve_total` on the field), \
         (file, type, field): {unstated:?}. A vector a tick fills needs a stated room, derived \
         from what bounds it, or its first push inside a tick allocates"
    );
    assert!(
        walked
            .reserved
            .iter()
            .any(|(file, ty, field)| file == "src/tables.rs"
                && ty == "DefendTable"
                && field == "protected"),
        "the room check reaches the Defend protect list"
    );
}

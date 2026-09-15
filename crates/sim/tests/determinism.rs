// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The determinism artefact itself: the hash chain, save/restore, and the
//! properties that keep a performance knob out of hashed state.
//!
//! This file also **produces** `<target>/golden/determinism/actual.hashes.txt`,
//! which is what `cargo xtask ci`'s `golden` step compares against the
//! committed `tests/golden/determinism/expected.hashes.txt`. The `test` step
//! runs before `golden`, so the fresh output is always there by the time the
//! comparison happens.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules — not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::{Path, PathBuf};

use pharmakos_proto::gp;
use pharmakos_sim::encoding::{Enc, hex};
use pharmakos_sim::knowledge::{AssetId, AssetKind, Position, SeatKnowledge, Sighting};
use pharmakos_sim::math::fixed::{Fx, Sq};
use pharmakos_sim::math::quantity::{Ms, Tick};
use pharmakos_sim::snapshot::{SNAPSHOT_VERSION, Snapshot, SnapshotError};
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::world::{PHASE_ORDER, Phase};
use pharmakos_sim::{RulesError, RulesTable, World, WorldConfig};

/// The tick count `cargo xtask ci` runs the determinism binary at.
///
/// PLACEHOLDER: raised from 1 200 once the real sim carries a segment (owner,
/// at T20). Keep it in step with `DETERMINISM_TICKS` in `xtask/src/main.rs`.
const GOLDEN_TICKS: u32 = 1_200;

/// The repository root, found by probing relative paths.
fn repo_root() -> PathBuf {
    let here = PathBuf::from(".");
    if here.join("rules").join("rules.v1.json").is_file() {
        return here;
    }
    PathBuf::from("..").join("..")
}

/// The cargo target directory: `<target>/<profile>/deps/<test exe>`.
fn target_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let deps = exe.parent()?;
    let profile = deps.parent()?;
    Some(profile.parent()?.to_path_buf())
}

fn rules() -> RulesTable {
    RulesTable::load(&repo_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table loads")
}

/// The committed table with one edit made to the **message**.
///
/// A `RulesTable` has no public fields, because a field mutated after
/// construction could disagree with the message `rules_hash` is taken over. So
/// a test that wants a different value edits the schema message and re-derives
/// the view, which is the same path a tuning pull request takes.
fn rules_edited(edit: impl FnOnce(&mut gp::v1::RulesTable)) -> RulesTable {
    let text = std::fs::read_to_string(repo_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table exists");
    let mut message: gp::v1::RulesTable =
        pharmakos_proto::json::decode(&text).expect("the committed table is canonical gp.v1 JSON");
    edit(&mut message);
    RulesTable::from_message(&message).expect("the edited table is still a table")
}

fn world_with(rules: RulesTable) -> World {
    world_of(rules, pharmakos_sim::DETERMINISM_UNITS_PER_SEAT)
}

/// The harness world at a chosen size, for the tests that need two worlds of
/// different shapes.
fn world_of(rules: RulesTable, units_per_seat: u32) -> World {
    World::new(&WorldConfig {
        match_seed: pharmakos_sim::DETERMINISM_MATCH_SEED,
        seats: pharmakos_sim::DETERMINISM_SEATS,
        units_per_seat,
        chunk_count: pharmakos_sim::chunks::SKELETON_CHUNK_COUNT,
        rules,
    })
    .expect("the rules table describes a broadphase grid")
}

/// The chain, in the file format `xtask` validates: `tick<TAB>hash\n`.
fn chain(world: &mut World, ticks: u32) -> String {
    let mut enc = Enc::with_capacity(64 * 1024);
    let mut out = String::with_capacity(usize::try_from(ticks).unwrap_or(0).saturating_mul(24));
    for tick in 0..ticks {
        let hash = if tick == 0 {
            world.encode(&mut enc);
            enc.finish()
        } else {
            world.step(&mut enc)
        };
        out.push_str(&tick.to_string());
        out.push('\t');
        out.push_str(&hex(hash));
        out.push('\n');
    }
    out
}

#[test]
fn the_hash_chain_matches_its_golden() {
    let fresh = chain(&mut world_with(rules()), GOLDEN_TICKS);

    // Hand the fresh output to `cargo xtask ci`'s golden step before asserting,
    // so a failure here still leaves it a readable first-difference report.
    if let Some(target) = target_dir() {
        let out = target.join("golden").join("determinism");
        std::fs::create_dir_all(&out).expect("creating the golden output directory");
        std::fs::write(out.join("actual.hashes.txt"), fresh.as_bytes())
            .expect("writing actual.hashes.txt");
    }

    let golden_path = repo_root()
        .join("tests")
        .join("golden")
        .join("determinism")
        .join("expected.hashes.txt");
    let golden = std::fs::read_to_string(&golden_path).expect("the committed hash chain exists");

    if golden != fresh {
        let first = golden
            .lines()
            .zip(fresh.lines())
            .find(|(a, b)| a != b)
            .map_or_else(
                || "the chains differ in length".to_owned(),
                |(a, b)| format!("expected `{a}`, found `{b}`"),
            );
        panic!(
            "the hash chain moved: {} differs from this run.\n  {first}\n  A moved hash chain is \
             a behaviour change. Explain it in the pull request; never re-bless it to get a red \
             build to green.",
            golden_path.display()
        );
    }
}

#[test]
fn the_golden_has_no_carriage_return_and_ends_with_a_newline() {
    let golden_path = repo_root()
        .join("tests")
        .join("golden")
        .join("determinism")
        .join("expected.hashes.txt");
    let bytes = std::fs::read(&golden_path).expect("the committed hash chain exists");
    assert!(
        !bytes.contains(&b'\r'),
        "a carriage return reached the hash chain; the three operating systems must produce \
         byte-identical files (.gitattributes marks *.hashes.txt as -text)"
    );
    assert_eq!(bytes.last().copied(), Some(b'\n'));
    let lines = bytes.split(|b| *b == b'\n').count().saturating_sub(1);
    assert_eq!(u32::try_from(lines).unwrap_or(0), GOLDEN_TICKS);
}

#[test]
fn the_binary_reproduces_the_library_in_a_fresh_process() {
    // The chain must not depend on anything a process brings with it — an
    // address-space layout, a hash seed, an allocator's mood. Two fresh
    // processes and the in-process run all produce the same bytes.
    let Some(target) = target_dir() else {
        panic!("could not find the cargo target directory");
    };
    let rules_path = repo_root().join("rules").join("rules.v1.json");
    let mut runs: Vec<Vec<u8>> = Vec::new();
    for attempt in 0..2 {
        let out = target.join(format!("determinism-crosscheck-{attempt}.hashes.txt"));
        let status = std::process::Command::new(env!("CARGO_BIN_EXE_determinism"))
            .arg("--ticks")
            .arg("200")
            .arg("--out")
            .arg(&out)
            .arg("--rules")
            .arg(&rules_path)
            .status()
            .expect("running the determinism binary");
        assert!(status.success(), "the determinism binary failed");
        runs.push(std::fs::read(&out).expect("reading the binary's chain"));
    }
    assert_eq!(
        runs.first(),
        runs.get(1),
        "two runs of the determinism binary disagree"
    );
    let in_process = chain(&mut world_with(rules()), 200);
    assert_eq!(
        runs.first().map(Vec::as_slice),
        Some(in_process.as_bytes()),
        "the binary and the library disagree about the same 200 ticks"
    );
}

#[test]
fn save_and_restore_round_trips_hash_identically() {
    // Fifty round-trips at fifty different points in the run, each one
    // asserting that the restored world hashes to exactly what the saved one
    // did and then continues to the same next hash.
    let mut world = world_with(rules());
    let mut enc = Enc::with_capacity(64 * 1024);
    for round in 0..50 {
        for _ in 0..10 {
            let _ = world.step(&mut enc);
        }
        let before = world.state_hash();
        let bytes = Snapshot::capture(&world)
            .to_bytes()
            .expect("encoding a snapshot");
        let decoded = Snapshot::from_bytes(&bytes).expect("decoding a snapshot");

        let mut restored = world_with(rules());
        decoded
            .restore_into(&mut restored)
            .expect("restoring a snapshot");
        assert_eq!(
            restored.state_hash(),
            before,
            "round {round}: the restored world does not hash to the saved one"
        );

        // And it must keep agreeing: a restore that is right for one tick and
        // wrong for the next is the desync this test exists to catch.
        let mut continued = world.clone();
        assert_eq!(
            restored.step(&mut enc),
            continued.step(&mut enc),
            "round {round}: the restored world diverges on the next tick"
        );
        assert_eq!(
            Snapshot::capture(&world).to_bytes().expect("re-encoding"),
            bytes,
            "round {round}: capture is not stable"
        );
    }
}

#[test]
fn a_snapshot_written_by_one_process_continues_the_chain_in_another() {
    // The other half of T2's acceptance line. The test above round-trips fifty
    // snapshots inside one process; a save file's actual job is to be written
    // by one process and read by a different one, so this spawns two: the first
    // runs 400 ticks and drops a snapshot at tick 200, the second starts from
    // that file and must produce the first run's remaining 200 lines exactly.
    let Some(target) = target_dir() else {
        panic!("could not find the cargo target directory");
    };
    let rules_path = repo_root().join("rules").join("rules.v1.json");
    let reference_path = target.join("determinism-resume-reference.hashes.txt");
    let resumed_path = target.join("determinism-resume-continued.hashes.txt");
    let snapshot_path = target.join("determinism-resume.snapshot.postcard");

    let first = std::process::Command::new(env!("CARGO_BIN_EXE_determinism"))
        .args(["--ticks", "400", "--save-at", "200"])
        .arg("--out")
        .arg(&reference_path)
        .arg("--snapshot")
        .arg(&snapshot_path)
        .arg("--rules")
        .arg(&rules_path)
        .status()
        .expect("running the determinism binary");
    assert!(first.success(), "the first process failed");

    let second = std::process::Command::new(env!("CARGO_BIN_EXE_determinism"))
        .args(["--ticks", "200"])
        .arg("--out")
        .arg(&resumed_path)
        .arg("--resume")
        .arg(&snapshot_path)
        .arg("--rules")
        .arg(&rules_path)
        .status()
        .expect("running the determinism binary");
    assert!(second.success(), "the resuming process failed");

    let reference = std::fs::read_to_string(&reference_path).expect("reading the first chain");
    let resumed = std::fs::read_to_string(&resumed_path).expect("reading the resumed chain");

    let tail: Vec<&str> = reference.lines().skip(200).collect();
    let continued: Vec<&str> = resumed.lines().collect();
    assert_eq!(tail.len(), 200, "the reference run is the wrong length");
    assert_eq!(
        continued.first().map(|l| l.split('\t').next()),
        Some(Some("200")),
        "a resumed run numbers its lines from the snapshot's own tick"
    );
    assert_eq!(
        continued, tail,
        "a snapshot written by one process does not continue the chain in another; that is the \
         desync a save file would ship to a player"
    );
}

#[test]
fn a_restore_resizes_the_derived_index_to_the_restored_world() {
    // `World::restore_tables` replaces the unit table wholesale. The broadphase
    // is sized for the unit count the *receiving* world was built for, so a
    // snapshot of a larger world used to restore cleanly, hash correctly, and
    // then query an empty index for the rest of the match. Restoring 200 units
    // into a world built for 4 is that case, made small.
    let mut big = world_with(rules());
    let mut enc = Enc::with_capacity(64 * 1024);
    for _ in 0..20 {
        let _ = big.step(&mut enc);
    }
    let expected_units = big.units().len();

    let mut small = world_of(rules(), 1);
    assert!(small.units().len() < expected_units);
    Snapshot::capture(&big)
        .restore_into(&mut small)
        .expect("restoring a larger world");
    assert_eq!(small.state_hash(), big.state_hash());

    // One more tick, then ask the index for everything: a grid that was not
    // resized answers with nothing at all.
    assert_eq!(small.step(&mut enc), big.step(&mut enc));
    let centre = [Fx::from_voxels(192), Fx::from_voxels(192), Fx::ZERO];
    let found = small.candidates_near(centre, Fx::from_voxels(1_024)).len();
    assert_eq!(
        u32::try_from(found).unwrap_or(0),
        expected_units,
        "the broadphase did not grow with the restored unit table"
    );
}

#[test]
fn the_work_counter_is_not_in_the_state_encoding() {
    // The counter is a seam with no teeth yet (see `seams::WorkCounter`), and
    // a counter that affects nothing must not be able to move a golden file.
    let mut world = world_with(rules());
    let mut before = Enc::with_capacity(64 * 1024);
    world.encode(&mut before);
    let before_bytes = before.as_bytes().to_vec();
    let before_hash = world.state_hash();

    world
        .work_mut()
        .charge(1_234)
        .expect("the harness budget covers this charge");
    assert_eq!(world.work().spent(), 1_234, "the charge did not land");

    let mut after = Enc::with_capacity(64 * 1024);
    world.encode(&mut after);
    assert_eq!(
        after.as_bytes(),
        before_bytes.as_slice(),
        "the work counter reached the canonical encoding"
    );
    assert_eq!(world.state_hash(), before_hash);
}

#[test]
fn the_tables_move_every_tick_and_not_merely_the_header() {
    // A chain of 1 200 distinct hashes is not evidence that the world moves:
    // `World::encode` writes the tick into the header, so the hashes would all
    // differ even with an empty movement phase. What is load-bearing is that
    // the encoding *after* the header differs from tick to tick.
    const HEADER_BYTES: usize = 1 + 8 + 4; // version byte, match seed, tick

    let mut world = world_with(rules());
    let mut enc = Enc::with_capacity(64 * 1024);
    let mut previous: Option<Vec<u8>> = None;
    let mut moved = 0_u32;
    for _ in 0..200 {
        let _ = world.step(&mut enc);
        let body = enc
            .as_bytes()
            .get(HEADER_BYTES..)
            .unwrap_or_default()
            .to_vec();
        if let Some(before) = previous {
            if before != body {
                moved = moved.saturating_add(1);
            }
        }
        previous = Some(body);
    }
    assert_eq!(
        moved, 199,
        "the hashed tables did not change on every tick; the chain's distinctness comes from the \
         tick in the header, not from the world"
    );
}

#[test]
fn the_sighting_order_is_total() {
    // Item 62 at the type that hands a list to an operator: two sightings that
    // agree on owner, kind and place must still have exactly one order, or two
    // machines can iterate them differently and the intents they produce
    // diverge. The list is also one record per asset, which is what makes the
    // key's last field unique.
    let seat = SeatId::new(0);
    let at = Position {
        x: Fx::from_voxels(12),
        y: Fx::from_voxels(34),
        z: Fx::ZERO,
    };
    let collide = |id: u32, age: i32| Sighting {
        id: AssetId::new(id),
        owner: seat,
        kind: AssetKind::Unit,
        at,
        age: Ms::new(age),
    };

    let mut forwards = SeatKnowledge::new(seat, Tick::ZERO);
    for id in [7_u32, 3, 9, 1] {
        forwards.push_sighting(collide(id, 100));
    }
    let mut backwards = SeatKnowledge::new(seat, Tick::ZERO);
    for id in [1_u32, 9, 3, 7] {
        backwards.push_sighting(collide(id, 100));
    }
    assert_eq!(
        forwards.sightings(),
        backwards.sightings(),
        "the order of a colliding sighting key depends on insertion order"
    );
    let ids: Vec<u32> = forwards.sightings().iter().map(|s| s.id.raw()).collect();
    assert_eq!(ids, vec![1, 3, 7, 9], "the key must end in the asset id");

    // A second sighting of the same asset is a refresh, not a second record:
    // that is the invariant the key's uniqueness rests on.
    forwards.push_sighting(collide(3, 0));
    assert_eq!(forwards.sightings().len(), 4, "a refresh added a record");
    assert_eq!(
        forwards
            .sightings()
            .iter()
            .find(|s| s.id == AssetId::new(3))
            .map(|s| s.age),
        Some(Ms::new(0)),
        "a refresh did not replace the remembered sighting"
    );
}

#[test]
fn a_snapshot_from_another_version_refuses_to_load() {
    let world = world_with(rules());
    let mut snapshot = Snapshot::capture(&world);
    snapshot.version = SNAPSHOT_VERSION + 1;
    let bytes = snapshot.to_bytes().expect("encoding a snapshot");
    match Snapshot::from_bytes(&bytes) {
        Err(SnapshotError::Version { found, expected }) => {
            assert_eq!(found, SNAPSHOT_VERSION + 1);
            assert_eq!(expected, SNAPSHOT_VERSION);
        }
        other => panic!("a snapshot from another version loaded: {other:?}"),
    }
}

#[test]
fn a_ragged_snapshot_is_refused_rather_than_half_applied() {
    let world = world_with(rules());
    let mut snapshot = Snapshot::capture(&world);
    snapshot.unit_pos.pop();
    let mut into = world_with(rules());
    let before = into.state_hash();
    assert!(matches!(
        snapshot.restore_into(&mut into),
        Err(SnapshotError::Ragged(_))
    ));
    assert_eq!(into.state_hash(), before, "a refused restore changed state");
}

#[test]
fn a_performance_knob_is_not_hashed_state() {
    // G3′ §9.17: keep the calibration constant outside hashed state, and *test*
    // that it is. The cell size, the repath cap and the mesher's drain budget
    // are performance knobs; changing one must leave the chain byte-identical.
    let base = rules();
    let reference = chain(&mut world_with(base.clone()), 200);

    for cell_size in [4_u32, 8, 16, 32, 64] {
        let altered = rules_edited(|message| {
            message.broadphase.get_or_insert_default().cell_size_voxels = cell_size;
            let locomotion = message.locomotion.get_or_insert_default();
            locomotion.repath_cap_per_tick =
                locomotion.repath_cap_per_tick.saturating_add(cell_size);
            let mesher = message.mesher.get_or_insert_default();
            mesher.surfaces_per_frame = mesher.surfaces_per_frame.saturating_add(4);
            mesher.bytes_per_frame = mesher.bytes_per_frame.saturating_sub(1);
        });
        assert_ne!(
            altered.rules_hash(),
            base.rules_hash(),
            "the altered table must be a different table"
        );
        assert_eq!(
            chain(&mut world_with(altered), 200),
            reference,
            "the hash chain moved when only a performance knob changed (cell size {cell_size})"
        );
    }
}

#[test]
fn the_rules_table_is_not_in_the_state_encoding() {
    let base = rules();
    let altered = rules_edited(|message| {
        message.broadphase.get_or_insert_default().cell_size_voxels = 64;
        message
            .locomotion
            .get_or_insert_default()
            .repath_cap_per_tick = 99;
        // A row the sim never reads, to show the encoding is blind to those
        // too: `rules_hash` moves, the state encoding does not.
        message
            .interface_times
            .get_or_insert_default()
            .place_beacon_deploy_ms = 999;
    });
    assert_ne!(
        altered.rules_hash(),
        base.rules_hash(),
        "a row the sim does not read must still move `rules_hash` (it is one of \
         the verifier's `report_hash` inputs)"
    );

    let mut a = Enc::with_capacity(64 * 1024);
    world_with(base).encode(&mut a);
    let mut b = Enc::with_capacity(64 * 1024);
    world_with(altered).encode(&mut b);
    assert_eq!(
        a.as_bytes(),
        b.as_bytes(),
        "the rules table reached the canonical encoding; it is an input to the sim, not state"
    );
}

#[test]
fn the_broadphase_answer_does_not_depend_on_the_cell_size() {
    // The radius is in voxels and the range test is exact, so three grids of
    // different cell edge have to agree on the answer's *membership*, not only
    // on its order.
    //
    // The radius is deliberately SMALL. An earlier version of this test asked
    // for 64 cells, which covers the whole 384 x 384 map at every cell size, so
    // all three runs returned every unit and the test passed without touching
    // the property it is named for. This radius is smaller than the largest
    // cell edge under test, which is exactly the query a radius-in-cells
    // broadphase gets wrong.
    let radius = Fx::from_voxels(48);
    let whole_map = Fx::from_voxels(1_024);
    let centre = [Fx::from_voxels(192), Fx::from_voxels(192), Fx::ZERO];

    let mut answers: Vec<Vec<u32>> = Vec::new();
    let mut everything: Vec<usize> = Vec::new();
    for cell_size in [4_u32, 16, 64] {
        let altered = rules_edited(|message| {
            message.broadphase.get_or_insert_default().cell_size_voxels = cell_size;
        });
        let mut world = world_with(altered);
        let mut enc = Enc::with_capacity(64 * 1024);
        for _ in 0..20 {
            let _ = world.step(&mut enc);
        }
        answers.push(world.candidates_near(centre, radius).to_vec());
        everything.push(world.candidates_near(centre, whole_map).len());
    }

    let first = answers.first().cloned().unwrap_or_default();
    assert!(
        !first.is_empty(),
        "the broadphase found nothing at all within {} voxels of the middle of \
         the map; the test cannot see the property it is checking",
        radius.floor_voxels()
    );
    let all = everything.first().copied().unwrap_or(0);
    assert!(
        first.len() < all,
        "the small query returned every unit ({} of {}), so it is not a small \
         query and this test is vacuous",
        first.len(),
        all
    );
    for (index, answer) in answers.iter().enumerate() {
        assert_eq!(
            answer, &first,
            "the broadphase's answer changed with the cell size (run {index})"
        );
    }
    for (index, count) in everything.iter().enumerate() {
        assert_eq!(
            *count, all,
            "a map-wide query changed with the cell size (run {index})"
        );
    }

    let mut sorted = first.clone();
    sorted.sort_unstable();
    assert_eq!(first, sorted, "candidates must come back in id order");

    // And the exact test is the thing doing the work: every id in the answer is
    // genuinely within the radius, and every id outside it is genuinely not.
    let mut world = world_with(rules());
    let mut enc = Enc::with_capacity(64 * 1024);
    for _ in 0..20 {
        let _ = world.step(&mut enc);
    }
    let near = world.candidates_near(centre, radius).to_vec();
    let positions: Vec<[Fx; 3]> = world.units().positions().to_vec();
    let limit = Sq::of_radius(radius);
    for (id, pos) in positions.iter().enumerate() {
        let id = u32::try_from(id).unwrap();
        assert_eq!(
            near.binary_search(&id).is_ok(),
            Sq::between(*pos, centre) <= limit,
            "unit {id} is on the wrong side of the radius"
        );
    }
}

#[test]
fn the_phase_order_is_the_documented_one() {
    // The tick's shape is determinism code (AGENTS.md §5). Pinning it here
    // means reordering a phase fails a test rather than quietly moving a
    // golden file that someone then re-blesses.
    assert_eq!(
        PHASE_ORDER,
        [
            Phase::Broadphase,
            Phase::Programs,
            Phase::Movement,
            Phase::Combat,
            Phase::KillCredit,
            Phase::Power,
            Phase::Quartermaster,
            Phase::Decision,
            Phase::Pathing,
            Phase::Voxels,
            Phase::Hash,
        ]
    );
}

#[test]
fn a_tick_advances_the_clock_by_exactly_one() {
    let mut world = world_with(rules());
    let mut enc = Enc::with_capacity(64 * 1024);
    assert_eq!(world.tick(), Tick::ZERO);
    for expected in 1..=10 {
        let _ = world.step(&mut enc);
        assert_eq!(world.tick(), Tick::new(expected));
    }
}

#[test]
fn the_chunk_digests_are_in_the_hash() {
    // The chunk store's seam has to be load-bearing from the day it exists,
    // or T5 will discover at the worst moment that voxels are not hashed.
    let mut world = world_with(rules());
    let before = world.state_hash();
    assert!(world.chunks_mut().refresh(7, 0xDEAD_BEEF));
    assert_ne!(
        world.state_hash(),
        before,
        "a refreshed chunk digest did not move the state hash"
    );
    assert!(
        !world.chunks_mut().refresh(u32::MAX, 1),
        "an off-map chunk index must be refused"
    );
}

#[test]
fn the_rules_hash_is_pinned_to_the_committed_table() {
    // One of the verifier's five `report_hash` inputs, and stamped into every
    // save. If this moves, either `rules/rules.v1.json` moved or
    // `RulesTable::encode` did, and the pull request owes the explanation.
    //
    // It moved once, from 2b9f1bd9f5a54425, when revision 3 of the table
    // landed decisions-log items 90, 91 and 94: the per-kind walking speeds,
    // the decision tick, the whole economy and power blocks, and the new
    // commander, beacon, map, units, structures and verifier blocks. The table
    // is an INPUT, never hashed state, so the per-tick chain in
    // tests/golden/determinism did not move with it — and
    // `the_rules_table_is_not_in_the_state_encoding` below is what makes that
    // a fact rather than a hope.
    assert_eq!(
        hex(rules().rules_hash()),
        "34c3f2152cb440e5",
        "the rules hash moved; say in the pull request which row changed and why"
    );
}

#[test]
fn the_rules_reader_rejects_what_it_should() {
    // The reader is `pharmakos-proto`'s canonical JSON codec (item 74), not a
    // parser of the sim's own. What is checked here is that the sim inherits
    // the codec's strictness rather than softening it on the way through.
    let good = std::fs::read_to_string(repo_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table exists");
    assert!(RulesTable::from_canonical_json(&good).is_ok());

    // An unknown field is rejected, never ignored.
    let with_unknown = good.replace("\"step_cost_cardinal\"", "\"step_cost_cardinalish\"");
    assert!(
        matches!(
            RulesTable::from_canonical_json(&with_unknown),
            Err(RulesError::Json(_))
        ),
        "an unknown field was accepted"
    );
    // A float is not a uint32, and the sim has no floats.
    let with_float = good.replace("\"step_cost_cardinal\": 10", "\"step_cost_cardinal\": 10.5");
    assert!(RulesTable::from_canonical_json(&with_float).is_err());
    // Trailing content is not canonical JSON.
    let trailing = format!("{good}{{}}");
    assert!(RulesTable::from_canonical_json(&trailing).is_err());

    // A block the sim reads is required. Proto3 cannot tell an absent message
    // from an empty one, so a table with no `locomotion` has to be refused:
    // accepting it would hand the sim a locomotion table of zeroes and a unit
    // that never moves.
    assert_eq!(
        RulesTable::from_canonical_json("{}"),
        Err(RulesError::MissingBlock("locomotion")),
        "a table with no locomotion block was accepted"
    );

    // A value the schema allows but the sim's own type does not.
    let too_wide = good.replace(
        "\"step_cost_cardinal\": 10",
        "\"step_cost_cardinal\": 4000000000",
    );
    assert!(matches!(
        RulesTable::from_canonical_json(&too_wide),
        Err(RulesError::OutOfRange { .. })
    ));
}

#[test]
fn the_sim_reads_the_rows_the_schema_puts_them_in() {
    // The reconciliation this module's doc comment promised for T1's merge:
    // one `gp.v1.RulesTable`, decoded once by the one codec, with the sim's
    // flat view derived from it and from nothing else. If a row moves in the
    // schema this fails here, rather than quietly reading a zero.
    let text = std::fs::read_to_string(repo_root().join("rules").join("rules.v1.json"))
        .expect("the committed rules table exists");
    let message: gp::v1::RulesTable =
        pharmakos_proto::json::decode(&text).expect("the committed table is canonical gp.v1 JSON");
    let view = RulesTable::from_message(&message).expect("the sim's view of it");

    let locomotion = message.locomotion.expect("the locomotion block");
    let broadphase = message.broadphase.expect("the broadphase block");
    let mesher = message.mesher.expect("the mesher block");
    let matched = message.r#match.expect("the match block");

    assert_eq!(view.mesher_drain_surfaces(), mesher.surfaces_per_frame);
    assert_eq!(view.mesher_drain_bytes(), mesher.bytes_per_frame);
    assert_eq!(
        u32::try_from(view.step_cardinal()).unwrap(),
        locomotion.step_cost_cardinal
    );
    assert_eq!(
        u32::try_from(view.step_diagonal()).unwrap(),
        locomotion.step_cost_diagonal
    );
    assert_eq!(
        u32::try_from(view.climb_surcharge()).unwrap(),
        locomotion.climb_surcharge
    );
    assert_eq!(
        u32::try_from(view.move_cost_per_tick()).unwrap(),
        locomotion.move_cost_per_tick
    );
    assert_eq!(view.repath_cap_per_tick(), locomotion.repath_cap_per_tick);
    assert_eq!(
        u32::try_from(view.csr_cell_size_voxels()).unwrap(),
        broadphase.cell_size_voxels
    );
    assert_eq!(
        view.segment_lengths_ms(),
        matched.segment_lengths_ms.as_slice()
    );

    // And the table `load` returns is the same table, so the file reader and
    // the codec cannot drift apart.
    assert_eq!(view, rules());
}

#[test]
fn the_committed_rules_table_carries_the_decided_values() {
    let table = rules();
    assert_eq!(table.step_cardinal(), 10, "item 59");
    assert_eq!(table.step_diagonal(), 14, "item 59");
    assert_eq!(table.climb_surcharge(), 4, "item 59");
    assert_eq!(table.move_cost_per_tick(), 3, "item 59");
    assert_eq!(table.repath_cap_per_tick(), 16, "item 69");
    assert_eq!(
        table.csr_cell_size_voxels(),
        16,
        "item 67: the cell edge spike G3′ measured with; still a PLACEHOLDER"
    );
    assert_eq!(table.mesher_drain_surfaces(), 4, "item 54, K");
    assert_eq!(table.mesher_drain_bytes(), 512 * 1024, "item 54, B");
    assert_eq!(
        table.segment_lengths_ms(),
        [180_000, 300_000, 480_000],
        "item 68: the 3 / 5 / 8 ladder"
    );
}

#[test]
fn the_default_rules_path_is_relative() {
    let path: Option<PathBuf> = pharmakos_sim::default_rules_path();
    let found = path.expect("rules/rules.v1.json is reachable from the crate directory");
    assert!(
        found.is_relative(),
        "the rules path must not be absolute: nothing may bake a build machine's path in"
    );
    assert!(Path::new(&found).is_file());
}

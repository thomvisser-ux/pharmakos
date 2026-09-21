// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **Paths A and B produce identical geometry digests on the same chunk set** — skeleton
//! plan T12's acceptance, and decisions-log item 53's "geometry is byte-identical" turned
//! from a spike measurement into a check that runs on every CI leg.
//!
//! # Why this can run with no engine
//!
//! `crate::upload` decides what to do with a chunk and returns an `UploadOp`; the engine
//! executes it through `RenderingServer` or `ArrayMesh`, and `ResidentSurface` executes
//! it as plain data. One decision path, two executors. So this file drives the real
//! decision code over real meshes from the real mesher, and compares what the engine
//! would then be holding — on Windows, Linux and macOS alike, with no GPU and no Godot.
//!
//! # What would actually differ, if anything did
//!
//! Not the vertices: both paths send the same `MeshBuffers`. The two places a difference
//! could appear are the two item 53 names:
//!
//! * **the colour quantisation.** A create or a rebuild hands the engine floats and lets
//!   it quantise; a patch writes the bytes itself. If those disagreed by one
//!   least-significant bit, a chunk's shade would depend on which branch its last upload
//!   took, and nothing in a frame time would ever show it;
//! * **the index array.** A patch writes the vertex and attribute regions and leaves the
//!   index buffer alone. If the guard let a patch through when the indices had changed,
//!   path B would draw path A's geometry with the previous winding — back-face-culled
//!   holes. `an_index_array_change_is_rebuilt_on_both_paths_and_still_matches` is exactly
//!   that case, with equal counts and different indices.
//!
//! The digest is **xxh3-64**, the project's one hash function (AGENTS.md section 3's
//! approved list; the sim's state hash and the mesher's geometry golden are the same), as
//! a dev dependency so the shipped bridge carries no hash at all.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use pharmakos_client_gdext::surface::{ColourQuantisation, Surface, SurfaceProbe};
use pharmakos_client_gdext::upload::{ResidentSurface, UploadPath, Uploader};
use pharmakos_mesher::{
    AIR, CHUNK_EDGE, CHUNK_VOLUME, ChunkInput, LightParams, MeshBuffers, Mesher, voxel_index,
};
use xxhash_rust::xxh3::xxh3_64;

/// Chunks in the set.
const CASES: usize = 6;

/// The passes each chunk goes through, and what each one is for.
///
/// | pass | the chunk | path A | path B |
/// |---|---|---|---|
/// | 0 | the floor | create | create |
/// | 1 | a crater in it | rebuild | rebuild, vertex count moved |
/// | 2 | **the same crater again** | rebuild | **patch** |
/// | 3 | the crater, re-tinted | rebuild | rebuild |
///
/// A patch needs two CONSECUTIVE meshes that agree, which is why pass 2 repeats pass 1
/// rather than returning to pass 0. The fourth branch — an equal vertex count with a
/// changed index array — does not fall out of a four-pass set of real chunks, so it has
/// a test of its own below, built from a real surface with its winding flipped.
const PASSES: usize = 4;

/// A probe that says the surface layout is the one an in-place write assumes. Standing in
/// for the engine's read-back, so that path B's fast branch is reachable here.
fn probed() -> SurfaceProbe {
    SurfaceProbe {
        vertex_stride: Some(pharmakos_client_gdext::surface::VERTEX_STRIDE),
        attribute_stride: Some(pharmakos_client_gdext::surface::ATTRIBUTE_STRIDE),
        quantisation: Some(ColourQuantisation::RoundHalfUp),
    }
}

/// Builds one chunk, in the mesher's own voxel order.
///
/// `pass` decides the shape: 0 is a stepped floor, 1 and 2 are the same floor with a
/// crater in it — identical to each other, which is what lets path B patch — and 3 is
/// that crater with the materials rotated, so the mask key changes and the greedy merge
/// splits the quads differently.
fn materials(case: usize, pass: usize) -> Vec<u8> {
    let mut out = vec![AIR; CHUNK_VOLUME];
    let base = 8 + case % 5;
    for y in 0..CHUNK_EDGE {
        for z in 0..CHUNK_EDGE {
            for x in 0..CHUNK_EDGE {
                // y is up in the mesher's axes. `checked_div` rather than `/`:
                // `integer_division` stays denied inside a walled crate too, because
                // being explicit about rounding is not something the wall lifts
                // (AGENTS.md section 4.9).
                let step = x.checked_div(8).unwrap_or(0) + z.checked_div(8).unwrap_or(0);
                let height = base + step % 3;
                let cratered = pass >= 1
                    && (10..20).contains(&x)
                    && (10..20).contains(&z)
                    && y.saturating_add(3) >= height;
                if y < height && !cratered {
                    let material = if pass == 3 {
                        // The same solid set, different material ids: the mask key is
                        // (material, light), so the greedy merge splits the quads
                        // differently and the surface is a genuine remesh rather than the
                        // same bytes a third time.
                        let tint =
                            x.checked_div(4).unwrap_or(0) + y + z.checked_div(4).unwrap_or(0);
                        u8::try_from(1 + (tint % 3)).unwrap_or(1)
                    } else {
                        u8::try_from(1 + (case % 3)).unwrap_or(1)
                    };
                    if let Some(slot) = out.get_mut(voxel_index(x, y, z)) {
                        *slot = material;
                    }
                }
            }
        }
    }
    out
}

fn mesh(mesher: &mut Mesher, buffers: &mut MeshBuffers, case: usize, pass: usize) -> Surface {
    let input = ChunkInput::new(
        materials(case, pass),
        vec![15_u8; CHUNK_VOLUME],
        ChunkInput::no_borders(),
    )
    .expect("a full chunk and no borders");
    mesher
        .mesh_chunk_into(&input.as_view(), buffers)
        .expect("under the 16-bit index cap");
    Surface::from_mesh(buffers).expect("under the cap")
}

/// The digest of everything the engine would be holding for one chunk.
///
/// Positions by their `f32` **bit patterns**, not their printed values: two operating
/// systems must agree on the bits, and a digest over formatted decimals would hide a
/// one-ulp difference behind rounding. Colours as the bytes they are, indices as
/// little-endian `u16`, and the visibility flag, because a hidden surface and a drawn one
/// are not the same state.
fn digest(resident: &ResidentSurface) -> u64 {
    let mut bytes: Vec<u8> = Vec::new();
    for position in &resident.positions {
        for axis in position {
            bytes.extend_from_slice(&axis.to_bits().to_le_bytes());
        }
    }
    for colour in &resident.colours {
        bytes.extend_from_slice(colour);
    }
    for index in &resident.indices {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes.push(u8::from(resident.visible));
    xxh3_64(&bytes)
}

/// Drives the whole chunk set through one path and returns the per-chunk digests plus the
/// uploader's counters.
fn run(path: UploadPath) -> (Vec<u64>, pharmakos_client_gdext::upload::UploadCounters) {
    let mut mesher = Mesher::new(LightParams::new(15, 1).expect("legal"));
    let mut buffers = MeshBuffers::empty();
    let mut uploader = Uploader::new(path, CASES);
    uploader.record_probe(probed());
    let mut resident = vec![ResidentSurface::default(); CASES];

    for pass in 0..PASSES {
        for case in 0..CASES {
            let surface = mesh(&mut mesher, &mut buffers, case, pass);
            let op = uploader.plan(case, &surface).expect("in range");
            resident
                .get_mut(case)
                .expect("in range")
                .apply(&op, ColourQuantisation::RoundHalfUp)
                .expect("the op fits the resident surface");
        }
    }

    (resident.iter().map(digest).collect(), uploader.counters())
}

#[test]
fn paths_a_and_b_produce_identical_geometry_digests_on_the_same_chunk_set() {
    let (digests_a, counters_a) = run(UploadPath::ArrayMesh);
    let (digests_b, counters_b) = run(UploadPath::RenderingServerRids);

    assert_eq!(
        digests_a.len(),
        CASES,
        "the chunk set shrank; the comparison would be weaker than it reads"
    );
    for (case, (left, right)) in digests_a.iter().zip(digests_b.iter()).enumerate() {
        assert_eq!(
            left, right,
            "chunk {case}: path A left {left:016x} resident, path B left {right:016x}. \
             The two paths send the same vertices, so a difference is one of item 53's two: \
             the colour quantisation, or an index buffer a patch did not update.\n\
             path A {counters_a:?}\npath B {counters_b:?}"
        );
    }
}

/// The comparison above is only worth making if path B took each of its branches. A run
/// in which B never patched would be comparing two rebuild paths and would prove nothing
/// about the one write that reaches the engine's buffers directly. The fourth branch, the
/// index-array guard, has its own test below.
#[test]
fn the_chunk_set_drives_path_b_down_every_branch() {
    let (_, counters) = run(UploadPath::RenderingServerRids);
    let cases = u64::try_from(CASES).expect("six");

    assert_eq!(counters.creates, cases, "pass 0 must create every chunk");
    assert!(
        counters.rebuilds_vertex_count > 0,
        "pass 1 must move a vertex count: {counters:?}"
    );
    assert_eq!(
        counters.patches, cases,
        "pass 2 must patch every chunk in place: {counters:?}"
    );
    assert_eq!(counters.rebuilds_layout, 0, "the probe said patchable");
}

/// The 29-of-2 570 case, on a real surface: the same vertex count, a different index
/// array. G1 measured it at about one upload in ninety, and a count-only guard would have
/// patched every one of them — leaving path B drawing the new vertices with the previous
/// winding, which is back-face-culled holes and no timing signature at all.
///
/// It is built here rather than meshed, because a four-pass set of real chunks does not
/// happen to produce a consecutive pair with equal counts and different indices; what is
/// real is the surface, which comes from the mesher, and the change, which is the winding
/// flip the guard exists for.
#[test]
fn an_index_array_change_is_rebuilt_on_both_paths_and_still_matches() {
    let mut mesher = Mesher::new(LightParams::new(15, 1).expect("legal"));
    let mut buffers = MeshBuffers::empty();
    let first = mesh(&mut mesher, &mut buffers, 0, 0);

    // The same vertices, with every triangle's last two indices swapped: the array a
    // remesh produces when a quad moves between face directions.
    let mut flipped_indices = first.indices().to_vec();
    for triangle in flipped_indices.chunks_exact_mut(3) {
        triangle.swap(1, 2);
    }
    let flipped = Surface::new(
        first.positions().to_vec(),
        first.colours().to_vec(),
        flipped_indices,
    )
    .expect("matching lengths");
    assert_eq!(
        first.vertex_count(),
        flipped.vertex_count(),
        "the whole point is that the counts agree"
    );
    assert_ne!(
        first.indices(),
        flipped.indices(),
        "and that the arrays do not"
    );

    let mut path_b = Uploader::new(UploadPath::RenderingServerRids, 1);
    path_b.record_probe(probed());
    let mut resident_b = ResidentSurface::default();
    let mut path_a = Uploader::new(UploadPath::ArrayMesh, 1);
    let mut resident_a = ResidentSurface::default();

    for surface in [&first, &flipped] {
        for (uploader, resident) in [
            (&mut path_a, &mut resident_a),
            (&mut path_b, &mut resident_b),
        ] {
            let op = uploader.plan(0, surface).expect("in range");
            resident
                .apply(&op, ColourQuantisation::RoundHalfUp)
                .expect("the op fits");
        }
    }

    assert_eq!(
        path_b.counters().rebuilds_index_changed,
        1,
        "path B must refuse the in-place branch here: {:?}",
        path_b.counters()
    );
    assert_eq!(path_b.counters().patches, 0);
    assert_eq!(
        digest(&resident_a),
        digest(&resident_b),
        "and both paths must end holding the flipped index array"
    );
    assert_eq!(
        resident_b.indices,
        flipped.indices(),
        "a patch here would have left the previous winding resident"
    );
}

/// Path A's whole cost, and the reason item 53 preferred B: a fresh resource per remesh.
/// Recorded as a number here so that "A churns resources" is measured rather than quoted.
#[test]
fn path_a_rebuilds_every_time_and_path_b_does_not() {
    let (_, counters_a) = run(UploadPath::ArrayMesh);
    let (_, counters_b) = run(UploadPath::RenderingServerRids);
    let uploads = u64::try_from(CASES.saturating_mul(PASSES)).expect("small");

    assert_eq!(
        counters_a.creates.saturating_add(counters_a.rebuilds),
        uploads,
        "path A builds a surface on every upload"
    );
    assert_eq!(counters_a.patches, 0, "path A has no in-place branch");
    assert!(
        counters_b.creates.saturating_add(counters_b.rebuilds) < uploads,
        "path B must avoid at least one rebuild, or it is path A with more code: {counters_b:?}"
    );
}

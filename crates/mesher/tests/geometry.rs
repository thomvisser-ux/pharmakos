// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The geometry golden: per-chunk vertex and index digests over a fixed chunk set.
//!
//! This is the **headless CPU proxy** the whole wall exists to make possible. It links
//! without gdext, so CI checks the real mesher's geometry with no GPU and no engine
//! between the check and the thing checked (decisions log section 2.7 item 56; skeleton
//! plan T4).
//!
//! The producing test writes `<target>/golden/mesher/actual.digests.txt`, where
//! `cargo xtask ci`'s `golden` step byte-compares it with
//! `tests/golden/mesher/expected.digests.txt`. `cargo xtask golden --bless` accepts a move,
//! and a blessed golden has to be explained in the pull request that moved it (AGENTS.md
//! section 5). `tests/golden/mesher/README.md` says what a diff in each direction means.
//!
//! # The format
//!
//! One record per line, tab-separated, LF endings, a trailing newline, no carriage return:
//!
//! ```text
//! <case>\t<vertex-digest>\t<index-digest>\t<vertices>\t<indices>
//! ```
//!
//! Both digests are **xxh3-64**, written as sixteen lowercase hex digits — the project's
//! one hash function (AGENTS.md section 3's approved list; the sim's state hash is the
//! same). The vertex digest runs over the emitted vertices in order, each contributing
//! **28 bytes**: position x, y, z and normal x, y, z as `f32::to_bits().to_le_bytes()`,
//! then the four colour bytes. The index digest runs over the `u16` indices in order, each
//! as `to_le_bytes()`. Counts are decimal.
//!
//! Hashing the float **bit patterns** rather than the printed values is the point: a
//! Windows and a Linux run must agree on the bits, and a digest over formatted decimals
//! would hide a one-ulp difference behind rounding.
//!
//! # The cases
//!
//! Generated here, procedurally, by [`Case::build`] — no committed fixture files, because a
//! 32 KiB material array per case is not human-diffable and the generator is. Every case
//! goes through the real path: bake the light with [`LightField`], gather the six borders
//! with [`ChunkBorders`], mesh with [`Mesher`].

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use pharmakos_mesher::{
    AIR, CHUNK_EDGE, ChunkBorders, ChunkGrid, LightField, LightParams, MAX_VERTICES, MeshBuffers,
    Mesher, chunk_view, map_offset,
};
use xxhash_rust::xxh3::xxh3_64;

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

/// The committed `rules/rules.v1.json` `mesher` block's light pair, stated at the call site
/// because this crate may not read the rules table (crate docs, "No constants that belong
/// to the rules table"). If the owner moves either row, this line moves with it and the
/// golden moves with that — which is the behaviour change the README asks to see explained.
const LIGHT_MAX: u8 = 15;
/// The committed `light_atten`.
const LIGHT_ATTEN: u8 = 1;

fn params() -> LightParams {
    LightParams::new(LIGHT_MAX, LIGHT_ATTEN).expect("the committed rows are a legal pair")
}

// ---------------------------------------------------------------------------
// The tiny integer generator
// ---------------------------------------------------------------------------

/// A 64-bit mixing function — the `splitmix64` finaliser, three xor-shift-multiply rounds.
///
/// Every case that wants a pseudo-random number gets it from here, seeded from the voxel's
/// own coordinates, so a case's terrain is a pure function of its seed and reproduces
/// identically on every platform. Integer throughout; `wrapping_mul` is deliberate, and it
/// is the only arithmetic in this file that is allowed to wrap.
fn mix(seed: u64) -> u64 {
    let mut value = seed ^ 0x9E37_79B9_7F4A_7C15;
    value = (value ^ (value >> 30_u32)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27_u32)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31_u32)
}

/// `mix` of three coordinates and a seed, reduced to `0..modulus`.
fn noise(seed: u64, x: i32, y: i32, z: i32, modulus: u64) -> u64 {
    let key = u64::from(x.unsigned_abs()).wrapping_mul(73_856_093)
        ^ u64::from(y.unsigned_abs()).wrapping_mul(19_349_663)
        ^ u64::from(z.unsigned_abs()).wrapping_mul(83_492_791)
        ^ seed;
    mix(key).checked_rem(modulus).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Cases
// ---------------------------------------------------------------------------

/// One golden case: a whole little map, and the chunk of it that gets meshed.
struct Case {
    name: &'static str,
    grid: ChunkGrid,
    chunk: u32,
    materials: Vec<u8>,
}

/// The map's extent, so a case can write voxels without repeating the arithmetic.
fn set(materials: &mut [u8], grid: ChunkGrid, x: i32, y: i32, z: i32, material: u8) {
    if let Some(at) = map_offset(grid, x, y, z) {
        if let Some(slot) = materials.get_mut(at) {
            *slot = material;
        }
    }
}

/// A grid, checked.
fn grid(x: u32, y: u32, z: u32) -> ChunkGrid {
    ChunkGrid::new(x, y, z).expect("the cases' grids are legal")
}

impl Case {
    /// Every case, in the order they are written to the golden.
    fn all() -> Vec<Self> {
        vec![
            Self::empty(),
            Self::full(),
            Self::single_voxel(),
            Self::flat_floor(),
            Self::stair(),
            Self::overhang_tunnel(),
            Self::crater(),
            Self::checkerboard(),
            Self::six_borders(),
            Self::light_split(),
        ]
    }

    /// A blank map of `grid`, with the chunk to mesh named.
    fn blank(name: &'static str, grid: ChunkGrid, chunk: u32) -> Self {
        Self {
            name,
            grid,
            chunk,
            materials: vec![AIR; grid.voxel_count()],
        }
    }

    /// All air. Meshes to nothing at all — not to a surface with no vertices.
    fn empty() -> Self {
        Self::blank("empty", grid(1, 1, 1), 0)
    }

    /// All stone, one chunk. Only the six outward faces exist, and the map's rim borders
    /// are synthesised sky, so it is six full-face quads.
    fn full() -> Self {
        let mut case = Self::blank("full", grid(1, 1, 1), 0);
        case.materials.fill(1);
        case
    }

    /// One floating voxel in the middle: six quads, the smallest non-empty mesh.
    fn single_voxel() -> Self {
        let mut case = Self::blank("single_voxel", grid(1, 1, 1), 0);
        let dims = case.grid;
        set(&mut case.materials, dims, 16, 16, 16, 2);
        case
    }

    /// Eight solid layers across the whole chunk. The greedy merge's easiest win, and the
    /// case whose colours are checked by hand.
    fn flat_floor() -> Self {
        let mut case = Self::blank("flat_floor", grid(1, 1, 1), 0);
        let dims = case.grid;
        let [voxels_x, _, voxels_z] = dims.voxels();
        for z in 0..voxels_z {
            for x in 0..voxels_x {
                for y in 0..8 {
                    set(&mut case.materials, dims, x, y, z, 1);
                }
            }
        }
        case
    }

    /// A staircase along x, four voxels to a step, with the material changing per step: the
    /// mask key splits on material as well as on light.
    fn stair() -> Self {
        let mut case = Self::blank("stair", grid(1, 1, 1), 0);
        let dims = case.grid;
        let [voxels_x, _, voxels_z] = dims.voxels();
        for z in 0..voxels_z {
            for x in 0..voxels_x {
                let step = x >> 2_u32;
                let material = u8::try_from(1 + (step % 3)).unwrap_or(1);
                for y in 0..=step {
                    set(&mut case.materials, dims, x, y, z, material);
                }
            }
        }
        case
    }

    /// A cliff with a tunnel bored into it at head height, lit only from beside it.
    ///
    /// This is G1 section 10.12's case: a flood fill that seeded the lowest sky cell per
    /// column would leave the tunnel and everything under the overhang at light 0, and the
    /// digest would move.
    fn overhang_tunnel() -> Self {
        let mut case = Self::blank("overhang_tunnel", grid(1, 1, 1), 0);
        let dims = case.grid;
        let [voxels_x, _, voxels_z] = dims.voxels();
        for z in 0..voxels_z {
            for x in 0..voxels_x {
                let top = if x < 16 { 20 } else { 6 };
                for y in 0..top {
                    set(&mut case.materials, dims, x, y, z, 1);
                }
            }
        }
        // An overhang: put the cliff's lip back over the low half at y = 19.
        for z in 0..voxels_z {
            for x in 16..22 {
                set(&mut case.materials, dims, x, 19, z, 1);
            }
        }
        // The tunnel: bore in from the cliff face at y = 8, two voxels tall, six deep.
        for z in 12..20 {
            for y in 8..10 {
                for x in 10..16 {
                    set(&mut case.materials, dims, x, y, z, AIR);
                }
            }
        }
        case
    }

    /// A solid block with a crater blown out of it and rubble scattered around the rim —
    /// the shape G1 measured at 503 microseconds and 6 660 vertices, and the one the perf
    /// alarm times.
    fn crater() -> Self {
        let mut case = Self::blank("crater", grid(1, 1, 1), 0);
        let dims = case.grid;
        let [voxels_x, _, voxels_z] = dims.voxels();
        for z in 0..voxels_z {
            for x in 0..voxels_x {
                for y in 0..24 {
                    // Three materials in bands, so the mask key is not uniform.
                    let material = u8::try_from(1 + ((y >> 3_u32) % 3)).unwrap_or(1);
                    set(&mut case.materials, dims, x, y, z, material);
                }
            }
        }
        // A sphere of radius 11 centred on the surface, with a ragged edge: a voxel within
        // one of the boundary goes or stays by the generator, which is what makes a crater
        // wall hundreds of quads rather than a smooth shell.
        let centre = [16_i32, 22_i32, 16_i32];
        for z in 0..voxels_z {
            for y in 0..24 {
                for x in 0..voxels_x {
                    let squared =
                        (x - centre[0]).pow(2) + (y - centre[1]).pow(2) + (z - centre[2]).pow(2);
                    let ragged = i32::try_from(noise(0xC0FF_EE01, x, y, z, 3)).unwrap_or(0);
                    if squared <= 121 - ragged * 4 {
                        set(&mut case.materials, dims, x, y, z, AIR);
                    }
                }
            }
        }
        case
    }

    /// The worst case the 16-bit index headroom is asserted against: a 16-cubed block of
    /// isolated voxels, every one of them six quads that cannot merge with anything.
    ///
    /// 2 048 voxels x 6 quads x 4 vertices = 49 152 vertices, three quarters of the 65 536
    /// cap. A full-chunk checkerboard would be 393 216 and is refused rather than truncated
    /// — `sweep`'s own test covers that.
    fn checkerboard() -> Self {
        let mut case = Self::blank("checkerboard", grid(1, 1, 1), 0);
        let dims = case.grid;
        for z in 8..24 {
            for y in 8..24 {
                for x in 8..24 {
                    if (x + y + z) % 2 == 0 {
                        set(&mut case.materials, dims, x, y, z, 1);
                    }
                }
            }
        }
        case
    }

    /// A 3 x 3 x 3 map of generated terrain, meshing the middle chunk, so all six borders
    /// are real neighbours rather than the map's rim.
    ///
    /// The case that catches a border mix-up: get the `+z` slice's indexing wrong and this
    /// digest moves while every one-chunk case above stays put.
    fn six_borders() -> Self {
        let dims = grid(3, 3, 3);
        let middle = dims.chunk_index([1, 1, 1]).expect("inside a 3-cubed grid");
        let mut case = Self::blank("six_borders", dims, middle);
        let [voxels_x, _, voxels_z] = dims.voxels();
        for z in 0..voxels_z {
            for x in 0..voxels_x {
                let height = 40 + i32::try_from(noise(0x5EED_0007, x, 0, z, 12)).unwrap_or(0);
                for y in 0..height {
                    let material = u8::try_from(1 + ((y >> 4_u32) % 4)).unwrap_or(1);
                    set(&mut case.materials, dims, x, y, z, material);
                }
            }
        }
        // A shaft down the middle chunk, so the interior is lit and the borders carry a
        // light gradient rather than a flat value.
        for y in 0..64 {
            for z in 44..52 {
                for x in 44..52 {
                    set(&mut case.materials, dims, x, y, z, AIR);
                }
            }
        }
        case
    }

    /// A flat roof under a slot in the ceiling above it: the light falls off across the
    /// roof, so one otherwise-uniform quad is split into a run of narrow ones by the
    /// `(material, light)` mask key alone.
    fn light_split() -> Self {
        let mut case = Self::blank("light_split", grid(1, 1, 1), 0);
        let dims = case.grid;
        let [voxels_x, _, voxels_z] = dims.voxels();
        // A floor, and a ceiling with one slot in it.
        for z in 0..voxels_z {
            for x in 0..voxels_x {
                for y in 0..4 {
                    set(&mut case.materials, dims, x, y, z, 1);
                }
                set(&mut case.materials, dims, x, 20, z, 1);
            }
        }
        for z in 14..18 {
            for x in 0..2 {
                set(&mut case.materials, dims, x, 20, z, AIR);
            }
        }
        case
    }

    /// Meshes the case, through the real bake-gather-mesh path.
    fn mesh(&self) -> MeshBuffers {
        let mut field = LightField::new(self.grid, params());
        field
            .bake_all(&self.materials)
            .expect("the case built its map at the grid's size");
        let mut borders = ChunkBorders::new();
        borders.gather(
            self.grid,
            &self.materials,
            field.all(),
            params(),
            self.chunk,
        );
        let view = chunk_view(&self.materials, &field, &borders, self.chunk)
            .expect("the case names a chunk inside its grid");
        let mut mesher = Mesher::new(params());
        let mut out = MeshBuffers::empty();
        mesher
            .mesh_chunk_into(&view, &mut out)
            .unwrap_or_else(|error| panic!("case {}: {error}", self.name));
        out
    }
}

// ---------------------------------------------------------------------------
// Digests
// ---------------------------------------------------------------------------

/// The vertex digest: 28 bytes per vertex, in emission order.
fn vertex_digest(buffers: &MeshBuffers) -> u64 {
    let mut bytes: Vec<u8> = Vec::with_capacity(buffers.positions().len() * 28);
    for index in 0..buffers.positions().len() {
        let position = buffers.positions().get(index).copied().unwrap_or([0.0; 3]);
        let normal = buffers.normals().get(index).copied().unwrap_or([0.0; 3]);
        let colour = buffers.colours().get(index).copied().unwrap_or([0; 4]);
        for value in position.into_iter().chain(normal) {
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        bytes.extend_from_slice(&colour);
    }
    xxh3_64(&bytes)
}

/// The index digest: two bytes per index, in emission order.
fn index_digest(buffers: &MeshBuffers) -> u64 {
    let mut bytes: Vec<u8> = Vec::with_capacity(buffers.indices().len() * 2);
    for index in buffers.indices() {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    xxh3_64(&bytes)
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// The workspace root: this crate is `<root>/crates/mesher`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/mesher sits two levels below the workspace root"))
        .to_path_buf()
}

/// Cargo's target directory. `CARGO_TARGET_TMPDIR` is `<target>/tmp`, and it is the only
/// way a test can find the target directory that also honours `CARGO_TARGET_DIR` — which
/// every agent worktree sets to its own lane.
fn target_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .unwrap_or_else(|| panic!("CARGO_TARGET_TMPDIR always has a parent"))
        .to_path_buf()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn the_geometry_digests_match_the_committed_golden() {
    let mut report = String::new();
    for case in Case::all() {
        let buffers = case.mesh();
        // `writeln!` to a String cannot fail, and its line ending is `\n` on every
        // platform — the golden is byte-compared across Windows and Linux, so nothing here
        // may emit `\r` (tests/golden/README.md rule 3).
        writeln!(
            report,
            "{}\t{:016x}\t{:016x}\t{}\t{}",
            case.name,
            vertex_digest(&buffers),
            index_digest(&buffers),
            buffers.positions().len(),
            buffers.indices().len(),
        )
        .expect("writing to a String cannot fail");
    }

    let directory = target_dir().join("golden").join("mesher");
    fs::create_dir_all(&directory)
        .unwrap_or_else(|error| panic!("creating {}: {error}", directory.display()));
    // Written as bytes, so the platform never gets a say in the line endings.
    fs::write(directory.join("actual.digests.txt"), report.as_bytes())
        .unwrap_or_else(|error| panic!("writing actual.digests.txt: {error}"));

    let golden = workspace_root()
        .join("tests")
        .join("golden")
        .join("mesher")
        .join("expected.digests.txt");
    let Ok(expected) = fs::read_to_string(&golden) else {
        panic!(
            "no committed golden at {}. Run `cargo xtask golden --bless` once the fresh \
             output has been read and understood, and say in the pull request what it pins.",
            golden.display()
        );
    };
    assert_eq!(
        expected, report,
        "the geometry digests moved. tests/golden/mesher/README.md says what a diff here \
         means; `cargo xtask golden --bless` accepts it, and the pull request has to say \
         which behaviour moved."
    );
}

#[test]
fn the_flat_floor_colours_are_what_godot_will_read() {
    // Hand-computed from `quantise_channel`'s formula, stated in its doc comment:
    //
    //   light_256 = 64 + (192 * light) / light_max
    //   channel   = (base * shade_256 * light_256 + 32_768) >> 16
    //
    // Every face of the floor looks out at an air cell lit to `light_max` — the sky above,
    // and the map's rim, which `ChunkBorders::gather` synthesises as open sky — so
    // light_256 is 64 + 192 = 256 for all six, and only the face shade differs. Material 1
    // is stone, [133, 133, 143].
    //
    //   -x, shade 184: (133 * 184 * 256 + 32768) >> 16 =  96   (143 -> 103)
    //   +x, shade 210:                                   109   (143 -> 117)
    //   -y, shade 115:                                    60   (143 ->  64)
    //   +y, shade 256:                                   133   (143 -> 143)
    //   -z, shade 159:                                    83   (143 ->  89)
    //   +z, shade 235:                                   122   (143 -> 131)
    let buffers = Case::flat_floor().mesh();
    let mut seen: Vec<[u8; 4]> = buffers.colours().to_vec();
    seen.sort_unstable();
    seen.dedup();

    let mut expected = vec![
        [96_u8, 96, 103, 255],
        [109, 109, 117, 255],
        [60, 60, 64, 255],
        [133, 133, 143, 255],
        [83, 83, 89, 255],
        [122, 122, 131, 255],
    ];
    expected.sort_unstable();
    assert_eq!(
        seen, expected,
        "the quantised vertex colours are what Godot reads byte for byte; a difference here \
         is a colour formula change, not a rounding accident"
    );
}

#[test]
fn the_worst_case_keeps_its_sixteen_bit_headroom() {
    let buffers = Case::checkerboard().mesh();
    let vertices = buffers.positions().len();
    assert_eq!(
        vertices,
        2_048 * 24,
        "2 048 isolated voxels, six quads each"
    );
    assert!(
        vertices < MAX_VERTICES,
        "the checkerboard needs {vertices} vertices against the {MAX_VERTICES} cap"
    );
    // Every index is reachable by a u16, which is the property the cap exists for.
    let highest = buffers.indices().iter().copied().max().unwrap_or(0);
    assert_eq!(usize::from(highest), vertices - 1);
    assert!(
        buffers.positions().len() > 6_660,
        "this case is meant to be harder than G1's measured worst chunk"
    );
}

#[test]
fn the_empty_case_produces_no_surface_at_all() {
    let buffers = Case::empty().mesh();
    assert_eq!(buffers.surface_count(), 0);
    assert!(buffers.indices().is_empty());
}

#[test]
fn the_full_case_is_six_whole_face_quads() {
    // A chunk of solid stone at the map's rim: every outward face is open sky, every
    // interior face is hidden, and each of the six merges to one 32 x 32 quad.
    let buffers = Case::full().mesh();
    assert_eq!(buffers.quad_count(), 6);
    assert_eq!(buffers.positions().len(), 24);
}

#[test]
fn the_light_split_case_really_splits() {
    // The roof under the ceiling slot: if the light were uniform the top face would be one
    // quad, so the case would be pinning nothing. A light gradient makes it many.
    let buffers = Case::light_split().mesh();
    assert!(
        buffers.quad_count() > 20,
        "the (material, light) key should fragment the roof: {} quads",
        buffers.quad_count()
    );
    let distinct: usize = {
        let mut colours: Vec<[u8; 4]> = buffers.colours().to_vec();
        colours.sort_unstable();
        colours.dedup();
        colours.len()
    };
    assert!(
        distinct > 6,
        "a light gradient must produce more than one colour per face direction: {distinct}"
    );
}

#[test]
fn the_overhang_tunnel_is_lit_rather_than_black() {
    // G1 section 10.12's case, asserted rather than assumed: the tunnel's only lit
    // neighbour is a sky cell above the low half's floor, so a flood fill that seeded the
    // lowest sky cell per column would leave every one of these at 0 and the whole case
    // would be pinning a black hole.
    let case = Case::overhang_tunnel();
    let mut field = LightField::new(case.grid, params());
    field
        .bake_all(&case.materials)
        .expect("the case built its map at the grid's size");
    let mouth = field.light_at(15, 8, 16);
    assert!(
        mouth > 0 && mouth < LIGHT_MAX,
        "the tunnel mouth is lit, and dimmer than the sky: {mouth}"
    );
    let deep = field.light_at(12, 8, 16);
    assert!(deep > 0 && deep < mouth, "three voxels in: {deep}");
    // Under the overhang, which is the half seeding the lowest sky cell per column misses:
    // that cell's light arrives from a sky cell *above* the low half's floor, which the
    // naive seeding sets to `light_max` and then never queues.
    let sheltered = field.light_at(18, 10, 16);
    assert!(
        sheltered > deep,
        "under the cliff lip the light comes straight off the open sky beside it, so it \
         must beat the far end of the tunnel: {sheltered} against {deep}"
    );
}

#[test]
fn every_case_is_deterministic_within_a_run() {
    for case in Case::all() {
        let first = case.mesh();
        let second = case.mesh();
        assert_eq!(
            vertex_digest(&first),
            vertex_digest(&second),
            "case {}",
            case.name
        );
        assert_eq!(
            index_digest(&first),
            index_digest(&second),
            "case {}",
            case.name
        );
    }
}

#[test]
fn the_chunk_edge_the_cases_assume_is_the_one_the_crate_has() {
    assert_eq!(CHUNK_EDGE, 32);
}

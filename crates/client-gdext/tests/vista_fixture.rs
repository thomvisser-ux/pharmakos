// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The committed keyframe fixture, and what the client makes of it.
//!
//! `tests/golden/gateway/view_keyframe/expected.keyframe.jsonl` is seat 0's `get_view`
//! keyframe at the opening Lull of the golden seed, one served result per line, produced
//! and drift-checked by the gateway (T16a). It is the one source of truth for what the
//! client is fed with no host running (`docs/design/skeleton-plan-t16a-notes.md` section C
//! (5)), and two things are checked against it here:
//!
//! * `the_vista_fixture_is_the_gateways_golden` — Godot cannot load a file outside
//!   `res://`, so `godot/fixtures/view_keyframe.jsonl` is a byte-identical copy, and this
//!   test is what keeps it one. When the gateway re-blesses its golden, copy the file
//!   again; the vista golden and the geometry digests below will move with it, and the pull
//!   request says why the gateway's moved.
//! * `a_view_response_decodes_and_meshes_to_the_committed_geometry_digest` — every line
//!   goes through the same decode the live client uses (`view::decode_page` into
//!   `ViewModel::apply`), every chunk is meshed with its six real neighbours and its baked
//!   light, and each chunk's surface is digested the way the mesher's own golden digests
//!   (`tests/golden/mesher/README.md`: xxh3-64 over 28 bytes a vertex and 2 bytes an
//!   index). The wire carries no chunk digest (T16a cut it), so this is the client's
//!   geometry pinned end to end, from the gateway's bytes to the mesher's buffers.
//!
//! The digest file is `tests/golden/vista/expected.geometry.txt`. The `golden` step leaves
//! `vista/` to the steps that produce it (`xtask/src/golden.rs`, `SELF_COMPARED_AREAS`), so
//! this test compares it itself, writes the fresh output to
//! `<target>/golden/vista/actual.geometry.txt` either way, and rewrites the committed file
//! only when `PHARMAKOS_BLESS_VISTA` is set — the same shape `tests/fixtures.rs` uses for
//! this crate's own fixtures. `tests/golden/vista/README.md` says what a diff means.

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

use pharmakos_client_gdext::view::{ViewModel, decode_page_text};
use pharmakos_mesher::{DrainBudget, LightParams, MeshBuffers, Mesher};
use xxhash_rust::xxh3::xxh3_64;

/// Set this to rewrite `tests/golden/vista/expected.geometry.txt` from a fresh run.
const BLESS: &str = "PHARMAKOS_BLESS_VISTA";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn gateway_golden() -> PathBuf {
    root()
        .join("tests")
        .join("golden")
        .join("gateway")
        .join("view_keyframe")
        .join("expected.keyframe.jsonl")
}

fn godot_copy() -> PathBuf {
    root()
        .join("godot")
        .join("fixtures")
        .join("view_keyframe.jsonl")
}

#[test]
fn the_vista_fixture_is_the_gateways_golden() {
    let golden = fs::read(gateway_golden()).expect("the gateway's keyframe golden");
    let copy = fs::read(godot_copy()).expect("godot/fixtures/view_keyframe.jsonl");
    assert!(
        golden == copy,
        "godot/fixtures/view_keyframe.jsonl is not a byte-identical copy of \
         tests/golden/gateway/view_keyframe/expected.keyframe.jsonl. The gateway is the \
         producer: copy its golden over the fixture, and say in the pull request why the \
         gateway's moved — the vista golden and tests/golden/vista/expected.geometry.txt \
         move with it."
    );
}

/// The committed table's light and drain rows, read the way the bridge reads them.
fn rules() -> (LightParams, DrainBudget, [u32; 3]) {
    let text = fs::read_to_string(root().join("rules").join("rules.v1.json"))
        .expect("rules/rules.v1.json");
    let table = pharmakos_client_gdext::rules::table_from_json(&text).expect("canonical");
    let rules = pharmakos_client_gdext::rules::mesher_rules(&table).expect("a mesher row");
    let extent = pharmakos_client_gdext::rules::map_extent(&table).expect("a map row");
    (rules.light, rules.budget, extent)
}

/// The mesher golden's vertex digest: 28 bytes a vertex, in emission order.
fn vertex_digest(buffers: &MeshBuffers) -> u64 {
    let mut bytes: Vec<u8> = Vec::with_capacity(buffers.positions().len() * 28);
    for ((position, normal), colour) in buffers
        .positions()
        .iter()
        .zip(buffers.normals())
        .zip(buffers.colours())
    {
        for value in position.iter().chain(normal.iter()) {
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        bytes.extend_from_slice(colour);
    }
    xxh3_64(&bytes)
}

/// The mesher golden's index digest: two bytes an index.
fn index_digest(buffers: &MeshBuffers) -> u64 {
    let mut bytes: Vec<u8> = Vec::with_capacity(buffers.indices().len() * 2);
    for index in buffers.indices() {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    xxh3_64(&bytes)
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root().join("target"), PathBuf::from)
}

#[test]
fn a_view_response_decodes_and_meshes_to_the_committed_geometry_digest() {
    let (params, budget, extent) = rules();
    let mut model = ViewModel::new(params, budget, extent);
    let text = fs::read_to_string(godot_copy()).expect("the fixture");
    let mut pages = 0_usize;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let page = decode_page_text(line).expect("every line is a get_view result");
        model.apply(page).expect("the page applies");
        pages += 1;
    }
    let grid = model
        .grid()
        .expect("the keyframe completes and builds the grid");

    let mut mesher = Mesher::new(params);
    let mut buffers = MeshBuffers::empty();
    let mut fresh = String::new();
    let _ = writeln!(
        fresh,
        "# The client's geometry for the committed keyframe fixture (godot/fixtures/\n\
         # view_keyframe.jsonl, {pages} page(s)): decoded, lit and meshed exactly as the live\n\
         # client does it. Chunk index and corner are the MESHER's (y up); digests are the\n\
         # mesher golden's (xxh3-64, 28 bytes a vertex, 2 an index).\n\
         # grid {} x {} x {} chunks (x, up, north)\n\
         # chunk\tcorner\tvertex-digest\tindex-digest\tvertices\tindices",
        grid.chunks_x(),
        grid.chunks_y(),
        grid.chunks_z()
    );
    let mut vertices = 0_usize;
    for chunk in 0..grid.chunk_count() {
        let corner = model
            .mesh(chunk, &mut mesher, &mut buffers)
            .expect("every chunk of the map meshes");
        vertices += buffers.positions().len();
        let _ = writeln!(
            fresh,
            "{chunk}\t({},{},{})\t{:016x}\t{:016x}\t{}\t{}",
            corner[0],
            corner[1],
            corner[2],
            vertex_digest(&buffers),
            index_digest(&buffers),
            buffers.positions().len(),
            buffers.indices().len()
        );
    }
    assert!(vertices > 0, "the generated map has geometry");

    let actual = target_dir()
        .join("golden")
        .join("vista")
        .join("actual.geometry.txt");
    fs::create_dir_all(actual.parent().expect("a parent")).expect("the output directory");
    fs::write(&actual, &fresh).expect("the fresh output");

    let expected = root()
        .join("tests")
        .join("golden")
        .join("vista")
        .join("expected.geometry.txt");
    if std::env::var_os(BLESS).is_some() {
        fs::write(&expected, &fresh).expect("blessing the committed digests");
        return;
    }
    let committed = fs::read_to_string(&expected).unwrap_or_default();
    assert!(
        committed == fresh,
        "the client's geometry for the keyframe fixture moved. The fresh digests are at {}. \
         A move here is one of: the gateway's keyframe golden moved (copy it again and \
         explain it), the mesher moved (tests/golden/mesher/ will have moved too), or the \
         client's decode, palette lookup or light wiring moved. Read \
         tests/golden/vista/README.md, then re-bless with {BLESS}=1 and say which in the \
         pull request.",
        actual.display()
    );
}

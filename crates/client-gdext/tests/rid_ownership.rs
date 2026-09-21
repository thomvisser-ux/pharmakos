// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **A source-level test for item 53's fourth guard: a RID is never taken from a
//! `Resource` nobody owns.**
//!
//! The other three of item 53's guards have a test that goes red when the guard is
//! removed — `an_index_array_change_under_an_equal_vertex_count_forces_a_rebuild`,
//! `a_layout_we_cannot_patch_disables_the_in_place_path`,
//! `the_probe_names_a_conversion_that_reproduces_the_engines_bytes`. This one had none,
//! and it is the guard whose failure is the most expensive to find: a `Gd<Material>`
//! whose last Rust handle drops is freed, the `Rid` copied out of it before that dangles,
//! and Godot answers with a stream of "Parameter material is null" and a chunk that draws
//! nothing. That is G1's first real path B bug (spike G1 section 10.12), and nothing in
//! `cargo test` or in the CI client leg would reproduce it: the dummy rendering server
//! under `--headless` accepts a dead RID without complaint, so only a real GPU run —
//! which this stage does not have — would show it.
//!
//! What can be checked without a GPU is the SHAPE the bug takes, which is always the
//! same: `get_rid()` called on something that is not owned for as long as the RID is
//! used. `crate::engine::Material` exists to make that unwritable — it holds the
//! `Gd<StandardMaterial3D>` for the renderer's whole life and hands the RID out only
//! through `&self` — and this test asserts that the type is still the only way through.
//! A later author writing `StandardMaterial3D::new_gd().get_rid()` at a call site turns
//! this red, which is the removal the guard actually has to survive.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::fs;
use std::path::{Path, PathBuf};

/// The call that takes a RID out of a resource.
const TAKE_RID: &str = ".get_rid()";

/// The one file, type and method the call is allowed in.
const OWNER_FILE: &str = "engine.rs";
const OWNER_IMPL: &str = "impl Material {";

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` directly under `src/`, in sorted order.
///
/// `fs::read_dir` is on clippy.toml's disallowed-methods list because directory order
/// differs between filesystems; the sort below is the remedy the ban asks for, and this
/// crate is walled, so the lint is lifted for it on the command line by
/// `cargo xtask clippy` pass 2 rather than by an attribute here.
fn sources() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(src_dir())
        .expect("the crate has a src directory")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("rs"))
        .collect();
    files.sort();
    assert!(
        files.len() >= 8,
        "the scan found only {} source files; it is meant to read the whole bridge",
        files.len()
    );
    files
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_owned()
}

/// Every `(file, line number, line)` in which a RID is taken from a resource.
fn rid_sites() -> Vec<(String, usize, String)> {
    let mut sites: Vec<(String, usize, String)> = Vec::new();
    for path in sources() {
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
        for (index, line) in text.lines().enumerate() {
            // Comments describe the rule in the words the rule is about, so a line that
            // is only prose must not trip the scan.
            let code = line.split("//").next().unwrap_or_default();
            if code.contains(TAKE_RID) {
                sites.push((
                    name_of(&path),
                    index.saturating_add(1),
                    line.trim().to_owned(),
                ));
            }
        }
    }
    sites
}

/// The one place a RID may be taken, and the ownership that makes it safe.
#[test]
fn a_rid_is_only_ever_taken_from_the_material_the_renderer_owns() {
    let sites = rid_sites();
    let count = sites.len();
    assert_eq!(
        count, 1,
        "`{TAKE_RID}` appears {count} times in this crate's source. Item 53's fourth guard is \
         that a RID is taken only from a resource something owns for as long as the RID is \
         used, and `engine::Material` is the type that makes that so. A second call site is \
         how G1's dangling-RID bug came back: a `get_rid()` on a temporary compiles, runs, \
         and draws nothing while Godot prints \"Parameter material is null\". Sites: {sites:#?}"
    );
    let (file, line, text) = sites.first().expect("exactly one, asserted above");
    assert_eq!(
        file, OWNER_FILE,
        "the one RID call is in {file} line {line} (`{text}`), not in {OWNER_FILE}"
    );
    assert!(
        text.contains("self.handle"),
        "the RID must come from the owned handle, not from a temporary: {file} line {line} \
         says `{text}`"
    );
}

/// The call site is inside `impl Material`, not merely in the same file.
#[test]
fn the_rid_call_sits_inside_the_type_that_owns_the_handle() {
    let path = src_dir().join(OWNER_FILE);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));

    // The enclosing impl block is the last `impl ... {` opened before the call. Reading
    // it this way rather than by counting braces keeps the test to something a reader can
    // check by eye, and the assertion above has already pinned the call to one line.
    let mut enclosing: Option<String> = None;
    let mut found = false;
    for line in text.lines() {
        let code = line.split("//").next().unwrap_or_default();
        if code.trim_start().starts_with("impl ") {
            enclosing = Some(code.trim().to_owned());
        }
        if code.contains(TAKE_RID) {
            found = true;
            break;
        }
    }
    assert!(found, "{OWNER_FILE} no longer takes a RID at all");
    assert_eq!(
        enclosing.as_deref(),
        Some(OWNER_IMPL),
        "the RID call must sit in `{OWNER_IMPL}`, the type that owns the handle for the \
         renderer's whole life (crates/client-gdext/src/engine.rs, module header)"
    );
}

/// The ownership itself: `Material` holds the handle, rather than borrowing one.
///
/// Without this, the two tests above would still pass on a `Material` that took a
/// `&Gd<StandardMaterial3D>` from its caller — which is the same bug with one more hop in
/// it.
#[test]
fn the_material_type_owns_its_handle_for_the_renderers_whole_life() {
    let path = src_dir().join(OWNER_FILE);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    assert!(
        text.contains("handle: Gd<StandardMaterial3D>"),
        "engine::Material must OWN a `Gd<StandardMaterial3D>`; a borrowed handle would let \
         the resource be freed while a RID taken from it is still in a mesh"
    );
    assert!(
        text.contains("material: Material,"),
        "ChunkRenderer must hold the Material by value, so the handle outlives every RID \
         the renderer handed the engine"
    );
}

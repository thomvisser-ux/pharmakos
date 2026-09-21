// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `godot/` and this crate have to agree, and nothing else checks that they do.
//!
//! `crates/client-gdext` and `godot/` are one ownable unit (decisions-log item 72), and
//! every claim they make about each other — the class name, the library file name, the
//! entry symbol, the scene the CI leg runs — is a **string on both sides**. GDScript has
//! no compiler, `.tscn` and `.gdextension` are read by the engine at run time, and a
//! mismatch surfaces as a Godot error inside a job that has already spent ten minutes
//! building. Worse, on a fresh checkout the extension's classes instantiate as
//! placeholders anyway, so the first symptom of a renamed class looks exactly like the
//! first symptom of a missing import (spike G1 section 10.12).
//!
//! So the strings are checked here, in a test that costs milliseconds and runs on every
//! leg, including the macOS one where `stage-client` skips.
//!
//! This file also pins the two settings item 56 rests on — Single-Safe threading, which
//! is what makes path B's `RenderingServer` calls legal from `_process` without a
//! render-thread hop, and `forward_plus`, the renderer every G1 number was taken on.

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

use pharmakos_client_gdext::BRIDGE_CLASS_NAME;

fn godot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("godot")
}

fn read(relative: &str) -> String {
    let path = godot_dir().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

#[test]
fn the_gdextension_names_the_library_cargo_builds() {
    let extension = read("pharmakos.gdextension");
    // The cdylib's file stem is the package name with hyphens replaced, which is Cargo's
    // rule rather than a choice; xtask's CLIENT_LIB_STEM says the same thing and
    // `stage-client` re-reads this file to check the pair.
    let stem = env!("CARGO_PKG_NAME").replace('-', "_");
    for expected in [
        format!("res://bin/{stem}.dll"),
        format!("res://bin/lib{stem}.so"),
    ] {
        assert!(
            extension.contains(&expected),
            "pharmakos.gdextension does not name `{expected}`"
        );
    }
}

#[test]
fn the_gdextension_carries_item_73s_shape() {
    let extension = read("pharmakos.gdextension");
    for expected in [
        "entry_symbol = \"gdext_rust_init\"",
        "compatibility_minimum = 4.7",
        "reloadable = false",
        "windows.debug.x86_64",
        "windows.release.x86_64",
        "linux.debug.x86_64",
        "linux.release.x86_64",
    ] {
        assert!(
            extension.contains(expected),
            "pharmakos.gdextension is missing `{expected}` (decisions-log item 73)"
        );
    }
    assert!(
        !extension.contains("macos"),
        "item 73 names Windows and Linux only; a macOS entry would make \
         `cargo xtask stage-client` skip for the wrong reason on that leg"
    );
}

#[test]
fn the_check_scene_instantiates_the_class_this_crate_registers() {
    let scene = read("scenes/client_check.tscn");
    assert!(
        scene.contains(&format!("type=\"{BRIDGE_CLASS_NAME}\"")),
        "scenes/client_check.tscn does not instantiate `{BRIDGE_CLASS_NAME}`. A scene that \
         names a class this crate does not register loads as a placeholder node, which is \
         indistinguishable from a missing `--import` until something calls a method on it."
    );
    assert!(
        scene.contains("res://scripts/client_check.gd"),
        "the check scene does not attach the check script"
    );
}

#[test]
fn the_check_script_asks_for_the_class_by_the_name_this_crate_registers() {
    let script = read("scripts/client_check.gd");
    assert!(
        script.contains(&format!("\"{BRIDGE_CLASS_NAME}\"")),
        "scripts/client_check.gd does not check for `{BRIDGE_CLASS_NAME}` by name"
    );
    // The assertion the whole client leg exists for.
    assert!(
        script.contains("caught_panics"),
        "the check script does not read the caught-panic count, which is what CI asserts \
         on (skeleton plan T12, acceptance)"
    );
}

#[test]
fn the_project_pins_the_settings_item_56_rests_on() {
    let project = read("project.godot");
    for (setting, why) in [
        (
            "driver/threads/thread_model=1",
            "Single-Safe: the main thread owns the RenderingServer, which is what makes \
             path B's RID calls legal from _process (item 56)",
        ),
        (
            "renderer/rendering_method=\"forward_plus\"",
            "the renderer every G1 number was taken on",
        ),
        (
            "window/vsync/vsync_mode=0",
            "frame time is measured, not clamped to the display's refresh interval",
        ),
        (
            "run/delta_smoothing=false",
            "Godot 4.7 defaults this to true; it must not rewrite delta into an estimate",
        ),
        (
            "anti_aliasing/quality/msaa_3d=0",
            "MSAA off is why the mesher's T-junctions have never shown",
        ),
    ] {
        assert!(
            project.contains(setting),
            "project.godot is missing `{setting}` — {why}"
        );
    }
    assert!(
        project.contains("config/features=PackedStringArray(\"4.7\""),
        "project.godot must declare the 4.7 feature set (decisions-log item 73 pins 4.7.2)"
    );
}

#[test]
fn the_staging_directory_is_in_git_but_its_contents_are_not() {
    let keep = godot_dir().join("bin").join(".gitkeep");
    assert!(
        keep.is_file(),
        "godot/bin/.gitkeep is missing; without it the staging directory does not exist on \
         a fresh checkout and `res://bin/...` cannot resolve"
    );

    let ignore_path = godot_dir().join("..").join(".gitignore");
    let ignore = fs::read_to_string(&ignore_path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", ignore_path.display()));
    for pattern in ["*.dll", "*.so", "*.dylib", ".godot/"] {
        assert!(
            ignore.lines().any(|line| line.trim() == pattern),
            ".gitignore does not exclude `{pattern}`; the staged library and Godot's import \
             cache are build output and must not be committed"
        );
    }
}

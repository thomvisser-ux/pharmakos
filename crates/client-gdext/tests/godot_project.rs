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
use pharmakos_client_gdext::rules::{map_extent, mesher_rules, table_from_json};

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
        (
            "common/physics_jitter_fix=0.0",
            "the PROJECT setting, not a line in one scene's _ready. At its 0.5 default it \
             rewrites delta into a quantised estimate — 647 of 1 000 steady G1 frames \
             reported exactly 1.3889 ms — and a scene added later would silently inherit \
             it (spike G1 section 10.12)",
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

/// The inline rules table in godot/ is the committed one.
///
/// `scripts/mesher_rules.gd` carries item 54's five numbers as a JSON literal, because a
/// `res://` path inside Godot cannot reach `rules/rules.v1.json` at the repository root.
/// That makes it a second copy of a tuning row, and a copy nothing compares is exactly what
/// AGENTS.md section 12 forbids: the row moves, the copy does not, every check stays green
/// and the client draws with a table that no longer exists. The bridge's own inline copy
/// is pinned the same way by `bridge::tests::the_self_checks_table_is_the_committed_one`.
///
/// It is also the ONLY copy in godot/: a second `const RULES_JSON` in any script is a copy
/// this test would not be reading, so the test counts them.
#[test]
fn the_inline_rules_table_is_the_committed_one() {
    let script = read("scripts/mesher_rules.gd");
    let line = script
        .lines()
        .find(|line| line.starts_with("const RULES_JSON"))
        .expect("mesher_rules.gd declares RULES_JSON");
    let json = line
        .split_once('\'')
        .and_then(|(_, rest)| rest.rsplit_once('\''))
        .map(|(body, _)| body)
        .expect("RULES_JSON is a single-quoted GDScript literal");

    let inline = mesher_rules(&table_from_json(json).expect("the inline table is canonical JSON"))
        .expect("the inline table has a mesher row");

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("rules")
        .join("rules.v1.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    let committed =
        mesher_rules(&table_from_json(&text).expect("the committed table is canonical gp.v1 JSON"))
            .expect("the committed table has a mesher row");

    // The MESHER ROW, not the whole table: `revision` is the committed table's own
    // version and moves whenever any lane adds a row anywhere in it. See
    // `bridge::tests::the_self_checks_table_is_the_committed_one`, which pins the Rust
    // copy the same way and for the same reason.
    assert_eq!(
        inline.budget, committed.budget,
        "godot/scripts/mesher_rules.gd's RULES_JSON carries a drain budget that is no \
         longer rules/rules.v1.json's. Update it, `bridge::round_trip_rules` and the \
         committed table together, and say in the pull request which row moved."
    );
    assert_eq!(
        inline.light, committed.light,
        "godot/scripts/mesher_rules.gd's RULES_JSON carries light parameters that are no \
         longer rules/rules.v1.json's. Update it, `bridge::round_trip_rules` and the \
         committed table together, and say in the pull request which row moved."
    );

    let lull =
        |table: &pharmakos_proto::gp::v1::RulesTable| table.r#match.as_ref().map(|row| row.lull_ms);
    assert_eq!(
        lull(&table_from_json(json).expect("canonical")),
        lull(&table_from_json(&text).expect("canonical")),
        "godot/scripts/mesher_rules.gd's RULES_JSON carries a Lull length that is no longer \
         rules/rules.v1.json's match.lull_ms. Update both together and say which moved."
    );
    assert_eq!(
        map_extent(&table_from_json(json).expect("canonical")).ok(),
        map_extent(&table_from_json(&text).expect("canonical")).ok(),
        "godot/scripts/mesher_rules.gd's RULES_JSON carries a map extent that is no longer \
         rules/rules.v1.json's map.size_x/size_y/size_z. Update both together and say which \
         moved."
    );

    let scripts = godot_dir().join("scripts");
    let mut copies = 0_usize;
    for name in SCRIPTS {
        let text = fs::read_to_string(scripts.join(name))
            .unwrap_or_else(|error| panic!("reading scripts/{name}: {error}"));
        copies += text
            .lines()
            .filter(|line| line.starts_with("const RULES_JSON"))
            .count();
    }
    assert_eq!(
        copies, 1,
        "one pinned copy of the rules row in godot/, not {copies}"
    );
}

/// Every GDScript file in the project, named rather than walked, so a script this list
/// forgot is a failure rather than a file nothing checks.
const SCRIPTS: &[&str] = &[
    "boot.gd",
    "camera_rig.gd",
    "client_check.gd",
    "host_link.gd",
    "lobby.gd",
    "mesher_rules.gd",
    "vista.gd",
    "vista_shot.gd",
    "watch_check.gd",
];

/// Every script is on [`SCRIPTS`], and every scene names scripts and scenes that exist.
///
/// GDScript has no compiler and a `.tscn` is read by the engine at run time, so a renamed
/// file surfaces as an error inside a job that has already spent ten minutes building.
///
/// `fs::read_dir` is on clippy.toml's disallowed-methods list because directory order
/// differs between filesystems; the names are sorted before anything compares them, which
/// is the remedy the ban asks for, and this crate is walled (AGENTS.md section 4.9).
#[test]
fn every_script_is_listed_and_every_scene_names_files_that_exist() {
    let scripts = godot_dir().join("scripts");
    let mut on_disk: Vec<String> = Vec::new();
    for entry in fs::read_dir(&scripts).expect("godot/scripts").flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if Path::new(&name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gd"))
        {
            on_disk.push(name);
        }
    }
    on_disk.sort();
    assert_eq!(
        on_disk,
        SCRIPTS
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>(),
        "godot/scripts and this test's list disagree"
    );

    for scene in [
        "scenes/boot.tscn",
        "scenes/client_check.tscn",
        "scenes/lobby.tscn",
        "scenes/vista.tscn",
        "scenes/vista_shot.tscn",
        "scenes/watch_check.tscn",
    ] {
        let text = read(scene);
        for line in text.lines().filter(|line| line.contains("path=\"res://")) {
            let path = line
                .split("path=\"res://")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .expect("a res:// path");
            assert!(
                godot_dir().join(path).is_file(),
                "{scene} names res://{path}, which does not exist"
            );
        }
    }
    assert!(
        read("scenes/vista.tscn").contains(&format!("type=\"{BRIDGE_CLASS_NAME}\"")),
        "the vista instantiates the bridge"
    );
}

/// The screenshot step renders `res://scenes/vista_shot.tscn` through the project's main
/// scene, passing `--scene=` and `--shot=` after `--` (`xtask/src/main.rs`,
/// `step_screenshot`), so the main scene is the boot scene that honours both, and the shot
/// is taken from the committed keyframe fixture with no host running.
#[test]
fn the_main_scene_dispatches_the_screenshot_steps_arguments() {
    let project = read("project.godot");
    assert!(
        project.contains("run/main_scene=\"res://scenes/boot.tscn\""),
        "project.godot's main scene is the boot scene"
    );
    let boot = read("scripts/boot.gd");
    assert!(boot.contains("--scene="), "boot.gd reads --scene=");
    let shot = read("scripts/vista_shot.gd");
    assert!(shot.contains("--shot="), "vista_shot.gd reads --shot=");
    assert!(
        shot.contains("res://fixtures/view_keyframe.jsonl"),
        "the shot renders the committed keyframe fixture, with no host running"
    );
    assert!(
        !shot.contains("host_link") && !shot.contains("execute_with_pipe"),
        "the shot starts no host (skeleton-plan-t16a-notes.md section A (4))"
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

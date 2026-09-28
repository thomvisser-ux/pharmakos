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
    "credits.gd",
    "editor.gd",
    "host_link.gd",
    "lobby.gd",
    "mesher_rules.gd",
    "rows.gd",
    "rows_shot.gd",
    "rule_list.gd",
    "smoke_check.gd",
    "strings.gd",
    "vista.gd",
    "vista_shot.gd",
    "watch_check.gd",
    "wizard.gd",
    "wizard_shot.gd",
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
        "scenes/credits.tscn",
        "scenes/lobby.tscn",
        "scenes/rows_shot.tscn",
        "scenes/smoke_check.tscn",
        "scenes/vista.tscn",
        "scenes/vista_shot.tscn",
        "scenes/watch_check.tscn",
        "scenes/wizard_shot.tscn",
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

/// The body of one GDScript function in `text`: its `func` line and every line after it up
/// to the next line that starts at column zero.
fn gd_function<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let head = format!("func {name}(");
    let mut lines = text.lines().skip_while(|line| {
        !line.starts_with(&head) && !line.starts_with(&format!("static {head}"))
    });
    let mut body: Vec<&str> = Vec::new();
    if let Some(first) = lines.next() {
        body.push(first);
        body.extend(
            lines.take_while(|line| {
                line.is_empty() || line.starts_with('\t') || line.starts_with(' ')
            }),
        );
    }
    assert!(!body.is_empty(), "the script has no `func {name}`");
    body
}

/// `lobby.gd`'s lines that are not comments.
fn lobby_code(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect()
}

// The lobby's New, Resume, remember and forget paths, pinned by their source (w6 notes A4
// item 2; decisions-log items 111 (C9) and 113 (11)). One check drives the lobby with a
// host: the shipped smoke check (`scenes/smoke_check.tscn`, decisions-log item 117 (7)),
// which lets it idle, opens and closes the credits overlay, presses New match and waits for
// the Lull, then frees it; `cargo xtask package` runs it in the editor and CI's clean-launch
// jobs in the export. It never presses Resume, and the watch check drives `host_link.gd`
// directly and leaves the remembered line alone, so the tests below still pin what the
// lobby may do with a remembered match, and T22's run sheet ("quit in a Lull, resume from
// the lobby", w6 notes A5) is where a person sees that run.

/// The one file the lobby writes is `REMEMBERED`, a `user://` path, and it writes it once
/// the host has announced, with the line it wrote and nothing else (no token: the tokens
/// stay in `host_link.gd`'s variables, and the lobby names none).
#[test]
fn the_lobby_remembers_one_line_and_no_token() {
    let text = read("scripts/lobby.gd");
    let code = lobby_code(&text);
    assert!(
        text.contains("const REMEMBERED := \"user://"),
        "the lobby's remembered line lives in user://"
    );
    let writes: Vec<&&str> = code
        .iter()
        .filter(|line| line.contains("FileAccess.WRITE") || line.contains("FileAccess.open("))
        .collect();
    assert_eq!(writes.len(), 1, "the lobby opens one file: {writes:?}");
    assert!(
        writes
            .iter()
            .all(|line| line.contains("FileAccess.open(REMEMBERED, FileAccess.WRITE)")),
        "the one file the lobby writes is REMEMBERED: {writes:?}"
    );
    let remember = gd_function(&text, "_remember");
    assert!(
        remember
            .iter()
            .any(|line| line.contains("store_string(_line")),
        "the lobby remembers the line it wrote: {remember:?}"
    );
    let announced = gd_function(&text, "_on_announced");
    assert!(
        announced.iter().any(|line| line.trim() == "_remember()"),
        "the line is remembered once the host has announced: {announced:?}"
    );
    let remembered_where: Vec<&&str> = code
        .iter()
        .filter(|line| line.contains("_remember()") && !line.starts_with("func "))
        .collect();
    assert_eq!(
        remembered_where.len(),
        1,
        "the line is remembered in one place: {remembered_where:?}"
    );
    assert!(
        !code
            .iter()
            .any(|line| line.to_ascii_lowercase().contains("token")),
        "the lobby names no token"
    );
}

/// A new match is named only through `HostLink.match_id`, and a resume line is built only
/// by `HostLink.resume_line` from the remembered line, so the lobby spells no `resume` of
/// its own.
#[test]
fn the_lobby_names_and_resumes_only_through_host_links_helpers() {
    let text = read("scripts/lobby.gd");
    let code = lobby_code(&text);
    let new_match = gd_function(&text, "new_match");
    assert!(
        new_match
            .iter()
            .any(|line| line.contains("HostLink.match_id(")),
        "a new match is named by host_link.gd's helper: {new_match:?}"
    );
    let start = gd_function(&text, "_start");
    assert!(
        start
            .iter()
            .any(|line| line.contains("HostLink.resume_line(")),
        "a resume line is built by host_link.gd: {start:?}"
    );
    assert!(
        !code
            .iter()
            .any(|line| line.contains("\\tresume") || line.contains("\"resume")),
        "the lobby spells no `resume` of its own"
    );
    let resume = gd_function(&text, "resume_match");
    assert!(
        resume.iter().any(|line| line.contains("remembered_line()")),
        "Resume goes on with the remembered line: {resume:?}"
    );
}

/// When a footer says the match ENDED, the lobby forgets it: `_forget` removes
/// `REMEMBERED`, and the status line says so through the string table. A refusal is shown
/// through the string table with the host's own reason.
#[test]
fn the_lobby_forgets_an_ended_match_and_shows_a_refusal_as_the_host_wrote_it() {
    let text = read("scripts/lobby.gd");
    let received = gd_function(&text, "_on_received");
    let ended = received
        .iter()
        .position(|line| line.contains("\"ended\""))
        .expect("the lobby reads the phase `ended`");
    assert!(
        received
            .iter()
            .skip(ended)
            .take(4)
            .any(|line| line.trim() == "_forget()"),
        "the lobby forgets a match when it ends: {received:?}"
    );
    let forget = gd_function(&text, "_forget");
    assert!(
        forget.iter().any(|line| line.contains("REMEMBERED"))
            && forget.iter().any(|line| line.contains("remove_absolute")),
        "_forget removes the remembered line: {forget:?}"
    );
    assert!(
        text.contains("Strings.text(\"lobby_forgotten\")"),
        "the lobby says it forgot the match, through the string table"
    );

    // A refusal is shown with the host's own reason.
    let failed = gd_function(&text, "_on_failed");
    assert!(
        failed
            .iter()
            .any(|line| line.contains("lobby_resumed_failed"))
            && failed
                .iter()
                .any(|line| line.contains("\"reason\": reason")),
        "a refused resume is shown with the host's own words: {failed:?}"
    );
}

/// The patterns of one comma-separated Godot export filter.
fn filter_patterns(filter: &str) -> Vec<String> {
    filter
        .split(',')
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Godot's export filters match a `res://`-relative path with `*` (any run of characters,
/// `/` included) and `?` (one character), case-insensitively (`String::matchn`).
fn glob_matches(pattern: &[u8], path: &[u8]) -> bool {
    match (pattern.split_first(), path.split_first()) {
        (None, None) => true,
        (Some((b'*', rest)), _) => {
            glob_matches(rest, path)
                || path
                    .split_first()
                    .is_some_and(|(_, tail)| glob_matches(pattern, tail))
        }
        (Some((b'?', rest)), Some((_, tail))) => glob_matches(rest, tail),
        (Some((expected, rest)), Some((found, tail))) => {
            expected.eq_ignore_ascii_case(found) && glob_matches(rest, tail)
        }
        _ => false,
    }
}

fn excluded(patterns: &[String], path: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| glob_matches(pattern.as_bytes(), path.as_bytes()))
}

/// Every file under `godot/` whose name says it is a check, a shot or a fixture, relative to
/// the project and with forward slashes, sorted.
fn test_files() -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for folder in ["scenes", "scripts", "fixtures"] {
        for entry in fs::read_dir(godot_dir().join(folder))
            .expect("a godot folder")
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            let stem = name.split('.').next().unwrap_or_default();
            let script_or_scene = Path::new(&name).extension().is_some_and(|extension| {
                extension.eq_ignore_ascii_case("gd") || extension.eq_ignore_ascii_case("tscn")
            });
            if folder == "fixtures"
                || (script_or_scene && (stem.ends_with("_check") || stem.ends_with("_shot")))
            {
                found.push(format!("{folder}/{name}"));
            }
        }
    }
    found.sort();
    found
}

/// **What the export ships** (decisions-log item 117 (6)). Every `*_check` and `*_shot`
/// scene and script except the smoke check, and every fixture, is matched by BOTH presets'
/// exclude filters; the smoke check, the lobby, the credits overlay and the boot scene are
/// not. The filter names the client and watch checks by name, never as a `*_check`
/// wildcard: Godot applies the exclude filter after the include filter, so a wildcard would
/// drop the smoke check and no include could bring it back (the probe of item 117).
/// `cargo xtask package` also byte-scans the written `.pck` for the same thing.
#[test]
fn the_export_ships_the_smoke_check_and_no_other_test_file() {
    let presets = read("export_presets.cfg");
    let filters: Vec<Vec<String>> = presets
        .lines()
        .filter_map(|line| line.strip_prefix("exclude_filter="))
        .map(|value| filter_patterns(value.trim_matches('"')))
        .collect();
    assert_eq!(
        filters.len(),
        2,
        "two presets, Windows and Linux: {filters:?}"
    );
    for (name, platform) in [
        ("name=\"Windows Desktop\"", "platform=\"Windows Desktop\""),
        ("name=\"Linux\"", "platform=\"Linux\""),
    ] {
        assert!(
            presets.contains(name) && presets.contains(platform),
            "{name}"
        );
    }
    for setting in [
        "binary_format/embed_pck=false",
        "binary_format/architecture=\"x86_64\"",
        "application/modify_resources=true",
        "application/product_version=\"0.1.0-dev+skeleton\"",
        "application/file_version=\"0.1.0.0\"",
    ] {
        assert!(
            presets.contains(setting),
            "export_presets.cfg lacks `{setting}`"
        );
    }
    assert!(
        !presets.contains('#') && !presets.contains(';'),
        "Godot rewrites export_presets.cfg when the editor saves presets, so it keeps no \
         comment; its PLACEHOLDERs are in godot/README.md"
    );
    let tests = test_files();
    assert!(
        tests.len() >= 10,
        "the scan found the checks, shots and fixtures: {tests:?}"
    );
    for patterns in &filters {
        assert!(
            !patterns.iter().any(|pattern| pattern.contains("_check*")
                || pattern.as_str() == "*_check.tscn"
                || pattern.as_str() == "*_check.gd"),
            "a *_check wildcard would drop the smoke check: {patterns:?}"
        );
        for path in &tests {
            let smoke =
                path.starts_with("scenes/smoke_check.") || path.starts_with("scripts/smoke_check.");
            if smoke {
                assert!(
                    !excluded(patterns, path),
                    "the smoke check must ship: {path}"
                );
            } else {
                assert!(excluded(patterns, path), "{path} would ship: {patterns:?}");
            }
        }
        for shipped in [
            "scenes/smoke_check.tscn",
            "scripts/smoke_check.gd",
            "scenes/lobby.tscn",
            "scripts/lobby.gd",
            "scenes/credits.tscn",
            "scripts/credits.gd",
            "scenes/boot.tscn",
            "scripts/host_link.gd",
        ] {
            assert!(!excluded(patterns, shipped), "{shipped} must ship");
        }
    }
    // The matcher itself, both ways.
    assert!(glob_matches(b"fixtures/*", b"fixtures/rows_report.json"));
    assert!(glob_matches(b"*_shot.tscn", b"scenes/vista_shot.tscn"));
    assert!(!glob_matches(b"*_shot.tscn", b"scenes/vista_shot.gd"));
    assert!(!glob_matches(
        b"scenes/client_check.tscn",
        b"scenes/smoke_check.tscn"
    ));
}

/// **The idle lobby reads no pipe** (decisions-log item 117 (3)). The lobby pumps its host
/// link every frame, also while it waits for New match; before `start()` has spawned the
/// host there is no pipe, and reading it is a SCRIPT ERROR each frame in the editor and a
/// crash on the second frame in an exported build. So `pump()` returns before any read
/// until `start()` has set the pipe, and nothing else reads it.
#[test]
fn the_idle_lobby_reads_no_pipe_before_the_host_is_spawned() {
    let text = read("scripts/host_link.gd");
    let pump = gd_function(&text, "pump");
    let code: Vec<&str> = pump
        .iter()
        .skip(1)
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    assert_eq!(
        code.first().copied(),
        Some("if _done or _stdio == null:"),
        "pump() must return before anything reads the host's pipe: {pump:?}"
    );
    assert_eq!(code.get(1).copied(), Some("return"), "{pump:?}");
    // The pipe is read in `_read_announce` alone, which only `pump` reaches.
    let readers: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("_stdio.get_buffer"))
        .collect();
    assert_eq!(
        readers.len(),
        1,
        "one reader of the host's pipe: {readers:?}"
    );
    let callers: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("_read_announce()") && !line.starts_with("func "))
        .collect();
    assert_eq!(
        callers.len(),
        1,
        "only pump() reads the announce: {callers:?}"
    );
    let start = gd_function(&text, "start");
    assert!(
        start
            .iter()
            .any(|line| line.contains("_stdio = spawned.get(\"stdio\")")),
        "start() sets the pipe it spawned: {start:?}"
    );
}

/// **The packaged root is the executable's folder** (decisions-log item 117 (3)): an export
/// has no `res://` on disk, so `find_root` falls back to the executable's folder when
/// `OS.has_feature("template")`, and to the folder above the project otherwise.
#[test]
fn an_exported_build_finds_its_root_beside_the_executable() {
    let text = read("scripts/host_link.gd");
    let root = gd_function(&text, "find_root");
    let template = root
        .iter()
        .position(|line| line.contains("OS.has_feature(\"template\")"))
        .expect("find_root asks whether this is an export");
    assert!(
        root.get(template + 1)
            .is_some_and(|line| line.contains("OS.get_executable_path().get_base_dir()")),
        "an export's root is the executable's folder: {root:?}"
    );
    let project = root
        .iter()
        .position(|line| line.contains("globalize_path(\"res://\")"))
        .expect("the editor's fallback stays");
    assert!(
        template < project,
        "the export's case comes first: {root:?}"
    );
}

/// **The camera's keys are text while a GUI control has keyboard focus** (decisions-log item
/// 123 (2) 8). `camera_rig.gd` polls its free-look keys only when no control holds focus,
/// and a mouse press on the 3D view that no control took releases focus, so typing a note
/// or a wizard value never pans the camera and the keys come back without a restart. The
/// behaviour is checked headless by `watch_check.gd`'s `_camera_keys` (ci.yml's `client
/// extension` jobs); this pins the source on every leg. Following is the lobby's Follow
/// button: no key binds it.
#[test]
fn the_camera_ignores_its_keys_while_a_control_has_focus() {
    let text = read("scripts/camera_rig.gd");
    let taken = gd_function(&text, "_keys_taken");
    assert!(
        taken
            .iter()
            .any(|line| line.contains("viewport.gui_get_focus_owner() != null")),
        "the keys are taken while a control has focus: {taken:?}"
    );
    let keys = gd_function(&text, "_key_move");
    let first_poll = keys
        .iter()
        .position(|line| line.contains("Input.is_key_pressed("))
        .expect("_key_move polls the keys");
    let guard = keys
        .iter()
        .position(|line| line.trim() == "if _keys_taken():")
        .expect("_key_move asks whether the keys are taken");
    assert!(
        guard < first_poll
            && keys
                .get(guard + 1)
                .is_some_and(|line| line.trim() == "return move"),
        "no key is polled while a control has focus: {keys:?}"
    );
    let polls = text
        .lines()
        .filter(|line| line.contains("Input.is_key_pressed("))
        .count();
    let polls_in_key_move = keys
        .iter()
        .filter(|line| line.contains("Input.is_key_pressed("))
        .count();
    assert_eq!(polls, polls_in_key_move, "every key poll is in _key_move");
    let unhandled = gd_function(&text, "_unhandled_input");
    let press = unhandled
        .iter()
        .position(|line| line.contains("event is InputEventMouseButton and event.pressed"))
        .expect("_unhandled_input reads a mouse press");
    assert!(
        unhandled
            .iter()
            .skip(press)
            .take(3)
            .any(|line| line.trim() == "get_viewport().gui_release_focus()"),
        "a press on the view releases the GUI focus: {unhandled:?}"
    );
    assert!(
        !text.contains("F toggles") && text.contains("Follow"),
        "the header names the lobby's Follow button, not an F key"
    );
    for script in [
        "scripts/camera_rig.gd",
        "scripts/lobby.gd",
        "scripts/editor.gd",
        "scripts/vista.gd",
    ] {
        assert!(!read(script).contains("KEY_F)"), "{script} binds no F key");
    }
}

/// **The lobby hosts three rounds** (decisions-log item 123 (2) 3): skeleton-plan section
/// 1.1's Probation-shaped match. The config line carries the value in its sixth field and
/// keeps its six tab-separated fields, so a remembered `last_match.txt` still parses; the
/// PLACEHOLDER above the constants records the decision and keeps the settings screen and
/// the Probation preset the owner's, at S1.
#[test]
fn the_lobby_hosts_a_three_round_match() {
    let text = read("scripts/host_link.gd");
    let limits: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("const ROUND_LIMIT"))
        .collect();
    assert_eq!(
        limits,
        ["const ROUND_LIMIT := 3"],
        "the lobby hosts three rounds"
    );
    let block: String = text
        .lines()
        .skip_while(|line| !line.starts_with("## The match the lobby hosts"))
        .take_while(|line| line.starts_with("##"))
        .collect::<Vec<&str>>()
        .join(" ");
    for needle in ["PLACEHOLDER", "item 123 (2) 3", "Probation preset", "at S1"] {
        assert!(
            block.contains(needle),
            "the round limit's PLACEHOLDER names `{needle}`: {block:?}"
        );
    }
    let line = gd_function(&text, "config_line");
    assert!(
        line.iter().any(|row| row.contains(
            "\"%s\\t%s\\t%d\\t%d\\t%s\\t%d\\n\" % [match_id, MATCH_SEED, seats, HUMAN_SEAT, \
             ladder, ROUND_LIMIT]"
        )),
        "the config line keeps its six fields, the round limit last: {line:?}"
    );
}

/// **The Credits button** (decisions-log item 117 (11)): in the pre-match chooser only, it
/// adds the credits overlay as a child and hides the chooser until Back; it never changes
/// the scene (freeing the lobby ends its host) and does nothing once a match has started.
#[test]
fn the_credits_button_opens_an_overlay_from_the_chooser() {
    let text = read("scripts/lobby.gd");
    assert!(
        text.contains("_button(_chooser, Strings.text(\"lobby_about\"), _show_about)"),
        "the Credits button is in the pre-match chooser"
    );
    assert!(
        text.contains("const AboutScene := preload(\"res://scenes/credits.tscn\")"),
        "the overlay is the credits scene"
    );
    let handler = gd_function(&text, "_show_about");
    let code: Vec<&str> = handler.iter().map(|line| line.trim()).collect();
    assert!(
        code.contains(&"if _started:"),
        "no overlay once a host runs: {handler:?}"
    );
    assert!(
        code.iter()
            .any(|line| line.starts_with("add_child(overlay)")),
        "an overlay, added as a child: {handler:?}"
    );
    assert!(
        code.iter()
            .any(|line| line.contains("_chooser.visible = false")),
        "the overlay covers the chooser: {handler:?}"
    );
    assert!(
        !text.contains("change_scene"),
        "the lobby never changes scene, which would free it and end its host"
    );
    let overlay = read("scripts/credits.gd");
    for engine in [
        "Engine.get_license_text()",
        "Engine.get_copyright_info()",
        "Engine.get_license_info()",
    ] {
        assert!(overlay.contains(engine), "the overlay draws {engine}");
    }
    assert!(
        !overlay.contains("FileAccess") && !overlay.contains("DirAccess"),
        "the overlay reads no file: the licences by area are strings.gd's"
    );
    let scene = read("scenes/credits.tscn");
    assert!(
        scene.contains("[node name=\"About\" type=\"CanvasLayer\"]"),
        "the overlay's root is the `About` canvas layer the smoke check finds"
    );
}

/// **The smoke check goes through the lobby** (decisions-log item 117 (7)): it instances the
/// real lobby, presses its own buttons, and ends by freeing it; it spawns no host of its
/// own and calls no bridge method that would start or steer a match around the lobby.
#[test]
fn the_smoke_check_drives_the_real_lobby() {
    let text = read("scripts/smoke_check.gd");
    assert!(text.contains("preload(\"res://scenes/lobby.tscn\")"));
    assert!(text.contains("_idle()") && text.contains("for _i in IDLE_WAITS:"));
    for forbidden in [
        "execute_with_pipe",
        "link.start(",
        "watch_command(",
        "new_match()",
    ] {
        assert!(
            !text.contains(forbidden),
            "the smoke check must not go around the lobby: {forbidden}"
        );
    }
    assert!(
        text.contains("caught_panics()"),
        "it asserts the caught-panic count"
    );
    assert!(text.contains("\"lull\""), "it waits for the Lull by name");
    let ok: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("[smoke] OK"))
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect();
    assert_eq!(ok.len(), 1, "exactly one OK line: {ok:?}");
    let scene = read("scenes/smoke_check.tscn");
    assert!(scene.contains("res://scripts/smoke_check.gd"));
}

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
    assert!(
        !project.contains("enable_file_logging=false")
            && !project.contains("enable_file_logging.pc=false"),
        "the engine's own log is the only record of a start that fails before the main scene"
    );
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
            "run/flush_stdout_on_print=true",
            "F3 (decisions-log item 126 (3)): a release build otherwise flushes its log only on \
             an error or a clean exit, so an instance that hangs and is ended by hand leaves \
             an empty log, as the demo's hung second instance did",
        ),
        (
            "file_logging/enable_file_logging.pc=true",
            "a start that fails before the main scene leaves its only record in the engine's \
             log on a double-clicked Windows game, whose standard streams go nowhere; finding \
             N's per-instance log sits beside it (scripts/instance_log.gd)",
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
    // Round 1's Lull has its own row (decisions-log item 127 (2)); the client times it, so
    // its copy is pinned beside the later Lulls'.
    let first_lull = |table: &pharmakos_proto::gp::v1::RulesTable| {
        table.r#match.as_ref().map(|row| row.first_lull_ms)
    };
    let inline_first = first_lull(&table_from_json(json).expect("canonical"));
    assert_eq!(
        inline_first,
        first_lull(&table_from_json(&text).expect("canonical")),
        "godot/scripts/mesher_rules.gd's RULES_JSON carries a first Lull length that is no \
         longer rules/rules.v1.json's match.first_lull_ms. Update both together and say which \
         moved."
    );
    assert!(
        inline_first.is_some_and(|length| length > 0),
        "the inline table times round 1's Lull: {inline_first:?}"
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
    "condition_picker.gd",
    "credits.gd",
    "editor.gd",
    "host_link.gd",
    "instance_log.gd",
    "lobby.gd",
    "lobby_shot.gd",
    "mesher_rules.gd",
    "picker_shot.gd",
    "ring_shot.gd",
    "rows.gd",
    "rows_shot.gd",
    "rule_list.gd",
    "rule_list_shot.gd",
    "smoke_check.gd",
    "strings.gd",
    "value_field.gd",
    "values.gd",
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
        "scenes/picker_shot.tscn",
        "scenes/rows_shot.tscn",
        "scenes/rule_list_shot.tscn",
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

/// **Ready says what it does** (decisions-log item 143 (4)): its label names the Lull's end, the
/// line under it, shown in a Lull only (a branch on the bridge's phase string), says what
/// happens with nothing sealed, and the client's two lines about Ready state the rig's own gate
/// rather than a gateway rule.
#[test]
fn ready_says_what_it_does_and_the_gate_is_the_clients() {
    let strings = read("scripts/strings.gd");
    assert!(string_row(&strings, "lobby_ready").contains("Ready — end the Lull"));
    assert!(string_row(&strings, "lobby_ready_help").contains("safe playbook"));
    let lobby = read("scripts/lobby.gd");
    assert!(
        gd_code(&lobby, "_process").iter().any(|line| line
            .contains("_ready_help.visible = String(state.get(\"phase\", \"\")) == \"lull\"")),
        "the explanation is for the Lull"
    );
    assert!(
        gd_code(&lobby, "_process")
            .iter()
            .any(|line| line.contains("_ready_help.text = ready_line(vista.bridge.editor_state())")),
        "the line says what Ready does now (fixp)"
    );
    assert!(
        gd_code(&lobby, "ready_line").iter().any(|line| line.contains(
            "\"lobby_ready_sealed\" if editor_state.get(\"sealed_this_round\", false) else \"lobby_ready_help\""
        )),
        "the bridge's word picks the line"
    );
    assert!(string_row(&strings, "lobby_ready_sealed").contains("Ready keeps it"));
    for key in ["status_closed_ready", "unchecked_ready"] {
        let row = string_row(&strings, key);
        assert!(!row.contains("final"), "{key} claims a gateway rule: {row}");
        assert!(
            row.contains("this client sends no planning call"),
            "{key}: {row}"
        );
    }
    for key in ["status_closed_push", "status_closed_recap"] {
        let row = string_row(&strings, key);
        assert!(
            !row.contains("opens again in the next Lull"),
            "{key} promises a Lull the final recap does not have (item 143 (3)): {row}"
        );
    }
}

/// **Each instance keeps its own log** (finding N, decisions-log item 139 (2)): the main scene
/// adds the logger before anything else happens, on a path named by this process; the check's
/// own run, `client_check.gd`'s `_check_the_logs`, writes through two of them.
#[test]
fn every_instance_logs_to_its_own_file() {
    let boot_script = read("scripts/boot.gd");
    let boot = gd_code(&boot_script, "_ready");
    assert!(
        boot.first().is_some_and(|line| line
            .contains("OS.add_logger(InstanceLog.new(InstanceLog.path_of(OS.get_process_id())))")),
        "the main scene's first act names the log by this process: {boot:?}"
    );
    assert!(
        gd_code(&read("scripts/client_check.gd"), "_check_the_logs")
            .iter()
            .any(|line| line.contains("InstanceLog.new(")),
        "the client check writes through the logger"
    );
}

/// **The lobby hosts three rounds** (decisions-log item 123 (2) 3): skeleton-plan section
/// 1.1's Probation-shaped match. The config line carries the value in its sixth field; the
/// PLACEHOLDER above the constants records the decision and keeps the settings screen the
/// owner's, at S6, the Probation preset having moved to S3 (decisions-log item 153 (2),
/// from item 127 (10)). Since S2's `rnds` the sixth field is `round_limit()`'s text, which
/// is `ROUND_LIMIT` unless the launch says `--rounds` (the next test), so the field is
/// written with `%s`. Since S3's `gwp` the line has a **seventh field, the preset**, which
/// `pharmakos_gateway::serve::Config` requires (S3's plan, E6): the lobby writes it in the
/// same pull request the gateway starts reading it, so `main`'s lobby is never refused, and
/// the seed and the preset are parameters the watch check names its own of (AGENTS.md
/// section 5).
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
    for needle in [
        "PLACEHOLDER",
        "item 123 (2) 3",
        "Probation preset",
        "OWNER, S6",
        "item 127 (10)",
        "item 153 (2)",
        "PROBATION_SEED",
    ] {
        assert!(
            block.contains(needle),
            "the round limit's PLACEHOLDER names `{needle}`: {block:?}"
        );
    }
    let line = gd_function(&text, "config_line");
    assert!(
        line.iter().any(|row| row.contains(
            "static func config_line(match_id: String, ladder: String, seats: int = \
             MATCH_SEATS, seed: String = MATCH_SEED, preset: String = PRESET) -> String:"
        )),
        "the lobby's line is its own seed and preset unless a caller names others: {line:?}"
    );
    assert!(
        line.iter().any(|row| row.contains(
            "\"%s\\t%s\\t%d\\t%d\\t%s\\t%s\\t%s\\n\" % [match_id, seed, seats, HUMAN_SEAT, \
             ladder, round_limit(), preset]"
        )),
        "the config line's seven fields, the round limit sixth and the preset last: {line:?}"
    );
    let presets: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("const PRESET"))
        .collect();
    assert_eq!(
        presets,
        ["const PRESET := \"probation\""],
        "the lobby hosts Probation, whose untimed Lull the rig reads from the footer"
    );
    let limit = gd_function(&text, "round_limit");
    let code: Vec<&str> = limit
        .iter()
        .skip(1)
        .map(|row| row.trim())
        .filter(|row| !row.is_empty() && !row.starts_with('#'))
        .collect();
    assert_eq!(
        code,
        [
            "var given := rounds_arg(OS.get_cmdline_user_args())",
            "if given[\"given\"]:",
            "return String(given[\"text\"])",
            "return str(ROUND_LIMIT)",
        ],
        "absent `--rounds`, the line keeps ROUND_LIMIT: {limit:?}"
    );
}

/// **The watch check plays the lobby's Probation match** (decisions-log item 160 (4)): its main
/// legs keep `custom`, whose countdown they drive, and its last leg hosts the lobby's own line,
/// chained after the resume leg, asserting what a Probation Lull must be; there is one OK line.
#[test]
fn the_watch_check_plays_the_lobbys_probation_match() {
    let check = read("scripts/watch_check.gd");
    assert!(
        check.contains("const CHECK_PRESET := \"custom\""),
        "the watch check's main legs keep `custom`, whose countdown they drive"
    );
    assert!(
        check.contains(
            "_line = HostLink.config_line(HostLink.match_id(\"watch-check-probation\"), HostLink.LADDER)"
        ),
        "the watch check's last leg hosts the lobby's own line, Probation's preset and seed"
    );
    let leg = gd_code(&check, "_run_probation");
    for wanted in [
        "if _refusals.size() != refused_before:",
        "if not state.get(\"untimed\", false):",
        "if String(state.get(\"timer\", \"\")) != \"\":",
        "if answered < 1:",
        "if countdowns != 0:",
        "if not state.get(\"untimed_by_rule\", false):",
    ] {
        assert!(
            leg.iter().any(|line| line.contains(wanted)),
            "the Probation leg checks `{wanted}`: {leg:?}"
        );
    }
    // The leg is chained, not optional: the resumed leg hands over to it, the scene's start
    // routes to it, and the one OK line is the Probation leg's, naming the three hosts.
    let finish = gd_code(&check, "_finish");
    for wanted in [
        "carried[\"stage\"] = \"probation\"",
        "get_tree().change_scene_to_file(\"res://scenes/watch_check.tscn\")",
        "print(\"[watch-check] OK (host pids %d, %d and %d)\" % [int(carried.get(\"first_pid\", -1)), int(carried.get(\"resumed_pid\", -1)), pid])",
    ] {
        assert!(
            finish.iter().any(|line| line.contains(wanted)),
            "`{wanted}`: {finish:?}"
        );
    }
    assert_eq!(
        check.matches("[watch-check] OK").count(),
        1,
        "exactly one OK line"
    );
    assert!(
        check.contains("_probation = _resumed and String((Engine.get_meta(CARRY) as Dictionary).get(\"stage\", \"\")) == \"probation\"")
            && gd_code(&check, "_ready").iter().any(|line| line.contains("_run_probation()")),
        "the scene's start routes the third scene to the Probation leg"
    );
}

/// **The lobby's seed is Probation's** (S3's decision 13; decisions-log item 156 (1) and
/// (3)): `host_link.gd`'s `MATCH_SEED` is `sim3a`'s pin, `PROBATION_SEED` in
/// `crates/sim/tests/probation_seed.rs`, read here from that file as text and compared,
/// restating neither number, so a re-pin there cannot leave the lobby on a stale seed. The
/// watch check keeps the golden seed (`CHECK_SEED`), which its committed playbooks name.
#[test]
fn the_lobbys_seed_is_the_probation_seed_the_sim_pins() {
    let pin_path = godot_dir()
        .join("..")
        .join("crates")
        .join("sim")
        .join("tests")
        .join("probation_seed.rs");
    let pin_text = fs::read_to_string(&pin_path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", pin_path.display()));
    let pins: Vec<u64> = pin_text
        .lines()
        .filter_map(|line| line.strip_prefix("const PROBATION_SEED: u64 = "))
        .map(|rest| {
            rest.trim_end_matches(';')
                .replace('_', "")
                .parse::<u64>()
                .unwrap_or_else(|error| panic!("the pin `{rest}` is not a u64: {error}"))
        })
        .collect();
    let [pin] = pins.as_slice() else {
        panic!("probation_seed.rs pins PROBATION_SEED once: {pins:?}");
    };
    let host_link = read("scripts/host_link.gd");
    let seeds: Vec<u64> = host_link
        .lines()
        .filter_map(|line| line.strip_prefix("const MATCH_SEED := \"0x"))
        .map(|rest| {
            let digits = rest.trim_end_matches('"');
            assert_eq!(
                digits.len(),
                16,
                "a seed is spelt with 16 hex digits: {rest}"
            );
            u64::from_str_radix(digits, 16)
                .unwrap_or_else(|error| panic!("`{rest}` is not a hex seed: {error}"))
        })
        .collect();
    assert_eq!(
        seeds.as_slice(),
        [*pin],
        "host_link.gd's MATCH_SEED is the Probation seed the sim pins"
    );
    let check = read("scripts/watch_check.gd");
    assert!(
        check.contains("const CHECK_SEED := \"0x00000000ca5caded\""),
        "the watch check keeps the golden seed"
    );
    assert!(
        check.contains("CHECK_SEATS, CHECK_SEED, CHECK_PRESET)"),
        "the watch check names its own seed and preset on its line"
    );
}

/// **`-- --rounds <n>`, the test-only round limit** (decisions-log item 143 (5); S2's
/// `rnds`; s2-plan E21). `host_link.gd` reads it beside `--gamectl=` and `--root=`, in
/// either spelling, and writes the argument's text into the config line's round-limit
/// field as it was given: the client parses and bounds nothing, so a value that is not a
/// whole number, is 0 or does not fit a `u32` reaches the gateway, whose refusal the lobby
/// draws as it draws any host's. The client's own refusals are about the line's framing and
/// the command line's shape only (a tab or a line break, no value, the option twice), and
/// `start()` makes them before it spawns anything. The lobby does not show the option.
#[test]
fn the_rounds_option_writes_its_text_and_refuses_only_the_lines_framing() {
    let text = read("scripts/host_link.gd");
    assert!(
        text.contains("const ROUNDS_ARG := \"--rounds\""),
        "the option is spelt `--rounds`"
    );
    let reader = gd_function(&text, "rounds_arg");
    let code: Vec<&str> = reader
        .iter()
        .map(|row| row.trim())
        .filter(|row| !row.is_empty() && !row.starts_with('#'))
        .collect();
    let repeated = "return {\"given\": true, \"text\": \"\", \"refusal\": RoundsRefusal.REPEATED}";
    let no_value = "return {\"given\": true, \"text\": \"\", \"refusal\": RoundsRefusal.NO_VALUE}";
    assert_eq!(
        code,
        [
            "static func rounds_arg(args: PackedStringArray) -> Dictionary:",
            "var found := {\"given\": false, \"text\": \"\", \"refusal\": RoundsRefusal.NONE}",
            "var expecting := false",
            "for arg in args:",
            "var text := \"\"",
            "if expecting and (arg == ROUNDS_ARG or arg.begins_with(ROUNDS_ARG + \"=\")):",
            repeated,
            "elif expecting and arg.begins_with(\"--\"):",
            no_value,
            "elif expecting:",
            "expecting = false",
            "text = arg",
            "elif arg == ROUNDS_ARG:",
            "expecting = true",
            "continue",
            "elif arg.begins_with(ROUNDS_ARG + \"=\"):",
            "text = arg.trim_prefix(ROUNDS_ARG + \"=\")",
            "else:",
            "continue",
            "if found[\"given\"]:",
            repeated,
            "if text.contains(\"\\t\") or text.contains(\"\\n\") or text.contains(\"\\r\"):",
            "return {\"given\": true, \"text\": \"\", \"refusal\": RoundsRefusal.BREAKS_LINE}",
            "found = {\"given\": true, \"text\": text, \"refusal\": RoundsRefusal.NONE}",
            "if expecting:",
            no_value,
            "return found",
        ],
        "the reader's lines, in order: an option where a value belongs is NO_VALUE, \
         `--rounds` there is REPEATED, and nothing else is judged: {reader:?}"
    );
    // The client parses and bounds nothing: no number is read from the text, and nothing
    // compares it with a bound.
    for parse in [
        "to_int",
        "is_valid_int",
        "int(text",
        "int(arg",
        "text.length",
        "> 0",
        ">= 1",
        "== \"0\"",
    ] {
        assert!(
            !code.iter().any(|row| row.contains(parse)),
            "the reader parses or bounds nothing (`{parse}`): {reader:?}"
        );
    }
    // Each refusal has its words, and start() makes them before it spawns the host.
    let refusal = gd_function(&text, "rounds_refusal");
    for arm in [
        "RoundsRefusal.NONE:",
        "RoundsRefusal.NO_VALUE:",
        "RoundsRefusal.REPEATED:",
        "RoundsRefusal.BREAKS_LINE:",
    ] {
        assert!(
            refusal.iter().any(|row| row.trim() == arm),
            "rounds_refusal() has its `{arm}` arm: {refusal:?}"
        );
    }
    let start = gd_function(&text, "start");
    let refused = start
        .iter()
        .position(|row| row.trim() == "var rounds := rounds_refusal()")
        .expect("start() asks whether `--rounds` can go on a line");
    let spawned = start
        .iter()
        .position(|row| row.contains("OS.execute_with_pipe("))
        .expect("start() spawns the host");
    assert!(
        refused < spawned
            && start.get(refused + 1).map(|row| row.trim()) == Some("if rounds != \"\":")
            && start.get(refused + 2).map(|row| row.trim()) == Some("failed.emit(rounds)")
            && start.get(refused + 3).map(|row| row.trim()) == Some("return false"),
        "a refused `--rounds` starts no host and says why: {start:?}"
    );
    // The argument is read in host_link.gd alone, and the lobby neither shows it nor reads it.
    for script in [
        "scripts/lobby.gd",
        "scripts/strings.gd",
        "scripts/editor.gd",
    ] {
        assert!(
            !read(script).contains("--rounds") && !read(script).contains("ROUNDS_ARG"),
            "{script} neither shows nor reads `--rounds`"
        );
    }
}

/// **`--gamectl=` and `--root=` refuse a repeat as `--rounds` does** (S2's `view`;
/// decisions-log item 146 (4)): before, each reader took the first match and ignored the
/// rest. Both now read through one `prefixed_arg`, which answers `repeated` for a second
/// match and judges nothing about the value; `find_gamectl` and `find_root` read through
/// it; and `start()` refuses a repeat with its words before it reads `--rounds` and before
/// it spawns anything.
#[test]
fn the_gamectl_and_root_readers_refuse_a_repeat_before_any_host_starts() {
    let text = read("scripts/host_link.gd");
    assert!(text.contains("const GAMECTL_ARG := \"--gamectl=\""));
    assert!(text.contains("const ROOT_ARG := \"--root=\""));
    let code = |name: &str| -> Vec<String> {
        gd_function(&text, name)
            .iter()
            .map(|row| row.trim().to_owned())
            .filter(|row| !row.is_empty() && !row.starts_with('#'))
            .collect()
    };
    assert_eq!(
        code("prefixed_arg"),
        [
            "static func prefixed_arg(args: PackedStringArray, prefix: String) -> Dictionary:",
            "var found := {\"given\": false, \"text\": \"\", \"repeated\": false}",
            "for arg in args:",
            "if not arg.begins_with(prefix):",
            "continue",
            "if found[\"given\"]:",
            "return {\"given\": true, \"text\": \"\", \"repeated\": true}",
            "found = {\"given\": true, \"text\": arg.trim_prefix(prefix), \"repeated\": false}",
            "return found",
        ],
        "a second match is `repeated`, and nothing about the value is judged"
    );
    for (finder, prefix) in [("find_gamectl", "GAMECTL_ARG"), ("find_root", "ROOT_ARG")] {
        let lines = code(finder);
        assert_eq!(
            lines.get(1).map(String::as_str),
            Some(
                format!("var given := prefixed_arg(OS.get_cmdline_user_args(), {prefix})").as_str()
            ),
            "{finder} reads its option through prefixed_arg: {lines:?}"
        );
        assert!(
            !lines.iter().any(|row| row.contains("for arg in")),
            "{finder} no longer walks the arguments itself: {lines:?}"
        );
    }
    let repeated = code("repeated_option");
    assert!(
        repeated
            .iter()
            .any(|row| row == "for prefix in [GAMECTL_ARG, ROOT_ARG]:"),
        "repeated_option() asks about both options: {repeated:?}"
    );
    let start = gd_function(&text, "start");
    let at = |needle: &str| {
        start
            .iter()
            .position(|row| row.trim() == needle)
            .unwrap_or_else(|| panic!("start() has `{needle}`: {start:?}"))
    };
    let asked = at("var repeated := repeated_option()");
    assert!(
        asked < at("var rounds := rounds_refusal()")
            && start.get(asked + 1).map(|row| row.trim()) == Some("if repeated != \"\":")
            && start.get(asked + 2).map(|row| row.trim()) == Some("failed.emit(repeated)")
            && start.get(asked + 3).map(|row| row.trim()) == Some("return false"),
        "a repeated option starts no host and says why, before `--rounds` is read: {start:?}"
    );
    let check = read("scripts/watch_check.gd");
    let rounds = gd_function(&check, "_rounds_cases");
    assert!(
        rounds.iter().any(|row| row.trim() == "_option_cases()"),
        "the watch check runs the option table after the rounds table: {rounds:?}"
    );
    let table = gd_function(&check, "_option_cases");
    for row in [
        "[[\"--gamectl=x\", \"--gamectl=x\"], \"--gamectl=\", true, \"\", true],",
        "[[\"--root=a\", \"--rounds=3\", \"--root=b\"], \"--root=\", true, \"\", true],",
        "[[\"--gamectl=x\"], \"--gamectl=\", true, \"x\", false],",
    ] {
        assert!(
            table.iter().any(|line| line.trim() == row),
            "the option table has `{row}`: {table:?}"
        );
    }
}

/// **The `--rounds` reader is checked by its behaviour too** (S2's `rnds`): `watch_check.gd`
/// runs a table of argument lists through `host_link.gd`'s `rounds_arg` before anything
/// else and fails the check on any mismatch (ci.yml's `client extension` jobs).
#[test]
fn the_watch_check_runs_the_rounds_readers_table_first() {
    let check = read("scripts/watch_check.gd");
    let ready = gd_function(&check, "_ready");
    assert_eq!(
        ready.get(1).map(|row| row.trim()),
        Some("_rounds_cases()"),
        "the watch check runs the reader's table first: {ready:?}"
    );
    let table = gd_function(&check, "_rounds_cases");
    for row in [
        "[[\"--rounds\", \"--gamectl=x\"], true, \"\", no_value],",
        "[[\"--rounds\", \"--rounds=4\"], true, \"\", repeated],",
        "[[\"--rounds=4\", \"--rounds=4\"], true, \"\", repeated],",
        "[[\"--rounds=\"], true, \"\", none],",
        "[[\"--rounds=4\\r\"], true, \"\", breaks],",
        "_failures.append(\"rounds_arg(%s) answered %s, not %s\" % [JSON.stringify(case[0]), \
         JSON.stringify(got), JSON.stringify(want)])",
    ] {
        assert!(
            table.iter().any(|line| line.trim() == row),
            "the reader's table has `{row}`: {table:?}"
        );
    }
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

/// The lines of `func name` in `text` that are code, trimmed, with comments and blank lines
/// dropped.
fn gd_code<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    gd_function(text, name)
        .into_iter()
        .skip(1)
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

/// **The first click lands** (F1, decisions-log item 126 (3); S1's plan, task `fixc`). The
/// behaviour is checked headless by `watch_check.gd`'s `_first_click_lands` (ci.yml's
/// `client extension` jobs), which injects one click on Submit, a Fix button and Save with
/// the rows redrawn under the press, with a map menu open and after a panel scroll; this
/// pins the source that makes it hold on every leg:
///
/// * the rows are rebuilt only when they changed, and a Fix button takes no focus;
/// * a press on the panel closes the map menus;
/// * a left-click the map surface takes releases the GUI focus (item 124 (5) (g));
/// * the lobby's containers, drawn over the editor's layer, never take the mouse;
/// * the watch check runs the first-click check in round 1's Lull.
#[test]
fn the_first_click_on_the_panel_reaches_its_handler() {
    let rows = read("scripts/rows.gd");
    let fill = gd_code(&rows, "fill");
    let unchanged = fill
        .iter()
        .position(|line| line.starts_with("if container.has_meta(DRAWN)"))
        .expect("fill compares the rows with the rows it drew");
    let rebuilt = fill
        .iter()
        .position(|line| line.contains("queue_free()"))
        .expect("fill still rebuilds rows that changed");
    assert!(
        unchanged < rebuilt && fill.get(unchanged + 3).copied() == Some("return"),
        "rows that did not change keep their controls: {fill:?}"
    );
    let row = gd_code(&rows, "_row");
    assert!(
        row.contains(&"button.focus_mode = Control.FOCUS_NONE"),
        "a Fix button takes no keyboard focus: {row:?}"
    );

    let editor = read("scripts/editor.gd");
    let input = gd_code(&editor, "_input");
    assert!(
        input
            .iter()
            .any(|line| line.contains("menu_open() and _panel.get_global_rect().has_point(")),
        "a press on the panel is looked for while a map menu is open: {input:?}"
    );
    assert!(
        input.last().copied() == Some("close_menu()"),
        "and closes the menus: {input:?}"
    );
    let take = gd_code(&editor, "_take_map_click");
    assert_eq!(
        take,
        [
            "get_viewport().set_input_as_handled()",
            "get_viewport().gui_release_focus()"
        ],
        "a map click is handled and releases the GUI focus (item 124 (5) (g))"
    );
    let unhandled = gd_code(&editor, "_unhandled_input");
    assert!(
        unhandled
            .first()
            .is_some_and(|line| line.contains("not visible")),
        "a hidden editor takes no map click: {unhandled:?}"
    );
    assert_eq!(
        unhandled.last().copied(),
        Some("map_click(event.position, alt_held(event))"),
        "a left press goes to the map surface's handler: {unhandled:?}"
    );
    let click = gd_code(&editor, "map_click");
    for handler in [
        "_on_beacon_clicked(",
        "_on_vent_clicked(",
        "_on_ground_clicked(",
    ] {
        let at = click
            .iter()
            .position(|line| line.starts_with(handler))
            .unwrap_or_else(|| panic!("map_click calls {handler}: {click:?}"));
        assert_eq!(
            at.checked_sub(1)
                .and_then(|before| click.get(before))
                .copied(),
            Some("_take_map_click()"),
            "the map click is taken before {handler}: {click:?}"
        );
    }

    let lobby = read("scripts/lobby.gd");
    for container in ["column", "_chooser", "buttons"] {
        assert!(
            lobby.contains(&format!(
                "\t{container}.mouse_filter = Control.MOUSE_FILTER_IGNORE"
            )),
            "the lobby's `{container}` is drawn over the editor's panel and must not take its \
             clicks"
        );
    }

    let check = read("scripts/watch_check.gd");
    let run = gd_code(&check, "_run");
    let camera = run
        .iter()
        .position(|line| line.contains("_camera_keys()"))
        .expect("the camera check runs");
    assert!(
        run.get(camera + 3)
            .is_some_and(|line| line.contains("await _first_click_lands()")),
        "the first-click check runs in round 1's Lull, after the camera's: {run:?}"
    );
    assert!(
        check.contains("const CLICKED := [\"fix\", \"save\", \"submit\"]")
            && check.contains("const CASES := [\"redraw\", \"menu\", \"scroll\"]"),
        "every button F1 names, in every case"
    );
}

/// **Before a match, the chooser alone** (F6, decisions-log item 126 (3)): the watch strip
/// and the editor's panel are hidden until the host announces a match, and the status line
/// names the watch speed only in a Push.
#[test]
fn the_chooser_hides_the_watch_strip_and_the_editor_until_a_match_starts() {
    let lobby = read("scripts/lobby.gd");
    let ready = gd_code(&lobby, "_ready");
    let setup = ready
        .iter()
        .position(|line| *line == "editor.setup(vista)")
        .expect("the lobby sets the editor up");
    assert_eq!(
        ready.get(setup + 1).copied(),
        Some("editor.visible = false"),
        "the editor's panel is hidden at once: {ready:?}"
    );
    let build = gd_code(&lobby, "_build_ui");
    let strip = build
        .iter()
        .position(|line| *line == "var buttons := HBoxContainer.new()")
        .expect("the watch strip");
    assert!(
        build
            .iter()
            .skip(strip)
            .take(4)
            .any(|line| *line == "buttons.visible = false"),
        "the watch strip starts hidden: {build:?}"
    );
    let announced = gd_code(&lobby, "_on_announced");
    for shown in ["_strip.visible = true", "editor.visible = true"] {
        assert!(
            announced.contains(&shown),
            "`{shown}` once the host has announced: {announced:?}"
        );
    }
    let process = gd_code(&lobby, "_process");
    let speed = process
        .iter()
        .position(|line| line.contains("\"lobby_speed\""))
        .expect("the status line can name the speed");
    assert_eq!(
        speed
            .checked_sub(1)
            .and_then(|before| process.get(before))
            .copied(),
        Some("if String(state.get(\"phase\", \"\")) == \"push\":"),
        "the speed is named only in a Push: {process:?}"
    );
}

/// **The countdown and the recap are the bridge's, drawn as they came** (S1's plan, task
/// `ui`; decisions-log item 135). The lobby shows the bridge's `timer`, which is empty while
/// the admin footer says the phase is `untimed`, and reads no footer field of its own; and it
/// shows `get_recap`'s prose, through the bridge's `watch_recap`, only in a recap or after the
/// match has ended - a branch on the bridge's phase string, for display.
#[test]
fn the_lobby_draws_the_bridges_countdown_and_the_gateways_recap() {
    let lobby = read("scripts/lobby.gd");
    let code = lobby_code(&lobby).join("\n");
    for field in ["phase_remaining_ms", "untimed", "remaining_ms"] {
        assert!(
            !code.contains(field),
            "lobby.gd reads `{field}` itself; the countdown is the bridge's `timer`"
        );
    }
    let recap = gd_code(&lobby, "_draw_recap");
    assert!(
        recap
            .iter()
            .any(|line| line.contains("vista.bridge.watch_recap()")),
        "the recap is the bridge's: {recap:?}"
    );
    assert!(
        recap
            .iter()
            .any(|line| line.contains("_recap.text = recap_text_of(recap)")),
        "drawn through the one function that writes its words: {recap:?}"
    );
    assert!(
        recap
            .iter()
            .any(|line| line.contains("phase == \"recap\" or phase == \"ended\"")),
        "shown in a recap and after the match has ended: {recap:?}"
    );
    let words = gd_code(&lobby, "recap_text_of");
    assert!(
        words
            .iter()
            .any(|line| line.contains("\"prose\": recap.get(\"prose\", \"\")")),
        "the prose as it came: {words:?}"
    );
    assert!(
        words
            .iter()
            .any(|line| line.contains("recap.get(\"seat_tallies\", [])")),
        "and every seat's tally when the answer carries it (item 155 (3) 7): {words:?}"
    );
    let process = gd_code(&lobby, "_process");
    assert!(
        process
            .iter()
            .any(|line| line.contains("_draw_recap(String(state.get(\"phase\", \"\")))")),
        "drawn every frame from the bridge's phase: {process:?}"
    );
}

/// **A click on a vent offers both its name and its description** (`docs/design/targeting.md`,
/// "Surfaces"; S1's plan, task `ui`; the P6a finding, S3's `ed3b`). At P6a, on a real Windows
/// window, Alt-click and plain click both gave "Place beacon covering this vent": the Alt
/// never reached the click. So the menu no longer hangs on a modifier: every vent click
/// offers "Place beacon covering this vent" (the vent's name) AND "the nearest vent you can
/// cover" (the description), and Alt only puts the description first. Alt is read from the
/// event or from the key's own state, and `--log-input` logs both and the other modifiers on
/// every map click, so a hand check can show what the window delivered. Each entry asks the
/// bridge for `place_covering` with its own target, whose step the bridge writes.
#[test]
fn a_vent_click_offers_a_beacon_covering_it_and_alt_click_the_nearest_you_can_cover() {
    let editor = read("scripts/editor.gd");
    let pick = gd_code(&editor, "_vent_under");
    assert!(
        pick.iter()
            .any(|line| line.contains("bridge.editor_features()")),
        "the vents are the bridge's: {pick:?}"
    );
    let choices = gd_code(&editor, "vent_choices");
    for wanted in [
        "var named := {\"key\": \"menu_cover\", \"id\": ITEM_COVER, \"target\": {\"feature\": vent_id}}",
        "var described := {\"key\": \"menu_cover_nearest\", \"id\": ITEM_COVER_NEAREST, \"target\": {\"nearest_uncovered\": \"vent\"}}",
        "return [described, named] if alt else [named, described]",
    ] {
        assert!(
            choices.contains(&wanted),
            "both entries, every time, Alt first-ing the description: {choices:?}"
        );
    }
    let click = gd_code(&editor, "_on_vent_clicked");
    assert!(
        click
            .iter()
            .any(|line| line.contains("vent_choices(String(vent[\"id\"]), alt)")),
        "the menu is the two choices: {click:?}"
    );
    let held = gd_code(&editor, "alt_held");
    assert!(
        held.contains(&"return event.alt_pressed or Input.is_key_pressed(KEY_ALT)"),
        "Alt is the event's flag or the key held: {held:?}"
    );
    let unhandled = gd_code(&editor, "_unhandled_input");
    assert!(
        unhandled
            .iter()
            .any(|line| line.contains("map_click(event.position, alt_held(event))")),
        "{unhandled:?}"
    );
    assert!(
        unhandled.iter().any(|line| line.contains("if log_input:"))
            && unhandled
                .iter()
                .any(|line| line.contains("event.alt_pressed")
                    && line.contains("Input.is_key_pressed(KEY_ALT)")
                    && line.contains("event.shift_pressed")
                    && line.contains("event.ctrl_pressed")),
        "--log-input logs the modifiers of every map click: {unhandled:?}"
    );
    let menu = gd_code(&editor, "_on_menu");
    let cover = menu
        .iter()
        .position(|line| *line == "ITEM_COVER, ITEM_COVER_NEAREST:")
        .expect("the menu routes both cover entries");
    assert_eq!(
        menu.get(cover.saturating_add(1)).copied(),
        Some("act(\"place_covering\", _menu_targets.get(id, {}))")
    );
    // A beacon's target choice is in its menu every time too: this beacon, or a selector.
    let beacon = gd_code(&editor, "_on_beacon_clicked");
    assert!(
        !beacon.contains(&"if alt:"),
        "the target choice does not hang on Alt: {beacon:?}"
    );
    assert!(
        beacon
            .iter()
            .any(|line| line.contains("_selector = SELECTORS[0] if alt else \"\"")),
        "Alt pre-selects the nearest own beacon: {beacon:?}"
    );
    assert!(
        beacon
            .iter()
            .any(|line| line.contains("Strings.text(\"selector_this\")")),
        "this beacon is a choice of its own: {beacon:?}"
    );
    let unhandled = gd_code(&editor, "map_click");
    let beacon = unhandled
        .iter()
        .position(|line| line.contains("_beacon_under("))
        .expect("beacons are picked");
    let vent = unhandled
        .iter()
        .position(|line| line.contains("_vent_under("))
        .expect("vents are picked");
    let ground = unhandled
        .iter()
        .position(|line| line.contains("bridge.view_pick("))
        .expect("the ground is picked");
    assert!(
        beacon < vent && vent < ground,
        "a beacon, then a vent, then the ground: {unhandled:?}"
    );
}

/// **The next BMI is labelled a prediction** (decisions-log item 135 (2) (g)): the BMI at
/// the band held now, ignoring committed spend, and the meter says the key-core is netted
/// out of draw (the register's S1-31). Both lines are the string table's.
#[test]
fn the_meter_labels_the_next_bmi_as_the_band_held_now_ignoring_committed_spend() {
    let strings = read("scripts/strings.gd");
    let row = |key: &str| {
        strings
            .lines()
            .find(|line| line.trim_start().starts_with(&format!("\"{key}\":")))
            .unwrap_or_else(|| panic!("strings.gd has no `{key}`"))
            .to_owned()
    };
    let bmi = row("meter_bmi");
    assert!(
        bmi.contains("{bmi}")
            && bmi.contains("band you hold now")
            && bmi.contains("ignores the spend your orders have committed"),
        "{bmi}"
    );
    assert!(row("meter_key_core").contains("key-core"));
    let editor = read("scripts/editor.gd");
    let meter = gd_code(&editor, "_draw_meter");
    assert!(
        meter
            .iter()
            .any(|line| line.contains("if meter.get(\"has_bmi_next_dollars\", false):")),
        "a BMI the answer left out is not drawn as 0: {meter:?}"
    );
}

/// A string-table row of `strings.gd`, by its key.
fn string_row(strings: &str, key: &str) -> String {
    strings
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("\"{key}\":")))
        .unwrap_or_else(|| panic!("strings.gd has no `{key}`"))
        .to_owned()
}

/// **A, round 1's "This round" sentence** (S1's stage demo; decisions-log item 139 (2)).
/// `editor.rs`'s `round_ones_sentence_comes_with_its_first_accepted_submit` pins that the
/// bridge's editor has the sentence after round 1's first accepted Submit; this pins that the
/// panel draws whatever sentence the bridge holds, and hides the heading only when there is
/// none.
#[test]
fn the_panel_draws_the_this_round_sentence_the_bridge_holds() {
    let editor = read("scripts/editor.gd");
    let state = gd_code(&editor, "_draw_state");
    assert!(
        state.contains(&"_draw_this_round(String(state.get(\"this_round\", \"\")))"),
        "every redraw draws the bridge's sentence: {state:?}"
    );
    let draw = gd_code(&editor, "_draw_this_round");
    assert!(
        draw.contains(&"_this_round.text = sentence")
            && draw.contains(&"_this_round_box.visible = sentence != \"\""),
        "{draw:?}"
    );
}

/// **G, Save as offers the text's own file** (S1's stage demo): Load hands the bridge the
/// path it read, Save hands it the path it wrote, and Save and Save as read the text's file
/// back from the bridge's editor, which forgets it for a template's playbook and a carried
/// draft (`editor.rs`'s `the_texts_file_is_the_one_it_was_opened_from_or_saved_to_and_no_other`).
/// With no file, Save as offers no name. The save dialog is a save-file dialog, whose own
/// confirmation asks before an overwrite: this pins the mode only, and the native dialog's
/// prompt is the package hand check. The watch check's `_save_as_offers` reads the dialog,
/// after the save as well, under a name the opened file does not share.
#[test]
fn save_as_offers_the_texts_own_file_and_no_other() {
    let editor = read("scripts/editor.gd");
    assert!(
        !editor.contains("var file_path"),
        "the editor keeps no file of its own beside the bridge's"
    );
    assert!(gd_code(&editor, "load_file").contains(&"return bridge.editor_load_file(bytes, path)"));
    assert!(gd_code(&editor, "save_file").contains(&"bridge.editor_saved_to(path)"));
    assert_eq!(
        gd_code(&editor, "text_file"),
        ["return String(bridge.editor_state().get(\"file\", \"\"))"]
    );
    let offer = gd_code(&editor, "open_save_as");
    assert_eq!(
        offer,
        [
            "var file := text_file()",
            "if file == \"\":",
            "_save_dialog.current_file = \"\"",
            "else:",
            "_save_dialog.current_path = file",
            "_save_dialog.popup_centered_ratio(0.6)"
        ],
        "Save as offers the text's file, or no name"
    );
    let save = gd_code(&editor, "_save");
    assert!(
        save.contains(&"var file := text_file()") && save.contains(&"open_save_as()"),
        "Save writes the text's file, or asks: {save:?}"
    );
    assert!(editor.contains("_button(buttons, \"save_as\", open_save_as)"));
    assert!(
        editor.contains("_save_dialog = _file_dialog(FileDialog.FILE_MODE_SAVE_FILE)"),
        "the save dialog is a save-file dialog, whose own confirmation asks before an overwrite"
    );
    let check = read("scripts/watch_check.gd");
    assert!(
        check.contains("await _save_as_offers(\"\", \"%s: the template's playbook\" % id)")
            && check.contains("await _save_as_offers(\"\", \"%s: the carried draft\" % where)")
            && check.contains(
                "await _save_as_offers(COMMITTED.get_file(), \"the opened committed playbook\")"
            )
            && check.contains("await _save_as_offers(SAVED.get_file(), \"the saved playbook\")"),
        "the watch check reads the dialog after a load, a save, a template and a carried draft"
    );
    let saved = check
        .lines()
        .find_map(|line| line.strip_prefix("const SAVED := "))
        .expect("the saved file");
    let committed = check
        .lines()
        .find_map(|line| line.strip_prefix("const COMMITTED := "))
        .expect("the committed file");
    let base = |path: &str| {
        path.trim_matches('"')
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned()
    };
    assert_ne!(
        base(saved),
        base(committed),
        "the saved file's name differs from the opened file's, so the check tells them apart"
    );
}

/// **J, after a timeout** (S1's stage demo): the verdict line's words about sealing are the
/// bridge's `seal`, which says "Ready to seal" only in an open Lull and, outside one, that
/// the safe playbook was filed (`editor.rs`'s `ready_to_seal_is_said_only_in_an_open_lull`;
/// `rig.rs`'s `a_safe_playbook_filed_line_in_the_feed_reaches_the_editor`). The panel never
/// decides it from `qualifies` itself.
#[test]
fn the_verdict_line_says_ready_to_seal_only_by_the_bridges_word() {
    let editor = read("scripts/editor.gd");
    let state = gd_code(&editor, "_draw_state");
    assert!(
        !state.iter().any(|line| line.contains("qualifies")),
        "the panel decides nothing about sealing from `qualifies`: {state:?}"
    );
    assert!(
        state
            .iter()
            .any(|line| line.contains("Strings.text(\"seal_\" + String(state[\"seal\"]))")),
        "{state:?}"
    );
    let strings = read("scripts/strings.gd");
    for name in ["ready", "not_ready", "filed"] {
        string_row(&strings, &format!("seal_{name}"));
    }
    assert!(string_row(&strings, "seal_filed").contains("Not sealed"));
    assert!(string_row(&strings, "status_safe_filed").contains("{detail}"));
    for gone in ["qualifies_yes", "qualifies_no"] {
        assert!(
            !strings.contains(&format!("\"{gone}\"")),
            "`{gone}` is gone"
        );
    }
}

/// **K, a shut door** (S1's stage demo): once the match has ended the map offers no menu, by
/// the bridge's phase string, and in a Push it offers what it always did (item 139 (2));
/// the words that wait for a check give way, on that check's own line, to why it will not
/// come; and every door the bridge can name has its sentences (`rig.rs`'s
/// `after_the_match_ends_no_ask_waits_and_no_ghost_hangs` and
/// `a_check_refused_as_the_lull_ends_says_so_and_nothing_waits`; the watch check's
/// `_refused_in_the_push`).
#[test]
fn the_map_is_shut_once_the_match_ends_and_nothing_waits_on_a_shut_door() {
    use pharmakos_client_gdext::editor::Door;

    let editor = read("scripts/editor.gd");
    assert_eq!(
        gd_code(&editor, "map_open"),
        ["return String(bridge.watch_state().get(\"phase\", \"\")) != \"ended\""]
    );
    let unhandled = gd_code(&editor, "map_click");
    let gate = unhandled
        .iter()
        .position(|line| *line == "if not map_open():")
        .expect("the map click asks whether the map is open");
    let pick = unhandled
        .iter()
        .position(|line| line.contains("_beacon_under("))
        .expect("beacons are picked");
    assert!(
        gate < pick,
        "nothing is picked on a shut map: {unhandled:?}"
    );
    assert_eq!(
        unhandled.get(gate + 2).copied(),
        Some("return false"),
        "a shut map offers no menu: {unhandled:?}"
    );
    let state = gd_code(&editor, "_draw_state");
    for check in pharmakos_client_gdext::editor::Check::ALL {
        assert!(
            state
                .iter()
                .any(|line| line.contains(&format!("unchecked_text(state, \"{}\")", check.name()))),
            "each line reads its own check's reason, `{}`: {state:?}",
            check.name()
        );
    }
    for (draw, waiting) in [
        ("_draw_chips", "chips_waiting"),
        ("_draw_rules", "rules_waiting"),
        ("_draw_route", "route_waiting"),
    ] {
        let code = gd_code(&editor, draw);
        assert!(
            code.iter().any(|line| line.contains(&format!(
                "unchecked if unchecked != \"\" else Strings.text(\"{waiting}\")"
            ))),
            "{draw} says why a check will not come: {code:?}"
        );
    }
    let strings = read("scripts/strings.gd");
    string_row(&strings, "unchecked_refused");
    for door in [Door::Ready, Door::Push, Door::Recap, Door::Ended] {
        string_row(&strings, &format!("unchecked_{}", door.name()));
        string_row(&strings, &format!("status_closed_{}", door.name()));
    }
    let check = read("scripts/watch_check.gd");
    let run = gd_code(&check, "_run");
    let push = run
        .iter()
        .position(|line| line.contains("Ready ended the Lull; Push begun at 4x"))
        .expect("the Push begins");
    assert!(
        run.get(push + 1)
            .is_some_and(|line| line.contains("await _refused_in_the_push()")),
        "the Push's map is checked as the Push begins: {run:?}"
    );
    assert!(
        run.iter()
            .any(|line| line.contains("await _map_click_opens_a_menu()")),
        "with its positive control in round 1's Lull: {run:?}"
    );
    let push = gd_code(&check, "_refused_in_the_push");
    assert!(
        push.iter()
            .any(|line| line.contains("if not taken or not opened:")),
        "in the Push the map's menu opens as before: {push:?}"
    );
    for check_click in ["_map_click_opens_a_menu", "_refused_in_the_push"] {
        assert!(
            gd_code(&check, check_click)
                .iter()
                .any(|line| line.contains("editor.map_click(at, false)")),
            "{check_click} clicks through the map surface's own handler"
        );
    }
}

/// **"Not here" at the click** (the P6a finding, S3's `ed3b`). The gateway's legality
/// sentence for a ground click ("Not here: ..." or "A beacon can go here.") showed in the side
/// panel, away from the menu the click opened. The ground menu now carries the ghost's words
/// as its own first, disabled line, filled in when QUICK's answer comes, and "Place beacon" is
/// greyed while the gateway says the spot is illegal. The words are the ghost's, which are the
/// verifier's sentence; the client only places them.
#[test]
fn the_ground_menu_carries_the_ghosts_words_and_greys_place_when_the_gateway_says_no() {
    let editor = read("scripts/editor.gd");
    let ground = gd_code(&editor, "_on_ground_clicked");
    let ghost = ground
        .iter()
        .position(|line| line.contains("ITEM_GHOST"))
        .expect("the ground menu has the ghost's line");
    let place = ground
        .iter()
        .position(|line| line.contains("ITEM_PLACE"))
        .expect("the ground menu has Place beacon");
    assert!(ghost < place, "the ghost's words head the menu: {ground:?}");
    assert!(
        ground
            .iter()
            .any(|line| line.contains("set_item_disabled(_menu.get_item_index(ITEM_GHOST), true)")),
        "the words are a line, not an action: {ground:?}"
    );
    let draw = gd_code(&editor, "_draw_ghost");
    assert!(
        draw.iter().any(|line| line.contains("_ghost_in_menu(")),
        "a redrawn ghost updates the open menu: {draw:?}"
    );
    let menu = gd_code(&editor, "_ghost_in_menu");
    assert!(
        menu.iter().any(|line| line
            .contains("_menu.set_item_text(_menu.get_item_index(ITEM_GHOST), _ghost_label.text)")),
        "the menu says what the panel's ghost line says: {menu:?}"
    );
    assert!(
        menu.iter().any(|line| line.contains(
            "_menu.set_item_disabled(_menu.get_item_index(ITEM_PLACE), String(ghost.get(\"state\", \"\")) == \"illegal\")"
        )),
        "Place beacon is greyed exactly when the gateway's ghost is illegal: {menu:?}"
    );
}

/// **The map opens on the seat's own commander** (the P6a finding, S3's `ed3b`). At P6a the map
/// opened framed on the whole map, too far out to read, with the core at the panel's edge. The
/// first complete view now opens on this seat's commander, who stands at the core in round 1,
/// at the rig's opening distance, with the commander drawn in the middle of what the editor's
/// panel leaves of the window; with no commander in the view it frames the whole map as
/// before, and the Whole map button still does. Camera only: the commander is the view's own
/// marker, and nothing else is read.
#[test]
fn the_map_opens_on_the_seats_own_commander_clear_of_the_panel() {
    let lobby = read("scripts/lobby.gd");
    let received = gd_code(&lobby, "_on_received");
    let first = received
        .iter()
        .position(|line| line.contains("if first and view.get(\"complete\", false):"))
        .expect("the first complete view is framed");
    assert_eq!(
        received.get(first.saturating_add(1)).copied(),
        Some("if not vista.open_on_own_commander(editor.PANEL_WIDTH):"),
        "it opens on the commander, clear of the panel: {received:?}"
    );
    assert_eq!(
        received.get(first.saturating_add(2)).copied(),
        Some("vista.frame_whole_map()"),
        "and frames the whole map when the view has no commander: {received:?}"
    );
    assert!(
        lobby.contains("func() -> void: vista.frame_whole_map())"),
        "Whole map stays"
    );
    let vista = read("scripts/vista.gd");
    let open = gd_code(&vista, "open_on_own_commander");
    assert!(
        open.iter().any(|line| line.contains("my_commander()"))
            && open
                .iter()
                .any(|line| line.contains("rig.open_on(commander.global_position, cover)")),
        "{open:?}"
    );
    let rig = read("scripts/camera_rig.gd");
    let on = gd_code(&rig, "open_on");
    for wanted in [
        "distance = OPENING_DISTANCE",
        "pitch = OPENING_PITCH",
        "target = null",
    ] {
        assert!(on.contains(&wanted), "{wanted}: {on:?}");
    }
}

/// **New** (the P6a finding, S3's `ed3b`): the Orders bar's New puts the empty playbook the
/// client ships on screen. The text is one data file under `godot/` (`playbooks/new.jsonc`),
/// never text assembled in code, and both export presets pack it: a `.jsonc` is not a
/// resource, so `all_resources` alone would leave it out of the `.pck`. It starts a player's
/// own file, so it carries no comment (comments round-trip into the player's text) and no
/// header: it is MIT OR Apache-2.0 by its own `REUSE.toml` entry (the owner's ruling of
/// 2026-11-17). It carries the verifier's own `on_death` and `fallback` stubs, so one map
/// step makes it sealable. The button asks the bridge (`editor_new`) with the file's bytes,
/// and the door gates it as it gates Fix.
#[test]
fn new_puts_the_shipped_empty_playbook_on_screen() {
    let text = read("playbooks/new.jsonc");
    assert!(
        !text.contains("//") && !text.contains("/*"),
        "the new playbook carries no comment: {text}"
    );
    for wanted in [
        "\"kind\": \"PLAYBOOK\"",
        "\"route\": []",
        "\"on_death\": {\"on_respawn\": \"CONTINUE\", \"max_deaths_before_fallback\": 2}",
        "\"fallback\": {\"hold\": {\"at\": {\"beacon_anchor\": {\"safest\": {}}}}}",
    ] {
        assert!(text.contains(wanted), "{wanted}: {text}");
    }
    let reuse = fs::read_to_string(godot_dir().join("..").join("REUSE.toml")).expect("REUSE.toml");
    let entry = reuse
        .split("[[annotations]]")
        .find(|block| block.contains("path = [\"godot/playbooks/new.jsonc\"]"))
        .expect("new.jsonc's own REUSE.toml entry");
    assert!(
        entry.contains("precedence = \"override\"")
            && entry.contains("SPDX-License-Identifier = \"MIT OR Apache-2.0\""),
        "{entry}"
    );
    let presets = read("export_presets.cfg");
    let includes: Vec<&str> = presets
        .lines()
        .filter_map(|line| line.strip_prefix("include_filter="))
        .collect();
    assert_eq!(includes.len(), 2, "two presets: {includes:?}");
    for include in includes {
        let patterns = filter_patterns(include.trim_matches('"'));
        assert!(
            excluded(&patterns, "playbooks/new.jsonc"),
            "the export packs the new playbook: {patterns:?}"
        );
    }
    let editor = read("scripts/editor.gd");
    assert!(
        editor.contains("const NEW_PLAYBOOK := \"res://playbooks/new.jsonc\""),
        "New reads the shipped file"
    );
    let new = gd_code(&editor, "new_playbook");
    assert!(
        new.iter()
            .any(|line| line.contains("FileAccess.get_file_as_bytes(NEW_PLAYBOOK)"))
            && new
                .iter()
                .any(|line| line.contains("bridge.editor_new(bytes)")),
        "{new:?}"
    );
    let draw = gd_code(&editor, "_draw_state");
    assert!(
        draw.iter()
            .any(|line| line.contains("_new_button.disabled = not open")),
        "the door gates New: {draw:?}"
    );
}

/// **The re-check is visible** (the P6a finding, S3's `ed3b`: "i did not see the re-check and
/// redraw"). While a check of the text on screen is on its way, the verdict line and the size
/// meter's line wear the checking look; when its answer arrives, or the verdict changes, both
/// are highlighted, and the highlight stays until the player's next input (a key, a click, a
/// wheel notch). No clock: no timer, no tween (`no_arithmetic.rs`' clock scan holds the editor
/// to that), so nothing flashes and nothing moves, and there is no motion for a reduced-motion
/// setting to turn off. The words stay the gateway's.
#[test]
fn a_checks_answer_is_highlighted_until_the_next_input() {
    let editor = read("scripts/editor.gd");
    let mark = gd_code(&editor, "answer_arrived");
    assert!(
        mark.contains(&"return not checking and (was_checking or before != after)"),
        "an answer arriving, or a changed verdict, is what lights the line: {mark:?}"
    );
    let draw = gd_code(&editor, "_draw_state");
    assert!(
        draw.iter()
            .any(|line| line.contains("_mark_check(checking, line)")),
        "every redraw tells the highlight whether a check is on its way: {draw:?}"
    );
    let look = gd_code(&editor, "_mark_check");
    assert!(
        look.iter()
            .any(|line| line
                .contains("answer_arrived(_was_checking, checking, _verdict.text, after)")),
        "{look:?}"
    );
    for label in ["_verdict", "_size_line"] {
        assert!(
            look.iter()
                .any(|line| line.contains(&format!("_look({label}, "))),
            "{label} wears the look: {look:?}"
        );
    }
    let input = gd_code(&editor, "_input");
    assert!(
        input.iter().any(|line| line.contains("_clear_highlight()")),
        "the player's next input clears it: {input:?}"
    );
    let clear = gd_code(&editor, "next_input");
    assert!(
        clear.iter().any(|line| line.contains("InputEventKey"))
            && clear
                .iter()
                .any(|line| line.contains("InputEventMouseButton")),
        "a key, a click and a wheel notch are inputs; a mouse move is not: {clear:?}"
    );
}

/// **The rule list draws the gateway's cut and forwards every click** (S3's `ed3b`). A line is
/// drawn from the bridge's runs, a chip as a link whose meta is its index, and nothing in the
/// script reads a sentence; a click on a chip opens the editor its shape names; a rule's row
/// drags onto another of its own list and the drop asks the bridge for the move; the W0702
/// connector is drawn on the list's own `draw`; the gateway's refusal of an edit and the
/// verifier's conflict sentence are lines under the rule. A map click while a pick is on goes
/// to the bridge's pick, not the menus, and Escape calls the pick off.
#[test]
fn the_rule_list_draws_the_gateways_cut_and_forwards_every_click() {
    let list = read("scripts/rule_list.gd");
    let code: Vec<&str> = list
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .collect();
    for forbidden in [".split(", ".find(", ".substr(", "RegEx", ".begins_with("] {
        assert!(
            !code.iter().any(|line| line.contains(forbidden)),
            "rule_list.gd reads a sentence with `{forbidden}`"
        );
    }
    let row = gd_code(&list, "_row");
    for wanted in [
        "row.push_meta(chip)",
        "row.add_text(piece)",
        "row.meta_clicked.connect(func(meta: Variant) -> void: on_chip.call(int(meta)))",
        "row.accessibility_name = text",
        "row.set_meta(\"line\", text)",
    ] {
        assert!(row.contains(&wanted), "{wanted}: {row:?}");
    }
    let drag = gd_code(&list, "_draggable");
    assert!(
        drag.iter().any(|line| line.contains(
            "on_move.call(int(data[\"rule\"]), String(data[\"pointer\"]), rule, pointer, int(data[\"revision\"]))"
        )),
        "a drop names both rules by index and pointer, and the rendering: {drag:?}"
    );
    assert!(
        drag.iter()
            .any(|line| line.contains("int(data.get(\"revision\", -1)) == revision")),
        "a drag from another rendering drops nowhere: {drag:?}"
    );
    assert!(
        drag.iter().any(|line| line.contains("if not editable:")),
        "nothing drags while the list takes no click: {drag:?}"
    );
    let fill = gd_code(&list, "fill");
    for wanted in ["rule_conflict", "rule_refused", "container.queue_redraw()"] {
        assert!(
            fill.iter().any(|line| line.contains(wanted)),
            "{wanted}: {fill:?}"
        );
    }
    let editor = read("scripts/editor.gd");
    assert!(
        editor.contains(
            "_rules_box.draw.connect(func() -> void: RuleList.draw_connectors(_rules_box))"
        ),
        "the connector is drawn on the list's own draw"
    );
    let chip = gd_code(&editor, "_on_rule_chip");
    for wanted in [
        "\"number\":",
        "\"choice\":",
        "\"pick\":",
        "bridge.editor_begin_pick(index, _rules_rendering, named[\"pointer\"])",
        "_say_local(\"rule_chip_read_only_\" + String(chip.get(\"why\", \"\")), {})",
    ] {
        assert!(chip.contains(&wanted), "{wanted}: {chip:?}");
    }
    let number = gd_code(&editor, "_open_number");
    assert!(
        number.iter().any(|line| line
            .contains("_number_note.text = Strings.text(\"rule_number_bounds\", bounds)"))
            && !number
                .iter()
                .any(|line| line.contains("min_value") || line.contains("max_value")),
        "the gateway's bounds are said in the note and never applied by the field (S3's `fixp`): {number:?}"
    );
    let click = gd_code(&editor, "map_click");
    let pick = click
        .iter()
        .position(|line| line.contains("return _pick_at(camera, at)"))
        .expect("a pick takes the click");
    let beacon = click
        .iter()
        .position(|line| line.contains("_beacon_under("))
        .expect("beacons are picked");
    assert!(pick < beacon, "a pick comes before the menus: {click:?}");
    assert!(
        gd_code(&editor, "_pick_at").contains(&"bridge.editor_pick(target)"),
        "the bridge composes the pick"
    );
    assert!(
        gd_code(&editor, "_input").contains(&"bridge.editor_cancel_pick()"),
        "Escape calls a pick off"
    );
}

/// The hostless rule-list scene draws a byte-identical copy of the gateway's own render
/// golden, as the wizard and vista shots draw theirs.
#[test]
fn the_rule_list_shot_draws_a_byte_copy_of_the_gateways_render_golden() {
    let copy =
        fs::read(godot_dir().join("fixtures").join("rule_list_render.json")).expect("the fixture");
    let golden = fs::read(
        godot_dir()
            .join("..")
            .join("tests")
            .join("golden")
            .join("gateway")
            .join("demo_expand_and_mine_render")
            .join("expected.response.json"),
    )
    .expect("the golden");
    assert!(
        copy == golden,
        "godot/fixtures/rule_list_render.json is not the golden's bytes"
    );
    let shot = read("scripts/rule_list_shot.gd");
    assert!(shot.contains("res://fixtures/rule_list_render.json"));
    assert!(shot.contains("bridge.editor_rule_list_of_response("));
    assert!(
        !shot.contains("host_link") && !shot.contains("execute_with_pipe"),
        "the shot starts no host"
    );
}

/// **The route says the gateway's travel figure** (S3-18, S3's `ed3b`): the whole route and
/// each leg are drawn with the text `estimate_route` rendered ("1 min 24 s", a fogged leg
/// labelled as a bound), not the raw milliseconds; the client formats nothing. A figure the
/// gateway sent no text for is drawn as the raw milliseconds it did send, and says so.
#[test]
fn the_route_draws_the_gateways_rendered_travel_text() {
    let editor = read("scripts/editor.gd");
    let route = gd_code(&editor, "_draw_route");
    assert!(
        route
            .iter()
            .any(|line| line.contains("route.get(\"whole_text\", \"\")")),
        "the whole route's text: {route:?}"
    );
    assert!(
        route
            .iter()
            .any(|line| line.contains("route.get(\"leg_texts\", PackedStringArray())")),
        "each leg's text: {route:?}"
    );
    let strings = read("scripts/strings.gd");
    for key in ["route_whole_text", "leg_text", "route_whole", "leg"] {
        assert!(
            strings.contains(&format!("\"{key}\":")),
            "strings.gd has `{key}`"
        );
    }
    assert!(
        !strings.contains(concat!(
            "PLACE",
            "HOLDER: travel times as raw game milliseconds"
        )),
        "S3-18's marker is resolved by the gateway's text"
    );
}

/// **The fix pass's small items** (S3's `ed3b` resume, L4 and contract LOW-4): a ghost that goes
/// while its ground menu shows takes the menu with it, rather than leave "waiting" on its first
/// line; and a bare modifier key (Alt held before an Alt-click) does not take an answer's
/// highlight down.
#[test]
fn a_gone_ghost_closes_its_menu_and_a_modifier_is_not_an_input() {
    let editor = read("scripts/editor.gd");
    let ghost = gd_code(&editor, "_draw_ghost");
    let empty = ghost
        .iter()
        .position(|line| *line == "if ghost.is_empty():")
        .expect("the empty ghost");
    let returned = ghost
        .iter()
        .skip(empty)
        .position(|line| *line == "return")
        .expect("its return");
    assert!(
        ghost
            .iter()
            .skip(empty)
            .take(returned)
            .any(|line| *line == "_menu.hide()"),
        "{ghost:?}"
    );
    let input = gd_code(&editor, "next_input");
    assert!(
        input
            .iter()
            .any(|line| line
                .contains("if event.keycode in [KEY_ALT, KEY_SHIFT, KEY_CTRL, KEY_META]:")),
        "{input:?}"
    );
    let rig = read("scripts/camera_rig.gd");
    assert!(
        rig.contains("## PLACEHOLDER: the opening yaw — OWNER, S6.\nconst OPENING_YAW := 0.8"),
        "the opening yaw carries its marker (L6)"
    );
}

/// **A chip's editor names its chip, and never clamps** (the `ed3b` resume, M1 and L1): an
/// edit names the chip's index, the rendering and its pointer, so it cannot land on a chip of
/// another rendering; no editor opens while the list takes no edit, and the editors close when
/// its rendering is redrawn; the value field shows the file's value as it is, applies no bound, and
/// says the gateway's bounds and when that value lies outside them (S3's `fixp`: the one value
/// field, `value_field.gd`, which holds only text).
#[test]
fn a_chip_editor_names_its_chip_and_never_clamps() {
    let editor = read("scripts/editor.gd");
    let chip = gd_code(&editor, "_on_rule_chip");
    let number = gd_code(&editor, "_open_number");
    assert!(
        gd_code(&editor, "_on_number_send").iter().any(|line| line.contains(
            "bridge.editor_set_number_text(int(_number_chip.get(\"index\", -1)), int(_number_chip.get(\"revision\", -1)), String(_number_chip.get(\"pointer\", \"\")), text)"
        )),
        "an edit names its chip's index, rendering and pointer (M1), and sends the text as typed (fixp)"
    );
    assert!(
        gd_code(&editor, "_on_choice").iter().any(|line| line.contains(
            "bridge.editor_choose(int(_choice_chip.get(\"index\", -1)), int(_choice_chip.get(\"revision\", -1)), String(_choice_chip.get(\"pointer\", \"\")), which)"
        ))
    );
    assert!(
        gd_code(&editor, "_on_rule_move")
            .contains(&"bridge.editor_move_rule(from, from_pointer, to, to_pointer, rendering)")
    );
    // M1: no editor opens while the list takes no edit, and the editors close with the list.
    assert!(
        chip.iter()
            .position(|line| line.contains("if _rules_blocked != \"\":"))
            .is_some_and(|blocked| chip
                .get(blocked.saturating_add(1))
                .is_some_and(|line| line
                    .contains("_say_local(\"status_rule_refused_\" + _rules_blocked, {})"))),
        "the bridge's reason is said, and no editor opens: {chip:?}"
    );
    let rules = gd_code(&editor, "_draw_rules");
    for wanted in [
        "if redrawn and rendering_moved:",
        "_number_popup.hide()",
        "_choice_menu.hide()",
    ] {
        assert!(
            rules.iter().any(|line| line.contains(wanted)),
            "{wanted}: {rules:?}"
        );
    }
    // L1, fixp: no float and no bound in the client; the text goes to the bridge as typed.
    assert!(
        number.iter().any(|line| line.contains(
            "_number_field.open(String(chip.get(\"value_json\", \"\")), String(chip.get(\"unit\", \"\")), String(chip.get(\"text\", \"\")))"
        )),
        "{number:?}"
    );
    assert!(
        !editor.contains("SpinBox") && !editor.contains("set_bounds"),
        "no SpinBox and no client bounds are left"
    );
    let check = read("scripts/client_check.gd");
    assert!(
        check.contains("_check_the_value_field(stage, editor, failures)")
            && check.contains("_check_the_refused_ghost_line(editor, failures)"),
        "the client check runs both hostless checks"
    );
    assert!(
        number
            .iter()
            .any(|line| line.contains("_number_note.visible = outside or bounded"))
            && number
                .iter()
                .any(|line| line.contains("rule_number_outside")),
        "a value outside the bounds sent is shown and said, never clamped: {number:?}"
    );
}

/// **The condition picker draws the vocabulary as sent** (S3's `ed3b`, part 2): every family,
/// predicate and parameter is the bridge's `vocabulary`, a locked one greyed with the gateway's
/// reason and not selectable, and Use asks the bridge for the one patch, naming the rule, the
/// rendering and the slot. The hostless scene draws the gateway's golden and starts no host.
#[test]
fn the_picker_draws_the_vocabulary_as_sent_and_a_lock_with_its_reason() {
    let picker = read("scripts/condition_picker.gd");
    let fill = gd_code(&picker, "fill");
    for wanted in [
        "item.set_selectable(0, locked == \"\" and family_locked == \"\")",
        "item.set_text(0, Strings.text(\"picker_entry_locked\", {\"title\": entry.get(\"title\", \"\"), \"locked\": locked}))",
        "item.set_metadata(0, [family_index, entry_index])",
    ] {
        assert!(fill.contains(&wanted), "{wanted}: {fill:?}");
    }
    let editor = read("scripts/editor.gd");
    assert!(
        gd_code(&editor, "_on_picker_use").iter().any(|line| line.contains(
            "bridge.editor_choose_predicate(int(_picker_slot.get(\"slot\", -1)), String(_picker_slot.get(\"pointer\", \"\")), int(_picker_slot.get(\"revision\", -1)), int(picked[0]), int(picked[1]))"
        )),
        "the pick names the slot by index and pointer, and the rendering"
    );
    let open = gd_code(&editor, "open_condition_picker");
    assert!(
        open.iter()
            .position(|line| line.contains("if _rules_blocked != \"\":"))
            .is_some_and(|blocked| open
                .get(blocked.saturating_add(1))
                .is_some_and(|line| line
                    .contains("_say_local(\"status_rule_refused_\" + _rules_blocked, {})"))),
        "no picker opens while the list takes no edit: {open:?}"
    );
    let rules = gd_code(&editor, "_draw_rules");
    assert!(
        rules
            .iter()
            .any(|line| line.contains("_picker_popup.hide()")),
        "the picker closes with the list it was opened on: {rules:?}"
    );
    let list = read("scripts/rule_list.gd");
    let slots = gd_code(&list, "_slots");
    assert!(
        slots.iter().any(|line| line.contains(
            "if _index(slot.get(\"rule\", -1), unread) != rule or not slot.get(\"pickable\", false):"
        )),
        "a button only on a slot the bridge calls pickable (no group, no unknown op): {slots:?}"
    );
    assert!(
        slots.iter().any(|line| line.contains(
            "button.text = Strings.text(\"rule_slot\", {\"title\": slot.get(\"title\", \"\")})"
        )),
        "the slot is named in the gateway's words: {slots:?}"
    );
    assert!(
        rules
            .iter()
            .any(|line| line.contains("Callable(self, \"open_condition_picker\")")),
        "the rule list opens the picker on a slot: {rules:?}"
    );
    let shot = read("scripts/picker_shot.gd");
    assert!(shot.contains("res://fixtures/vocabulary.json"));
    assert!(shot.contains("bridge.editor_vocabulary_of_response("));
    assert!(
        !shot.contains("host_link") && !shot.contains("execute_with_pipe"),
        "the shot starts no host"
    );
}

/// **A name chip and the Visit & change rows go through the bridge** (S3's `ed3b`, part 2): a
/// step-label or rule-id chip offers the names the bridge read from the text and sends the
/// pick's index; each Visit & change row is the bridge's (`editor_visit_row`,
/// `editor_change_setting_choice`, `editor_change_setting_number_text`); an anchor pick takes the
/// next map click, and Escape stops it.
#[test]
fn a_name_chip_and_the_visit_rows_go_through_the_bridge() {
    let editor = read("scripts/editor.gd");
    let chip = gd_code(&editor, "_on_rule_chip");
    assert!(
        chip.contains(&"\"name\":") && chip.contains(&"_open_names(named, chip)"),
        "{chip:?}"
    );
    assert!(
        gd_code(&editor, "_on_name").iter().any(|line| line.contains(
            "bridge.editor_choose_name(int(_name_chip.get(\"index\", -1)), int(_name_chip.get(\"revision\", -1)), String(_name_chip.get(\"pointer\", \"\")), which)"
        ))
    );
    // Each Visit & change pick is the bridge's row, and closes the map menus first (S3's `fixp`,
    // P6b finding 3; `client_check.gd`'s `_check_an_action_pick_closes_the_menus` drives it).
    let visit_row = gd_code(&editor, "_on_visit_row");
    assert!(
        visit_row.first().copied() == Some("close_menu()")
            && visit_row.contains(&"bridge.editor_visit_row(_visit_beacon, row, which)"),
        "a mandate or a blueprint is the bridge's row, and the menus close: {visit_row:?}"
    );
    let setting_choice = gd_code(&editor, "_on_setting_choice");
    assert!(
        setting_choice.first().copied() == Some("close_menu()")
            && setting_choice.contains(
                &"bridge.editor_change_setting_choice(_visit_beacon, which, pointer, value)"
            ),
        "an enum setting's value is the bridge's row, and the menus close: {setting_choice:?}"
    );
    let on_menu = gd_code(&editor, "_on_menu");
    let target_return = on_menu
        .iter()
        .position(|line| *line == "return")
        .expect("the target choice returns");
    let closed = on_menu
        .iter()
        .position(|line| *line == "close_menu()")
        .expect("the menus close");
    let first_action = on_menu
        .iter()
        .position(|line| line.starts_with("act(") || line.starts_with("open_entry_list("))
        .expect("an action");
    assert!(
        target_return < closed && closed < first_action,
        "every action pick but a target choice closes the map menus first: {on_menu:?}"
    );
    // The vocabulary's lists open as the editor's own entry list (P6b finding 5), a locked entry
    // greyed with the gateway's sentence wrapped under it; client_check.gd measures it.
    let entry = gd_code(&editor, "_entry");
    for wanted in [
        "pick.disabled = true",
        "why.text = Strings.text(\"menu_entry_locked\", {\"locked\": locked})",
        "why.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART",
    ] {
        assert!(entry.contains(&wanted), "{wanted}: {entry:?}");
    }
    assert!(
        gd_code(&editor, "_on_entry")
            .contains(&"bridge.editor_visit_row(_entry_beacon, _entry_row, which)"),
        "an entry is the bridge's row"
    );
    assert!(
        read("scripts/client_check.gd")
            .contains("await _check_the_entry_list(stage, editor, failures)"),
        "the client check measures the entry list"
    );
    assert!(
        read("scripts/client_check.gd")
            .contains("await _check_an_action_pick_closes_the_menus(stage, editor, failures)"),
        "the client check drives the picks"
    );
    assert!(
        gd_code(&editor, "_on_number_send")
            .iter()
            .any(|line| line.contains("bridge.editor_change_setting_number_text(")),
        "a number setting's value is the bridge's row"
    );
    let click = gd_code(&editor, "map_click");
    assert!(
        click
            .iter()
            .any(|line| line.contains("if picking_on(bridge.editor_state()):")),
        "an anchor pick takes the next map click: {click:?}"
    );
    assert!(
        gd_code(&editor, "picking_on")
            .iter()
            .any(|line| line.contains("state.get(\"anchoring\", false)")),
        "Escape stops an anchor pick as it stops a chip's"
    );
    assert!(
        gd_code(&editor, "_build_picker")
            .iter()
            .any(|line| line.contains("bridge.editor_vocabulary_again()")),
        "a refused vocabulary can be asked for again from the picker"
    );
    assert!(
        gd_code(&editor, "_fill_visit_menu")
            .iter()
            .any(|line| line.contains(
                "_visit_menu.set_item_checked(_visit_menu.item_count - 1, now == String(row[1]))"
            )),
        "the priority the beacon's summary sends is marked"
    );
}

/// **Every popup the editor owns draws the one opaque panel** (S3's `fixp`, P6b finding 6): each
/// `PopupPanel.new()` and `PopupMenu.new()` in `editor.gd` is given it on the next line, and the
/// client check reads every popup's resolved panel.
#[test]
fn every_popup_the_editor_owns_is_opaque() {
    let editor = read("scripts/editor.gd");
    let lines: Vec<&str> = editor.lines().map(str::trim).collect();
    let mut made = 0_usize;
    for (at, line) in lines.iter().enumerate() {
        let Some((name, _)) = line
            .strip_prefix("var ")
            .unwrap_or(line)
            .split_once(" := PopupPanel.new()")
            .or_else(|| {
                line.strip_prefix("var ")
                    .unwrap_or(line)
                    .split_once(" := PopupMenu.new()")
            })
            .or_else(|| line.split_once(" = PopupPanel.new()"))
            .or_else(|| line.split_once(" = PopupMenu.new()"))
        else {
            continue;
        };
        made = made.saturating_add(1);
        assert_eq!(
            lines.get(at.saturating_add(1)).copied(),
            Some(format!("opaque({name})").as_str()),
            "{name} is made without the editor's panel"
        );
    }
    assert!(made >= 8, "the scan found {made} popups");
    assert!(
        read("scripts/client_check.gd").contains("_check_the_popups_are_opaque(editor, failures)")
    );
}

/// **Build targets are marked in the Lull** (S3's `fixp`, P6b finding 7): the panel's redraw draws
/// the bridge's `build_targets`, and the client check counts the marks.
#[test]
fn the_build_targets_are_marked_from_the_bridges_reading() {
    let editor = read("scripts/editor.gd");
    assert!(
        gd_code(&editor, "_draw_state").contains(&"_draw_build(state.get(\"build_targets\", []))")
    );
    let build = gd_code(&editor, "_draw_build");
    assert!(
        build
            .iter()
            .any(|line| line
                .contains("_box_outline(boxes, target[\"box_min\"], target[\"box_max\"])")),
        "a wall's box is outlined as written: {build:?}"
    );
    assert!(
        read("scripts/client_check.gd").contains("await _check_the_build_marks(editor, failures)")
    );
}

/// **No click on the dormant box is lost without a word** (S3's `fixp`, P6b finding 9): the box
/// is disabled only by the bridge's `enabled`, which only the shut door clears, every click goes
/// to the bridge, a refusal is drawn beside the box, and `--log-input` logs the box's presses,
/// its toggles and, for each press the lobby logs, whether it fell inside the box.
#[test]
fn every_click_on_the_dormant_box_reaches_the_bridge() {
    let editor = read("scripts/editor.gd");
    assert!(
        gd_code(&editor, "_on_dormant_toggled")
            .contains(&"var taken: bool = bridge.editor_set_allow_dormant(on)")
    );
    assert!(gd_code(&editor, "_draw_dormant").iter().any(|line| {
        line.contains("_dormant_said.text = Strings.text(\"status_dormant_refused_\" + refused)")
    }));
    assert!(
        gd_code(&editor, "_on_dormant_down")
            .contains(&"print(\"[editor] allow dormant: button_down\")")
    );
    assert!(
        gd_code(&read("scripts/lobby.gd"), "_input")
            .iter()
            .any(|line| line.contains("editor.dormant_log(event.position)")),
        "the lobby's press log says where the box was"
    );
    assert!(
        read("scripts/client_check.gd")
            .contains("await _check_the_dormant_box(stage, editor, failures)")
    );
}

/// **No integer reader can fail at run time** (S3's `fixp`, the `godot_int` sweep's GDScript
/// half): no script reads a count the bridge sends through `int_variant` with a bare `int(...)`,
/// or types `caught_panics()` as an `int`; each goes through `values.gd`'s `int_of`, and the
/// client check feeds every drawing path the bridge's refusal.
#[test]
fn every_count_the_bridge_sends_is_read_through_int_of() {
    let keys = [
        "changes",
        "revision",
        "undo_depth",
        "answers",
        "picking",
        "step",
        "pending",
        "index",
        "rule",
        "chip",
        "earlier",
        "later",
    ];
    let mut found = Vec::new();
    for script in SCRIPTS {
        let text = read(&format!("scripts/{script}"));
        for (number, line) in text.lines().enumerate() {
            let code = line.split('#').next().unwrap_or_default();
            if code.contains("caught_panics()") && code.contains(": int =") {
                found.push(format!(
                    "{script}:{}: {}",
                    number.saturating_add(1),
                    line.trim()
                ));
            }
            // The bridge's own dictionaries, under the names the scripts give them; the editor's
            // own (`_number_chip`, a drag's `data`) hold ints it put there itself.
            for receiver in [
                "state",
                "s",
                "meter",
                "recap",
                "chip",
                "target",
                "fix",
                "line",
                "segment",
                "conflict",
                "refusal",
                "slot",
                "drained",
                "_editor()",
                "picking",
            ] {
                for key in keys {
                    if code.contains(&format!("int({receiver}.get(\"{key}\"")) {
                        found.push(format!(
                            "{script}:{}: {}",
                            number.saturating_add(1),
                            line.trim()
                        ));
                    }
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "a count read with a bare int(): {found:#?}"
    );
    assert!(
        read("scripts/client_check.gd").contains("_check_the_unreadable_counts(editor, failures)")
    );
}

// --- S3's `prob`, part 1: the Probation lobby, the guided pick, the memos, the ring --------

/// The New match button names the preset New match writes, through the string table's
/// `preset_<name>` row fed `HostLink.PRESET` itself (no second literal), and the table has a
/// row for every preset the schema names; the status line names the hosted preset through the
/// same rows. Nothing says what a preset does.
#[test]
fn the_new_match_button_names_the_preset_it_writes_from_the_one_constant() {
    let lobby = read("scripts/lobby.gd");
    let strings = read("scripts/strings.gd");
    let text = gd_function(&lobby, "new_match_text");
    assert!(
        text.iter()
            .any(|line| line.contains("preset_name(HostLink.PRESET)")),
        "the label is the constant the lobby writes: {text:?}"
    );
    let build = gd_function(&lobby, "_build_ui");
    assert!(
        build
            .iter()
            .any(|line| line.contains("_button(_chooser, new_match_text(), new_match)")),
        "the chooser's New match button carries the label: {build:?}"
    );
    let name = gd_function(&lobby, "preset_name");
    assert!(
        name.iter()
            .any(|line| line.contains(r#"Strings.text("preset_" + wire)"#)),
        "a preset is named by its wire name's row: {name:?}"
    );
    assert!(string_row(&strings, "lobby_new_match").contains("{preset}"));
    // The shipped smoke check presses the button by the lobby's own words, so the two never
    // drift apart (the package's smoke run found the old fixed label missing).
    assert!(
        read("scripts/smoke_check.gd").contains("_chooser_button(Lobby.new_match_text())"),
        "the smoke check finds New match by the lobby's label"
    );
    for preset in pharmakos_client_gdext::rig::MatchPreset::ALL {
        let row = string_row(&strings, &format!("preset_{}", preset.name()));
        assert!(
            !row.contains('{'),
            "a name, with nothing put into it: {row}"
        );
    }
    let host_link = read("scripts/host_link.gd");
    let written = host_link
        .lines()
        .find(|line| line.starts_with(r#"const PRESET := ""#))
        .expect("host_link.gd's PRESET");
    let value = written
        .trim_start_matches(r#"const PRESET := ""#)
        .trim_end_matches('"');
    assert!(
        pharmakos_client_gdext::rig::MatchPreset::of_name(value).is_some(),
        "PRESET is a preset the schema names: {value}"
    );
    // The status line: the gateway's preset through the same rows, a disagreement said.
    let line = gd_function(&lobby, "preset_line");
    for needle in [
        "preset_name(served)",
        r#""lobby_preset_disagrees""#,
        r#""lobby_preset_none""#,
        r#""lobby_status_" + failed"#,
    ] {
        assert!(
            line.iter().any(|one| one.contains(needle)),
            "`{needle}` in preset_line: {line:?}"
        );
    }
    let start = gd_function(&lobby, "_start");
    assert!(
        start
            .iter()
            .any(|one| one.contains("vista.bridge.watch_expect_preset(HostLink.PRESET)")),
        "New match holds the gateway's answer to the preset it wrote: {start:?}"
    );
}

/// A refused Resume, whatever its cause, shows the host's refusal as it came and offers the
/// chooser again with New match and no Resume (S3's plan, E6). The lobby reads nothing but
/// "a Resume was refused before an announce": no word of the refusal and no field of the line.
/// The host that refused is let go through `host_link.gd`'s `reset`, so New match starts
/// another; the remembered line stays until New match announces.
#[test]
fn a_refused_resume_shows_the_refusal_and_offers_new_match_only() {
    let lobby = read("scripts/lobby.gd");
    let failed = gd_function(&lobby, "_on_failed");
    let branch = failed
        .iter()
        .position(|line| line.trim() == "if _resuming and not _announced:")
        .expect("_on_failed branches on a refused Resume");
    assert!(
        failed
            .iter()
            .take(branch)
            .any(|line| line.contains(r#"Strings.text(key, {"reason": reason})"#)),
        "the refusal is shown as it came, first: {failed:?}"
    );
    assert!(
        failed
            .iter()
            .skip(branch)
            .take(4)
            .any(|line| line.trim() == "_offer_new_match_only()"),
        "{failed:?}"
    );
    for line in &failed {
        let code = line.trim();
        assert!(
            code.starts_with('#') || !(code.contains("reason.") || code.contains("in reason")),
            "the lobby reads no word of the refusal: {code}"
        );
    }
    let offer = gd_function(&lobby, "_offer_new_match_only");
    for needle in [
        "link.reset()",
        "_started = false",
        "_resuming = false",
        "_resume_button.visible = false",
        "_chooser.visible = true",
    ] {
        assert!(
            offer.iter().any(|line| line.trim().ends_with(needle)),
            "`{needle}` in _offer_new_match_only: {offer:?}"
        );
    }
    assert!(
        !offer
            .iter()
            .any(|line| line.contains("REMEMBERED") || line.contains("_forget")),
        "a refusal it cannot type loses no remembered match: {offer:?}"
    );
    let announced = gd_function(&lobby, "_on_announced");
    assert!(
        announced
            .iter()
            .any(|line| line.trim() == "_announced = true")
    );
    // The restart clause of the old PLACEHOLDER is gone with the behaviour it described.
    assert!(
        !lobby.contains("restarts the client"),
        "the lobby no longer asks a player to restart after a refused Resume"
    );
    // The link lets go of everything it held of the refused host, its tokens included.
    let host_link = read("scripts/host_link.gd");
    let reset = gd_function(&host_link, "reset");
    for needle in [
        "quit()",
        "_pid = -1",
        "_port = 0",
        "_done = false",
        r#"_stderr_text = """#,
    ] {
        assert!(
            reset.iter().any(|line| line.trim() == needle),
            "`{needle}` in reset: {reset:?}"
        );
    }
    let quit = gd_function(&host_link, "quit");
    assert!(
        quit.iter()
            .any(|line| line.trim() == r#"_tokens = ["", ""]"#),
        "reset forgets the tokens through quit: {quit:?}"
    );
}

/// The Ledger's memos: a feed row the bridge marks as the Ledger's is drawn under the speaker
/// word from the string table in place of the kind, and the recap draws the memos, each as the
/// gateway wrote it, under a heading only when there are any. The licence line is the prose's
/// own sentence, drawn whole with it.
#[test]
fn the_lobby_draws_the_ledgers_memos_as_the_gateway_wrote_them() {
    let lobby = read("scripts/lobby.gd");
    let strings = read("scripts/strings.gd");
    let row = gd_function(&lobby, "event_text_of");
    assert!(
        row.iter()
            .any(|line| line.contains(r#"row.get("ledger", false)"#))
            && row.iter().any(|line| line.contains(r#""event_ledger""#)),
        "{row:?}"
    );
    assert!(string_row(&strings, "event_ledger").contains("The Ledger"));
    assert!(string_row(&strings, "event_ledger").contains("{text}"));
    let received = gd_function(&lobby, "_on_received");
    assert!(
        received
            .iter()
            .any(|line| line.contains("_rows.append(event_text_of(row))")),
        "{received:?}"
    );
    let recap = gd_function(&lobby, "recap_text_of");
    let heading = recap
        .iter()
        .position(|line| line.contains(r#""lobby_recap_memos""#))
        .expect("the memos' heading");
    assert!(
        recap
            .get(heading.saturating_sub(1))
            .is_some_and(|line| line.contains("if not memos.is_empty():")),
        "no heading without memos: {recap:?}"
    );
    assert!(
        recap
            .iter()
            .any(|line| { line.contains(r#""lobby_recap_memo", {"text": memo.get("text", "")}"#) }),
        "{recap:?}"
    );
    assert!(
        recap
            .iter()
            .any(|line| line.contains(r#""prose": recap.get("prose", "")"#)),
        "the prose, licence sentence and all, is drawn whole: {recap:?}"
    );
    assert!(string_row(&strings, "lobby_recap").contains("{prose}"));
}

/// The ring: the vista draws a beacon's ring at the radius the view sent, read through
/// `Values.int_of`, and draws none where the view sent none; it names no subtype.
#[test]
fn the_vista_draws_a_beacons_ring_at_the_radius_the_view_sent() {
    let vista = read("scripts/vista.gd");
    let set = gd_function(&vista, "_set_ring");
    assert!(
        set.iter()
            .any(|line| line.contains(r#"entity.has("ring")"#))
            && set
                .iter()
                .any(|line| line.contains(r#"Values.int_of(entity["ring"])"#)),
        "{set:?}"
    );
    let show = gd_function(&vista, "_show");
    assert!(
        show.iter()
            .any(|line| line.trim() == "_set_ring(marker, entity)")
    );
    let ring = gd_function(&vista, "_ring");
    assert!(
        ring.iter()
            .any(|line| line.contains("band.inner_radius = float(radius)")),
        "the ring's size is the radius as sent: {ring:?}"
    );
    for line in set.iter().chain(&ring) {
        assert!(
            !line.contains("subtype"),
            "the ring names no subtype: {line}"
        );
    }
}

/// The hostless shots `prob` adds draw byte copies of gateway goldens, and each copy is its
/// golden byte for byte.
#[test]
fn the_probation_fixtures_are_the_gateways_goldens() {
    for (copy, golden) in [
        (
            "get_status_probation.json",
            "get_status_probation/expected.response.json",
        ),
        (
            "recap_probation_round.json",
            "recap_probation_round/expected.response.json",
        ),
        (
            "recap_probation_ended.json",
            "recap_probation_ended/expected.response.json",
        ),
        ("view_built.jsonl", "view_built/expected.keyframe.jsonl"),
    ] {
        let ours = fs::read(godot_dir().join("fixtures").join(copy)).expect("the copy");
        let theirs = fs::read(
            godot_dir()
                .join("..")
                .join("tests")
                .join("golden")
                .join("gateway")
                .join(golden),
        )
        .expect("the golden");
        assert!(
            ours == theirs,
            "godot/fixtures/{copy} is not a byte-identical copy of tests/golden/gateway/{golden}: \
             the gateway is the producer, so the pull request that moves the golden re-copies it"
        );
    }
    for (script, fixture) in [
        (
            "scripts/lobby_shot.gd",
            "res://fixtures/get_status_probation.json",
        ),
        (
            "scripts/lobby_shot.gd",
            "res://fixtures/feed_ledger_memo.json",
        ),
        (
            "scripts/lobby_shot.gd",
            "res://fixtures/recap_probation_round.json",
        ),
        ("scripts/ring_shot.gd", "res://fixtures/view_built.jsonl"),
        ("scripts/ring_shot.gd", "res://fixtures/view_keyframe.jsonl"),
    ] {
        assert!(read(script).contains(fixture), "{script} draws {fixture}");
    }
}

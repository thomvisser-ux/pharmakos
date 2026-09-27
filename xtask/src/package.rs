// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask package`: the unsigned zip for this platform (skeleton plan
//! T21; decisions-log item 117).
//!
//! Not a step: `cargo xtask ci` and `cargo xtask list` do not know it, like
//! `ci-scope` and `perf-alarms`. CI's `package (<os>)` jobs run it and nothing
//! else of substance, so every check below also runs on a developer's machine
//! (item 117 (8)). It fails, never skips: a missing Godot, export template,
//! `reuse`, `objdump` (Linux) or golden is a failure with the reason, and on
//! macOS it refuses, because there is no macOS package (items 8 and 117 (14)).
//!
//! In this order:
//!
//! 1. resolve the target directory from `cargo metadata`, and empty the
//!    folders it writes, all under `<target>/package/` (every path it removes
//!    is asserted to have that prefix);
//! 2. build the client library with `--profile release-client` (item 117 (4))
//!    and `gamectl` with `--release`, both for the host triple; on Windows with
//!    `+crt-static` through `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS`, for
//!    these two builds only, refusing when `RUSTFLAGS` or
//!    `CARGO_ENCODED_RUSTFLAGS` would replace it (item 117 (5));
//! 3. on Windows, byte-scan both binaries for the C runtime's DLL names, and
//!    fail on a hit;
//! 4. copy `godot/` without `.godot/` and `bin/` into the stage, stage the
//!    library in the copy's `bin/`, byte-scan it for `gdext_rust_init`, and run
//!    the import on the copy (item 72: the import runs first in every
//!    packaging step);
//! 5. run the smoke check once in the imported copy with the editor binary,
//!    failing on a non-zero exit, a missing `[smoke] OK`, any `SCRIPT ERROR`
//!    line (the debug VM reports what the release VM crashes on) or a line
//!    holding a 64-hex run, the shape of a seat token;
//! 6. export the platform's preset headless into an emptied folder, requiring
//!    exit 0 (a failed export leaves files behind), except for Godot's crash at
//!    exit after its `savepack` line with all three outputs present, which is a
//!    warning naming item 102 (2);
//! 7. byte-scan the `.pck` for test files that must not ship, and for the smoke
//!    check that must;
//! 8. assemble `Pharmakos/`: the export, `gamectl`, `rules/`, `library/`, the
//!    licences, the zip's REUSE manifest, the note, the changelog and the
//!    third-party notices; on Linux read the glibc floor from the three ELF
//!    files with `objdump -T` first;
//! 9. write the zip from `Pharmakos/` alone ([`crate::zip`]), read its central
//!    directory back, and compare that list with `tests/golden/package/`;
//! 10. extract the zip with the same reader where no repository is above it,
//!     and run `reuse lint` over it (acceptance (c));
//! 11. replay every committed scenario with the packaged `gamectl`, which
//!     compares each chain with its golden itself.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use crate::{
    CLIENT_PACKAGE, CLIENT_STAGE_DIR, Ctx, ENTRY_SYMBOL, GDEXTENSION_FILE, GODOT_PROJECT_DIR,
    SCENARIO_BIN, contains_bytes, describe_exit, find_bytes, first_difference, godot_program,
    import_project, metadata, render_command, run, scenario, tool_available, walk, zip,
};

/// The version string of every zip until the wk-35.5 release (item 117 (2)).
/// `[workspace.package] version` stays `0.0.0`.
pub(crate) const VERSION: &str = "0.1.0-dev+skeleton";

/// The folder the zip holds, and the zip's only top-level entry.
const FOLDER: &str = "Pharmakos";

/// Everything `package` writes lives under `<target>/package/`.
const PACKAGE_DIR: &str = "package";

/// The Godot version whose export templates the export needs.
const TEMPLATE_VERSION: &str = "4.7.2.stable";

/// The scene the smoke check runs, through `boot.gd`'s `--scene=`.
const SMOKE_SCENE: &str = "res://scenes/smoke_check.tscn";

/// The smoke check's one pass line.
const SMOKE_OK: &str = "[smoke] OK";

/// The note, the notices and the changelog at the zip's root.
const NOTE_FILE: &str = "README.txt";
const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES.txt";
const CHANGELOG_FILE: &str = "CHANGELOG.md";

/// Where the templates live in the repository.
const PACKAGING_DIR: &str = "packaging";
const MANIFEST_TEMPLATE: &str = "REUSE.toml.in";

/// The manifest golden's area (a self-compared area, [`crate::golden`]).
const GOLDEN_AREA: [&str; 3] = ["tests", "golden", "package"];

/// The C runtime's DLL names, lower-case (item 117 (5)). `api-ms-win-core-*`
/// and `bcryptprimitives.dll` are the operating system's and are not here.
const CRT_NEEDLES: &[&str] = &["vcruntime", "msvcp", "ucrtbase", "api-ms-win-crt-"];

/// The Windows target's rustflags variable, which carries `+crt-static` into
/// `package`'s own two builds and nowhere else.
const CRT_STATIC_VARIABLE: &str = "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS";
const CRT_STATIC_FLAGS: &str = "-C target-feature=+crt-static";

/// The godot-rust crates: MPL-2.0, and shipping no licence file of their own.
/// The first four are linked into the client library; the last three run at
/// build time and their generated code is compiled into godot-core and
/// godot-ffi (item 117 (11)).
const GODOT_RUST_LINKED: &[&str] = &["godot", "godot-cell", "godot-core", "godot-ffi"];
const GODOT_RUST_BUILD: &[&str] = &["godot-bindings", "godot-codegen", "gdextension-api"];

/// One platform's names.
#[derive(Debug)]
struct Platform {
    /// `windows` or `linux`: the golden's, the note's and the zip's name.
    name: &'static str,
    /// The name people read: `Windows` or `Linux`.
    display: &'static str,
    /// The host triple both binaries are built for.
    triple: &'static str,
    /// The export preset in `godot/export_presets.cfg`.
    preset: &'static str,
    /// The exported executable.
    executable: &'static str,
    /// The client library, as Godot copies it beside the executable.
    library: &'static str,
    /// `gamectl`.
    gamectl: &'static str,
    /// The release export template Godot reads.
    template: &'static str,
}

const WINDOWS: Platform = Platform {
    name: "windows",
    display: "Windows",
    triple: "x86_64-pc-windows-msvc",
    preset: "Windows Desktop",
    executable: "Pharmakos.exe",
    library: "pharmakos_client_gdext.dll",
    gamectl: "gamectl.exe",
    template: "windows_release_x86_64.exe",
};

const LINUX: Platform = Platform {
    name: "linux",
    display: "Linux",
    triple: "x86_64-unknown-linux-gnu",
    preset: "Linux",
    executable: "Pharmakos.x86_64",
    library: "libpharmakos_client_gdext.so",
    gamectl: "gamectl",
    template: "linux_release.x86_64",
};

/// Everything `package` learned, for its summary.
struct Summary {
    lines: Vec<String>,
}

impl Summary {
    fn note(&mut self, line: String) {
        println!("   {line}");
        self.lines.push(line);
    }
}

/// `cargo xtask package`. Returns the summary on success.
pub(crate) fn run_package(ctx: &Ctx) -> Result<String, String> {
    if cfg!(target_os = "macos") {
        return Err(
            "there is no macOS package: signing, notarisation and macOS are hardening's \
             (decisions-log items 8 and 117 (14)), and godot/pharmakos.gdextension declares no \
             macOS library (item 73)"
                .to_owned(),
        );
    }
    let platform = if cfg!(target_os = "windows") {
        &WINDOWS
    } else {
        &LINUX
    };
    let host = host_triple()?;
    if host != platform.triple {
        return Err(format!(
            "the host triple is {host}; `package` builds {} only (item 117 (2): Windows and \
             Linux x86_64)",
            platform.triple
        ));
    }
    if platform.name == "windows" {
        for variable in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"] {
            if env::var_os(variable).is_some() {
                return Err(format!(
                    "{variable} is set, and it would replace {CRT_STATIC_VARIABLE}, which carries \
                     `{CRT_STATIC_FLAGS}` into this command's two builds (decisions-log item \
                     117 (5)); unset {variable} and run it again"
                ));
            }
        }
    }

    let godot = godot_program();
    if !tool_available(&godot) {
        return Err(format!(
            "`{godot}` is not installed ($PHARMAKOS_GODOT overrides the name); the package is \
             Godot's export and cannot be made without it"
        ));
    }
    let template = template_dir()?.join(platform.template);
    if !template.is_file() {
        return Err(format!(
            "the release export template {} is missing; the package jobs in \
             .github/workflows/ci.yml install it from Godot's checked release archive",
            template.display()
        ));
    }
    if !tool_available("reuse") {
        return Err(
            "the reuse tool is not installed (`pipx install reuse==6.2.0`); the zip's licence \
             check is acceptance (c) and cannot be skipped"
                .to_owned(),
        );
    }
    if platform.name == "linux" && !tool_available("objdump") {
        return Err(
            "objdump is not installed (binutils); the note states the glibc floor it reads"
                .to_owned(),
        );
    }

    let workspace = metadata(ctx)?;
    let target = workspace.target_dir.clone();
    let base = target.join(PACKAGE_DIR);
    let stage = base.join("stage");
    let check = base.join("check");
    let logs = base.join("logs");
    let dist = base.join("dist");
    for folder in [&stage, &check, &logs, &dist] {
        reset_dir(&base, folder)?;
    }
    let mut summary = Summary { lines: Vec::new() };
    let commit = commit_stamp(ctx)?;
    summary.note(format!("{VERSION}, {}", commit.line));

    // 2. The two binaries.
    let (library, gamectl) = build(ctx, platform, &target)?;

    // 3. The C runtime (Windows).
    if platform.name == "windows" {
        for binary in [&library, &gamectl] {
            let hits = crt_hits(&read(binary)?);
            if !hits.is_empty() {
                return Err(format!(
                    "{} names the C runtime, so it would not start on a clean Windows without the \
                     Visual C++ runtime; `{CRT_STATIC_FLAGS}` did not take (decisions-log item \
                     117 (5)):\n      {}",
                    binary.display(),
                    hits.join("\n      ")
                ));
            }
        }
        summary.note(format!(
            "C runtime: neither {} nor {} names {}",
            platform.library,
            platform.gamectl,
            CRT_NEEDLES.join(", ")
        ));
    }

    // 4. The staged Godot project.
    let project = stage.join(GODOT_PROJECT_DIR);
    copy_project(&ctx.root.join(GODOT_PROJECT_DIR), &project)?;
    stage_library_copy(&library, &project, platform)?;
    let import_note = import_project(ctx, &godot, &project)?;
    summary.note(format!("{} staged and {import_note}", platform.library));

    // 5. The smoke check in the editor.
    let smoke_log = logs.join("smoke-editor.log");
    let status = run_logged(
        &godot,
        &[
            "--headless".to_owned(),
            "--path".to_owned(),
            path_arg(&project),
            "--".to_owned(),
            format!("--scene={SMOKE_SCENE}"),
            format!("--gamectl={}", path_arg(&gamectl)),
            format!("--root={}", path_arg(&ctx.root)),
        ],
        &ctx.root,
        &[],
        &smoke_log,
    )?;
    let smoke = judge_smoke(status, &read_text(&smoke_log)?)?;
    summary.note(format!("in-editor smoke check: {smoke}"));

    // 6. The export.
    let out = base.join("out").join(platform.name);
    reset_dir(&base, &out)?;
    let export_log = logs.join("export.log");
    let status = run_logged(
        &godot,
        &[
            "--headless".to_owned(),
            "--path".to_owned(),
            path_arg(&project),
            "--export-release".to_owned(),
            platform.preset.to_owned(),
            path_arg(&out.join(platform.executable)),
        ],
        &ctx.root,
        &[],
        &export_log,
    )?;
    let outputs = [
        platform.executable.to_owned(),
        pck_name(platform),
        platform.library.to_owned(),
    ];
    let present: Vec<bool> = outputs
        .iter()
        .map(|name| out.join(name).is_file())
        .collect();
    let export_text = read_text(&export_log)?;
    match judge_export(status.success(), export_text.contains("savepack"), &present) {
        ExportVerdict::Clean => summary.note(format!(
            "exported the `{}` preset: {}",
            platform.preset,
            outputs.join(", ")
        )),
        ExportVerdict::CrashAtExit => {
            let line = format!(
                "::warning title=package export::Godot exited {} after its savepack line, with {} \
                 all written: the engine's crash at exit with a GDExtension loaded \
                 (decisions-log item 102 (2)); the export is taken",
                describe_exit(status),
                outputs.join(", ")
            );
            summary.note(line);
        }
        ExportVerdict::Failed => {
            print_tail(&export_text, 40);
            return Err(format!(
                "`godot --export-release \"{}\"` failed ({}); its output is in {} (a failed \
                 export leaves files behind, so its exit code is the verdict)",
                platform.preset,
                describe_exit(status),
                export_log.display()
            ));
        }
    }

    // 7. What the .pck may and must hold.
    let pck = read(&out.join(pck_name(platform)))?;
    scan_pck(&pck)?;
    summary.note(format!(
        "{} ({} bytes) names smoke_check and no fixture, shot or other check",
        pck_name(platform),
        pck.len()
    ));
    if platform.name == "windows" {
        let hits = crt_hits(&read(&out.join(platform.executable))?);
        summary.note(format!(
            "C runtime in the Godot executable (reported, not judged): {}",
            if hits.is_empty() {
                "no hit".to_owned()
            } else {
                hits.join("; ")
            }
        ));
    }

    // 8. The tree.
    let tree = stage.join(FOLDER);
    fs::create_dir_all(&tree).map_err(|error| format!("creating {}: {error}", tree.display()))?;
    copy_folder_files(&out, &tree)?;
    copy_file(&gamectl, &tree.join(platform.gamectl))?;
    for name in [
        "rules.v1.json",
        "rules.v1.json.license",
        "README.md",
        "LICENSE",
    ] {
        copy_file(
            &ctx.root.join("rules").join(name),
            &tree.join("rules").join(name),
        )?;
    }
    copy_library(&ctx.root.join("library"), &tree.join("library"))?;
    copy_file(&ctx.root.join(CHANGELOG_FILE), &tree.join(CHANGELOG_FILE))?;

    let glibc = if platform.name == "linux" {
        let floor = glibc_floor(&[
            tree.join(platform.executable),
            tree.join(platform.library),
            tree.join(platform.gamectl),
        ])?;
        summary.note(format!("glibc floor: GLIBC_{floor}"));
        Some(floor)
    } else {
        None
    };

    let licences = licences(ctx, platform)?;
    let notices = notices_text(platform, &commit, &licences)?;
    write_text(&tree.join(NOTICES_FILE), &notices)?;
    let manifest = render_manifest(ctx, platform, &licences)?;
    write_text(&tree.join("REUSE.toml"), &manifest)?;
    let used = licence_ids(&manifest)?;
    copy_licences(ctx, &used, &tree.join("LICENSES"))?;
    summary.note(format!(
        "licences: {}; gamectl `{}`, client library `{}`",
        used.join(", "),
        licences.gamectl_expression,
        licences.client_expression
    ));
    let note = render_note(ctx, platform, &commit, glibc.as_deref())?;
    write_text(&tree.join(NOTE_FILE), &note)?;

    // 9. The zip, and its manifest golden.
    let entries = zip::tree(&tree, FOLDER, &[platform.executable, platform.gamectl])?;
    let zip_path = dist.join(format!("pharmakos-{}-x86_64.zip", platform.name));
    let size = zip::write_file(&entries, &zip_path)?;
    summary.note(format!(
        "{} written: {size} bytes, {} entries",
        zip_path.display(),
        entries.len()
    ));
    let bytes = read(&zip_path)?;
    let listed = zip::list(&bytes)?;
    let manifest_text = manifest_list(&listed);
    compare_manifest(ctx, &target, platform, &manifest_text)?;
    summary.note(format!(
        "manifest matches tests/golden/package/expected.{}.txt",
        platform.name
    ));
    for (name, text) in [
        (NOTE_FILE, &note),
        (NOTICES_FILE, &notices),
        ("REUSE.toml", &manifest),
    ] {
        write_text(&logs.join(name), text)?;
    }
    write_text(
        &logs.join(format!("manifest.{}.txt", platform.name)),
        &manifest_text,
    )?;

    // 10. reuse over the extracted zip.
    let extracted = zip::extract(&bytes, &check)?;
    let reuse_log = logs.join("reuse.json");
    let (reuse_total, expected) = reuse_check(&base, &check.join(FOLDER), &listed, &reuse_log)?;
    summary.note(format!(
        "reuse: {reuse_total} file(s) linted, all with copyright and licence information; the \
         manifest's {} file entries less those reuse never lints is {expected} ({extracted} \
         entries extracted)",
        listed.iter().filter(|entry| !entry.is_directory()).count()
    ));

    // 11. The packaged gamectl replays every committed scenario.
    let packaged = tree.join(platform.gamectl);
    let files = scenario::collect(&ctx.root)?;
    if files.is_empty() {
        return Err("no committed scenario to replay (scenarios/**/*.scenario.jsonc)".to_owned());
    }
    for file in &files {
        let relative = file
            .strip_prefix(&ctx.root)
            .map_err(|error| format!("{}: {error}", file.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        run(
            ctx,
            &path_arg(&packaged),
            &["scenario".to_owned(), "run".to_owned(), relative.clone()],
        )
        .map_err(|error| {
            format!(
                "the packaged {SCENARIO_BIN} disagrees with the committed golden for {relative}: \
                 {error}. The shipped build (release, thin LTO{}) plays a different match from \
                 the one the suite pins; that is a stop, not a re-bless (item 117 (0))",
                if platform.name == "windows" {
                    ", +crt-static"
                } else {
                    ""
                }
            )
        })?;
        summary.note(format!(
            "replayed {relative} with the packaged {SCENARIO_BIN}: ok"
        ));
    }

    Ok(summary.lines.join("\n      "))
}

// ---------------------------------------------------------------------------
// Paths and folders
// ---------------------------------------------------------------------------

/// Removes and recreates `folder`, which must lie strictly inside `base`
/// (`<target>/package/`): `package` deletes nothing anywhere else.
pub(crate) fn reset_dir(base: &Path, folder: &Path) -> Result<(), String> {
    if !is_strictly_inside(base, folder) {
        return Err(format!(
            "refusing to empty {}: `cargo xtask package` removes only what lies inside {}",
            folder.display(),
            base.display()
        ));
    }
    if folder.exists() {
        fs::remove_dir_all(folder)
            .map_err(|error| format!("emptying {}: {error}", folder.display()))?;
    }
    fs::create_dir_all(folder).map_err(|error| format!("creating {}: {error}", folder.display()))
}

/// Whether `path` is below `base`, component by component, and not `base`
/// itself. A `..` anywhere in `path` is never inside.
fn is_strictly_inside(base: &Path, path: &Path) -> bool {
    use std::path::Component;
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return false;
    }
    path != base && path.starts_with(base)
}

/// A path as a command-line argument: forward slashes, which Godot, cargo and
/// the shells on both platforms all read.
fn path_arg(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|error| format!("reading {}: {error}", path.display()))
}

fn read_text(path: &Path) -> Result<String, String> {
    Ok(String::from_utf8_lossy(&read(path)?).replace("\r\n", "\n"))
}

fn write_text(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    }
    fs::write(path, text.as_bytes()).map_err(|error| format!("writing {}: {error}", path.display()))
}

fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    }
    fs::copy(from, to)
        .map_err(|error| format!("copying {} to {}: {error}", from.display(), to.display()))?;
    Ok(())
}

/// Copies the Godot project without its import cache and staging folder, which
/// are the worktree's own and are rebuilt in the copy.
fn copy_project(from: &Path, to: &Path) -> Result<(), String> {
    let mut files: Vec<PathBuf> = Vec::new();
    walk(from, &mut files).map_err(|error| format!("reading {}: {error}", from.display()))?;
    for file in files {
        let relative = file
            .strip_prefix(from)
            .map_err(|error| format!("{}: {error}", file.display()))?;
        let first = relative
            .components()
            .next()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .unwrap_or_default();
        if first == ".godot" || first == CLIENT_STAGE_DIR {
            continue;
        }
        copy_file(&file, &to.join(relative))?;
    }
    Ok(())
}

/// Every file directly in `from`, copied into `to`.
fn copy_folder_files(from: &Path, to: &Path) -> Result<(), String> {
    let mut files: Vec<PathBuf> = Vec::new();
    walk(from, &mut files).map_err(|error| format!("reading {}: {error}", from.display()))?;
    for file in files {
        let relative = file
            .strip_prefix(from)
            .map_err(|error| format!("{}: {error}", file.display()))?;
        copy_file(&file, &to.join(relative))?;
    }
    Ok(())
}

/// `library/`'s templates and its `LICENSE`, and nothing else.
fn copy_library(from: &Path, to: &Path) -> Result<(), String> {
    let mut files: Vec<PathBuf> = Vec::new();
    walk(from, &mut files).map_err(|error| format!("reading {}: {error}", from.display()))?;
    let mut copied = 0_usize;
    for file in files {
        let name = crate::file_name(&file);
        let template = Path::new(&name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonc"));
        if template || name == "LICENSE" {
            copy_file(&file, &to.join(&name))?;
            copied += 1;
        }
    }
    if copied < 2 {
        return Err(format!(
            "{} holds no template; Easy and the wizard read it (item 117 (3))",
            from.display()
        ));
    }
    Ok(())
}

/// Where Godot looks for this version's export templates on this platform.
fn template_dir() -> Result<PathBuf, String> {
    let base = if cfg!(target_os = "windows") {
        PathBuf::from(env::var_os("APPDATA").ok_or_else(|| "APPDATA is not set".to_owned())?)
            .join("Godot")
    } else {
        match env::var_os("XDG_DATA_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir).join("godot"),
            _ => PathBuf::from(env::var_os("HOME").ok_or_else(|| "HOME is not set".to_owned())?)
                .join(".local")
                .join("share")
                .join("godot"),
        }
    };
    Ok(base.join("export_templates").join(TEMPLATE_VERSION))
}

fn pck_name(platform: &Platform) -> String {
    let stem = platform
        .executable
        .rsplit_once('.')
        .map_or(platform.executable, |(stem, _)| stem);
    format!("{stem}.pck")
}

// ---------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------

fn host_triple() -> Result<String, String> {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let output = Command::new(&rustc)
        .arg("-vV")
        .output()
        .map_err(|error| format!("failed to run `{rustc} -vV`: {error}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(|host| host.trim().to_owned())
        .ok_or_else(|| format!("`{rustc} -vV` printed no host line"))
}

fn rustc_version() -> Result<String, String> {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let output = Command::new(&rustc)
        .arg("-V")
        .output()
        .map_err(|error| format!("failed to run `{rustc} -V`: {error}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The environment `package`'s own builds run with: `+crt-static` for the
/// Windows target, and nothing else.
fn build_env(platform: &Platform) -> Vec<(String, String)> {
    if platform.name == "windows" {
        vec![(CRT_STATIC_VARIABLE.to_owned(), CRT_STATIC_FLAGS.to_owned())]
    } else {
        Vec::new()
    }
}

/// Builds the client library and `gamectl` for the host triple; returns both
/// paths.
fn build(ctx: &Ctx, platform: &Platform, target: &Path) -> Result<(PathBuf, PathBuf), String> {
    let environment = build_env(platform);
    let mut client: Vec<String> = [
        "build",
        "--profile",
        "release-client",
        "--target",
        platform.triple,
        "--package",
        CLIENT_PACKAGE,
        "--lib",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    let mut host: Vec<String> = [
        "build",
        "--release",
        "--target",
        platform.triple,
        "--package",
        "pharmakos-gamectl",
        "--bin",
        SCENARIO_BIN,
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    if ctx.locked {
        client.push("--locked".to_owned());
        host.push("--locked".to_owned());
    }
    run_with_env(&ctx.root, &ctx.cargo, &client, &environment)?;
    run_with_env(&ctx.root, &ctx.cargo, &host, &environment)?;
    let library = target
        .join(platform.triple)
        .join("release-client")
        .join(platform.library);
    let gamectl = target
        .join(platform.triple)
        .join("release")
        .join(platform.gamectl);
    for built in [&library, &gamectl] {
        if !built.is_file() {
            return Err(format!(
                "{} was not produced; cargo's target directory is {}",
                built.display(),
                target.display()
            ));
        }
    }
    Ok((library, gamectl))
}

fn run_with_env(
    cwd: &Path,
    program: &str,
    args: &[String],
    environment: &[(String, String)],
) -> Result<(), String> {
    let prefix: Vec<String> = environment
        .iter()
        .map(|(key, value)| format!("{key}=\"{value}\" "))
        .collect();
    println!("   $ {}{}", prefix.concat(), render_command(program, args));
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    for (key, value) in environment {
        command.env(key, value);
    }
    let status = command
        .status()
        .map_err(|error| format!("failed to launch `{program}`: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "`{}` failed ({})",
            render_command(program, args),
            describe_exit(status)
        ))
    }
}

/// Runs a command with its standard output and error both written to `log`.
fn run_logged(
    program: &str,
    args: &[String],
    cwd: &Path,
    environment: &[(String, String)],
    log: &Path,
) -> Result<ExitStatus, String> {
    println!(
        "   $ {}  > {}",
        render_command(program, args),
        log.display()
    );
    let file =
        fs::File::create(log).map_err(|error| format!("creating {}: {error}", log.display()))?;
    let error_file = file
        .try_clone()
        .map_err(|error| format!("{}: {error}", log.display()))?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(error_file));
    for (key, value) in environment {
        command.env(key, value);
    }
    command
        .status()
        .map_err(|error| format!("failed to launch `{program}`: {error}"))
}

/// Stages the release-client library into the copy's `bin/`, checks the
/// extension file names it, and byte-scans it for gdext's entry symbol.
fn stage_library_copy(library: &Path, project: &Path, platform: &Platform) -> Result<(), String> {
    let extension = fs::read_to_string(project.join(GDEXTENSION_FILE))
        .map_err(|error| format!("reading the copy's {GDEXTENSION_FILE}: {error}"))?;
    let reference = format!("res://{CLIENT_STAGE_DIR}/{}", platform.library);
    if !extension.contains(&reference) {
        return Err(format!("{GDEXTENSION_FILE} does not name `{reference}`"));
    }
    let staged = project.join(CLIENT_STAGE_DIR).join(platform.library);
    copy_file(library, &staged)?;
    if !contains_bytes(&read(&staged)?, ENTRY_SYMBOL.as_bytes()) {
        return Err(format!(
            "{} does not contain `{ENTRY_SYMBOL}`",
            staged.display()
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Scans
// ---------------------------------------------------------------------------

/// Every C-runtime name in `bytes`, compared on a lower-cased copy, as `needle
/// at offset: context`. Empty when there is none.
pub(crate) fn crt_hits(bytes: &[u8]) -> Vec<String> {
    let lowered = bytes.to_ascii_lowercase();
    let mut hits: Vec<String> = Vec::new();
    for needle in CRT_NEEDLES {
        if !contains_bytes(&lowered, needle.as_bytes()) {
            continue;
        }
        let mut from = 0_usize;
        while let Some(found) = lowered
            .get(from..)
            .and_then(|rest| find_bytes(rest, needle.as_bytes()))
        {
            let at = from + found;
            hits.push(format!("`{needle}` at byte {at}: {}", context(bytes, at)));
            from = at + needle.len();
            if hits.len() >= 20 {
                return hits;
            }
        }
    }
    hits
}

/// About 32 bytes around `at`, printable ASCII as itself and anything else as
/// a dot.
fn context(bytes: &[u8], at: usize) -> String {
    let start = at.saturating_sub(8);
    let end = (at + 24).min(bytes.len());
    bytes
        .get(start..end)
        .unwrap_or_default()
        .iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                '.'
            }
        })
        .collect()
}

/// The `.pck` must name the smoke check and must not name a fixture, a shot or
/// any other check (item 117 (6)); the manifest golden cannot see inside it.
pub(crate) fn scan_pck(pck: &[u8]) -> Result<(), String> {
    let mut problems: Vec<String> = Vec::new();
    for banned in ["fixtures/", "_shot."] {
        if let Some(at) = find_bytes(pck, banned.as_bytes()) {
            problems.push(format!(
                "names `{banned}` at byte {at}: {}",
                context(pck, at)
            ));
        }
    }
    let mut from = 0_usize;
    while let Some(found) = pck
        .get(from..)
        .and_then(|rest| find_bytes(rest, b"_check."))
    {
        let at = from + found;
        let smoke = at
            .checked_sub(5)
            .and_then(|start| pck.get(start..at))
            .is_some_and(|before| before == b"smoke");
        if !smoke {
            problems.push(format!(
                "names a check other than the smoke check at byte {at}: {}",
                context(pck, at)
            ));
        }
        from = at + 1;
    }
    if !contains_bytes(pck, b"smoke_check") {
        problems.push("does not name smoke_check, the one check that ships".to_owned());
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the exported .pck {} (godot/export_presets.cfg's exclude filter names fixtures/*, \
             *_shot.* and the client and watch checks by name)",
            problems.join("; ")
        ))
    }
}

/// Whether a line holds a run of exactly 64 lower-case hex digits: the shape
/// of a seat token (decisions-log item 107 (2)).
pub(crate) fn has_token_shape(line: &str) -> bool {
    let mut run = 0_usize;
    for byte in line.bytes().chain(std::iter::once(b' ')) {
        if byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) {
            run += 1;
        } else {
            if run == 64 {
                return true;
            }
            run = 0;
        }
    }
    false
}

/// The smoke run's verdict: exit 0, one `[smoke] OK`, no `SCRIPT ERROR`, and
/// no token-shaped run. The log is printed only when no line has that shape;
/// otherwise only the line numbers are.
fn judge_smoke(status: ExitStatus, log: &str) -> Result<String, String> {
    let tokens: Vec<usize> = log
        .lines()
        .enumerate()
        .filter(|(_, line)| has_token_shape(line))
        .map(|(index, _)| index + 1)
        .collect();
    if !tokens.is_empty() {
        return Err(format!(
            "the smoke log holds a 64-hex run, the shape of a seat token, on line(s) {tokens:?}; \
             the log is not printed (decisions-log item 107 (2))"
        ));
    }
    for line in log.lines() {
        println!("      | {line}");
    }
    let errors: Vec<&str> = log
        .lines()
        .filter(|line| line.starts_with("SCRIPT ERROR"))
        .collect();
    let ok = log
        .lines()
        .filter(|line| line.starts_with(SMOKE_OK))
        .count();
    if !status.success() {
        return Err(format!(
            "the smoke check failed ({})",
            describe_exit(status)
        ));
    }
    if !errors.is_empty() {
        return Err(format!(
            "the smoke check printed {} SCRIPT ERROR line(s); the debug VM reports what the \
             release VM crashes on (item 117 (6)): {}",
            errors.len(),
            errors.join(" | ")
        ));
    }
    if ok != 1 {
        return Err(format!(
            "the smoke check printed {ok} `{SMOKE_OK}` lines, not one"
        ));
    }
    let line = log
        .lines()
        .find(|line| line.starts_with(SMOKE_OK))
        .unwrap_or_default();
    Ok(line.to_owned())
}

/// What the export's exit code and outputs say.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExportVerdict {
    /// Exit 0 and every output present.
    Clean,
    /// A non-zero exit after the `savepack` line, with every output present in
    /// the emptied folder: item 102 (2)'s crash at exit.
    CrashAtExit,
    /// Anything else.
    Failed,
}

/// The export's verdict. Pure: `present` says, per expected output, whether it
/// is in the folder emptied before the export.
pub(crate) fn judge_export(exit_ok: bool, saw_savepack: bool, present: &[bool]) -> ExportVerdict {
    let all = !present.is_empty() && present.iter().all(|here| *here);
    match (exit_ok, saw_savepack, all) {
        (true, _, true) => ExportVerdict::Clean,
        (false, true, true) => ExportVerdict::CrashAtExit,
        _ => ExportVerdict::Failed,
    }
}

fn print_tail(text: &str, count: usize) {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(count);
    for line in lines.get(start..).unwrap_or_default() {
        println!("      | {line}");
    }
}

/// The highest `GLIBC_<n>.<m>[.<p>]` any of `files` asks for, read with
/// `objdump -T`; `GLIBC_PRIVATE` is ignored.
fn glibc_floor(files: &[PathBuf]) -> Result<String, String> {
    let mut best: Option<Vec<u32>> = None;
    for file in files {
        let output = Command::new("objdump")
            .arg("-T")
            .arg(file)
            .output()
            .map_err(|error| format!("failed to run objdump: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "`objdump -T {}` failed ({})",
                file.display(),
                describe_exit(output.status)
            ));
        }
        if let Some(version) = highest_glibc(&String::from_utf8_lossy(&output.stdout)) {
            if best.as_ref().is_none_or(|current| version > *current) {
                best = Some(version);
            }
        }
    }
    let best =
        best.ok_or_else(|| "objdump found no GLIBC_ version in the shipped ELF files".to_owned())?;
    Ok(best
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join("."))
}

/// The highest `GLIBC_` version named in `objdump -T` output, as integers.
pub(crate) fn highest_glibc(text: &str) -> Option<Vec<u32>> {
    let mut best: Option<Vec<u32>> = None;
    for (index, _) in text.match_indices("GLIBC_") {
        let rest = text.get(index + 6..).unwrap_or_default();
        let digits: String = rest
            .chars()
            .take_while(|character| character.is_ascii_digit() || *character == '.')
            .collect();
        let digits = digits.trim_end_matches('.');
        if digits.is_empty() {
            continue; // GLIBC_PRIVATE and the like
        }
        let parts: Result<Vec<u32>, _> = digits.split('.').map(str::parse::<u32>).collect();
        let Ok(parts) = parts else { continue };
        if best.as_ref().is_none_or(|current| parts > *current) {
            best = Some(parts);
        }
    }
    best
}

// ---------------------------------------------------------------------------
// The note
// ---------------------------------------------------------------------------

/// The commit the note is stamped with.
struct Commit {
    /// Twelve hex digits.
    short: String,
    /// The note's line: the commit, or a pull request's merge preview.
    line: String,
}

fn commit_stamp(ctx: &Ctx) -> Result<Commit, String> {
    let full = match env::var("GITHUB_SHA") {
        Ok(sha) if !sha.trim().is_empty() => sha.trim().to_owned(),
        _ => {
            let output = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&ctx.root)
                .output()
                .map_err(|error| format!("failed to run `git rev-parse HEAD`: {error}"))?;
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
    };
    let short: String = full.chars().take(12).collect();
    if short.len() != 12 || !short.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("`{full}` is not a commit"));
    }
    let pull_request = env::var("GITHUB_EVENT_NAME").is_ok_and(|event| event == "pull_request");
    let line = if pull_request {
        format!("pull-request build, merge preview of {short}")
    } else {
        format!("commit {short}")
    };
    Ok(Commit { short, line })
}

fn render_note(
    ctx: &Ctx,
    platform: &Platform,
    commit: &Commit,
    glibc: Option<&str>,
) -> Result<String, String> {
    let path = ctx
        .root
        .join(PACKAGING_DIR)
        .join(format!("README.{}.txt.in", platform.name));
    let template = read_text(&path)?;
    let text = template
        .replace("@VERSION@", VERSION)
        .replace("@BUILD@", &commit.line)
        .replace("@COMMIT@", &commit.short)
        .replace("@GLIBC@", glibc.unwrap_or("-"));
    unfilled(&path, &text)?;
    Ok(text)
}

/// Fails when a `@NAME@` marker is left in rendered text.
fn unfilled(template: &Path, text: &str) -> Result<(), String> {
    let left: Vec<&str> = text
        .split('@')
        .skip(1)
        .step_by(2)
        .filter(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
        })
        .collect();
    if left.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} left markers unfilled: {}",
            template.display(),
            left.join(", ")
        ))
    }
}

// ---------------------------------------------------------------------------
// Licences and notices
// ---------------------------------------------------------------------------

/// One crate compiled into a shipped binary.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Crate {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) licence: String,
    pub(crate) source: String,
    /// A workspace member: ours, GPL or permissive, and not a third party.
    pub(crate) member: bool,
}

/// What the notices and the manifest need.
struct Licences {
    gamectl: Vec<Crate>,
    client: Vec<Crate>,
    /// The godot-rust crates that run at build time.
    build_time: Vec<Crate>,
    gamectl_expression: String,
    client_expression: String,
}

/// One `cargo tree --prefix none -f '{p} {l} {r}'` listing, parsed: `(*)`
/// duplicates dropped, one row per name and version, sorted.
pub(crate) fn parse_tree(listing: &str) -> Vec<Crate> {
    let mut crates: Vec<Crate> = Vec::new();
    for line in listing.lines() {
        let line = line.trim();
        if line.is_empty() || line.ends_with("(*)") {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let (Some(name), Some(version)) = (words.first(), words.get(1)) else {
            continue;
        };
        let mut rest: Vec<&str> = words.get(2..).unwrap_or_default().to_vec();
        let mut member = false;
        if rest.first().is_some_and(|word| word.starts_with('(')) {
            member = true;
            while let Some(word) = rest.first() {
                let closes = word.ends_with(')');
                rest.remove(0);
                if closes {
                    break;
                }
            }
        }
        let source = if rest.last().is_some_and(|word| word.starts_with("http")) {
            rest.pop().unwrap_or_default().to_owned()
        } else {
            String::new()
        };
        crates.push(Crate {
            name: (*name).to_owned(),
            version: version.trim_start_matches('v').to_owned(),
            licence: normalise_expression(&rest.join(" ")),
            source,
            member,
        });
    }
    crates.sort();
    crates.dedup();
    crates
}

/// A crate's `license` field as an SPDX expression: cargo's old `/` spelling
/// becomes ` OR `.
pub(crate) fn normalise_expression(licence: &str) -> String {
    licence
        .split('/')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// `GPL-3.0-or-later AND` every distinct licence expression of `crates`, each
/// in parentheses when it is compound.
pub(crate) fn binary_expression(crates: &[Crate]) -> String {
    let mut parts: Vec<String> = vec!["GPL-3.0-or-later".to_owned()];
    let mut distinct: Vec<String> = crates
        .iter()
        .map(|item| item.licence.clone())
        .filter(|licence| !licence.is_empty())
        .collect();
    distinct.sort();
    distinct.dedup();
    for licence in distinct {
        let wrapped = if licence.contains(' ') {
            format!("({licence})")
        } else {
            licence
        };
        if !parts.contains(&wrapped) {
            parts.push(wrapped);
        }
    }
    parts.join(" AND ")
}

fn cargo_tree(ctx: &Ctx, package: &str, edges: &str, triple: &str) -> Result<String, String> {
    let args: Vec<String> = [
        "tree",
        "--offline",
        "--locked",
        "-e",
        edges,
        "--target",
        triple,
        "-p",
        package,
        "--prefix",
        "none",
        "-f",
        "{p} {l} {r}",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    crate::capture(ctx, &ctx.cargo, &args)
}

fn licences(ctx: &Ctx, platform: &Platform) -> Result<Licences, String> {
    // The offline trees below resolve dev- and build-dependencies too, and a
    // cold runner has fetched only what the two builds compiled; this makes
    // every source they name present (and the licence files readable).
    run(
        ctx,
        &ctx.cargo,
        &[
            "fetch".to_owned(),
            "--locked".to_owned(),
            "--target".to_owned(),
            platform.triple.to_owned(),
        ],
    )?;
    let gamectl = parse_tree(&cargo_tree(
        ctx,
        "pharmakos-gamectl",
        "normal,no-proc-macro",
        platform.triple,
    )?);
    let client = parse_tree(&cargo_tree(
        ctx,
        CLIENT_PACKAGE,
        "normal,no-proc-macro",
        platform.triple,
    )?);
    let build_time: Vec<Crate> = parse_tree(&cargo_tree(
        ctx,
        CLIENT_PACKAGE,
        "normal,build,no-proc-macro",
        platform.triple,
    )?)
    .into_iter()
    .filter(|item| GODOT_RUST_BUILD.contains(&item.name.as_str()))
    .collect();
    for name in GODOT_RUST_BUILD {
        if !build_time.iter().any(|item| item.name == *name) {
            return Err(format!(
                "the client's build graph has no `{name}`; the notices name it (item 117 (11))"
            ));
        }
    }
    Ok(Licences {
        gamectl_expression: binary_expression(&gamectl),
        client_expression: binary_expression(&client),
        gamectl,
        client,
        build_time,
    })
}

/// Where cargo unpacks registry sources.
fn registry_src() -> Result<PathBuf, String> {
    let home = match env::var_os("CARGO_HOME") {
        Some(home) if !home.is_empty() => PathBuf::from(home),
        _ => {
            let user = env::var_os("USERPROFILE")
                .or_else(|| env::var_os("HOME"))
                .ok_or_else(|| "neither CARGO_HOME nor a home directory is set".to_owned())?;
            PathBuf::from(user).join(".cargo")
        }
    };
    Ok(home.join("registry").join("src"))
}

/// A crate's licence, copying and notice files, verbatim, sorted by name.
// `fs::read_dir` is on clippy.toml's disallowed-methods list because its order
// differs between filesystems; both listings here are sorted before use, which
// is the remedy the ban asks for.
#[allow(clippy::disallowed_methods)]
fn licence_files(registry: &Path, item: &Crate) -> Result<Vec<(String, String)>, String> {
    let folder = format!("{}-{}", item.name, item.version);
    let mut indexes: Vec<PathBuf> = fs::read_dir(registry)
        .map_err(|error| format!("reading {}: {error}", registry.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    indexes.sort();
    let Some(source) = indexes
        .iter()
        .map(|index| index.join(&folder))
        .find(|candidate| candidate.is_dir())
    else {
        return Err(format!(
            "the source of {folder} is not under {}; `cargo fetch --locked` unpacks it",
            registry.display()
        ));
    };
    let mut files: Vec<PathBuf> = fs::read_dir(&source)
        .map_err(|error| format!("reading {}: {error}", source.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    let mut texts: Vec<(String, String)> = Vec::new();
    for file in files {
        let name = crate::file_name(&file);
        let upper = name.to_ascii_uppercase();
        if ["LICENSE", "LICENCE", "COPYING", "NOTICE"]
            .iter()
            .any(|prefix| upper.starts_with(prefix))
        {
            texts.push((name, read_text(&file)?));
        }
    }
    Ok(texts)
}

fn notices_text(
    platform: &Platform,
    commit: &Commit,
    licences: &Licences,
) -> Result<String, String> {
    let registry = registry_src()?;
    let mut out: Vec<String> = Vec::new();
    let rule = "=".repeat(78);
    out.push(format!(
        "Third-party notices for Pharmakos {VERSION} ({}), {} x86_64\n\n",
        commit.line, platform.display
    ));
    out.push(format!(
        "This file lists what is compiled into the two Rust binaries in this folder,\n\
         {} and {}, besides the game's own code, which is\n\
         GPL-3.0-or-later (LICENSES/GPL-3.0-or-later.txt). Each crate's own licence\n\
         and notice files follow at the end, verbatim, as its package ships them.\n\n",
        platform.gamectl, platform.library
    ));
    out.push(format!(
        "The game executable, {}, is Godot Engine's export template (MIT;\n\
         LICENSES/MIT.txt). It contains third-party components whose notices the\n\
         game's Credits screen shows, as the engine itself reports them; this zip's\n\
         REUSE.toml records the executable under Godot's own licence and copyright.\n\n",
        platform.executable
    ));
    out.push(format!(
        "Rust's standard library (std, core, alloc and the crates they are built\n\
         from) is statically linked into both binaries: MIT OR Apache-2.0, built\n\
         with {}.\n",
        rustc_version()?
    ));
    if platform.name == "windows" {
        out.push(
            "Both binaries link Microsoft's C runtime statically (+crt-static), so\n\
             they need no Visual C++ runtime installed.\n"
                .to_owned(),
        );
    }
    out.push("\n".to_owned());

    let mut third_party: Vec<Crate> = Vec::new();
    for (heading, crates) in [
        (platform.gamectl, &licences.gamectl),
        (platform.library, &licences.client),
    ] {
        out.push(format!("{rule}\nCompiled into {heading}\n{rule}\n\n"));
        for item in crates.iter().filter(|item| !item.member) {
            out.push(format!(
                "{} {}  {}  {}\n",
                item.name,
                item.version,
                item.licence,
                crate_source(item)
            ));
            third_party.push(item.clone());
        }
        out.push("\n".to_owned());
    }
    out.push(format!(
        "{rule}\nRun at build time; their generated code is compiled into godot-core and\n\
         godot-ffi\n{rule}\n\n"
    ));
    for item in &licences.build_time {
        out.push(format!(
            "{} {}  {}  {}\n",
            item.name,
            item.version,
            item.licence,
            crate_source(item)
        ));
        third_party.push(item.clone());
    }
    out.push("\n".to_owned());
    third_party.sort();
    third_party.dedup();

    out.push(format!("{rule}\nLicence and notice texts\n{rule}\n"));
    for item in &third_party {
        let texts = licence_files(&registry, item)?;
        out.push(format!(
            "\n--- {} {} ({}) ---\n",
            item.name, item.version, item.licence
        ));
        if texts.is_empty() {
            let godot_rust = GODOT_RUST_LINKED.contains(&item.name.as_str())
                || GODOT_RUST_BUILD.contains(&item.name.as_str());
            if !godot_rust {
                return Err(format!(
                    "{} {} ships no licence file, and it is not a godot-rust crate; a shipped \
                     crate without one is a stop (item 117 (0))",
                    item.name, item.version
                ));
            }
            out.push(format!(
                "This crate ships no licence file of its own. It is licensed under the\n\
                 Mozilla Public License 2.0 (LICENSES/MPL-2.0.txt). The source code of\n\
                 this library is available at https://crates.io/crates/{}/{}\n\
                 and {} (MPL-2.0 section 3.2).\n",
                item.name,
                item.version,
                if item.source.is_empty() {
                    "its repository"
                } else {
                    item.source.as_str()
                }
            ));
            continue;
        }
        for (name, text) in texts {
            out.push(format!("\n[{name}]\n\n{}\n", text.trim_end()));
        }
    }
    Ok(out.concat())
}

fn crate_source(item: &Crate) -> String {
    if item.source.is_empty() {
        format!("https://crates.io/crates/{}/{}", item.name, item.version)
    } else {
        item.source.clone()
    }
}

fn render_manifest(ctx: &Ctx, platform: &Platform, licences: &Licences) -> Result<String, String> {
    let path = ctx.root.join(PACKAGING_DIR).join(MANIFEST_TEMPLATE);
    let template = read_text(&path)?;
    let mut text = String::new();
    for line in template.lines() {
        if line.contains("REUSE-IgnoreStart") || line.contains("REUSE-IgnoreEnd") {
            continue;
        }
        text.push_str(line);
        text.push('\n');
    }
    let text = text
        .replace("@EXECUTABLE@", platform.executable)
        .replace("@PCK@", &pck_name(platform))
        .replace("@LIBRARY@", platform.library)
        .replace("@GAMECTL@", platform.gamectl)
        .replace("@CLIENT_LICENCE@", &licences.client_expression)
        .replace("@GAMECTL_LICENCE@", &licences.gamectl_expression);
    unfilled(&path, &text)?;
    Ok(text)
}

/// Every licence identifier the manifest's `SPDX-License-Identifier` values
/// name, plus MIT and Apache-2.0, which the files with their own headers
/// (`rules/`, `library/`, the changelog) carry. Sorted.
pub(crate) fn licence_ids(manifest: &str) -> Result<Vec<String>, String> {
    let mut ids: Vec<String> = vec!["Apache-2.0".to_owned(), "MIT".to_owned()];
    for line in manifest.lines() {
        let Some(value) = line.trim().strip_prefix("SPDX-License-Identifier") else {
            continue;
        };
        let value = value
            .trim_start()
            .trim_start_matches('=')
            .trim()
            .trim_matches('"');
        for word in value.split(|character: char| {
            character.is_whitespace() || character == '(' || character == ')'
        }) {
            match word {
                "" | "AND" | "OR" => {}
                "WITH" => {
                    return Err(format!(
                        "`{value}` carries a licence exception, whose text this command does not \
                         yet ship"
                    ));
                }
                id => ids.push(id.to_owned()),
            }
        }
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Copies exactly the licences named into the zip's `LICENSES/`: the
/// repository's own texts, or `packaging/LICENSE-<id>.txt`.
fn copy_licences(ctx: &Ctx, ids: &[String], to: &Path) -> Result<(), String> {
    for id in ids {
        let ours = ctx.root.join("LICENSES").join(format!("{id}.txt"));
        let packaged = ctx
            .root
            .join(PACKAGING_DIR)
            .join(format!("LICENSE-{id}.txt"));
        let from = if ours.is_file() {
            ours
        } else if packaged.is_file() {
            packaged
        } else {
            return Err(format!(
                "the zip's REUSE manifest names {id}, and neither LICENSES/{id}.txt nor \
                 packaging/LICENSE-{id}.txt holds its text; a record whose licence cannot be \
                 sourced is a stop (item 117 (0))"
            ));
        };
        copy_file(&from, &to.join(format!("{id}.txt")))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The manifest golden
// ---------------------------------------------------------------------------

/// The zip's entry names, one per line, sorted in byte order, LF, with a
/// trailing newline.
pub(crate) fn manifest_list(listed: &[zip::Listed]) -> String {
    let mut names: Vec<&str> = listed.iter().map(|entry| entry.name.as_str()).collect();
    names.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    let mut text = names.join("\n");
    text.push('\n');
    text
}

/// Writes the actual list under `<target>/golden/package/` and compares it
/// with the committed one; `--bless` writes the committed one instead.
fn compare_manifest(
    ctx: &Ctx,
    target: &Path,
    platform: &Platform,
    actual: &str,
) -> Result<(), String> {
    let actual_path = target
        .join("golden")
        .join("package")
        .join(format!("actual.{}.txt", platform.name));
    write_text(&actual_path, actual)?;
    let golden = GOLDEN_AREA
        .iter()
        .fold(ctx.root.clone(), |path, part| path.join(part))
        .join(format!("expected.{}.txt", platform.name));
    if ctx.bless {
        write_text(&golden, actual)?;
        println!(
            "   {} written from this zip: say in the pull request why the zip's contents moved",
            golden.display()
        );
        return Ok(());
    }
    manifest_verdict(
        fs::read(&golden).ok().as_deref(),
        actual.as_bytes(),
        &golden,
    )
}

/// The manifest's verdict. Pure, so the readable diff is unit-tested.
pub(crate) fn manifest_verdict(
    golden: Option<&[u8]>,
    actual: &[u8],
    path: &Path,
) -> Result<(), String> {
    match golden {
        Some(committed) if committed == actual => Ok(()),
        Some(committed) => {
            println!("   the zip's manifest, as written:");
            for line in String::from_utf8_lossy(actual).lines() {
                println!("      | {line}");
            }
            Err(format!(
                "the zip's contents moved: its manifest differs from {}\n{}\n      A moved \
                 manifest is a change to what ships. Read the diff, and if it is meant, re-bless \
                 with `cargo xtask package --bless` and say why in the pull request \
                 (tests/golden/package/README.md)",
                path.display(),
                first_difference(committed, actual)
            ))
        }
        None => {
            println!("   the zip's manifest, as written:");
            for line in String::from_utf8_lossy(actual).lines() {
                println!("      | {line}");
            }
            Err(format!(
                "there is no committed manifest at {}; a zip that compares nothing checks \
                 nothing",
                path.display()
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// reuse
// ---------------------------------------------------------------------------

/// Whether reuse lints a file of the zip at all (reuse 6.2.0, probe 117
/// section 4): not `LICENSES/**`, not a `LICENSE*`, `LICENCE*` or `COPYING*`
/// name, not a `.license` sidecar, not `REUSE.toml`, not an empty file.
pub(crate) fn reuse_lints(name: &str, size: u32) -> bool {
    let relative = name.split_once('/').map_or(name, |(_, rest)| rest);
    let base = relative.rsplit('/').next().unwrap_or(relative);
    let upper = base.to_ascii_uppercase();
    !(relative.starts_with("LICENSES/")
        || upper.starts_with("LICENSE")
        || upper.starts_with("LICENCE")
        || upper.starts_with("COPYING")
        || base.ends_with(".license")
        || base == "REUSE.toml"
        || size == 0)
}

/// How many of the zip's file entries reuse must have linted.
pub(crate) fn expected_lint_count(listed: &[zip::Listed]) -> usize {
    listed
        .iter()
        .filter(|entry| !entry.is_directory() && reuse_lints(&entry.name, entry.size))
        .count()
}

fn reuse_check(
    base: &Path,
    root: &Path,
    listed: &[zip::Listed],
    log: &Path,
) -> Result<(usize, usize), String> {
    let args: Vec<String> = vec![
        "--root".to_owned(),
        path_arg(root),
        "lint".to_owned(),
        "--json".to_owned(),
    ];
    let status = run_logged(
        "reuse",
        &args,
        base,
        &[("GIT_CEILING_DIRECTORIES".to_owned(), path_arg(base))],
        log,
    )?;
    let text = read_text(log)?;
    // Standard error shares the log, so a warning reuse prints there may sit
    // around the report: parse the one JSON object it wrote.
    let report = match (text.find('{'), text.rfind('}')) {
        (Some(start), Some(end)) if start < end => text.get(start..=end).unwrap_or_default(),
        _ => text.as_str(),
    };
    let json = crate::Json::parse(report).map_err(|error| {
        format!(
            "reuse printed no JSON ({error}); its output is in {}",
            log.display()
        )
    })?;
    let summary = json
        .get("summary")
        .ok_or_else(|| format!("reuse's JSON has no `summary`: {}", log.display()))?;
    let count = |key: &str| -> Result<usize, String> {
        match summary.get(key) {
            Some(crate::Json::Num(number)) => number
                .parse::<usize>()
                .map_err(|error| format!("reuse's `{key}` is not a count: {error}")),
            _ => Err(format!("reuse's summary has no `{key}`")),
        }
    };
    let total = count("files_total")?;
    let copyright = count("files_with_copyright_info")?;
    let licensing = count("files_with_licensing_info")?;
    let expected = expected_lint_count(listed);
    let mut problems: Vec<String> = Vec::new();
    if !status.success() {
        problems.push(format!("`reuse lint` failed ({})", describe_exit(status)));
    }
    if copyright != total || licensing != total {
        problems.push(format!(
            "of {total} file(s), {copyright} have copyright and {licensing} licence information"
        ));
    }
    if total != expected {
        problems.push(format!(
            "reuse linted {total} file(s), and the manifest's file entries less those reuse never \
             lints are {expected}"
        ));
    }
    if problems.is_empty() {
        Ok((total, expected))
    } else {
        Err(format!(
            "reuse over the extracted zip: {}; its report is {}",
            problems.join("; "),
            log.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_outside_the_package_folder_is_ever_emptied() {
        let base = env::temp_dir().join(format!("pharmakos-xtask-package-{}", std::process::id()));
        fs::create_dir_all(&base).expect("temp");
        // Inside: emptied and recreated.
        let inside = base.join("stage");
        fs::create_dir_all(inside.join("old")).expect("temp");
        reset_dir(&base, &inside).expect("inside is emptied");
        assert!(inside.is_dir() && !inside.join("old").exists());
        // The base itself, its parent, a sibling and a `..` escape: refused.
        for outside in [
            base.clone(),
            base.parent().expect("a parent").to_path_buf(),
            base.with_file_name("elsewhere"),
            base.join("stage").join("..").join(".."),
        ] {
            let refused = reset_dir(&base, &outside).expect_err("refused");
            assert!(refused.contains("refusing to empty"), "{refused}");
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn the_crt_scan_finds_the_runtime_and_not_the_operating_system() {
        let clean =
            b"KERNEL32.dll\0api-ms-win-core-synch-l1-2-0.dll\0bcryptprimitives.dll\0msvcrt.dll";
        assert!(crt_hits(clean).is_empty());
        let dirty =
            b"xxVCRUNTIME140.dll\0api-ms-win-crt-heap-l1-1-0.dll\0MSVCP140.dll\0ucrtbase.dll";
        let hits = crt_hits(dirty);
        assert_eq!(hits.len(), 4, "{hits:?}");
        assert!(
            hits.iter()
                .any(|hit| hit.starts_with("`vcruntime` at byte 2")),
            "{hits:?}"
        );
    }

    #[test]
    fn the_pck_scan_keeps_the_smoke_check_and_nothing_else() {
        assert!(scan_pck(b"scenes/smoke_check.tscn.remap\0scripts/smoke_check.gd.remap").is_ok());
        for bad in [
            &b"scenes/smoke_check.tscn\0scenes/client_check.tscn"[..],
            b"scenes/smoke_check.tscn\0fixtures/rows_report.json",
            b"scenes/smoke_check.tscn\0scenes/vista_shot.tscn",
            b"scenes/lobby.tscn",
        ] {
            assert!(scan_pck(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn a_token_shaped_run_is_found_and_nothing_else_is() {
        let token = "0123456789abcdef".repeat(4);
        assert!(has_token_shape(&token));
        assert!(has_token_shape(&format!("port 5 {token} x")));
        assert!(
            !has_token_shape(&format!("{token}0")),
            "65 digits is not a token"
        );
        assert!(!has_token_shape(&token.to_ascii_uppercase()));
        assert!(!has_token_shape("[smoke] OK (host pid 1234 exited)"));
    }

    #[test]
    fn an_export_is_judged_by_its_exit_code_first() {
        assert_eq!(
            judge_export(true, true, &[true, true, true]),
            ExportVerdict::Clean
        );
        assert_eq!(
            judge_export(false, true, &[true, true, true]),
            ExportVerdict::CrashAtExit
        );
        // A failed export leaves files behind: without savepack, still a failure.
        assert_eq!(
            judge_export(false, false, &[true, true, true]),
            ExportVerdict::Failed
        );
        assert_eq!(
            judge_export(false, true, &[true, true, false]),
            ExportVerdict::Failed
        );
        assert_eq!(
            judge_export(true, true, &[true, false, true]),
            ExportVerdict::Failed
        );
    }

    #[test]
    fn the_glibc_floor_is_the_highest_version_as_integers() {
        let text = "\
0000 DF *UND* 0000 (GLIBC_2.2.5) memcpy
0000 DF *UND* 0000 (GLIBC_2.34) pthread_create
0000 DF *UND* 0000 (GLIBC_2.9) pipe2
0000 DF *UND* 0000 (GLIBC_PRIVATE) __x
0000 DF *UND* 0000 (GLIBC_2.18) __cxa_thread_atexit_impl
";
        assert_eq!(highest_glibc(text), Some(vec![2, 34]));
        assert_eq!(highest_glibc("(GLIBC_PRIVATE) x"), None);
    }

    #[test]
    fn the_tree_listing_is_parsed_into_crates() {
        let listing = "\
pharmakos-gamectl v0.0.0 (C:\\repo\\crates\\gamectl) GPL-3.0-or-later https://example.invalid/pharmakos
bytes v1.12.1 MIT https://github.com/tokio-rs/bytes
cfg-if v1.0.4 MIT OR Apache-2.0 https://github.com/rust-lang/cfg-if
xxhash-rust v0.8.18 BSL-1.0 https://github.com/DoumanAsh/xxhash-rust
old v1.0.0 MIT/Apache-2.0
bytes v1.12.1 MIT https://github.com/tokio-rs/bytes (*)
";
        let crates = parse_tree(listing);
        let names: Vec<(&str, &str, &str, bool)> = crates
            .iter()
            .map(|item| {
                (
                    item.name.as_str(),
                    item.version.as_str(),
                    item.licence.as_str(),
                    item.member,
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                ("bytes", "1.12.1", "MIT", false),
                ("cfg-if", "1.0.4", "MIT OR Apache-2.0", false),
                ("old", "1.0.0", "MIT OR Apache-2.0", false),
                ("pharmakos-gamectl", "0.0.0", "GPL-3.0-or-later", true),
                ("xxhash-rust", "0.8.18", "BSL-1.0", false),
            ]
        );
        assert_eq!(
            binary_expression(&crates),
            "GPL-3.0-or-later AND BSL-1.0 AND MIT AND (MIT OR Apache-2.0)"
        );
    }

    #[test]
    fn the_licences_the_manifest_names_are_collected() {
        let manifest = "\
[[annotations]]
SPDX-License-Identifier = \"GPL-3.0-or-later AND (MIT OR Apache-2.0) AND MPL-2.0\"
SPDX-License-Identifier = \"MIT\"
";
        assert_eq!(
            licence_ids(manifest).expect("ids"),
            ["Apache-2.0", "GPL-3.0-or-later", "MIT", "MPL-2.0"]
        );
        assert!(
            licence_ids("SPDX-License-Identifier = \"GPL-2.0 WITH Classpath-exception-2.0\"")
                .is_err()
        );
    }

    #[test]
    fn reuse_is_expected_to_lint_every_file_but_the_ones_it_never_does() {
        let entry = |name: &str, size: u32| zip::Listed {
            name: name.to_owned(),
            external: 0,
            crc: 0,
            size,
            offset: 0,
        };
        let listed = [
            entry("Pharmakos/", 0),
            entry("Pharmakos/CHANGELOG.md", 10),
            entry("Pharmakos/LICENSES/", 0),
            entry("Pharmakos/LICENSES/MIT.txt", 10),
            entry("Pharmakos/Pharmakos.exe", 10),
            entry("Pharmakos/REUSE.toml", 10),
            entry("Pharmakos/THIRD-PARTY-NOTICES.txt", 10),
            entry("Pharmakos/library/LICENSE", 10),
            entry("Pharmakos/rules/rules.v1.json", 10),
            entry("Pharmakos/rules/rules.v1.json.license", 10),
            entry("Pharmakos/empty.txt", 0),
            entry("Pharmakos/COPYING.md", 10),
        ];
        // CHANGELOG.md, Pharmakos.exe, THIRD-PARTY-NOTICES.txt, rules.v1.json.
        assert_eq!(expected_lint_count(&listed), 4);
    }

    #[test]
    fn a_manifest_that_moved_fails_with_a_readable_diff() {
        let path = Path::new("tests/golden/package/expected.windows.txt");
        let golden = b"Pharmakos/\nPharmakos/gamectl.exe\nPharmakos/rules/\n";
        assert!(manifest_verdict(Some(golden), golden, path).is_ok());
        let moved = b"Pharmakos/\nPharmakos/gamectl.exe\nPharmakos/rulez/\n";
        let error = manifest_verdict(Some(golden), moved, path).expect_err("moved");
        assert!(error.contains("line 3"), "{error}");
        assert!(
            error.contains("Pharmakos/rules/") && error.contains("Pharmakos/rulez/"),
            "{error}"
        );
        let missing = manifest_verdict(None, golden, path).expect_err("missing");
        assert!(missing.contains("no committed manifest"), "{missing}");
    }

    #[test]
    fn an_unfilled_marker_is_refused() {
        let path = Path::new("packaging/README.windows.txt.in");
        assert!(unfilled(path, "version 0.1 for you@example").is_ok());
        assert!(unfilled(path, "floor @GLIBC@ here").is_err());
    }

    #[test]
    fn the_packaging_templates_fill_every_marker_they_carry() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for (name, markers) in [
            (
                "README.windows.txt.in",
                &["@VERSION@", "@BUILD@", "@COMMIT@"][..],
            ),
            (
                "README.linux.txt.in",
                &["@VERSION@", "@BUILD@", "@COMMIT@", "@GLIBC@"][..],
            ),
            (
                MANIFEST_TEMPLATE,
                &[
                    "@EXECUTABLE@",
                    "@PCK@",
                    "@LIBRARY@",
                    "@GAMECTL@",
                    "@CLIENT_LICENCE@",
                    "@GAMECTL_LICENCE@",
                ][..],
            ),
        ] {
            let text = fs::read_to_string(root.join(PACKAGING_DIR).join(name)).expect("a template");
            for marker in markers {
                assert!(text.contains(marker), "{name} lacks {marker}");
            }
        }
    }
}

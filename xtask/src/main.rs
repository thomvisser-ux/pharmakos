// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask` — the one command that runs every check.
//!
//! `cargo xtask ci` runs the steps below in order and stops at the first
//! failure (fail fast). Each step is a plain function that shells out; nothing
//! here is clever, and nothing here has a dependency (std only), so a cold
//! checkout can run it before a single third-party crate is fetched.
//!
//! Steps, in order — this is the harness-part-1 subset of AGENTS.md section 9,
//! in code. It is a subset on purpose: section 9 item 10 (`gamectl scenario
//! run` and the headless Godot screenshots) lives in
//! `.github/workflows/nightly-scenarios.yml` until harness part 2 builds it,
//! and item 11 (the G3′ tick budget and the P1 verifier budgets) arrives with
//! the gates that set those numbers. AGENTS.md section 9 marks both. Everything
//! else section 9 lists is below.
//!
//! | step             | what it does                                                                |
//! |------------------|-----------------------------------------------------------------------------|
//! | `fmt`            | `cargo fmt --all --check`                                                    |
//! | `clippy`         | `-D warnings` plus the determinism lint set; walled crates linted separately |
//! | `profiles`       | overflow checks are on in every profile, release included                    |
//! | `test`           | `cargo test --workspace` without the `research` feature                      |
//! | `test-research`  | `cargo test --workspace --features pharmakos-sim/research`                    |
//! | `research-guard` | plan-core / verifier / operator / gateway must not reach `research`          |
//! | `wall-guard`     | those crates and `sim` must not depend on a walled presentation/solve crate  |
//! | `deny`           | `cargo deny check` (licences, advisories, banned crates)                     |
//! | `buf`            | `buf lint` and `buf breaking --against .git#branch=main`                     |
//! | `golden`         | `tests/golden/**/expected.*` against the fresh `target/golden/**/actual.*`   |
//! | `determinism`    | runs the determinism binary and checks its per-tick hash chain               |
//! | `reuse`          | REUSE licence-manifest check                                                 |
//!
//! A step that cannot run yet — a tool that is not installed, a crate that does
//! not exist — reports `skipped` with the reason. A step never reports `ok` for
//! work it did not do.
//!
//! Two further steps are **harness part 2's**, landed here before the things
//! they check exist (decisions-log item 75, plan section 5). Both skip with a
//! named reason and self-activate the moment their producer lands, which is how
//! `step_determinism` already behaves:
//!
//! | step         | what it does                                                             |
//! |--------------|--------------------------------------------------------------------------|
//! | `scenario`   | validates `scenarios/**` today; shells `gamectl scenario run` from T15    |
//! | `screenshot` | renders the vista under xvfb + lavapipe and compares it to a golden PNG   |
//!
//! Their formats — the scenario file ([`scenario`]), the golden-file convention
//! ([`golden`]) and the screenshot comparison ([`png`]) — are frozen now so
//! that every later task delivers *into* a format rather than inventing one,
//! and no golden's shape is renegotiated under deadline.
//!
//! Usage:
//!
//! ```text
//! cargo xtask ci                    # everything, fail fast
//! cargo xtask ci --quick            # fmt, clippy, test — the inner loop
//! cargo xtask ci --fix              # rustfmt and the machine-applicable clippy fixes
//! cargo xtask ci --skip buf         # everything except one step
//! cargo xtask clippy -p pharmakos-sim   # one step, scoped (what the pre-commit hook runs)
//! cargo xtask golden --bless        # accept the fresh outputs as the new goldens
//! cargo xtask list                  # list the steps
//! ```
//!
//! # The determinism hash file
//!
//! This is a contract: `.github/workflows/ci.yml` uploads exactly this path per
//! operating system and a later job byte-compares the three.
//!
//! * path: `target/determinism/hashes.txt`, relative to the workspace root;
//! * one line per simulated tick, in tick order, starting at tick 0;
//! * each line is `<tick in decimal>` TAB `<xxh3-64 state hash as 16 lowercase
//!   hex digits>`;
//! * `\n` endings — never `\r\n`, because the file is byte-compared across
//!   Windows, Linux and macOS — and a trailing newline at end of file;
//! * when `tests/golden/determinism/expected.hashes.txt` exists, the fresh file
//!   must equal it byte for byte (spike G4's committed hash chain).
//!
//! Nothing here may use floats, `as` casts, `HashMap`/`HashSet` or wall-clock
//! time: xtask is linted by the same set it enforces, which is also why `ci`
//! reports no step timings.

// Justified crate-level allowances. The workspace lints (`clippy::pedantic` at
// warn, and `-D warnings` in the clippy step) are tuned for sim code; a task
// runner trips these three for no benefit:
#![allow(
    clippy::too_many_lines,        // `run_cli` is a flat argument parser, splitting it hides it
    clippy::doc_markdown,          // shell snippets and tool names in the docs above
    clippy::missing_panics_doc     // nothing here is a library API
)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus, Stdio};

// Harness part 2's formats, each in its own file so that the step table above
// stays readable and so that a task working on one of them touches one file.
mod annotate;
mod golden;
mod png;
mod scenario;

// ---------------------------------------------------------------------------
// Policy constants. These are the knobs; everything below them is machinery.
// ---------------------------------------------------------------------------

/// Lint flags handed to clippy for every crate that is not behind the wall.
///
/// This is a **re-assertion of the determinism subset**, not a copy of the
/// whole `[workspace.lints.clippy]` table in the root `Cargo.toml`. The
/// manifest is what a plain `cargo build` obeys, and it denies more than this
/// list (the three `cast_*` lints, `indexing_slicing`, `unwrap_used`,
/// `integer_division`); `-D warnings` below promotes every one of those to an
/// error anyway, so CI is never weaker than the manifest. What this list adds
/// is that the determinism bans still hold if someone edits the manifest.
/// Weakening an entry here, or removing one from the manifest, is a contract
/// change either way.
///
/// Justification, lint by lint:
/// * `warnings` — nothing lands with a warning.
/// * `clippy::float_arithmetic` — the sim is integer-only (Q16.16 positions,
///   Q32.32 squared distances, u16 angles, integer HP and $).
/// * `clippy::float_cmp` — comparing floats is float maths under another name.
/// * `clippy::as_conversions` — `as` truncates and wraps in silence; `From` and
///   `TryFrom` make a narrowing conversion a decision rather than an accident.
/// * `clippy::disallowed_types` — `HashMap`/`HashSet` iterate in an
///   unspecified, per-process-seeded order (list in `clippy.toml`).
/// * `clippy::disallowed_methods` — wall-clock readers and the other
///   order-dependent helpers (list in `clippy.toml`).
/// * `clippy::disallowed_macros` — enabled with an empty list so that adding an
///   entry to `clippy.toml` takes effect without touching this file.
const DETERMINISM_DENY: &[&str] = &[
    "-D",
    "warnings",
    "-D",
    "clippy::float_arithmetic",
    "-D",
    "clippy::float_cmp",
    "-D",
    "clippy::as_conversions",
    "-D",
    "clippy::disallowed_types",
    "-D",
    "clippy::disallowed_methods",
    "-D",
    "clippy::disallowed_macros",
];

/// The allowances applied when linting the walled presentation/solve crates.
///
/// The wall is a crate boundary, not an `#[allow]` sprinkled through the tree:
/// floats, `as` casts, hash maps and clock reads are legal inside these crates
/// and illegal everywhere else. `wall-guard` keeps the deterministic crates
/// from depending on them, which is what makes the allowance safe.
///
/// The three `cast_*` entries are here because converting float vertex data to
/// integers is the reason the wall exists: without them a walled crate would be
/// denied by `[workspace.lints.clippy]`'s `cast_possible_truncation`,
/// `cast_precision_loss` and `cast_sign_loss` with no crate-level way out, and
/// pass 2 could not pass. Keep this list and `clippy.toml`'s "pass 2" bullet in
/// step.
const WALL_ALLOW: &[&str] = &[
    "-A",
    "clippy::float_arithmetic",
    "-A",
    "clippy::float_cmp",
    "-A",
    "clippy::as_conversions",
    "-A",
    "clippy::cast_possible_truncation",
    "-A",
    "clippy::cast_precision_loss",
    "-A",
    "clippy::cast_sign_loss",
    "-A",
    "clippy::disallowed_types",
    "-A",
    "clippy::disallowed_methods",
];

/// Packages behind the wall. Matched with and without the `pharmakos-` prefix.
///
/// Two of the four names are the spec's own wording — "a walled
/// presentation/solve module" (spec section 15, Maths). The other two are
/// crates: `client-gdext`, the thin gdext bridge, and `mesher`, the greedy
/// mesher, which decisions log section 2.7 item 56 made its own walled crate
/// rather than a module inside the bridge — it takes integer chunk data in and
/// produces vertex buffers out, links without gdext so the headless CPU proxy
/// and the CI geometry check can reuse it, and is depended on by `client-gdext`
/// alone today; `wall-guard` below enforces the half that matters to
/// determinism — that no crate in [`WALL_GUARDED_PACKAGES`] ever reaches it.
/// A crate that is not on this list gets no float allowance. Adding a name
/// widens the float, cast, hash-map and clock allowance for a whole crate, so
/// it is a contract change and needs owner approval — keep this list,
/// `clippy.toml`'s header comment and AGENTS.md section 4.9 in step.
///
/// PLACEHOLDER: `pharmakos-client-gdext` and `pharmakos-mesher` exist;
/// `presentation` and `solve` are named by the spec but not yet created.
const WALLED_PACKAGES: &[&str] = &["presentation", "solve", "client-gdext", "mesher"];

/// Crates that may never reach the `research` feature, which gates `fork`.
///
/// `sim` is deliberately absent: it is the crate that *defines* the feature, so
/// it is the one package for which reaching `research` is correct. Walled
/// crates are kept away by the separate [`WALL_GUARDED_PACKAGES`] list below,
/// which does include `sim`.
const GUARDED_PACKAGES: &[&str] = &["plan-core", "verifier", "operator", "gateway"];

/// Crates that may never depend on a walled crate, transitively included.
///
/// [`GUARDED_PACKAGES`] plus `sim`. The four guarded crates must not reach the
/// float, cast, hash-map and clock allowance, and neither must the sim — it is
/// the crate that owns hashed state, so it is the one the wall exists to
/// protect (AGENTS.md section 4.9). The two lists are separate rather than one
/// because `sim` defines the `research` feature and so cannot join the research
/// guard. Keep this list, AGENTS.md section 4.9 and `clippy.toml`'s header in
/// step; widening it is a contract change like any other.
const WALL_GUARDED_PACKAGES: &[&str] = &["sim", "plan-core", "verifier", "operator", "gateway"];

/// Candidate names for the one crate that owns the `research` feature.
const SIM_PACKAGES: &[&str] = &["sim"];

/// The compile-time feature that gates `fork`. Release builds never enable it.
const RESEARCH_FEATURE: &str = "research";

/// Where the per-tick state hashes are written, relative to Cargo's target
/// directory (`target/` by default, so CI uploads `target/determinism/hashes.txt`;
/// a user-level `build.target-dir` moves it along with everything else).
/// See the module docs for the format.
const HASH_FILE: &str = "determinism/hashes.txt";

/// The committed hash chain, when there is one. Compared byte for byte.
const HASH_GOLDEN: &str = "tests/golden/determinism/expected.hashes.txt";

/// The binary the `determinism` step runs once it exists.
const DETERMINISM_BIN: &str = "determinism";

/// PLACEHOLDER: 1 200 ticks is one minute at 20 Hz — a smoke run. Raise it to
/// G4's bar (10 matches × 9 600 ticks) once the sim can carry it; a long run
/// belongs in the nightly workflow rather than in every pull request.
const DETERMINISM_TICKS: &str = "1200";

/// The profile the determinism run uses: release optimisation with debug
/// assertions still on, declared in the root `Cargo.toml`.
const DETERMINISM_PROFILE: &str = "release-checked";

/// The inner-loop subset, as documented in AGENTS.md and CLAUDE.md.
const QUICK_STEPS: &[&str] = &["fmt", "clippy", "test"];

// --- harness part 2 (AGENTS.md section 9 items 10 and 11) ------------------
//
// These constants are the *formats* the skeleton freezes in wave 1. The
// producers arrive later: `gamectl scenario run` at T15, the Godot vista at
// T16, and T20 promotes both steps from skipping to required. Keep each
// constant's comment naming the task that fills it, so a skip is never
// mistaken for a pass that happens to be quiet.

/// The binary that runs a scenario, and the subcommand it must carry.
const SCENARIO_BIN: &str = "gamectl";
const SCENARIO_SUBCOMMAND: &str = "scenario";

/// Where the Godot project lives (decisions-log item 72: `godot/` at the
/// repository root, one ownable unit with `crates/client-gdext`).
const GODOT_PROJECT_DIR: &str = "godot";

/// The vista golden the `screenshot` step compares against, and where the fresh
/// render is written. The golden is committed by T16; until it exists the step
/// skips with that reason.
const VISTA_GOLDEN: &str = "tests/golden/vista/expected.vista.png";
const VISTA_ACTUAL: &str = "golden/vista/actual.vista.png";

/// The render the screenshot step asks Godot for, and the xvfb screen it runs
/// on. 1280 × 720 is G1's geometry resolution. The two must agree: a windowed
/// run is clamped by the screen size — G1 asked for 1920 × 1080 on a
/// 1920 × 1080 screen and got 1920 × 1061 — and a differently sized image fails
/// the comparison for the wrong reason.
const VISTA_RESOLUTION: &str = "1280x720";
const VISTA_SCREEN: &str = "-screen 0 1280x720x24";

/// PLACEHOLDER: the scene the vista job loads and the flag that makes it take
/// one shot and quit are T16's to name — the Godot project does not exist yet.
/// When it does, this becomes the scene path passed to `godot --path godot`.
/// Owner/T16.
const VISTA_SCENE: &str = "res://scenes/vista_shot.tscn";

// ---------------------------------------------------------------------------
// Step table
// ---------------------------------------------------------------------------

struct Step {
    name: &'static str,
    about: &'static str,
    run: fn(&Ctx) -> Result<Outcome, String>,
}

enum Outcome {
    /// The step ran and passed.
    Done(String),
    /// The step could not run yet, and that is expected: a tool that is not
    /// installed, or a part of the tree that does not exist. Never used to
    /// paper over a real failure.
    Skipped(String),
}

const STEPS: &[Step] = &[
    Step {
        name: "fmt",
        about: "cargo fmt --all --check",
        run: step_fmt,
    },
    Step {
        name: "clippy",
        about: "-D warnings plus the determinism lint set",
        run: step_clippy,
    },
    Step {
        name: "profiles",
        about: "overflow checks stay on in every profile",
        run: step_profiles,
    },
    Step {
        name: "test",
        about: "cargo test --workspace, without the research feature",
        run: step_test,
    },
    Step {
        name: "test-research",
        about: "cargo test --workspace, with the research feature",
        run: step_test_research,
    },
    Step {
        name: "research-guard",
        about: "plan-core/verifier/operator/gateway must not reach `research`",
        run: step_research_guard,
    },
    Step {
        name: "wall-guard",
        about: "those crates and sim must not depend on a walled crate",
        run: step_wall_guard,
    },
    Step {
        name: "deny",
        about: "cargo deny check — licences, advisories, banned crates",
        run: step_deny,
    },
    Step {
        name: "buf",
        about: "buf lint and buf breaking --against .git#branch=main",
        run: step_buf,
    },
    Step {
        name: "golden",
        about: "tests/golden/**/expected.* against the fresh outputs",
        run: step_golden,
    },
    Step {
        name: "determinism",
        about: "run the determinism binary and check its hash chain",
        run: step_determinism,
    },
    Step {
        name: "reuse",
        about: "REUSE licence-manifest check",
        run: step_reuse,
    },
    // Harness part 2. Both skip with a named reason until their producer
    // exists, and both self-activate the moment it does.
    Step {
        name: "scenario",
        about: "validate scenarios/**, then run them once gamectl can",
        run: step_scenario,
    },
    Step {
        name: "screenshot",
        about: "render the vista headless and compare it to its golden PNG",
        run: step_screenshot,
    },
];

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run_cli(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

struct Ctx {
    /// Workspace root: the directory holding the root `Cargo.toml`.
    root: PathBuf,
    /// The cargo to shell out to (`$CARGO` when cargo invoked us).
    cargo: String,
    /// `--bless`: rewrite golden files from the fresh outputs.
    bless: bool,
    /// `--require-tools` (or `PHARMAKOS_REQUIRE_TOOLS=1`): fail instead of
    /// skipping when buf, cargo-deny or reuse is missing. The CI workflow
    /// passes it after installing all three, so a runner that lost a tool goes
    /// red rather than quietly green.
    require_tools: bool,
    /// `--locked`: pass `--locked` to cargo. On by default in CI, but only once
    /// a `Cargo.lock` is committed — `--locked` with no lock file is an error,
    /// and the lock file does not exist until the first dependency lands.
    locked: bool,
    /// `-p/--package`: restrict package-scoped steps (the pre-commit hook).
    packages: Vec<String>,
    /// Workspace metadata, or the reason it could not be read.
    workspace: Result<Workspace, String>,
}

fn run_cli(args: &[String]) -> Result<bool, String> {
    let mut commands: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut packages: Vec<String> = Vec::new();
    let mut bless = false;
    let mut quick = false;
    let mut fix = false;
    let mut require_tools = env::var_os("PHARMAKOS_REQUIRE_TOOLS").is_some();
    // `None` until a flag says otherwise; resolved below, once the root is known.
    let mut locked: Option<bool> = None;

    let mut index = 0;
    while index < args.len() {
        let arg: &str = match args.get(index) {
            Some(value) => value.as_str(),
            None => break,
        };
        match arg {
            "-h" | "--help" | "help" => {
                print_help();
                return Ok(true);
            }
            "list" | "--list" => {
                for step in STEPS {
                    println!("{:<15} {}", step.name, step.about);
                }
                return Ok(true);
            }
            "--quick" => quick = true,
            "--fix" => fix = true,
            "--bless" => bless = true,
            "--require-tools" => require_tools = true,
            "--no-require-tools" => require_tools = false,
            "--locked" => locked = Some(true),
            "--no-locked" => locked = Some(false),
            "--skip" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--skip needs a step name".to_owned())?;
                skipped.push(value.clone());
            }
            "-p" | "--package" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--package needs a package name".to_owned())?;
                packages.push(value.clone());
            }
            other => {
                if other.starts_with('-') {
                    return Err(format!("unknown flag `{other}` (try `cargo xtask help`)"));
                }
                commands.push(other.to_owned());
            }
        }
        index += 1;
    }

    if commands.is_empty() {
        print_help();
        return Err("no command given".to_owned());
    }
    for name in &skipped {
        if !STEPS.iter().any(|step| step.name == name) {
            return Err(format!("--skip names an unknown step `{name}`"));
        }
    }

    let root = workspace_root()?;
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let locked =
        locked.unwrap_or_else(|| env::var_os("CI").is_some() && root.join("Cargo.lock").is_file());
    let workspace = load_workspace(&cargo, &root);
    if let Err(reason) = &workspace {
        println!("note: workspace metadata unavailable ({reason})");
        println!("note: steps that need it will report as skipped");
    }
    let ctx = Ctx {
        root,
        cargo,
        bless,
        require_tools,
        locked,
        packages,
        workspace,
    };

    // `--fix` is a mode, not a step: it edits the tree and then stops, so that
    // nobody mistakes "xtask rewrote my code" for "the checks passed".
    if fix {
        return run_fix(&ctx).map(|()| true);
    }

    let mut plan: Vec<&Step> = Vec::new();
    for name in &commands {
        if name == "ci" {
            for step in STEPS {
                let wanted = !quick || QUICK_STEPS.contains(&step.name);
                if wanted && !skipped.iter().any(|s| s == step.name) {
                    plan.push(step);
                }
            }
        } else {
            let step = STEPS
                .iter()
                .find(|step| step.name == name)
                .ok_or_else(|| format!("unknown command `{name}` (try `cargo xtask list`)"))?;
            plan.push(step);
        }
    }

    let mut results: Vec<(&'static str, String)> = Vec::new();
    let mut ok = true;
    for step in plan {
        println!();
        println!("== {} — {}", step.name, step.about);
        match (step.run)(&ctx) {
            Ok(Outcome::Done(note)) => {
                println!("   ok: {note}");
                results.push((step.name, format!("ok       {note}")));
            }
            Ok(Outcome::Skipped(note)) => {
                println!("   skipped: {note}");
                results.push((step.name, format!("skipped  {note}")));
            }
            Err(message) => {
                eprintln!("   FAILED: {message}");
                results.push((step.name, format!("FAILED   {message}")));
                ok = false;
                break; // fail fast
            }
        }
    }

    println!();
    println!("== summary");
    for (name, line) in &results {
        println!("{name:<15} {line}");
    }
    if !ok {
        println!();
        println!("`cargo xtask ci` failed. Fix the step above and run it again;");
        println!("do not reach for a different command, and never silence a");
        println!("determinism lint to get to green (AGENTS.md section 3).");
    }
    Ok(ok)
}

/// `cargo xtask ci --fix`: rustfmt, then the machine-applicable clippy fixes.
fn run_fix(ctx: &Ctx) -> Result<(), String> {
    run(ctx, &ctx.cargo, &["fmt".to_owned(), "--all".to_owned()])?;
    let mut args: Vec<String> = vec![
        "clippy".to_owned(),
        "--workspace".to_owned(),
        "--all-targets".to_owned(),
        "--fix".to_owned(),
        "--allow-dirty".to_owned(),
        "--allow-staged".to_owned(),
        "--".to_owned(),
    ];
    for flag in DETERMINISM_DENY {
        args.push((*flag).to_owned());
    }
    run(ctx, &ctx.cargo, &args)?;
    println!();
    println!("fixes applied — review the diff, then run `cargo xtask ci`.");
    Ok(())
}

fn print_help() {
    println!("cargo xtask — the one command that runs every check");
    println!();
    println!("USAGE:");
    println!("    cargo xtask ci [--quick|--fix] [--skip <step>]...");
    println!("    cargo xtask <step> [flags]");
    println!("    cargo xtask list");
    println!();
    println!("FLAGS:");
    println!("    --quick            fmt, clippy and tests only — the inner loop");
    println!("    --fix              apply rustfmt and machine-applicable clippy fixes, then stop");
    println!("    --skip <step>      leave one step out of `ci`");
    println!("    -p, --package <n>  restrict package-scoped steps to these packages");
    println!("    --bless            rewrite golden files from the fresh outputs");
    println!("    --require-tools    fail instead of skipping when a tool is missing");
    println!("    --locked           pass --locked to cargo (automatic when $CI is set)");
    println!();
    println!("STEPS:");
    for step in STEPS {
        println!("    {:<15} {}", step.name, step.about);
    }
}

// ---------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------

fn step_fmt(ctx: &Ctx) -> Result<Outcome, String> {
    let mut args: Vec<String> = vec!["fmt".to_owned()];
    if ctx.packages.is_empty() {
        args.push("--all".to_owned());
    } else {
        for package in &ctx.packages {
            args.push("--package".to_owned());
            args.push(package.clone());
        }
    }
    args.push("--check".to_owned());
    run(ctx, &ctx.cargo, &args)?;
    Ok(Outcome::Done("formatting is clean".to_owned()))
}

fn step_clippy(ctx: &Ctx) -> Result<Outcome, String> {
    let walled = walled_packages(ctx);
    let mut notes: Vec<String> = Vec::new();

    // Pass 1 — everything outside the wall, with the full deny set.
    let mut args: Vec<String> = vec!["clippy".to_owned()];
    if ctx.packages.is_empty() {
        args.push("--workspace".to_owned());
        for package in &walled {
            args.push("--exclude".to_owned());
            args.push(package.clone());
        }
    } else {
        for package in &ctx.packages {
            args.push("--package".to_owned());
            args.push(package.clone());
        }
    }
    args.push("--all-targets".to_owned());
    if ctx.locked {
        args.push("--locked".to_owned());
    }
    args.push("--".to_owned());
    for flag in DETERMINISM_DENY {
        args.push((*flag).to_owned());
    }
    run(ctx, &ctx.cargo, &args)?;
    notes.push("deterministic crates clean".to_owned());

    if !ctx.packages.is_empty() {
        return Ok(Outcome::Done(format!(
            "clean for {} (scoped run)",
            ctx.packages.join(", ")
        )));
    }

    // Pass 2 — the walled crates, still `-D warnings`, with the float, cast,
    // hash-map and clock allowances that the wall exists to contain.
    if !walled.is_empty() {
        let mut walled_args: Vec<String> = vec!["clippy".to_owned()];
        for package in &walled {
            walled_args.push("--package".to_owned());
            walled_args.push(package.clone());
        }
        walled_args.push("--all-targets".to_owned());
        if ctx.locked {
            walled_args.push("--locked".to_owned());
        }
        walled_args.push("--".to_owned());
        for flag in DETERMINISM_DENY {
            walled_args.push((*flag).to_owned());
        }
        for flag in WALL_ALLOW {
            walled_args.push((*flag).to_owned());
        }
        run(ctx, &ctx.cargo, &walled_args)?;
        notes.push(format!("walled crates clean ({})", walled.join(", ")));
    }

    // Pass 3 — the research configuration, so `fork` code is linted too. Named
    // explicitly rather than with `--all-features`, which would turn `research`
    // on everywhere and hide the very ban `research-guard` checks.
    if let Some(feature) = research_feature_spec(ctx) {
        let mut research_args: Vec<String> = vec!["clippy".to_owned(), "--workspace".to_owned()];
        for package in &walled {
            research_args.push("--exclude".to_owned());
            research_args.push(package.clone());
        }
        research_args.push("--all-targets".to_owned());
        research_args.push("--features".to_owned());
        research_args.push(feature.clone());
        if ctx.locked {
            research_args.push("--locked".to_owned());
        }
        research_args.push("--".to_owned());
        for flag in DETERMINISM_DENY {
            research_args.push((*flag).to_owned());
        }
        run(ctx, &ctx.cargo, &research_args)?;
        notes.push(format!("research build clean ({feature})"));
    }

    Ok(Outcome::Done(notes.join("; ")))
}

/// Overflow checks stay on — in debug, and above all in release, where cargo
/// would otherwise turn them off. A wrap that happens on one platform's path
/// and not another's is exactly the failure this project cannot absorb, so a
/// profile that switches the check off fails the build.
fn step_profiles(ctx: &Ctx) -> Result<Outcome, String> {
    let manifest_path = ctx.root.join("Cargo.toml");
    let Ok(manifest) = fs::read_to_string(&manifest_path) else {
        let display = manifest_path.display();
        return Ok(Outcome::Skipped(format!("cannot read {display}")));
    };
    let (release_checked, mut offenders) = scan_profiles(&manifest);

    // A cargo config can override the manifest's profiles, so it is checked too
    // — otherwise the ban could be lifted in a file nobody reads.
    let config_path = ctx.root.join(".cargo").join("config.toml");
    if let Ok(config) = fs::read_to_string(&config_path) {
        let (_, config_offenders) = scan_profiles(&config);
        for profile in config_offenders {
            offenders.push(format!(".cargo/config.toml [{profile}]"));
        }
    }

    if !offenders.is_empty() {
        return Err(format!(
            "overflow checks are switched off in {}; they stay on in every profile, release \
             included (spec section 15, Maths) — a silent wrap on one platform and a panic on \
             another is the failure this project cannot absorb",
            offenders.join(", ")
        ));
    }
    if !release_checked {
        return Err(format!(
            "{} must set `overflow-checks = true` under `[profile.release]`",
            manifest_path.display()
        ));
    }
    Ok(Outcome::Done(
        "overflow checks on in release, and switched off nowhere".to_owned(),
    ))
}

/// Reads the `[profile.*]` tables out of a TOML file's text. Returns whether
/// `[profile.release]` sets `overflow-checks = true`, and the names of any
/// profiles that set it to `false`.
///
/// Text scanning rather than a TOML parser, because xtask takes no
/// dependencies; it copes with comments and whitespace, which is all the two
/// files in question need.
fn scan_profiles(text: &str) -> (bool, Vec<String>) {
    let mut release_checked = false;
    let mut offenders: Vec<String> = Vec::new();
    let mut table = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('[') {
            rest.trim_end_matches(']').clone_into(&mut table);
            continue;
        }
        if !table.starts_with("profile.") {
            continue;
        }
        let compact: String = trimmed
            .split('#')
            .next()
            .unwrap_or("")
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        if compact == "overflow-checks=true" && table == "profile.release" {
            release_checked = true;
        }
        if compact == "overflow-checks=false" {
            offenders.push(table.clone());
        }
    }
    (release_checked, offenders)
}

fn step_test(ctx: &Ctx) -> Result<Outcome, String> {
    let mut args: Vec<String> = vec!["test".to_owned()];
    if ctx.packages.is_empty() {
        args.push("--workspace".to_owned());
    } else {
        for package in &ctx.packages {
            args.push("--package".to_owned());
            args.push(package.clone());
        }
    }
    if ctx.locked {
        args.push("--locked".to_owned());
    }
    run(ctx, &ctx.cargo, &args)?;
    Ok(Outcome::Done("tests pass without `research`".to_owned()))
}

fn step_test_research(ctx: &Ctx) -> Result<Outcome, String> {
    let Some(feature) = research_feature_spec(ctx) else {
        return Ok(Outcome::Skipped(format!(
            "no crate declares a `{RESEARCH_FEATURE}` feature yet"
        )));
    };
    // `--features <pkg>/<feature>` is legal from the workspace root because the
    // package is a workspace member (resolver 2/3).
    let mut args: Vec<String> = vec![
        "test".to_owned(),
        "--workspace".to_owned(),
        "--features".to_owned(),
        feature.clone(),
    ];
    if ctx.locked {
        args.push("--locked".to_owned());
    }
    run(ctx, &ctx.cargo, &args)?;
    Ok(Outcome::Done(format!("tests pass with {feature}")))
}

/// `fork` lives behind the `research` feature, and release builds never enable
/// it. This proves the plan core, verifier, operator and gateway cannot reach
/// it: once with their default features (the shipped configuration), once with
/// every feature of their own turned on (so no feature of theirs can forward to
/// it later). `--all-features` here is a `cargo tree` query, not a build — the
/// AGENTS.md ban on `--all-features` is about compiling, where it would enable
/// `research` everywhere and hide this very check.
fn step_research_guard(ctx: &Ctx) -> Result<Outcome, String> {
    let workspace = match &ctx.workspace {
        Ok(workspace) => workspace,
        Err(reason) => return Ok(Outcome::Skipped(format!("no metadata: {reason}"))),
    };

    let present = workspace.present(GUARDED_PACKAGES);
    if present.is_empty() {
        return Ok(Outcome::Skipped(
            "none of plan-core, verifier, operator, gateway exist yet".to_owned(),
        ));
    }

    for package in &present {
        for all_features in [false, true] {
            let mut args: Vec<String> = vec![
                "tree".to_owned(),
                "--package".to_owned(),
                package.clone(),
                "--edges".to_owned(),
                "features".to_owned(),
                "--format".to_owned(),
                "{p}|{f}".to_owned(),
            ];
            if all_features {
                args.push("--all-features".to_owned());
            }
            if ctx.locked {
                args.push("--locked".to_owned());
            }
            let tree = capture(ctx, &ctx.cargo, &args)?;
            let offenders: Vec<&str> = tree
                .lines()
                .filter(|line| line_enables_research(line))
                .collect();
            if !offenders.is_empty() {
                let how = if all_features {
                    "with --all-features"
                } else {
                    "with its default features"
                };
                let mut report = format!(
                    "`{package}` reaches the `{RESEARCH_FEATURE}` feature {how}.\n      \
                     The plan core, verifier, operator and gateway may never depend on `fork`: \
                     \"no dry runs\" is a hard guarantee in shipped builds (spec section 15).\n      \
                     Offending feature edges:\n"
                );
                for line in offenders.iter().take(20) {
                    report.push_str("        ");
                    report.push_str(line.trim_end());
                    report.push('\n');
                }
                return Err(report);
            }
        }
    }

    Ok(Outcome::Done(format!(
        "`{RESEARCH_FEATURE}` is unreachable from {}",
        present.join(", ")
    )))
}

/// The wall is a crate boundary. Floats, `as` casts and hash maps are legal
/// inside the walled crates, so nothing deterministic may depend on them —
/// otherwise the allowance leaks into hashed state. "Nothing deterministic" is
/// [`WALL_GUARDED_PACKAGES`]: the four guarded crates and the sim itself.
fn step_wall_guard(ctx: &Ctx) -> Result<Outcome, String> {
    let workspace = match &ctx.workspace {
        Ok(workspace) => workspace,
        Err(reason) => return Ok(Outcome::Skipped(format!("no metadata: {reason}"))),
    };

    let walled = workspace.present(WALLED_PACKAGES);
    let guarded = workspace.present(WALL_GUARDED_PACKAGES);
    if walled.is_empty() || guarded.is_empty() {
        return Ok(Outcome::Skipped(
            "the walled crates or the guarded crates do not exist yet".to_owned(),
        ));
    }

    for package in &guarded {
        let mut args: Vec<String> = vec![
            "tree".to_owned(),
            "--package".to_owned(),
            package.clone(),
            "--all-features".to_owned(),
            "--prefix".to_owned(),
            "none".to_owned(),
            "--format".to_owned(),
            "{p}".to_owned(),
        ];
        if ctx.locked {
            args.push("--locked".to_owned());
        }
        let tree = capture(ctx, &ctx.cargo, &args)?;
        for line in tree.lines() {
            let name = line.split_whitespace().next().unwrap_or("");
            if walled.iter().any(|walled_name| walled_name == name) {
                return Err(format!(
                    "`{package}` depends on the walled crate `{name}`; floats and unordered \
                     collections are legal there and must not reach the deterministic crates"
                ));
            }
        }
    }

    Ok(Outcome::Done(format!(
        "{} do not depend on {}",
        guarded.join(", "),
        walled.join(", ")
    )))
}

fn step_deny(ctx: &Ctx) -> Result<Outcome, String> {
    if !ctx.root.join("deny.toml").is_file() {
        return Ok(Outcome::Skipped("no deny.toml".to_owned()));
    }
    if !cargo_subcommand_available(&ctx.cargo, "deny") {
        return skip_or_fail(
            ctx,
            "cargo-deny is not installed (`cargo install --locked cargo-deny`)",
        );
    }
    run(ctx, &ctx.cargo, &["deny".to_owned(), "check".to_owned()])?;
    Ok(Outcome::Done(
        "licences, advisories and banned crates are clean".to_owned(),
    ))
}

fn step_buf(ctx: &Ctx) -> Result<Outcome, String> {
    let Some(config_dir) = buf_config_dir(&ctx.root) else {
        return Ok(Outcome::Skipped(
            "no buf.yaml / buf.work.yaml yet".to_owned(),
        ));
    };
    if !tool_available("buf") {
        return skip_or_fail(
            ctx,
            "buf is not installed (https://buf.build/docs/installation)",
        );
    }

    // Both commands run from the WORKSPACE ROOT, with the module directory as
    // the input. That is not a detail: `buf breaking --against .git#...`
    // resolves the git path relative to the invocation directory, and `.git`
    // lives at the repository root, not in `proto/`. Running buf inside
    // `proto/` would look for `proto/.git` and fail with "could not read git
    // repository" on every run — and in CI, where PHARMAKOS_REQUIRE_TOOLS is
    // set, that is a red leg rather than a skip. Keep this in step with the
    // command block in proto/buf.yaml's header and with examples/README.md.
    let relative = config_dir
        .strip_prefix(&ctx.root)
        .map_err(|error| format!("the buf config is outside the workspace: {error}"))?;
    // Forward slashes: buf's input and reference syntax is not a filesystem path.
    let mut input = relative.to_string_lossy().replace('\\', "/");
    if input.is_empty() {
        // The buf config sits at the workspace root, so the module is the root.
        ".".clone_into(&mut input);
    }

    run(ctx, "buf", &["lint".to_owned(), input.clone()])?;

    // `buf breaking` compares against the base branch in the local object
    // store, so CI checks out with fetch-depth: 0 and fetches main. A fresh
    // clone that has no main yet simply skips the comparison rather than
    // inventing a baseline.
    if !git_ref_exists(ctx, "refs/heads/main") {
        return Ok(Outcome::Done(
            "buf lint clean; breaking skipped (no local `main` to compare against)".to_owned(),
        ));
    }

    let against = if input == "." {
        ".git#branch=main".to_owned()
    } else {
        format!(".git#branch=main,subdir={input}")
    };
    run(
        ctx,
        "buf",
        &[
            "breaking".to_owned(),
            input,
            "--against".to_owned(),
            against,
        ],
    )?;

    // PLACEHOLDER: once v1.1 publishes gp.api.v1, add a second comparison
    // against the last release tag (`.git#tag=vX.Y.Z`) in WIRE_JSON mode —
    // the branch comparison catches churn, the tag comparison catches a
    // released-schema break (AGENTS.md section 9, item 8).
    Ok(Outcome::Done(
        "buf lint clean; no breaking changes against main".to_owned(),
    ))
}

/// Golden files: `tests/golden/<area>/<case>/expected.<ext>` is byte-compared
/// with the fresh output the producing test leaves at
/// `<target>/golden/<area>/<case>/actual.<ext>`, and every area carries a
/// `README.md` saying what a diff in it means. `cargo xtask golden --bless`
/// copies actual over expected — and a blessed golden has to be explained in
/// the pull request that moves it.
///
/// The convention, the per-area README rule and the self-test that a mismatched
/// fixture produces a readable first-difference report all live in
/// [`golden`]; this function is the step wrapper around them.
///
/// Two areas are not this step's to compare —
/// [`golden::SELF_COMPARED_AREAS`]. `determinism/` is compared by
/// [`step_determinism`], which runs the sim and validates the chain's format
/// line by line; `vista/` is compared with a tolerance by [`png`] from
/// [`step_screenshot`], which runs after this step and only on Linux. Both would
/// otherwise fail this step's "a missing fresh output is a failure" rule on a
/// clean checkout, on every operating system.
///
/// PLACEHOLDER: the area layout and its READMEs are committed, but no case has
/// a golden yet, so this skips. The producing side (plan-core's canonical form,
/// the JSONC round-trip, verifier `report_hash`, `render_plan` prose, the
/// mapgen and mesher digests) writes the `actual.*` files as its tests run, and
/// the step self-activates the moment one is committed.
fn step_golden(ctx: &Ctx) -> Result<Outcome, String> {
    let golden_root = ctx.root.join("tests").join("golden");
    let target_dir = match &ctx.workspace {
        Ok(workspace) => workspace.target_dir.clone(),
        Err(_) => ctx.root.join("target"),
    };
    if !golden_root.is_dir() {
        return Ok(Outcome::Skipped(
            "no golden files yet (tests/golden does not exist)".to_owned(),
        ));
    }
    if !golden::has_goldens(&golden_root)? {
        return Ok(Outcome::Skipped(format!(
            "the tests/golden area layout is committed but no case this step compares has an \
             expected.* file yet (the producing tasks fill them; see tests/golden/README.md). \
             {} are compared by the steps that own them",
            golden::SELF_COMPARED_AREAS.join(" and ")
        )));
    }

    let report = golden::compare_tree(&golden_root, &target_dir.join("golden"), ctx.bless)?;
    let deferred = if report.deferred == 0 {
        String::new()
    } else {
        format!(
            "; {} left to the step that owns it ({})",
            report.deferred,
            golden::SELF_COMPARED_AREAS.join(", ")
        )
    };
    if report.blessed > 0 {
        return Ok(Outcome::Done(format!(
            "{} golden file(s) rewritten, {} already matched across {} area(s){deferred} — explain the move in the PR",
            report.blessed, report.matched, report.areas
        )));
    }
    Ok(Outcome::Done(format!(
        "{} golden file(s) match across {} area(s){deferred}",
        report.matched, report.areas
    )))
}

/// Runs the sim's determinism binary, validates the per-tick hash chain it
/// writes, and compares it with the committed chain when there is one. CI
/// uploads the chain per operating system and a later job diffs the three.
///
/// PLACEHOLDER: the binary arrives with spike G4, so until then this reports
/// and passes.
fn step_determinism(ctx: &Ctx) -> Result<Outcome, String> {
    let workspace = match &ctx.workspace {
        Ok(workspace) => workspace,
        Err(reason) => return Ok(Outcome::Skipped(format!("no metadata: {reason}"))),
    };
    let Some(owner) = workspace.package_with_bin(DETERMINISM_BIN) else {
        return Ok(Outcome::Skipped(format!(
            "no `{DETERMINISM_BIN}` binary in the workspace yet (it arrives with spike G4)"
        )));
    };

    let hash_path = workspace.target_dir.join(HASH_FILE);
    if let Some(parent) = hash_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    }
    // Remove any stale chain first, so a crashed run cannot pass on an old one.
    if hash_path.exists() {
        fs::remove_file(&hash_path)
            .map_err(|error| format!("removing {}: {error}", hash_path.display()))?;
    }

    let mut args: Vec<String> = vec![
        "run".to_owned(),
        "--profile".to_owned(),
        DETERMINISM_PROFILE.to_owned(),
        "--package".to_owned(),
        owner,
        "--bin".to_owned(),
        DETERMINISM_BIN.to_owned(),
    ];
    if ctx.locked {
        args.push("--locked".to_owned());
    }
    args.push("--".to_owned());
    args.push("--ticks".to_owned());
    args.push(DETERMINISM_TICKS.to_owned());
    args.push("--out".to_owned());
    args.push(hash_path.to_string_lossy().into_owned());
    run(ctx, &ctx.cargo, &args)?;

    let ticks = validate_hash_file(&hash_path)?;

    // Compare with the committed chain when one exists.
    let golden_path = ctx.root.join(HASH_GOLDEN);
    if golden_path.is_file() {
        let golden = fs::read(&golden_path)
            .map_err(|error| format!("reading {}: {error}", golden_path.display()))?;
        let fresh = fs::read(&hash_path)
            .map_err(|error| format!("reading {}: {error}", hash_path.display()))?;
        if golden != fresh {
            if ctx.bless {
                fs::write(&golden_path, &fresh)
                    .map_err(|error| format!("writing {}: {error}", golden_path.display()))?;
                return Ok(Outcome::Done(format!(
                    "{ticks} ticks; hash chain re-baselined — say in the PR why the hashes moved"
                )));
            }
            return Err(format!(
                "the hash chain moved: {} differs from {}\n{}\n      A moved hash chain is a \
                 behaviour change. Explain it in the pull request; never re-bless it to get a \
                 red build to green.",
                HASH_FILE,
                HASH_GOLDEN,
                first_difference(&golden, &fresh)
            ));
        }
        return Ok(Outcome::Done(format!(
            "{ticks} ticks; hash chain matches {HASH_GOLDEN}"
        )));
    }

    Ok(Outcome::Done(format!(
        "{ticks} per-tick hashes written to {HASH_FILE} (no committed chain to compare yet)"
    )))
}

fn step_reuse(ctx: &Ctx) -> Result<Outcome, String> {
    if !ctx.root.join("REUSE.toml").is_file() {
        return Ok(Outcome::Skipped("no REUSE.toml".to_owned()));
    }
    if !tool_available("reuse") {
        return skip_or_fail(
            ctx,
            "the reuse tool is not installed (`pipx install reuse`)",
        );
    }
    run(ctx, "reuse", &["lint".to_owned()])?;
    Ok(Outcome::Done(
        "every file carries an SPDX header and the manifest is complete".to_owned(),
    ))
}

/// Harness part 2, half one: the scenario files.
///
/// A scenario is a headless match written down — map seed, one playbook per
/// seat, the segment list, and assertions on events **and** on the hash chain
/// (AGENTS.md section 9 item 10, section 10 item 4). `gamectl scenario run`
/// executes one and arrives at T15; T20 promotes this step from skipping to
/// required.
///
/// Until then the step is not idle. Every committed scenario file is parsed and
/// validated against the frozen format — unknown keys rejected, seeds and
/// durations checked, playbook paths resolved, the assertion vocabulary
/// enforced — so a malformed scenario is a red build today rather than a
/// surprise at T15. A file that fails validation is an **error**; the absence
/// of a runner is a **skip**, with the reason and the task named.
fn step_scenario(ctx: &Ctx) -> Result<Outcome, String> {
    let files = scenario::collect(&ctx.root)?;
    if files.is_empty() {
        return Ok(Outcome::Skipped(
            "no scenario files yet (scenarios/**/*.scenario.jsonc matched nothing); \
             scenarios/README.md documents the format"
                .to_owned(),
        ));
    }

    let mut valid: Vec<scenario::Scenario> = Vec::new();
    for file in &files {
        valid.push(scenario::validate(&ctx.root, file)?);
    }
    let names: Vec<String> = valid
        .iter()
        .map(|item| {
            // The rules table is named only when it is not the default one.
            // A scenario pinned to a different table is the interesting case —
            // its hash chain is a claim about those values — and printing the
            // same default path once per scenario would bury it.
            let rules = if item.rules == scenario::DEFAULT_RULES {
                String::new()
            } else {
                format!(", rules {}", item.rules)
            };
            format!(
                "{} ({} seats, {} assertions{rules})",
                item.name, item.seats, item.assertions
            )
        })
        .collect();
    let checked = format!(
        "{} scenario file(s) valid against {}: {}",
        valid.len(),
        scenario::FORMAT,
        names.join(", ")
    );

    let workspace = match &ctx.workspace {
        Ok(workspace) => workspace,
        Err(reason) => {
            return Ok(Outcome::Skipped(format!(
                "{checked}; no metadata: {reason}"
            )));
        }
    };
    let Some(owner) = workspace.package_with_bin(SCENARIO_BIN) else {
        return Ok(Outcome::Skipped(format!(
            "{checked}; there is no `{SCENARIO_BIN}` binary in the workspace yet — \
             `{SCENARIO_BIN} {SCENARIO_SUBCOMMAND} run` arrives with T15, and this step turns \
             itself on when it does"
        )));
    };

    let probe = cargo_run_args(ctx, &owner, &[SCENARIO_SUBCOMMAND, "--help"]);
    if !command_succeeds(&ctx.root, &ctx.cargo, &probe) {
        return Ok(Outcome::Skipped(format!(
            "{checked}; `{SCENARIO_BIN}` exists but has no `{SCENARIO_SUBCOMMAND}` subcommand yet \
             — T15 fills it"
        )));
    }

    for item in &valid {
        // Forward slashes: the path goes on a command line that is quoted in
        // workflow files and in scenario logs on three operating systems.
        let relative = item.relative.to_string_lossy().replace('\\', "/");
        let args = cargo_run_args(ctx, &owner, &[SCENARIO_SUBCOMMAND, "run", &relative]);
        run(ctx, &ctx.cargo, &args)
            .map_err(|error| format!("scenario `{}` failed: {error}", item.name))?;
    }

    Ok(Outcome::Done(format!(
        "{} scenario(s) pass on their event and hash assertions",
        valid.len()
    )))
}

/// Harness part 2, half two: the vista screenshot.
///
/// The obvious shape of this step — run Godot, upload the PNG — asserts
/// nothing; the comparison in [`png`] is the assertion, and its report goes out
/// through [`annotate`] because GitHub hides logs and step summaries from
/// logged-out viewers (G1 section 10.12).
///
/// Four preconditions, each a named skip rather than a quiet pass:
///
/// * the Godot project exists (T16 builds it);
/// * a golden PNG is committed (T16 commits the first one, after eyeballing it
///   for cracks, missing faces and inverted winding);
/// * the platform is Linux — the golden is rendered under **xvfb + lavapipe**,
///   and a second rasteriser's output would make the alarm permanently red
///   (skeleton-plan section 7 decision 22, recommended and not yet logged; G1
///   measured 1.14 % of pixels differing between a Quadro and lavapipe on
///   identical geometry);
/// * `godot` and `xvfb-run` are installed.
///
/// And one pre-step that is not optional: `godot --headless --path godot
/// --import`. A non-editor Godot run loads GDExtensions only from
/// `res://.godot/extension_list.cfg`, which the editor writes when it scans the
/// project and which is git-ignored — so on a fresh checkout, every CI runner,
/// the extension's classes instantiate as placeholders and the first call on
/// them fails. G1 lost four runs to this.
///
/// Note that `--headless` is used **only** for the import. It selects the dummy
/// rendering driver, under which `frame_post_draw` never fires and a screenshot
/// coroutine parks for ever — silently, producing no PNG and no error. The
/// render itself runs windowed under xvfb.
fn step_screenshot(ctx: &Ctx) -> Result<Outcome, String> {
    let project = ctx.root.join(GODOT_PROJECT_DIR);
    if !project.join("project.godot").is_file() {
        return Ok(Outcome::Skipped(format!(
            "no {GODOT_PROJECT_DIR}/project.godot yet — the Godot project and the vista arrive \
             with T16, and this step turns itself on when they do"
        )));
    }
    let golden_path = ctx.root.join(VISTA_GOLDEN);
    if !golden_path.is_file() {
        return Ok(Outcome::Skipped(format!(
            "no committed vista golden at {VISTA_GOLDEN} yet — T16 commits the first one after \
             eyeballing it for cracks, missing faces and inverted winding"
        )));
    }
    if !cfg!(target_os = "linux") {
        return Ok(Outcome::Skipped(
            "the vista golden is rendered under xvfb + lavapipe on Linux only (skeleton-plan \
             section 7 decision 22, recommended and not yet logged); comparing a second \
             platform's rasteriser against it would be permanently red and would say nothing \
             about the geometry"
                .to_owned(),
        ));
    }
    if !tool_available("godot") {
        return skip_or_fail(
            ctx,
            "godot is not installed (the CI job installs the pinned 4.7.2 build)",
        );
    }
    if !tool_available("xvfb-run") {
        return skip_or_fail(
            ctx,
            "xvfb-run is not installed (`apt-get install xvfb`); `godot --headless` is not a \
             substitute — it selects the dummy driver and cannot take a screenshot",
        );
    }

    // The pre-step every fresh checkout needs. Without it the extension's
    // classes are placeholders and the scene fails on its first call.
    run(
        ctx,
        "godot",
        &[
            "--headless".to_owned(),
            "--path".to_owned(),
            GODOT_PROJECT_DIR.to_owned(),
            "--import".to_owned(),
        ],
    )?;

    let target_dir = match &ctx.workspace {
        Ok(workspace) => workspace.target_dir.clone(),
        Err(_) => ctx.root.join("target"),
    };
    let actual_path = target_dir.join(VISTA_ACTUAL);
    if let Some(parent) = actual_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    }
    // A stale PNG from a previous run must not be able to pass for this one.
    if actual_path.exists() {
        fs::remove_file(&actual_path)
            .map_err(|error| format!("removing {}: {error}", actual_path.display()))?;
    }

    // PLACEHOLDER: the scene name and the "take one shot and quit" flag are
    // T16's — the project does not exist yet, so this command line has never
    // run. Owner/T16 ratifies it with the first real vista.
    run(
        ctx,
        "xvfb-run",
        &[
            "-a".to_owned(),
            "-s".to_owned(),
            VISTA_SCREEN.to_owned(),
            "godot".to_owned(),
            "--path".to_owned(),
            GODOT_PROJECT_DIR.to_owned(),
            "--resolution".to_owned(),
            VISTA_RESOLUTION.to_owned(),
            "--".to_owned(),
            format!("--scene={VISTA_SCENE}"),
            format!("--shot={}", actual_path.to_string_lossy()),
        ],
    )?;

    let shot_bytes = fs::read(&actual_path).map_err(|error| {
        format!(
            "{} was not produced ({error}). Godot rendered nothing — check that the run was \
             windowed under xvfb rather than `--headless`, which selects the dummy driver and \
             parks a screenshot coroutine for ever",
            actual_path.display()
        )
    })?;
    let shot =
        png::decode(&shot_bytes).map_err(|error| format!("{}: {error}", actual_path.display()))?;
    let variance = shot.red_variance()?;
    if variance < png::MIN_VARIANCE {
        return Err(format!(
            "the vista is blank or near-uniform (red-channel variance {variance}, floor {}); \
             nothing was drawn",
            png::MIN_VARIANCE
        ));
    }

    let golden_bytes = fs::read(&golden_path)
        .map_err(|error| format!("reading {}: {error}", golden_path.display()))?;
    let golden_image =
        png::decode(&golden_bytes).map_err(|error| format!("{VISTA_GOLDEN}: {error}"))?;
    let diff = png::compare(&shot, &golden_image)?;

    let report = format!(
        "{} ({}x{}, red-channel variance {variance})\nvs {VISTA_GOLDEN}\n{}",
        actual_path.display(),
        shot.width,
        shot.height,
        diff.describe()
    );
    annotate::notice("vista-screenshot", &report);
    diff.check(png::MAX_MEAN_THOUSANDTHS, png::MAX_HARD_PPM)?;

    Ok(Outcome::Done(format!(
        "the vista matches {VISTA_GOLDEN} ({} of {} pixels differ at all)",
        diff.differing, diff.pixels
    )))
}

/// The `cargo run` argument list for one of the workspace's own binaries.
fn cargo_run_args(ctx: &Ctx, package: &str, tail: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "run".to_owned(),
        "--quiet".to_owned(),
        "--package".to_owned(),
        package.to_owned(),
        "--bin".to_owned(),
        SCENARIO_BIN.to_owned(),
    ];
    if ctx.locked {
        args.push("--locked".to_owned());
    }
    args.push("--".to_owned());
    for argument in tail {
        args.push((*argument).to_owned());
    }
    args
}

/// Runs a command for its exit status alone, swallowing its output. Used to ask
/// "does this subcommand exist yet", where a non-zero status is an answer and
/// not a failure.
fn command_succeeds(cwd: &Path, program: &str, args: &[String]) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// A missing external tool is a skip locally and a failure in CI, where
/// `--require-tools` is set: a runner without buf must not look green.
fn skip_or_fail(ctx: &Ctx, message: &str) -> Result<Outcome, String> {
    if ctx.require_tools {
        Err(message.to_owned())
    } else {
        Ok(Outcome::Skipped(format!("{message} — step skipped")))
    }
}

// ---------------------------------------------------------------------------
// Hash chain validation
// ---------------------------------------------------------------------------

/// Checks the hash file against the documented convention and returns the tick
/// count. Strict on purpose: the file is byte-compared across three operating
/// systems, so a stray `\r` or an out-of-order tick is a real defect, not a
/// cosmetic one.
fn validate_hash_file(path: &Path) -> Result<usize, String> {
    let bytes = fs::read(path).map_err(|error| format!("reading {}: {error}", path.display()))?;
    if bytes.is_empty() {
        return Err(format!("{} is empty", path.display()));
    }
    if bytes.contains(&b'\r') {
        return Err(format!(
            "{} contains a carriage return; the hash chain uses \\n endings so the three \
             operating systems produce byte-identical files",
            path.display()
        ));
    }
    if bytes.last() != Some(&b'\n') {
        return Err(format!("{} must end with a newline", path.display()));
    }
    let text = String::from_utf8(bytes)
        .map_err(|error| format!("{} is not UTF-8: {error}", path.display()))?;

    let mut expected_tick: u64 = 0;
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let Some((tick_text, hash_text)) = line.split_once('\t') else {
            return Err(format!(
                "{}:{number}: expected `<tick>\\t<16 hex digits>`, found `{line}`",
                path.display()
            ));
        };
        let tick: u64 = tick_text.parse().map_err(|error| {
            format!(
                "{}:{number}: tick `{tick_text}` is not a number: {error}",
                path.display()
            )
        })?;
        if tick != expected_tick {
            return Err(format!(
                "{}:{number}: ticks run in order from 0; expected {expected_tick}, found {tick}",
                path.display()
            ));
        }
        let hash_is_valid = hash_text.len() == 16
            && hash_text
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
        if !hash_is_valid {
            return Err(format!(
                "{}:{number}: `{hash_text}` is not 16 lowercase hex digits (xxh3-64)",
                path.display()
            ));
        }
        expected_tick = expected_tick
            .checked_add(1)
            .ok_or_else(|| "the tick counter overflowed".to_owned())?;
    }

    usize::try_from(expected_tick).map_err(|error| format!("the tick count does not fit: {error}"))
}

// ---------------------------------------------------------------------------
// Workspace metadata
// ---------------------------------------------------------------------------

struct Workspace {
    packages: Vec<Package>,
    /// Cargo's `target_directory` from `cargo metadata`: honours `CARGO_TARGET_DIR`
    /// and `build.target-dir`, so build products are looked up where Cargo put them.
    target_dir: PathBuf,
}

struct Package {
    name: String,
    features: Vec<String>,
    bins: Vec<String>,
}

impl Workspace {
    /// The real package names matching any of `candidates`, comparing both the
    /// bare name and the `pharmakos-`-prefixed one. Sorted and deduplicated —
    /// ordered collections, no hash sets.
    fn present(&self, candidates: &[&str]) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for package in &self.packages {
            let bare = package
                .name
                .strip_prefix("pharmakos-")
                .unwrap_or(&package.name);
            if candidates.contains(&bare) {
                found.push(package.name.clone());
            }
        }
        found.sort();
        found.dedup();
        found
    }

    fn package_with_feature(&self, candidates: &[&str], feature: &str) -> Option<String> {
        let names = self.present(candidates);
        self.packages
            .iter()
            .find(|package| {
                names.contains(&package.name)
                    && package.features.iter().any(|owned| owned == feature)
            })
            .map(|package| package.name.clone())
    }

    fn package_with_bin(&self, bin: &str) -> Option<String> {
        self.packages
            .iter()
            .find(|package| package.bins.iter().any(|name| name == bin))
            .map(|package| package.name.clone())
    }
}

fn load_workspace(cargo: &str, root: &Path) -> Result<Workspace, String> {
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("failed to run `{cargo} metadata`: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "`{cargo} metadata` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|error| format!("`{cargo} metadata` produced non-UTF-8 output: {error}"))?;
    parse_workspace(&text)
}

fn parse_workspace(metadata: &str) -> Result<Workspace, String> {
    let json = Json::parse(metadata)?;
    let entries = json
        .get("packages")
        .and_then(Json::as_array)
        .ok_or_else(|| "cargo metadata has no `packages` array".to_owned())?;

    let mut packages: Vec<Package> = Vec::new();
    for entry in entries {
        let Some(name) = entry.get("name").and_then(Json::as_str) else {
            continue;
        };
        let features: Vec<String> = match entry.get("features").and_then(Json::as_object) {
            Some(members) => members.iter().map(|(key, _)| key.clone()).collect(),
            None => Vec::new(),
        };
        let mut bins: Vec<String> = Vec::new();
        if let Some(targets) = entry.get("targets").and_then(Json::as_array) {
            for target in targets {
                let is_bin = target
                    .get("kind")
                    .and_then(Json::as_array)
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("bin")));
                if is_bin {
                    if let Some(target_name) = target.get("name").and_then(Json::as_str) {
                        bins.push(target_name.to_owned());
                    }
                }
            }
        }
        packages.push(Package {
            name: name.to_owned(),
            features,
            bins,
        });
    }
    let target_dir = json
        .get("target_directory")
        .and_then(Json::as_str)
        .map(PathBuf::from)
        .or_else(|| {
            json.get("workspace_root")
                .and_then(Json::as_str)
                .map(|root| Path::new(root).join("target"))
        })
        .ok_or_else(|| {
            "cargo metadata has neither `target_directory` nor `workspace_root`".to_owned()
        })?;
    Ok(Workspace {
        packages,
        target_dir,
    })
}

/// The `--features` argument that turns the research build on — for example
/// `pharmakos-sim/research` — or `None` while no crate declares the feature.
fn research_feature_spec(ctx: &Ctx) -> Option<String> {
    let workspace = ctx.workspace.as_ref().ok()?;
    workspace
        .package_with_feature(SIM_PACKAGES, RESEARCH_FEATURE)
        .map(|name| format!("{name}/{RESEARCH_FEATURE}"))
}

fn walled_packages(ctx: &Ctx) -> Vec<String> {
    match &ctx.workspace {
        Ok(workspace) => workspace.present(WALLED_PACKAGES),
        Err(_) => Vec::new(),
    }
}

/// True when a `cargo tree -e features --format {p}|{f}` line shows the
/// research feature enabled — as a feature node, or in a package's feature list.
fn line_enables_research(line: &str) -> bool {
    let (package, features) = line.split_once('|').unwrap_or((line, ""));
    if package.contains(&format!("feature \"{RESEARCH_FEATURE}\"")) {
        return true;
    }
    features
        .split(',')
        .any(|feature| feature.trim() == RESEARCH_FEATURE)
}

// ---------------------------------------------------------------------------
// A minimal JSON reader (std only)
// ---------------------------------------------------------------------------

/// Numbers keep their source text: xtask needs none of their values, and
/// holding them as text keeps `f64` out of a crate that forbids floats.
// The parser accepts every JSON type, because one that skipped a type would
// quietly accept malformed input; xtask itself only ever reads strings, arrays
// and objects, so the other payloads are written and never read.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum Json {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn parse(input: &str) -> Result<Json, String> {
        let mut parser = JsonParser {
            bytes: input.as_bytes(),
            index: 0,
        };
        let value = parser.value()?;
        parser.skip_whitespace();
        if parser.index < parser.bytes.len() {
            return Err(format!("trailing JSON input at byte {}", parser.index));
        }
        Ok(value)
    }

    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(text) => Some(text),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    fn as_object(&self) -> Option<&Vec<(String, Json)>> {
        match self {
            Json::Object(members) => Some(members),
            _ => None,
        }
    }
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl JsonParser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.index += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.index += 1;
            Ok(())
        } else {
            Err(format!(
                "expected `{}` at byte {}",
                char::from(byte),
                self.index
            ))
        }
    }

    fn literal(&mut self, text: &str) -> Result<(), String> {
        let end = self.index + text.len();
        if self.bytes.get(self.index..end) == Some(text.as_bytes()) {
            self.index = end;
            Ok(())
        } else {
            Err(format!("expected `{text}` at byte {}", self.index))
        }
    }

    fn slice(&self, start: usize, end: usize) -> Result<&str, String> {
        let bytes = self
            .bytes
            .get(start..end)
            .ok_or_else(|| format!("unexpected end of JSON input at byte {start}"))?;
        std::str::from_utf8(bytes).map_err(|error| format!("JSON input is not UTF-8: {error}"))
    }

    fn value(&mut self) -> Result<Json, String> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => {
                self.literal("true")?;
                Ok(Json::Bool(true))
            }
            Some(b'f') => {
                self.literal("false")?;
                Ok(Json::Bool(false))
            }
            Some(b'n') => {
                self.literal("null")?;
                Ok(Json::Null)
            }
            Some(_) => self.number(),
            None => Err("unexpected end of JSON input".to_owned()),
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut members: Vec<(String, Json)> = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.index += 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            let value = self.value()?;
            members.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.index += 1,
                Some(b'}') => {
                    self.index += 1;
                    return Ok(Json::Object(members));
                }
                _ => return Err(format!("expected `,` or `}}` at byte {}", self.index)),
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut items: Vec<Json> = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.index += 1;
            return Ok(Json::Array(items));
        }
        loop {
            let value = self.value()?;
            items.push(value);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.index += 1,
                Some(b']') => {
                    self.index += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(format!("expected `,` or `]` at byte {}", self.index)),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self
                .peek()
                .ok_or_else(|| "unterminated JSON string".to_owned())?;
            match byte {
                b'"' => {
                    self.index += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.index += 1;
                    let escape = self
                        .peek()
                        .ok_or_else(|| "unterminated JSON escape".to_owned())?;
                    self.index += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let end = self.index + 4;
                            let code = {
                                let digits = self.slice(self.index, end)?;
                                u32::from_str_radix(digits, 16)
                                    .map_err(|error| format!("bad \\u escape: {error}"))?
                            };
                            self.index = end;
                            // cargo metadata never emits surrogate pairs; an
                            // unpaired surrogate becomes U+FFFD rather than
                            // failing the whole run.
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        other => {
                            return Err(format!("unknown JSON escape `\\{}`", char::from(other)));
                        }
                    }
                }
                // Everything else is copied through in one chunk. Bytes >= 0x80
                // belong to multi-byte UTF-8 sequences and never collide with
                // the ASCII delimiters scanned for here.
                _ => {
                    let start = self.index;
                    let mut end = self.index;
                    while self
                        .bytes
                        .get(end)
                        .is_some_and(|byte| *byte != b'"' && *byte != b'\\')
                    {
                        end += 1;
                    }
                    out.push_str(self.slice(start, end)?);
                    self.index = end;
                }
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.index;
        while matches!(
            self.peek(),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.index += 1;
        }
        if start == self.index {
            return Err(format!("unexpected byte at {}", self.index));
        }
        Ok(Json::Num(self.slice(start, self.index)?.to_owned()))
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn workspace_root() -> Result<PathBuf, String> {
    if let Ok(dir) = env::var("CARGO_MANIFEST_DIR") {
        let manifest_dir = PathBuf::from(dir);
        if let Some(parent) = manifest_dir.parent() {
            if parent.join("Cargo.toml").is_file() {
                return Ok(parent.to_path_buf());
            }
        }
    }
    let mut current = env::current_dir()
        .map_err(|error| format!("cannot read the current directory: {error}"))?;
    loop {
        if current.join("Cargo.toml").is_file() && current.join("xtask").is_dir() {
            return Ok(current);
        }
        if !current.pop() {
            return Err(
                "could not find the workspace root (a directory holding Cargo.toml and xtask/)"
                    .to_owned(),
            );
        }
    }
}

fn run(ctx: &Ctx, program: &str, args: &[String]) -> Result<(), String> {
    let root = ctx.root.clone();
    run_in(ctx, program, args, &root)
}

fn run_in(_ctx: &Ctx, program: &str, args: &[String], cwd: &Path) -> Result<(), String> {
    println!("   $ {}", render_command(program, args));
    let status = Command::new(program)
        .args(args)
        .current_dir(cwd)
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

fn capture(ctx: &Ctx, program: &str, args: &[String]) -> Result<String, String> {
    println!("   $ {}", render_command(program, args));
    let output = Command::new(program)
        .args(args)
        .current_dir(&ctx.root)
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("failed to launch `{program}`: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "`{}` failed ({}): {}",
            render_command(program, args),
            describe_exit(output.status),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("`{program}` produced non-UTF-8 output: {error}"))
}

fn describe_exit(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => "terminated by a signal".to_owned(),
    }
}

fn render_command(program: &str, args: &[String]) -> String {
    let mut line = String::from(program);
    for arg in args {
        line.push(' ');
        if arg.contains(' ') {
            line.push('"');
            line.push_str(arg);
            line.push('"');
        } else {
            line.push_str(arg);
        }
    }
    line
}

/// True when the tool answers `--version`. On Windows this finds `tool.exe` but
/// not a `tool.cmd` shim, which is why CI installs buf as a real binary.
fn tool_available(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn cargo_subcommand_available(cargo: &str, subcommand: &str) -> bool {
    Command::new(cargo)
        .args([subcommand, "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn git_ref_exists(ctx: &Ctx, reference: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", reference])
        .current_dir(&ctx.root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The directory holding the buf configuration, if there is one yet.
fn buf_config_dir(root: &Path) -> Option<PathBuf> {
    for candidate in ["buf.work.yaml", "buf.yaml"] {
        if root.join(candidate).is_file() {
            return Some(root.to_path_buf());
        }
    }
    let proto = root.join("proto");
    for candidate in ["buf.work.yaml", "buf.yaml"] {
        if proto.join(candidate).is_file() {
            return Some(proto);
        }
    }
    None
}

fn file_name(path: &Path) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => String::new(),
    }
}

/// Walks `dir` in sorted order and collects every file below it. Sorted because
/// `read_dir` order differs between ext4, NTFS and APFS, and a golden run must
/// not depend on which filesystem it happens to be on.
// `fs::read_dir` is on clippy.toml's disallowed-methods list for exactly that
// reason. Allowed here because the entries are sorted before anything reads
// them — which is the remedy the ban asks for, not a way around it.
#[allow(clippy::disallowed_methods)]
fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(dir)? {
        entries.push(entry?.path());
    }
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// A readable diff: where the two files first part company, and both lines.
fn first_difference(expected: &[u8], actual: &[u8]) -> String {
    let mut offset: usize = 0;
    for (left, right) in expected.iter().zip(actual.iter()) {
        if left != right {
            break;
        }
        offset += 1;
    }
    let line_number = expected
        .iter()
        .take(offset)
        .filter(|byte| **byte == b'\n')
        .count()
        + 1;
    format!(
        "      first difference at line {line_number} (byte {offset})\n      \
         expected: {}\n      actual:   {}",
        line_at(expected, offset),
        line_at(actual, offset)
    )
}

fn line_at(bytes: &[u8], offset: usize) -> String {
    if bytes.is_empty() {
        return "<empty file>".to_owned();
    }
    let clamped = offset.min(bytes.len() - 1);
    let start = bytes
        .iter()
        .take(clamped)
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    let end = bytes
        .iter()
        .skip(start)
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |position| start + position);
    let text = String::from_utf8_lossy(bytes.get(start..end).unwrap_or_default()).into_owned();
    if text.chars().count() > 120 {
        let mut truncated: String = text.chars().take(120).collect();
        truncated.push_str(" …");
        truncated
    } else {
        text
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_names_features_and_bins_from_metadata() {
        let metadata = r#"{"packages":[
            {"name":"pharmakos-sim","features":{"default":[],"research":[]},
             "targets":[{"kind":["lib"],"name":"pharmakos_sim"},
                        {"kind":["bin"],"name":"determinism"}]},
            {"name":"pharmakos-gateway","features":{},"targets":[{"kind":["lib"],"name":"x"}]}
        ],"version":1,"workspace_root":"/tmp/x"}"#;
        let workspace = parse_workspace(metadata).expect("metadata parses");

        assert_eq!(
            workspace.present(SIM_PACKAGES),
            vec!["pharmakos-sim".to_owned()]
        );
        assert_eq!(
            workspace.package_with_feature(SIM_PACKAGES, RESEARCH_FEATURE),
            Some("pharmakos-sim".to_owned())
        );
        assert_eq!(
            workspace.package_with_bin("determinism"),
            Some("pharmakos-sim".to_owned())
        );
        assert_eq!(workspace.package_with_bin("gamectl"), None);
        assert_eq!(
            workspace.present(GUARDED_PACKAGES),
            vec!["pharmakos-gateway".to_owned()]
        );
    }

    #[test]
    fn json_strings_handle_escapes_and_unicode() {
        let json = Json::parse(r#"{"a":"b\\c\"d\n","b":"é ok"}"#).expect("parses");
        assert_eq!(json.get("a").and_then(Json::as_str), Some("b\\c\"d\n"));
        assert_eq!(json.get("b").and_then(Json::as_str), Some("é ok"));
    }

    #[test]
    fn json_rejects_trailing_input() {
        assert!(Json::parse("{} {}").is_err());
        assert!(Json::parse("{\"a\":").is_err());
    }

    #[test]
    fn research_edges_are_detected() {
        assert!(line_enables_research(
            "pharmakos-sim v0.0.0|default,research"
        ));
        assert!(line_enables_research("pharmakos-sim feature \"research\"|"));
        assert!(!line_enables_research(
            "pharmakos-sim v0.0.0|default,replay"
        ));
        assert!(!line_enables_research("pharmakos-gateway v0.0.0|"));
        // `researching` must not trip it.
        assert!(!line_enables_research("crate v1|researching"));
    }

    #[test]
    fn hash_chains_are_validated() {
        let dir = env::temp_dir().join("pharmakos-xtask-tests");
        fs::create_dir_all(&dir).expect("temp dir");

        let good = dir.join("good.txt");
        fs::write(&good, "0\t0123456789abcdef\n1\tfedcba9876543210\n").expect("write");
        assert_eq!(validate_hash_file(&good).expect("valid"), 2);

        let out_of_order = dir.join("out-of-order.txt");
        fs::write(&out_of_order, "0\t0123456789abcdef\n2\tfedcba9876543210\n").expect("write");
        assert!(validate_hash_file(&out_of_order).is_err());

        let crlf = dir.join("crlf.txt");
        fs::write(&crlf, "0\t0123456789abcdef\r\n").expect("write");
        assert!(validate_hash_file(&crlf).is_err());

        let uppercase = dir.join("uppercase.txt");
        fs::write(&uppercase, "0\t0123456789ABCDEF\n").expect("write");
        assert!(validate_hash_file(&uppercase).is_err());

        let short = dir.join("short.txt");
        fs::write(&short, "0\tdeadbeef\n").expect("write");
        assert!(validate_hash_file(&short).is_err());
    }

    #[test]
    fn profiles_are_read_out_of_manifest_text() {
        let good = "\
[workspace]\n\
members = [\"xtask\"]\n\
\n\
[profile.release]\n\
overflow-checks = true   # stays on, spec section 15\n\
lto = \"thin\"\n\
\n\
[profile.dev]\n\
overflow-checks = true\n";
        assert_eq!(scan_profiles(good), (true, Vec::new()));

        let switched_off = "[profile.release]\noverflow-checks = false\n";
        assert_eq!(
            scan_profiles(switched_off),
            (false, vec!["profile.release".to_owned()])
        );

        // A key outside a profile table must not be mistaken for one.
        let elsewhere = "[workspace.metadata]\noverflow-checks = false\n";
        assert_eq!(scan_profiles(elsewhere), (false, Vec::new()));
    }

    #[test]
    fn diffs_name_the_first_differing_line() {
        let report = first_difference(b"alpha\nbeta\n", b"alpha\ngamma\n");
        assert!(report.contains("line 2"), "{report}");
        assert!(report.contains("beta"), "{report}");
        assert!(report.contains("gamma"), "{report}");
    }

    #[test]
    fn every_quick_step_is_a_real_step() {
        for name in QUICK_STEPS {
            assert!(
                STEPS.iter().any(|step| step.name == *name),
                "--quick names a step that does not exist: {name}"
            );
        }
    }
}

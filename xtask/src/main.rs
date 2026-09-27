// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask` — the one command that runs every check.
//!
//! `cargo xtask ci` runs the steps below in order and stops at the first
//! failure (fail fast). Each step is a plain function that shells out; nothing
//! here is clever, and nothing here has a dependency (std only), so a cold
//! checkout can run it before a single third-party crate is fetched.
//!
//! Steps, in order — AGENTS.md section 9 items 1 to 10, in code. Item 11 (the
//! G3′ tick budget and the P1 verifier budgets) arrives with the gates that
//! set those numbers, S1's and S2's; until then performance is published, never
//! gated, by `cargo xtask perf-alarms` (below), which is not a step.
//!
//! | step             | what it does                                                                 |
//! |------------------|------------------------------------------------------------------------------|
//! | `fmt`            | `cargo fmt --all --check`                                                     |
//! | `clippy`         | `-D warnings` plus the determinism lint set; walled crates linted separately  |
//! | `profiles`       | overflow checks are on in every profile, release included                     |
//! | `test`           | `cargo test --workspace` without the `research` feature                       |
//! | `test-research`  | `cargo test --package pharmakos-sim --features pharmakos-sim/research`        |
//! | `research-guard` | plan-core / verifier / operator / gateway / gamectl must not reach `research` |
//! | `wall-guard`     | those crates and `sim` never reach a walled crate; the client never the core  |
//! | `deny`           | `cargo deny check` (licences, advisories, banned crates)                      |
//! | `buf`            | `buf lint` and `buf breaking --against .git#branch=main`                      |
//! | `golden`         | `tests/golden/**/expected.*` against the fresh `target/golden/**/actual.*`    |
//! | `determinism`    | runs the determinism binary and compares its per-tick hash chain              |
//! | `reuse`          | REUSE licence-manifest check                                                  |
//! | `scenario`       | validates `scenarios/**`, then plays each with `gamectl scenario run`         |
//! | `screenshot`     | stages the client, renders the three shots under xvfb + lavapipe, compares    |
//! | `stage-client`   | builds the gdext cdylib, stages it into `godot/bin` and imports the project   |
//!
//! The formats of the last three — the scenario file ([`scenario`]), the
//! golden-file convention ([`golden`]) and the screenshot comparison ([`png`])
//! — were frozen before their producers existed (decisions-log item 75), so
//! that every task delivered *into* a format rather than inventing one.
//!
//! # Required means required
//!
//! A step whose input is missing — no scenario files, no `gamectl`, no
//! committed golden, no determinism chain, no `deny.toml`, no buf config, no
//! workspace metadata — **fails** with the reason (decisions-log item
//! 116 (6)(g)). A step never reports `ok` for work it did not do. Three skips
//! remain, and each is a statement about the platform or the caller rather than
//! a missing producer:
//!
//! * `screenshot` off Linux: the PNG goldens are rendered under xvfb and
//!   lavapipe on Linux only (skeleton-plan section 7 decision 22, logged as
//!   decisions-log item 116 (6)(a));
//! * `stage-client` on macOS: `godot/pharmakos.gdextension` declares no macOS
//!   library (decisions-log item 73);
//! * `test-research` when `--package` leaves `pharmakos-sim` out: a filter the
//!   caller chose.
//!
//! A missing *tool* (buf, cargo-deny, reuse, godot, xvfb-run) is a skip
//! locally and a failure under `--require-tools`, which CI sets.
//!
//! # Where AGENTS.md section 9 item 8's regenerate-and-compare runs
//!
//! Not in the `buf` step, and not twice. The prost tree and its descriptor set
//! are regenerated and diffed by `crates/proto/tests/generated.rs`; the
//! `get_schema` answer by `crates/gateway/tests/methods.rs`; `gamectl docs` by
//! `crates/gamectl/tests/docs.rs` — all three through the `test` step, and the
//! last two write fresh outputs the `golden` step compares. Generated JSON
//! Schema is v1.1's (decisions-log item 116 (6)(p)).
//!
//! # The perf alarms
//!
//! `cargo xtask perf-alarms` is not a step: `ci` and `list` do not know it. It
//! runs the mesher's p99 alarm exactly as `crates/mesher/tests/perf_alarm.rs`
//! says (`--release`, `--ignored`, `--nocapture`), which prints its own
//! `::notice::` lines, and publishes the allocations per tick as a notice
//! naming the test that asserts it. It has no threshold, compares nothing
//! across operating systems, and never exits non-zero: a failed measurement is
//! a `::warning::` (skeleton-plan section 7 decision 23, logged as
//! decisions-log item 116 (6)(b)).
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
//! cargo xtask stage-client --check  # build the cdylib, stage, import, run the client check
//! cargo xtask list                  # list the steps
//! cargo xtask ci-scope              # CI's docs-only fast path: `full` or `prose`
//! cargo xtask perf-alarms           # the per-runner perf notices; never fails
//! cargo xtask package               # the unsigned zip for this platform (T21)
//! ```
//!
//! # The research tests
//!
//! `test-research` runs `pharmakos-sim`'s tests alone with the `research`
//! feature on, because the sim is the one crate whose code the feature changes
//! (`pub mod research` in its `lib.rs`, and `tests/fork.rs`); no other crate
//! reads it, so their tests run once, in `test` (decisions-log item 113 (9),
//! chosen by item 115 (5)). Every non-walled crate is still *compiled* against
//! a research sim, by `clippy`'s research pass on an unscoped run. With `-p`,
//! the step runs when the list may name the sim (its name, `name@version`, a
//! glob or a package-id URL) and skips, with the reason, when it cannot.
//!
//! # The docs-only fast path
//!
//! `cargo xtask ci-scope` is not a step: `ci` and `list` do not know it. Each
//! CI job that builds runs it after its checkout and toolchain steps; it prints
//! `full` or `prose` with its reason and appends `prose_only=true|false` to
//! `$GITHUB_OUTPUT`, and the job's later steps run only when the answer is not
//! `prose`. It answers `prose` only for a `pull_request` whose merge commit
//! changes nothing but `docs/**`, `AGENTS.md`, `CLAUDE.md`, `.claude/**` and
//! top-level `*.md`, and `full` on any error (decisions-log item 115 (4)); see
//! [`scope`] for the rules and why each holds.
//!
//! # The package
//!
//! `cargo xtask package` is not a step either: `ci` and `list` do not know it.
//! It builds, exports, checks and zips this platform's unsigned build — the
//! Windows or Linux zip — and every check it makes runs here as in CI's
//! `package (<os>)` jobs, which run it and nothing else of substance
//! (skeleton plan T21; decisions-log item 117). See [`package`] for its steps
//! and [`zip`] for the zip format; `--bless` rewrites its manifest golden,
//! `tests/golden/package/expected.<platform>.txt`, which the `golden` step
//! leaves to it ([`golden::SELF_COMPARED_AREAS`]).
//!
//! # The determinism hash file
//!
//! This is a contract: `.github/workflows/ci.yml` uploads exactly this path per
//! operating system, beside the fresh scenario chains, the path hashes and the
//! mesher, mapgen and vista geometry digests, and the `cross-OS determinism
//! guard` job byte-compares each of them across the three.
//!
//! * path: `target/determinism/hashes.txt`, relative to the workspace root;
//! * one line per simulated tick, in tick order, starting at tick 0;
//! * each line is `<tick in decimal>` TAB `<xxh3-64 state hash as 16 lowercase
//!   hex digits>`;
//! * `\n` endings — never `\r\n`, because the file is byte-compared across
//!   Windows, Linux and macOS — and a trailing newline at end of file;
//! * the fresh file must equal `tests/golden/determinism/expected.hashes.txt`
//!   byte for byte; a missing committed chain is a failure, and
//!   `cargo xtask determinism --bless` is how one is written.
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
    clippy::missing_panics_doc,    // nothing here is a library API
    clippy::struct_excessive_bools // `Ctx` is the parsed flag list; each bool IS a flag
)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus, Stdio};

// Harness part 2's formats, each in its own file so that the step table above
// stays readable and so that a task working on one of them touches one file.
mod annotate;
mod golden;
mod package;
mod png;
mod scenario;
mod scope;
mod zip;

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
/// which does include `sim`. `gamectl` joined at decisions-log item 109: it
/// takes the sim with default features off, because the gateway's API is
/// written in the sim's types.
const GUARDED_PACKAGES: &[&str] = &["plan-core", "verifier", "operator", "gateway", "gamectl"];

/// Crates that may never depend on a walled crate, transitively included.
///
/// [`GUARDED_PACKAGES`] plus `sim`. The guarded crates must not reach the
/// float, cast, hash-map and clock allowance, and neither must the sim — it is
/// the crate that owns hashed state, so it is the one the wall exists to
/// protect (AGENTS.md section 4.9). The two lists are separate rather than one
/// because `sim` defines the `research` feature and so cannot join the research
/// guard. Keep this list, AGENTS.md section 4.9 and `clippy.toml`'s header in
/// step; widening it is a contract change like any other.
const WALL_GUARDED_PACKAGES: &[&str] = &[
    "sim",
    "plan-core",
    "verifier",
    "operator",
    "gateway",
    "gamectl",
];

/// The client side of the wall: walled crates that may never reach the named
/// deterministic crates, transitively included (decisions-log item 102 (3),
/// taken as item 116 (6)(d)).
///
/// `wall-guard`'s second relation. The first keeps the deterministic crates
/// away from the float, cast, hash-map and clock allowance; this one keeps the
/// thin client away from the rules. `client-gdext` marshals and decides nothing
/// (AGENTS.md section 3 rule 4), so it reaches neither the sim nor the crates
/// that hold a rule, a verdict or a socket; the mesher takes integer chunk data
/// in and "nothing from the sim". Each entry is a walled crate by name, and a
/// future walled crate joins by adding its own line. It is deliberately NOT
/// "no walled crate reaches the sim": that would forbid the walled harness
/// AGENTS.md section 4.5 sanctions, a walled crate driving the sim from
/// outside to put a millisecond figure on it. Names are bare, as in
/// [`WALLED_PACKAGES`], and matched with and without the `pharmakos-` prefix.
/// `crates/client-gdext/tests/no_sim.rs` checks the client's line from
/// `Cargo.lock` as well; the two agree by construction.
const CLIENT_WALL: &[(&str, &[&str])] = &[
    ("client-gdext", &["sim", "verifier", "plan-core", "gateway"]),
    ("mesher", &["sim"]),
];

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

/// PLACEHOLDER: 1 200 ticks is one minute at 20 Hz — a smoke run of the sim's
/// harness segment list. It stays until S1 (owner, at S1): the full-segment
/// chain T10 asked for is the demo scenario's, `scenarios/skeleton/
/// against-easy-three-rounds`, whose three rounds are played and compared on
/// all three operating systems, so lengthening this run would add leg time and
/// a `crates/sim` determinism change for no new coverage (decisions-log item
/// 116 (6)(e)). G4's bar (10 matches × 9 600 ticks) belongs in the nightly
/// workflow rather than in every pull request.
const DETERMINISM_TICKS: &str = "1200";

/// The profile the determinism run uses: release optimisation with debug
/// assertions still on, declared in the root `Cargo.toml`.
const DETERMINISM_PROFILE: &str = "release-checked";

/// The inner-loop subset, as documented in AGENTS.md and CLAUDE.md.
const QUICK_STEPS: &[&str] = &["fmt", "clippy", "test"];

// --- harness part 2 (AGENTS.md section 9 item 10) ---------------------------
//
// These constants are the *formats* the skeleton froze in wave 1, now with
// their producers: `gamectl scenario run` (T15), the Godot vista (T16) and the
// editor's rows and wizard shots (T19). Both steps are required: a missing
// input is a failure, and only the screenshot's platform statement skips.

/// The binary that runs a scenario, and the subcommand it must carry.
const SCENARIO_BIN: &str = "gamectl";
const SCENARIO_SUBCOMMAND: &str = "scenario";

/// Where the Godot project lives (decisions-log item 72: `godot/` at the
/// repository root, one ownable unit with `crates/client-gdext`).
const GODOT_PROJECT_DIR: &str = "godot";

/// One screenshot the `screenshot` step renders and compares.
struct Shot {
    /// The name the notices and the report use.
    name: &'static str,
    /// The scene, passed as `--scene=` after `--`; the project's main scene
    /// routes on it.
    scene: &'static str,
    /// The committed golden, relative to the workspace root.
    golden: &'static str,
    /// Where the fresh render goes, relative to Cargo's target directory.
    actual: &'static str,
    /// The red-channel variance under which the frame counts as blank.
    min_variance: u64,
}

/// Every shot, in the order it is rendered. All three go through the bridge, so
/// all three need the staged extension. The vista is the geometry alarm (T16);
/// the rows and the wizard's first page are the editor's pictures of the
/// verifier's diagnostics and of Easy's suggestion (T19; decisions-log items
/// 110 (4) and 116 (6)(h)).
const SHOTS: &[Shot] = &[
    Shot {
        name: "vista",
        scene: "res://scenes/vista_shot.tscn",
        golden: "tests/golden/vista/expected.vista.png",
        actual: "golden/vista/actual.vista.png",
        min_variance: png::MIN_VARIANCE,
    },
    Shot {
        name: "rows",
        scene: "res://scenes/rows_shot.tscn",
        golden: "tests/golden/vista/expected.rows.png",
        actual: "golden/vista/actual.rows.png",
        min_variance: png::MIN_VARIANCE,
    },
    Shot {
        name: "wizard",
        scene: "res://scenes/wizard_shot.tscn",
        golden: "tests/golden/vista/expected.wizard.png",
        actual: "golden/vista/actual.wizard.png",
        min_variance: png::MIN_VARIANCE,
    },
];

/// The render the screenshot step asks Godot for, and the xvfb screen it runs
/// on. 1280 × 720 is G1's geometry resolution. The two must agree: a windowed
/// run is clamped by the screen size — G1 asked for 1920 × 1080 on a
/// 1920 × 1080 screen and got 1920 × 1061 — and a differently sized image fails
/// the comparison for the wrong reason.
const VISTA_RESOLUTION: &str = "1280x720";
const VISTA_SCREEN: &str = "-screen 0 1280x720x24";

// --- the client extension (T12) --------------------------------------------
//
// `cargo xtask stage-client` builds the gdext cdylib, copies it into the Godot
// project's staging directory, byte-scans it for the entry symbol, and runs the
// import that a fresh checkout cannot do without. Every constant below is one
// half of a pair that has to agree with `godot/pharmakos.gdextension`; the step
// reads that file and says so when they do not, because a mismatch is otherwise
// a run-time failure inside Godot and a silent one on a fresh checkout, where
// the extension's classes instantiate as placeholders anyway.

/// The package holding the bridge, and its library target's file stem.
const CLIENT_PACKAGE: &str = "pharmakos-client-gdext";
const CLIENT_LIB_STEM: &str = "pharmakos_client_gdext";

/// Where the staged library goes, relative to [`GODOT_PROJECT_DIR`], and the
/// extension file that points at it.
const CLIENT_STAGE_DIR: &str = "bin";
const GDEXTENSION_FILE: &str = "pharmakos.gdextension";

/// gdext's entry point, as `pharmakos.gdextension` names it.
///
/// The byte-scan for this string is not belt and braces. Building a second
/// target into the same directory can replace the staged library with one that
/// has no gdext in it; Godot then reports "Can't resolve symbol gdext_rust_init,
/// error 127", which reads like an ABI failure and is not one (spike G1 §10.12,
/// "Two builds, two target directories").
const ENTRY_SYMBOL: &str = "gdext_rust_init";

/// The scene `--check` runs: the headless client check, which prints its report
/// and quits non-zero if the bridge caught a panic or the two upload paths
/// disagreed.
const CLIENT_CHECK_SCENE: &str = "res://scenes/client_check.tscn";

/// The profile `stage-client` and `screenshot` stage the client from.
///
/// Debug, and deliberately: the guard is real there and the inner loop fast.
/// `[profile.release]` sets `panic = "abort"`, under which `catch_unwind`
/// catches nothing, so the bridge's caught-panic counter would be dead. The
/// packaged library is built with `[profile.release-client]` instead, which
/// inherits `release` with `panic = "unwind"` (decisions-log item 102 (1),
/// named by item 117 (4)); `cargo xtask package` builds it, and its smoke
/// check asserts the counter at zero in the packaged build. CI's `client
/// extension` job keeps the debug library, as this does.
const CLIENT_PROFILE_DIR: &str = "debug";

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
    /// The step does not run here, for a reason that is not a missing input: a
    /// platform that cannot run it (`screenshot` off Linux, `stage-client` on
    /// macOS), a `--package` filter that leaves its crate out, or a tool that is
    /// not installed on a run without `--require-tools`. A missing input — a
    /// file, a crate, a golden — is a failure, never this (decisions-log item
    /// 116 (6)(g)); `only_the_named_skips_remain` pins the list.
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
        about: "cargo test on pharmakos-sim alone, with the research feature",
        run: step_test_research,
    },
    Step {
        name: "research-guard",
        about: "plan-core/verifier/operator/gateway/gamectl must not reach `research`",
        run: step_research_guard,
    },
    Step {
        name: "wall-guard",
        about: "those crates and sim never reach a walled crate; the client never the core",
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
    // Harness part 2. Both are required: a missing input fails, and only the
    // screenshot's platform statement (Linux only) skips.
    Step {
        name: "scenario",
        about: "validate scenarios/**, then play each with gamectl scenario run",
        run: step_scenario,
    },
    Step {
        name: "screenshot",
        about: "stage the client, render the vista, rows and wizard shots, compare each PNG",
        run: step_screenshot,
    },
    // The client extension. It needs Godot, which the deterministic crates do
    // not, so a missing Godot goes through `skip_or_fail`; on macOS it skips,
    // because there is no macOS library to stage.
    Step {
        name: "stage-client",
        about: "build the gdext cdylib, stage it into godot/bin and import the project",
        run: step_stage_client,
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
    /// `--require-tools` (or `PHARMAKOS_REQUIRE_TOOLS` set): fail instead of
    /// skipping when buf, cargo-deny, reuse, godot or xvfb-run is missing
    /// ([`skip_or_fail`]). CI sets the variable for the whole workflow after
    /// installing each tool its job needs, so a runner that lost a tool goes red
    /// rather than quietly green.
    require_tools: bool,
    /// `--locked`: pass `--locked` to cargo. On by default in CI, but only once
    /// a `Cargo.lock` is committed — `--locked` with no lock file is an error,
    /// and the lock file does not exist until the first dependency lands.
    locked: bool,
    /// `-p/--package`: restrict package-scoped steps (the pre-commit hook).
    packages: Vec<String>,
    /// `--check`: `stage-client` also runs the headless client check and takes
    /// its exit code as its own. The CI client leg passes it; `cargo xtask ci`
    /// does not, so a developer without a Godot binary is not stopped by it.
    client_check: bool,
    /// Workspace metadata, or the reason it could not be read.
    workspace: Result<Workspace, String>,
}

fn run_cli(args: &[String]) -> Result<bool, String> {
    let mut commands: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut packages: Vec<String> = Vec::new();
    let mut bless = false;
    let mut client_check = false;
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
            // Not a step, and never in `ci`: CI's docs-only fast path asks it
            // whether the suite runs at all. Every other argument makes it
            // answer `full`, so nothing typed can force `prose`.
            "ci-scope" => {
                let extra: Vec<&String> = args.iter().filter(|arg| *arg != "ci-scope").collect();
                return Ok(scope::run_from_env(&extra));
            }
            // Not a step either: the per-runner perf notices of decision 23.
            // It always answers success, so the job that runs it can never go
            // red (decisions-log item 116 (6)(b)).
            "perf-alarms" => {
                perf_alarms();
                return Ok(true);
            }
            "--quick" => quick = true,
            "--fix" => fix = true,
            "--bless" => bless = true,
            "--check" => client_check = true,
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
        println!("note: the steps that need it will fail (decisions-log item 116 (6)(g))");
    }
    let ctx = Ctx {
        root,
        cargo,
        bless,
        require_tools,
        locked,
        packages,
        client_check,
        workspace,
    };

    // `--fix` is a mode, not a step: it edits the tree and then stops, so that
    // nobody mistakes "xtask rewrote my code" for "the checks passed".
    if fix {
        return run_fix(&ctx).map(|()| true);
    }

    // Not a step: the unsigned zip, which CI's package jobs build. It needs the
    // parsed flags (`--locked`, `--bless`), so it is dispatched here, once the
    // context exists, and it runs alone.
    if commands.iter().any(|name| name == "package") {
        if commands.len() != 1 {
            return Err("`package` runs alone; it is not a step of `ci`".to_owned());
        }
        println!();
        println!("== package — the unsigned zip for this platform (T21)");
        let summary = package::run_package(&ctx)?;
        println!();
        println!("== package: ok");
        println!("      {summary}");
        return Ok(true);
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
    println!("    cargo xtask ci-scope       CI's docs-only fast path: prints `full` or `prose`");
    println!("    cargo xtask perf-alarms    per-runner perf notices, no threshold; never fails");
    println!(
        "    cargo xtask package        the unsigned zip for this platform; --bless its manifest"
    );
    println!();
    println!("FLAGS:");
    println!("    --quick            fmt, clippy and tests only — the inner loop");
    println!("    --fix              apply rustfmt and machine-applicable clippy fixes, then stop");
    println!("    --skip <step>      leave one step out of `ci`");
    println!("    -p, --package <n>  restrict package-scoped steps to these packages");
    println!("    --bless            rewrite golden files from the fresh outputs");
    println!("    --check            stage-client: also run the headless client check");
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
    let manifest = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
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
    let sim = ctx
        .workspace
        .as_ref()
        .ok()
        .and_then(|workspace| workspace.package_with_feature(SIM_PACKAGES, RESEARCH_FEATURE));
    match research_test_args(sim.as_deref(), &ctx.packages, ctx.locked)? {
        ResearchTests::Run { args, feature } => {
            run(ctx, &ctx.cargo, &args)?;
            Ok(Outcome::Done(format!(
                "tests pass with {feature}; no other crate's code reads the feature, \
                 and clippy's research pass compiles every non-walled crate against it"
            )))
        }
        ResearchTests::Filtered(reason) => Ok(Outcome::Skipped(reason)),
    }
}

/// What the `test-research` step does.
#[derive(Debug, PartialEq, Eq)]
enum ResearchTests {
    /// Run cargo with these arguments; `feature` is the `--features` value.
    Run { args: Vec<String>, feature: String },
    /// Skip, because `--package` leaves the sim out: a filter the caller chose.
    Filtered(String),
}

/// The `test-research` step's cargo arguments and its `--features` value, the
/// reason it skips, or — when no crate declares the feature, which the stage
/// has — the reason it fails. Pure, so the three `-p` cases are unit-tested.
///
/// `sim` is the package that declares the `research` feature. The step tests
/// that package alone, because it is the one crate whose code the feature
/// changes: every other crate's tests would compile and run exactly as they do
/// in `test`, against a sim they never ship with (decisions-log items 113 (9)
/// and 115 (5)).
///
/// * no `-p`: the sim's tests with the feature on;
/// * a `-p` list that names the sim: the same, and nothing else;
/// * a `-p` list that leaves the sim out: skipped, because nothing in it changes;
/// * no crate declaring the feature: a failure, because `fork` lives behind it
///   and the stage has it (decisions-log item 116 (6)(g)).
///
/// A `-p` value counts as naming the sim when [`package_spec_may_name`] says
/// it may: cargo also takes `name@version`, package-id URLs and globs there,
/// and a value this cannot rule out runs the step rather than skip it.
fn research_test_args(
    sim: Option<&str>,
    packages: &[String],
    locked: bool,
) -> Result<ResearchTests, String> {
    let Some(sim) = sim else {
        return Err(format!(
            "no crate declares a `{RESEARCH_FEATURE}` feature; `pharmakos-sim` defines it, \
             and `fork` lives behind it"
        ));
    };
    if !packages.is_empty()
        && !packages
            .iter()
            .any(|package| package_spec_may_name(package, sim))
    {
        return Ok(ResearchTests::Filtered(format!(
            "the {RESEARCH_FEATURE} feature changes only {sim}, which --package leaves out"
        )));
    }
    let feature = format!("{sim}/{RESEARCH_FEATURE}");
    let mut args: Vec<String> = vec![
        "test".to_owned(),
        "--package".to_owned(),
        sim.to_owned(),
        "--features".to_owned(),
        feature.clone(),
    ];
    if locked {
        args.push("--locked".to_owned());
    }
    Ok(ResearchTests::Run { args, feature })
}

/// Whether a `--package` value may select the package called `name`: the name
/// itself, `name@version`, or any value this does not parse: a glob (`*`,
/// `?`, `[`) or a package-id URL (`#`, `:`, `/`, `\`). Errs towards `true`,
/// because a wrong `true` runs a step that could have skipped and a wrong
/// `false` skips one that should have run.
fn package_spec_may_name(spec: &str, name: &str) -> bool {
    if spec.contains(['*', '?', '[', '#', ':', '/', '\\']) {
        return true;
    }
    spec.split_once('@').map_or(spec, |(before, _)| before) == name
}

/// `fork` lives behind the `research` feature, and release builds never enable
/// it. This proves the plan core, verifier, operator and gateway cannot reach
/// it: once with their default features (the shipped configuration), once with
/// every feature of their own turned on (so no feature of theirs can forward to
/// it later). `--all-features` here is a `cargo tree` query, not a build — the
/// AGENTS.md ban on `--all-features` is about compiling, where it would enable
/// `research` everywhere and hide this very check.
fn step_research_guard(ctx: &Ctx) -> Result<Outcome, String> {
    let workspace = metadata(ctx)?;

    let present = workspace.present(GUARDED_PACKAGES);
    if present.is_empty() {
        return Err(
            "none of plan-core, verifier, operator, gateway, gamectl exist, so there is nothing \
             to guard; the workspace lost its crates or the metadata is wrong"
                .to_owned(),
        );
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

/// The wall is a crate boundary, checked in both directions.
///
/// 1. Floats, `as` casts and hash maps are legal inside the walled crates, so
///    nothing deterministic may depend on them — otherwise the allowance leaks
///    into hashed state. "Nothing deterministic" is [`WALL_GUARDED_PACKAGES`]:
///    the guarded crates and the sim itself.
/// 2. The thin client never reaches the core: each walled crate named in
///    [`CLIENT_WALL`] never reaches the crates its line forbids (decisions-log
///    item 102 (3)).
///
/// Both relations read the same listing, `cargo tree --package <p>
/// --all-features --prefix none --format {p}`, and are judged by one pure
/// function, [`first_reached`], which the unit tests break both ways.
fn step_wall_guard(ctx: &Ctx) -> Result<Outcome, String> {
    let workspace = metadata(ctx)?;

    let walled = workspace.present(WALLED_PACKAGES);
    let guarded = workspace.present(WALL_GUARDED_PACKAGES);
    if walled.is_empty() || guarded.is_empty() {
        return Err(format!(
            "the walled crates ({}) or the guarded crates ({}) are missing from the workspace, \
             so the wall has nothing to separate; the metadata is wrong or a crate was removed",
            if walled.is_empty() { "none" } else { "present" },
            if guarded.is_empty() {
                "none"
            } else {
                "present"
            },
        ));
    }

    for package in &guarded {
        let tree = dependency_listing(ctx, package)?;
        if let Some(name) = first_reached(package, &tree, &walled) {
            return Err(format!(
                "`{package}` depends on the walled crate `{name}`; floats and unordered \
                 collections are legal there and must not reach the deterministic crates"
            ));
        }
    }

    let mut client_notes: Vec<String> = Vec::new();
    for (client, forbidden_bare) in CLIENT_WALL {
        let present = workspace.present(&[*client]);
        let Some(package) = present.first() else {
            return Err(format!(
                "`{client}` is named in CLIENT_WALL and is not in the workspace; remove its line \
                 in the same pull request that removes the crate"
            ));
        };
        let forbidden = workspace.present(forbidden_bare);
        let missing = missing_names(forbidden_bare, &forbidden);
        if !missing.is_empty() {
            return Err(format!(
                "CLIENT_WALL forbids `{client}` to reach {}, which are not in the workspace; \
                 remove them from its line in the same pull request that removes the crates, \
                 so the check never guards fewer crates than it names",
                missing.join(", ")
            ));
        }
        let tree = dependency_listing(ctx, package)?;
        if let Some(name) = first_reached(package, &tree, &forbidden) {
            return Err(format!(
                "`{package}` reaches `{name}`. The client side of the wall marshals and decides \
                 nothing (AGENTS.md section 3 rule 4; decisions-log item 102 (3)): a walled \
                 crate with an edge to the core has the rules, a verdict or the sim's stepping \
                 API within reach"
            ));
        }
        client_notes.push(format!(
            "{package} reaches none of {}",
            forbidden.join(", ")
        ));
    }

    Ok(Outcome::Done(format!(
        "{} do not depend on {}; {}",
        guarded.join(", "),
        walled.join(", "),
        client_notes.join("; ")
    )))
}

/// `cargo tree --package <p> --all-features --prefix none --format {p}`: every
/// package `package` reaches, one per line, itself first. `--all-features` so
/// that no feature of the package can bring an edge in later; this is a query,
/// not a build.
fn dependency_listing(ctx: &Ctx, package: &str) -> Result<String, String> {
    let mut args: Vec<String> = vec![
        "tree".to_owned(),
        "--package".to_owned(),
        package.to_owned(),
        "--all-features".to_owned(),
        "--prefix".to_owned(),
        "none".to_owned(),
        "--format".to_owned(),
        "{p}".to_owned(),
    ];
    if ctx.locked {
        args.push("--locked".to_owned());
    }
    capture(ctx, &ctx.cargo, &args)
}

/// The first name in a `cargo tree --prefix none --format {p}` listing that is
/// one of `forbidden`, skipping `package` itself. Pure: both of `wall-guard`'s
/// relations are this function over a listing, so both are unit-tested on
/// synthetic listings. A line is `<name> v<version>[ (<path>)][ (*)]`; the name
/// is its first word.
fn first_reached<'a>(package: &str, listing: &'a str, forbidden: &[String]) -> Option<&'a str> {
    listing
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| *name != package)
        .find(|name| {
            forbidden
                .iter()
                .any(|forbidden_name| forbidden_name == name)
        })
}

/// The bare names in `wanted` that no package in `found` answers to, with or
/// without the `pharmakos-` prefix, in `wanted`'s order. Pure: `wall-guard`
/// fails when the client relation would guard fewer crates than it names.
fn missing_names<'a>(wanted: &[&'a str], found: &[String]) -> Vec<&'a str> {
    wanted
        .iter()
        .copied()
        .filter(|bare| {
            !found.iter().any(|name| {
                name.as_str() == *bare || name.strip_prefix("pharmakos-") == Some(*bare)
            })
        })
        .collect()
}

/// The workspace metadata, or the failure that says why it is missing. Every
/// step that needs it fails without it (decisions-log item 116 (6)(g)).
fn metadata(ctx: &Ctx) -> Result<&Workspace, String> {
    ctx.workspace
        .as_ref()
        .map_err(|reason| format!("no workspace metadata: {reason}"))
}

fn step_deny(ctx: &Ctx) -> Result<Outcome, String> {
    if !ctx.root.join("deny.toml").is_file() {
        return Err(
            "no deny.toml at the workspace root; the licence, advisory and banned-crate policy \
             lives there (AGENTS.md section 3 rule 5)"
                .to_owned(),
        );
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
        return Err(
            "no buf.yaml / buf.work.yaml at the root or under proto/; the schema step has \
             nothing to lint"
                .to_owned(),
        );
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
    // store, so CI checks out with fetch-depth: 0 and fetches main. Without a
    // local main the comparison cannot run, and an `ok` that did not compare is
    // the failure this harness exists to prevent (decisions-log item
    // 116 (6)(g) and (p)).
    if !git_ref_exists(ctx, "refs/heads/main") {
        return Err(
            "buf lint is clean, but there is no local `main` for `buf breaking` to compare \
             against; fetch it (`git fetch origin main:main`) and run the step again"
                .to_owned(),
        );
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
        "buf lint clean; no breaking changes against main. The regenerate-and-compare half \
         of AGENTS.md section 9 item 8 runs as tests (crates/proto/tests/generated.rs, \
         crates/gateway/tests/methods.rs, crates/gamectl/tests/docs.rs) through `test` and \
         `golden`; generated JSON Schema is v1.1's"
            .to_owned(),
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
/// Three areas are not this step's to compare —
/// [`golden::SELF_COMPARED_AREAS`]. `determinism/` is compared by
/// [`step_determinism`], which runs the sim and validates the chain's format
/// line by line; `vista/` is compared with a tolerance by [`png`] from
/// [`step_screenshot`], which runs after this step and only on Linux; and
/// `package/` by `cargo xtask package` ([`package`]), which CI's package jobs
/// run and `ci` does not. Each would otherwise fail this step's "a missing
/// fresh output is a failure" rule on a clean checkout, on every operating
/// system.
///
/// Every area the producing tasks filled is compared: `tests/golden` missing,
/// or holding no case this step compares, is a failure rather than a skip
/// (decisions-log item 116 (6)(g)) — the stage has goldens, so a tree without
/// them is a tree that lost them. Producers join by writing their fresh output
/// where this step looks and committing the `expected.*` beside the area's
/// README.
fn step_golden(ctx: &Ctx) -> Result<Outcome, String> {
    let golden_root = ctx.root.join("tests").join("golden");
    let target_dir = match &ctx.workspace {
        Ok(workspace) => workspace.target_dir.clone(),
        Err(_) => ctx.root.join("target"),
    };
    if !golden_root.is_dir() {
        return Err(
            "tests/golden does not exist, so no golden is compared; the stage's goldens live \
             there (tests/golden/README.md)"
                .to_owned(),
        );
    }
    // Before the check below, not after it: a tree whose only goldens belong
    // to the steps that own them still has bytes, and rule 3 is about the tree.
    golden::check_endings(&golden_root)?;

    if !golden::has_goldens(&golden_root)? {
        return Err(format!(
            "no case this step compares has an expected.* file under tests/golden (only {} \
             hold goldens, and they are compared by the steps that own them); the stage's \
             goldens are missing",
            golden::SELF_COMPARED_AREAS.join(" and ")
        ));
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
/// writes, and compares it with the committed chain. CI uploads the chain per
/// operating system and the `cross-OS determinism guard` job compares the
/// three. A missing binary, a missing committed chain or missing metadata is a
/// failure (decisions-log item 116 (6)(g)); `--bless` writes the chain.
fn step_determinism(ctx: &Ctx) -> Result<Outcome, String> {
    let workspace = metadata(ctx)?;
    let Some(owner) = workspace.package_with_bin(DETERMINISM_BIN) else {
        return Err(format!(
            "no `{DETERMINISM_BIN}` binary in the workspace; `pharmakos-sim` builds it \
             (crates/sim/src/bin/determinism.rs)"
        ));
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

    let golden_path = ctx.root.join(HASH_GOLDEN);
    let golden = if golden_path.is_file() {
        Some(
            fs::read(&golden_path)
                .map_err(|error| format!("reading {}: {error}", golden_path.display()))?,
        )
    } else {
        None
    };
    let fresh = fs::read(&hash_path)
        .map_err(|error| format!("reading {}: {error}", hash_path.display()))?;
    match chain_verdict(golden.as_deref(), &fresh, ctx.bless) {
        ChainVerdict::Matches => Ok(Outcome::Done(format!(
            "{ticks} ticks; hash chain matches {HASH_GOLDEN}"
        ))),
        ChainVerdict::Bless => {
            fs::write(&golden_path, &fresh)
                .map_err(|error| format!("writing {}: {error}", golden_path.display()))?;
            Ok(Outcome::Done(format!(
                "{ticks} ticks; hash chain written to {HASH_GOLDEN} — say in the PR why the \
                 hashes moved"
            )))
        }
        ChainVerdict::Missing => Err(format!(
            "{ticks} per-tick hashes written to {HASH_FILE}, and there is no committed chain at \
             {HASH_GOLDEN} to compare them with. A chain that compares nothing checks nothing; \
             `cargo xtask determinism --bless` writes it, and the pull request says why"
        )),
        ChainVerdict::Moved => Err(format!(
            "the hash chain moved: {} differs from {}\n{}\n      A moved hash chain is a \
             behaviour change. Explain it in the pull request; never re-bless it to get a \
             red build to green.",
            HASH_FILE,
            HASH_GOLDEN,
            first_difference(golden.as_deref().unwrap_or_default(), &fresh)
        )),
    }
}

/// What the `determinism` step makes of the fresh chain.
#[derive(Debug, PartialEq, Eq)]
enum ChainVerdict {
    /// The committed chain equals the fresh one.
    Matches,
    /// `--bless`: write the fresh chain as the committed one.
    Bless,
    /// No committed chain, and no `--bless`: a failure.
    Missing,
    /// The committed chain differs, and no `--bless`: a failure.
    Moved,
}

/// The `determinism` step's decision, pure so that "a missing committed chain
/// fails" is unit-tested rather than asserted in prose.
fn chain_verdict(golden: Option<&[u8]>, fresh: &[u8], bless: bool) -> ChainVerdict {
    match golden {
        Some(golden) if golden == fresh => ChainVerdict::Matches,
        _ if bless => ChainVerdict::Bless,
        Some(_) => ChainVerdict::Moved,
        None => ChainVerdict::Missing,
    }
}

fn step_reuse(ctx: &Ctx) -> Result<Outcome, String> {
    if !ctx.root.join("REUSE.toml").is_file() {
        return Err(
            "no REUSE.toml at the workspace root; it is the licence manifest `reuse lint` reads"
                .to_owned(),
        );
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
/// (AGENTS.md section 9 item 10, section 10 item 4). Every committed scenario
/// file is first validated against the frozen format — unknown keys rejected,
/// seeds and durations checked, playbook paths resolved, the assertion
/// vocabulary enforced — and then played by `gamectl scenario run`, whose exit
/// status is the verdict.
///
/// Required (decisions-log items 75 and 116 (6)(g)): no scenario files, no
/// metadata, no `gamectl` binary or no `scenario` subcommand is a **failure**
/// with the reason, never a skip, because section 10 item 4 is not met by a
/// step that played nothing.
fn step_scenario(ctx: &Ctx) -> Result<Outcome, String> {
    let files = scenario::collect(&ctx.root)?;
    if files.is_empty() {
        return Err(
            "no scenario files (scenarios/**/*.scenario.jsonc matched nothing), so no headless \
             match was played; AGENTS.md section 10 item 4 needs the stage's scenarios, and \
             scenarios/README.md documents the format"
                .to_owned(),
        );
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

    let workspace = ctx.workspace.as_ref().map_err(|reason| {
        format!("{checked}; no workspace metadata to find the runner: {reason}")
    })?;
    let Some(owner) = workspace.package_with_bin(SCENARIO_BIN) else {
        return Err(format!(
            "{checked}; there is no `{SCENARIO_BIN}` binary in the workspace, so none of them \
             was played (`{SCENARIO_BIN} {SCENARIO_SUBCOMMAND} run` is the runner, T15)"
        ));
    };

    let probe = cargo_run_args(ctx, &owner, &[SCENARIO_SUBCOMMAND, "--help"]);
    if !command_succeeds(&ctx.root, &ctx.cargo, &probe) {
        return Err(format!(
            "{checked}; `{SCENARIO_BIN} {SCENARIO_SUBCOMMAND} --help` did not succeed, so the \
             runner is missing or does not build, and none of them was played"
        ));
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

/// Harness part 2, half two: the screenshots.
///
/// The obvious shape of this step — run Godot, upload the PNG — asserts
/// nothing; the comparison in [`png`] is the assertion, and its report goes out
/// through [`annotate`] because GitHub hides logs and step summaries from
/// logged-out viewers (G1 section 10.12).
///
/// In this order, and nothing builds or stages before the platform check, so
/// the Windows and macOS legs and a local `--local-verify` run keep their plain
/// skip:
///
/// 1. the Godot project exists (a failure if not);
/// 2. the platform is Linux — the goldens are rendered under **xvfb +
///    lavapipe**, and a second rasteriser's output would make the alarm
///    permanently red (skeleton-plan section 7 decision 22, logged as
///    decisions-log item 116 (6)(a); G1 measured 1.14 % of pixels differing
///    between a Quadro and lavapipe on identical geometry). The one skip;
/// 3. `godot` answers `--version` and `xvfb-run` is on `PATH` — found by path,
///    because `xvfb-run` has no `--version` and exits non-zero on it
///    (decisions-log item 110 (4)). Through `skip_or_fail`;
/// 4. the staging pre-step: the cdylib is built and staged exactly as
///    `stage-client` stages it ([`stage_library`]), because every shot draws
///    through the bridge;
/// 5. `godot --headless --path godot --import` ([`import_project`]). A
///    non-editor Godot run loads GDExtensions only from
///    `res://.godot/extension_list.cfg`, which the editor writes and git
///    ignores, so on a fresh checkout the extension's classes are placeholders
///    until the import has run. G1 lost four runs to this;
/// 6. every shot in [`SHOTS`] is rendered windowed under xvfb and its actual
///    written, and a `::notice title=<shot> render::` line gives its size and
///    red-channel variance — before any floor or golden check, so a first run
///    carries the figures a variance floor is set from;
/// 7. per shot: the variance floor, then a missing golden **fails** (the
///    render-only path: the actual is written and uploaded, to be looked at
///    and committed, decisions-log item 105 (5)) and a present one is compared
///    with the tolerance. Every shot is judged before the step answers.
///
/// `--headless` is used **only** for the import. It selects the dummy
/// rendering driver, under which `frame_post_draw` never fires and a
/// screenshot coroutine parks for ever — silently, producing no PNG and no
/// error.
fn step_screenshot(ctx: &Ctx) -> Result<Outcome, String> {
    let project = ctx.root.join(GODOT_PROJECT_DIR);
    if !project.join("project.godot").is_file() {
        return Err(format!(
            "no {GODOT_PROJECT_DIR}/project.godot, so there is no project to render; the \
             screenshots are required (decisions-log item 116 (6)(g))"
        ));
    }
    if !cfg!(target_os = "linux") {
        return Ok(Outcome::Skipped(
            "the PNG goldens are rendered under xvfb + lavapipe on Linux only (skeleton-plan \
             section 7 decision 22, logged as decisions-log item 116 (6)(a)); comparing a \
             second platform's rasteriser against them would be permanently red and would say \
             nothing about the geometry"
                .to_owned(),
        ));
    }
    let godot = godot_program();
    if !tool_available(&godot) {
        return skip_or_fail(
            ctx,
            &format!(
                "`{godot}` is not installed (the CI job installs the pinned 4.7.2 build; \
                 $PHARMAKOS_GODOT overrides the name)"
            ),
        );
    }
    let Some(xvfb_run) = find_on_path("xvfb-run") else {
        return skip_or_fail(
            ctx,
            "xvfb-run is not on PATH (`apt-get install xvfb`); `godot --headless` is not a \
             substitute — it selects the dummy driver and cannot take a screenshot",
        );
    };
    let xvfb_run = xvfb_run.to_string_lossy().into_owned();

    // The staging pre-step: every shot draws through the bridge.
    let workspace = metadata(ctx)?;
    let (library, staged_bytes) = stage_library(ctx, workspace, &project)?;
    let import_note = import_project(ctx, &godot, &project)?;
    println!("   {library} staged ({staged_bytes} bytes); {import_note}");

    // Render every shot first, so each actual is on disk for the upload even
    // when an earlier shot fails its check.
    let mut rendered: Vec<(&Shot, Result<png::Image, String>)> = Vec::new();
    for shot in SHOTS {
        let actual_path = workspace.target_dir.join(shot.actual);
        let image = render_shot(ctx, &godot, &xvfb_run, shot, &actual_path);
        match &image {
            Ok(image) => {
                let variance = image.red_variance()?;
                println!(
                    "::notice title={} render::{}x{}, red-channel variance {variance} (floor {})",
                    shot.name, image.width, image.height, shot.min_variance
                );
            }
            Err(error) => println!("::error title={} render::{error}", shot.name),
        }
        rendered.push((shot, image));
    }

    let mut failures: Vec<String> = Vec::new();
    let mut passed: Vec<String> = Vec::new();
    for (shot, image) in rendered {
        match judge_shot(ctx, shot, image) {
            Ok(note) => passed.push(note),
            Err(error) => failures.push(format!("{}: {error}", shot.name)),
        }
    }
    if failures.is_empty() {
        Ok(Outcome::Done(passed.join("; ")))
    } else {
        Err(failures.join("\n      "))
    }
}

/// Renders one shot to `actual_path` and decodes it. A stale PNG from an
/// earlier run is removed first, so it can never pass for this one.
fn render_shot(
    ctx: &Ctx,
    godot: &str,
    xvfb_run: &str,
    shot: &Shot,
    actual_path: &Path,
) -> Result<png::Image, String> {
    if let Some(parent) = actual_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    }
    if actual_path.exists() {
        fs::remove_file(actual_path)
            .map_err(|error| format!("removing {}: {error}", actual_path.display()))?;
    }
    run(
        ctx,
        xvfb_run,
        &[
            "-a".to_owned(),
            "-s".to_owned(),
            VISTA_SCREEN.to_owned(),
            godot.to_owned(),
            "--path".to_owned(),
            GODOT_PROJECT_DIR.to_owned(),
            "--resolution".to_owned(),
            VISTA_RESOLUTION.to_owned(),
            "--".to_owned(),
            format!("--scene={}", shot.scene),
            format!("--shot={}", actual_path.to_string_lossy()),
        ],
    )?;
    let bytes = fs::read(actual_path).map_err(|error| {
        format!(
            "{} was not produced ({error}). Godot rendered nothing — check that the run was \
             windowed under xvfb rather than `--headless`, which selects the dummy driver and \
             parks a screenshot coroutine for ever",
            actual_path.display()
        )
    })?;
    png::decode(&bytes).map_err(|error| format!("{}: {error}", actual_path.display()))
}

/// One shot's verdict: its variance floor, then its golden — missing is a
/// failure after the render, present is compared with the tolerance.
fn judge_shot(ctx: &Ctx, shot: &Shot, image: Result<png::Image, String>) -> Result<String, String> {
    let image = image?;
    let variance = image.red_variance()?;
    if variance < shot.min_variance {
        return Err(format!(
            "the render is blank or near-uniform (red-channel variance {variance}, floor {}); \
             nothing was drawn",
            shot.min_variance
        ));
    }
    let golden_path = ctx.root.join(shot.golden);
    if !golden_path.is_file() {
        return Err(format!(
            "rendered ({}x{}, variance {variance}), and there is no committed golden at {} to \
             compare it with. The render is the job's `vista-screenshot` artefact: look at it, \
             commit it there and say in tests/golden/vista/README.md what it shows and which run \
             rendered it (decisions-log item 105 (5))",
            image.width, image.height, shot.golden
        ));
    }
    let golden_bytes = fs::read(&golden_path)
        .map_err(|error| format!("reading {}: {error}", golden_path.display()))?;
    let golden_image =
        png::decode(&golden_bytes).map_err(|error| format!("{}: {error}", shot.golden))?;
    let diff = png::compare(&image, &golden_image)?;
    let report = format!(
        "{} ({}x{}, red-channel variance {variance})\nvs {}\n{}",
        shot.actual,
        image.width,
        image.height,
        shot.golden,
        diff.describe()
    );
    annotate::notice(&format!("{}-screenshot", shot.name), &report);
    diff.check(png::MAX_MEAN_THOUSANDTHS, png::MAX_HARD_PPM)?;
    Ok(format!(
        "{} matches {} ({} of {} pixels differ at all)",
        shot.name, shot.golden, diff.differing, diff.pixels
    ))
}

/// Builds the gdext cdylib, stages it where Godot can load it, and runs the
/// import a fresh checkout cannot do without.
///
/// Four things happen, in this order, and the third is the one that saves time
/// later:
///
/// 1. `cargo build -p pharmakos-client-gdext`;
/// 2. the library is copied from **Cargo's own target directory**, read back out
///    of `cargo metadata`, into `godot/bin/`. Never an assumed `target/`: a
///    user-level `build.target-dir` or `CARGO_TARGET_DIR` moves it, a `res://`
///    path cannot escape the project folder, and staging from the wrong place
///    is how the two-target-directories trap starts;
/// 3. the staged file is **byte-scanned for `gdext_rust_init`**, and
///    `pharmakos.gdextension` is read back to check it names this exact file.
///    A library without the symbol, or an extension file pointing at a name
///    nothing writes, both surface inside Godot as "Can't resolve symbol
///    gdext_rust_init, error 127", which reads like an ABI failure and is not
///    one (spike G1 §10.12);
/// 4. `godot --headless --path godot --import`. **Not optional**: a non-editor
///    Godot run loads GDExtensions only from `res://.godot/extension_list.cfg`,
///    which the editor writes when it scans the project and which is
///    git-ignored, so on a fresh checkout — every CI runner — the extension's
///    classes instantiate as placeholders and the first call on them fails. G1
///    lost four runs to this.
///
/// With `--check`, the headless client check runs afterwards and its exit code
/// is the step's: that is where T12's "assert the caught-panic count is 0"
/// lives, and where paths A and B are compared inside the engine rather than
/// only under `cargo test`.
///
/// A missing project, missing metadata or a workspace without the client crate
/// is a failure (decisions-log item 116 (6)(g)); macOS is the one skip.
fn step_stage_client(ctx: &Ctx) -> Result<Outcome, String> {
    let project = ctx.root.join(GODOT_PROJECT_DIR);
    if !project.join("project.godot").is_file() {
        return Err(format!(
            "no {GODOT_PROJECT_DIR}/project.godot, so there is no project to stage the client \
             into"
        ));
    }
    // macOS is not a skip for want of a tool; it is a skip because there is
    // nothing to stage. `pharmakos.gdextension` declares windows.x86_64 and
    // linux.x86_64 only (decisions-log item 73) — the two platforms the client
    // ships on — so a macOS library would have no entry to be loaded from.
    if cfg!(target_os = "macos") {
        return Ok(Outcome::Skipped(format!(
            "{GODOT_PROJECT_DIR}/{GDEXTENSION_FILE} declares windows.x86_64 and linux.x86_64 \
             only (decisions-log item 73), so there is no macOS library for Godot to load; \
             the client ships on Windows and Linux (spec section 15)"
        )));
    }

    let workspace = metadata(ctx)?;
    let (library, staged_bytes) = stage_library(ctx, workspace, &project)?;

    let godot = godot_program();
    if !tool_available(&godot) {
        return skip_or_fail(
            ctx,
            &format!(
                "the library is staged in {}, but `{godot}` is not installed, so the import that \
                 a fresh checkout needs could not run (the `client` job in \
                 .github/workflows/ci.yml installs the pinned 4.7.2 build; $PHARMAKOS_GODOT \
                 overrides the name)",
                project.join(CLIENT_STAGE_DIR).display()
            ),
        );
    }

    // The pre-step every fresh checkout needs; see this function's doc comment.
    let import_note = import_project(ctx, &godot, &project)?;

    if !ctx.client_check {
        return Ok(Outcome::Done(format!(
            "{library} staged ({staged_bytes} bytes, carries `{ENTRY_SYMBOL}`); {import_note}; \
             `--check` also runs the headless client check"
        )));
    }

    run(
        ctx,
        &godot,
        &[
            "--headless".to_owned(),
            "--path".to_owned(),
            GODOT_PROJECT_DIR.to_owned(),
            CLIENT_CHECK_SCENE.to_owned(),
        ],
    )
    .map_err(|error| {
        format!(
            "{error}\n      The client check failed inside Godot. Its own report is above: a \
             non-zero caught-panic count means gdext caught a panic at a `#[func]` boundary and \
             turned it into a healthy-looking result, which is the failure the counter exists \
             to make visible (spike G1 section 10.12)."
        )
    })?;

    Ok(Outcome::Done(format!(
        "{library} staged ({staged_bytes} bytes, carries `{ENTRY_SYMBOL}`); {import_note}; the \
         headless client check passed with no caught panics"
    )))
}

/// Steps 1 to 3 of [`step_stage_client`]: build the cdylib, check the
/// extension file names it, copy it into `godot/bin` and byte-scan the staged
/// copy for the entry symbol. Returns the library's file name and its size.
/// `screenshot` runs the same function as its staging pre-step, so the two
/// can never stage differently.
fn stage_library(
    ctx: &Ctx,
    workspace: &Workspace,
    project: &Path,
) -> Result<(String, usize), String> {
    if !workspace
        .packages
        .iter()
        .any(|package| package.name == CLIENT_PACKAGE)
    {
        return Err(format!(
            "`{CLIENT_PACKAGE}` is not a member of this workspace, so there is no client to \
             stage"
        ));
    }

    let mut args: Vec<String> = vec![
        "build".to_owned(),
        "--package".to_owned(),
        CLIENT_PACKAGE.to_owned(),
        "--lib".to_owned(),
    ];
    if ctx.locked {
        args.push("--locked".to_owned());
    }
    run(ctx, &ctx.cargo, &args)?;

    let library = client_library_name();
    let built = workspace.target_dir.join(CLIENT_PROFILE_DIR).join(&library);
    if !built.is_file() {
        return Err(format!(
            "{} was not produced. Cargo's target directory for this workspace is {}, which is \
             what `cargo metadata` reports and what this step stages from — if the library is \
             somewhere else, a second target directory is in play (spike G1 section 10.12)",
            built.display(),
            workspace.target_dir.display()
        ));
    }

    // The extension file is the contract between this step and Godot; read it
    // back rather than trusting that the two have stayed in step.
    let extension_path = project.join(GDEXTENSION_FILE);
    let extension = fs::read_to_string(&extension_path)
        .map_err(|error| format!("reading {}: {error}", extension_path.display()))?;
    let reference = format!("res://{CLIENT_STAGE_DIR}/{library}");
    if !extension.contains(&reference) {
        return Err(format!(
            "{} does not name `{reference}`, so Godot would look for a library this step does \
             not write. Keep the [libraries] paths and xtask's CLIENT_LIB_STEM in step.",
            extension_path.display()
        ));
    }
    if !extension.contains(ENTRY_SYMBOL) {
        return Err(format!(
            "{} does not set `entry_symbol = \"{ENTRY_SYMBOL}\"`",
            extension_path.display()
        ));
    }

    let stage_dir = project.join(CLIENT_STAGE_DIR);
    fs::create_dir_all(&stage_dir)
        .map_err(|error| format!("creating {}: {error}", stage_dir.display()))?;
    let staged = stage_dir.join(&library);
    fs::copy(&built, &staged).map_err(|error| {
        format!(
            "copying {} to {}: {error}",
            built.display(),
            staged.display()
        )
    })?;

    let bytes =
        fs::read(&staged).map_err(|error| format!("reading {}: {error}", staged.display()))?;
    if !contains_bytes(&bytes, ENTRY_SYMBOL.as_bytes()) {
        return Err(format!(
            "{} does not contain the symbol `{ENTRY_SYMBOL}`. Godot reports this as \"Can't \
             resolve symbol {ENTRY_SYMBOL}, error 127\", which reads like an ABI problem and is \
             not one: something built a different target over the staged library (spike G1 \
             section 10.12, \"Two builds, two target directories\").",
            staged.display()
        ));
    }
    Ok((library, bytes.len()))
}

/// `godot --headless --path <project> --import`, and the check that it produced
/// what it is run for. The project path is passed as given, absolute, so the
/// same function imports `godot/` for `stage-client` and `screenshot` and the
/// staged copy for `package` (Godot resolves a relative `--path` against the
/// working directory).
///
/// **Godot 4.7.2 segfaults at the end of a COLD import when a GDExtension
/// registering a class is present.** Measured on Windows against gdext 0.5.5:
/// the first import of a project with no `.godot/` exits 139 / 0xC0000005 after
/// the scan has finished and the editor layout has loaded; a second import of
/// the same project exits 0, and a scene run after the crashed import loads the
/// extension and passes. Bisected: an extension with no registered class does
/// not crash, an extension with a single empty `#[class(base = Node3D)]` does,
/// so it is a teardown fault in the engine or in gdext and not in this crate's
/// code. The artefact the import exists to produce —
/// `.godot/extension_list.cfg` — is written by the crashing run.
///
/// So the exit code alone is not the question worth asking. This function asks
/// the one that is: **is the extension list there, and does it name our
/// extension?** That is precisely what a fresh checkout lacks and what G1 lost
/// four runs to. A failed import is retried once, because the second run is the
/// one that exits cleanly, and the run is only accepted when the file is
/// present either way. If it is not, the failure is reported with the reason.
fn import_project(ctx: &Ctx, godot: &str, project: &Path) -> Result<String, String> {
    let import_args: Vec<String> = vec![
        "--headless".to_owned(),
        "--path".to_owned(),
        project.to_string_lossy().replace('\\', "/"),
        "--import".to_owned(),
    ];
    let first = run(ctx, godot, &import_args);
    let mut note = "project imported".to_owned();
    if let Err(error) = first {
        println!(
            "   note: the import exited badly ({error}); this is the known Godot 4.7.2 cold-import \
             teardown fault with a GDExtension loaded — retrying, and the extension list is \
             checked either way"
        );
        run(ctx, godot, &import_args)?;
        "project imported (the cold import crashed at teardown and the retry exited cleanly)"
            .clone_into(&mut note);
    }

    let extension_list = project.join(".godot").join("extension_list.cfg");
    let listed = fs::read_to_string(&extension_list).map_err(|error| {
        format!(
            "{} was not produced ({error}). This file is the whole reason the import runs: a \
             non-editor Godot loads GDExtensions only from it, so without it the extension's \
             classes instantiate as placeholders and the first call on one fails (spike G1 \
             section 10.12)",
            extension_list.display()
        )
    })?;
    if !listed.contains(GDEXTENSION_FILE) {
        return Err(format!(
            "{} does not name {GDEXTENSION_FILE}; it holds:\n      {}",
            extension_list.display(),
            listed.trim()
        ));
    }
    Ok(note)
}

/// The cdylib's file name on this platform.
///
/// macOS is listed for completeness; [`step_stage_client`] returns before it
/// ever gets here, because `pharmakos.gdextension` has no macOS entry.
fn client_library_name() -> String {
    if cfg!(target_os = "windows") {
        format!("{CLIENT_LIB_STEM}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{CLIENT_LIB_STEM}.dylib")
    } else {
        format!("lib{CLIENT_LIB_STEM}.so")
    }
}

/// Whether `haystack` contains `needle` anywhere, byte for byte.
///
/// A library is not text, so this is a plain window scan rather than anything
/// that would have to decide an encoding first.
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    find_bytes(haystack, needle).is_some()
}

/// Where `needle` first occurs in `haystack`, byte for byte: the scan behind
/// [`contains_bytes`], for the scans that report an offset.
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
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
// The perf alarms (not a step)
// ---------------------------------------------------------------------------

/// The package and test target of the mesher's p99 alarm.
const MESHER_PACKAGE: &str = "pharmakos-mesher";
const MESHER_PERF_TEST: &str = "perf_alarm";

/// `cargo xtask perf-alarms`: skeleton-plan section 7 decision 23, logged as
/// decisions-log item 116 (6)(b). Per runner, never compared, no threshold,
/// and it never fails — the job that runs it is not required, and
/// `scripts/merge-train.sh` stops on any check that is not pass or skipping,
/// so a failed measurement is a `::warning::` and the command still answers
/// success.
///
/// * **mesher p99**: `crates/mesher/tests/perf_alarm.rs`, run exactly as its
///   header says (`--release` is not optional; `--ignored`; `--nocapture`, so
///   its own `::notice::` lines reach the annotations).
/// * **allocations per tick**: a notice naming the test that asserts it. The
///   count is zero by construction — `crates/sim/tests/allocations.rs` fails
///   otherwise, in every leg's `test` step — so it is published, not
///   measured again and never compared.
///
/// The third alarm the plan named, the tick-minus-pathing mean, is not built:
/// a millisecond figure for the sim comes only from a walled crate driving it
/// from outside (AGENTS.md section 4.5), none exists, and it moves to S2's
/// G3′-real gate. The budgets come with S1's P1 and S2's G3′-real gates.
/// xtask itself reads no clock; the mesher's test does, behind the wall.
fn perf_alarms() {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let root = match workspace_root() {
        Ok(root) => root,
        Err(error) => {
            println!("::warning title=perf alarms::no measurement: {error}");
            return;
        }
    };
    let os = env::consts::OS;
    // Frozen install in CI, as every other cargo call xtask makes there: a
    // lockfile mismatch is then the usual warning below, and the job stays green.
    let locked: &[&str] = if env::var_os("CI").is_some() {
        &["--locked"]
    } else {
        &[]
    };
    let args: Vec<String> = ["test", "--release"]
        .iter()
        .chain(locked)
        .chain(&[
            "--package",
            MESHER_PACKAGE,
            "--test",
            MESHER_PERF_TEST,
            "--",
            "--ignored",
            "--nocapture",
        ])
        .map(|arg| (*arg).to_owned())
        .collect();
    println!("   $ {}", render_command(&cargo, &args));
    let status = Command::new(&cargo).args(&args).current_dir(&root).status();
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => println!(
            "::warning title=mesher p99::not measured on {os}: `{}` failed ({}); an alarm, not a \
             gate, so this job stays green",
            render_command(&cargo, &args),
            describe_exit(status)
        ),
        Err(error) => println!(
            "::warning title=mesher p99::not measured on {os}: could not launch `{cargo}`: \
             {error}"
        ),
    }
    println!("{}", allocations_notice(os));
}

/// The allocations-per-tick notice, word for word. Pure, so its text is pinned.
fn allocations_notice(os: &str) -> String {
    format!(
        "::notice title=allocations per tick::0 on {os}: asserted by \
         crates/sim/tests/allocations.rs::a_tick_allocates_nothing in this commit's test step; \
         zero by construction, never compared"
    )
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

/// The Godot binary to run: `$PHARMAKOS_GODOT` when it names one, plain `godot`
/// otherwise.
///
/// Every `godot` invocation in this file goes through here, and the reason is a
/// failure that already cost this lane a CI run. `$GITHUB_PATH` entries written
/// from Git Bash on the Windows runner are MSYS POSIX paths
/// (`/c/Users/runneradmin/godot`); bash resolves such an entry and
/// `Command::new` does not, so a workflow step could print `godot: 4.7.2` and
/// the very next step could report the tool missing. The workflow now writes a
/// native path, and this override means the steps do not depend on it having
/// done so: CI points the variable straight at the binary it installed.
///
/// An empty or blank value is treated as unset, so `PHARMAKOS_GODOT=` in an
/// environment file does not turn into an attempt to execute the empty string.
fn godot_program() -> String {
    match env::var("PHARMAKOS_GODOT") {
        Ok(path) if !path.trim().is_empty() => path,
        _ => GODOT_DEFAULT_PROGRAM.to_owned(),
    }
}

/// The name Godot is looked for under when `$PHARMAKOS_GODOT` is not set: the
/// name a developer has locally.
const GODOT_DEFAULT_PROGRAM: &str = "godot";

/// Where `program` is on `PATH`, found by looking rather than by running it.
///
/// For a tool with no `--version`: `xvfb-run` prints its usage and exits
/// non-zero on it, so [`tool_available`] reported it missing on a runner where
/// it was installed and CI carried a shim until T20 (decisions-log item
/// 110 (4)). Only `screenshot` uses this, on Linux.
fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let dirs: Vec<PathBuf> = env::split_paths(&path).collect();
    find_in_dirs(program, &dirs)
}

/// The first `dir/program` among `dirs` that is a file. Pure over the
/// filesystem it is given, so it is unit-tested on a temporary directory.
fn find_in_dirs(program: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// True when the tool answers `--version`. On Windows this finds `tool.exe` but
/// not a `tool.cmd` shim, which is why CI installs buf as a real binary. A tool
/// with no `--version` (`xvfb-run`) is found with [`find_on_path`] instead.
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

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn research_tests_run_on_the_sim_alone() {
        let expected = strings(&[
            "test",
            "--package",
            "pharmakos-sim",
            "--features",
            "pharmakos-sim/research",
        ]);
        let run = |args: Vec<String>| ResearchTests::Run {
            args,
            feature: "pharmakos-sim/research".to_owned(),
        };
        // No -p: the sim alone, never --workspace.
        assert_eq!(
            research_test_args(Some("pharmakos-sim"), &[], false),
            Ok(run(expected.clone()))
        );

        // --locked is passed on as before.
        let mut expected_locked = expected.clone();
        expected_locked.push("--locked".to_owned());
        assert_eq!(
            research_test_args(Some("pharmakos-sim"), &[], true),
            Ok(run(expected_locked))
        );

        // -p naming the sim, alone or among others: the same command.
        for packages in [
            strings(&["pharmakos-sim"]),
            strings(&["pharmakos-gateway", "pharmakos-sim"]),
        ] {
            assert_eq!(
                research_test_args(Some("pharmakos-sim"), &packages, false),
                Ok(run(expected.clone())),
                "{packages:?}"
            );
        }

        // A -p value cargo resolves to the sim, or one this cannot rule out:
        // the same command, never a skip with the wrong reason.
        for spec in [
            "pharmakos-sim@0.1.0",
            "pharmakos-*",
            "pharmakos-si?",
            "path+file:///repo/crates/sim#0.1.0",
        ] {
            assert_eq!(
                research_test_args(Some("pharmakos-sim"), &strings(&[spec]), false),
                Ok(run(expected.clone())),
                "{spec}"
            );
        }

        // -p leaving the sim out: skipped, with the reason (a filter the
        // caller chose, the one skip this step keeps).
        assert_eq!(
            research_test_args(
                Some("pharmakos-sim"),
                &strings(&["pharmakos-gateway"]),
                false
            ),
            Ok(ResearchTests::Filtered(
                "the research feature changes only pharmakos-sim, which --package leaves out"
                    .to_owned()
            ))
        );

        // No crate declares the feature: a failure now, not a skip
        // (decisions-log item 116 (6)(g)).
        let missing = research_test_args(None, &[], false).expect_err("fails");
        assert!(
            missing.contains("no crate declares a `research` feature"),
            "{missing}"
        );
    }

    /// A synthetic `cargo tree --prefix none --format {p}` listing.
    const LISTING: &str = "\
pharmakos-client-gdext v0.1.0 (/repo/crates/client-gdext)
godot v0.5.5
pharmakos-mesher v0.1.0 (/repo/crates/mesher)
pharmakos-proto v0.1.0 (/repo/crates/proto)
prost v0.14.1
pharmakos-mesher v0.1.0 (/repo/crates/mesher) (*)
";

    #[test]
    fn wall_guard_relation_one_catches_a_deterministic_crate_reaching_the_wall() {
        let walled = strings(&["pharmakos-client-gdext", "pharmakos-mesher"]);
        // The gateway's listing, clean.
        let clean =
            "pharmakos-gateway v0.1.0 (/r)\npharmakos-sim v0.1.0 (/r)\nxxhash-rust v0.8.15\n";
        assert_eq!(first_reached("pharmakos-gateway", clean, &walled), None);
        // The same with the mesher pulled in through a third crate.
        let broken = format!("{clean}some-helper v1.0.0\npharmakos-mesher v0.1.0 (/r)\n");
        assert_eq!(
            first_reached("pharmakos-gateway", &broken, &walled),
            Some("pharmakos-mesher")
        );
    }

    #[test]
    fn wall_guard_relation_two_catches_the_client_reaching_the_core() {
        let forbidden = |bare: &[&str]| -> Vec<String> {
            bare.iter()
                .map(|name| format!("pharmakos-{name}"))
                .collect()
        };
        let (client, client_forbidden) = CLIENT_WALL
            .iter()
            .find(|(name, _)| *name == "client-gdext")
            .expect("the client's line");
        assert_eq!(*client, "client-gdext");
        let client_forbidden = forbidden(client_forbidden);
        // Today's shape: gdext, the mesher and proto. Clean.
        assert_eq!(
            first_reached("pharmakos-client-gdext", LISTING, &client_forbidden),
            None
        );
        // Each forbidden crate, reached directly or through another, is caught.
        for name in [
            "pharmakos-sim",
            "pharmakos-verifier",
            "pharmakos-plan-core",
            "pharmakos-gateway",
        ] {
            let broken = format!("{LISTING}{name} v0.1.0 (/repo)\n");
            assert_eq!(
                first_reached("pharmakos-client-gdext", &broken, &client_forbidden),
                Some(name),
                "{name}"
            );
        }
        // The mesher may reach nothing of the sim.
        let (_, mesher_forbidden) = CLIENT_WALL
            .iter()
            .find(|(name, _)| *name == "mesher")
            .expect("the mesher's line");
        let mesher_forbidden = forbidden(mesher_forbidden);
        let mesher = "pharmakos-mesher v0.1.0 (/r)\nxxhash-rust v0.8.15\n";
        assert_eq!(
            first_reached("pharmakos-mesher", mesher, &mesher_forbidden),
            None
        );
        let mesher_broken = format!("{mesher}pharmakos-sim v0.1.0 (/r)\n");
        assert_eq!(
            first_reached("pharmakos-mesher", &mesher_broken, &mesher_forbidden),
            Some("pharmakos-sim")
        );
        // A crate is never its own offence, and a prefix is not a name.
        assert_eq!(
            first_reached(
                "pharmakos-sim",
                "pharmakos-sim v0.1.0\npharmakos-simulator v1.0.0\n",
                &strings(&["pharmakos-sim"])
            ),
            None
        );
    }

    #[test]
    fn a_forbidden_crate_that_left_the_workspace_is_named() {
        let all = strings(&[
            "pharmakos-gateway",
            "pharmakos-plan-core",
            "pharmakos-sim",
            "pharmakos-verifier",
        ]);
        let wanted: &[&str] = &["sim", "verifier", "plan-core", "gateway"];
        assert!(missing_names(wanted, &all).is_empty());
        // `pharmakos-verifier` renamed: the relation must not quietly guard three.
        let renamed = strings(&["pharmakos-gateway", "pharmakos-plan-core", "pharmakos-sim"]);
        assert_eq!(missing_names(wanted, &renamed), vec!["verifier"]);
        // A prefix is not a name.
        let prefixed = strings(&["pharmakos-simulator"]);
        assert_eq!(missing_names(&["sim"], &prefixed), vec!["sim"]);
    }

    #[test]
    fn the_client_wall_names_walled_crates_only() {
        // Every line is a walled crate by name, and names no walled crate as
        // forbidden: the relation is client to core, never inside the wall.
        for (client, forbidden) in CLIENT_WALL {
            assert!(WALLED_PACKAGES.contains(client), "{client} is not walled");
            for name in *forbidden {
                assert!(!WALLED_PACKAGES.contains(name), "{name} is walled");
                assert!(
                    WALL_GUARDED_PACKAGES.contains(name),
                    "{name} is not a deterministic crate"
                );
            }
        }
    }

    #[test]
    fn a_missing_committed_chain_fails_and_bless_writes_it() {
        assert_eq!(
            chain_verdict(Some(b"0\ta\n"), b"0\ta\n", false),
            ChainVerdict::Matches
        );
        assert_eq!(
            chain_verdict(Some(b"0\ta\n"), b"0\ta\n", true),
            ChainVerdict::Matches
        );
        assert_eq!(
            chain_verdict(Some(b"0\ta\n"), b"0\tb\n", false),
            ChainVerdict::Moved
        );
        assert_eq!(
            chain_verdict(Some(b"0\ta\n"), b"0\tb\n", true),
            ChainVerdict::Bless
        );
        // The old `ok` with "no committed chain to compare yet" is a failure.
        assert_eq!(chain_verdict(None, b"0\ta\n", false), ChainVerdict::Missing);
        assert_eq!(chain_verdict(None, b"0\ta\n", true), ChainVerdict::Bless);
    }

    #[test]
    fn a_tool_is_found_by_path_without_running_it() {
        let dir = env::temp_dir().join("pharmakos-xtask-find-on-path");
        let empty = dir.join("empty");
        let full = dir.join("full");
        fs::create_dir_all(&empty).expect("temp dir");
        fs::create_dir_all(&full).expect("temp dir");
        // A file that would fail if run: the probe must not run it.
        fs::write(full.join("xvfb-run"), "#!/bin/sh\nexit 2\n").expect("write");
        assert_eq!(
            find_in_dirs("xvfb-run", &[empty.clone(), full.clone()]),
            Some(full.join("xvfb-run"))
        );
        assert_eq!(find_in_dirs("xvfb-run", std::slice::from_ref(&empty)), None);
        // A directory of that name is not the tool.
        fs::create_dir_all(empty.join("godot")).expect("temp dir");
        assert_eq!(find_in_dirs("godot", &[empty]), None);
    }

    #[test]
    fn the_allocations_notice_is_word_for_word() {
        assert_eq!(
            allocations_notice("linux"),
            "::notice title=allocations per tick::0 on linux: asserted by \
             crates/sim/tests/allocations.rs::a_tick_allocates_nothing in this commit's test \
             step; zero by construction, never compared"
        );
    }

    #[test]
    fn perf_alarms_is_not_a_step() {
        assert!(STEPS.iter().all(|step| step.name != "perf-alarms"));
    }

    #[test]
    fn every_shot_is_a_png_golden_under_vista() {
        let mut names: Vec<&str> = Vec::new();
        for shot in SHOTS {
            assert!(
                shot.golden.starts_with("tests/golden/vista/expected."),
                "{}",
                shot.golden
            );
            assert!(
                Path::new(shot.golden)
                    .extension()
                    .is_some_and(|extension| extension == "png"),
                "{}",
                shot.golden
            );
            assert!(
                shot.actual.starts_with("golden/vista/actual."),
                "{}",
                shot.actual
            );
            assert_eq!(
                shot.golden
                    .trim_start_matches("tests/golden/vista/expected."),
                shot.actual.trim_start_matches("golden/vista/actual."),
                "a shot's golden and actual share their name"
            );
            assert!(shot.scene.starts_with("res://scenes/"), "{}", shot.scene);
            assert!(shot.min_variance > 0, "{}", shot.name);
            names.push(shot.name);
        }
        assert_eq!(names, ["vista", "rows", "wizard"]);
    }

    /// Decisions-log item 116 (6)(g): every skip that stood for a missing input
    /// is a failure, and exactly three skips remain — `screenshot` off Linux,
    /// `stage-client` on macOS and `test-research`'s `--package` filter — plus
    /// `skip_or_fail`'s missing tool, which `--require-tools` turns into a
    /// failure. Counted over this file's own text, so a new skip is a red test
    /// that names this decision rather than a quiet addition.
    #[test]
    fn only_the_named_skips_remain() {
        let source = include_str!("main.rs");
        let needle = ["Outcome::", "Skipped("].concat();
        // run_cli's match arm reads a skip; it does not make one.
        let arm = ["Ok(", needle.as_str(), "note))"].concat();
        let mut constructions = 0_usize;
        for line in source.lines() {
            let code = line.trim_start();
            if code.starts_with("//") || code.starts_with(&arm) {
                continue;
            }
            constructions += code.matches(needle.as_str()).count();
        }
        // step_screenshot's platform skip, step_stage_client's macOS skip,
        // step_test_research's filter and skip_or_fail.
        assert_eq!(
            constructions, 4,
            "a new `Outcome::Skipped` was added or one was removed; a missing input is a \
             failure (decisions-log item 116 (6)(g)), and only the platform statements, the \
             --package filter and a missing tool skip"
        );
    }

    #[test]
    fn package_specs_name_the_sim_or_leave_it_out() {
        let sim = "pharmakos-sim";
        for spec in ["pharmakos-sim", "pharmakos-sim@0.1.0", "x*", "[p]", "a#b"] {
            assert!(package_spec_may_name(spec, sim), "{spec}");
        }
        for spec in [
            "pharmakos-gateway",
            "pharmakos-gateway@0.1.0",
            "pharmakos-sim-extra",
            "pharmakos",
            "",
        ] {
            assert!(!package_spec_may_name(spec, sim), "{spec}");
        }
    }

    #[test]
    fn package_is_not_a_step() {
        assert!(STEPS.iter().all(|step| step.name != "package"));
    }

    /// Decisions-log item 117 (4): the client's own profile passes `profiles`,
    /// which fails only a profile that switches overflow checks off.
    #[test]
    fn the_release_client_profile_passes_the_profiles_step() {
        let manifest = include_str!("../../Cargo.toml");
        assert_eq!(scan_profiles(manifest), (true, Vec::new()));
        let block: Vec<&str> = manifest
            .lines()
            .skip_while(|line| line.trim() != "[profile.release-client]")
            .skip(1)
            .take_while(|line| !line.trim().starts_with('['))
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();
        assert_eq!(block, ["inherits = \"release\"", "panic = \"unwind\""]);
        // And the step would still catch the profile switching them off.
        let broken = "[profile.release]\noverflow-checks = true\n[profile.release-client]\n\
                      inherits = \"release\"\noverflow-checks = false\n";
        assert_eq!(
            scan_profiles(broken),
            (true, vec!["profile.release-client".to_owned()])
        );
    }

    #[test]
    fn ci_scope_is_not_a_step() {
        assert!(STEPS.iter().all(|step| step.name != "ci-scope"));
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

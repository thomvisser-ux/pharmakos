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
//! | `profiles`       | the fast first look at the TOML: overflow checks on in every profile, release included; reads every profile line or fails |
//! | `test`           | `cargo test --workspace` without `research`, each invocation's resolved profile checked first: debug assertions and overflow checks on, the sim and the crates it runs optimised (its allocation test at opt-level 0), the godot family not, rustflags pinned to none and refused in the environment |
//! | `test-research`  | `cargo test --package pharmakos-sim --features pharmakos-sim/research`, checked as `test` is |
//! | `research-guard` | plan-core / verifier / operator / gateway / gamectl / bench never reach `research` |
//! | `wall-guard`     | `sim` and the deterministic crates never reach a walled crate; the client never the core |
//! | `deny`           | `cargo deny check` (licences, advisories, banned crates)                      |
//! | `buf`            | `buf lint` and `buf breaking --against .git#branch=main`                      |
//! | `golden`         | `tests/golden/**/expected.*` against the fresh `target/golden/**/actual.*`    |
//! | `determinism`    | builds the determinism binary at `release-checked`, its resolved profile checked (both checks on, rustflags pinned), runs it and compares its per-tick hash chain |
//! | `reuse`          | REUSE licence-manifest check, then the library-quotation licence check        |
//! | `placeholders`   | every `PLACEHOLDER` marker in the tracked tree parses against the grammar     |
//! | `scenario`     | validates `scenarios/**`, then plays each with `gamectl scenario run`         |
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
//! Missing workspace metadata is a failure in every step that reads it,
//! including the three that used to fall back without it — `golden`'s target
//! directory, `clippy`'s walled-crate list and its research pass — so no step
//! lints, compares or skips something other than what the metadata says (S1's
//! plan, decision 9, ruling the register's S1-05).
//!
//! # `PHARMAKOS_REQUIRE_TOOLS`, read one way everywhere
//!
//! `1` requires the tools; unset or `0` does not; any other value is an error
//! naming the variable, never a guess (S1-06). `--require-tools` and
//! `--no-require-tools` override it, and every child process xtask starts —
//! each step's, the metadata read, the tool probes, `perf-alarms`',
//! `ci-scope`'s and `package`'s — is built by [`child_command`], which sets the
//! variable to `1` or removes it, so the answer reaches a test that needs a tool
//! too (S1-07). `every_child_goes_through_child_command` pins that over the
//! source text.
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
//! says, and P1's wall-clock half from the walled bench,
//! `crates/bench/tests/perf_alarm.rs` (QUICK's p50 and p99 over the committed
//! verifier cases and `budget_128`, and ns per size unit per decision tick in
//! a hosted match at the size budget; S1's plan, task `p1`), and G3′-real's
//! figures from the same crate, `crates/bench/tests/g3_real.rs` (the
//! 8-minute segment at the gate's load phase by phase, the support search's
//! adversarial tick and a fourth seat's decision-tick cost; S2's plan, task
//! `g3h`), each with
//! `--release`, `--ignored` and `--nocapture`, and each printing its own
//! `::notice::` lines; from S3's `bench3` also P1 FULL (FULL over the
//! committed cases and both depths over the generated shapes at each size of
//! E24's curve), the decision tick at each size in nanoseconds and counted
//! evaluation units (`p1-decision`), and P3 (`crates/bench/tests/p3_alarm.rs`:
//! the repath and the travel estimate on three seeds); then it publishes the
//! allocations per
//! tick as a notice naming the test that asserts it. It has no threshold, compares nothing
//! across operating systems, and never exits non-zero: a failed measurement is
//! a `::warning::` (skeleton-plan section 7 decision 23, logged as
//! decisions-log item 116 (6)(b)). Named (`mesher`, `p1`, `p1-cap`,
//! `p1-decision`, `p3`, `g3`, `allocations`), it runs that one alone, which is how CI gives each its
//! own step: GitHub annotates only a step's first ten notices (item 149 (4)).
//!
//! `cargo xtask g3-budget` is not a step either ([`g3`]): G3′-real's CI alarm
//! (S2's plan, E18), which runs the bench's `g3_budget_segment` and, unlike
//! the perf alarms, fails at the gate's numbers doubled or with no verdict.
//! `cargo xtask p3-budget` and `cargo xtask p1-budget` ([`budget`]) are S3's
//! two authoring alarms of the same kind (S3's plan, E12): each runs one bench
//! test and fails at its gate's numbers doubled or with no verdict.
//!
//! Usage:
//!
//! ```text
//! cargo xtask ci                    # everything, fail fast
//! cargo xtask ci --quick            # fmt, clippy, test — the inner loop
//! cargo xtask ci --fix              # rustfmt and the machine-applicable clippy fixes
//! cargo xtask ci --skip buf         # everything except one step
//! cargo xtask ci --shard 2/3        # one of CI's shards: see [`shard`]
//! cargo xtask clippy -p pharmakos-sim   # one step, scoped (what the pre-commit hook runs)
//! cargo xtask fmt clippy            # two steps, whole workspace (what the pre-push hook runs)
//! cargo xtask golden --bless        # accept the fresh outputs as the new goldens
//! cargo xtask stage-client --check  # build the cdylib, stage, import, run the client check
//! cargo xtask list                  # list the steps
//! cargo xtask ci-scope              # CI's docs-only fast path: `full` or `prose`
//! cargo xtask perf-alarms           # the per-runner perf notices; never fails
//! cargo xtask perf-alarms g3        # one alarm (mesher, p1, p1-cap, p1-decision, p1-kinds, p3, g3, allocations)
//! cargo xtask g3-budget             # G3′'s CI alarm: fails at the gate's numbers doubled
//! cargo xtask p3-budget             # P3's CI alarm: a repath p99 over 10 ms, an estimate over 2 ms
//! cargo xtask p1-budget             # P1's CI alarm: QUICK over 10 ms, FULL or end to end over 100 ms at the budget
//! cargo xtask package               # the unsigned zip for this platform (T21)
//! cargo xtask placeholders          # the PLACEHOLDER register; --check fails off-grammar
//! ```
//!
//! # The PLACEHOLDER register
//!
//! `cargo xtask placeholders` collects every `PLACEHOLDER` marker in the
//! tracked tree, parses each against the one-line grammar, `<what> — <who>,
//! <when>` after the marker's colon ([`placeholders::GRAMMAR`]), prints the
//! register grouped by when and by who, and lists the markers still to be
//! reworded. With `--check` it fails while any marker is off the grammar.
//! Since S1's last `xtask` pull request (`tune`), which reworded every marker
//! off the grammar, the same check is the `placeholders` step of `ci` (S1's
//! plan, decision 9, ruling the register's S1-10): the step prints only the
//! count, or the markers that fail it, and the command prints the register;
//! see [`placeholders`].
//!
//! # The shards
//!
//! CI runs each operating system's suite as three jobs, `cargo xtask ci
//! --shard N/3` (decisions-log item 148 (2)): every shard runs its part of
//! `test`, `test-research` and `scenario`, split by a hash of each test
//! target's or scenario's name; shard 1 also runs every other step but
//! `golden`, which the leg's `cargo xtask ci (<os>)` job runs over the merged
//! outputs. Without `--shard`, `ci` runs every step over every unit, as it
//! always has. See [`shard`] for the split and why it holds.
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

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus, Stdio};

// Harness part 2's formats, each in its own file so that the step table above
// stays readable and so that a task working on one of them touches one file.
mod annotate;
mod budget;
mod g3;
mod golden;
mod licence_quotes;
mod package;
mod placeholders;
mod png;
mod resolved;
mod runner;
mod scenario;
mod scope;
mod shard;
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
/// Two of the five names are the spec's own wording — "a walled
/// presentation/solve module" (spec section 15, Maths). The other three are
/// crates: `client-gdext`, the thin gdext bridge; `mesher`, the greedy
/// mesher, which decisions log section 2.7 item 56 made its own walled crate
/// rather than a module inside the bridge — it takes integer chunk data in and
/// produces vertex buffers out, links without gdext so the headless CPU proxy
/// and the CI geometry check can reuse it, and is depended on by `client-gdext`
/// alone today; and `bench`, P1's timing harness (S1's plan, decision 2,
/// approved by decisions-log item 128 (3) (b)), the walled harness AGENTS.md
/// section 4.5 sanctions: it drives the verifier and a hosted match from
/// outside to put a wall-clock figure on them, is never shipped, and reports
/// through `cargo xtask perf-alarms`. `wall-guard` below enforces the half
/// that matters to determinism — that no crate in [`WALL_GUARDED_PACKAGES`]
/// ever reaches a walled crate. A crate that is not on this list gets no float
/// or clock allowance. Adding a name
/// widens the float, cast, hash-map and clock allowance for a whole crate, so
/// it is a contract change and needs owner approval — keep this list,
/// `clippy.toml`'s header comment and AGENTS.md section 4.9 in step.
///
/// PLACEHOLDER: `presentation` and `solve`, named and not created — owner, if either is proposed.
/// The spec names them; `pharmakos-client-gdext`, `pharmakos-mesher` and `pharmakos-bench` exist,
/// and a proposal of either of the other two is an AGENTS.md section 5 change (the register's
/// M-02).
const WALLED_PACKAGES: &[&str] = &["presentation", "solve", "client-gdext", "mesher", "bench"];

/// Crates that may never reach the `research` feature, which gates `fork`.
///
/// `sim` is deliberately absent: it is the crate that *defines* the feature, so
/// it is the one package for which reaching `research` is correct. Walled
/// crates are kept away from the deterministic ones by the separate
/// [`WALL_GUARDED_PACKAGES`] list below, which does include `sim`. `gamectl`
/// joined at decisions-log item 109: it takes the sim with default features
/// off, because the gateway's API is written in the sim's types. `bench`
/// joined with S1's `p1` (decision 2, decisions-log item 128 (3) (b)): it is
/// walled, and it times the verifier and a hosted match, so a `fork` within
/// its reach would let it time a dry run nobody ships. It is the one walled
/// crate here, and so the one name on this list that is not also on
/// [`WALL_GUARDED_PACKAGES`].
const GUARDED_PACKAGES: &[&str] = &[
    "plan-core",
    "verifier",
    "operator",
    "gateway",
    "gamectl",
    "bench",
];

/// Crates that may never depend on a walled crate, transitively included.
///
/// The deterministic crates: `sim`, and every crate of [`GUARDED_PACKAGES`]
/// that is not itself walled — all of them but `bench`, the walled timing
/// harness, which is on the research guard and behind the wall at once. The
/// deterministic crates must not reach the float, cast, hash-map and clock
/// allowance, and neither must the sim — it is the crate that owns hashed
/// state, so it is the one the wall exists to protect (AGENTS.md section 4.9).
/// The two lists are separate rather than one because `sim` defines the
/// `research` feature and so cannot join the research guard, and because a
/// walled crate guarded from `research` is still a walled crate.
/// `the_guard_lists_partition_the_crates` pins the relation. Keep this list,
/// AGENTS.md section 4.9 and `clippy.toml`'s header in step; widening it is a
/// contract change like any other.
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
/// taken as item 116 (6)(d); the operator on the client's line by the owner's
/// decision, item 118 (5)).
///
/// `wall-guard`'s second relation. The first keeps the deterministic crates
/// away from the float, cast, hash-map and clock allowance; this one keeps the
/// thin client away from the rules. `client-gdext` marshals and decides nothing
/// (AGENTS.md section 3 rule 4), so it reaches neither the sim nor the crates
/// that hold a rule, a verdict, a socket or a gameplay decision: the verifier,
/// plan-core, the gateway, and the operator, the crate of templates and utility
/// scoring, whose library depends on `pharmakos-proto` alone and so would pass
/// every other check if the client reached it; the mesher takes integer chunk data
/// in and "nothing from the sim". Each entry is a walled crate by name, and a
/// walled crate on the client side of the wall — one that must decide nothing
/// — joins by adding its own line. Not every walled crate has one: `bench`,
/// P1's timing harness, exists to reach the sim, the verifier and the gateway,
/// so a line for it would name nothing it may not reach (S1's plan, section 5
/// and decision 2). It is deliberately NOT "no walled crate reaches the sim":
/// that would forbid the walled harness AGENTS.md section 4.5 sanctions, a
/// walled crate driving the sim from outside to put a millisecond figure on
/// it, which `bench` is. Names are bare, as in
/// [`WALLED_PACKAGES`], and matched with and without the `pharmakos-` prefix.
/// `crates/client-gdext/tests/no_sim.rs` checks the client's line from
/// `Cargo.lock` as well, and its
/// `forbidden_names_the_crates_on_client_walls_client_line` reads this line
/// out of this file and fails when its own list names other crates.
const CLIENT_WALL: &[(&str, &[&str])] = &[
    (
        "client-gdext",
        &["sim", "verifier", "plan-core", "gateway", "operator"],
    ),
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

/// The ticks the `determinism` step runs: 3 600 lines, the initial state and
/// the first 3 599 ticks of the first-round Push (three minutes at 20 Hz on
/// the rules table's 3 / 5 / 8 ladder), stopping one tick before the tick
/// that closes the segment, which no chain this step writes holds; with every
/// seat sealing the committed copy of the Hold & Build template's
/// instantiation (`pharmakos_sim::DETERMINISM_PLAYBOOK_PATH`) on
/// `DETERMINISM_MATCH_SEED` (S1's plan, decision 16; register S1-26). It was
/// 1 200, a one-minute smoke run of a harness playbook over short harness
/// segments, until S1's `tune` lane. The chain covers a real playbook's walk,
/// deploy, Build target, fabrication and build, where the harness covered
/// fifty walkers a seat. G4's bar (10 matches of 9 600 ticks) is the nightly
/// workflow's `g4` job (S2's `night`), and G3'-real's load is S2's.
/// `crates/sim/tests/determinism.rs`'s `GOLDEN_TICKS` holds the same number.
const DETERMINISM_TICKS: &str = "3600";

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
        about: "overflow checks stay on in every profile; every profile line is read (a first look)",
        run: step_profiles,
    },
    Step {
        name: "test",
        about: "cargo test --workspace, without the research feature, its resolved profile checked: \
                checks on, the sim optimised, rustflags pinned",
        run: step_test,
    },
    Step {
        name: "test-research",
        about: "cargo test on pharmakos-sim alone, with the research feature, checked as test is",
        run: step_test_research,
    },
    Step {
        name: "research-guard",
        about: "plan-core/verifier/operator/gateway/gamectl/bench must not reach `research`",
        run: step_research_guard,
    },
    Step {
        name: "wall-guard",
        about: "sim and the deterministic crates never reach a walled crate; the client never the core",
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
        about: "build the determinism binary, its resolved profile checked; run it and check its \
                hash chain",
        run: step_determinism,
    },
    Step {
        name: "reuse",
        about: "REUSE licence-manifest check, and the library-quotation licence check",
        run: step_reuse,
    },
    Step {
        name: "placeholders",
        about: "every PLACEHOLDER marker in the one-line grammar `<what> — <who>, <when>`",
        run: step_placeholders,
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

/// The hidden subcommand that prints the runner's `--config` value
/// ([`runner_config_for`]).
const RUNNER_CONFIG_SUBCOMMAND: &str = "__runner-config";

fn main() -> ExitCode {
    // xtask's test runner (`runner.rs`), as cargo starts it in a checked run:
    // read before anything else, and as the operating system's strings, so a
    // test's arguments reach it untouched.
    let raw: Vec<std::ffi::OsString> = env::args_os().skip(1).collect();
    if let Some((first, rest)) = raw.split_first()
        && first == runner::SUBCOMMAND
    {
        return runner::main(rest);
    }
    // Every command, a step or not, passes the toolchain's door first, before
    // anything builds (`fmt`, `clippy` and `deny` included).
    if let Err(message) = toolchain_at_the_door() {
        eprintln!("xtask: {message}");
        return ExitCode::FAILURE;
    }
    // The runner's `--config` value for a cargo working in the folder named,
    // for `xtask/tests/doctest_runner.rs`: the one spelling rule, asked of
    // xtask itself.
    if let Some((first, rest)) = raw.split_first()
        && first == RUNNER_CONFIG_SUBCOMMAND
    {
        let answer = match rest {
            [cwd] => env::current_exe()
                .map_err(|error| format!("finding xtask's own executable: {error}"))
                .and_then(|xtask| runner_config_for(&xtask, Path::new(cwd))),
            _ => Err(format!(
                "`{RUNNER_CONFIG_SUBCOMMAND}` takes one argument, the folder cargo works in"
            )),
        };
        return match answer {
            Ok(value) => {
                println!("{value}");
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("xtask: {message}");
                ExitCode::FAILURE
            }
        };
    }
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
    /// skipping when buf, cargo-deny, reuse, godot or xvfb-run is missing
    /// ([`skip_or_fail`]). CI sets the variable for the whole workflow (ci.yml's
    /// top-level `env`) and installs each tool its job needs, so a runner that
    /// lost a tool goes red rather than quietly green. Every child xtask starts
    /// inherits the answer through [`REQUIRE_TOOLS_VAR`] ([`child_command`]), so
    /// a test that needs a tool fails with it too. The variable is read by
    /// [`require_tools_from`]: `1` on, unset or `0` off, anything else an error.
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
    /// `--shard N/M`: run shard N's part of the sharded steps, and on shard 1
    /// every step that is not sharded ([`shard`]). `None` runs everything.
    shard: Option<shard::Shard>,
}

fn run_cli(args: &[String]) -> Result<bool, String> {
    let mut commands: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut packages: Vec<String> = Vec::new();
    let mut shard: Option<shard::Shard> = None;
    let mut bless = false;
    let mut client_check = false;
    let mut quick = false;
    let mut fix = false;
    // Read strictly, before any flag: a value other than `1`, `0` or unset is
    // a mistake in the environment, and a flag that happened to override it
    // would leave the mistake for the next run to trip over (S1-06).
    let mut require_tools = require_tools_from(env::var_os(REQUIRE_TOOLS_VAR).as_deref())?;
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
                // Only what follows the command names the alarm; a flag
                // before it was read by this loop like any other.
                let selected: Vec<&String> = args.iter().skip(index + 1).collect();
                let only = match selected.as_slice() {
                    [] => None,
                    [one] => Some(alarm_selection(one)?),
                    _ => {
                        return Err("`perf-alarms` takes at most one alarm name: mesher, p1, \
                                    p1-cap, p1-decision, p1-kinds, p3, g3 or allocations"
                            .to_owned());
                    }
                };
                perf_alarms(only);
                return Ok(true);
            }
            // Not steps: builds and runs the nightly jobs and the watch check
            // make, each through the checked call, so what they test, hash or
            // compare comes from a checked build.
            "determinism-run" => {
                let mut seed: Option<&String> = None;
                let mut ticks: Option<&String> = None;
                let mut out: Option<&String> = None;
                let mut rest = args.iter().skip(index + 1);
                while let Some(flag) = rest.next() {
                    let slot = match flag.as_str() {
                        "--seed" => &mut seed,
                        "--ticks" => &mut ticks,
                        "--out" => &mut out,
                        other => {
                            return Err(format!(
                                "`determinism-run` takes --seed, --ticks and --out, and was \
                                 given `{other}`"
                            ));
                        }
                    };
                    if slot.is_some() {
                        return Err(format!("`determinism-run` was given {flag} twice"));
                    }
                    *slot = Some(rest.next().ok_or_else(|| format!("{flag} needs a value"))?);
                }
                let (Some(seed), Some(ticks), Some(out)) = (seed, ticks, out) else {
                    return Err("`determinism-run` needs --seed, --ticks and --out".to_owned());
                };
                let ctx = standalone_ctx(require_tools)?;
                determinism_run(&ctx, seed, ticks, out)?;
                return Ok(true);
            }
            "build-gamectl" => {
                let release = match args.get(index + 1).map(String::as_str) {
                    None => false,
                    Some("--release") if args.get(index + 2).is_none() => true,
                    Some(other) => {
                        return Err(format!(
                            "`build-gamectl` takes only `--release`, and was given `{other}`"
                        ));
                    }
                };
                let ctx = standalone_ctx(require_tools)?;
                let owner = metadata(&ctx)?
                    .package_with_bin(SCENARIO_BIN)
                    .ok_or_else(|| format!("no `{SCENARIO_BIN}` binary in the workspace"))?;
                let path = checked_gamectl(&ctx, &owner, release)?;
                println!("   built {path}");
                return Ok(true);
            }
            "checked-test" => {
                let ctx = standalone_ctx(require_tools)?;
                let rest: Vec<String> = args.iter().skip(index + 1).cloned().collect();
                checked_test(&ctx, &rest)?;
                return Ok(true);
            }
            // Not a step: G3′-real's CI alarm (S2's plan, E18; task `g3r`).
            // Unlike `perf-alarms` it fails, at the gate's numbers doubled and
            // whenever it gives no verdict.
            "g3-budget" => {
                // A flag before it (`--require-tools`) was read by this loop;
                // anything after it is a mistake.
                if let Some(other) = args.get(index + 1) {
                    return Err(format!(
                        "`g3-budget` takes no arguments, and was given `{other}`"
                    ));
                }
                let root = workspace_root()?;
                let locked = env::var_os("CI").is_some();
                return g3::run(&root, require_tools, locked).map_err(|error| {
                    println!("::error title=G3' budget::no verdict: {error}");
                    error.to_string()
                });
            }
            // Not steps: S3's authoring alarms (S3's plan, E12; task
            // `bench3`). Like `g3-budget` they fail, at the gates' numbers
            // doubled and whenever they give no verdict.
            command if budget::BUDGETS.iter().any(|alarm| alarm.command == command) => {
                let alarm = budget::BUDGETS
                    .iter()
                    .find(|alarm| alarm.command == command)
                    .ok_or_else(|| format!("`{command}` is not an authoring alarm"))?;
                if let Some(other) = args.get(index + 1) {
                    return Err(format!(
                        "`{}` takes no arguments, and was given `{other}`",
                        alarm.command
                    ));
                }
                let root = workspace_root()?;
                let locked = env::var_os("CI").is_some();
                return budget::run(alarm, &root, require_tools, locked).map_err(|error| {
                    println!("::error title={}::no verdict: {error}", alarm.title);
                    error.to_string()
                });
            }
            // The PLACEHOLDER register, printed whole. The `placeholders`
            // step runs the same check and prints only its verdict; this
            // command, which `--check` makes fail off the grammar, is the one
            // to read the register with. `--check` is the only flag it reads.
            "placeholders" => {
                let check = args.iter().any(|arg| arg == "--check");
                if let Some(other) = args
                    .iter()
                    .find(|arg| *arg != "placeholders" && *arg != "--check")
                {
                    return Err(format!(
                        "`placeholders` takes only `--check`, and was given `{other}`"
                    ));
                }
                let root = workspace_root()?;
                return placeholders::run(&root, require_tools, check);
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
            "--shard" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--shard needs `N/M`, shard N of M".to_owned())?;
                if shard.is_some() {
                    return Err("--shard is given once".to_owned());
                }
                shard = Some(shard::Shard::parse(value).map_err(|error| error.to_string())?);
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
    if let Some(shard) = shard {
        shard_flags_allowed(&commands, &packages, quick, fix, bless)
            .map_err(|reason| format!("--shard {shard}: {reason}"))?;
    }

    let root = workspace_root()?;
    let cargo = cargo_program()?;
    let locked =
        locked.unwrap_or_else(|| env::var_os("CI").is_some() && root.join("Cargo.lock").is_file());
    let workspace = load_workspace(require_tools, &cargo, &root);
    if let Err(reason) = &workspace {
        println!("note: workspace metadata unavailable ({reason})");
        println!(
            "note: every step that reads it fails (decisions-log item 116 (6)(g); S1's plan, \
             decision 9) — none falls back"
        );
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
        shard,
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
                if wanted
                    && !skipped.iter().any(|s| s == step.name)
                    && step_in_shard(step.name, ctx.shard)
                {
                    plan.push(step);
                }
            }
            if let Some(shard) = ctx.shard {
                println!();
                println!(
                    "== shard {shard}: {} over this shard's part of their units{}; {} on no \
                     shard, run over every shard's merged outputs (`cargo xtask golden`)",
                    shard::SHARDED_STEPS.join(", "),
                    if shard.is_first() {
                        ", and every other step"
                    } else {
                        "; every other step runs on shard 1"
                    },
                    shard::MERGED_STEPS.join(", ")
                );
            }
        } else {
            let step = STEPS
                .iter()
                .find(|step| step.name == name)
                .ok_or_else(|| format!("unknown command `{name}` (try `cargo xtask list`)"))?;
            plan.push(step);
        }
    }

    // A plan that runs tests needs this xtask named as their runner: settled
    // here, by the same function the run uses, before any step builds
    // anything, so a target folder cargo cannot be given fails first, not
    // after fmt, clippy and deny.
    if plan
        .iter()
        .any(|step| step.name == "test" || step.name == "test-research")
    {
        let xtask = env::current_exe()
            .map_err(|error| format!("finding xtask's own executable: {error}"))?;
        runner_config_for(&xtask, &ctx.root)?;
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

/// Whether `ci`'s step `name` runs on `shard`: every step without `--shard`;
/// with it, the sharded steps on every shard, the merged ones on none, and the
/// rest on shard 1 ([`shard`]).
fn step_in_shard(name: &str, shard: Option<shard::Shard>) -> bool {
    let Some(shard) = shard else {
        return true;
    };
    if shard::SHARDED_STEPS.contains(&name) {
        true
    } else if shard::MERGED_STEPS.contains(&name) {
        false
    } else {
        shard.is_first()
    }
}

/// `--shard` belongs to `ci` alone, and to no flag that would make a shard
/// mean something else: `--quick` would run part of the inner loop's tests on
/// a shard and none of the rest; `--package` narrows the units cargo builds, which the
/// split's `--workspace` selection must not change; `--fix` edits rather than
/// checks; and `--bless` would rewrite goldens from a part of the outputs.
fn shard_flags_allowed(
    commands: &[String],
    packages: &[String],
    quick: bool,
    fix: bool,
    bless: bool,
) -> Result<(), String> {
    if commands.len() != 1 || commands.first().map(String::as_str) != Some("ci") {
        return Err("it splits `ci` and is given with `ci` alone".to_owned());
    }
    if !packages.is_empty() {
        return Err(
            "it cannot be combined with --package: the shards select from the whole workspace"
                .to_owned(),
        );
    }
    if quick {
        return Err(
            "it cannot be combined with --quick: the inner loop is a local run, and a shard \
             would run only part of its tests"
                .to_owned(),
        );
    }
    if fix {
        return Err("it cannot be combined with --fix, which edits and then stops".to_owned());
    }
    if bless {
        return Err(
            "it cannot be combined with --bless: a shard holds only part of the fresh outputs"
                .to_owned(),
        );
    }
    Ok(())
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
    run(ctx, &ctx.cargo, &with_pinned_linker(&args)?)?;
    println!();
    println!("fixes applied — review the diff, then run `cargo xtask ci`.");
    Ok(())
}

fn print_help() {
    println!("cargo xtask — the one command that runs every check");
    println!();
    println!("USAGE:");
    println!("    cargo xtask ci [--quick|--fix] [--skip <step>]... [--shard <N/M>]");
    println!("    cargo xtask <step> [flags]");
    println!("    cargo xtask list");
    println!("    cargo xtask ci-scope       CI's docs-only fast path: prints `full` or `prose`");
    println!("    cargo xtask perf-alarms [<alarm>]   per-runner perf notices; never fails");
    println!(
        "                               (one of mesher, p1, p1-cap, p1-decision, p3, g3, allocations; \
         none: all)"
    );
    println!(
        "    cargo xtask g3-budget      G3' CI alarm: fails at a p99 over 50 ms or a mean over 25 ms"
    );
    println!(
        "    cargo xtask p3-budget      P3 CI alarm: fails at a repath p99 over 10 ms or an \
         estimate p99 over 2 ms"
    );
    println!(
        "    cargo xtask p1-budget      P1 CI alarm: fails at a QUICK p99 over 10 ms or a FULL \
         p99 over 100 ms at the size budget"
    );
    println!(
        "    cargo xtask package        the unsigned zip for this platform; --bless its manifest"
    );
    println!(
        "    cargo xtask placeholders   the PLACEHOLDER register; --check fails off the grammar"
    );
    println!();
    println!("FLAGS:");
    println!("    --quick            fmt, clippy and tests only — the inner loop");
    println!("    --fix              apply rustfmt and machine-applicable clippy fixes, then stop");
    println!("    --skip <step>      leave one step out of `ci`");
    println!(
        "    --shard <N/M>      ci: shard N of M: its part of test, test-research and scenario,"
    );
    println!(
        "                       and on shard 1 every step but golden (run on the merged outputs)"
    );
    println!("    -p, --package <n>  restrict package-scoped steps to these packages");
    println!("    --bless            rewrite golden files from the fresh outputs");
    println!("    --check            stage-client: also run the headless client check;");
    println!("                       placeholders: fail while a marker is off the grammar");
    println!("    --require-tools    fail instead of skipping when a tool is missing");
    println!("                       (or PHARMAKOS_REQUIRE_TOOLS=1; 0 or unset is off)");
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
    let walled = walled_packages(&ctx.workspace)?;
    let mut notes: Vec<String> = Vec::new();

    if !ctx.packages.is_empty() {
        // A scoped run (the pre-commit hook's `cargo xtask clippy -p <crate>`)
        // lints each named crate the way the unscoped run does: a deterministic
        // crate in pass 1, a walled one in pass 2 with the wall's allowances.
        for args in scoped_clippy_args(&ctx.packages, &walled, ctx.locked) {
            run(ctx, &ctx.cargo, &with_pinned_linker(&args)?)?;
        }
        return Ok(Outcome::Done(format!(
            "clean for {} (scoped run)",
            ctx.packages.join(", ")
        )));
    }

    // Pass 1 — everything outside the wall, with the full deny set.
    let mut args: Vec<String> = vec!["clippy".to_owned(), "--workspace".to_owned()];
    for package in &walled {
        args.push("--exclude".to_owned());
        args.push(package.clone());
    }
    args.push("--all-targets".to_owned());
    if ctx.locked {
        args.push("--locked".to_owned());
    }
    args.push("--".to_owned());
    for flag in DETERMINISM_DENY {
        args.push((*flag).to_owned());
    }
    run(ctx, &ctx.cargo, &with_pinned_linker(&args)?)?;
    notes.push("deterministic crates clean".to_owned());

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
        run(ctx, &ctx.cargo, &with_pinned_linker(&walled_args)?)?;
        notes.push(format!("walled crates clean ({})", walled.join(", ")));
    }

    // Pass 3 — the research configuration, so `fork` code is linted too. Named
    // explicitly rather than with `--all-features`, which would turn `research`
    // on everywhere and hide the very ban `research-guard` checks. A workspace
    // with no crate declaring the feature fails here rather than skipping the
    // pass (S1-05): the stage has `fork`, so a lint run without it is a run
    // that checked less than it says.
    let feature = research_feature_spec(&ctx.workspace)?;
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
    run(ctx, &ctx.cargo, &with_pinned_linker(&research_args)?)?;
    notes.push(format!("research build clean ({feature})"));

    Ok(Outcome::Done(notes.join("; ")))
}

/// The `clippy` commands of a scoped run (`-p` given), in order: the named
/// deterministic crates with the full deny set (pass 1), then the named walled
/// crates with the deny set plus [`WALL_ALLOW`] (pass 2), each pass only when it
/// names something. Without this split a scoped run linted a walled crate with
/// pass 1's flags and stopped, so `cargo xtask clippy -p pharmakos-client-gdext`
/// (the pre-commit hook's call) failed on the floats the wall exists to hold.
///
/// A `-p` value goes to pass 2 only when it names a walled crate outright, as
/// `name` or `name@version`. Anything else, a glob or a package-id URL included,
/// stays in pass 1: a value that might reach a walled crate is linted under the
/// stricter set and fails loudly rather than lint a deterministic crate with
/// the wall's allowances.
///
/// Cargo also lints a named crate's path dependencies that are workspace
/// members, under the same flags. So pass 2 reaches whatever a walled crate
/// depends on (`bench`: the sim, the verifier, the gateway), exactly as the
/// unscoped pass 2 does; the deterministic crates' own check is pass 1.
fn scoped_clippy_args(packages: &[String], walled: &[String], locked: bool) -> Vec<Vec<String>> {
    let names_walled = |spec: &&String| {
        let name = spec
            .split_once('@')
            .map_or(spec.as_str(), |(before, _)| before);
        !spec.contains(['*', '?', '[', '#', ':', '/', '\\'])
            && walled.iter().any(|walled| walled == name)
    };
    let (in_wall, outside): (Vec<&String>, Vec<&String>) = packages.iter().partition(names_walled);

    let mut passes: Vec<Vec<String>> = Vec::new();
    for (selected, allow) in [(outside, false), (in_wall, true)] {
        if selected.is_empty() {
            continue;
        }
        let mut args: Vec<String> = vec!["clippy".to_owned()];
        for package in selected {
            args.push("--package".to_owned());
            args.push(package.clone());
        }
        args.push("--all-targets".to_owned());
        if locked {
            args.push("--locked".to_owned());
        }
        args.push("--".to_owned());
        args.extend(DETERMINISM_DENY.iter().map(|flag| (*flag).to_owned()));
        if allow {
            args.extend(WALL_ALLOW.iter().map(|flag| (*flag).to_owned()));
        }
        passes.push(args);
    }
    passes
}

/// Overflow checks stay on — in debug, and above all in release, where cargo
/// would otherwise turn them off. A wrap that happens on one platform's path
/// and not another's is exactly the failure this project cannot absorb, so a
/// profile that switches the check off fails the build.
///
/// This step is the fast first look: it reads the committed TOML, every
/// profile in it, release and `release-client` included, before anything
/// builds, and fails closed: a `[profile.*]` line in a form it cannot read (a
/// dotted key, an inline table, a quoted key, a bare `[profile]` table, an
/// array of tables, a multi-line value) is a failure, never a pass. It cannot
/// see `--config`, a config file outside the repository or rustflags. What
/// cargo actually resolves is checked from cargo's own answer, on every build
/// xtask makes to test, to hash or to ship ([`resolved`]): the `test` and
/// `test-research` steps' invocations, the `determinism` step's
/// `release-checked` build and `cargo xtask package`'s two release builds.
///
/// A profile of the workspace's own (`release-checked`, `release-client`,
/// `xtask`) cannot fall outside the rule: cargo refuses one without an
/// `inherits`, with an unknown parent or in a loop, so every profile's checks
/// come from `dev` (on by cargo's default, and set on here) or `release`
/// (held on above), and the scan of both files catches any profile that
/// switches them off.
fn step_profiles(ctx: &Ctx) -> Result<Outcome, String> {
    let manifest_path = ctx.root.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let manifest = scan_profiles(&manifest);
    // A cargo config can override the manifest's profiles, so it is checked too
    // — otherwise the ban could be lifted in a file nobody reads.
    let config =
        read_config(&ctx.root.join(".cargo").join("config.toml"))?.map(|text| scan_profiles(&text));
    let mut failures = profile_failures(&manifest, config.as_ref());
    // The package tables against the packages cargo resolves: a stale table,
    // or one that keeps a crate outside the godot family at opt-level 0.
    let graph = sim_graph(ctx)?;
    failures.extend(dev_package_table_failures(&manifest, &graph));
    if !failures.is_empty() {
        return Err(failures.join("; "));
    }
    Ok(Outcome::Done(
        "overflow checks on in release, and switched off nowhere; every profile line read; \
         every dev package table names a package cargo resolves, and only the godot family \
         sits at opt-level 0 (debug assertions and the sim's optimisation are the test \
         step's, from cargo)"
            .to_owned(),
    ))
}

/// The cargo config at `path`: `None` when there is none, which leaves nothing
/// to check; a config that is there and cannot be read is a failure, not a
/// pass.
fn read_config(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot read {}: {error}", path.display())),
    }
}

/// What [`scan_profiles`] found in one TOML file's `[profile.*]` tables.
#[derive(Debug, Default, PartialEq, Eq)]
struct ProfileScan {
    /// `[profile.release]` sets `overflow-checks = true`.
    release_checked: bool,
    /// Every profile table that sets `overflow-checks = false`.
    overflow_off: Vec<String>,
    /// Every profile line the scan cannot read, as `line N: text`.
    unreadable: Vec<String>,
    /// Each profile table's `opt-level`, as written (quotes taken off), by the
    /// table's name.
    opt_levels: BTreeMap<String, String>,
}

/// The optimisation the hash gate's `profile` (which inherits `release`)
/// promises, from a committed manifest's scan: the release profile's
/// `opt-level`, or cargo's default for a release profile, 3, and never one of
/// its own. An `opt-level` in any of `profile`'s tables (its own, or a package
/// or build override) is refused: the gate hashes the code release ships, so
/// it may not be built at another level.
fn promised_opt_level(scan: &ProfileScan, profile: &str) -> Result<resolved::OptLevel, String> {
    let own = format!("profile.{profile}");
    let set_here: Vec<&String> = scan
        .opt_levels
        .keys()
        .filter(|table| {
            table.as_str() == own
                || table
                    .strip_prefix(own.as_str())
                    .is_some_and(|rest| rest.starts_with('.'))
        })
        .collect();
    if !set_here.is_empty() {
        return Err(format!(
            "Cargo.toml sets an `opt-level` in [{}]; `{profile}` is built at the release \
             profile's optimisation and no other, so the hash gate hashes the code release \
             ships: remove it",
            set_here
                .iter()
                .map(|table| table.as_str())
                .collect::<Vec<_>>()
                .join("], [")
        ));
    }
    match scan.opt_levels.get("profile.release") {
        Some(text) => resolved::OptLevel::parse(text).ok_or_else(|| {
            format!("Cargo.toml gives `release` the opt-level `{text}`, which cargo does not take")
        }),
        None => Ok(resolved::OptLevel::O3),
    }
}

/// Every way the manifest's `[profile.dev.package.<name>]` tables disagree
/// with the package graph, one sentence each: a table that names no package
/// cargo resolves is stale, and a table at opt-level 0 for a package outside
/// the godot family keeps a crate unoptimised the suite runs optimised.
fn dev_package_table_failures(scan: &ProfileScan, graph: &resolved::Graph) -> Vec<String> {
    let family = graph.godot_family_names();
    let mut failures: Vec<String> = Vec::new();
    // The family members a table names, as cargo matches a spec: by name, and
    // by version when the spec gives one.
    let mut tabled: Vec<String> = Vec::new();
    for (table, level) in &scan.opt_levels {
        let Some(spec) = table.strip_prefix("profile.dev.package.") else {
            continue;
        };
        if spec == "*" {
            continue;
        }
        let name = spec.split_once('@').map_or(spec, |(name, _)| name);
        if !graph.has_package(spec) {
            failures.push(format!(
                "Cargo.toml's [{table}] names `{spec}`, which matches no package cargo \
                 resolves: remove the table"
            ));
        } else if graph.spec_in_family(spec) {
            if level == "0" {
                tabled.push(name.to_owned());
            } else {
                failures.push(format!(
                    "Cargo.toml's [{table}] puts `{name}`, of the godot family, at opt-level \
                     {level}; the family serves the walled client alone and stays at 0"
                ));
            }
        } else if level == "0" && name != resolved::SIM {
            failures.push(format!(
                "Cargo.toml's [{table}] keeps `{name}` at opt-level 0, and `{name}` is not of \
                 the godot family ({}), the only third-party crates the suite builds \
                 unoptimised: remove the table",
                family.join(", ")
            ));
        }
    }
    for member in &family {
        if !tabled.contains(member) {
            failures.push(format!(
                "`{member}` is of the godot family, and Cargo.toml has no \
                 `[profile.dev.package.{member}]` with `opt-level = 0`"
            ));
        }
    }
    failures
}

/// The tables in `manifest` and `config`, each named with the file it is in.
fn named_tables(manifest: &[String], config: Option<&[String]>) -> Vec<String> {
    let mut names: Vec<String> = manifest
        .iter()
        .map(|table| format!("Cargo.toml [{table}]"))
        .collect();
    for table in config.into_iter().flatten() {
        names.push(format!(".cargo/config.toml [{table}]"));
    }
    names
}

/// Every way `manifest`, and the cargo config beside it, break the
/// `profiles` rule, one sentence each; empty when they keep it.
fn profile_failures(manifest: &ProfileScan, config: Option<&ProfileScan>) -> Vec<String> {
    let mut failures = Vec::new();
    let overflow_off = named_tables(
        &manifest.overflow_off,
        config.map(|config| config.overflow_off.as_slice()),
    );
    if !overflow_off.is_empty() {
        failures.push(format!(
            "overflow checks are switched off in {}; they stay on in every profile, release \
             included (spec section 15, Maths) — a silent wrap on one platform and a panic on \
             another is the failure this project cannot absorb",
            overflow_off.join(", ")
        ));
    }
    if !manifest.release_checked {
        failures.push(
            "Cargo.toml must set `overflow-checks = true` under `[profile.release]`".to_owned(),
        );
    }
    let mut unreadable: Vec<String> = manifest
        .unreadable
        .iter()
        .map(|line| format!("Cargo.toml {line}"))
        .collect();
    if let Some(config) = config {
        unreadable.extend(
            config
                .unreadable
                .iter()
                .map(|line| format!(".cargo/config.toml {line}")),
        );
    }
    if !unreadable.is_empty() {
        failures.push(format!(
            "the profiles step cannot read {}; it reads `[profile.<name>]`, \
             `[profile.<name>.build-override]` and `[profile.<name>.package.<spec>]` tables of \
             bare `key = value` lines, and fails rather than pass a form it cannot read",
            unreadable.join(", ")
        ));
    }
    failures
}

/// A TOML table header's dotted segments, each trimmed, quotes kept; `None`
/// for a header this does not read (an unclosed quote, an empty segment).
fn header_segments(inner: &str) -> Option<Vec<String>> {
    let mut segments: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for character in inner.chars() {
        match quote {
            Some(open) => {
                current.push(character);
                if character == open {
                    quote = None;
                }
            }
            None if character == '"' || character == '\'' => {
                quote = Some(character);
                current.push(character);
            }
            None if character == '.' => {
                segments.push(current.trim().to_owned());
                current.clear();
            }
            None => current.push(character),
        }
    }
    if quote.is_some() {
        return None;
    }
    segments.push(current.trim().to_owned());
    if segments.iter().any(String::is_empty) {
        return None;
    }
    Some(segments)
}

/// A bare TOML key: letters, digits, `-` and `_`.
fn is_bare_key(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

/// A scalar value the scan reads: a boolean, an integer, or a one-line string
/// with no quote inside it.
fn is_plain_value(text: &str) -> bool {
    let quoted = |mark: char| {
        text.strip_prefix(mark)
            .and_then(|rest| rest.strip_suffix(mark))
            .is_some_and(|inner| !inner.contains(['"', '\'']))
    };
    text == "true"
        || text == "false"
        || (!text.is_empty() && text.chars().all(|character| character.is_ascii_digit()))
        || quoted('"')
        || quoted('\'')
}

/// What a header names, for the scan: a profile table it reads, a profile
/// table it cannot read, or another table.
enum Header {
    Profile(String),
    UnreadableProfile,
    Other,
}

/// Reads a `[...]` header line (the line trimmed, its comment taken off).
fn read_header(line: &str) -> Header {
    let names_profile = |inner: &str| {
        inner
            .trim()
            .trim_start_matches(['"', '\''])
            .starts_with("profile")
    };
    if let Some(inner) = line.strip_prefix("[[") {
        return if names_profile(inner) {
            Header::UnreadableProfile
        } else {
            Header::Other
        };
    }
    let Some(inner) = line
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    else {
        return Header::UnreadableProfile;
    };
    let Some(segments) = header_segments(inner) else {
        return if names_profile(inner) {
            Header::UnreadableProfile
        } else {
            Header::Other
        };
    };
    let Some(first) = segments.first().map(String::as_str) else {
        return Header::Other;
    };
    if first.trim_matches(['"', '\'']) != "profile" {
        return Header::Other;
    }
    let readable = first == "profile"
        && match segments.as_slice() {
            [_, name] => is_bare_key(name),
            [_, name, over] => is_bare_key(name) && over == "build-override",
            [_, name, package, spec] => {
                let spec = spec.trim_matches(['"', '\'']);
                is_bare_key(name) && package == "package" && !spec.is_empty()
            }
            _ => false,
        };
    if readable {
        Header::Profile(
            segments
                .iter()
                .map(|segment| segment.trim_matches(['"', '\'']))
                .collect::<Vec<_>>()
                .join("."),
        )
    } else {
        Header::UnreadableProfile
    }
}

/// Reads the `[profile.*]` tables out of a TOML file's text: whether
/// `[profile.release]` sets `overflow-checks = true`, the tables that set it
/// to `false`, and every profile line in a form this does not read.
///
/// Text scanning rather than a TOML parser, because xtask takes no
/// dependencies; it reads the plain forms the two files use and fails closed
/// on every other form a profile could be written in.
fn scan_profiles(text: &str) -> ProfileScan {
    let mut scan = ProfileScan::default();
    // `None` outside a profile table; `Some(None)` in one this cannot read.
    let mut table: Option<Option<String>> = None;
    let mut in_root = true;
    // Numbered from 1 by a range that cannot overflow: a string holds fewer
    // lines than bytes, and fewer bytes than `usize::MAX`.
    for (number, raw) in (1_usize..).zip(text.lines()) {
        let line = raw.split_once('#').map_or(raw, |(before, _)| before).trim();
        let unreadable = |scan: &mut ProfileScan| {
            scan.unreadable
                .push(format!("line {number}: `{}`", raw.trim()));
        };
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            in_root = false;
            table = match read_header(line) {
                Header::Profile(name) => Some(Some(name)),
                Header::UnreadableProfile => {
                    unreadable(&mut scan);
                    Some(None)
                }
                Header::Other => None,
            };
            continue;
        }
        let key_part = line.split_once('=').map_or(line, |(key, _)| key).trim();
        if in_root
            && key_part
                .trim_start_matches(['"', '\''])
                .starts_with("profile")
        {
            // A dotted key at the root, `profile.dev.opt-level = 0`.
            unreadable(&mut scan);
            continue;
        }
        let Some(name) = &table else {
            continue;
        };
        let Some(name) = name else {
            // The header was already reported; its lines are not read.
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            unreadable(&mut scan);
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if !is_bare_key(key) || !is_plain_value(value) {
            unreadable(&mut scan);
            continue;
        }
        if key == "overflow-checks" && value == "true" && name == "profile.release" {
            scan.release_checked = true;
        }
        if key == "overflow-checks" && value == "false" {
            scan.overflow_off.push(name.clone());
        }
        if key == "opt-level" {
            scan.opt_levels
                .insert(name.clone(), value.trim_matches(['"', '\'']).to_owned());
        }
    }
    scan
}

/// `cargo test --workspace` without `research`, after emptying
/// `<target>/golden/` on an unscoped run.
///
/// The `golden` step compares every committed `expected.*` with the fresh
/// `actual.*` beside it, and the tests run here write those. A warm target
/// directory also holds every `actual.*` an earlier run wrote, including one
/// whose producer has since stopped writing it — a scenario taken off
/// `crates/gamectl/tests/scenarios.rs`'s list, a case renamed — so without the
/// emptying the `golden` step could compare a stale output, pass locally and
/// fail on a clean CI runner (decisions-log item 129 (4), carried by S1's
/// `p1`). Every `actual.*` the step then reads was therefore written by this
/// run. A `--package` run empties nothing, because it runs only some of the
/// producers; it says so instead.
///
/// The tests run as [`run_units`] runs them: each invocation's resolved
/// profile checked before it runs, and the allocation test with the sim
/// unoptimised ([`resolved`]).
fn step_test(ctx: &Ctx) -> Result<Outcome, String> {
    let fresh_note = if ctx.packages.is_empty() {
        let golden = golden_target_dir(&ctx.workspace)?.join("golden");
        empty_fresh_outputs(&golden)?;
        format!(
            "{} emptied first, so every fresh golden output is this run's",
            golden.display()
        )
    } else {
        "a --package run leaves the golden outputs of the packages it does not test as an \
         earlier run wrote them"
            .to_owned()
    };
    let mut base: Vec<String> = vec!["test".to_owned()];
    if ctx.packages.is_empty() {
        base.push("--workspace".to_owned());
    } else {
        for package in &ctx.packages {
            base.push("--package".to_owned());
            base.push(package.clone());
        }
    }
    let workspace = metadata(ctx)?;
    let selected = selected_packages(workspace, &ctx.packages)?;
    let all = workspace_test_units(workspace, &selected)?;
    let sim_selected = selected.iter().any(|name| name == resolved::SIM);
    let (report, checked) = if let Some(shard) = ctx.shard {
        run_shard_units(
            ctx,
            shard,
            "test",
            &all,
            &base,
            sim_selected,
            Some(ALLOCATION_CHECK),
        )?
    } else {
        let checked = run_units(ctx, &base, &all, sim_selected, Some(ALLOCATION_CHECK))?;
        (ShardPart::Ran(format!("{} unit(s)", all.len())), checked)
    };
    let allocation_note = allocation_check_note(checked, sim_selected);
    Ok(Outcome::Done(match report {
        ShardPart::Ran(part) => format!(
            "{part}; tests pass without `research`, each invocation's resolved profile \
             checked first; {allocation_note}; {fresh_note}"
        ),
        ShardPart::Empty(part) => format!("{part}; {allocation_note}; {fresh_note}"),
    }))
}

/// What the `test` step says of [`ALLOCATION_CHECK`]: the check's sentence
/// when this run held the test, else why it did not. Pure, so its words are
/// pinned.
fn allocation_check_note(checked: Option<String>, sim_selected: bool) -> String {
    match checked {
        Some(sentence) => sentence,
        None if !sim_selected => format!(
            "the --package list leaves {} out, so `{}` is not checked in this run",
            resolved::SIM,
            ALLOCATION_CHECK.path
        ),
        None => format!(
            "`{}` is checked by the shard that runs `test:{}`",
            ALLOCATION_CHECK.path, ALLOCATION_CHECK.target
        ),
    }
}

/// The workspace packages a `--package` list selects, by name: every package
/// for an empty list (`--workspace`), else exactly the ones the list names
/// ([`package_spec_name`]); a name no workspace package has is refused.
fn selected_packages(workspace: &Workspace, specs: &[String]) -> Result<Vec<String>, String> {
    if specs.is_empty() {
        return Ok(workspace
            .packages
            .iter()
            .map(|package| package.name.clone())
            .collect());
    }
    let mut names: Vec<String> = Vec::new();
    for spec in specs {
        let name = package_spec_name(spec)?;
        if !workspace
            .packages
            .iter()
            .any(|package| package.name == name)
        {
            return Err(format!(
                "--package `{spec}` names no package of this workspace"
            ));
        }
        names.push(name.to_owned());
    }
    names.sort();
    names.dedup();
    Ok(names)
}

/// Every unit `cargo test` runs over the packages called `selected`, from the
/// metadata.
fn workspace_test_units(
    workspace: &Workspace,
    selected: &[String],
) -> Result<Vec<shard::Unit>, String> {
    shard::units(
        workspace
            .packages
            .iter()
            .filter(|package| selected.contains(&package.name))
            .flat_map(|package| {
                package
                    .targets
                    .iter()
                    .map(|target| (package.name.as_str(), target))
            }),
    )
    .map_err(|error| error.to_string())
}

/// The units of `all` that fall to `shard`, for `step`.
fn shard_part(shard: shard::Shard, step: &str, all: &[shard::Unit]) -> Vec<shard::Unit> {
    all.iter()
        .filter(|unit| shard.holds(step, &unit.key()))
        .cloned()
        .collect()
}

/// Runs `base` (a `cargo test` command line up to its target selection) over
/// `shard`'s part of `all`, through [`run_units`], and says what it ran. A
/// shard that holds none of the units runs nothing and says so; the split's
/// tests prove that some shard holds each.
fn run_shard_units(
    ctx: &Ctx,
    shard: shard::Shard,
    step: &str,
    all: &[shard::Unit],
    base: &[String],
    sim_selected: bool,
    required: Option<RequiredTest>,
) -> Result<(ShardPart, Option<String>), String> {
    let part = shard_part(shard, step, all);
    let keys: Vec<String> = part.iter().map(shard::Unit::key).collect();
    println!(
        "   shard {shard}: {} of {} unit(s): {}",
        part.len(),
        all.len(),
        if keys.is_empty() {
            "none".to_owned()
        } else {
            keys.join(", ")
        }
    );
    let checked = run_units(ctx, base, &part, sim_selected, required)?;
    if part.is_empty() {
        return Ok((
            ShardPart::Empty(format!(
                "none of the step's {} unit(s) falls to shard {shard}, so this shard ran none",
                all.len()
            )),
            checked,
        ));
    }
    Ok((
        ShardPart::Ran(format!(
            "shard {shard} ran {} of the step's {} unit(s)",
            part.len(),
            all.len()
        )),
        checked,
    ))
}

/// Runs `base` (a `cargo test` command line up to its target selection, its
/// first argument `test`) over `units`, one invocation per
/// [`resolved::invocations`] entry (`sim_selected`: whether the selection
/// holds the sim's targets). Each invocation is one [`resolved::TestRun`], and
/// its check and its run are both derived from it: before the tests run, the
/// check builds the same thing with `--no-run --message-format=json` (a
/// doctest invocation through [`resolved::doc_check`]) and
/// [`resolved::faults`] reads the profile cargo resolved for every unit it
/// built; a fault stops the step before a test runs. A flags variable in the
/// environment is refused first.
///
/// `required`, when `units` hold its target and the sim is selected, is held
/// to libtest's own lines in its invocation ([`RequiredTestReader`]): the
/// answer is the check's sentence, and `None` when `units` do not hold it.
fn run_units(
    ctx: &Ctx,
    base: &[String],
    units: &[shard::Unit],
    sim_selected: bool,
    required: Option<RequiredTest>,
) -> Result<Option<String>, String> {
    let required = required.filter(|required| {
        sim_selected && units.contains(&shard::Unit::Test(required.target.to_owned()))
    });
    let runs = resolved::invocations(units, sim_selected);
    if runs.is_empty() {
        return Ok(None);
    }
    let Some(("test", selection)) = base
        .split_first()
        .map(|(first, rest)| (first.as_str(), rest))
    else {
        return Err(format!(
            "a test invocation starts with `test`, and this one is `{}`",
            base.join(" ")
        ));
    };
    refuse_codegen_variables()?;
    let graph = sim_graph(ctx)?;
    let workspace = metadata(ctx)?;
    for invocation in &runs {
        if invocation.sim != resolved::SimRule::Unoptimised {
            continue;
        }
        let ["--test", name] = invocation
            .selectors
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()[..]
        else {
            return Err(format!(
                "an unoptimised invocation selects one test target, and this one selects `{}`",
                invocation.selectors.join(" ")
            ));
        };
        resolved::check_owners(name, &test_target_owners(workspace, name), &graph)
            .map_err(|fault| fault.to_string())?;
    }
    let colour = colour();
    let mut cargo = ProcessCargo::of(ctx);
    let mut checked: Option<String> = None;
    for invocation in runs {
        // The required test's own invocation: the unoptimised one that selects
        // its target alone.
        let watched = required.filter(|required| {
            invocation.sim == resolved::SimRule::Unoptimised
                && invocation.selectors == ["--test", required.target]
        });
        let test = resolved::TestRun::new(selection.to_vec(), invocation, ctx.locked, colour);
        let mut reader = watched.map(RequiredTestReader::new);
        resolved::execute(
            &test,
            &graph,
            &mut cargo,
            &mut print_diagnostic,
            &mut |line| {
                if let Some(reader) = reader.as_mut() {
                    reader.feed(line);
                }
                print_line(line)
            },
        )?;
        if let (Some(required), Some(reader)) = (watched, reader) {
            reader.verdict().map_err(|fault| fault.to_string())?;
            let sentence = format!(
                "libtest reported `{}` (`--test {}`) passed, once, so the allocations notice's \
                 figure is asserted",
                required.path, required.target
            );
            println!("   checked: {sentence}");
            checked = Some(sentence);
        }
    }
    match (required, checked) {
        (Some(required), None) => Err(format!(
            "the units hold `--test {}`, and no invocation ran it alone and unoptimised, so \
             `{}` was not checked",
            required.target, required.path
        )),
        (_, checked) => Ok(checked),
    }
}

/// The workspace packages that own a test target called `name`: the ones
/// `cargo test --workspace --test <name>` selects.
fn test_target_owners(workspace: &Workspace, name: &str) -> Vec<String> {
    workspace
        .packages
        .iter()
        .filter(|package| {
            package
                .targets
                .iter()
                .any(|target| target.name == name && target.kinds.iter().any(|kind| kind == "test"))
        })
        .map(|package| package.name.clone())
        .collect()
}

/// Refuses a variable that changes what rustc compiles behind cargo's report
/// ([`resolved::refused_variables`]), set in the environment, before anything
/// builds: a checked build pins its own, so it would override the variable in
/// silence. On Windows names are compared without regard to case, as Windows
/// reads them.
pub(crate) fn refuse_codegen_variables() -> Result<(), String> {
    let case_insensitive = cfg!(windows);
    let found = resolved::refused_variables(
        env::vars_os().filter_map(|(name, value)| {
            Some((name.into_string().ok()?, value.into_string().ok()?))
        }),
        case_insensitive,
    );
    // A variable whose name or value is not Unicode cannot be read here, but
    // it can be one cargo reads: refuse it too.
    let unreadable: Vec<String> = env::vars_os()
        .filter(|(name, value)| name.to_str().is_none() || value.to_str().is_none())
        .filter_map(|(name, _)| {
            let name = name.to_string_lossy().into_owned();
            resolved::is_refused_variable(&name, case_insensitive).then_some(name)
        })
        .collect();
    let all: Vec<String> = found.into_iter().chain(unreadable).collect();
    resolved::refuse_variables(&all).map_err(|fault| fault.to_string())
}

/// The channel `rust-toolchain.toml` pins, read strictly
/// ([`resolved::pinned_channel_in`]) from the file this xtask was built from.
fn pinned_channel() -> Result<String, String> {
    resolved::pinned_channel_in(include_str!("../../rust-toolchain.toml"))
}

/// Whether a checked build's diagnostics carry colour: when standard error is
/// a terminal, or `CARGO_TERM_COLOR=always` asks for it (CI's logs render it).
pub(crate) fn colour() -> bool {
    use std::io::IsTerminal;
    env::var("CARGO_TERM_COLOR").is_ok_and(|value| value == "always")
        || std::io::stderr().is_terminal()
}

/// Writes one diagnostic of a checked build to standard error, as cargo
/// would have.
pub(crate) fn print_diagnostic(rendered: &str) {
    eprint!("{rendered}");
}

/// Writes one line of a checked run's test output to standard output, as the
/// test printed it.
/// Writes one line of a test's output, as the bytes the test wrote.
pub(crate) fn print_line(line: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    out.write_all(line)
        .and_then(|()| out.write_all(b"\n"))
        .map_err(|error| format!("writing a test's output: {error}"))
}

/// The package graph [`resolved::faults`] judges by, from `cargo metadata`
/// with its dependencies (the cached metadata has none).
pub(crate) fn sim_graph(ctx: &Ctx) -> Result<resolved::Graph, String> {
    graph_with(&mut ProcessCargo::of(ctx), ctx.locked)
}

/// [`sim_graph`] through `cargo`: `cargo metadata` as a checked call (the
/// pinned tools and environment), because cargo asks rustc about the target
/// here, and a config file's compiler or wrapper would otherwise run for it.
pub(crate) fn graph_with(
    cargo: &mut ProcessCargo,
    locked: bool,
) -> Result<resolved::Graph, String> {
    let mut args: Vec<String> = ["metadata", "--format-version", "1"]
        .map(str::to_owned)
        .to_vec();
    if locked {
        args.push("--locked".to_owned());
    }
    let call = resolved::CargoCall::pinned(args, "");
    let mut text = String::new();
    let success = resolved::Cargo::report(cargo, &call.command_line(), &call.env, &mut |line| {
        text.push_str(line);
        text.push('\n');
    })?;
    if !success {
        return Err(format!("`cargo {}` failed", call.command_line().join(" ")));
    }
    resolved::graph(&text).map_err(|fault| fault.to_string())
}

/// The real [`resolved::Cargo`]: a cargo, in the workspace root, with
/// exactly the command line and environment it is handed, behind the process
/// door ([`resolved::Host::door_line`]): the host's default linker pinned on
/// every call, and on a test run the host's runner pinned to this xtask.
pub(crate) struct ProcessCargo {
    cargo: String,
    root: PathBuf,
    require_tools: bool,
    /// This xtask as the runner's `--config` names it ([`runner_spelling`]),
    /// settled by `prepare_run` before anything builds.
    runner: Option<String>,
}

/// The cargo xtask shells out to: the one that started it (`CARGO`), or the
/// one on `PATH` when xtask was started some other way. A `CARGO` that is
/// not Unicode is refused: read as absent, it would run another cargo.
fn cargo_program_from(value: Option<std::ffi::OsString>) -> Result<String, String> {
    match value {
        None => Ok("cargo".to_owned()),
        Some(value) => value
            .into_string()
            .map_err(|_| "`CARGO` holds a value that is not Unicode".to_owned()),
    }
}

/// [`cargo_program_from`] of this process's `CARGO`.
pub(crate) fn cargo_program() -> Result<String, String> {
    cargo_program_from(env::var_os("CARGO"))
}

/// The host cargo builds for, read once from `rustc -vV` (the `rustc` the
/// pinned `build.rustc` names) and kept for the process.
pub(crate) fn host() -> Result<&'static resolved::Host, String> {
    static HOST: std::sync::OnceLock<Result<resolved::Host, String>> = std::sync::OnceLock::new();
    HOST.get_or_init(|| resolved::Host::from_rustc(rustc_verbose()?))
        .as_ref()
        .map_err(Clone::clone)
}

/// `rustc -vV` of the `rustc` the checked calls name, run once in the root, as
/// cargo runs (the rustup proxy picks the toolchain by the directory it starts
/// in), and kept for the process.
fn rustc_verbose() -> Result<&'static str, String> {
    static VERBOSE: std::sync::OnceLock<Result<String, String>> = std::sync::OnceLock::new();
    VERBOSE
        .get_or_init(|| {
            let output = child_command(false, "rustc", &["-vV"], &workspace_root()?)
                .stderr(Stdio::inherit())
                .output()
                .map_err(|error| format!("failed to run `rustc -vV`: {error}"))?;
            if !output.status.success() {
                return Err(format!(
                    "`rustc -vV` failed ({})",
                    describe_exit(output.status)
                ));
            }
            String::from_utf8(output.stdout).map_err(|error| {
                format!("`rustc -vV` printed something that is not UTF-8: {error}")
            })
        })
        .as_deref()
        .map_err(Clone::clone)
}

/// The door every command passes before it runs anything: the compiler the
/// checked calls will use and the cargo they run both report the release
/// `rust-toolchain.toml` pins ([`resolved::check_toolchain`]).
fn toolchain_at_the_door() -> Result<(), String> {
    resolved::check_toolchain(rustc_verbose()?, &cargo_verbose()?, &pinned_channel()?)
}

/// `$CARGO -vV` ([`cargo_program`], the cargo every checked call runs), in
/// the root.
fn cargo_verbose() -> Result<String, String> {
    let cargo = cargo_program()?;
    let output = child_command(false, &cargo, &["-vV"], &workspace_root()?)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("failed to run `{cargo} -vV`: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "`{cargo} -vV` failed ({})",
            describe_exit(output.status)
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("`{cargo} -vV` printed something that is not UTF-8: {error}"))
}

/// The `--config` value naming this xtask, `xtask`, as the runner for a cargo
/// working in `cwd`: [`runner_exists`], [`runner_spelling`] and the host's
/// runner key ([`resolved::Host::runner_config`]), the one rule for both the
/// checked calls and `xtask/tests/doctest_runner.rs`, which asks for it
/// through `xtask __runner-config <cwd>`.
pub(crate) fn runner_config_for(xtask: &Path, cwd: &Path) -> Result<String, String> {
    runner_exists(xtask)?;
    host()?.runner_config(&runner_spelling(xtask, cwd)?)
}

/// `args`, a cargo command xtask runs outside the checked calls (clippy),
/// with the host's default linker pinned as the checked calls pin it, so the
/// units the two share (build scripts, proc-macros) are not rebuilt between
/// them, and a config file's linker runs in neither.
pub(crate) fn with_pinned_linker(args: &[String]) -> Result<Vec<String>, String> {
    host()?.door_line(args, None)
}

impl ProcessCargo {
    pub(crate) fn of(ctx: &Ctx) -> ProcessCargo {
        ProcessCargo {
            cargo: ctx.cargo.clone(),
            root: ctx.root.clone(),
            require_tools: ctx.require_tools,
            runner: None,
        }
    }

    /// For the commands that run outside `ci`, with no `Ctx`.
    pub(crate) fn new(root: &Path, require_tools: bool) -> Result<ProcessCargo, String> {
        Ok(ProcessCargo {
            cargo: cargo_program()?,
            root: root.to_path_buf(),
            require_tools,
            runner: None,
        })
    }

    /// The command line, with its environment changes before it, as printed.
    fn render(&self, line: &[String], env: &[resolved::EnvChange]) -> String {
        let mut prefix = String::new();
        for (key, value) in env {
            if let Some(value) = value {
                prefix.push_str(key);
                prefix.push_str("=\"");
                prefix.push_str(value);
                prefix.push_str("\" ");
            } else {
                prefix.push_str("-u ");
                prefix.push_str(key);
                prefix.push(' ');
            }
        }
        format!("{prefix}{}", render_command(&self.cargo, line))
    }

    fn command(&self, line: &[String], env: &[resolved::EnvChange]) -> Command {
        let mut command = child_command(self.require_tools, &self.cargo, line, &self.root);
        for (key, value) in env {
            match value {
                Some(value) => command.env(key, value),
                None => command.env_remove(key),
            };
        }
        command
    }

    /// Hands `each` every line of `reader` as it arrives, as text: cargo's
    /// JSON report, read strictly.
    fn lines(
        reader: impl std::io::Read,
        each: &mut dyn FnMut(&str),
        what: &str,
    ) -> Result<(), String> {
        use std::io::BufRead;
        for line in std::io::BufReader::new(reader).lines() {
            let line = line.map_err(|error| format!("reading {what}: {error}"))?;
            each(&line);
        }
        Ok(())
    }

    /// Hands `each` every line of `reader` as it arrives, as bytes, without
    /// its `\n`: a test's output, whatever it holds.
    fn byte_lines(
        reader: impl std::io::Read,
        each: &mut dyn FnMut(&[u8]),
        what: &str,
    ) -> Result<(), String> {
        use std::io::BufRead;
        for line in std::io::BufReader::new(reader).split(b'\n') {
            let line = line.map_err(|error| format!("reading {what}: {error}"))?;
            each(&line);
        }
        Ok(())
    }

    /// Starts cargo with `line` and `env`, standard error inherited and
    /// standard output handed to `read` as it arrives, so neither pipe can
    /// fill and stall the child. Answers whether it succeeded.
    fn spawn(
        &self,
        line: &[String],
        env: &[resolved::EnvChange],
        read: &mut dyn FnMut(std::process::ChildStdout) -> Result<(), String>,
    ) -> Result<bool, String> {
        println!("   $ {}", self.render(line, env));
        let mut child = self
            .command(line, env)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("failed to launch `{}`: {error}", self.cargo))?;
        let result = match child.stdout.take() {
            Some(out) => read(out),
            None => Err("cargo's standard output was not captured".to_owned()),
        };
        let status = child
            .wait()
            .map_err(|error| format!("waiting for `{}`: {error}", self.cargo))?;
        result?;
        Ok(status.success())
    }
}

impl resolved::Cargo for ProcessCargo {
    fn report(
        &mut self,
        line: &[String],
        env: &[resolved::EnvChange],
        stdout: &mut dyn FnMut(&str),
    ) -> Result<bool, String> {
        let line = host()?.door_line(line, None)?;
        self.spawn(&line, env, &mut |out| {
            ProcessCargo::lines(out, stdout, "cargo's report")
        })
    }

    fn run(
        &mut self,
        line: &[String],
        env: &[resolved::EnvChange],
        target_dir: &str,
        stdout: &mut dyn FnMut(&[u8]),
    ) -> Result<resolved::RunOutcome, String> {
        if self.runner.is_none() {
            self.prepare_run()?;
        }
        let runner = self
            .runner
            .clone()
            .ok_or_else(|| "the runner's spelling was not settled".to_owned())?;
        let line = host()?.door_line(line, Some(&runner))?;
        let (success, started) = runner::with_record_folder(Path::new(target_dir), |folder| {
            let record = folder
                .to_str()
                .ok_or_else(|| format!("the record folder {} is not Unicode", folder.display()))?;
            let mut env = env.to_vec();
            env.push((runner::RECORD_VAR.to_owned(), Some(record.to_owned())));
            self.spawn(&line, &env, &mut |out| {
                ProcessCargo::byte_lines(out, stdout, "cargo's standard output")
            })
        })?;
        Ok(resolved::RunOutcome { success, started })
    }

    fn prepare_run(&mut self) -> Result<(), String> {
        let xtask = env::current_exe()
            .map_err(|error| format!("finding xtask's own executable: {error}"))?;
        runner_exists(&xtask)?;
        self.runner = Some(runner_spelling(&xtask, &self.root)?);
        Ok(())
    }
}

/// Whether this xtask's executable, `xtask`, is still there to be named as
/// the runner.
fn runner_exists(xtask: &Path) -> Result<(), String> {
    if xtask.is_file() {
        return Ok(());
    }
    Err(format!(
        "xtask's own executable {} is no longer there: a build replaced it while it ran, \
         which happens when a checked build and the `cargo xtask` alias build it with \
         different settings",
        xtask.display()
    ))
}

/// This xtask, `xtask`, as the runner's `--config` names it to cargo, which
/// splits a runner on whitespace and resolves a relative one against its own
/// working directory, `root` (measured with cargo 1.98.1: a workspace in a
/// folder whose name holds a space, the runner named `bin/rr.bat`, and the
/// test started in its package's own folder, ran through it). Relative,
/// with `/`, when it lies under `root`, as an in-tree `target` does; else
/// absolute. The spelling must hold no whitespace and no quote (a TOML
/// literal string cannot hold one), and when neither spelling is free of
/// them the error says what to move.
fn runner_spelling(xtask: &Path, root: &Path) -> Result<String, String> {
    let clean = |text: &str| {
        !text.contains(|character: char| character == '\'' || character.is_whitespace())
    };
    let relative = xtask.strip_prefix(root).ok().and_then(|rest| {
        let parts: Option<Vec<&str>> = rest
            .components()
            .map(|component| match component {
                std::path::Component::Normal(part) => part.to_str(),
                _ => None,
            })
            .collect();
        parts.map(|parts| parts.join("/"))
    });
    if let Some(relative) = relative.filter(|text| !text.is_empty() && clean(text)) {
        return Ok(relative);
    }
    let absolute = xtask.to_str().ok_or_else(|| {
        format!(
            "xtask's own executable {} is not at a Unicode path",
            xtask.display()
        )
    })?;
    if clean(absolute) {
        return Ok(absolute.to_owned());
    }
    Err(format!(
        "xtask's own executable is {absolute}, and cargo splits a runner on whitespace: it \
         cannot be named to cargo there. Move the target directory (CARGO_TARGET_DIR) to a \
         path without whitespace or quotes, or inside the checkout {} if that path has none",
        root.display()
    ))
}

/// What a shard did with its part of a step: ran it, or held none of it, in
/// which case its report says so rather than "tests pass".
#[derive(Debug)]
enum ShardPart {
    Ran(String),
    Empty(String),
}

/// Remove `<target>/golden/` and everything under it, if it is there. Its
/// absence is the state this wants, so a missing folder is not an error.
fn empty_fresh_outputs(golden: &Path) -> Result<(), String> {
    match fs::remove_dir_all(golden) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "could not empty {} before the tests write fresh golden outputs: {error}",
            golden.display()
        )),
    }
}

fn step_test_research(ctx: &Ctx) -> Result<Outcome, String> {
    // Metadata that cannot be read is a failure of its own, never "no crate
    // declares the feature".
    let sim = metadata(ctx)?.package_with_feature(SIM_PACKAGES, RESEARCH_FEATURE);
    // `--locked` is added by `run_units`, after the selectors.
    let (args, feature) = match research_test_args(sim.as_deref(), &ctx.packages, false)? {
        ResearchTests::Run { args, feature } => (args, feature),
        ResearchTests::Filtered(reason) => {
            if ctx.shard.is_some() {
                return Err(
                    "a sharded run takes no --package filter, so the research tests cannot be \
                     filtered out"
                        .to_owned(),
                );
            }
            return Ok(Outcome::Skipped(reason));
        }
    };
    let workspace = metadata(ctx)?;
    let all = shard::units(
        workspace
            .packages
            .iter()
            .filter(|package| sim.as_deref() == Some(package.name.as_str()))
            .flat_map(|package| {
                package
                    .targets
                    .iter()
                    .map(|target| (package.name.as_str(), target))
            }),
    )
    .map_err(|error| error.to_string())?;
    if let Some(shard) = ctx.shard {
        return Ok(Outcome::Done(
            match run_shard_units(ctx, shard, "test-research", &all, &args, true, None)?.0 {
                ShardPart::Ran(part) => format!(
                    "{part}; tests pass with {feature}, each invocation's resolved profile \
                     checked first"
                ),
                ShardPart::Empty(part) => part,
            },
        ));
    }
    run_units(ctx, &args, &all, true, None)?;
    Ok(Outcome::Done(format!(
        "tests pass with {feature}, each invocation's resolved profile checked first; no other \
         crate's code reads the feature, and clippy's research pass compiles every \
         non-walled crate against it"
    )))
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
/// A `-p` value is read by [`package_spec_name`]: a name or `name@version`,
/// and a glob or a package-id URL refused, so whether the list names the sim
/// is exact.
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
    let mut names_sim = false;
    for spec in packages {
        names_sim |= package_spec_name(spec)? == sim;
    }
    if !packages.is_empty() && !names_sim {
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

/// The package a `--package` value names: the name itself, or the name of
/// `name@version`. Cargo also takes a glob (`*`, `?`, `[`) or a package-id URL
/// (`#`, `:`, `/`, `\\`) there; the test steps refuse both, because which
/// packages one selects is cargo's resolution to make, and a step that judged
/// it by guessing would run, or skip, the sim's checks on a guess.
fn package_spec_name(spec: &str) -> Result<&str, String> {
    let name = spec.split_once('@').map_or(spec, |(before, _)| before);
    if spec.contains(['*', '?', '[', '#', ':', '/', '\\']) || name.is_empty() {
        return Err(format!(
            "--package `{spec}` is not a package's name or `name@version`; the test steps \
             take those alone, so which packages they test, and whether the sim is among \
             them, is exact"
        ));
    }
    Ok(name)
}

/// `fork` lives behind the `research` feature, and release builds never enable
/// it. This proves the crates of [`GUARDED_PACKAGES`] — plan-core, the
/// verifier, the operator, the gateway, `gamectl` and the walled bench —
/// cannot reach it: once with their default features (the shipped configuration), once with
/// every feature of their own turned on (so no feature of theirs can forward to
/// it later). `--all-features` here is a `cargo tree` query, not a build — the
/// AGENTS.md ban on `--all-features` is about compiling, where it would enable
/// `research` everywhere and hide this very check.
fn step_research_guard(ctx: &Ctx) -> Result<Outcome, String> {
    let workspace = metadata(ctx)?;

    let present = workspace.present(GUARDED_PACKAGES);
    if present.is_empty() {
        return Err(
            "none of plan-core, verifier, operator, gateway, gamectl, bench exist, so there is \
             nothing to guard; the workspace lost its crates or the metadata is wrong"
                .to_owned(),
        );
    }

    for package in &present {
        for all_features in [false, true] {
            let args = research_tree_args(package, all_features, ctx.locked);
            let tree = capture(ctx, &ctx.cargo, &args)?;
            judge_research_tree(package, all_features, &tree)?;
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
///    the research guard's crates less the walled bench, and the sim itself.
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
    capture(
        ctx,
        &ctx.cargo,
        &dependency_listing_args(package, ctx.locked),
    )
}

/// [`dependency_listing`]'s command line, its shape pinned: `--color never`
/// (CI sets `CARGO_TERM_COLOR=always`, and the flag beats it) and `--prefix
/// none`, so each line is one package name and nothing else.
fn dependency_listing_args(package: &str, locked: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "tree",
        "--color",
        "never",
        "--package",
        package,
        "--all-features",
        "--prefix",
        "none",
        "--format",
        "{p}",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    if locked {
        args.push("--locked".to_owned());
    }
    args
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
    if !cargo_subcommand_available(ctx, "deny") {
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
    if !tool_available(ctx, "buf") {
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

    // PLACEHOLDER: a second comparison, against the last release tag — owner, v1.1.
    // Once v1.1 publishes gp.api.v1, `buf breaking` also runs against
    // `.git#tag=vX.Y.Z` in WIRE_JSON mode: the branch comparison catches churn,
    // the tag comparison a released-schema break (AGENTS.md section 9, item 8).
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
    let target_dir = golden_target_dir(&ctx.workspace)?;
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

    let binary = checked_determinism_binary(ctx, &owner)?;
    run(
        ctx,
        &binary,
        &[
            "--ticks".to_owned(),
            DETERMINISM_TICKS.to_owned(),
            "--out".to_owned(),
            hash_path.to_string_lossy().into_owned(),
        ],
    )?;

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
        ChainVerdict::Moved(golden) => Err(format!(
            "the hash chain moved: {} differs from {}\n{}\n      A moved hash chain is a \
             behaviour change. Explain it in the pull request; never re-bless it to get a \
             red build to green.",
            HASH_FILE,
            HASH_GOLDEN,
            first_difference(golden, &fresh)
        )),
    }
}

/// The cross-OS hash gate's build of the `determinism` binary, made through
/// the checked call and held to the whole of what `release-checked` promises
/// ([`resolved::Expect::release_checked`]): `[profile.release]`'s
/// optimisation, as the committed manifest declares it, in every workspace
/// unit and every crate the sim runs, overflow checks and debug assertions
/// on, the sim's library built, and its tools and flags pinned. A manifest
/// that gives `release-checked` an `opt-level` of its own is refused: the
/// gate hashes the code release ships. Returns the binary's path as the build
/// reported it, so what is checked is what runs. The `determinism` step and
/// `cargo xtask determinism-run` (G4's nightly matches) both build it here.
fn checked_determinism_binary(ctx: &Ctx, owner: &str) -> Result<String, String> {
    refuse_codegen_variables()?;
    let manifest_path = ctx.root.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let promised = promised_opt_level(&scan_profiles(&manifest), DETERMINISM_PROFILE)?;
    let graph = sim_graph(ctx)?;
    let mut args: Vec<String> = vec![
        "build".to_owned(),
        "--profile".to_owned(),
        DETERMINISM_PROFILE.to_owned(),
        "--package".to_owned(),
        owner.to_owned(),
        "--bin".to_owned(),
        DETERMINISM_BIN.to_owned(),
        resolved::message_format(colour()).to_owned(),
    ];
    if ctx.locked {
        args.push("--locked".to_owned());
    }
    let built = resolved::checked(
        &mut ProcessCargo::of(ctx),
        &resolved::CargoCall::pinned(args, ""),
        &graph,
        resolved::Expect::release_checked(promised),
        &mut print_diagnostic,
    )?;
    let binary = resolved::reported_artefact(
        &built,
        &graph,
        owner,
        resolved::Wanted::Bin(DETERMINISM_BIN),
    )?;
    binary.into_os_string().into_string().map_err(|path| {
        format!(
            "the `{DETERMINISM_BIN}` binary's path {} is not Unicode",
            path.display()
        )
    })
}

/// `cargo xtask determinism-run --seed <0x...> --ticks <n> --out <file>`:
/// builds the `determinism` binary as the `determinism` step does
/// ([`checked_determinism_binary`]) and plays one match with it. G4's
/// nightly job plays its ten seeds through this, so its chains come from the
/// checked build.
fn determinism_run(ctx: &Ctx, seed: &str, ticks: &str, out: &str) -> Result<(), String> {
    let workspace = metadata(ctx)?;
    let Some(owner) = workspace.package_with_bin(DETERMINISM_BIN) else {
        return Err(format!(
            "no `{DETERMINISM_BIN}` binary in the workspace; `pharmakos-sim` builds it"
        ));
    };
    let binary = checked_determinism_binary(ctx, &owner)?;
    run(
        ctx,
        &binary,
        &[
            "--ticks".to_owned(),
            ticks.to_owned(),
            "--seed".to_owned(),
            seed.to_owned(),
            "--out".to_owned(),
            out.to_owned(),
        ],
    )
}

/// What the `determinism` step makes of the fresh chain.
#[derive(Debug, PartialEq, Eq)]
enum ChainVerdict<'golden> {
    /// The committed chain equals the fresh one.
    Matches,
    /// `--bless`: write the fresh chain as the committed one.
    Bless,
    /// No committed chain, and no `--bless`: a failure.
    Missing,
    /// The committed chain differs, and no `--bless`: a failure. It carries
    /// the committed chain, so the report has it to compare.
    Moved(&'golden [u8]),
}

/// The `determinism` step's decision, pure so that "a missing committed chain
/// fails" is unit-tested rather than asserted in prose.
fn chain_verdict<'golden>(
    golden: Option<&'golden [u8]>,
    fresh: &[u8],
    bless: bool,
) -> ChainVerdict<'golden> {
    match golden {
        Some(golden) if golden == fresh => ChainVerdict::Matches,
        _ if bless => ChainVerdict::Bless,
        Some(golden) => ChainVerdict::Moved(golden),
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
    if !tool_available(ctx, "reuse") {
        return skip_or_fail(
            ctx,
            "the reuse tool is not installed (`pipx install reuse`)",
        );
    }
    run(ctx, "reuse", &["lint".to_owned()])?;
    // The manifest is complete; now a file that reproduces the template library's text has to
    // carry the library's licence beside the game's (`licence_quotes.rs`). It needs `reuse`,
    // so it runs here, where the tool is guaranteed, and not in a unit test.
    let quotes = licence_quotes::check_tree(&ctx.root, ctx.require_tools)?;
    Ok(Outcome::Done(format!(
        "every file carries an SPDX header and the manifest is complete; {quotes}"
    )))
}

/// The PLACEHOLDER register's grammar, as a step (S1's plan, decision 9; the
/// register's S1-10): every marker in the tracked tree is written in
/// [`placeholders::GRAMMAR`] on one line, so the register can be collected by
/// `cargo xtask placeholders` rather than by a person. Required: a tree that
/// `git grep` cannot read fails, and so does any marker off the grammar, each
/// one named. `cargo xtask placeholders` prints the whole register.
fn step_placeholders(ctx: &Ctx) -> Result<Outcome, String> {
    let register = placeholders::collect(&ctx.root, ctx.require_tools)?;
    if !register.off_grammar.is_empty() {
        let mut message = format!(
            "{} marker(s) are off the grammar; reword each to `{}` on one line:",
            register.off_grammar.len(),
            placeholders::GRAMMAR
        );
        for (place, text) in &register.off_grammar {
            message.push_str("\n      ");
            message.push_str(place);
            message.push_str("  ");
            message.push_str(text);
        }
        return Err(message);
    }
    Ok(Outcome::Done(format!(
        "{} marker(s) in {} file(s), every one in the grammar",
        register.in_grammar(),
        register.files
    )))
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

    // The runner, built through the checked call and run from the path its
    // build reported: the chains it compares come from a checked build.
    let gamectl = checked_gamectl(ctx, &owner, false)?;
    let probe = [SCENARIO_SUBCOMMAND.to_owned(), "--help".to_owned()];
    if !command_succeeds(ctx, &gamectl, &probe) {
        return Err(format!(
            "{checked}; `{SCENARIO_BIN} {SCENARIO_SUBCOMMAND} --help` did not succeed, so the \
             runner is missing or does not build, and none of them was played"
        ));
    }

    // The nightly arms are validated above and never played here: they carry
    // no chain, and `.github/workflows/nightly-scenarios.yml` plays them over
    // the seed set (S2's plan, E20). `collect_played` is the list this step
    // and `cargo xtask package`'s replay both play.
    let played = scenario::collect_played(&ctx.root)?;
    let arms = valid.iter().filter(|item| item.nightly).count();
    let to_play = valid.iter().filter(|item| !item.nightly).count();
    // On a shard, every file is validated above and only the shard's part is
    // played, keyed by its relative path (`scenario_shard_key`).
    let mine = |item: &scenario::Scenario| scenario_on_shard(ctx.shard, &item.relative);
    for item in valid.iter().filter(|item| !item.nightly) {
        if !played.iter().any(|path| path.ends_with(&item.relative)) {
            return Err(format!(
                "{checked}; {} is validated and not in the played list, which is the step and \
                 `collect_played` disagreeing",
                item.relative.display()
            ));
        }
        if !mine(item) {
            continue;
        }
        // Forward slashes: the path goes on a command line that is quoted in
        // workflow files and in scenario logs on three operating systems.
        let relative = scenario_shard_key(&item.relative);
        let args = [
            SCENARIO_SUBCOMMAND.to_owned(),
            "run".to_owned(),
            relative.clone(),
        ];
        run(ctx, &gamectl, &args)
            .map_err(|error| format!("scenario `{}` failed: {error}", item.name))?;
    }
    let ran = valid
        .iter()
        .filter(|item| !item.nightly && mine(item))
        .count();

    let part = match ctx.shard {
        Some(shard) if ran == 0 => {
            return Ok(Outcome::Done(format!(
                "{checked}; none of the {to_play} played scenario(s) falls to shard {shard}, so \
                 this shard played none; {arms} nightly arm(s) valid"
            )));
        }
        Some(shard) => format!("shard {shard} played {ran} of {to_play}; "),
        None => String::new(),
    };
    Ok(Outcome::Done(format!(
        "{part}{ran} scenario(s) pass on their event and hash assertions; {arms} nightly arm(s) \
         valid, played by the nightly workflow and not here"
    )))
}

/// Whether the scenario at `relative` is played on `shard` (every one
/// without `--shard`): the scenario step's part of the split, keyed by
/// [`scenario_shard_key`].
fn scenario_on_shard(shard: Option<shard::Shard>, relative: &Path) -> bool {
    shard.is_none_or(|shard| shard.holds("scenario", &scenario_shard_key(relative)))
}

/// A scenario's relative path with forward slashes: its name on a command
/// line, and its key in the split ([`shard::Shard::holds`]), the same on every
/// operating system.
fn scenario_shard_key(relative: &Path) -> String {
    relative.to_string_lossy().replace('\\', "/")
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
    if !tool_available(ctx, &godot) {
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
        diff.describe()?
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
/// Both Godot runs, the import and the check, use the step's own user folders,
/// `<target>/client-check/data/`, emptied at the start of every run
/// ([`client_env`]): never the machine's, whose `user://` is the owner's real
/// game folder (decisions-log item 129 (4)).
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
    if !tool_available(ctx, &godot) {
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

    // Godot's user folders for both runs below, emptied first: never the
    // machine's own (decisions-log item 129 (4)).
    let data = client_data_dir(&workspace.target_dir);
    let environment = client_env(&data);
    reset_client_data(&workspace.target_dir, &data, &environment)?;

    // The pre-step every fresh checkout needs; see this function's doc comment.
    let import_note = import_project_in(ctx, &godot, &project, &environment)?;

    if !ctx.client_check {
        return Ok(Outcome::Done(format!(
            "{library} staged ({staged_bytes} bytes, carries `{ENTRY_SYMBOL}`); {import_note}, \
             with Godot's user folders under {}; `--check` also runs the headless client check",
            data.display()
        )));
    }

    run_with_env(
        ctx,
        &godot,
        &[
            "--headless".to_owned(),
            "--path".to_owned(),
            GODOT_PROJECT_DIR.to_owned(),
            CLIENT_CHECK_SCENE.to_owned(),
        ],
        &environment,
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
         headless client check passed with no caught panics, with Godot's user folders under {}",
        data.display()
    )))
}

/// The client check's own user folders: `<target>/client-check/data/`.
fn client_data_dir(target_dir: &Path) -> PathBuf {
    target_dir.join("client-check").join("data")
}

/// The environment the client check's Godot runs under: every folder Godot
/// reads its user folders from — `user://` (the project's log, which Godot
/// rotates, `last_match.txt`, the checks' own `.jsonc` outputs), its editor
/// settings and its shader caches — moved under `data`.
///
/// Before S1's `p1`, the import and the check ran with the machine's own
/// folders, so every local `cargo xtask ci` wrote into the owner's real game
/// folder: in S1's wave 1 a check run rotated away the demo's log there and
/// left four check outputs beside it (decisions-log item 129 (4)).
/// `cargo xtask package` already gave its smoke run folders of its own the
/// same way (`package::smoke_env`, decisions-log item 117 (7)); this is the
/// same list, by platform.
fn client_env(data: &Path) -> Vec<(String, String)> {
    let at = |name: &str| data.join(name).to_string_lossy().into_owned();
    if cfg!(target_os = "windows") {
        vec![
            ("APPDATA".to_owned(), at("appdata")),
            ("LOCALAPPDATA".to_owned(), at("localappdata")),
        ]
    } else {
        vec![
            ("HOME".to_owned(), at("home")),
            ("XDG_CACHE_HOME".to_owned(), at("cache")),
            ("XDG_CONFIG_HOME".to_owned(), at("config")),
            ("XDG_DATA_HOME".to_owned(), at("data")),
        ]
    }
}

/// Empty `data` and create each folder of `environment` under it, so every
/// run starts from nothing and leaves only its own outputs. Refuses a `data`
/// that is not strictly inside `target_dir`, because it is about to be
/// deleted.
fn reset_client_data(
    target_dir: &Path,
    data: &Path,
    environment: &[(String, String)],
) -> Result<(), String> {
    if !data.starts_with(target_dir) || data == target_dir {
        return Err(format!(
            "the client check's user folders {} are not inside the target directory {}; \
             refusing to empty them",
            data.display(),
            target_dir.display()
        ));
    }
    match fs::remove_dir_all(data) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("emptying {}: {error}", data.display())),
    }
    for (_, folder) in environment {
        fs::create_dir_all(folder).map_err(|error| format!("creating {folder}: {error}"))?;
    }
    Ok(())
}

/// [`run`], with `environment` set for the child on top of what
/// [`child_command`] sets, and printed in front of the command.
fn run_with_env(
    ctx: &Ctx,
    program: &str,
    args: &[String],
    environment: &[(String, String)],
) -> Result<(), String> {
    let prefix: Vec<String> = environment
        .iter()
        .map(|(key, value)| format!("{key}=\"{value}\" "))
        .collect();
    println!("   $ {}{}", prefix.concat(), render_command(program, args));
    let mut command = child_command(ctx.require_tools, program, args, &ctx.root);
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

    // Built through the checked call, in the dev profile the suite builds
    // with, and staged from the path the build reported
    // ([`resolved::reported_artefact`]): what the client check loads is what
    // cargo's report of that build names, never a path composed here. The dev profile deliberately: the guard is real there
    // and the inner loop fast. `[profile.release]` sets `panic = "abort"`,
    // under which `catch_unwind` catches nothing, so the bridge's caught-panic
    // counter would be dead. The packaged library is built with
    // `[profile.release-client]` instead, which inherits `release` with
    // `panic = "unwind"` (decisions-log item 102 (1), named by item 117 (4));
    // `cargo xtask package` builds it, and its smoke check asserts the counter
    // at zero in the packaged build. CI's `client extension` job keeps the
    // debug library, as this does.
    refuse_codegen_variables()?;
    let graph = sim_graph(ctx)?;
    let mut args: Vec<String> = vec![
        "build".to_owned(),
        "--package".to_owned(),
        CLIENT_PACKAGE.to_owned(),
        "--lib".to_owned(),
        resolved::message_format(colour()).to_owned(),
    ];
    if ctx.locked {
        args.push("--locked".to_owned());
    }
    let library = client_library_name();
    let units = resolved::checked(
        &mut ProcessCargo::of(ctx),
        &resolved::CargoCall::pinned(args, ""),
        &graph,
        resolved::Expect::suite(resolved::SimRule::OptimisedIfBuilt),
        &mut print_diagnostic,
    )?;
    let built = resolved::reported_artefact(
        &units,
        &graph,
        CLIENT_PACKAGE,
        resolved::Wanted::Cdylib(&library),
    )?;

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
    import_project_in(ctx, godot, project, &[])
}

/// [`import_project`], with `environment` set for Godot: `stage-client` passes
/// its own user folders ([`client_env`]); the screenshot step and the package
/// pass none, so their Godot runs still use the machine's own user folders (a
/// Linux developer's real `user://` for the screenshot step, the package's
/// import on any host). Item 129 (4) asked this for the client check only.
fn import_project_in(
    ctx: &Ctx,
    godot: &str,
    project: &Path,
    environment: &[(String, String)],
) -> Result<String, String> {
    let import_args: Vec<String> = vec![
        "--headless".to_owned(),
        "--path".to_owned(),
        project.to_string_lossy().replace('\\', "/"),
        "--import".to_owned(),
    ];
    let first = run_with_env(ctx, godot, &import_args, environment);
    let mut note = "project imported".to_owned();
    if let Err(error) = first {
        println!(
            "   note: the import exited badly ({error}); this is the known Godot 4.7.2 cold-import \
             teardown fault with a GDExtension loaded — retrying, and the extension list is \
             checked either way"
        );
        run_with_env(ctx, godot, &import_args, environment)?;
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
/// `gamectl`, built through the checked call (`release` for the release
/// profile, else the dev profile the suite builds with) and returned as the
/// build reported it ([`resolved::reported_artefact`]).
/// The `scenario` step, `cargo xtask build-gamectl` (the watch check's and the
/// nightly arms' runner) and nothing else build it.
fn checked_gamectl(ctx: &Ctx, owner: &str, release: bool) -> Result<String, String> {
    refuse_codegen_variables()?;
    let graph = sim_graph(ctx)?;
    let mut args: Vec<String> = vec!["build".to_owned()];
    if release {
        args.push("--release".to_owned());
    }
    for arg in ["--package", owner, "--bin", SCENARIO_BIN] {
        args.push(arg.to_owned());
    }
    args.push(resolved::message_format(colour()).to_owned());
    if ctx.locked {
        args.push("--locked".to_owned());
    }
    let expect = if release {
        resolved::Expect::RELEASE
    } else {
        resolved::Expect::suite(resolved::SimRule::OptimisedIfBuilt)
    };
    let built = resolved::checked(
        &mut ProcessCargo::of(ctx),
        &resolved::CargoCall::pinned(args, ""),
        &graph,
        expect,
        &mut print_diagnostic,
    )?;
    resolved::reported_artefact(&built, &graph, owner, resolved::Wanted::Bin(SCENARIO_BIN))?
        .into_os_string()
        .into_string()
        .map_err(|path| {
            format!(
                "the `{SCENARIO_BIN}` binary's path {} is not Unicode",
                path.display()
            )
        })
}

/// The context for a command run outside `ci` and its steps, from the
/// environment alone: the root, the cargo, `--locked` as CI has it, the
/// workspace's metadata.
fn standalone_ctx(require_tools: bool) -> Result<Ctx, String> {
    let root = workspace_root()?;
    // As `ci` reads it: the cargo that started xtask sets `CARGO`; started some
    // other way, the cargo on `PATH` is the rule, not a fallback
    // ([`cargo_program`], which refuses a `CARGO` that is not Unicode).
    let cargo = cargo_program()?;
    let locked = env::var_os("CI").is_some() && root.join("Cargo.lock").is_file();
    let workspace = load_workspace(require_tools, &cargo, &root);
    Ok(Ctx {
        root,
        cargo,
        bless: false,
        require_tools,
        locked,
        packages: Vec::new(),
        client_check: false,
        workspace,
        shard: None,
    })
}

/// `cargo xtask checked-test --package <name> --test <target> [--ignored]
/// [--nocapture]`: one test target, run through the checked call and the
/// one door ([`resolved::execute`]), with the harness options it names. The
/// nightly fuzz job runs the verifier's ignored fuzz target through this.
fn checked_test(ctx: &Ctx, args: &[String]) -> Result<(), String> {
    let mut package: Option<String> = None;
    let mut target: Option<String> = None;
    let mut harness: Vec<resolved::Harness> = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--package" | "-p" => {
                let value = rest
                    .next()
                    .ok_or_else(|| "--package needs a package name".to_owned())?;
                package = Some(package_spec_name(value)?.to_owned());
            }
            "--test" => {
                target = Some(
                    rest.next()
                        .ok_or_else(|| "--test needs a test target's name".to_owned())?
                        .clone(),
                );
            }
            "--ignored" => harness.push(resolved::Harness::Ignored),
            "--nocapture" => harness.push(resolved::Harness::Nocapture),
            other => {
                return Err(format!(
                    "`checked-test` takes --package, --test, --ignored and --nocapture, and was \
                     given `{other}`"
                ));
            }
        }
    }
    let package = package.ok_or_else(|| "`checked-test` needs --package".to_owned())?;
    let target = target.ok_or_else(|| "`checked-test` needs --test".to_owned())?;
    let workspace = metadata(ctx)?;
    if !test_target_owners(workspace, &target).contains(&package) {
        return Err(format!("{package} has no test target called `{target}`"));
    }
    let sim = if package == resolved::SIM && resolved::UNOPTIMISED_TESTS.contains(&target.as_str())
    {
        resolved::SimRule::Unoptimised
    } else {
        resolved::SimRule::OptimisedIfBuilt
    };
    refuse_codegen_variables()?;
    let graph = sim_graph(ctx)?;
    let test = resolved::TestRun::new(
        vec!["--package".to_owned(), package],
        resolved::Invocation {
            selectors: vec!["--test".to_owned(), target],
            sim,
        },
        ctx.locked,
        colour(),
    )
    .with_harness(harness);
    resolved::execute(
        &test,
        &graph,
        &mut ProcessCargo::of(ctx),
        &mut print_diagnostic,
        &mut print_line,
    )
}

/// Runs a command for its exit status alone, swallowing its output. Used to ask
/// "does this subcommand exist yet", where a non-zero status is an answer and
/// not a failure.
fn command_succeeds(ctx: &Ctx, program: &str, args: &[String]) -> bool {
    child_command(ctx.require_tools, program, args, &ctx.root)
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

/// One perf alarm: an `#[ignore]`d test target run with `--release`,
/// `--ignored` and `--nocapture`, which prints its own `::notice::` lines.
struct Alarm {
    /// The name `cargo xtask perf-alarms <name>` selects it by, so CI runs
    /// each alarm in a step of its own: GitHub annotates only the first ten
    /// notices of a step (S2's plan, task `g3h`, item 149 (4)).
    name: &'static str,
    /// The package that owns the target.
    package: &'static str,
    /// The test target.
    test: &'static str,
    /// The one test of the target to run, by its exact name, when the target
    /// holds more than one `#[ignore]`d test: `--ignored` alone would run them
    /// side by side on libtest's threads, sharing the machine.
    exact: Option<&'static str>,
    /// The annotation title its warning goes under when it cannot measure.
    title: &'static str,
}

/// The alarms `cargo xtask perf-alarms` runs, in order, one after the other,
/// so no two measurements share the machine.
///
/// * **mesher p99**: `crates/mesher/tests/perf_alarm.rs`, run exactly as its
///   header says: the per-chunk mesh CPU p99, and `view`'s crater stream
///   through the client's relight and drain queue (its remesh latency at 20
///   and 40 explosions a second; S2's plan, task `view`).
/// * **P1's wall-clock half** (S1's plan, task `p1`): `crates/bench/tests/
///   perf_alarm.rs` — QUICK's p50 and p99 over the committed verifier cases
///   and `budget_128`, and a decision tick's cost in ns per size unit in a
///   hosted match at the size budget. The bench is walled, which is what lets
///   it read a clock while driving the verifier and the sim from outside
///   (AGENTS.md section 4.5).
/// * **G3′-real's figures** (S2's plan, section 5 and task `g3h`):
///   `crates/bench/tests/g3_real.rs` — the 8-minute segment at the gate's
///   load (three seats, 300 units and 40 beacons as world totals, the crater
///   stream at 20 a second), every tick timed phase by phase: the tick's p99,
///   p50 and mean, ticks a second, the tick-minus-pathing mean, the repath
///   backlog against the cap, the broadphase's share, kill-credit settlement,
///   the peak events per tick, the queues' bytes, live heap and the tick p99
///   per minute, allocations per tick, and `seen_by` at the gate's load; the
///   support search's adversarial tick with Mine's footing check at its
///   worst; and a fourth, unplaced seat's decision-tick cost. Published,
///   never gated (item 148's CI ruling); `g3r` certifies them on the owner's
///   machine, and the CI budget job is `g3r`'s.
/// * **P1 FULL** (S3's plan, section 5.2 and task `bench3`) shares the `p1`
///   alarm's target and step: FULL over the committed cases, and QUICK and
///   FULL over the generated shapes at each size of E24's curve with the
///   end-to-end `verify_plan` beside them (ten notices with S1's two). QUICK's
///   cost at the transport cap is `p1-cap`'s, and **the decision tick** at the
///   size budget on two seeds and at each size of the curve, in nanoseconds
///   and in counted evaluation units, is `p1-decision`'s: each a step of its
///   own for the ten-notice limit, which the bench's tests hold each alarm
///   under at compile time.
/// * **The decision tick by kind** (S3's plan, section 5.2 item 4; items
///   175 (8) and 196; `cert3`): `crates/bench/tests/g3_real.rs`'s
///   `p1_decision_kinds` — ns per counted unit for each kind of decision work
///   (condition, served-walk ranker, threat-ranking selector, seam upkeep,
///   commit, `toward`, deploy) on both seeds, whether the kinds agree within
///   a stated tolerance, and the seeds' gap against E12's rule; in the G3′
///   binary for its counting allocator, `p1-kinds`.
/// * **P3** (S3's plan, section 5.1): `crates/bench/tests/p3_alarm.rs` — the
///   repath on the sim's own graph, cold and under the crater stream with the
///   repair beside it, and the travel estimate through the gateway's
///   `estimate_route` in a hosted Lull and through the sim's estimator alone,
///   on three seeds. The gates are certified on the owner's machine (`cert3`);
///   `p3-budget` and `p1-budget` ([`budget`]) are the loose CI alarms.
const ALARMS: &[Alarm] = &[
    Alarm {
        name: "mesher",
        package: "pharmakos-mesher",
        test: "perf_alarm",
        exact: None,
        title: "mesher p99",
    },
    Alarm {
        name: "p1",
        package: "pharmakos-bench",
        test: "perf_alarm",
        // The target also holds `p1_transport_cap_timings` and
        // `p1_decision_tick_timings` (the next two alarms'), `p1_budget`
        // (`cargo xtask p1-budget`'s) and `p1_certify` (run by hand).
        exact: Some("p1_quick_and_full_timings"),
        title: "P1 timings",
    },
    Alarm {
        name: "p1-cap",
        package: "pharmakos-bench",
        test: "perf_alarm",
        exact: Some("p1_transport_cap_timings"),
        title: "P1 at the transport cap",
    },
    Alarm {
        name: "p1-decision",
        package: "pharmakos-bench",
        test: "perf_alarm",
        exact: Some("p1_decision_tick_timings"),
        title: "P1 decision tick",
    },
    Alarm {
        name: "p1-kinds",
        package: "pharmakos-bench",
        // The G3' binary, for its counting allocator: each kind reports the
        // allocations of its ticks (`cert3`'s B4, a `toward` step among them).
        test: "g3_real",
        exact: Some("p1_decision_kinds"),
        title: "P1 decision tick by kind",
    },
    Alarm {
        name: "p3",
        package: "pharmakos-bench",
        test: "p3_alarm",
        // The target also holds `p3_budget` (`cargo xtask p3-budget`'s).
        exact: Some("p3_pathing_timings"),
        title: "P3 timings",
    },
    Alarm {
        name: "g3",
        package: "pharmakos-bench",
        test: "g3_real",
        // The target also holds `g3_budget_segment` (`cargo xtask g3-budget`'s),
        // `p1_decision_kinds` (the `p1-kinds` alarm's), and `g3_power_sweep` and
        // `g3_raids_segments` (both run by hand).
        exact: Some("g3_real_segment_support_tick_and_fourth_seat"),
        title: "G3' timings",
    },
];

/// What `cargo xtask perf-alarms <name>` runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AlarmSelection {
    /// The [`ALARMS`] entry of this name.
    Alarm(&'static str),
    /// The allocations-per-tick notice alone.
    Allocations,
}

/// The selection a name names, or the names there are.
fn alarm_selection(name: &str) -> Result<AlarmSelection, String> {
    if name == "allocations" {
        return Ok(AlarmSelection::Allocations);
    }
    ALARMS
        .iter()
        .find(|alarm| alarm.name == name)
        .map(|alarm| AlarmSelection::Alarm(alarm.name))
        .ok_or_else(|| {
            let names: Vec<&str> = ALARMS.iter().map(|alarm| alarm.name).collect();
            format!(
                "`perf-alarms` knows the alarms {} and allocations, not `{name}`",
                names.join(", ")
            )
        })
}

/// `cargo xtask perf-alarms`: skeleton-plan section 7 decision 23, logged as
/// decisions-log item 116 (6)(b). Per runner, never compared, no threshold,
/// and it never fails — the job that runs it is not required, and
/// `scripts/merge-train.sh` stops on any check that is not pass or skipping,
/// so a failed measurement is a `::warning::` and the command still answers
/// success.
///
/// * every [`ALARMS`] entry, run as [`alarm_test`] builds it: `--release` is
///   not optional (a debug figure is a different number, not a slow version of
///   the real one), `--ignored`, `--nocapture` so the target's own
///   `::notice::` lines reach the annotations. No `--test-threads=1`: on one
///   thread libtest prints `test <name> ... ` before each test runs, so the
///   test's first `::notice::` would not start its line and GitHub would not
///   read it. A target whose measurements must not share the machine runs them
///   in one test, as the bench does;
/// * **allocations per tick**: a notice naming the test that asserts it. The
///   count is zero by construction — `crates/sim/tests/allocations.rs` fails
///   otherwise, in every leg's `test` step, which also fails unless libtest
///   reported that test passed, once ([`ALLOCATION_CHECK`]) — so it is
///   published, not measured again and never compared.
///
/// The tick-minus-pathing mean the skeleton plan named is G3′-real's, and
/// published by the `g3` alarm since S2's `g3h`; G3′'s budget is not gated
/// here but by `cargo xtask g3-budget`'s loose alarm ([`g3`]), and certified
/// on the owner's machine (`g3r`). xtask itself reads no clock; the measuring
/// tests do, behind the wall.
///
/// `only` runs one alarm, or the allocations notice alone. A name it does not
/// know is refused before anything runs ([`alarm_selection`]): the command
/// never fails over a measurement, but a typo is not a measurement.
fn perf_alarms(only: Option<AlarmSelection>) {
    let root = match workspace_root() {
        Ok(root) => root,
        Err(error) => {
            println!("::warning title=perf alarms::no measurement: {error}");
            return;
        }
    };
    let os = env::consts::OS;
    // Read as `ci` reads it, and passed on through `child_command` like every
    // other child's (S1-07). A value the reader refuses is a warning here, not
    // a failure, because this command never fails; the child then runs as if
    // the variable were unset.
    let require_tools = match require_tools_from(env::var_os(REQUIRE_TOOLS_VAR).as_deref()) {
        Ok(required) => required,
        Err(error) => {
            println!("::warning title=perf alarms::{error}; the measurement runs without it");
            false
        }
    };
    // Frozen install in CI, as every other cargo call xtask makes there: a
    // lockfile mismatch is then the usual warning below, and the job stays green.
    let locked = env::var_os("CI").is_some();
    let selected = |alarm: &&Alarm| match only {
        None => true,
        Some(AlarmSelection::Alarm(name)) => alarm.name == name,
        Some(AlarmSelection::Allocations) => false,
    };
    for alarm in ALARMS.iter().filter(selected) {
        if let Err(error) = run_measurement(&root, require_tools, &alarm_test(alarm, locked)) {
            println!(
                "::warning title={}::not measured on {os}: {error}; an alarm, not a gate, so \
                 this job stays green",
                alarm.title
            );
        }
    }
    if matches!(only, None | Some(AlarmSelection::Allocations)) {
        println!("{}", allocations_notice(os));
    }
}

/// The checked run one alarm measures, through the one door
/// ([`resolved::execute`]). Pure, so its command line is pinned by a unit
/// test.
fn alarm_test(alarm: &Alarm, locked: bool) -> resolved::TestRun {
    let mut harness = vec![resolved::Harness::Ignored, resolved::Harness::Nocapture];
    if let Some(name) = alarm.exact {
        harness.push(resolved::Harness::Exact(name.to_owned()));
    }
    resolved::TestRun::new(
        vec!["--package".to_owned(), alarm.package.to_owned()],
        resolved::Invocation {
            selectors: vec!["--test".to_owned(), alarm.test.to_owned()],
            sim: resolved::SimRule::OptimisedIfBuilt,
        },
        locked,
        false,
    )
    .release()
    .with_harness(harness)
}

/// Runs a measurement, `test`, through the checked call and the one door
/// ([`resolved::execute`]), its tools and environment pinned and its build
/// held to the release profile's promise, echoing what the test prints as it
/// comes; answers that output, for the alarm to read its report from.
pub(crate) fn run_measurement(
    root: &Path,
    require_tools: bool,
    test: &resolved::TestRun,
) -> Result<String, String> {
    refuse_codegen_variables()?;
    let mut cargo = ProcessCargo::new(root, require_tools)?;
    let graph = graph_with(&mut cargo, test.locked())?;
    let mut output: Vec<u8> = Vec::new();
    resolved::execute(
        test,
        &graph,
        &mut cargo,
        &mut print_diagnostic,
        &mut |line| {
            print_line(line)?;
            output.extend_from_slice(line);
            output.push(b'\n');
            Ok(())
        },
    )?;
    // The measurement's figures are read from this text, so it must be text;
    // the test is this workspace's own and prints none that is not.
    String::from_utf8(output)
        .map_err(|error| format!("the measurement printed something that is not UTF-8: {error}"))
}

/// The file of the test the allocations notice names, from the workspace root.
const ALLOCATIONS_TEST_FILE: &str = "crates/sim/tests/allocations.rs";

/// The test the allocations notice names, by its libtest path in
/// [`ALLOCATIONS_TEST_FILE`]: the tick tests sit in a `tick` module that the
/// `research` build leaves out (`simtest` #225). The notice and the `test`
/// step's check ([`ALLOCATION_CHECK`]) both read this one constant.
const ALLOCATIONS_TEST: &str = "tick::a_tick_allocates_nothing";

/// The allocations-per-tick notice, word for word. Pure, so its text is pinned.
fn allocations_notice(os: &str) -> String {
    format!(
        "::notice title=allocations per tick::0 on {os}: asserted by \
         {ALLOCATIONS_TEST_FILE}::{} in this commit's test step; zero by \
         construction, never compared",
        ALLOCATION_CHECK.path
    )
}

/// A test the `test` step requires libtest to report as passed, exactly once,
/// in the invocation that runs its target: a figure published as "asserted by
/// this test in this commit's test step" is then held by what the step saw,
/// not by a reading of the source, which would trail the language (a `cfg` on
/// a platform, a `cfg_attr` that ignores, `should_panic`, a file-level gate).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RequiredTest {
    /// The integration-test target, as `--test <target>` selects it.
    pub(crate) target: &'static str,
    /// The test's libtest path in that target.
    pub(crate) path: &'static str,
}

/// The `test` step's one required test: the allocation test, in the
/// opt-level-0 invocation `--test allocations` (xtask's `resolved` module).
pub(crate) const ALLOCATION_CHECK: RequiredTest = RequiredTest {
    target: "allocations",
    path: ALLOCATIONS_TEST,
};

/// Why libtest's output does not show a [`RequiredTest`] passing once.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RequiredTestFault {
    /// No top-level `test <path> ...` line: the test did not run (filtered
    /// out, compiled out, renamed, or a `should_panic` test, which libtest
    /// names `<path> - should panic`).
    Absent { path: &'static str },
    /// More than one top-level line for the path.
    Repeated { path: &'static str, count: usize },
    /// It ran, and its outcome is not `ok` (`ignored`, `FAILED`, or a
    /// `should_panic` variant's).
    NotOk { path: &'static str, outcome: String },
    /// A `test result:` line with no `running N tests` line open: output
    /// this reader cannot place, so it does not guess.
    Unbalanced { path: &'static str },
}

impl std::fmt::Display for RequiredTestFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Absent { path } => write!(
                f,
                "libtest reported no run of `{path}` in this invocation, so the figure the \
                 allocations notice publishes as asserted by it is not asserted"
            ),
            Self::Repeated { path, count } => write!(
                f,
                "libtest reported `{path}` {count} times in this invocation; the notice names \
                 one test"
            ),
            Self::NotOk { path, outcome } => write!(
                f,
                "libtest reported `{path}` as `{outcome}`, not `ok`, so the figure the \
                 allocations notice publishes is not asserted"
            ),
            Self::Unbalanced { path } => write!(
                f,
                "libtest's output for `{path}`'s invocation holds a `test result:` line with no \
                 run open, so whether the test ran cannot be read"
            ),
        }
    }
}

/// Reads one invocation's test output, line by line as the step prints it,
/// for one [`RequiredTest`]. Only top-level lines count: a test that prints a
/// nested libtest run of its own (`doctest_runner`'s shape) opens a second
/// `running N tests` block, and lines inside it are that inner run's.
#[derive(Debug)]
pub(crate) struct RequiredTestReader {
    test: RequiredTest,
    /// How many `running N tests` blocks are open.
    depth: usize,
    /// The outcome of each top-level line naming the test.
    outcomes: Vec<String>,
    unbalanced: bool,
}

impl RequiredTestReader {
    pub(crate) fn new(test: RequiredTest) -> Self {
        Self {
            test,
            depth: 0,
            outcomes: Vec::new(),
            unbalanced: false,
        }
    }

    /// Reads one line of output, as the test wrote it.
    pub(crate) fn feed(&mut self, line: &[u8]) {
        let Ok(text) = std::str::from_utf8(line) else {
            // Not text, so not one of libtest's lines.
            return;
        };
        let text = text.trim_end_matches('\r');
        if is_running_line(text) {
            match self.depth.checked_add(1) {
                Some(depth) => self.depth = depth,
                None => self.unbalanced = true,
            }
            return;
        }
        if text.starts_with("test result: ") {
            match self.depth.checked_sub(1) {
                Some(depth) => self.depth = depth,
                None => self.unbalanced = true,
            }
            return;
        }
        if self.depth != 1 {
            return;
        }
        let Some(rest) = text
            .strip_prefix("test ")
            .and_then(|rest| rest.strip_prefix(self.test.path))
        else {
            return;
        };
        if let Some(outcome) = rest.strip_prefix(" ... ") {
            self.outcomes.push(outcome.to_owned());
        } else if let Some(variant) = rest.strip_prefix(" - ") {
            // `<path> - should panic ... ok`: a different test from the one named.
            self.outcomes.push(variant.to_owned());
        }
        // Anything else (`has been running for over 60 seconds`, a longer
        // name that starts with this one) is not this test's result line.
    }

    /// The verdict once the invocation has ended.
    pub(crate) fn verdict(self) -> Result<(), RequiredTestFault> {
        let path = self.test.path;
        if self.unbalanced {
            return Err(RequiredTestFault::Unbalanced { path });
        }
        match self.outcomes.as_slice() {
            [] => Err(RequiredTestFault::Absent { path }),
            [outcome] if outcome == "ok" => Ok(()),
            [outcome] => Err(RequiredTestFault::NotOk {
                path,
                outcome: outcome.clone(),
            }),
            more => Err(RequiredTestFault::Repeated {
                path,
                count: more.len(),
            }),
        }
    }
}

/// Whether `line` is libtest's `running N test(s)` line, which opens a run.
fn is_running_line(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("running ") else {
        return false;
    };
    let Some(count) = rest
        .strip_suffix(" tests")
        .or_else(|| rest.strip_suffix(" test"))
    else {
        return false;
    };
    !count.is_empty() && count.bytes().all(|byte| byte.is_ascii_digit())
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
    /// Every target, with the flags `cargo xtask ci --shard` splits by.
    targets: Vec<shard::Target>,
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

fn load_workspace(require_tools: bool, cargo: &str, root: &Path) -> Result<Workspace, String> {
    let output = child_command(
        require_tools,
        cargo,
        &["metadata", "--format-version", "1", "--no-deps"],
        root,
    )
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
        let mut targets: Vec<shard::Target> = Vec::new();
        if let Some(entries) = entry.get("targets").and_then(Json::as_array) {
            for target in entries {
                // A target without its name or its kinds cannot be split, and
                // dropping it would take it out of the shards in silence.
                let Some(target_name) = target.get("name").and_then(Json::as_str) else {
                    return Err(format!(
                        "cargo metadata gives {name} a target with no `name`"
                    ));
                };
                let kinds: Vec<String> = match target.get("kind").and_then(Json::as_array) {
                    Some(kinds) if !kinds.is_empty() => kinds
                        .iter()
                        .map(|kind| {
                            kind.as_str().map(str::to_owned).ok_or_else(|| {
                                format!(
                                    "cargo metadata gives {name}'s target `{target_name}` a \
                                     kind that is not a string"
                                )
                            })
                        })
                        .collect::<Result<_, _>>()?,
                    _ => {
                        return Err(format!(
                            "cargo metadata gives {name}'s target `{target_name}` no `kind`"
                        ));
                    }
                };
                if kinds.iter().any(|kind| kind == "bin") {
                    bins.push(target_name.to_owned());
                }
                let flag = |key: &str| match target.get(key) {
                    Some(Json::Bool(value)) => Some(*value),
                    _ => None,
                };
                targets.push(shard::Target {
                    name: target_name.to_owned(),
                    kinds,
                    test: flag("test"),
                    doctest: flag("doctest"),
                });
            }
        }
        packages.push(Package {
            name: name.to_owned(),
            features,
            bins,
            targets,
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
/// `pharmakos-sim/research`.
///
/// A failure, not `None`, when the metadata is missing or no crate declares
/// the feature: `clippy`'s research pass used to be skipped in silence in both
/// cases, which was one of the three fall-backs S1-05 names. Pure over the
/// metadata, so `the_three_fall_backs_fail` breaks it.
fn research_feature_spec(workspace: &Result<Workspace, String>) -> Result<String, String> {
    let workspace = workspace
        .as_ref()
        .map_err(|reason| format!("no workspace metadata for the research pass: {reason}"))?;
    workspace
        .package_with_feature(SIM_PACKAGES, RESEARCH_FEATURE)
        .map(|name| format!("{name}/{RESEARCH_FEATURE}"))
        .ok_or_else(|| {
            format!(
                "no crate declares a `{RESEARCH_FEATURE}` feature, so clippy's research pass has \
                 nothing to lint; `pharmakos-sim` defines it, and `fork` lives behind it"
            )
        })
}

/// The walled crates in the workspace, for `clippy`'s two passes.
///
/// A failure when the metadata is missing. It used to answer an empty list,
/// under which pass 1 linted the walled crates with the full deny set and pass
/// 2 did not run — a run that checked something other than the wall (S1-05).
fn walled_packages(workspace: &Result<Workspace, String>) -> Result<Vec<String>, String> {
    workspace
        .as_ref()
        .map(|workspace| workspace.present(WALLED_PACKAGES))
        .map_err(|reason| format!("no workspace metadata to find the walled crates: {reason}"))
}

/// Cargo's target directory, where the producing tests write their fresh
/// goldens.
///
/// A failure when the metadata is missing. It used to fall back to
/// `<root>/target`, which is the wrong place whenever `CARGO_TARGET_DIR` or a
/// `build.target-dir` moves it — and then every fresh output would read as
/// missing, or worse, as a stale one from an earlier run (S1-05).
fn golden_target_dir(workspace: &Result<Workspace, String>) -> Result<PathBuf, String> {
    workspace
        .as_ref()
        .map(|workspace| workspace.target_dir.clone())
        .map_err(|reason| {
            format!("no workspace metadata to find the fresh goldens' target directory: {reason}")
        })
}

/// Whether `tree`, `cargo tree -e features --format {p}|{f}` for `package`,
/// lets it reach the research feature; the failure names every line that
/// does (the first twenty).
fn judge_research_tree(package: &str, all_features: bool, tree: &str) -> Result<(), String> {
    let mut offenders: Vec<&str> = Vec::new();
    for line in tree.lines().filter(|line| !line.trim().is_empty()) {
        if line_enables_research(line)? {
            offenders.push(line);
        }
    }
    if offenders.is_empty() {
        return Ok(());
    }
    let how = if all_features {
        "with --all-features"
    } else {
        "with its default features"
    };
    let mut report = format!(
        "`{package}` reaches the `{RESEARCH_FEATURE}` feature {how}.\n      \
         The crates of GUARDED_PACKAGES may never depend on `fork`: \"no dry \
         runs\" is a hard guarantee in shipped builds (spec section 15), and the \
         walled bench must not time one.\n      \
         Offending feature edges:\n"
    );
    for line in offenders.iter().take(20) {
        report.push_str("        ");
        report.push_str(line.trim_end());
        report.push('\n');
    }
    Err(report)
}

/// The characters of cargo tree's drawing, which [`research_tree_args`]'s
/// `--prefix none` turns off: a line holding one is not the shape asked for.
const TREE_DRAWING: &[char] = &['│', '├', '└', '─'];

/// The `cargo tree` query the research guard reads: every feature edge of
/// `package` (normal, build and dev dependencies alike, the tree's default
/// kinds), its output shape pinned on the command line so that no
/// environment or config can change it. `--color never`: CI sets
/// `CARGO_TERM_COLOR=always`, and the flag beats it. `--prefix none`: no tree
/// drawing and no `[dev-dependencies]` or `[build-dependencies]` headers,
/// which are part of the drawing, so every line is one node. Deduplication
/// stays on: cargo prints each node in full where it first meets it and marks
/// every later meeting ` (*)`, so a feature reached anywhere has a line of its
/// own, and `--no-dedupe` would only repeat lines (and grows with the graph's
/// paths, not its nodes).
fn research_tree_args(package: &str, all_features: bool, locked: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "tree",
        "--color",
        "never",
        "--prefix",
        "none",
        "--package",
        package,
        "--edges",
        "features",
        "--format",
        "{p}|{f}",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    if all_features {
        args.push("--all-features".to_owned());
    }
    if locked {
        args.push("--locked".to_owned());
    }
    args
}

/// Whether a line of [`research_tree_args`]'s output shows the research
/// feature enabled. Its shape is pinned, so exactly two are read, each with
/// an optional ` (*)` after a node already shown: a package line, `{p}|{f}`,
/// whose feature list is read; and a feature node, which cargo prints in its
/// own fixed form, `<name> feature "<feature>"`. Any other line is a failure,
/// a control character (a colour escape) or the tree's drawing included: read
/// as "no feature", it would let the guard pass on output whose shape
/// changed.
fn line_enables_research(line: &str) -> Result<bool, String> {
    let refused = || {
        Err(format!(
            "`cargo tree --color never --prefix none --format {{p}}|{{f}}` printed a line that \
             is no package line or feature node: `{}`; the research guard reads every line, and \
             does not guess at this one",
            line.escape_debug()
        ))
    };
    if line.chars().any(char::is_control)
        || line.contains(TREE_DRAWING)
        || line.starts_with(char::is_whitespace)
    {
        return refused();
    }
    let node = line.strip_suffix(" (*)").unwrap_or(line);
    let Some((package, features)) = node.split_once('|') else {
        if let Some((_, feature)) = node.rsplit_once(" feature \"")
            && let Some(feature) = feature.strip_suffix('"')
            && !feature.contains('"')
        {
            return Ok(feature == RESEARCH_FEATURE);
        }
        return refused();
    };
    if package.contains(&format!("feature \"{RESEARCH_FEATURE}\"")) {
        return Ok(true);
    }
    Ok(features
        .split(',')
        .any(|feature| feature.trim() == RESEARCH_FEATURE))
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

/// One JSON string escape, read from `rest`, the text after its backslash:
/// the character it spells and the bytes it took. The whole of JSON's set:
/// `\" \\ \/ \b \f \n \r \t`, and `\uXXXX`, four hex digits and no
/// sign, a surrogate pair read as the one character it spells. A surrogate
/// without its other half is not a character and is refused, never read as
/// U+FFFD. The one escape decoder in xtask: the JSON reader and the licence
/// check's quotation reader both read escapes through it.
pub(crate) fn json_escape(rest: &str) -> Result<(char, usize), String> {
    let hex4 = |at: usize| -> Result<u32, String> {
        let digits = rest
            .get(at..at + 4)
            .ok_or_else(|| "a `\\u` escape ends before its four hex digits".to_owned())?;
        if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("bad \\u escape `{digits}`"));
        }
        u32::from_str_radix(digits, 16).map_err(|error| format!("bad \\u escape: {error}"))
    };
    let simple = match rest.as_bytes().first() {
        None => return Err("unterminated JSON escape".to_owned()),
        Some(b'"') => '"',
        Some(b'\\') => '\\',
        Some(b'/') => '/',
        Some(b'b') => '\u{0008}',
        Some(b'f') => '\u{000c}',
        Some(b'n') => '\n',
        Some(b'r') => '\r',
        Some(b't') => '\t',
        Some(b'u') => {
            let code = hex4(1)?;
            return match code {
                0xD800..=0xDBFF => {
                    if rest.get(5..7) != Some("\\u") {
                        return Err(format!("an unpaired high surrogate \\u{code:04x}"));
                    }
                    let low = hex4(7)?;
                    if !(0xDC00..=0xDFFF).contains(&low) {
                        return Err(format!(
                            "the high surrogate \\u{code:04x} is followed by \\u{low:04x}, not a \
                             low surrogate"
                        ));
                    }
                    char::from_u32(0x1_0000 + ((code - 0xD800) << 10) + (low - 0xDC00))
                        .map(|character| (character, 11))
                        .ok_or_else(|| format!("\\u{code:04x}\\u{low:04x} is not a character"))
                }
                0xDC00..=0xDFFF => Err(format!("an unpaired low surrogate \\u{code:04x}")),
                _ => char::from_u32(code)
                    .map(|character| (character, 5))
                    .ok_or_else(|| format!("\\u{code:04x} is not a character")),
            };
        }
        Some(other) => {
            return Err(format!("unknown JSON escape `\\{}`", char::from(*other)));
        }
    };
    Ok((simple, 1))
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
            let at = self.index;
            let key = self.string()?;
            // A repeated key has no one value: refused, never read as the
            // first or the last.
            if members.iter().any(|(name, _)| *name == key) {
                return Err(format!(
                    "the key `{key}` appears twice in one object, at byte {at}"
                ));
            }
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
                    // One escape is at most 11 bytes after its backslash
                    // (`uXXXX\uXXXX`), all ASCII: read that much, as text.
                    let window = self
                        .bytes
                        .iter()
                        .skip(self.index)
                        .take(11)
                        .take_while(|byte| byte.is_ascii())
                        .count();
                    let text = self.slice(self.index, self.index + window)?;
                    let (character, used) = json_escape(text)?;
                    out.push(character);
                    self.index += used;
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
        if let Some(parent) = manifest_dir.parent()
            && parent.join("Cargo.toml").is_file()
        {
            return Ok(parent.to_path_buf());
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

/// The environment variable that asks for the tools: `1` turns a missing tool
/// from a skip into a failure, in xtask ([`Ctx::require_tools`]) and in the
/// tests that need one (`crates/proto/tests/generated.rs` needs buf and
/// `protoc-gen-prost`). CI sets it for the whole workflow. Read by
/// [`require_tools_from`].
const REQUIRE_TOOLS_VAR: &str = "PHARMAKOS_REQUIRE_TOOLS";

/// What [`REQUIRE_TOOLS_VAR`] asks for: `1` requires the tools, unset or `0`
/// does not, and anything else — `true`, `yes`, an empty string, a stray
/// space — is an error that names the variable (S1's plan, decision 9, ruling
/// the register's S1-06). Before it, the variable was read as set or unset, so
/// `PHARMAKOS_REQUIRE_TOOLS=0` meant on. `crates/proto/tests/generated.rs`
/// reads it by the same rule.
fn require_tools_from(value: Option<&OsStr>) -> Result<bool, String> {
    match value.map(OsStr::to_str) {
        None | Some(Some("0")) => Ok(false),
        Some(Some("1")) => Ok(true),
        Some(other) => Err(format!(
            "{REQUIRE_TOOLS_VAR} is `{}`; it is `1` (a missing tool fails), or `0` or unset (a \
             missing tool skips), and nothing else",
            other.unwrap_or("<not UTF-8>")
        )),
    }
}

/// A child process as xtask starts every one of its children: `program args`
/// in `cwd`, with [`REQUIRE_TOOLS_VAR`] set to `1` when the run requires its
/// tools and removed when it does not. So `--require-tools` alone makes a test
/// that needs a tool fail on a missing one, and `--no-require-tools` reaches
/// the children too, even from a shell that exported the variable
/// (decisions-log item 118 (4)).
///
/// The only `Command::new` in xtask outside its tests, since S1-07: the tool
/// probes, the metadata read, `perf-alarms`, `ci-scope`'s git and `package`'s
/// children all come through here, and `every_child_goes_through_child_command`
/// counts the constructions in every module's source.
pub(crate) fn child_command<S: AsRef<OsStr>>(
    require_tools: bool,
    program: &str,
    args: &[S],
    cwd: &Path,
) -> Command {
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    if require_tools {
        command.env(REQUIRE_TOOLS_VAR, "1");
    } else {
        command.env_remove(REQUIRE_TOOLS_VAR);
    }
    command
}

fn run_in(ctx: &Ctx, program: &str, args: &[String], cwd: &Path) -> Result<(), String> {
    println!("   $ {}", render_command(program, args));
    let status = child_command(ctx.require_tools, program, args, cwd)
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
    let output = child_command(ctx.require_tools, program, args, &ctx.root)
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
fn tool_available(ctx: &Ctx, program: &str) -> bool {
    child_command(ctx.require_tools, program, &["--version"], &ctx.root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn cargo_subcommand_available(ctx: &Ctx, subcommand: &str) -> bool {
    child_command(
        ctx.require_tools,
        &ctx.cargo,
        &[subcommand, "--version"],
        &ctx.root,
    )
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .is_ok_and(|status| status.success())
}

fn git_ref_exists(ctx: &Ctx, reference: &str) -> bool {
    child_command(
        ctx.require_tools,
        "git",
        &["rev-parse", "--verify", "--quiet", reference],
        &ctx.root,
    )
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

/// A path's last component, lossily as text; `None` for a path that has
/// none (a root, or one ending in `..`), which every caller handles itself.
fn file_name(path: &Path) -> Option<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
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
    // The line holding byte `offset`, or the file's last line when the offset
    // lies past it (the other file is the longer one). A final newline ends
    // the last line rather than opening an empty one.
    let body = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let mut line: &[u8] = &[];
    let mut start = 0;
    for piece in body.split(|byte| *byte == b'\n') {
        line = piece;
        let next = start + piece.len() + 1;
        if offset < next {
            break;
        }
        start = next;
    }
    let text = String::from_utf8_lossy(line).into_owned();
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
    fn json_refuses_a_duplicate_key_and_an_unpaired_surrogate() {
        // A repeated key has no one value.
        assert!(Json::parse(r#"{"opt_level":"2","opt_level":"0"}"#).is_err());
        assert!(Json::parse(r#"{"a":{"b":1,"b":1}}"#).is_err());
        assert!(
            Json::parse(r#"[{"b":1},{"b":1}]"#).is_ok(),
            "one key in each object"
        );
        // A pair is a character beyond the Basic Multilingual Plane.
        let pair = Json::parse(r#""\ud83d\ude00""#).expect("a pair");
        assert_eq!(pair.as_str(), Some("\u{1f600}"));
        // A surrogate without its other half is not a character.
        for unpaired in [
            r#""\ud83d""#,
            r#""\ud83d x""#,
            r#""\ud83d\u0041""#,
            r#""\ude00""#,
            r#""\u12g4""#,
            r#""\u+123""#,
        ] {
            assert!(Json::parse(unpaired).is_err(), "{unpaired}");
        }
    }

    /// The exemption of `resolved::UNOPTIMISED_TESTS` is a set of (package,
    /// target) pairs: the workspace's owners of each name are exactly the
    /// table's, and each owner but the sim does not depend on the sim, read
    /// from the real workspace's graph. A third package gaining such a target
    /// fails here.
    #[test]
    fn the_unoptimised_tests_have_the_owners_the_table_names() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let Some(root) = manifest.parent() else {
            panic!("xtask's manifest directory has no parent");
        };
        let workspace = match load_workspace(false, env!("CARGO"), root) {
            Ok(workspace) => workspace,
            Err(reason) => panic!("{reason}"),
        };
        let output = child_command(
            false,
            env!("CARGO"),
            &["metadata", "--format-version", "1"],
            root,
        )
        .output()
        .expect("cargo metadata runs");
        assert!(output.status.success(), "cargo metadata failed");
        let text = String::from_utf8(output.stdout).expect("UTF-8");
        let graph = resolved::graph(&text).expect("the graph reads");
        for name in resolved::UNOPTIMISED_TESTS {
            let owners = test_target_owners(&workspace, name);
            assert_eq!(
                resolved::check_owners(name, &owners, &graph),
                Ok(()),
                "{name}: {owners:?}"
            );
        }
        for (name, package, reason) in resolved::UNOPTIMISED_OWNERS {
            assert!(
                resolved::UNOPTIMISED_TESTS.contains(name),
                "{package} is listed for a test that is not unoptimised"
            );
            assert!(!reason.is_empty(), "{package} rides with no reason");
        }
    }

    /// The one escape decoder: JSON's whole set, `\u` with four hex digits and
    /// no sign, a pair read as its character, a lone surrogate refused.
    #[test]
    fn the_one_escape_decoder_reads_json_escapes_exactly() {
        assert_eq!(json_escape("n rest"), Ok(('\n', 1)));
        assert_eq!(json_escape("\"x"), Ok(('"', 1)));
        assert_eq!(json_escape("u0041x"), Ok(('A', 5)));
        assert_eq!(json_escape("ud83d\\ude00x"), Ok(('\u{1f600}', 11)));
        for bad in [
            "",
            "q",
            "u+041",
            "u004",
            "ud83d",
            "ud83dx",
            "ud83d\\u0041",
            "ude00",
            "u{41}",
        ] {
            assert!(json_escape(bad).is_err(), "{bad:?}");
        }
    }

    /// `release-checked` promises the release profile's optimisation: its own
    /// `opt-level` if it states one, else release's, else cargo's 3.
    #[test]
    fn the_gate_is_promised_releases_level_and_no_other() {
        let level = |text: &str| promised_opt_level(&scan_profiles(text), "release-checked");
        assert_eq!(
            level("[profile.release]\nlto = true\n"),
            Ok(resolved::OptLevel::O3)
        );
        assert_eq!(
            level("[profile.release]\nopt-level = 2\n"),
            Ok(resolved::OptLevel::O2)
        );
        // The gate is built at release's level and no other: an `opt-level`
        // anywhere in `release-checked`'s tables is refused.
        for own in [
            "[profile.release-checked]\nopt-level = 1\n",
            "[profile.release-checked.package.xxhash-rust]\nopt-level = 0\n",
            "[profile.release-checked.build-override]\nopt-level = 3\n",
        ] {
            let refused = level(&format!("[profile.release]\nopt-level = 2\n{own}"));
            assert!(
                refused
                    .as_ref()
                    .is_err_and(|why| why.contains("release-checked")),
                "{own}: {refused:?}"
            );
        }
        assert!(level("[profile.release]\nopt-level = 7\n").is_err());
        assert_eq!(
            promised_opt_level(
                &scan_profiles(include_str!("../../Cargo.toml")),
                "release-checked"
            ),
            Ok(resolved::OptLevel::O3)
        );
    }

    /// The dev package tables against the packages cargo resolves: a stale
    /// table, a table at 0 outside the godot family, and a family member
    /// without its table are each refused; the committed manifest passes.
    #[test]
    fn the_dev_package_tables_match_the_graph() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let Some(root) = manifest.parent() else {
            panic!("xtask's manifest directory has no parent");
        };
        let output = child_command(
            false,
            env!("CARGO"),
            &["metadata", "--format-version", "1"],
            root,
        )
        .output()
        .expect("cargo metadata runs");
        assert!(output.status.success(), "cargo metadata failed");
        let graph =
            resolved::graph(&String::from_utf8(output.stdout).expect("UTF-8")).expect("a graph");
        let committed = include_str!("../../Cargo.toml");
        assert_eq!(
            dev_package_table_failures(&scan_profiles(committed), &graph),
            Vec::<String>::new()
        );
        let with = |extra: &str| {
            dev_package_table_failures(&scan_profiles(&format!("{committed}{extra}")), &graph)
        };
        let syn = with("\n[profile.dev.package.syn]\nopt-level = 0\n");
        assert_eq!(syn.len(), 1, "{syn:?}");
        assert!(
            syn.first()
                .is_some_and(|failure| failure.contains("not of the godot family")),
            "{syn:?}"
        );
        let stale = with("\n[profile.dev.package.no-such-crate]\nopt-level = 0\n");
        assert!(
            stale
                .first()
                .is_some_and(|failure| failure.contains("no package cargo resolves")),
            "{stale:?}"
        );
        let without = committed.replace("[profile.dev.package.heck]\nopt-level = 0\n", "");
        assert_ne!(without, committed, "the heck table is committed");
        let missing = dev_package_table_failures(&scan_profiles(&without), &graph);
        assert!(
            missing
                .first()
                .is_some_and(|failure| failure.contains("`heck` is of the godot family")),
            "{missing:?}"
        );
        // Tables are matched as cargo matches a spec: a family member's table
        // by name and version is its table; a version no package has is a
        // stale table; a family table at another level says so.
        let versioned = committed.replace(
            "[profile.dev.package.glam]\nopt-level = 0\n",
            "[profile.dev.package.\"glam@0.32.1\"]\nopt-level = 0\n",
        );
        assert_ne!(versioned, committed, "the glam table is committed");
        assert_eq!(
            dev_package_table_failures(&scan_profiles(&versioned), &graph),
            Vec::<String>::new()
        );
        let wrong_version = with("\n[profile.dev.package.\"syn@0.0.1\"]\nopt-level = 3\n");
        assert!(
            wrong_version
                .first()
                .is_some_and(|failure| failure.contains("matches no package")),
            "{wrong_version:?}"
        );
        let family_at_one = committed.replace(
            "[profile.dev.package.glam]\nopt-level = 0\n",
            "[profile.dev.package.glam]\nopt-level = 1\n",
        );
        let at_one = dev_package_table_failures(&scan_profiles(&family_at_one), &graph);
        assert!(
            at_one
                .iter()
                .any(|failure| failure.contains("at opt-level 1; the family")),
            "{at_one:?}"
        );
        // An optimised table for a crate that exists is no business of this.
        assert_eq!(
            with("\n[profile.dev.package.syn]\nopt-level = 3\n"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn json_rejects_trailing_input() {
        assert!(Json::parse("{} {}").is_err());
        assert!(Json::parse("{\"a\":").is_err());
    }

    #[test]
    fn research_edges_are_detected() {
        assert_eq!(
            line_enables_research("pharmakos-sim v0.0.0|default,research"),
            Ok(true)
        );
        assert_eq!(
            line_enables_research("pharmakos-sim feature \"research\"|"),
            Ok(true)
        );
        assert_eq!(
            line_enables_research("pharmakos-sim v0.0.0|default,replay"),
            Ok(false)
        );
        assert_eq!(
            line_enables_research("pharmakos-gateway v0.0.0|"),
            Ok(false)
        );
        // `researching` must not trip it.
        assert_eq!(line_enables_research("crate v1|researching"), Ok(false));
        // A feature node, in cargo's own form, with and without ` (*)`.
        assert_eq!(
            line_enables_research("pharmakos-sim feature \"research\""),
            Ok(true)
        );
        assert_eq!(
            line_enables_research("pharmakos-proto feature \"default\" (*)"),
            Ok(false)
        );
        // A package line after one already shown, its features before the ` (*)`.
        assert_eq!(
            line_enables_research("pharmakos-sim v0.0.0 (/w/crates/sim)|research (*)"),
            Ok(true)
        );
        // A line in another shape is a failure, not "no feature".
        assert!(line_enables_research("pharmakos-sim v0.0.0 (research)").is_err());
        assert!(line_enables_research("pharmakos-sim feature \"rese").is_err());
    }

    /// The query pins `--prefix none` and `--color never`, so a line with
    /// the tree's drawing, a section header (part of the drawing) or a colour
    /// escape is not the shape asked for, and is refused, never read: a
    /// coloured package line would otherwise read `research\u{1b}[0m` as some
    /// other feature. The first line is CI's, from shard 1 (ubuntu) on
    /// `d12e9cea`.
    #[test]
    fn a_line_out_of_the_pinned_shape_is_refused() {
        for line in [
            "\u{1b}[2m│\u{1b}[0m   \u{1b}[2m\u{1b}[35m└──\u{1b}[0m proc-macro2 feature \"proc-macro\" \u{1b}[33m\u{1b}[2m(*)\u{1b}[39m\u{1b}[22m",
            "pharmakos-sim v0.0.0 (/w/crates/sim)|default,research\u{1b}[0m",
            "\u{1b}[1mpharmakos-sim\u{1b}[0m v0.0.0|research",
            "│   ├── pharmakos-sim feature \"research\"",
            "└── pharmakos-sim v0.0.0 (/w/crates/sim)|research (*)",
            "    pharmakos-sim feature \"research\"",
            "[dev-dependencies]",
            "[build-dependencies]",
        ] {
            let error = line_enables_research(line).expect_err(line);
            assert!(error.contains("does not guess"), "{error}");
        }
    }

    /// Both tree queries pin their output's shape on their own command line.
    #[test]
    fn the_tree_queries_pin_their_shape() {
        let pinned = |args: &[String]| {
            args.windows(2).any(|pair| pair == ["--color", "never"])
                && args.windows(2).any(|pair| pair == ["--prefix", "none"])
        };
        for all_features in [false, true] {
            for locked in [false, true] {
                let args = research_tree_args("pharmakos-verifier", all_features, locked);
                assert!(pinned(&args), "{args:?}");
                assert!(args.windows(2).any(|pair| pair == ["--format", "{p}|{f}"]));
                assert_eq!(args.contains(&"--all-features".to_owned()), all_features);
                assert!(!args.contains(&"--no-dedupe".to_owned()));
                let listing = dependency_listing_args("pharmakos-sim", locked);
                assert!(pinned(&listing), "{listing:?}");
            }
        }
    }

    /// A tree as cargo 1.98.1 prints it under the guard's own pins, for a
    /// crate with a normal, a dev- and a build-dependency (recorded from a
    /// scratch crate with `--color never --prefix none`): no headers, no
    /// drawing, every node on a line, and a node met again marked ` (*)`.
    fn pinned_tree(dev_features: &str, build_features: &str) -> String {
        [
            "pharmakos-verifier v0.0.0 (/w/crates/verifier)|",
            "pharmakos-proto feature \"default\"",
            "pharmakos-proto v0.0.0 (/w/crates/proto)|",
            "pharmakos-proto feature \"default\" (*)",
            &format!("pharmakos-proto v0.0.0 (/w/crates/proto)|{build_features} (*)"),
            "pharmakos-sim feature \"default\"",
            &format!("pharmakos-sim v0.0.0 (/w/crates/sim)|{dev_features}"),
            "pharmakos-proto feature \"default\" (*)",
        ]
        .join("\n")
    }

    /// AGENTS.md section 3 rule 3 sanctions a guarded crate's harmless
    /// dev-dependency; `research` reached through a dev- or a
    /// build-dependency, or as a feature node, is refused, and for that
    /// reason.
    #[test]
    fn the_research_guard_reads_dev_and_build_dependencies() {
        assert_eq!(
            judge_research_tree(
                "pharmakos-verifier",
                false,
                &pinned_tree("default", "default")
            ),
            Ok(())
        );
        for tree in [
            pinned_tree("default,research", "default"),
            pinned_tree("default", "research"),
            pinned_tree("default", "default") + "\npharmakos-sim feature \"research\"",
        ] {
            let error = judge_research_tree("pharmakos-verifier", true, &tree)
                .expect_err("research is reached");
            assert!(
                error.starts_with(
                    "`pharmakos-verifier` reaches the `research` feature with --all-features."
                ),
                "{error}"
            );
        }
        // A line of any other shape is not read as "no feature".
        let error = judge_research_tree(
            "pharmakos-verifier",
            false,
            &(pinned_tree("default", "default") + "\n[dev-dependencies]"),
        )
        .expect_err("a header the pins turn off");
        assert!(error.contains("does not guess"), "{error}");
    }

    /// A runner whose executable a build replaced while it ran (Linux's
    /// `current_exe` then answers `<path> (deleted)`) is refused with the
    /// true cause, not as a path with whitespace.
    #[test]
    fn a_runner_that_is_gone_is_refused() {
        let here = env::current_exe().expect("the test knows its executable");
        assert_eq!(runner_exists(&here), Ok(()));
        let gone = here.with_file_name("xtask-replaced-while-it-ran (deleted)");
        let error = runner_exists(&gone).expect_err("gone");
        assert!(error.contains("replaced"), "{error}");
    }

    /// The runner is named relative to cargo's working directory when it
    /// lies under it, so a checkout whose path holds a space still runs; it
    /// is refused only when no spelling is free of whitespace.
    #[test]
    fn the_runner_is_named_without_whitespace_or_refused() {
        let root = Path::new("/home/a b/pharmakos");
        // The `cargo xtask` alias runs xtask from its own profile's folder.
        assert_eq!(
            runner_spelling(&root.join("target/xtask/xtask"), root),
            Ok("target/xtask/xtask".to_owned())
        );
        assert_eq!(
            runner_spelling(Path::new("/build/citime/debug/xtask"), root),
            Ok("/build/citime/debug/xtask".to_owned())
        );
        let error = runner_spelling(Path::new("/build/sp ace/debug/xtask"), root)
            .expect_err("no spelling without a space");
        assert!(error.contains("Move the target directory"), "{error}");
        assert!(runner_spelling(&root.join("my target/debug/xtask"), root).is_err());
        assert!(runner_spelling(Path::new("/build/o'brien/xtask"), Path::new("/w")).is_err());
    }

    /// The channel the door holds the compiler's release to is the file's.
    #[test]
    fn the_pinned_channel_is_the_toolchain_files() {
        let channel = pinned_channel().expect("the file names a channel");
        assert!(
            include_str!("../../rust-toolchain.toml").contains(&format!("channel = \"{channel}\"")),
            "{channel}"
        );
    }

    /// The cargo xtask shells out to: `CARGO`, or the one on `PATH`; a
    /// `CARGO` that is not Unicode is refused, not read as absent.
    #[test]
    fn a_cargo_that_is_not_unicode_is_refused() {
        assert_eq!(cargo_program_from(None), Ok("cargo".to_owned()));
        assert_eq!(
            cargo_program_from(Some("C:/rust/cargo.exe".into())),
            Ok("C:/rust/cargo.exe".to_owned())
        );
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let bad = std::ffi::OsString::from_vec(vec![b'c', 0xff]);
            assert!(cargo_program_from(Some(bad)).is_err());
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            let bad = std::ffi::OsString::from_wide(&[0x63, 0xd800]);
            assert!(cargo_program_from(Some(bad)).is_err());
        }
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
        let scan = scan_profiles(good);
        assert!(scan.release_checked);
        assert!(scan.overflow_off.is_empty());

        let switched_off = "[profile.release]\noverflow-checks = false\n";
        let scan = scan_profiles(switched_off);
        assert!(!scan.release_checked);
        assert_eq!(scan.overflow_off, ["profile.release"]);

        // A key outside a profile table must not be mistaken for one.
        let elsewhere = "[workspace.metadata]\noverflow-checks = false\n";
        assert_eq!(scan_profiles(elsewhere), ProfileScan::default());
    }

    /// Every profile, `xtask` included, is held to the overflow rule by the
    /// `overflow_off` scan over both files: re-review 8's M1 and M2, the
    /// committed manifest with `overflow-checks = false` under
    /// `[profile.xtask]`, in `Cargo.toml` and in `.cargo/config.toml`, is red.
    #[test]
    fn a_profile_of_our_own_cannot_turn_the_checks_off() {
        let committed = include_str!("../../Cargo.toml");
        let config = include_str!("../../.cargo/config.toml");
        assert_eq!(
            profile_failures(&scan_profiles(committed), Some(&scan_profiles(config))),
            Vec::<String>::new()
        );
        let manifest_off = committed.replace(
            "[profile.xtask]\ninherits = \"dev\"\n",
            "[profile.xtask]\ninherits = \"dev\"\noverflow-checks = false\n",
        );
        assert_ne!(
            manifest_off, committed,
            "the manifest declares [profile.xtask]"
        );
        let config_off = format!("{config}\n[profile.xtask]\noverflow-checks = false\n");
        for (manifest, config) in [
            (manifest_off.as_str(), config),
            (committed, config_off.as_str()),
        ] {
            let failures = profile_failures(&scan_profiles(manifest), Some(&scan_profiles(config)));
            assert!(
                failures.iter().any(|failure| failure
                    .contains("overflow checks are switched off in")
                    && failure.contains("profile.xtask")),
                "{failures:?}"
            );
        }
    }

    /// The `profiles` step reads the TOML only for what the suite never
    /// builds, and fails closed: every form of a profile line it cannot read
    /// is a failure, among them the two forms cargo applies that a plain
    /// table scan misses (a dotted `package.<spec>.<key>` under
    /// `[profile.test]`, and an inline table under a bare `[profile]`).
    #[test]
    fn a_profile_line_the_scan_cannot_read_fails_closed() {
        let base = "[profile.release]\noverflow-checks = true\n\
                    [profile.dev]\noverflow-checks = true\n";
        assert_eq!(
            profile_failures(&scan_profiles(base), None),
            Vec::<String>::new()
        );
        for (form, extra) in [
            (
                "a dotted package key",
                "[profile.test]\npackage.pharmakos-sim.opt-level = 0\n",
            ),
            (
                "an inline table under a bare [profile]",
                "[profile]\ntest = { debug-assertions = false }\n",
            ),
            ("a bare [profile] table", "[profile]\n"),
            (
                "a dotted key under [profile.dev]",
                "[profile.dev]\nbuild-override.opt-level = 0\n",
            ),
            (
                "an inline table value",
                "[profile.dev.package]\npharmakos-sim = { opt-level = 0 }\n",
            ),
            (
                "a quoted key",
                "[profile.test]\n\"overflow-checks\" = false\n",
            ),
            (
                "an array of tables",
                "[[profile.test]]\ndebug-assertions = false\n",
            ),
            (
                "a quoted profile name",
                "[profile.\"test\"]\noverflow-checks = false\n",
            ),
            (
                "a multi-line value",
                "[profile.test]\nrustflags = [\n  \"-Cdebug-assertions=off\",\n]\n",
            ),
            (
                "a dotted key at the root",
                "profile.release.overflow-checks = false\n",
            ),
            ("a line with no value", "[profile.test]\noverflow-checks\n"),
        ] {
            let text = if form == "a dotted key at the root" {
                format!("{extra}{base}")
            } else {
                format!("{base}{extra}")
            };
            let scan = scan_profiles(&text);
            assert!(!scan.unreadable.is_empty(), "{form} is read: {text}");
            let failures = profile_failures(&scan, None);
            assert!(
                failures
                    .iter()
                    .any(|failure| failure.contains("cannot read")),
                "{form}: {failures:?}"
            );
            // And in the cargo config, named as the config's.
            let config = scan_profiles(extra);
            let failures = profile_failures(&scan_profiles(base), Some(&config));
            assert!(
                failures
                    .iter()
                    .any(|failure| failure.contains(".cargo/config.toml line")),
                "{form} in the config: {failures:?}"
            );
        }
        // The forms the scan does read: a package table under any spelling of
        // its spec, a build override, quoted and integer values, comments.
        let readable = format!(
            "{base}[profile.dev.package.\"*\"]\nopt-level = 2   # third party\n\
             [ profile.test.package.\"pharmakos-sim@0.0.0\" ]  # note\nopt-level = 's'\n\
             [profile.dev.build-override]\ndebug = false\n\
             [profile.release-client]\ninherits = \"release\"\n"
        );
        assert_eq!(scan_profiles(&readable).unreadable, Vec::<String>::new());
        // Overflow checks switched off in a readable table are still caught.
        let off = format!("{base}[profile.dev.package.\"*\"]\noverflow-checks = false\n");
        assert_eq!(scan_profiles(&off).overflow_off, ["profile.dev.package.*"]);
    }

    /// The committed manifest and cargo config pass the `profiles` step, every
    /// profile line read.
    #[test]
    fn the_committed_profiles_pass_and_are_read_whole() {
        let manifest = scan_profiles(include_str!("../../Cargo.toml"));
        let config = scan_profiles(include_str!("../../.cargo/config.toml"));
        assert_eq!(
            profile_failures(&manifest, Some(&config)),
            Vec::<String>::new()
        );
    }

    /// A cargo config that is there and cannot be read fails the step; one that
    /// is not there leaves nothing to check.
    #[test]
    fn an_unreadable_cargo_config_is_a_failure_not_a_pass() {
        let dir = std::env::temp_dir().join(format!("xtask-read-config-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("a scratch folder");
        // A folder where the file should be: there, and not readable as text.
        let error = read_config(&dir).expect_err("a folder is not a config");
        assert!(error.contains("cannot read"), "{error}");
        assert_eq!(read_config(&dir.join("absent.toml")), Ok(None));
        let file = dir.join("config.toml");
        fs::write(&file, "[profile.dev]\noverflow-checks = true\n").expect("write");
        assert_eq!(
            read_config(&file),
            Ok(Some("[profile.dev]\noverflow-checks = true\n".to_owned()))
        );
        let not_utf8 = dir.join("binary.toml");
        fs::write(&not_utf8, [0xff_u8, 0xfe, 0x00]).expect("write");
        assert!(read_config(&not_utf8).is_err());
        fs::remove_dir_all(&dir).expect("the scratch folder goes");
    }

    #[test]
    fn diffs_name_the_first_differing_line() {
        let report = first_difference(b"alpha\nbeta\n", b"alpha\ngamma\n");
        assert!(report.contains("line 2"), "{report}");
        assert!(report.contains("beta"), "{report}");
        assert!(report.contains("gamma"), "{report}");
    }

    /// The line shown is the one holding the offset, a newline's own line for
    /// an offset on it, and the last line for an offset past the end.
    #[test]
    fn the_line_shown_is_the_one_holding_the_offset() {
        assert_eq!(line_at(b"alpha\nbeta\n", 0), "alpha");
        assert_eq!(line_at(b"alpha\nbeta\n", 5), "alpha");
        assert_eq!(line_at(b"alpha\nbeta\n", 6), "beta");
        assert_eq!(line_at(b"alpha\nbeta\n", 10), "beta");
        assert_eq!(line_at(b"alpha\nbeta\n", 99), "beta");
        assert_eq!(line_at(b"alpha\nbeta", 99), "beta");
        assert_eq!(line_at(b"\n", 3), "");
        assert_eq!(line_at(b"", 3), "<empty file>");
        let report = first_difference(b"alpha\n", b"alpha\nmore\n");
        assert!(report.contains("expected: alpha"), "{report}");
        assert!(report.contains("actual:   more"), "{report}");
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

        // `name@version` names the sim; a glob or a package-id URL is
        // refused, never guessed at.
        assert_eq!(
            research_test_args(
                Some("pharmakos-sim"),
                &strings(&["pharmakos-sim@0.1.0"]),
                false
            ),
            Ok(run(expected.clone()))
        );
        for spec in [
            "pharmakos-*",
            "pharmakos-si?",
            "path+file:///repo/crates/sim#0.1.0",
        ] {
            assert!(
                research_test_args(Some("pharmakos-sim"), &strings(&[spec]), false).is_err(),
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
            "pharmakos-operator",
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
    fn require_tools_reaches_every_child_and_its_absence_does_too() {
        let args = ["test".to_owned(), "--workspace".to_owned()];
        let cwd = Path::new(".");
        let required = child_command(true, "cargo", &args, cwd);
        let set: Vec<_> = required.get_envs().collect();
        assert_eq!(
            set,
            vec![(OsStr::new(REQUIRE_TOOLS_VAR), Some(OsStr::new("1")))],
            "--require-tools sets the variable on the child"
        );
        let not_required = child_command(false, "cargo", &args, cwd);
        let removed: Vec<_> = not_required.get_envs().collect();
        assert_eq!(
            removed,
            vec![(OsStr::new(REQUIRE_TOOLS_VAR), None)],
            "--no-require-tools removes it, so an exported one does not leak through"
        );
        // The command itself is untouched.
        assert_eq!(required.get_program(), OsStr::new("cargo"));
        assert_eq!(
            required.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("test"), OsStr::new("--workspace")]
        );
        assert_eq!(required.get_current_dir(), Some(cwd));
        assert_eq!(REQUIRE_TOOLS_VAR, "PHARMAKOS_REQUIRE_TOOLS");
    }

    /// S1-06, as S1's plan's decision 9 rules it: `1` on, unset or `0` off,
    /// anything else an error that names the variable. Before it, `0` meant on.
    #[test]
    fn require_tools_reads_one_zero_or_unset_and_refuses_anything_else() {
        assert_eq!(require_tools_from(None), Ok(false));
        assert_eq!(require_tools_from(Some(OsStr::new("0"))), Ok(false));
        assert_eq!(require_tools_from(Some(OsStr::new("1"))), Ok(true));
        for wrong in [
            "", " 1", "1 ", "true", "yes", "on", "2", "01", "false", "off",
        ] {
            let error = require_tools_from(Some(OsStr::new(wrong))).expect_err(wrong);
            assert!(error.contains(REQUIRE_TOOLS_VAR), "{error}");
            assert!(error.contains(&format!("`{wrong}`")), "{error}");
        }
    }

    /// A scoped run lints a walled crate with the wall's allowances and a
    /// deterministic one without them, as the unscoped run does; a value that
    /// does not name a walled crate outright stays under the stricter set.
    #[test]
    fn a_scoped_clippy_run_keeps_the_wall() {
        let walled = strings(&["pharmakos-client-gdext", "pharmakos-mesher"]);
        let pass = |packages: &[&str], allow: bool, locked: bool| {
            let mut args = strings(&["clippy"]);
            for package in packages {
                args.push("--package".to_owned());
                args.push((*package).to_owned());
            }
            args.push("--all-targets".to_owned());
            if locked {
                args.push("--locked".to_owned());
            }
            args.push("--".to_owned());
            args.extend(DETERMINISM_DENY.iter().map(|flag| (*flag).to_owned()));
            if allow {
                args.extend(WALL_ALLOW.iter().map(|flag| (*flag).to_owned()));
            }
            args
        };

        // The pre-commit hook's call for a client commit: pass 2 alone.
        assert_eq!(
            scoped_clippy_args(&strings(&["pharmakos-client-gdext"]), &walled, false),
            vec![pass(&["pharmakos-client-gdext"], true, false)]
        );
        // A deterministic crate alone: pass 1 alone, never the allowances.
        assert_eq!(
            scoped_clippy_args(&strings(&["pharmakos-sim"]), &walled, true),
            vec![pass(&["pharmakos-sim"], false, true)]
        );
        // Both kinds: pass 1 first, then pass 2, each with its own crates.
        assert_eq!(
            scoped_clippy_args(
                &strings(&[
                    "pharmakos-mesher@0.1.0",
                    "pharmakos-gateway",
                    "pharmakos-sim"
                ]),
                &walled,
                false
            ),
            vec![
                pass(&["pharmakos-gateway", "pharmakos-sim"], false, false),
                pass(&["pharmakos-mesher@0.1.0"], true, false),
            ]
        );
        // A glob or an id URL may reach a walled crate and a deterministic one
        // alike, so it stays under the stricter set.
        for spec in ["pharmakos-*", "path+file:///repo/crates/mesher#0.1.0"] {
            assert_eq!(
                scoped_clippy_args(&strings(&[spec]), &walled, false),
                vec![pass(&[spec], false, false)],
                "{spec}"
            );
        }
        assert!(scoped_clippy_args(&[], &walled, false).is_empty());
    }

    /// S1-05, as S1's plan's decision 9 rules it: the three consumers of the
    /// workspace metadata that used to fall back without it — `golden`'s
    /// target directory, `clippy`'s walled list and its research pass — fail
    /// instead, and the research pass fails too when no crate declares the
    /// feature it lints.
    #[test]
    fn the_three_fall_backs_fail() {
        let missing: Result<Workspace, String> = Err("`cargo metadata` failed".to_owned());
        let golden = golden_target_dir(&missing).expect_err("golden fails");
        assert!(golden.contains("`cargo metadata` failed"), "{golden}");
        let walled = walled_packages(&missing).expect_err("clippy's walled list fails");
        assert!(walled.contains("`cargo metadata` failed"), "{walled}");
        let research = research_feature_spec(&missing).expect_err("the research pass fails");
        assert!(research.contains("`cargo metadata` failed"), "{research}");

        // With metadata, each answers what the metadata says.
        let present: Result<Workspace, String> = parse_workspace(
            r#"{"packages":[
                {"name":"pharmakos-sim","features":{"research":[]},"targets":[]},
                {"name":"pharmakos-mesher","features":{},"targets":[]}
            ],"target_directory":"/somewhere/else","workspace_root":"/repo"}"#,
        );
        assert_eq!(
            golden_target_dir(&present),
            Ok(PathBuf::from("/somewhere/else")),
            "the target directory is cargo's, never an assumed <root>/target"
        );
        assert_eq!(
            walled_packages(&present),
            Ok(vec!["pharmakos-mesher".to_owned()])
        );
        assert_eq!(
            research_feature_spec(&present),
            Ok("pharmakos-sim/research".to_owned())
        );

        // A workspace whose sim lost the feature: a failure, not a skipped pass.
        let featureless: Result<Workspace, String> = parse_workspace(
            r#"{"packages":[{"name":"pharmakos-sim","features":{},"targets":[]}],
                "workspace_root":"/repo"}"#,
        );
        let error = research_feature_spec(&featureless).expect_err("no research feature");
        assert!(
            error.contains("no crate declares a `research` feature"),
            "{error}"
        );
    }

    /// S1-07: every child process xtask starts is built by `child_command`,
    /// so `--require-tools` reaches it. Counted over every module's source
    /// text outside its tests: `Command::new` appears once, in
    /// `child_command` itself.
    #[test]
    fn every_child_goes_through_child_command() {
        let sources: &[(&str, &str)] = &[
            ("main.rs", include_str!("main.rs")),
            ("annotate.rs", include_str!("annotate.rs")),
            ("budget.rs", include_str!("budget.rs")),
            ("g3.rs", include_str!("g3.rs")),
            ("golden.rs", include_str!("golden.rs")),
            ("licence_quotes.rs", include_str!("licence_quotes.rs")),
            ("package.rs", include_str!("package.rs")),
            ("placeholders.rs", include_str!("placeholders.rs")),
            ("png.rs", include_str!("png.rs")),
            ("resolved.rs", include_str!("resolved.rs")),
            ("runner.rs", include_str!("runner.rs")),
            ("scenario.rs", include_str!("scenario.rs")),
            ("scope.rs", include_str!("scope.rs")),
            ("shard.rs", include_str!("shard.rs")),
            ("zip.rs", include_str!("zip.rs")),
        ];
        let needle = ["Command", "::new("].concat();
        let mut found: Vec<String> = Vec::new();
        for (name, source) in sources {
            // Everything from a file's test module on is its tests, which
            // build fixture repositories with git of their own.
            let code = source
                .split("\n#[cfg(test)]\nmod tests")
                .next()
                .unwrap_or(source);
            for (index, line) in code.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                if trimmed.contains(needle.as_str()) {
                    found.push(format!("{name}:{}", index + 1));
                }
            }
        }
        // The one exception: xtask's test runner (`runner.rs`) is the test
        // process itself, started by cargo with the environment xtask's own
        // `child_command` gave cargo, and it hands that environment to the test
        // untouched, so `--require-tools` reaches the test through it.
        let runner: Vec<String> = found
            .iter()
            .filter(|place| place.starts_with("runner.rs:"))
            .cloned()
            .collect();
        assert_eq!(
            runner.len(),
            1,
            "the runner starts its test in one place: {runner:?}"
        );
        found.retain(|place| !place.starts_with("runner.rs:"));
        assert_eq!(
            found.len(),
            1,
            "a child process is started outside child_command, so --require-tools does not \
             reach it (S1-07): {found:?}"
        );
        assert!(
            found
                .first()
                .is_some_and(|place| place.starts_with("main.rs:")),
            "{found:?}"
        );
        // And every module is in the list above: a new module is a new place
        // a child could be started from.
        let declared = include_str!("main.rs")
            .lines()
            .filter(|line| line.starts_with("mod ") && line.ends_with(';'))
            .count();
        assert_eq!(
            declared + 1,
            sources.len(),
            "a module is missing from the list"
        );
    }

    /// S1-01: the toolchain is pinned to one exact version, and the
    /// `toolchain:` input of every Rust install step in the workflows names
    /// it. The two workflows that install Rust, `ci.yml` and
    /// `nightly-scenarios.yml`, are read by name, and a listing of
    /// `.github/workflows/` fails the test when any other workflow installs
    /// Rust, so a new one cannot go unchecked. `[workspace.package] rust-version`
    /// and `clippy.toml`'s `msrv` agree with each other and carry the pin's
    /// `major.minor`, so a raise of the pin cannot leave them behind: clippy.toml
    /// says how they move with it.
    #[test]
    fn the_toolchain_is_pinned_once_everywhere() {
        /// `major.minor[.patch]` as numbers, for comparing versions.
        fn numbers(version: &str) -> Vec<u32> {
            version
                .split('.')
                .map(|part| {
                    part.parse::<u32>()
                        .unwrap_or_else(|_| panic!("`{version}` is not a version"))
                })
                .collect()
        }
        fn quoted_value<'a>(text: &'a str, key: &str) -> &'a str {
            text.lines()
                .map(str::trim)
                .find_map(|line| line.strip_prefix(key))
                .and_then(|rest| rest.trim_start().strip_prefix('='))
                .and_then(|rest| rest.trim().split('"').nth(1))
                .unwrap_or_else(|| panic!("no `{key} = \"...\"` line"))
        }
        let channel = quoted_value(include_str!("../../rust-toolchain.toml"), "channel");
        let parts: Vec<&str> = channel.split('.').collect();
        assert_eq!(
            parts.len(),
            3,
            "rust-toolchain.toml pins an exact version, never `stable`: `{channel}`"
        );
        assert!(
            parts.iter().all(|part| part.parse::<u32>().is_ok()),
            "{channel}"
        );
        let rust_version = quoted_value(include_str!("../../Cargo.toml"), "rust-version");
        let msrv = quoted_value(include_str!("../../clippy.toml"), "msrv");
        assert_eq!(
            rust_version, msrv,
            "[workspace.package] rust-version and clippy.toml's msrv are one number"
        );
        assert_eq!(
            numbers(msrv),
            numbers(channel).into_iter().take(2).collect::<Vec<u32>>(),
            "the MSRV ({msrv}) is not the pinned toolchain's major.minor ({channel})"
        );
        let pinned = format!("toolchain: {channel}");
        for (name, workflow) in [
            ("ci.yml", include_str!("../../.github/workflows/ci.yml")),
            (
                "nightly-scenarios.yml",
                include_str!("../../.github/workflows/nightly-scenarios.yml"),
            ),
        ] {
            let installs = workflow.matches("uses: dtolnay/rust-toolchain@").count();
            let named = workflow
                .lines()
                .filter(|line| line.trim() == pinned)
                .count();
            assert!(installs > 0, "{name} installs no toolchain");
            assert_eq!(
                named, installs,
                "every Rust install step in {name} names `{pinned}`"
            );
        }
        let workflows = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".github")
            .join("workflows");
        let mut files = Vec::new();
        walk(&workflows, &mut files).expect(".github/workflows");
        assert!(
            files.len() >= 2,
            "no workflows under {}",
            workflows.display()
        );
        let unchecked = unread_rust_installers(&files).unwrap_or_else(|error| panic!("{error}"));
        assert!(
            unchecked.is_empty(),
            "these workflows install Rust and this test does not read them: {unchecked:?}"
        );
    }

    /// The workflows other than the two the test reads that install Rust, by
    /// file name. A workflow that cannot be read, or whose name is not text,
    /// fails the scan: read as empty, it would pass as one that installs
    /// nothing.
    fn unread_rust_installers(files: &[PathBuf]) -> Result<Vec<String>, String> {
        let mut unchecked = Vec::new();
        for path in files {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| format!("{}: a workflow without a text name", path.display()))?
                .to_owned();
            if name == "ci.yml" || name == "nightly-scenarios.yml" {
                continue;
            }
            let text = std::fs::read_to_string(path)
                .map_err(|error| format!("reading {}: {error}", path.display()))?;
            if text.contains("dtolnay/rust-toolchain@") {
                unchecked.push(name);
            }
        }
        Ok(unchecked)
    }

    /// The scan refuses a workflow it cannot read, rather than passing it.
    #[test]
    fn a_workflow_the_scan_cannot_read_fails_it() {
        let missing = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".github")
            .join("workflows")
            .join("no-such-workflow.yml");
        let scanned = unread_rust_installers(&[missing]);
        assert!(
            scanned
                .as_ref()
                .is_err_and(|error| error.contains("no-such-workflow.yml")),
            "{scanned:?}"
        );
    }

    #[test]
    fn placeholders_is_a_step_after_reuse() {
        // S1's plan, decision 9: `tune` made the register's check a step.
        let names: Vec<&str> = STEPS.iter().map(|step| step.name).collect();
        let at = names.iter().position(|name| *name == "placeholders");
        assert!(at.is_some(), "{names:?}");
        assert_eq!(
            at.and_then(|index| index.checked_sub(1))
                .and_then(|index| names.get(index)),
            Some(&"reuse"),
            "{names:?}"
        );
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
    fn the_guard_lists_partition_the_crates() {
        // The wall's deterministic side is the research guard's crates that
        // are not walled, plus the sim, which defines `research` and so cannot
        // be on the research guard.
        let mut expected: Vec<&str> = GUARDED_PACKAGES
            .iter()
            .copied()
            .filter(|name| !WALLED_PACKAGES.contains(name))
            .chain(SIM_PACKAGES.iter().copied())
            .collect();
        expected.sort_unstable();
        let mut deterministic: Vec<&str> = WALL_GUARDED_PACKAGES.to_vec();
        deterministic.sort_unstable();
        assert_eq!(deterministic, expected);
        // The bench (S1's plan, decision 2): walled, on the research guard, on
        // the wall's deterministic side never, and with no client line.
        assert!(WALLED_PACKAGES.contains(&"bench"));
        assert!(GUARDED_PACKAGES.contains(&"bench"));
        assert!(!WALL_GUARDED_PACKAGES.contains(&"bench"));
        assert!(
            CLIENT_WALL
                .iter()
                .all(|(client, forbidden)| { *client != "bench" && !forbidden.contains(&"bench") })
        );
    }

    #[test]
    fn a_deterministic_crate_that_reached_the_bench_is_refused() {
        let walled: Vec<String> = WALLED_PACKAGES
            .iter()
            .map(|name| format!("pharmakos-{name}"))
            .collect();
        let clean = "pharmakos-gamectl v0.0.0 (/r)\npharmakos-gateway v0.0.0 (/r)\n";
        assert_eq!(first_reached("pharmakos-gamectl", clean, &walled), None);
        let broken = format!("{clean}pharmakos-bench v0.0.0 (/r)\n");
        assert_eq!(
            first_reached("pharmakos-gamectl", &broken, &walled),
            Some("pharmakos-bench")
        );
        // The bench reaching the sim, the verifier and the gateway is the point
        // of it, and no relation of wall-guard's names that.
        assert!(
            CLIENT_WALL
                .iter()
                .all(|(client, _)| !client.contains("bench"))
        );
    }

    #[test]
    fn every_alarm_runs_in_release_ignored_and_on_libtests_own_threads() {
        let packages: Vec<&str> = ALARMS.iter().map(|alarm| alarm.package).collect();
        assert_eq!(
            packages,
            vec![
                "pharmakos-mesher",
                "pharmakos-bench",
                "pharmakos-bench",
                "pharmakos-bench",
                "pharmakos-bench",
                "pharmakos-bench",
                "pharmakos-bench"
            ]
        );
        for alarm in ALARMS {
            for locked in [false, true] {
                let args = alarm_test(alarm, locked).run_line();
                let (cargo, test) =
                    args.split_at(args.iter().position(|arg| arg == "--").expect("a `--`"));
                assert!(cargo.iter().any(|arg| arg == "test"), "{args:?}");
                assert!(cargo.iter().any(|arg| arg == "--release"), "{args:?}");
                assert_eq!(cargo.iter().any(|arg| arg == "--locked"), locked);
                assert!(
                    cargo
                        .windows(2)
                        .any(|pair| pair == ["--package", alarm.package])
                );
                assert!(cargo.windows(2).any(|pair| pair == ["--test", alarm.test]));
                let mut expected = vec!["--", "--ignored", "--nocapture"];
                if let Some(name) = alarm.exact {
                    expected.extend(["--exact", name]);
                }
                assert_eq!(test, expected.as_slice(), "{args:?}");
            }
        }
    }

    #[test]
    fn the_client_check_keeps_its_user_folders_under_the_target_directory() {
        let target = env::temp_dir().join(format!(
            "pharmakos-xtask-client-data-{}",
            std::process::id()
        ));
        let data = client_data_dir(&target);
        let environment = client_env(&data);
        let keys: Vec<&str> = environment.iter().map(|(key, _)| key.as_str()).collect();
        if cfg!(target_os = "windows") {
            assert_eq!(keys, ["APPDATA", "LOCALAPPDATA"]);
        } else {
            assert_eq!(
                keys,
                ["HOME", "XDG_CACHE_HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME"]
            );
        }
        for (key, value) in &environment {
            let path = Path::new(value);
            assert!(path.starts_with(&data) && path != data, "{key}={value}");
        }

        // A run's leftovers are gone and every folder exists afterwards.
        let stale = data.join("appdata").join("Godot").join("old.jsonc");
        fs::create_dir_all(stale.parent().expect("a parent")).expect("temp dir");
        fs::write(&stale, "{}\n").expect("write");
        reset_client_data(&target, &data, &environment).expect("reset");
        assert!(!stale.exists());
        for (_, folder) in &environment {
            assert!(Path::new(folder).is_dir(), "{folder}");
        }
        // Never a folder outside the target directory, nor the directory itself.
        let elsewhere = env::temp_dir().join(format!(
            "pharmakos-xtask-not-the-target-{}",
            std::process::id()
        ));
        assert!(reset_client_data(&target, &elsewhere, &environment).is_err());
        assert!(reset_client_data(&target, &target, &environment).is_err());
        fs::remove_dir_all(&target).expect("clean up");
    }

    #[test]
    fn emptying_the_fresh_outputs_removes_a_stale_actual() {
        let parent = env::temp_dir().join(format!(
            "pharmakos-xtask-fresh-outputs-{}",
            std::process::id()
        ));
        let golden = parent.join("golden");
        let stale = golden
            .join("scenarios")
            .join("gone")
            .join("actual.hashes.txt");
        fs::create_dir_all(stale.parent().expect("a parent")).expect("temp dir");
        fs::write(&stale, "0\tdeadbeefdeadbeef\n").expect("write");
        empty_fresh_outputs(&golden).expect("emptied");
        assert!(!golden.exists());
        // Already empty is the state it wants, not an error.
        empty_fresh_outputs(&golden).expect("nothing to empty");
        fs::remove_dir_all(&parent).expect("clean up");
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
            ChainVerdict::Moved(b"0\ta\n")
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
             crates/sim/tests/allocations.rs::tick::a_tick_allocates_nothing in this commit's \
             test step; zero by construction, never compared"
        );
    }

    /// The test the notice names is the test the step's check holds.
    #[test]
    fn the_notice_names_the_test_the_step_checks() {
        assert!(
            allocations_notice("linux").contains(&format!(
                "{ALLOCATIONS_TEST_FILE}::{} in this commit's",
                ALLOCATION_CHECK.path
            )),
            "{}",
            allocations_notice("linux")
        );
        assert_eq!(
            ALLOCATIONS_TEST_FILE,
            format!("crates/sim/tests/{}.rs", ALLOCATION_CHECK.target)
        );
        assert!(resolved::UNOPTIMISED_TESTS.contains(&ALLOCATION_CHECK.target));
    }

    /// The `test` step's check of the allocation test, over libtest outputs
    /// whose verdicts are known.
    #[test]
    fn the_required_test_is_read_from_libtest_s_own_lines() {
        let read = |lines: &[&str]| {
            let mut reader = RequiredTestReader::new(ALLOCATION_CHECK);
            for line in lines {
                reader.feed(line.as_bytes());
            }
            reader.verdict()
        };
        let path = ALLOCATION_CHECK.path;
        let ok = format!("test {path} ... ok");
        let result = "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out";
        // Passed once, beside another test, with a CR and a long-run notice.
        assert_eq!(
            read(&[
                "",
                "running 2 tests",
                &format!("test {path} has been running for over 60 seconds"),
                "test tick::a_breach_search_allocates_nothing ... ok",
                &format!("{ok}\r"),
                "",
                result,
            ]),
            Ok(())
        );
        // Ignored, plain or with a reason.
        for outcome in ["ignored", "ignored, by hand"] {
            assert_eq!(
                read(&[
                    "running 1 test",
                    &format!("test {path} ... {outcome}"),
                    result
                ]),
                Err(RequiredTestFault::NotOk {
                    path,
                    outcome: outcome.to_owned()
                })
            );
        }
        // Failed.
        assert_eq!(
            read(&["running 1 test", &format!("test {path} ... FAILED"), result]),
            Err(RequiredTestFault::NotOk {
                path,
                outcome: "FAILED".to_owned()
            })
        );
        // A `should_panic` test of that path is not the test named.
        assert_eq!(
            read(&[
                "running 1 test",
                &format!("test {path} - should panic ... ok"),
                result
            ]),
            Err(RequiredTestFault::NotOk {
                path,
                outcome: "should panic ... ok".to_owned()
            })
        );
        // Absent: no binary ran it, or every test was filtered out.
        assert_eq!(read(&[]), Err(RequiredTestFault::Absent { path }));
        assert_eq!(
            read(&[
                "running 0 tests",
                "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out",
            ]),
            Err(RequiredTestFault::Absent { path })
        );
        // Twice: two binaries of the invocation both hold the path.
        assert_eq!(
            read(&["running 1 test", &ok, result, "running 1 test", &ok, result]),
            Err(RequiredTestFault::Repeated { path, count: 2 })
        );
        // A different module's test of the same name, and a longer name.
        assert_eq!(
            read(&[
                "running 3 tests",
                "test a_tick_allocates_nothing ... ok",
                "test other::tick::a_tick_allocates_nothing ... ok",
                &format!("test {path}_twice ... ok"),
                result,
            ]),
            Err(RequiredTestFault::Absent { path })
        );
        // A nested run printed by a test (`doctest_runner`'s shape) is not the
        // invocation's own: its line does not count, before or after.
        assert_eq!(
            read(&[
                "running 1 test",
                "running 2 tests",
                &ok,
                "test result: FAILED. 1 passed; 1 failed",
                "test outer ... ok",
                result,
            ]),
            Err(RequiredTestFault::Absent { path })
        );
        assert_eq!(
            read(&[
                "running 2 tests",
                "running 1 test",
                "test result: ok. 1 passed",
                &ok,
                result,
            ]),
            Ok(())
        );
        // A result with no run open cannot be placed.
        assert_eq!(
            read(&[result, "running 1 test", &ok, result]),
            Err(RequiredTestFault::Unbalanced { path })
        );
    }

    #[test]
    fn the_test_step_says_where_the_allocation_test_is_checked() {
        assert_eq!(
            allocation_check_note(Some("the sentence".to_owned()), true),
            "the sentence"
        );
        assert_eq!(
            allocation_check_note(None, false),
            "the --package list leaves pharmakos-sim out, so `tick::a_tick_allocates_nothing` \
             is not checked in this run"
        );
        assert_eq!(
            allocation_check_note(None, true),
            "`tick::a_tick_allocates_nothing` is checked by the shard that runs \
             `test:allocations`"
        );
    }

    #[test]
    fn perf_alarms_is_not_a_step() {
        assert!(STEPS.iter().all(|step| step.name != "perf-alarms"));
        assert!(STEPS.iter().all(|step| step.name != "g3-budget"));
        assert!(STEPS.iter().all(|step| step.name != "p3-budget"));
        assert!(STEPS.iter().all(|step| step.name != "p1-budget"));
    }

    /// The `perf alarms` job's shape: every measuring step continues on
    /// error, skips on the docs-only fast path and has a timeout of its own,
    /// and the job's timeout is above the sum of its steps' (the scope step's
    /// included), so a step that times out never brings the job to its own
    /// timeout, which would be red.
    #[test]
    fn the_perf_alarms_job_can_never_go_red() {
        let ci = include_str!("../../.github/workflows/ci.yml");
        let start = ci.find("\n  perf-alarms:\n").expect("the perf-alarms job");
        let job = ci
            .get(start + 1..)
            .and_then(|rest| {
                rest.find("\n  g3-budget:\n")
                    .and_then(|end| rest.get(..end))
            })
            .expect("the job ends where g3-budget begins");
        let minutes = |text: &str| -> u32 {
            text.trim()
                .strip_prefix("timeout-minutes: ")
                .and_then(|value| value.parse().ok())
                .unwrap_or_else(|| panic!("`{text}` is not a timeout"))
        };
        let job_timeout = job
            .lines()
            .find(|line| line.starts_with("    timeout-minutes: "))
            .map(minutes)
            .expect("the job's timeout");
        let steps: Vec<&str> = job.split("\n      - name: ").skip(1).collect();
        let mut sum = 0;
        let mut measuring = 0;
        for step in &steps {
            let timeout = step
                .lines()
                .find(|line| line.trim_start().starts_with("timeout-minutes: "));
            if let Some(line) = timeout {
                sum += minutes(line);
            }
            if step.starts_with("cargo xtask perf-alarms ") {
                measuring += 1;
                assert!(
                    step.contains("\n        continue-on-error: true\n"),
                    "{step}"
                );
                assert!(
                    step.contains("\n        if: steps.scope.outputs.prose_only != 'true'\n"),
                    "{step}"
                );
                assert!(
                    timeout.is_some(),
                    "a measuring step without a timeout: {step}"
                );
            }
        }
        assert_eq!(
            measuring,
            ALARMS.len() + 1,
            "every alarm and the allocations notice"
        );
        assert!(
            sum < job_timeout,
            "the steps' timeouts sum to {sum} minutes, not below the job's {job_timeout}"
        );
    }

    /// The `authoring budgets` job (S3's plan, E12): each authoring alarm in
    /// [`budget::BUDGETS`] runs in a step of its own, exactly once, and none
    /// continues on error, so the job can go red; every measuring step skips
    /// on the docs-only fast path, the second runs after a red first
    /// (`!cancelled()`), each has a timeout, and the job's timeout is above
    /// their sum with the scope step's.
    #[test]
    fn the_authoring_budgets_job_runs_both_alarms_and_can_go_red() {
        let ci = include_str!("../../.github/workflows/ci.yml");
        let start = ci
            .find("\n  authoring-budgets:\n")
            .expect("the authoring-budgets job");
        let body: Vec<&str> = ci
            .get(start + 1..)
            .expect("the job's text")
            .lines()
            .skip(1)
            // The job's body is indented past its key; the next job, or the
            // comment block before it, starts at two spaces.
            .take_while(|line| line.is_empty() || line.starts_with("   "))
            .collect();
        let job = body.join("\n");
        assert!(
            job.contains("    name: authoring budgets (ubuntu-24.04)\n"),
            "{job}"
        );
        assert!(job.contains("    runs-on: ubuntu-24.04\n"), "{job}");
        let minutes = |text: &str| -> u32 {
            text.trim()
                .strip_prefix("timeout-minutes: ")
                .and_then(|value| value.parse().ok())
                .unwrap_or_else(|| panic!("`{text}` is not a timeout"))
        };
        let job_timeout = job
            .lines()
            .find(|line| line.starts_with("    timeout-minutes: "))
            .map(minutes)
            .expect("the job's timeout");
        let steps: Vec<&str> = job.split("\n      - name: ").skip(1).collect();
        let mut sum = 0;
        for step in &steps {
            if let Some(line) = step
                .lines()
                .find(|line| line.trim_start().starts_with("timeout-minutes: "))
            {
                sum += minutes(line);
            }
        }
        for (at, alarm) in budget::BUDGETS.iter().enumerate() {
            let runs: Vec<&&str> = steps
                .iter()
                .filter(|step| step.contains(&format!("run: cargo xtask {}\n", alarm.command)))
                .collect();
            assert_eq!(
                runs.len(),
                1,
                "one step runs `cargo xtask {}`",
                alarm.command
            );
            let step = runs.first().expect("the step");
            assert!(
                !step.contains("continue-on-error"),
                "`{}` must be able to turn the job red: {step}",
                alarm.command
            );
            assert!(
                step.lines()
                    .any(|line| line.trim_start().starts_with("timeout-minutes: ")),
                "`{}` has no timeout: {step}",
                alarm.command
            );
            let guard = if at == 0 {
                "\n        if: steps.scope.outputs.prose_only != 'true'\n"
            } else {
                "\n        if: ${{ !cancelled() && steps.scope.outputs.prose_only != 'true' }}\n"
            };
            assert!(step.contains(guard), "`{}`: {step}", alarm.command);
        }
        assert!(
            sum < job_timeout,
            "the steps' timeouts sum to {sum} minutes, not below the job's {job_timeout}"
        );
    }

    /// Every alarm has its own step in the `perf alarms` job, which runs on all
    /// three operating systems: an alarm with no step publishes nothing, and
    /// two alarms in one step share its ten notices (item 149 (4)).
    #[test]
    fn every_alarm_has_its_own_step_on_every_runner() {
        let ci = include_str!("../../.github/workflows/ci.yml");
        assert!(ci.contains("os: [ubuntu-latest, windows-latest, macos-latest]"));
        for name in ALARMS.iter().map(|alarm| alarm.name).chain(["allocations"]) {
            let step = format!("run: cargo xtask perf-alarms {name}\n");
            assert_eq!(
                ci.matches(step.as_str()).count(),
                1,
                "ci.yml runs `cargo xtask perf-alarms {name}` in exactly one step"
            );
        }
    }

    /// An alarm's `exact` test exists in its target, `#[ignore]`d: a renamed
    /// test would leave libtest running nothing and the alarm silently empty.
    #[test]
    fn every_exact_alarm_names_an_ignored_test_of_its_target() {
        let root = workspace_root().expect("the workspace root");
        for alarm in ALARMS {
            let Some(name) = alarm.exact else { continue };
            let dir = alarm.package.trim_start_matches("pharmakos-");
            let path = root.join(format!("crates/{dir}/tests/{}.rs", alarm.test));
            let source = fs::read_to_string(&path).expect("the alarm's target");
            let lines: Vec<&str> = source.lines().map(str::trim).collect();
            let at = lines
                .iter()
                .position(|line| *line == format!("fn {name}() {{"))
                .unwrap_or_else(|| panic!("{} has no `{name}` test", path.display()));
            assert!(
                at >= 2
                    && lines
                        .get(at - 1)
                        .is_some_and(|line| line.starts_with("#[ignore"))
                    && lines.get(at - 2).is_some_and(|line| *line == "#[test]"),
                "`{name}` must be `#[test]` and `#[ignore]`d"
            );
        }
    }

    #[test]
    fn each_alarm_is_selected_by_its_own_name_and_a_typo_is_refused() {
        let names: Vec<&str> = ALARMS.iter().map(|alarm| alarm.name).collect();
        assert_eq!(
            names,
            [
                "mesher",
                "p1",
                "p1-cap",
                "p1-decision",
                "p1-kinds",
                "p3",
                "g3"
            ]
        );
        for alarm in ALARMS {
            assert_eq!(
                alarm_selection(alarm.name),
                Ok(AlarmSelection::Alarm(alarm.name))
            );
        }
        assert_eq!(
            alarm_selection("allocations"),
            Ok(AlarmSelection::Allocations)
        );
        assert!(alarm_selection("G3").is_err());
        let cli =
            |args: &[&str]| run_cli(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>());
        assert!(cli(&["perf-alarms", "g3", "p1"]).is_err());
        assert!(cli(&["perf-alarms", "nope"]).is_err());
        assert!(cli(&["g3-budget", "--quick"]).is_err());
        assert!(cli(&["g3-budget", "g3-budget"]).is_err());
        for alarm in budget::BUDGETS {
            assert_eq!(
                cli(&[alarm.command, "--quick"]),
                Err(format!(
                    "`{}` takes no arguments, and was given `--quick`",
                    alarm.command
                ))
            );
        }
        assert!(cli(&["p1-budget", "p3-budget"]).is_err());
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

    /// The split over this workspace's own metadata (decisions-log item
    /// 148 (2)): for the shard count CI runs and its neighbours, every unit of
    /// `test`, `test-research` and `scenario` falls to exactly one shard, so
    /// the shards' union is the unsharded set and nothing runs twice. The
    /// units are read the way a shard reads them, and the ones every run must
    /// hold are named, so a metadata reader that lost a kind is red here.
    #[test]
    fn the_shards_of_the_real_workspace_are_the_unsharded_set() {
        for total in 1..=4_u32 {
            for (step, keys) in &real_workspace_units() {
                let mut union: Vec<String> = Vec::new();
                for index in 1..=total {
                    let shard = match shard::Shard::parse(&format!("{index}/{total}")) {
                        Ok(shard) => shard,
                        Err(error) => panic!("{error}"),
                    };
                    union.extend(keys.iter().filter(|key| shard.holds(step, key)).cloned());
                }
                union.sort();
                let mut all = keys.clone();
                all.sort();
                assert_eq!(union, all, "{step} over {total} shard(s)");
            }
        }
    }

    /// `shard::SPLIT_SALT`'s rule over `shard::HEAVY_UNITS`, at the shard
    /// count ci.yml runs: each heavy unit, and each unit `shard::MEASURED`
    /// records, is a unit of this workspace (so a renamed one is red here, not
    /// a rule that silently stopped binding), there are no more heavy units
    /// than shards (no salt can set more apart: that needs more shards or a
    /// shorter unit, and the failure says so rather than sending the reader
    /// after a salt),
    /// each falls to a shard no other heavy unit falls to, and the heaviest
    /// does not fall to shard 1, which also runs every step that is not
    /// sharded. No salt below `SPLIT_SALT` meets the rule, so the salt is the
    /// one the rule picks and not a hand-tuned number.
    #[test]
    fn the_heavy_units_fall_apart_on_ci() {
        let total = ci_shards_total();
        let units = real_workspace_units();
        let measured = shard::MEASURED
            .iter()
            .flat_map(|measured| measured.units)
            .map(|(step, key, _)| (step, key));
        for (step, key) in shard::HEAVY_UNITS
            .iter()
            .map(|(step, key)| (step, key))
            .chain(measured)
        {
            let known = units
                .iter()
                .any(|(owner, keys)| owner == step && keys.iter().any(|owned| owned == key));
            assert!(known, "{step}/{key} is not a unit of this workspace");
        }
        // `None` is the split CI runs (`holds`); `Some(salt)` the same split
        // under another salt.
        let shards = |salt: Option<u32>| -> Vec<u32> {
            shard::HEAVY_UNITS
                .iter()
                .map(|(step, key)| {
                    let found: Vec<u32> = (1..=total)
                        .filter(
                            |index| match shard::Shard::parse(&format!("{index}/{total}")) {
                                Ok(shard) => match salt {
                                    None => shard.holds(step, key),
                                    Some(salt) => shard.holds_salted(salt, step, key),
                                },
                                Err(error) => panic!("{error}"),
                            },
                        )
                        .collect();
                    match found.as_slice() {
                        [one] => *one,
                        other => panic!("{step}/{key} falls to shards {other:?} of {total}"),
                    }
                })
                .collect()
        };
        let apart = |held: &[u32]| {
            let mut sorted = held.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            sorted.len() == held.len() && held.first().is_some_and(|first| *first != 1)
        };
        // Before any salt is tried: a salt only relabels the units, so no salt
        // sets more units apart than there are shards, and a search would
        // only fail with a message that sends the reader after a salt.
        let heavy = shard::HEAVY_UNITS.len();
        assert!(
            u32::try_from(heavy).is_ok_and(|heavy| heavy <= total),
            "HEAVY_RULE reads {heavy} heavy units {:?} over MEASURED, and CI runs {total} \
             shard(s): no salt can set more units apart than there are shards. The fixes are \
             more shards (ci.yml's CI_SHARDS and the shard matrices, a workflow and \
             required-check change, which is the owner's) or a heavy unit made shorter",
            shard::HEAVY_UNITS
        );
        let held = shards(None);
        assert!(
            apart(&held),
            "with salt {} the heavy units {:?} fall to shards {held:?} of {total}",
            shard::SPLIT_SALT,
            shard::HEAVY_UNITS
        );
        let smallest = (0..=shard::SPLIT_SALT).find(|salt| apart(&shards(Some(*salt))));
        assert_eq!(
            smallest,
            Some(shard::SPLIT_SALT),
            "SPLIT_SALT is the smallest salt that sets the heavy units apart"
        );
    }

    /// The shard count ci.yml's `CI_SHARDS` sets.
    fn ci_shards_total() -> u32 {
        let ci = include_str!("../../.github/workflows/ci.yml");
        let totals: Vec<&str> = ci
            .lines()
            .map(str::trim)
            .filter_map(|line| line.strip_prefix("CI_SHARDS: "))
            .collect();
        match totals.as_slice() {
            [one] => match one.trim_matches('"').parse() {
                Ok(total) if total >= 1 => total,
                other => panic!("CI_SHARDS is not a shard count: {other:?}"),
            },
            other => panic!("CI_SHARDS is set once: {other:?}"),
        }
    }

    /// Every unit of the three sharded steps, keyed by step, read the way a
    /// shard reads them; the ones every run must hold are named, so a metadata
    /// reader that lost a kind is red.
    fn real_workspace_units() -> [(&'static str, Vec<String>); 3] {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let Some(root) = manifest.parent() else {
            panic!("xtask's manifest directory has no parent");
        };
        let workspace = match load_workspace(false, env!("CARGO"), root) {
            Ok(workspace) => workspace,
            Err(reason) => panic!("{reason}"),
        };
        let names: Vec<String> = workspace
            .packages
            .iter()
            .map(|package| package.name.clone())
            .collect();
        let test_units = match workspace_test_units(&workspace, &names) {
            Ok(units) => units,
            Err(reason) => panic!("{reason}"),
        };
        for wanted in [
            shard::Unit::Lib,
            shard::Unit::Doc,
            shard::Unit::Bin("xtask".to_owned()),
            shard::Unit::Bin("gamectl".to_owned()),
            shard::Unit::Test("determinism".to_owned()),
            shard::Unit::Test("scenarios".to_owned()),
        ] {
            assert!(
                test_units.contains(&wanted),
                "{wanted:?} is not a test unit"
            );
        }
        let research_units = match shard::units(
            workspace
                .packages
                .iter()
                .filter(|package| package.name == "pharmakos-sim")
                .flat_map(|package| {
                    package
                        .targets
                        .iter()
                        .map(|target| (package.name.as_str(), target))
                }),
        ) {
            Ok(units) => units,
            Err(error) => panic!("{error}"),
        };
        assert!(research_units.contains(&shard::Unit::Test("determinism".to_owned())));
        let scenarios: Vec<String> = match scenario::collect_played(root) {
            Ok(paths) => paths
                .iter()
                .filter_map(|path| path.strip_prefix(root).ok())
                .map(scenario_shard_key)
                .collect(),
            Err(reason) => panic!("{reason}"),
        };
        assert!(!scenarios.is_empty(), "no scenario is played");

        [
            ("test", test_units.iter().map(shard::Unit::key).collect()),
            (
                "test-research",
                research_units.iter().map(shard::Unit::key).collect(),
            ),
            ("scenario", scenarios),
        ]
    }

    /// ci.yml's split agrees with itself: each operating system's `ci-shard-<os>`
    /// matrix lists shard 1 to `CI_SHARDS` exactly, and each shard runs `cargo xtask ci --shard n/CI_SHARDS`,
    /// so the leg job's count of reports and the shards' own `N/M` cannot drift
    /// apart (decisions-log item 148 (2)).
    #[test]
    fn the_ci_shards_are_the_matrix() {
        let ci = include_str!("../../.github/workflows/ci.yml");
        let value = |prefix: &str| -> Vec<String> {
            ci.lines()
                .map(str::trim)
                .filter_map(|line| line.strip_prefix(prefix))
                .map(str::to_owned)
                .collect()
        };
        let totals = value("CI_SHARDS: ");
        assert_eq!(totals.len(), 1, "CI_SHARDS is set once: {totals:?}");
        let total: u32 = match totals.first().map(|text| text.trim_matches('"').parse()) {
            Some(Ok(total)) => total,
            other => panic!("CI_SHARDS is not a number: {other:?}"),
        };
        assert!(total >= 1);
        let lists = value("shard: [");
        assert_eq!(
            lists.len(),
            3,
            "one shard matrix per operating system: {lists:?}"
        );
        for list in &lists {
            let Some(items) = list.strip_suffix(']') else {
                panic!("shard matrix `{list}` does not close on its line");
            };
            let listed: Vec<u32> = items
                .split(',')
                .map(|item| match item.trim().parse() {
                    Ok(number) => number,
                    Err(error) => panic!("shard matrix entry `{item}`: {error}"),
                })
                .collect();
            assert_eq!(listed, (1..=total).collect::<Vec<u32>>(), "{list}");
        }
        assert!(
            ci.contains(r#"cargo xtask ci --shard "${{ matrix.shard }}/${CI_SHARDS}""#),
            "each shard runs its own `--shard n/CI_SHARDS`"
        );
    }

    /// The split spreads by name, through the paths the steps themselves
    /// take: `shard_part` for `test` and `test-research` (with each unit's
    /// own key, its name included), and `scenario_on_shard` for `scenario`,
    /// all under the committed `SPLIT_SALT`. Each step's pinned units lie on
    /// all three shards, so a key that lost its name, a step hashed without
    /// the salt, or a split that stopped spreading moves one of them.
    /// Expected values: the committed split at 3 shards; a new salt or a
    /// renamed unit moves them, and this table moves in the same commit.
    #[test]
    fn named_units_fall_where_the_salted_hash_puts_them() {
        let total = 3_u32;
        let shard_of = |index: u32| match shard::Shard::parse(&format!("{index}/{total}")) {
            Ok(shard) => shard,
            Err(error) => panic!("{error}"),
        };
        let unit = |key: &str| match key {
            "lib" => shard::Unit::Lib,
            "doc" => shard::Unit::Doc,
            other => match other.split_once(':') {
                Some(("bin", name)) => shard::Unit::Bin(name.to_owned()),
                Some(("test", name)) => shard::Unit::Test(name.to_owned()),
                _ => panic!("`{other}` is not a unit key"),
            },
        };
        let pinned: [(&str, &[(&str, u32)]); 2] = [
            (
                "test",
                &[
                    ("test:advice", 1),
                    ("test:attack", 1),
                    ("bin:determinism", 2),
                    ("test:allocations", 2),
                    ("lib", 3),
                    ("bin:xtask", 3),
                ],
            ),
            (
                "test-research",
                &[
                    ("test:allocations", 1),
                    ("test:build", 1),
                    ("lib", 2),
                    ("bin:determinism", 2),
                    ("test:audit", 3),
                    ("test:combat", 3),
                ],
            ),
        ];
        for (step, cases) in pinned {
            let all: Vec<shard::Unit> = cases.iter().map(|(key, _)| unit(key)).collect();
            for (key, expected) in cases {
                let found: Vec<u32> = (1..=total)
                    .filter(|index| shard_part(shard_of(*index), step, &all).contains(&unit(key)))
                    .collect();
                assert_eq!(found, vec![*expected], "{step}/{key}");
            }
        }
        for (relative, expected) in [
            ("scenarios/s1/brownout-by-recycle.scenario.jsonc", 1),
            ("scenarios/s1/cover-nearest-vent.scenario.jsonc", 1),
            ("scenarios/s2/guards-escort-the-commander.scenario.jsonc", 2),
            ("scenarios/s2/mortar-breaks-wall.scenario.jsonc", 2),
            ("scenarios/s1/round-limit-audit.scenario.jsonc", 3),
            ("scenarios/s2/base-kit-at-the-core.scenario.jsonc", 3),
        ] {
            let path: PathBuf = relative.split('/').collect();
            let found: Vec<u32> = (1..=total)
                .filter(|index| scenario_on_shard(Some(shard_of(*index)), &path))
                .collect();
            assert_eq!(found, vec![expected], "{relative}");
            assert!(scenario_on_shard(None, &path), "{relative} unsharded");
        }
    }

    /// The leg job's half of the split: the `cargo xtask ci (<os>)` job, the
    /// required check, reads a report from every shard 1 to `CI_SHARDS` and
    /// counts as failed a shard that sent none, one that named another split,
    /// one that failed, and, off the fast path, one that did not run and pass
    /// its suite; a shard that ran the suite takes the leg off the fast path;
    /// and the job's last step, which always runs, fails on any failed shard.
    /// This pins the lines that do it, in order, so that narrowing the loop,
    /// dropping or inverting a check, or letting the last step be skipped is
    /// red here, while a comment or a blank line between them is not. It reads
    /// the script's text rather than running it: xtask takes no dependencies,
    /// and on Windows a bare `bash` can resolve to WSL's launcher rather than
    /// Git Bash, so a test that spawned one would not test the same thing on
    /// every machine.
    #[test]
    fn the_leg_job_requires_every_shards_report() {
        let ci = include_str!("../../.github/workflows/ci.yml");
        let report_step = step_lines(ci, "Read every shard's report");
        let every_shard = r#"for n in $(seq 1 "${CI_SHARDS}"); do"#;
        let report = r#"report="shards/shard-${{ matrix.os }}-${n}/status.txt""#;
        let failed = r#"failed="${failed} ${n}""#;
        for (what, wanted) in [
            (
                "the step starts from no failure, on the fast path until a shard says otherwise",
                vec![r#"failed="""#, "prose=true", r#"prose_shards="""#],
            ),
            (
                "a shard with no report fails",
                vec![
                    every_shard,
                    report,
                    r#"if [ ! -f "$report" ]; then"#,
                    "~sent no report",
                    failed,
                    "prose=false",
                    "continue",
                ],
            ),
            (
                "a report that names another split fails",
                vec![
                    r#"if ! grep -qx "shard=${n}/${CI_SHARDS}" "$report"; then"#,
                    "~names another split",
                    failed,
                ],
            ),
            (
                "a failed shard fails",
                vec![
                    r#"elif ! grep -qx 'status=success' "$report"; then"#,
                    "~failed; its log is",
                    failed,
                ],
            ),
            (
                "a shard that did not answer prose takes the leg off the fast path",
                vec![
                    r#"if grep -qx 'prose_only=true' "$report"; then"#,
                    r#"prose_shards="${prose_shards} ${n}""#,
                    "else",
                    "prose=false",
                    "fi",
                    "done",
                ],
            ),
            (
                "off the fast path, a shard that did not run and pass its suite fails",
                vec![
                    r#"if [ "$prose" = "false" ]; then"#,
                    every_shard,
                    report,
                    r#"[ -f "$report" ] || continue"#,
                    r#"if ! grep -qx 'suite=success' "$report"; then"#,
                    "~did not run and pass its suite",
                    failed,
                ],
            ),
            (
                "the fast path and the failed shards are the step's outputs",
                vec![
                    r#"echo "prose_only=${prose}" >> "$GITHUB_OUTPUT""#,
                    r#"echo "failed=${failed}" >> "$GITHUB_OUTPUT""#,
                ],
            ),
        ] {
            assert!(
                has_run(&report_step, &wanted),
                "the leg job's report check: {what}"
            );
        }
        let last_step = step_lines(ci, "Every shard of this leg passed");
        assert!(
            has_run(
                &last_step,
                &[
                    "if: always()",
                    "shell: bash",
                    "run: |",
                    r#"if [ "${{ steps.shards.outcome }}" != "success" ] || [ -n "${{ steps.shards.outputs.failed }}" ]; then"#,
                    "~did not pass, or their reports could not be read",
                    "exit 1",
                    "fi",
                ],
            ),
            "the leg job's last step always runs and fails on any failed shard"
        );
    }

    /// The lines of the one ci.yml step named `name`, from its `- name:` line
    /// to the next step's (or the file's end), trimmed, without blank lines
    /// and without lines that are only a comment.
    fn step_lines<'a>(ci: &'a str, name: &str) -> Vec<&'a str> {
        let heading = format!("- name: {name}");
        let lines: Vec<&str> = ci.lines().map(str::trim).collect();
        let starts: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| **line == heading)
            .map(|(index, _)| index)
            .collect();
        let [start] = starts.as_slice() else {
            panic!(
                "ci.yml has {} step(s) named `{name}`, not one",
                starts.len()
            );
        };
        let Some(rest) = lines.get(*start..).and_then(|tail| tail.get(1..)) else {
            panic!("the step `{name}` is ci.yml's last line");
        };
        rest.iter()
            .take_while(|line| !line.starts_with("- name: "))
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .copied()
            .collect()
    }

    /// Whether `lines` holds `wanted` as consecutive lines; a leading `~` in
    /// `wanted` matches a substring.
    fn has_run(lines: &[&str], wanted: &[&str]) -> bool {
        lines.windows(wanted.len()).any(|window| {
            window
                .iter()
                .zip(wanted)
                .all(|(line, want)| match want.strip_prefix('~') {
                    Some(part) => line.contains(part),
                    None => line == want,
                })
        })
    }

    #[test]
    fn a_shard_runs_its_part_of_the_slow_steps_and_shard_one_the_rest() {
        let first = shard::Shard::parse("1/3").ok();
        let second = shard::Shard::parse("2/3").ok();
        assert!(first.is_some() && second.is_some());
        for step in STEPS {
            assert!(
                step_in_shard(step.name, None),
                "{} runs unsharded",
                step.name
            );
            let sharded = shard::SHARDED_STEPS.contains(&step.name);
            let merged = shard::MERGED_STEPS.contains(&step.name);
            assert_eq!(step_in_shard(step.name, first), !merged, "{}", step.name);
            assert_eq!(step_in_shard(step.name, second), sharded, "{}", step.name);
        }
        for name in shard::SHARDED_STEPS.iter().chain(shard::MERGED_STEPS) {
            assert!(
                STEPS.iter().any(|step| step.name == *name),
                "the split names a step that does not exist: {name}"
            );
        }
        let ci = ["ci".to_owned()];
        assert!(shard_flags_allowed(&ci, &[], false, false, false).is_ok());
        assert!(shard_flags_allowed(&["test".to_owned()], &[], false, false, false).is_err());
        assert!(
            shard_flags_allowed(&ci, &["pharmakos-sim".to_owned()], false, false, false).is_err()
        );
        assert!(shard_flags_allowed(&ci, &[], true, false, false).is_err());
        assert!(shard_flags_allowed(&ci, &[], false, true, false).is_err());
        assert!(shard_flags_allowed(&ci, &[], false, false, true).is_err());
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
    fn package_specs_are_names_and_a_glob_is_refused() {
        assert_eq!(package_spec_name("pharmakos-sim"), Ok("pharmakos-sim"));
        assert_eq!(
            package_spec_name("pharmakos-sim@0.1.0"),
            Ok("pharmakos-sim")
        );
        assert_eq!(
            package_spec_name("pharmakos-sim-extra"),
            Ok("pharmakos-sim-extra")
        );
        for spec in [
            "x*",
            "[p]",
            "a#b",
            "pharmakos-si?",
            "path+file:///x",
            "",
            "@1.0",
        ] {
            assert!(package_spec_name(spec).is_err(), "{spec:?}");
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
        let scan = scan_profiles(manifest);
        assert!(scan.release_checked);
        assert!(scan.overflow_off.is_empty());
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
        let scan = scan_profiles(broken);
        assert!(scan.release_checked);
        assert_eq!(scan.overflow_off, ["profile.release-client"]);
    }

    #[test]
    fn ci_scope_is_not_a_step() {
        assert!(STEPS.iter().all(|step| step.name != "ci-scope"));
    }

    /// The machine-wide pre-push gate (decisions-log item 132): a clone runs
    /// the hooks kept in `scripts/githooks` (README, "Setting up a clone"), so
    /// an unwired clone fails here rather than pushing unchecked. A CI runner's
    /// checkout carries no hooks, so the test answers for a developer's clone
    /// only: it is skipped when `CI` or `GITHUB_ACTIONS` holds a value other
    /// than empty, `0` or `false` (`claude.yml` runs `--quick` with a scrubbed
    /// environment, which `GITHUB_ACTIONS` covers if `CI` is scrubbed).
    #[test]
    fn the_clone_runs_the_versioned_hooks() {
        let set = |name: &str| {
            env::var(name).is_ok_and(|value| !matches!(value.trim(), "" | "0" | "false"))
        };
        if set("CI") || set("GITHUB_ACTIONS") {
            return;
        }
        let root = workspace_root().expect("the workspace root is found");
        let git = |args: &[&str]| {
            let mut command = Command::new("git");
            command.args(args).current_dir(&root).stdin(Stdio::null());
            // The clone's own config, never a redirected one (as `scope.rs` does).
            for name in ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR", "GIT_CONFIG"] {
                command.env_remove(name);
            }
            command.output().expect("git runs")
        };
        let output = git(&["config", "--get", "core.hooksPath"]);
        // `git config --get` exits 1 when the key is unset; anything else is
        // not a clone this test can judge (an exported tree, a broken repo).
        match output.status.code() {
            Some(0 | 1) => {}
            other => panic!(
                "`git config --get core.hooksPath` failed ({other:?}): {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        }
        let hooks_path = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            hooks_path.trim(),
            "scripts/githooks",
            "this clone does not run the versioned hooks; wire it once with `git config core.hooksPath scripts/githooks`"
        );
        // Git runs a hook only if it is executable, and on Linux and macOS a
        // hook that lost its mode is skipped with a hint, so the index must
        // hold both at 100755.
        let staged = git(&["ls-files", "-s", "scripts/githooks"]);
        let staged = String::from_utf8_lossy(&staged.stdout);
        for hook in ["pre-commit", "pre-push"] {
            let path = format!("scripts/githooks/{hook}");
            assert!(
                staged
                    .lines()
                    .any(|line| line.starts_with("100755 ") && line.ends_with(&path)),
                "{path} is not tracked as executable (100755): {staged}"
            );
        }
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

    /// The test files that still read `CARGO_TARGET_DIR` raw, each with how
    /// many of its lines do and the lane that owes the change (decisions-log
    /// item 165 (13)): none since S3's `prob` moved the client's two to the
    /// parent of `CARGO_TARGET_TMPDIR` (item 172 (8) (b)). An entry fails
    /// [`no_test_reads_the_target_dir_variable_raw`] when its file's count
    /// differs from it: a second raw read added to an owed file is not owed,
    /// and a file that stops reading the variable has its entry deleted by the
    /// lane that fixes it.
    const RAW_TARGET_DIR_OWED: &[(&str, usize, &str)] = &[];

    /// `text`, Rust source, with its comments blanked: a line comment to its
    /// line's end and a block comment (nested, as Rust nests them) to its
    /// close, each character replaced by a space and its newlines kept, so
    /// line numbers hold. String literals, raw strings and character literals
    /// are kept whole and their contents never open a comment: `"http://x"`
    /// hides nothing after it. A lifetime (`'a`) is not a character literal.
    fn rust_code(text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let at = |index: usize| chars.get(index).copied();
        let mut out = String::with_capacity(text.len());
        let mut index = 0;
        while let Some(character) = at(index) {
            match (character, at(index + 1)) {
                ('/', Some('/')) => {
                    while let Some(next) = at(index) {
                        if next == '\n' {
                            break;
                        }
                        blank_char(&mut out, next);
                        index += 1;
                    }
                }
                ('/', Some('*')) => {
                    let mut depth = 0_usize;
                    while let Some(next) = at(index) {
                        match (next, at(index + 1)) {
                            ('/', Some('*')) => {
                                depth += 1;
                                out.push_str("  ");
                                index += 2;
                            }
                            ('*', Some('/')) => {
                                depth -= 1;
                                out.push_str("  ");
                                index += 2;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {
                                blank_char(&mut out, next);
                                index += 1;
                            }
                        }
                    }
                }
                ('r', Some('"' | '#')) if raw_string_at(&chars, index) => {
                    // `r`, the hashes, the quote; the string ends at a quote
                    // followed by as many hashes.
                    let hashes = chars
                        .iter()
                        .skip(index + 1)
                        .take_while(|next| **next == '#')
                        .count();
                    let mut end = index + 2 + hashes;
                    while let Some(next) = at(end) {
                        end += 1;
                        let closing = chars
                            .iter()
                            .skip(end)
                            .take(hashes)
                            .filter(|after| **after == '#')
                            .count();
                        if next == '"' && closing == hashes {
                            end += hashes;
                            break;
                        }
                    }
                    out.extend(chars.iter().take(end).skip(index));
                    index = end;
                }
                ('"', _) => {
                    out.push('"');
                    index += 1;
                    while let Some(next) = at(index) {
                        out.push(next);
                        index += 1;
                        if next == '\\' {
                            if let Some(escaped) = at(index) {
                                out.push(escaped);
                                index += 1;
                            }
                        } else if next == '"' {
                            break;
                        }
                    }
                }
                ('\'', Some('\\')) => {
                    // An escaped character literal, up to its closing quote.
                    out.push('\'');
                    out.push('\\');
                    index += 2;
                    while let Some(next) = at(index) {
                        out.push(next);
                        index += 1;
                        if next == '\'' {
                            break;
                        }
                    }
                }
                ('\'', Some(literal)) if at(index + 2) == Some('\'') => {
                    out.push('\'');
                    out.push(literal);
                    out.push('\'');
                    index += 3;
                }
                _ => {
                    out.push(character);
                    index += 1;
                }
            }
        }
        out
    }

    /// A commented-out character: a newline kept, anything else as many
    /// spaces as its bytes.
    fn blank_char(out: &mut String, character: char) {
        if character == '\n' {
            out.push('\n');
        } else {
            out.extend(std::iter::repeat_n(' ', character.len_utf8()));
        }
    }

    /// Whether a raw string literal opens at `index` (`r"`, `r#"`, ...)
    /// rather than an identifier ending in `r`.
    fn raw_string_at(chars: &[char], index: usize) -> bool {
        let starts_word = index
            .checked_sub(1)
            .and_then(|before| chars.get(before))
            .is_none_or(|before| !(before.is_alphanumeric() || *before == '_'));
        let quote = chars
            .iter()
            .skip(index + 1)
            .find(|next| **next != '#')
            .is_some_and(|next| *next == '"');
        starts_word && quote
    }

    /// The 1-based numbers of the lines of `text`, Rust source, whose code
    /// (comments blanked by [`rust_code`]) names `CARGO_TARGET_DIR`.
    fn raw_target_dir_lines(text: &str) -> Vec<usize> {
        rust_code(text)
            .lines()
            .zip(1..)
            .filter(|(line, _)| line.contains("CARGO_TARGET_DIR"))
            .map(|(_, number)| number)
            .collect()
    }

    #[test]
    fn a_raw_read_is_found_and_a_comment_is_not() {
        let text = "/// honours `CARGO_TARGET_DIR`\n\
                    fn a() {}\n\
                    let dir = std::env::var_os(\"CARGO_TARGET_DIR\");\n\
                    let tmp = env!(\"CARGO_TARGET_TMPDIR\"); // not CARGO_TARGET_DIR\n";
        assert_eq!(raw_target_dir_lines(text), vec![3]);
    }

    /// A `//` inside a string, a raw string or a character literal opens no
    /// comment, so a read after it on the line is still found (the review's
    /// probe T5); a block comment, nested or across lines, hides what it
    /// holds; a lifetime is not a character literal.
    #[test]
    fn the_reader_knows_strings_from_comments() {
        let text = [
            r#"fn f() { let u = "http://x"; let _ = std::env::var_os("CARGO_TARGET_DIR"); }"#,
            r##"let r = r#"a // "b" "#; let _ = env::var("CARGO_TARGET_DIR");"##,
            r#"let c = '"'; let _ = env::var("CARGO_TARGET_DIR");"#,
            r#"let e = '\''; let _ = env::var("CARGO_TARGET_DIR");"#,
            r#"fn g<'a>(x: &'a str) { let _ = env::var("CARGO_TARGET_DIR"); }"#,
            r"/* CARGO_TARGET_DIR /* nested */ CARGO_TARGET_DIR",
            r#"   CARGO_TARGET_DIR */ let s = "\"// a string"; let _ = env::var("CARGO_TARGET_DIR");"#,
            r#"let p = "a\\"; // CARGO_TARGET_DIR"#,
            r#"let q = r"CARGO_TARGET_DIR";"#,
        ]
        .join("\n");
        assert_eq!(raw_target_dir_lines(&text), vec![1, 2, 3, 4, 5, 7, 9]);
        assert_eq!(rust_code(&text).len(), text.len(), "offsets and lines hold");
    }

    /// The verdict over the raw reads found, against the owed list: every
    /// owed file's count must equal its entry's, and a file with no entry may
    /// have none.
    fn raw_target_dir_verdict(
        readers: &[(String, usize)],
        owed: &[(&str, usize, &str)],
    ) -> Result<(), String> {
        let mut problems = Vec::new();
        for (file, line) in readers {
            if !owed.iter().any(|(owed_file, _, _)| owed_file == file) {
                problems.push(format!(
                    "{file}:{line} reads CARGO_TARGET_DIR raw; take the parent of \
                     env!(\"CARGO_TARGET_TMPDIR\") instead"
                ));
            }
        }
        for (owed_file, count, lane) in owed {
            let found = readers.iter().filter(|(file, _)| file == owed_file).count();
            if found != *count {
                problems.push(format!(
                    "{owed_file} reads CARGO_TARGET_DIR raw on {found} line(s), and \
                     RAW_TARGET_DIR_OWED owes {count} to {lane}: a new raw read is not owed, \
                     and a file that reads it on fewer lines updates or deletes its entry"
                ));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("\n"))
        }
    }

    #[test]
    fn the_owed_list_is_kept_to_its_counts() {
        let owed = [("crates/x/tests/a.rs", 1, "a lane")];
        let one = vec![("crates/x/tests/a.rs".to_owned(), 80)];
        assert_eq!(raw_target_dir_verdict(&one, &owed), Ok(()));
        // The review's probe T8: a second raw read in an owed file.
        let two = vec![
            ("crates/x/tests/a.rs".to_owned(), 80),
            ("crates/x/tests/a.rs".to_owned(), 108),
        ];
        assert!(
            raw_target_dir_verdict(&two, &owed).is_err_and(|error| error.contains("2 line(s)"))
        );
        assert!(raw_target_dir_verdict(&[], &owed).is_err_and(|error| error.contains("0 line(s)")));
        let other = vec![
            ("crates/x/tests/a.rs".to_owned(), 80),
            ("crates/y/tests/b.rs".to_owned(), 3),
        ];
        assert!(raw_target_dir_verdict(&other, &owed).is_err_and(|error| error.contains("b.rs:3")));
    }

    /// A test that reads `CARGO_TARGET_DIR` itself is misled by a relative
    /// value (cargo resolves it against its own working folder, the test runs
    /// in its crate's) and by a user-level `build.target-dir` with the
    /// variable unset. The target directory is the parent of
    /// `CARGO_TARGET_TMPDIR`, which cargo sets absolute (item 165 (13)).
    #[test]
    fn no_test_reads_the_target_dir_variable_raw() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("crates");
        let mut files = Vec::new();
        walk(&crates, &mut files).expect("crates/");
        let mut readers: Vec<(String, usize)> = Vec::new();
        let mut scanned = 0_usize;
        for file in &files {
            // `<member>/tests/**/*.rs`, as `crates/<member>/tests/...`.
            let parts: Vec<String> = file
                .strip_prefix(&crates)
                .unwrap_or_else(|error| panic!("{}: {error}", file.display()))
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            let in_tests = parts.get(1).is_some_and(|part| part == "tests");
            if !in_tests || file.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            scanned += 1;
            let text = fs::read_to_string(file)
                .unwrap_or_else(|error| panic!("reading {}: {error}", file.display()));
            let relative = format!("crates/{}", parts.join("/"));
            readers.extend(
                raw_target_dir_lines(&text)
                    .into_iter()
                    .map(|line| (relative.clone(), line)),
            );
        }
        assert!(
            scanned > 50,
            "only {scanned} test files found under crates/*/tests"
        );
        if let Err(problems) = raw_target_dir_verdict(&readers, RAW_TARGET_DIR_OWED) {
            panic!("{problems}");
        }
    }

    /// The `#` comment block that ends on the line before the first line of
    /// `text` equal, trimmed, to `anchor`, as one line of words.
    fn comment_above(text: &str, anchor: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let at = lines
            .iter()
            .position(|line| line.trim() == anchor)
            .unwrap_or_else(|| panic!("no line `{anchor}`"));
        let block: Vec<&str> = lines
            .iter()
            .take(at)
            .rev()
            .map(|line| line.trim())
            .take_while(|line| line.starts_with('#'))
            .map(|line| line.trim_start_matches('#').trim())
            .collect();
        block
            .into_iter()
            .rev()
            .flat_map(str::split_whitespace)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The owner ratified the `[profile.dev.package.*]` tables on 2026-11-21
    /// (decisions-log item 168 (1)), so the two comments that recorded the
    /// change as the main session's ruling "which the owner can veto" cite the
    /// ratification instead, and a revert of the correction fails here.
    #[test]
    fn the_profile_comments_cite_the_ratification() {
        for (name, block) in [
            (
                "Cargo.toml",
                comment_above(
                    include_str!("../../Cargo.toml"),
                    "[profile.dev.package.pharmakos-sim]",
                ),
            ),
            (
                ".github/workflows/ci.yml",
                comment_above(
                    include_str!("../../.github/workflows/ci.yml"),
                    "timeout-minutes: 90",
                ),
            ),
        ] {
            assert!(block.contains("item 168 (1)"), "{name}: {block}");
            assert!(block.contains("item 165 (1)"), "{name}: {block}");
            assert!(!block.contains("owner can veto"), "{name}: {block}");
            assert!(
                !block.contains("records this change says so"),
                "{name}: {block}"
            );
        }
    }
}

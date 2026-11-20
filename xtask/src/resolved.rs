// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What cargo builds for a checked invocation, read from cargo itself.
//!
//! The suite runs the deterministic sim optimised, with debug assertions and
//! overflow checks on: at opt-level 0 its integer code ran 25 to 50 times
//! slower, which held CI's shards at 35 to 54 of 90 minutes. A reading of the
//! TOML cannot hold that in place: cargo also takes dotted keys, inline tables,
//! `--config` on the command line and config files outside the repository, and
//! a reader of the text would have to be cargo to see them all. So every build
//! of workspace code that xtask makes to test, to hash, to measure for a gate
//! or to ship goes through the one checked call [`checked`]:
//!
//! * every `cargo test` it runs (the `test` and `test-research` steps,
//!   `checked-test`, which the nightly fuzz job runs, and the alarms'
//!   measurements) goes through [`execute`], which builds the invocation's
//!   [`TestRun`] with `--no-run` and a JSON report first; a doctest invocation,
//!   which cargo will not build without running, is checked through the
//!   test-profile build of the same libraries ([`doc_check`]);
//! * the `determinism` step's and `determinism-run`'s `release-checked` build,
//!   the `scenario` step's and `build-gamectl`'s `gamectl`, the client the
//!   `stage-client` and `screenshot` steps stage, and `cargo xtask package`'s
//!   two release builds are read from their own JSON report, as they are made,
//!   and what runs or ships is the artefact that report names.
//!
//! [`faults`] reads the profile cargo RESOLVED for every unit it built, fresh
//! units included, against an [`Expect`]:
//!
//! * overflow checks on in every workspace unit, always;
//! * debug assertions on in every workspace unit where the profile promises
//!   them (the suite's and `release-checked`);
//! * for `release-checked`, `[profile.release]`'s optimisation in every
//!   workspace unit and every crate the sim runs, and the sim's library built;
//! * for the suite, `pharmakos-sim` optimised, every third-party crate the sim
//!   runs optimised, no crate of the godot family optimised, and no other
//!   third-party crate unoptimised.
//!
//! A build script runs on the host at the build-override's level and is
//! judged by no optimisation rule. The compiler's diagnostics travel in the
//! same JSON report, read a line at a time as cargo writes it ([`Report`]),
//! and each is written once for an invocation's check and run together, as it
//! arrives, so a failed build shows its errors.
//!
//! # What changes codegen behind the report
//!
//! Rustflags and rustdocflags override the profile and never show in the
//! resolved JSON: `RUSTFLAGS="-C debug-assertions=off"` builds a unit the JSON
//! still reports with assertions on. So do a rustc wrapper, which can add any
//! flag; another compiler or doc compiler; and `RUSTC_BOOTSTRAP`, which turns
//! on cargo's unstable `profile-rustflags`. So a checked call takes all of
//! these from nowhere but its own value ([`CargoCall::pinned`]):
//!
//! * `--config 'build.rustc="rustc"'` and `--config 'build.rustdoc="rustdoc"'`
//!   ([`pinned_config`]): a command line's `--config` overrides every config
//!   file, and the plain names are resolved by the rustup proxy on `PATH` to the
//!   pinned toolchain's tools.
//! * `CARGO_ENCODED_RUSTFLAGS` and `CARGO_ENCODED_RUSTDOCFLAGS` are set to
//!   exactly what that build passes: nothing, except the Windows package's
//!   `+crt-static`. Cargo takes rustflags from the first of
//!   `CARGO_ENCODED_RUSTFLAGS`, `RUSTFLAGS`, the `target.*.rustflags` entries
//!   and `build.rustflags` that is set, so a set `CARGO_ENCODED_RUSTFLAGS`, even
//!   empty, is the only source, and every config file's flags are ignored. A
//!   profile's own `rustflags` key is unstable and needs `RUSTC_BOOTSTRAP` or a
//!   nightly cargo.
//! * `RUSTC_WRAPPER` and `RUSTC_WORKSPACE_WRAPPER` are set empty, which turns
//!   off a config file's `build.rustc-wrapper` and
//!   `build.rustc-workspace-wrapper`.
//! * `RUSTC_BOOTSTRAP` is removed.
//!
//! And each variable that would change these, set in the environment, which
//! the caller meant to reach the build and which this would silently override,
//! is refused by name before anything builds ([`refused_variables`]): the
//! flags variables, `RUSTC`, `CARGO_BUILD_RUSTC`, `RUSTDOC`,
//! `CARGO_BUILD_RUSTDOC`, the wrapper variables, `RUSTC_BOOTSTRAP`, and every
//! `CARGO_TARGET_<triple>_` variable that sets rustflags, rustdocflags, a
//! runner or a linker. Cargo itself sets none of them for a nested `cargo
//! run`, and neither the versioned hooks nor `wsl-ci.sh` set one. On Windows,
//! where a variable's name is read without regard to case, so is the refusal.
//!
//! # What ran
//!
//! A config file's `runner` cannot be pinned empty (cargo refuses `""`), so a
//! test run proves its tests ran: every test executable the check reported
//! must be started (cargo's `Running` line) and every start must end in
//! libtest's `test result:` line, or the run is
//! [`ProfileFault::TestsDidNotRun`] ([`judge_run`]). A doctest run is held to
//! its exit status alone: libtest's output does not tie a doctest result to the
//! crate it came from. A runner that wraps `cargo xtask` itself and fakes its
//! exit status is outside what xtask can see; CI's clean checkout has no config
//! file but the repository's.
//!
//! # The allocation test runs the sim unoptimised
//!
//! `crates/sim/tests/allocations.rs`'s `a_tick_allocates_nothing` counts the
//! allocations a tick makes. The optimiser deletes an allocation whose value
//! is never read (a `Vec::with_capacity` dropped at once, a `Box` freed where
//! it was made), so at opt-level 2 a pull request that adds such code to a tick
//! phase passes the test, and its claim, that such a pull request fails there,
//! no longer holds. The allocation test therefore runs as an invocation of its
//! own with the sim at opt-level 0, through cargo's `--config`, in whichever
//! shard its unit falls to; the unit list, and so the split, is the same as
//! before. The invocation selects every target of that name, so it is exempt
//! for exactly the packages [`UNOPTIMISED_OWNERS`] names.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::Json;
use crate::shard::Unit;

/// The package the suite runs optimised.
pub(crate) const SIM: &str = "pharmakos-sim";

/// The integration tests that run with [`SIM`] at opt-level 0, by target name,
/// each as an invocation of its own: a test whose claim the optimiser can
/// weaken. Nothing else is exempted from [`faults`].
pub(crate) const UNOPTIMISED_TESTS: &[&str] = &["allocations"];

/// Every package that owns a test target named in [`UNOPTIMISED_TESTS`], with
/// the reason it rides in that invocation: `cargo test --test <name>` selects
/// every target of the name, so the exemption is for these (package, target)
/// pairs and no other. `main.rs`'s `the_unoptimised_tests_have_the_owners_the_table_names`
/// holds the workspace to this table.
pub(crate) const UNOPTIMISED_OWNERS: &[(&str, &str, &str)] = &[
    (
        "allocations",
        SIM,
        "the test the invocation exists for: it counts the allocations of the tick as written",
    ),
    (
        "allocations",
        "pharmakos-mesher",
        "selected with the sim's by its name; it does not depend on the sim, so the sim's level \
         cannot reach it",
    ),
];

/// The `--config` value that builds [`SIM`] at opt-level 0.
const SIM_UNOPTIMISED: &str = "profile.dev.package.pharmakos-sim.opt-level=0";

/// The `--config` values every checked call carries: the compiler and the
/// doc compiler by their plain names, so the rustup proxy on `PATH` resolves
/// the pinned toolchain's, and a config file's `build.rustc` or
/// `build.rustdoc` (which a command line's `--config` overrides) never runs.
const PINNED_TOOLS: &[&str] = &["build.rustc=\"rustc\"", "build.rustdoc=\"rustdoc\""];

/// The `--config` values every checked call carries ([`PINNED_TOOLS`]).
pub(crate) fn pinned_config() -> Vec<String> {
    PINNED_TOOLS
        .iter()
        .map(|value| (*value).to_owned())
        .collect()
}

/// The environment variables that change what rustc or rustdoc compiles, or
/// what runs a built test, behind cargo's report: flags, another compiler or
/// doc compiler, a wrapper, unstable features. A `CARGO_TARGET_<triple>_`
/// variable that sets `RUSTFLAGS`, `RUSTDOCFLAGS`, `RUNNER` or `LINKER` is
/// matched by [`is_refused_variable`] too.
const REFUSED_VARIABLES: &[&str] = &[
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
    "RUSTDOCFLAGS",
    "CARGO_ENCODED_RUSTDOCFLAGS",
    "CARGO_BUILD_RUSTDOCFLAGS",
    "RUSTC",
    "CARGO_BUILD_RUSTC",
    "RUSTDOC",
    "CARGO_BUILD_RUSTDOC",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    "RUSTC_BOOTSTRAP",
];

/// The suffixes of the `CARGO_TARGET_<triple>_` variables [`REFUSED_VARIABLES`]
/// stands for.
const REFUSED_TARGET_SUFFIXES: &[&str] = &["_RUSTFLAGS", "_RUSTDOCFLAGS", "_RUNNER", "_LINKER"];

/// The variable a checked build sets its rustflags through.
const ENCODED_RUSTFLAGS: &str = "CARGO_ENCODED_RUSTFLAGS";
/// The same for rustdoc, which compiles the doctests.
const ENCODED_RUSTDOCFLAGS: &str = "CARGO_ENCODED_RUSTDOCFLAGS";

/// Whether `name` is a variable [`refused_variables`] refuses. With
/// `case_insensitive` (Windows, where `RustFlags` is `RUSTFLAGS` to every
/// reader) the name is compared without regard to ASCII case.
pub(crate) fn is_refused_variable(name: &str, case_insensitive: bool) -> bool {
    let name = if case_insensitive {
        name.to_ascii_uppercase()
    } else {
        name.to_owned()
    };
    REFUSED_VARIABLES.contains(&name.as_str())
        || name.strip_prefix("CARGO_TARGET_").is_some_and(|rest| {
            REFUSED_TARGET_SUFFIXES
                .iter()
                .any(|suffix| rest.ends_with(suffix))
        })
}

/// The refused variables `environment` sets to something other than nothing,
/// sorted: each would change a checked build behind its report, or be
/// silently overridden by it, so each is refused by name.
pub(crate) fn refused_variables(
    environment: impl IntoIterator<Item = (String, String)>,
    case_insensitive: bool,
) -> Vec<String> {
    let mut found: Vec<String> = environment
        .into_iter()
        .filter(|(name, value)| is_refused_variable(name, case_insensitive) && !value.is_empty())
        .map(|(name, _)| name)
        .collect();
    found.sort();
    found
}

/// The refusal for [`refused_variables`]' answer, or `Ok` when it is empty.
pub(crate) fn refuse_variables(found: &[String]) -> Result<(), ProfileFault> {
    if found.is_empty() {
        Ok(())
    } else {
        Err(ProfileFault::RefusedInEnvironment {
            variables: found.to_vec(),
        })
    }
}

/// One change a checked build makes to its environment: a variable set to a
/// value, or removed.
pub(crate) type EnvChange = (String, Option<String>);

/// The environment every checked build carries (the module docs): its
/// rustflags exactly `rustflags` (cargo's encoding: arguments separated by
/// `\x1f`), no rustdocflags, no wrapper and no `RUSTC_BOOTSTRAP`, from any
/// source.
pub(crate) fn pinned_env(rustflags: &str) -> Vec<EnvChange> {
    vec![
        (ENCODED_RUSTFLAGS.to_owned(), Some(rustflags.to_owned())),
        (ENCODED_RUSTDOCFLAGS.to_owned(), Some(String::new())),
        ("RUSTC_WRAPPER".to_owned(), Some(String::new())),
        ("RUSTC_WORKSPACE_WRAPPER".to_owned(), Some(String::new())),
        ("RUSTC_BOOTSTRAP".to_owned(), None),
    ]
}

/// The `--message-format` a checked call reports with: cargo's JSON, whose
/// diagnostics carry their rendered text, with ANSI colour when `colour` (the
/// caller's terminal, or `CARGO_TERM_COLOR=always`, as CI sets).
pub(crate) fn message_format(colour: bool) -> &'static str {
    if colour {
        "--message-format=json-diagnostic-rendered-ansi"
    } else {
        "--message-format=json"
    }
}

/// One cargo command: its `--config` values, its environment and its
/// arguments from the subcommand on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CargoCall {
    pub(crate) config: Vec<String>,
    pub(crate) env: Vec<EnvChange>,
    pub(crate) args: Vec<String>,
}

impl CargoCall {
    /// A checked call: `args`, with the pinned tools and the pinned
    /// environment, rustflags exactly `rustflags`.
    pub(crate) fn pinned(args: Vec<String>, rustflags: &str) -> CargoCall {
        CargoCall {
            config: pinned_config(),
            env: pinned_env(rustflags),
            args,
        }
    }

    /// The command line after `cargo`: each `--config` value, then the
    /// arguments.
    pub(crate) fn command_line(&self) -> Vec<String> {
        let mut line: Vec<String> = Vec::new();
        for value in &self.config {
            line.push("--config".to_owned());
            line.push(value.clone());
        }
        line.extend(self.args.iter().cloned());
        line
    }
}

/// How xtask starts cargo: the process behind [`checked`] and [`execute`],
/// which hand it the final command line and environment, so nothing between
/// a [`TestRun`] and the process can change what runs.
pub(crate) trait Cargo {
    /// Runs cargo with `line` and `env` and hands `stdout` each line of its
    /// standard output as it arrives; its standard error goes to the
    /// caller's. Answers whether it succeeded.
    fn report(
        &mut self,
        line: &[String],
        env: &[EnvChange],
        stdout: &mut dyn FnMut(&str),
    ) -> Result<bool, String>;
    /// Runs cargo with `line` and `env`, handing `stdout` each line of its
    /// standard output as it arrives, while its standard error is echoed to
    /// the caller's as it arrives and kept. Answers whether it succeeded and
    /// the standard error's lines.
    fn run(
        &mut self,
        line: &[String],
        env: &[EnvChange],
        stdout: &mut dyn FnMut(&str),
    ) -> Result<(bool, Vec<String>), String>;
    /// The working directory cargo runs in, as cargo itself reads it: the
    /// directory a relative path cargo prints is relative to.
    fn working_dir(&self) -> Result<String, String>;
}

/// What a checked build requires of the sim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SimRule {
    /// The sim optimised wherever this build builds it (a selection that
    /// reaches no sim target builds none).
    OptimisedIfBuilt,
    /// The sim's library built, every unit of the sim at opt-level 0: the
    /// invocations of [`UNOPTIMISED_TESTS`] when the sim owns the selected
    /// target.
    Unoptimised,
    /// The sim's library built, at the level [`Expect::workspace_opt`] holds
    /// every workspace unit to.
    Built,
    /// Not judged: a release build, whose optimisation is the release
    /// profile's.
    Unjudged,
}

/// What [`faults`] holds a build to. Overflow checks on in every workspace
/// unit are always required.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Expect {
    /// Debug assertions on in every workspace unit.
    pub(crate) debug_assertions: bool,
    /// Every workspace unit but a build script at this level, and every
    /// third-party crate the sim runs too.
    pub(crate) workspace_opt: Option<OptLevel>,
    pub(crate) sim: SimRule,
    /// Every third-party crate in the sim's closure optimised.
    pub(crate) closure_optimised: bool,
    /// No crate of the godot family optimised, and no other third-party crate
    /// unoptimised.
    pub(crate) family: bool,
}

impl Expect {
    /// A test invocation, or a dev build of something tested: the dev profile
    /// the suite builds with.
    pub(crate) const fn suite(sim: SimRule) -> Expect {
        Expect {
            debug_assertions: true,
            workspace_opt: None,
            sim,
            closure_optimised: true,
            family: true,
        }
    }

    /// `release-checked`, the cross-OS hash gate's build: the release
    /// profile's optimisation (`release_opt`, `[profile.release]`'s as the
    /// committed manifest declares it) in every workspace unit and every
    /// third-party crate the sim runs, overflow checks and debug assertions
    /// on, and the sim's library built.
    pub(crate) const fn release_checked(release_opt: OptLevel) -> Expect {
        Expect {
            debug_assertions: true,
            workspace_opt: Some(release_opt),
            sim: SimRule::Built,
            closure_optimised: false,
            family: false,
        }
    }

    /// `release` and `release-client`, what ships, and `cargo test
    /// --release`, what the alarms measure: overflow checks on, debug
    /// assertions off.
    pub(crate) const RELEASE: Expect = Expect {
        debug_assertions: false,
        workspace_opt: None,
        sim: SimRule::Unjudged,
        closure_optimised: false,
        family: false,
    };
}

/// One `cargo test` invocation of a step's units: its target selectors and
/// what it requires of the sim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub(crate) selectors: Vec<String>,
    pub(crate) sim: SimRule,
}

/// The invocations that run `units`: [`crate::shard::selectors`]' lists over
/// every unit but [`UNOPTIMISED_TESTS`], then one invocation per unoptimised
/// test the units hold. Every unit is in exactly one invocation. When
/// `sim_selected` is false the selection holds no sim target of those names,
/// so they run with the rest.
pub(crate) fn invocations(units: &[Unit], sim_selected: bool) -> Vec<Invocation> {
    let unoptimised = |unit: &Unit| {
        sim_selected
            && matches!(unit, Unit::Test(name) if UNOPTIMISED_TESTS.contains(&name.as_str()))
    };
    let optimised: Vec<Unit> = units
        .iter()
        .filter(|unit| !unoptimised(unit))
        .cloned()
        .collect();
    let mut out: Vec<Invocation> = crate::shard::selectors(&optimised)
        .into_iter()
        .map(|selectors| Invocation {
            selectors,
            sim: SimRule::OptimisedIfBuilt,
        })
        .collect();
    for unit in units.iter().filter(|unit| unoptimised(unit)) {
        out.extend(
            crate::shard::selectors(std::slice::from_ref(unit))
                .into_iter()
                .map(|selectors| Invocation {
                    selectors,
                    sim: SimRule::Unoptimised,
                }),
        );
    }
    out
}

/// Holds `owners`, the packages owning a test target called `test`, to
/// [`UNOPTIMISED_OWNERS`], and every owner but the sim to not depending on
/// the sim: the exemption is a set of (package, target) pairs, never a name.
pub(crate) fn check_owners(
    test: &str,
    owners: &[String],
    graph: &Graph,
) -> Result<(), ProfileFault> {
    let mut found: Vec<String> = owners.to_vec();
    found.sort();
    found.dedup();
    let mut table: Vec<String> = UNOPTIMISED_OWNERS
        .iter()
        .filter(|(name, _, _)| *name == test)
        .map(|(_, package, _)| (*package).to_owned())
        .collect();
    table.sort();
    if found != table {
        return Err(ProfileFault::OwnersChanged {
            test: test.to_owned(),
            owners: found,
            table,
        });
    }
    for package in found.iter().filter(|package| package.as_str() != SIM) {
        if graph.reaches(package, SIM)? {
            return Err(ProfileFault::OwnerReachesSim {
                test: test.to_owned(),
                package: package.clone(),
            });
        }
    }
    Ok(())
}

/// An argument for the test harness, after `--`: typed, so a caller cannot
/// pass the harness (or cargo) anything this does not name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Harness {
    /// `--ignored`: run the tests marked `#[ignore]`, and only those.
    Ignored,
    /// `--nocapture`: the tests' own output as it comes.
    Nocapture,
    /// `--exact <name>`: the one test of that name.
    Exact(String),
}

impl Harness {
    fn args(&self) -> Vec<String> {
        match self {
            Harness::Ignored => vec!["--ignored".to_owned()],
            Harness::Nocapture => vec!["--nocapture".to_owned()],
            Harness::Exact(name) => vec!["--exact".to_owned(), name.clone()],
        }
    }
}

/// One test invocation, the single description both its check and its run
/// are derived from. Its parts, its check and its run are private to this
/// module: the only way to run one is [`execute`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TestRun {
    /// The arguments after `test` and before the selectors: the package
    /// selection and features.
    selection: Vec<String>,
    invocation: Invocation,
    locked: bool,
    colour: bool,
    /// `--release`: the alarms' measurements.
    release: bool,
    harness: Vec<Harness>,
}

impl TestRun {
    pub(crate) fn new(
        selection: Vec<String>,
        invocation: Invocation,
        locked: bool,
        colour: bool,
    ) -> TestRun {
        TestRun {
            selection,
            invocation,
            locked,
            colour,
            release: false,
            harness: Vec::new(),
        }
    }

    /// The same run in the release profile, as the alarms measure; its sim
    /// rule is the release profile's.
    pub(crate) fn release(mut self) -> TestRun {
        self.release = true;
        self.invocation.sim = SimRule::Unjudged;
        self
    }

    /// The command line the run makes, after `cargo`: for the tests that pin
    /// it. Running it is [`execute`]'s alone.
    #[cfg(test)]
    pub(crate) fn run_line(&self) -> Vec<String> {
        self.run().command_line()
    }

    /// Whether the run passes `--locked`.
    pub(crate) fn locked(&self) -> bool {
        self.locked
    }

    /// The same run with `harness`'s arguments for the test harness.
    pub(crate) fn with_harness(mut self, harness: Vec<Harness>) -> TestRun {
        self.harness = harness;
        self
    }

    /// What the check holds this invocation's build to.
    fn expect(&self) -> Expect {
        if self.release {
            Expect::RELEASE
        } else {
            Expect::suite(self.invocation.sim)
        }
    }

    /// Whether this is a doctest invocation.
    fn doc(&self) -> bool {
        self.invocation.selectors == ["--doc"]
    }

    /// The run's cargo arguments, before `--` and the harness's.
    fn cargo_args(&self) -> CargoCall {
        let mut config = pinned_config();
        if self.invocation.sim == SimRule::Unoptimised {
            config.push(SIM_UNOPTIMISED.to_owned());
        }
        let mut args: Vec<String> = vec!["test".to_owned()];
        if self.release {
            args.push("--release".to_owned());
        }
        args.extend(self.selection.iter().cloned());
        args.extend(self.invocation.selectors.iter().cloned());
        if self.locked {
            args.push("--locked".to_owned());
        }
        args.push(message_format(self.colour).to_owned());
        CargoCall {
            config,
            env: pinned_env(""),
            args,
        }
    }

    /// The command that runs the tests: its report, then `--` and the
    /// harness's arguments.
    fn run(&self) -> CargoCall {
        let mut run = self.cargo_args();
        if !self.harness.is_empty() {
            run.args.push("--".to_owned());
            for harness in &self.harness {
                run.args.extend(harness.args());
            }
        }
        run
    }

    /// The command that builds what [`TestRun::run`] will run, and reports
    /// it: the run's cargo arguments with `--no-run`, or for a doctest
    /// invocation [`doc_check`] of the run.
    fn check(&self) -> CargoCall {
        if self.doc() {
            return doc_check(&self.run(), self.colour);
        }
        let mut check = self.cargo_args();
        check.args.push("--no-run".to_owned());
        check
    }
}

/// The paths of the test executables a check's report names: the units
/// built as test harnesses (`profile.test`) with an executable, which the
/// run then runs.
fn test_executables(built: &[Built]) -> Vec<String> {
    let mut paths: Vec<String> = built
        .iter()
        .filter(|unit| unit.test)
        .filter_map(|unit| unit.executable.clone())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// `path` with one separator, `/`, and without Windows' verbatim prefix:
/// `\\?\D:\a` is `D:/a` and `\\?\UNC\host\share` is `//host/share`.
fn slashed(path: &str) -> String {
    let path = path.replace('\\', "/");
    if let Some(rest) = path.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = path.strip_prefix("//?/") {
        rest.to_owned()
    } else {
        path
    }
}

/// The root a slashed path starts with, and the rest: `//` for a UNC path,
/// `X:/` for a drive's, `/` for a Unix one, and nothing for a relative path
/// (a drive-relative `X:a` included, which cargo never prints).
fn split_root(path: &str) -> (&str, &str) {
    if path.starts_with("//") {
        return path.split_at(2);
    }
    if path.starts_with('/') {
        return path.split_at(1);
    }
    let mut bytes = path.bytes();
    if let (Some(drive), Some(b':'), Some(b'/')) = (bytes.next(), bytes.next(), bytes.next())
        && drive.is_ascii_alphabetic()
    {
        return path.split_at(3);
    }
    ("", path)
}

/// A path as the run's `Running` line and the report spell it, made
/// comparable lexically, and only lexically:
///
/// - one separator and no verbatim prefix ([`slashed`]), on both sides;
/// - a relative path (what cargo prints for an executable under its working
///   directory, CI's in-tree `target`) joined to `working_dir`, the directory
///   cargo ran in, as cargo read it ([`Cargo::working_dir`]);
/// - `.` and empty components dropped and `..` taken back, without reading the
///   disk: nothing is canonicalised, so no link is resolved (macOS's
///   `/var` stays `/var`; the working directory is already the one cargo read);
/// - on Windows, whose file names are case-insensitive, one case, which also
///   covers a drive letter spelled `D:` on one side and `d:` on the other.
fn comparable_path(path: &str, working_dir: &str, case_insensitive: bool) -> String {
    let path = slashed(path);
    let joined = if split_root(&path).0.is_empty() {
        format!("{}/{path}", slashed(working_dir))
    } else {
        path
    };
    let (root, rest) = split_root(&joined);
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else if root.is_empty() {
                    parts.push("..");
                }
                // `..` at a root is the root.
            }
            other => parts.push(other),
        }
    }
    let path = format!("{root}{}", parts.join("/"));
    if case_insensitive {
        path.to_lowercase()
    } else {
        path
    }
}

/// `text` with ANSI escape sequences (`ESC [ ... letter`) taken out, as cargo
/// colours its status lines when told to.
fn without_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            for inner in chars.by_ref() {
                if inner.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(character);
        }
    }
    out
}

/// The executable a cargo `Running` line names: `Running unittests src\lib.rs
/// (D:/t\debug\deps\x.exe)` names `D:/t\debug\deps\x.exe`.
fn running_path(line: &str) -> Option<String> {
    let line = without_ansi(line);
    let rest = line.trim().strip_prefix("Running ")?;
    let inner = rest.strip_suffix(')')?;
    let (_, path) = inner.rsplit_once(" (")?;
    Some(path.to_owned())
}

/// Whether the run ran exactly the test executables the check reported: each
/// named by a `Running` line on the run's standard error, no `Running` line
/// naming anything else, and as many libtest `test result:` lines on its
/// standard output as `Running` lines. Both sides are compared as paths
/// ([`comparable_path`]), a relative `Running` path resolved against
/// `working_dir`, the directory cargo ran in. A runner that ran nothing (a
/// config file's `runner` that exits 0) leaves the `Running` lines and no
/// result, and is [`ProfileFault::TestsDidNotRun`].
fn judge_run(
    expected: &[String],
    stderr: &[String],
    results: usize,
    working_dir: &str,
    case_insensitive: bool,
) -> Result<(), ProfileFault> {
    let started: Vec<(String, String)> = stderr
        .iter()
        .filter_map(|line| running_path(line))
        .map(|path| {
            let comparable = comparable_path(&path, working_dir, case_insensitive);
            (path, comparable)
        })
        .collect();
    let reported: Vec<String> = expected
        .iter()
        .map(|path| comparable_path(path, working_dir, case_insensitive))
        .collect();
    let missing: Vec<String> = expected
        .iter()
        .zip(&reported)
        .filter(|(_, comparable)| !started.iter().any(|(_, ran)| ran == *comparable))
        .map(|(path, _)| path.clone())
        .collect();
    let unreported: Vec<String> = started
        .iter()
        .filter(|(_, comparable)| !reported.contains(comparable))
        .map(|(path, _)| path.clone())
        .collect();
    if !missing.is_empty() || !unreported.is_empty() || results != started.len() {
        return Err(ProfileFault::TestsDidNotRun {
            expected: expected.len(),
            started: started.len(),
            results,
            missing,
            unreported,
        });
    }
    Ok(())
}

/// Runs `test`: its check through [`checked`], then, when the check holds,
/// its run, both through `cargo` and nothing else. Each diagnostic is
/// written to `diagnostics` once for the two, as it arrives; each line the
/// tests print goes to `test_output` as it arrives. After the run, every test
/// executable the check reported must have run and given its result
/// ([`judge_run`]); a doctest run is held to its exit status alone, because
/// libtest's output does not tie a doctest result to the crate it came from.
pub(crate) fn execute(
    test: &TestRun,
    graph: &Graph,
    cargo: &mut impl Cargo,
    diagnostics: &mut impl FnMut(&str),
    test_output: &mut impl FnMut(&str),
) -> Result<(), String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut once = |rendered: &str| {
        if seen.insert(rendered.to_owned()) {
            diagnostics(rendered);
        }
    };
    let built = checked(cargo, &test.check(), graph, test.expect(), &mut once)?;
    let run = test.run();
    let mut results: usize = 0;
    let mut stdout = |line: &str| {
        if let Some(message) = cargo_message(line) {
            if let Some(rendered) = message
                .get("message")
                .and_then(|diagnostic| diagnostic.get("rendered"))
                .and_then(Json::as_str)
            {
                once(rendered);
            }
        } else {
            if line.trim_start().starts_with("test result: ") {
                results += 1;
            }
            test_output(line);
        }
    };
    let (success, stderr) = cargo.run(&run.command_line(), &run.env, &mut stdout)?;
    if !success {
        return Err(format!(
            "`cargo {}` failed; its output is above",
            run.command_line().join(" ")
        ));
    }
    if !test.doc() {
        judge_run(
            &test_executables(&built),
            &stderr,
            results,
            &cargo.working_dir()?,
            cfg!(windows),
        )
        .map_err(|fault| fault.to_string())?;
    }
    Ok(())
}

/// `line` read as one of cargo's JSON messages: an object with a `reason`
/// cargo writes. A test's own output is never one, unless it prints exactly
/// such an object, which a test of this workspace does not.
fn cargo_message(line: &str) -> Option<Json> {
    let message = Json::parse(line).ok()?;
    let reason = message.get("reason").and_then(Json::as_str)?;
    REASONS.contains(&reason).then_some(message)
}

/// Makes `call`, a build that reports itself, through `cargo`, reading its
/// report as it arrives ([`Report`]), handing each diagnostic's rendered
/// text to `diagnostics`, and holds every unit it built to `expect`
/// ([`faults`]). Returns the units, so a caller can take what the build
/// produced from them.
pub(crate) fn checked(
    cargo: &mut impl Cargo,
    call: &CargoCall,
    graph: &Graph,
    expect: Expect,
    diagnostics: &mut impl FnMut(&str),
) -> Result<Vec<Built>, String> {
    let mut report = Report::default();
    let success = cargo.report(&call.command_line(), &call.env, &mut |line| {
        report.feed(line, diagnostics);
    })?;
    let built = report.finish().map_err(|fault| fault.to_string())?;
    if !success {
        return Err(
            "cargo failed, and its report ends in a successful `build-finished`".to_owned(),
        );
    }
    let faults = faults(&built, graph, expect);
    if faults.is_empty() {
        Ok(built)
    } else {
        Err(faults
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "))
    }
}

/// The check of a doctest run: cargo will not build doctests without running
/// them, so it builds, in the test profile and with the same `--config`,
/// environment, selection and features, the libraries the doctests link (the
/// same artefacts the doctest run then finds fresh). A dev-dependency built
/// only for doctests is not in this build; each one a package's doctests use
/// is built, and checked, for that package's unit or integration tests in
/// another invocation of the same leg. The doctest bodies are compiled by
/// rustdoc, whose flags the pinned `CARGO_ENCODED_RUSTDOCFLAGS` holds empty
/// and which the pinned `build.rustdoc` names.
pub(crate) fn doc_check(run: &CargoCall, colour: bool) -> CargoCall {
    let format = message_format(colour);
    let mut args: Vec<String> = Vec::new();
    for (index, arg) in run.args.iter().enumerate() {
        match (index, arg.as_str()) {
            (0, "test") => args.push("build".to_owned()),
            (_, "--doc") => {}
            (_, other) if other == format => {}
            _ => args.push(arg.clone()),
        }
    }
    for extra in ["--profile", "test", "--lib", format] {
        args.push(extra.to_owned());
    }
    CargoCall {
        config: run.config.clone(),
        env: run.env.clone(),
        args,
    }
}

/// An optimisation level as cargo reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OptLevel {
    O0,
    O1,
    O2,
    O3,
    S,
    Z,
}

impl OptLevel {
    /// Cargo's spelling, and nothing else.
    pub(crate) fn parse(text: &str) -> Option<OptLevel> {
        match text {
            "0" => Some(OptLevel::O0),
            "1" => Some(OptLevel::O1),
            "2" => Some(OptLevel::O2),
            "3" => Some(OptLevel::O3),
            "s" => Some(OptLevel::S),
            "z" => Some(OptLevel::Z),
            _ => None,
        }
    }

    fn optimised(self) -> bool {
        self != OptLevel::O0
    }
}

impl fmt::Display for OptLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            OptLevel::O0 => "0",
            OptLevel::O1 => "1",
            OptLevel::O2 => "2",
            OptLevel::O3 => "3",
            OptLevel::S => "s",
            OptLevel::Z => "z",
        })
    }
}

/// One unit cargo built, with the profile it resolved for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Built {
    pub(crate) package_id: String,
    pub(crate) target: String,
    pub(crate) kinds: Vec<String>,
    pub(crate) opt_level: OptLevel,
    pub(crate) debug_assertions: bool,
    pub(crate) overflow_checks: bool,
    /// Built as a test harness (`profile.test`): a test executable the run
    /// runs.
    pub(crate) test: bool,
    /// The files the unit produced.
    pub(crate) filenames: Vec<String>,
    /// The executable the unit produced, when it is one.
    pub(crate) executable: Option<String>,
}

impl Built {
    /// Whether the unit is of `kind` (`lib`, `bin`, `cdylib`, ...).
    pub(crate) fn is(&self, kind: &str) -> bool {
        self.kinds.iter().any(|own| own == kind)
    }
}

/// The packages [`faults`] judges by: the workspace's members, every
/// package's name, the sim, the third-party crates it runs and the godot
/// family.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Graph {
    members: BTreeSet<String>,
    names: BTreeMap<String, String>,
    /// Every package's version, by id.
    versions: BTreeMap<String, String>,
    sim: Option<String>,
    /// The sim's normal-dependency closure, proc-macros and what only they
    /// reach left out: the code the sim runs.
    sim_closure: BTreeSet<String>,
    /// The packages reachable from the `godot` package and from no
    /// workspace member except through it.
    godot_family: BTreeSet<String>,
    /// Every dependency edge, any kind.
    edges: BTreeMap<String, Vec<String>>,
}

impl Graph {
    /// The names of the godot family, sorted.
    pub(crate) fn godot_family_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .godot_family
            .iter()
            .filter_map(|id| self.names.get(id).cloned())
            .collect();
        names.sort();
        names
    }

    /// The ids of the packages a cargo package spec (`name`, or
    /// `name@version` with a whole or partial version) matches, as cargo
    /// matches them.
    fn spec_ids(&self, spec: &str) -> Vec<String> {
        let (name, version) = match spec.split_once('@') {
            Some((name, version)) => (name, Some(version)),
            None => (spec, None),
        };
        self.names
            .iter()
            .filter(|(id, package)| {
                package.as_str() == name
                    && version.is_none_or(|wanted| {
                        self.versions.get(*id).is_some_and(|have| {
                            let wanted: Vec<&str> = wanted.split('.').collect();
                            let have: Vec<&str> = have.split(['.', '-', '+']).collect();
                            wanted.len() <= have.len()
                                && wanted.iter().zip(&have).all(|(want, part)| want == part)
                        })
                    })
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Whether a cargo package spec matches some package in the graph.
    pub(crate) fn has_package(&self, spec: &str) -> bool {
        !self.spec_ids(spec).is_empty()
    }

    /// Whether every package a spec matches is of the godot family (and it
    /// matches one).
    pub(crate) fn spec_in_family(&self, spec: &str) -> bool {
        let ids = self.spec_ids(spec);
        !ids.is_empty() && ids.iter().all(|id| self.godot_family.contains(id))
    }

    /// Whether the package called `from` depends on the one called `to`,
    /// through any chain of dependencies of any kind.
    pub(crate) fn reaches(&self, from: &str, to: &str) -> Result<bool, ProfileFault> {
        let id_of = |name: &str| {
            self.names
                .iter()
                .find(|(_, package)| package.as_str() == name)
                .map(|(id, _)| id.clone())
                .ok_or_else(|| ProfileFault::Metadata(format!("no package is called {name}")))
        };
        let (from, to) = (id_of(from)?, id_of(to)?);
        Ok(reach(&self.edges, std::slice::from_ref(&from), None)
            .into_iter()
            .any(|id| id == to && id != from))
    }
}

/// Every package reachable from `roots` through `edges` (the roots included),
/// never entering `wall`.
fn reach(
    edges: &BTreeMap<String, Vec<String>>,
    roots: &[String],
    wall: Option<&str>,
) -> BTreeSet<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<String> = roots.to_vec();
    while let Some(id) = queue.pop() {
        if wall == Some(id.as_str()) || !seen.insert(id.clone()) {
            continue;
        }
        if let Some(next) = edges.get(&id) {
            queue.extend(next.iter().cloned());
        }
    }
    seen
}

/// A check that what cargo builds is what the build promises, failed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ProfileFault {
    /// A line of cargo's JSON messages that does not read as one.
    Unreadable { line: usize, why: String },
    /// A message whose `reason` this does not know: read strictly, never
    /// skipped.
    UnknownReason { line: usize, reason: String },
    /// An `opt_level` that is not one of cargo's six.
    BadOptLevel { line: usize, value: String },
    /// No message at all: a build that reports nothing is judged on nothing.
    NothingBuilt,
    /// Messages, and no `build-finished` after them.
    Unfinished,
    /// A `build-finished` that is not the one last message.
    FinishedTwice { line: usize },
    /// A `build-finished` that says the build failed.
    BuildFailed,
    /// `cargo metadata` that does not read as the graph this needs.
    Metadata(String),
    /// A unit of a package `cargo metadata` does not list.
    UnknownPackage { package_id: String },
    /// A workspace unit with a check switched off.
    CheckOff {
        package: String,
        target: String,
        check: &'static str,
    },
    /// A workspace unit at a level other than the one its profile promises.
    WrongOptLevel {
        package: String,
        target: String,
        found: OptLevel,
        promised: OptLevel,
    },
    /// The sim at opt-level 0 in a build that runs it optimised.
    SimUnoptimised { target: String },
    /// The sim optimised in an invocation of [`UNOPTIMISED_TESTS`].
    SimOptimised { target: String, opt_level: OptLevel },
    /// A build that requires the sim's library and built none.
    SimNotBuilt,
    /// A third-party crate the sim runs at opt-level 0.
    DependencyUnoptimised { package: String, target: String },
    /// A crate of the godot family optimised.
    GodotOptimised {
        package: String,
        target: String,
        opt_level: OptLevel,
    },
    /// A third-party crate outside the godot family at opt-level 0.
    StrayUnoptimised { package: String, target: String },
    /// A run that did not run every test executable its check reported.
    TestsDidNotRun {
        expected: usize,
        started: usize,
        results: usize,
        missing: Vec<String>,
        /// Started executables the check never reported.
        unreported: Vec<String>,
    },
    /// Variables set in the environment that change codegen behind the
    /// report.
    RefusedInEnvironment { variables: Vec<String> },
    /// The packages owning an unoptimised test's target are not the ones
    /// [`UNOPTIMISED_OWNERS`] names.
    OwnersChanged {
        test: String,
        owners: Vec<String>,
        table: Vec<String>,
    },
    /// An owner other than the sim that depends on the sim, so the sim's
    /// level would reach it.
    OwnerReachesSim { test: String, package: String },
}

impl fmt::Display for ProfileFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { line, why } => write!(
                f,
                "line {line} of cargo's JSON messages does not read as one ({why}); the \
                 profile check reads them strictly"
            ),
            Self::UnknownReason { line, reason } => write!(
                f,
                "line {line} of cargo's JSON messages has the reason `{reason}`, which the \
                 profile check does not know; it reads every message, and skips none"
            ),
            Self::BadOptLevel { line, value } => write!(
                f,
                "line {line} of cargo's JSON messages gives the opt-level `{value}`, which is \
                 none of cargo's 0, 1, 2, 3, s and z"
            ),
            Self::NothingBuilt => f.write_str(
                "cargo's JSON messages are empty; a build that reports nothing cannot be \
                 checked, so it fails",
            ),
            Self::Unfinished => f.write_str(
                "cargo's JSON messages end without `build-finished`; a build whose end is not \
                 reported cannot be checked, so it fails",
            ),
            Self::FinishedTwice { line } => write!(
                f,
                "line {line} of cargo's JSON messages comes after a `build-finished`; the \
                 report is one build, ended once"
            ),
            Self::BuildFailed => f.write_str(
                "cargo's `build-finished` says the build failed; its diagnostics are above",
            ),
            Self::Metadata(why) => write!(f, "cargo metadata does not give the graph: {why}"),
            Self::UnknownPackage { package_id } => write!(
                f,
                "cargo built a unit of `{package_id}`, which cargo metadata does not list"
            ),
            Self::CheckOff {
                package,
                target,
                check,
            } => write!(
                f,
                "{package}'s target `{target}` builds with {check} off, which this build \
                 promises on for every workspace unit"
            ),
            Self::WrongOptLevel {
                package,
                target,
                found,
                promised,
            } => write!(
                f,
                "{package}'s target `{target}` builds at opt-level {found}; this build promises \
                 the release profile's {promised} for every workspace unit and every crate the \
                 sim runs"
            ),
            Self::SimUnoptimised { target } => write!(
                f,
                "{SIM}'s target `{target}` builds at opt-level 0; the suite runs the sim \
                 optimised (Cargo.toml's [profile.dev.package.{SIM}]), and only the \
                 invocations of {} run it unoptimised",
                UNOPTIMISED_TESTS.join(", ")
            ),
            Self::SimOptimised { target, opt_level } => write!(
                f,
                "{SIM}'s target `{target}` builds at opt-level {opt_level} in an invocation of \
                 {}, which runs the sim at opt-level 0 so the optimiser cannot delete the \
                 allocations it counts",
                UNOPTIMISED_TESTS.join(", ")
            ),
            Self::SimNotBuilt => write!(
                f,
                "this build requires {SIM}'s library and built none, so what it promises of the \
                 sim was never in force"
            ),
            Self::DependencyUnoptimised { package, target } => write!(
                f,
                "{package}'s target `{target}`, which {SIM} runs, builds at opt-level 0; \
                 Cargo.toml's [profile.dev.package.\"*\"] optimises the third-party crates \
                 the sim runs"
            ),
            Self::GodotOptimised {
                package,
                target,
                opt_level,
            } => write!(
                f,
                "{package}'s target `{target}` builds at opt-level {opt_level}; it is of the \
                 godot family, which serves the walled client alone and stays at 0: \
                 Cargo.toml needs `[profile.dev.package.{package}]` with `opt-level = 0`"
            ),
            Self::StrayUnoptimised { package, target } => write!(
                f,
                "{package}'s target `{target}` builds at opt-level 0, and {package} is not of \
                 the godot family, the only third-party crates the suite builds unoptimised: \
                 remove `[profile.dev.package.{package}]` (or the setting that names it)"
            ),
            Self::TestsDidNotRun {
                expected,
                started,
                results,
                missing,
                unreported,
            } => write!(
                f,
                "the check built {expected} test executable(s); cargo started {started} and \
                 libtest reported {results} result(s){}{}. Something ran in their place (a \
                 config file's `runner`, say), or the run built and ran something the check \
                 did not report",
                if missing.is_empty() {
                    String::new()
                } else {
                    format!("; never started: {}", missing.join(", "))
                },
                if unreported.is_empty() {
                    String::new()
                } else {
                    format!("; started but never reported: {}", unreported.join(", "))
                }
            ),
            Self::RefusedInEnvironment { variables } => write!(
                f,
                "{} set in the environment; each changes what rustc compiles behind the \
                 profile cargo reports, or what runs a built test (flags, another compiler or \
                 doc compiler, a wrapper, a runner or linker, or unstable features), and a \
                 checked build pins its own, so it would override them in silence. Unset {} \
                 and run it again",
                variables.join(", "),
                if variables.len() == 1 { "it" } else { "them" }
            ),
            Self::OwnersChanged {
                test,
                owners,
                table,
            } => write!(
                f,
                "the test target `{test}` is owned by {} and the exemption names {}; \
                 `cargo test --test {test}` selects every target of the name, so \
                 xtask/src/resolved.rs's UNOPTIMISED_OWNERS states each owner and why it rides \
                 with the sim unoptimised",
                owners.join(", "),
                table.join(", ")
            ),
            Self::OwnerReachesSim { test, package } => write!(
                f,
                "{package} owns a `{test}` test and depends on {SIM}, so the opt-level the \
                 invocation sets for the sim would reach it"
            ),
        }
    }
}

/// The member of `object` called `key`, or why it is missing.
fn member<'a>(object: &'a Json, key: &str) -> Result<&'a Json, String> {
    object.get(key).ok_or_else(|| format!("no `{key}`"))
}

fn string_of<'a>(object: &'a Json, key: &str) -> Result<&'a str, String> {
    member(object, key)?
        .as_str()
        .ok_or_else(|| format!("`{key}` is not a string"))
}

fn bool_of(object: &Json, key: &str) -> Result<bool, String> {
    match member(object, key)? {
        Json::Bool(value) => Ok(*value),
        _ => Err(format!("`{key}` is not a boolean")),
    }
}

fn array_of<'a>(object: &'a Json, key: &str) -> Result<&'a Vec<Json>, String> {
    member(object, key)?
        .as_array()
        .ok_or_else(|| format!("`{key}` is not an array"))
}

fn strings_of(object: &Json, key: &str) -> Result<Vec<String>, String> {
    array_of(object, key)?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("an item of `{key}` is not a string"))
        })
        .collect()
}

/// The reasons cargo's JSON report writes for a build.
const REASONS: &[&str] = &[
    "compiler-artifact",
    "compiler-message",
    "build-script-executed",
    "build-finished",
];

/// A build's JSON report, read a line at a time as cargo writes it. Every
/// line must be a JSON object with a known `reason`; every
/// `compiler-artifact` must carry its package, target, resolved profile and
/// files; every `compiler-message` must carry its message, whose rendered
/// text goes to the caller's `diagnostics` as it is read; and the stream
/// must end in exactly one `build-finished` that says the build succeeded.
/// The first fault is kept and the rest of the stream is still read, so the
/// process's output is drained and every diagnostic is shown.
#[derive(Debug, Default)]
pub(crate) struct Report {
    built: Vec<Built>,
    finished: bool,
    any: bool,
    /// Lines read, the last one's number.
    line: usize,
    fault: Option<ProfileFault>,
}

impl Report {
    /// Reads one line of the report.
    pub(crate) fn feed(&mut self, text: &str, diagnostics: &mut impl FnMut(&str)) {
        // One more line than were read: a stream holds fewer lines than its
        // bytes, and fewer bytes than `usize::MAX`.
        self.line += 1;
        if text.trim().is_empty() {
            return;
        }
        let line = self.line;
        if let Err(fault) = self.read(line, text, diagnostics)
            && self.fault.is_none()
        {
            self.fault = Some(fault);
        }
    }

    fn read(
        &mut self,
        line: usize,
        text: &str,
        diagnostics: &mut impl FnMut(&str),
    ) -> Result<(), ProfileFault> {
        self.any = true;
        if self.finished {
            return Err(ProfileFault::FinishedTwice { line });
        }
        let unreadable = |why: String| ProfileFault::Unreadable { line, why };
        let message = Json::parse(text).map_err(unreadable)?;
        let reason = string_of(&message, "reason").map_err(unreadable)?;
        if !REASONS.contains(&reason) {
            return Err(ProfileFault::UnknownReason {
                line,
                reason: reason.to_owned(),
            });
        }
        if reason == "build-finished" {
            if !bool_of(&message, "success").map_err(unreadable)? {
                return Err(ProfileFault::BuildFailed);
            }
            self.finished = true;
            return Ok(());
        }
        if reason == "compiler-message" {
            let diagnostic = member(&message, "message").map_err(unreadable)?;
            return match member(diagnostic, "rendered").map_err(unreadable)? {
                Json::Str(rendered) => {
                    diagnostics(rendered);
                    Ok(())
                }
                Json::Null => Ok(()),
                _ => Err(unreadable(
                    "a diagnostic's `rendered` is neither text nor null".to_owned(),
                )),
            };
        }
        if reason != "compiler-artifact" {
            return Ok(());
        }
        let target = member(&message, "target").map_err(unreadable)?;
        let profile = member(&message, "profile").map_err(unreadable)?;
        let level = string_of(profile, "opt_level").map_err(unreadable)?;
        let opt_level = OptLevel::parse(level).ok_or_else(|| ProfileFault::BadOptLevel {
            line,
            value: level.to_owned(),
        })?;
        let executable = match member(&message, "executable").map_err(unreadable)? {
            Json::Null => None,
            Json::Str(path) => Some(path.clone()),
            _ => {
                return Err(unreadable(
                    "`executable` is neither null nor a string".to_owned(),
                ));
            }
        };
        let filenames = strings_of(&message, "filenames").map_err(unreadable)?;
        // Cargo writes every path absolute, a relative `CARGO_TARGET_DIR`
        // joined to its working directory; a relative one would be resolved
        // against xtask's, which is not the root, so it is refused.
        if let Some(relative) = filenames
            .iter()
            .chain(&executable)
            .find(|path| split_root(&slashed(path)).0.is_empty())
        {
            return Err(unreadable(format!(
                "cargo reported the relative path `{relative}`, and every path it reports is \
                 absolute"
            )));
        }
        self.built.push(Built {
            package_id: string_of(&message, "package_id")
                .map_err(unreadable)?
                .to_owned(),
            target: string_of(target, "name").map_err(unreadable)?.to_owned(),
            kinds: strings_of(target, "kind").map_err(unreadable)?,
            opt_level,
            debug_assertions: bool_of(profile, "debug_assertions").map_err(unreadable)?,
            overflow_checks: bool_of(profile, "overflow_checks").map_err(unreadable)?,
            test: bool_of(profile, "test").map_err(unreadable)?,
            filenames,
            executable,
        });
        Ok(())
    }

    /// The units the build reported, or the first fault, or why the stream
    /// was no whole report.
    pub(crate) fn finish(self) -> Result<Vec<Built>, ProfileFault> {
        if let Some(fault) = self.fault {
            return Err(fault);
        }
        if !self.any {
            return Err(ProfileFault::NothingBuilt);
        }
        if !self.finished {
            return Err(ProfileFault::Unfinished);
        }
        Ok(self.built)
    }
}

/// Every unit cargo built, from the whole stdout of a build that reports
/// itself, read as [`Report`] reads it.
#[cfg(test)]
pub(crate) fn parse_messages(
    text: &str,
    diagnostics: &mut impl FnMut(&str),
) -> Result<Vec<Built>, ProfileFault> {
    let mut report = Report::default();
    for line in text.lines() {
        report.feed(line, diagnostics);
    }
    report.finish()
}

/// The graph [`faults`] needs, from `cargo metadata --format-version 1`
/// (with its dependencies, so with `resolve`).
pub(crate) fn graph(metadata: &str) -> Result<Graph, ProfileFault> {
    let fault = ProfileFault::Metadata;
    let json = Json::parse(metadata).map_err(fault)?;
    let mut graph = Graph::default();
    let mut proc_macros: BTreeSet<String> = BTreeSet::new();
    for package in array_of(&json, "packages").map_err(fault)? {
        let id = string_of(package, "id").map_err(fault)?;
        let name = string_of(package, "name").map_err(fault)?;
        if name == SIM {
            graph.sim = Some(id.to_owned());
        }
        for target in array_of(package, "targets").map_err(fault)? {
            let kinds = array_of(target, "kind").map_err(fault)?;
            if kinds.iter().any(|kind| kind.as_str() == Some("proc-macro")) {
                proc_macros.insert(id.to_owned());
            }
        }
        graph.names.insert(id.to_owned(), name.to_owned());
        graph.versions.insert(
            id.to_owned(),
            string_of(package, "version").map_err(fault)?.to_owned(),
        );
    }
    for id in array_of(&json, "workspace_members").map_err(fault)? {
        let id = id
            .as_str()
            .ok_or_else(|| "a workspace member is not a string".to_owned())
            .map_err(fault)?;
        graph.members.insert(id.to_owned());
    }
    let nodes = member(&json, "resolve")
        .and_then(|resolve| array_of(resolve, "nodes"))
        .map_err(fault)?;
    // Each package's normal dependencies (a `dep_kinds` entry whose `kind` is
    // null, whatever its platform), and its dependencies of every kind.
    let mut normal: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for node in nodes {
        let id = string_of(node, "id").map_err(fault)?;
        let mut normal_out: Vec<String> = Vec::new();
        let mut all_out: Vec<String> = Vec::new();
        for dep in array_of(node, "deps").map_err(fault)? {
            let pkg = string_of(dep, "pkg").map_err(fault)?;
            let kinds = array_of(dep, "dep_kinds").map_err(fault)?;
            if kinds.is_empty() {
                return Err(ProfileFault::Metadata(format!(
                    "{id}'s dependency {pkg} has no dependency kind"
                )));
            }
            let mut is_normal = false;
            for kind in kinds {
                match member(kind, "kind").map_err(fault)? {
                    Json::Null => is_normal = true,
                    Json::Str(_) => {}
                    _ => {
                        return Err(ProfileFault::Metadata(
                            "a dependency kind is neither null nor a string".to_owned(),
                        ));
                    }
                }
            }
            all_out.push(pkg.to_owned());
            // A proc-macro runs in the compiler, not in the sim: neither it
            // nor what only it reaches is code the sim runs.
            if is_normal && !proc_macros.contains(pkg) {
                normal_out.push(pkg.to_owned());
            }
        }
        normal.insert(id.to_owned(), normal_out);
        graph.edges.insert(id.to_owned(), all_out);
    }
    let Some(sim) = graph.sim.clone() else {
        return Err(ProfileFault::Metadata(format!(
            "no package is called {SIM}"
        )));
    };
    let Some(first) = normal.get(&sim) else {
        return Err(ProfileFault::Metadata(format!(
            "{SIM} has no node in the resolve graph"
        )));
    };
    let mut closure = reach(&normal, first, None);
    closure.remove(&sim);
    graph.sim_closure = closure;
    // The godot family: everything the `godot` package reaches, less what
    // any workspace member reaches without passing through it.
    if let Some(godot) = graph
        .names
        .iter()
        .find(|(_, name)| name.as_str() == "godot")
        .map(|(id, _)| id.clone())
    {
        let members: Vec<String> = graph.members.iter().cloned().collect();
        let elsewhere = reach(&graph.edges, &members, Some(godot.as_str()));
        graph.godot_family = reach(&graph.edges, &[godot], None)
            .into_iter()
            .filter(|id| !elsewhere.contains(id))
            .collect();
    }
    Ok(graph)
}

/// Every way `built` breaks `expect` (the module docs). Empty when it keeps
/// it.
pub(crate) fn faults(built: &[Built], graph: &Graph, expect: Expect) -> Vec<ProfileFault> {
    let mut faults: Vec<ProfileFault> = Vec::new();
    let mut sim_library = false;
    for unit in built {
        let Some(package) = graph.names.get(&unit.package_id) else {
            faults.push(ProfileFault::UnknownPackage {
                package_id: unit.package_id.clone(),
            });
            continue;
        };
        let target = unit.target.clone();
        let member = graph.members.contains(&unit.package_id);
        if member {
            for (on, required, check) in [
                (
                    unit.debug_assertions,
                    expect.debug_assertions,
                    "debug assertions",
                ),
                (unit.overflow_checks, true, "overflow checks"),
            ] {
                if required && !on {
                    faults.push(ProfileFault::CheckOff {
                        package: package.clone(),
                        target: target.clone(),
                        check,
                    });
                }
            }
            // A build script runs on the host at the build-override's level;
            // it is not the code the profile's optimisation is promised for.
            if let Some(promised) = expect.workspace_opt
                && !unit.is("custom-build")
                && unit.opt_level != promised
            {
                faults.push(ProfileFault::WrongOptLevel {
                    package: package.clone(),
                    target: target.clone(),
                    found: unit.opt_level,
                    promised,
                });
            }
        }
        // A build script runs on the host at the build-override's level; it is
        // not the sim, nor a crate the sim runs, so no optimisation rule judges
        // one (a future `build.rs` of the sim raises nothing).
        let script = unit.is("custom-build");
        if !member
            && !script
            && graph.sim_closure.contains(&unit.package_id)
            && let Some(promised) = expect.workspace_opt
            && unit.opt_level != promised
        {
            faults.push(ProfileFault::WrongOptLevel {
                package: package.clone(),
                target: target.clone(),
                found: unit.opt_level,
                promised,
            });
        }
        if script {
            // Its checks are judged above when it is a member's.
        } else if graph.sim.as_deref() == Some(unit.package_id.as_str()) {
            if unit.is("lib") || unit.is("rlib") {
                sim_library = true;
            }
            match (expect.sim, unit.opt_level.optimised()) {
                (SimRule::OptimisedIfBuilt, false) => {
                    faults.push(ProfileFault::SimUnoptimised { target });
                }
                (SimRule::Unoptimised, true) => faults.push(ProfileFault::SimOptimised {
                    target,
                    opt_level: unit.opt_level,
                }),
                _ => {}
            }
        } else if member {
            // A workspace member other than the sim: its checks are judged
            // above, and its optimisation is not the suite's to require.
        } else if expect.closure_optimised
            && graph.sim_closure.contains(&unit.package_id)
            && !unit.opt_level.optimised()
        {
            faults.push(ProfileFault::DependencyUnoptimised {
                package: package.clone(),
                target,
            });
        } else if expect.family && graph.godot_family.contains(&unit.package_id) {
            if unit.opt_level.optimised() {
                faults.push(ProfileFault::GodotOptimised {
                    package: package.clone(),
                    target,
                    opt_level: unit.opt_level,
                });
            }
        } else if expect.family && !unit.opt_level.optimised() {
            faults.push(ProfileFault::StrayUnoptimised {
                package: package.clone(),
                target,
            });
        }
    }
    if matches!(expect.sim, SimRule::Unoptimised | SimRule::Built) && !sim_library {
        faults.push(ProfileFault::SimNotBuilt);
    }
    faults
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`parse_messages`] with its diagnostics dropped.
    fn read(text: &str) -> Result<Vec<Built>, ProfileFault> {
        parse_messages(text, &mut |_| {})
    }

    const SIM_ID: &str = "path+file:///w/crates/sim#pharmakos-sim@0.0.0";
    const PROTO_ID: &str = "path+file:///w/crates/proto#pharmakos-proto@0.0.0";
    const GATEWAY_ID: &str = "path+file:///w/crates/gateway#pharmakos-gateway@0.0.0";
    const CLIENT_ID: &str = "path+file:///w/crates/client-gdext#pharmakos-client-gdext@0.0.0";
    const XXH_ID: &str = "registry+https://github.com/rust-lang/crates.io-index#xxhash-rust@0.8.18";
    const PROST_ID: &str = "registry+https://github.com/rust-lang/crates.io-index#prost@0.14.4";
    const SERDE_ID: &str = "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.229";
    const DERIVE_ID: &str =
        "registry+https://github.com/rust-lang/crates.io-index#serde_derive@1.0.229";
    const SYN_ID: &str = "registry+https://github.com/rust-lang/crates.io-index#syn@2.0.0";
    const LEXOPT_ID: &str = "registry+https://github.com/rust-lang/crates.io-index#lexopt@0.3.2";
    const GODOT_ID: &str = "registry+https://github.com/rust-lang/crates.io-index#godot@0.5.5";
    const GODOT_CORE_ID: &str =
        "registry+https://github.com/rust-lang/crates.io-index#godot-core@0.5.5";
    const API_ID: &str =
        "registry+https://github.com/rust-lang/crates.io-index#gdextension-api@0.3.0";

    /// A workspace of four members:
    /// * the sim runs proto (which runs prost), xxhash and serde, whose derive
    ///   is a proc-macro that runs syn;
    /// * the gateway runs the sim and lexopt, and builds syn and xxhash for its
    ///   tests alone;
    /// * the client runs godot, which runs godot-core, which runs
    ///   gdextension-api (no `godot` prefix) and syn, which others reach too.
    fn metadata() -> String {
        let package = |id: &str, name: &str, kind: &str| {
            let version = id.rsplit_once('@').map_or("0.0.0", |(_, version)| version);
            format!(
                r#"{{"id":"{id}","name":"{name}","version":"{version}","targets":[{{"kind":["{kind}"],"name":"x"}}]}}"#
            )
        };
        let dep = |pkg: &str, kind: &str| {
            format!(r#"{{"name":"x","pkg":"{pkg}","dep_kinds":[{{"kind":{kind},"target":null}}]}}"#)
        };
        let node =
            |id: &str, deps: &[String]| format!(r#"{{"id":"{id}","deps":[{}]}}"#, deps.join(","));
        let packages = [
            package(SIM_ID, "pharmakos-sim", "lib"),
            package(PROTO_ID, "pharmakos-proto", "lib"),
            package(GATEWAY_ID, "pharmakos-gateway", "lib"),
            package(CLIENT_ID, "pharmakos-client-gdext", "cdylib"),
            package(XXH_ID, "xxhash-rust", "lib"),
            package(PROST_ID, "prost", "lib"),
            package(SERDE_ID, "serde", "lib"),
            package(DERIVE_ID, "serde_derive", "proc-macro"),
            package(SYN_ID, "syn", "lib"),
            package(LEXOPT_ID, "lexopt", "lib"),
            package(GODOT_ID, "godot", "lib"),
            package(GODOT_CORE_ID, "godot-core", "lib"),
            package(API_ID, "gdextension-api", "lib"),
        ];
        let nodes = [
            node(
                SIM_ID,
                &[
                    dep(PROTO_ID, "null"),
                    dep(XXH_ID, "null"),
                    dep(SERDE_ID, "null"),
                ],
            ),
            node(PROTO_ID, &[dep(PROST_ID, "null")]),
            node(
                GATEWAY_ID,
                &[
                    dep(SIM_ID, "null"),
                    dep(LEXOPT_ID, "null"),
                    dep(XXH_ID, "\"dev\""),
                    dep(SYN_ID, "\"dev\""),
                ],
            ),
            node(CLIENT_ID, &[dep(GODOT_ID, "null")]),
            node(XXH_ID, &[]),
            node(PROST_ID, &[]),
            node(SERDE_ID, &[dep(DERIVE_ID, "null")]),
            node(DERIVE_ID, &[dep(SYN_ID, "null")]),
            node(SYN_ID, &[]),
            node(LEXOPT_ID, &[]),
            node(GODOT_ID, &[dep(GODOT_CORE_ID, "null")]),
            node(
                GODOT_CORE_ID,
                &[dep(API_ID, "\"build\""), dep(SYN_ID, "\"build\"")],
            ),
            node(API_ID, &[]),
        ];
        format!(
            r#"{{"packages":[{}],
                "workspace_members":["{SIM_ID}","{PROTO_ID}","{GATEWAY_ID}","{CLIENT_ID}"],
                "resolve":{{"nodes":[{}],"root":null}}}}"#,
            packages.join(","),
            nodes.join(",")
        )
    }

    /// One `compiler-artifact` line as cargo 1.98.1 writes it, trimmed to the
    /// members the check reads (and a few it does not).
    fn artifact(id: &str, target: &str, opt: &str, assertions: bool, overflow: bool) -> String {
        format!(
            r#"{{"reason":"compiler-artifact","package_id":"{id}","manifest_path":"C:\\w\\Cargo.toml","target":{{"kind":["lib"],"crate_types":["lib"],"name":"{target}","src_path":"C:\\w\\src\\lib.rs","edition":"2024","doc":true,"doctest":true,"test":true}},"profile":{{"opt_level":"{opt}","debuginfo":2,"debug_assertions":{assertions},"overflow_checks":{overflow},"test":false}},"features":[],"filenames":[],"executable":null,"fresh":true}}"#
        )
    }

    const FINISHED: &str = r#"{"reason":"build-finished","success":true}"#;

    /// The build this lane commits: the sim and its third-party crates
    /// optimised, everything else at 0, every check on.
    fn good() -> Vec<String> {
        vec![
            artifact(XXH_ID, "xxhash_rust", "2", true, true),
            artifact(PROST_ID, "prost", "2", true, true),
            artifact(SYN_ID, "syn", "2", true, true),
            artifact(DERIVE_ID, "serde_derive", "2", true, true),
            artifact(SERDE_ID, "serde", "2", true, true),
            artifact(PROTO_ID, "pharmakos_proto", "0", true, true),
            artifact(SIM_ID, "pharmakos_sim", "2", true, true),
            artifact(SIM_ID, "determinism", "2", true, true),
            artifact(LEXOPT_ID, "lexopt", "2", true, true),
            artifact(GATEWAY_ID, "pharmakos_gateway", "0", true, true),
            artifact(GODOT_ID, "godot", "0", true, true),
            artifact(GODOT_CORE_ID, "godot_core", "0", true, true),
            artifact(API_ID, "gdextension_api", "0", true, true),
        ]
    }

    /// `lines` and a successful end.
    fn finished(lines: &[String]) -> String {
        let mut all = lines.to_vec();
        all.push(FINISHED.to_owned());
        all.join("\n")
    }

    fn judged(lines: &[String], expect: Expect) -> Vec<ProfileFault> {
        let graph = graph(&metadata()).expect("the graph reads");
        let built = read(&finished(lines)).expect("the messages read");
        faults(&built, &graph, expect)
    }

    fn suite() -> Expect {
        Expect::suite(SimRule::OptimisedIfBuilt)
    }

    #[test]
    fn the_sims_closure_is_the_code_it_runs() {
        let graph = graph(&metadata()).expect("the graph reads");
        assert_eq!(
            graph.sim_closure,
            BTreeSet::from([
                PROTO_ID.to_owned(),
                XXH_ID.to_owned(),
                PROST_ID.to_owned(),
                SERDE_ID.to_owned()
            ]),
            "serde_derive is a proc-macro and syn is reached only through it"
        );
        assert_eq!(graph.sim.as_deref(), Some(SIM_ID));
    }

    #[test]
    fn the_godot_family_is_what_only_godot_reaches() {
        let graph = graph(&metadata()).expect("the graph reads");
        assert_eq!(
            graph.godot_family_names(),
            ["gdextension-api", "godot", "godot-core"],
            "syn is reached by the gateway too"
        );
        assert_eq!(
            graph.reaches("pharmakos-gateway", "pharmakos-sim"),
            Ok(true)
        );
        assert_eq!(
            graph.reaches("pharmakos-client-gdext", "pharmakos-sim"),
            Ok(false)
        );
        assert_eq!(graph.reaches("pharmakos-sim", "pharmakos-sim"), Ok(false));
        assert!(matches!(
            graph.reaches("nobody", "pharmakos-sim"),
            Err(ProfileFault::Metadata(_))
        ));
    }

    #[test]
    fn the_committed_shape_passes() {
        assert_eq!(judged(&good(), suite()), Vec::new());
    }

    #[test]
    fn a_workspace_unit_with_a_check_off_is_refused() {
        for (assertions, overflow, check) in [
            (false, true, "debug assertions"),
            (true, false, "overflow checks"),
        ] {
            let mut lines = good();
            lines.push(artifact(GATEWAY_ID, "methods", "0", assertions, overflow));
            assert_eq!(
                judged(&lines, suite()),
                [ProfileFault::CheckOff {
                    package: "pharmakos-gateway".to_owned(),
                    target: "methods".to_owned(),
                    check,
                }]
            );
        }
        // A third-party crate's checks are its own business.
        let mut lines = good();
        lines.push(artifact(LEXOPT_ID, "lexopt", "2", false, false));
        assert_eq!(judged(&lines, suite()), Vec::new());
    }

    /// `release-checked` promises both checks; a release build promises
    /// overflow checks alone, and judges no optimisation.
    #[test]
    fn a_release_build_is_held_to_what_its_profile_promises() {
        let release = vec![
            artifact(SIM_ID, "pharmakos_sim", "3", false, true),
            artifact(PROTO_ID, "pharmakos_proto", "3", false, true),
            artifact(XXH_ID, "xxhash_rust", "0", false, false),
            artifact(GODOT_CORE_ID, "godot_core", "3", false, true),
        ];
        assert_eq!(judged(&release, Expect::RELEASE), Vec::new());
        let unchecked = vec![artifact(SIM_ID, "pharmakos_sim", "3", false, false)];
        assert_eq!(
            judged(&unchecked, Expect::RELEASE),
            [ProfileFault::CheckOff {
                package: "pharmakos-sim".to_owned(),
                target: "pharmakos_sim".to_owned(),
                check: "overflow checks",
            }]
        );
        let checked = vec![artifact(SIM_ID, "determinism", "3", true, true)];
        assert_eq!(
            judged(&checked, Expect::release_checked(OptLevel::O3)),
            Vec::new()
        );
        let unasserted = vec![artifact(SIM_ID, "determinism", "3", false, true)];
        assert_eq!(
            judged(&unasserted, Expect::release_checked(OptLevel::O3)),
            [ProfileFault::CheckOff {
                package: "pharmakos-sim".to_owned(),
                target: "determinism".to_owned(),
                check: "debug assertions",
            }]
        );
    }

    /// Whatever spelling put it there (`[profile.test]` with a dotted
    /// `package.pharmakos-sim.opt-level = 0`, a `--config`, a config file
    /// outside the repository), the sim at 0 is what cargo reports.
    #[test]
    fn the_sim_at_opt_level_0_is_refused_unless_the_invocation_asks_for_it() {
        let mut lines = good();
        lines.push(artifact(SIM_ID, "allocations", "0", true, true));
        assert_eq!(
            judged(&lines, suite()),
            [ProfileFault::SimUnoptimised {
                target: "allocations".to_owned()
            }]
        );
        // The allocation test's own invocation requires 0 of every sim unit.
        let lines = vec![
            artifact(XXH_ID, "xxhash_rust", "2", true, true),
            artifact(SIM_ID, "pharmakos_sim", "0", true, true),
            artifact(SIM_ID, "allocations", "0", true, true),
        ];
        let unoptimised = Expect::suite(SimRule::Unoptimised);
        assert_eq!(judged(&lines, unoptimised), Vec::new());
        let lines = vec![artifact(SIM_ID, "pharmakos_sim", "1", true, true)];
        assert_eq!(
            judged(&lines, unoptimised),
            [ProfileFault::SimOptimised {
                target: "pharmakos_sim".to_owned(),
                opt_level: OptLevel::O1
            }]
        );
        // ... and requires the sim to have been built at all.
        let lines = vec![artifact(XXH_ID, "xxhash_rust", "2", true, true)];
        assert_eq!(judged(&lines, unoptimised), [ProfileFault::SimNotBuilt]);
        // An optimised invocation that reaches no sim target builds none.
        assert_eq!(judged(&lines, suite()), Vec::new());
    }

    #[test]
    fn a_crate_the_sim_runs_at_opt_level_0_is_refused_and_a_proc_macro_is_not() {
        for (id, name) in [
            (XXH_ID, "xxhash-rust"),
            (PROST_ID, "prost"),
            (SERDE_ID, "serde"),
        ] {
            let mut lines = good();
            lines.push(artifact(id, "x", "0", true, true));
            assert_eq!(
                judged(&lines, suite()),
                [ProfileFault::DependencyUnoptimised {
                    package: name.to_owned(),
                    target: "x".to_owned()
                }],
                "{name}"
            );
        }
        // A proc-macro runs in the compiler, and what only it reaches with it,
        // so the closure's rule does not judge them: at 0 they are the family
        // rule's, a table to remove. A workspace member the sim runs may stay
        // at 0 (only the sim is optimised among the members).
        for (id, name) in [(DERIVE_ID, "serde_derive"), (SYN_ID, "syn")] {
            let mut lines = good();
            lines.push(artifact(id, name, "0", true, true));
            assert_eq!(
                judged(&lines, suite()),
                [ProfileFault::StrayUnoptimised {
                    package: name.to_owned(),
                    target: name.to_owned()
                }],
                "{name}"
            );
        }
        let mut lines = good();
        lines.push(artifact(PROTO_ID, "pharmakos_proto", "0", true, true));
        assert_eq!(judged(&lines, suite()), Vec::new());
    }

    #[test]
    fn an_optimised_crate_of_the_godot_family_is_refused_by_its_table() {
        for (id, name) in [
            (GODOT_ID, "godot"),
            (GODOT_CORE_ID, "godot-core"),
            (API_ID, "gdextension-api"),
        ] {
            let mut lines = good();
            lines.push(artifact(id, "x", "2", true, true));
            let faults = judged(&lines, suite());
            assert_eq!(
                faults,
                [ProfileFault::GodotOptimised {
                    package: name.to_owned(),
                    target: "x".to_owned(),
                    opt_level: OptLevel::O2
                }],
                "{name}"
            );
            assert!(
                faults.iter().any(|fault| fault
                    .to_string()
                    .contains(&format!("[profile.dev.package.{name}]"))),
                "the fault names the table: {faults:?}"
            );
        }
        // syn is reached by others too, so it is not of the family.
        let mut lines = good();
        lines.push(artifact(SYN_ID, "syn", "2", true, true));
        assert_eq!(judged(&lines, suite()), Vec::new());
    }

    #[test]
    fn an_unknown_package_is_refused() {
        let lines = vec![artifact(
            "registry+x#stranger@1.0.0",
            "stranger",
            "2",
            true,
            true,
        )];
        assert_eq!(
            judged(&lines, suite()),
            [ProfileFault::UnknownPackage {
                package_id: "registry+x#stranger@1.0.0".to_owned()
            }]
        );
    }

    /// Every path a report names is absolute, as cargo writes it (it joins a
    /// relative `CARGO_TARGET_DIR` to its working directory), so what xtask
    /// removes, runs, stages and packages never depends on xtask's own
    /// working directory. A relative one is refused, not resolved.
    #[test]
    fn a_reported_path_is_absolute_or_refused() {
        let base = artifact(SIM_ID, "pharmakos_sim", "2", true, true);
        for (field, absolute, relative) in [
            (
                "\"executable\":null",
                "\"executable\":\"/w/target/debug/deps/sim-1\"",
                "\"executable\":\"target/debug/deps/sim-1\"",
            ),
            (
                "\"filenames\":[]",
                "\"filenames\":[\"D:\\\\w\\\\target\\\\debug\\\\libsim.rlib\"]",
                "\"filenames\":[\"target\\\\debug\\\\libsim.rlib\"]",
            ),
        ] {
            let held = base.replace(field, absolute);
            assert!(read(&format!("{held}\n{FINISHED}")).is_ok(), "{held}");
            let refused = base.replace(field, relative);
            assert!(
                matches!(
                    read(&format!("{refused}\n{FINISHED}")),
                    Err(ProfileFault::Unreadable { line: 1, .. })
                ),
                "{refused}"
            );
        }
    }

    #[test]
    fn the_messages_are_read_strictly() {
        let fails = |text: &str| read(text).expect_err(text);
        assert!(matches!(
            fails("   Compiling x v1.0.0"),
            ProfileFault::Unreadable { line: 1, .. }
        ));
        assert!(matches!(
            fails("{\"no_reason\":1}"),
            ProfileFault::Unreadable { line: 1, .. }
        ));
        assert_eq!(
            fails("{\"reason\":\"timing-info\"}"),
            ProfileFault::UnknownReason {
                line: 1,
                reason: "timing-info".to_owned()
            }
        );
        // An artifact without its resolved profile is not skipped.
        let stripped = artifact(SIM_ID, "pharmakos_sim", "2", true, true)
            .replace(",\"overflow_checks\":true", "");
        assert!(matches!(
            fails(&finished(&[stripped])),
            ProfileFault::Unreadable { line: 1, .. }
        ));
        let stringly = artifact(SIM_ID, "pharmakos_sim", "2", true, true)
            .replace("\"debug_assertions\":true", "\"debug_assertions\":\"true\"");
        assert!(matches!(
            fails(&finished(&[stringly])),
            ProfileFault::Unreadable { line: 1, .. }
        ));
        // Cargo's six levels, and nothing else.
        for bad in ["", " 0", "4", "O2", "fast"] {
            let line = artifact(SIM_ID, "pharmakos_sim", bad, true, true);
            assert_eq!(
                fails(&finished(&[line])),
                ProfileFault::BadOptLevel {
                    line: 1,
                    value: bad.to_owned()
                },
                "{bad:?}"
            );
        }
        for good in ["0", "1", "2", "3", "s", "z"] {
            let line = artifact(SIM_ID, "pharmakos_sim", good, true, true);
            assert!(read(&finished(&[line])).is_ok(), "{good}");
        }
        // A duplicate key is not read as either value.
        let doubled = artifact(SIM_ID, "pharmakos_sim", "2", true, true).replace(
            "\"opt_level\":\"2\"",
            "\"opt_level\":\"2\",\"opt_level\":\"0\"",
        );
        assert!(matches!(
            fails(&finished(&[doubled])),
            ProfileFault::Unreadable { line: 1, .. }
        ));
        assert_eq!(read(&finished(&good())).map(|built| built.len()), Ok(13));
    }

    /// The reader never passes on nothing: the stream ends in exactly one
    /// `build-finished` that says the build succeeded.
    #[test]
    fn the_reader_never_passes_on_nothing() {
        let fails = |text: &str| read(text).expect_err(text);
        assert_eq!(fails(""), ProfileFault::NothingBuilt);
        assert_eq!(fails("\n  \n"), ProfileFault::NothingBuilt);
        let unfinished = artifact(SIM_ID, "pharmakos_sim", "2", true, true);
        assert_eq!(fails(&unfinished), ProfileFault::Unfinished);
        assert_eq!(
            fails("{\"reason\":\"build-finished\",\"success\":false}"),
            ProfileFault::BuildFailed
        );
        assert_eq!(
            fails(&format!("{FINISHED}\n{FINISHED}")),
            ProfileFault::FinishedTwice { line: 2 }
        );
        assert_eq!(
            fails(&format!("{FINISHED}\n{unfinished}")),
            ProfileFault::FinishedTwice { line: 2 }
        );
        assert!(matches!(
            fails("{\"reason\":\"build-finished\"}"),
            ProfileFault::Unreadable { line: 1, .. }
        ));
        // A build of nothing new is still a reported build.
        assert_eq!(read(FINISHED), Ok(Vec::new()));
    }

    #[test]
    fn metadata_without_the_graph_is_refused() {
        for text in [
            "{}",
            r#"{"packages":[],"workspace_members":[]}"#,
            r#"{"packages":[],"workspace_members":[],"resolve":{"nodes":[]}}"#,
        ] {
            assert!(
                matches!(graph(text), Err(ProfileFault::Metadata(_))),
                "{text}"
            );
        }
        // The sim with no node in the resolve graph is not a sim with no
        // dependencies.
        let nodeless = metadata().replace(
            &format!("\"id\":\"{SIM_ID}\",\"deps\""),
            "\"id\":\"elsewhere\",\"deps\"",
        );
        assert!(matches!(graph(&nodeless), Err(ProfileFault::Metadata(_))));
        // A dependency of no kind is not read as "not normal".
        let kindless = metadata().replacen(
            "\"dep_kinds\":[{\"kind\":null,\"target\":null}]",
            "\"dep_kinds\":[]",
            1,
        );
        assert!(matches!(graph(&kindless), Err(ProfileFault::Metadata(_))));
    }

    #[test]
    fn the_allocation_test_runs_alone_and_every_unit_runs_once() {
        let units = vec![
            Unit::Lib,
            Unit::Test("allocations".to_owned()),
            Unit::Test("determinism".to_owned()),
            Unit::Doc,
        ];
        let runs = invocations(&units, true);
        assert_eq!(
            runs,
            [
                Invocation {
                    selectors: ["--lib", "--test", "determinism"]
                        .map(str::to_owned)
                        .to_vec(),
                    sim: SimRule::OptimisedIfBuilt
                },
                Invocation {
                    selectors: vec!["--doc".to_owned()],
                    sim: SimRule::OptimisedIfBuilt
                },
                Invocation {
                    selectors: ["--test", "allocations"].map(str::to_owned).to_vec(),
                    sim: SimRule::Unoptimised
                },
            ]
        );
        // A selection without the sim runs its `allocations` with the rest.
        assert_eq!(
            invocations(&units, false)
                .into_iter()
                .map(|run| (run.selectors, run.sim))
                .collect::<Vec<_>>(),
            crate::shard::selectors(&units)
                .into_iter()
                .map(|selectors| (selectors, SimRule::OptimisedIfBuilt))
                .collect::<Vec<_>>()
        );
        // A part without the allocation test runs as the shards always ran it.
        let part = vec![Unit::Lib, Unit::Doc];
        assert_eq!(
            invocations(&part, true)
                .into_iter()
                .map(|run| run.selectors)
                .collect::<Vec<_>>(),
            crate::shard::selectors(&part)
        );
        assert!(invocations(&[], true).is_empty());
    }

    /// Every test run's check and run are one value: the same `--config`, the
    /// same environment, the same arguments, the check adding only what makes
    /// it a report (`--no-run --message-format=json`), or for a doctest run
    /// being `doc_check` of the run.
    fn assert_check_is_the_run(test: &TestRun) {
        let run = test.run();
        let check = test.check();
        if test.invocation.selectors == ["--doc"] {
            assert_eq!(check, doc_check(&run, test.colour), "{test:?}");
        } else {
            // The run's cargo arguments, before `--` and the harness's, with
            // `--no-run`: nothing else differs.
            let mut expected = run.clone();
            if let Some(at) = expected.args.iter().position(|arg| arg == "--") {
                expected.args.truncate(at);
            }
            expected.args.push("--no-run".to_owned());
            assert_eq!(check, expected, "{test:?}");
        }
        assert_eq!(check.env, pinned_env(""), "{test:?}");
        assert_eq!(check.config, run.config, "{test:?}");
        assert!(
            pinned_config()
                .iter()
                .all(|value| run.config.contains(value)),
            "the run names the pinned tools: {test:?}"
        );
        assert!(
            run.args.contains(&message_format(test.colour).to_owned()),
            "the run reports itself: {test:?}"
        );
        let unoptimised = test.invocation.sim == SimRule::Unoptimised;
        assert_eq!(
            run.config.contains(&SIM_UNOPTIMISED.to_owned()),
            unoptimised,
            "the run carries the sim's level: {test:?}"
        );
    }

    /// Every test run the steps can make, sharded and not, in both steps.
    fn every_test_run() -> Vec<TestRun> {
        let units = vec![
            Unit::Lib,
            Unit::Bin("determinism".to_owned()),
            Unit::Test("allocations".to_owned()),
            Unit::Test("determinism".to_owned()),
            Unit::Doc,
        ];
        let selections: [Vec<String>; 2] = [
            vec!["--workspace".to_owned()],
            [
                "--package",
                "pharmakos-sim",
                "--features",
                "pharmakos-sim/research",
            ]
            .map(str::to_owned)
            .to_vec(),
        ];
        let shards =
            ["1/3", "2/3", "3/3"].map(|text| crate::shard::Shard::parse(text).expect("a shard"));
        let mut runs: Vec<TestRun> = Vec::new();
        for selection in &selections {
            for step in ["test", "test-research"] {
                let parts: Vec<Vec<Unit>> = std::iter::once(units.clone())
                    .chain(shards.iter().map(|shard| {
                        units
                            .iter()
                            .filter(|unit| shard.holds(step, &unit.key()))
                            .cloned()
                            .collect()
                    }))
                    .collect();
                for part in parts {
                    for invocation in invocations(&part, true) {
                        for locked in [false, true] {
                            for colour in [false, true] {
                                runs.push(TestRun::new(
                                    selection.clone(),
                                    invocation.clone(),
                                    locked,
                                    colour,
                                ));
                            }
                        }
                    }
                }
            }
        }
        runs
    }

    #[test]
    fn the_check_and_the_run_are_one_value() {
        let runs = every_test_run();
        assert!(
            runs.iter()
                .any(|run| run.invocation.sim == SimRule::Unoptimised),
            "the allocation test's run is among them"
        );
        for test in &runs {
            assert_check_is_the_run(test);
        }
        // The doctest route, spelled out once.
        let doc = TestRun::new(
            vec!["--workspace".to_owned()],
            Invocation {
                selectors: vec!["--doc".to_owned()],
                sim: SimRule::OptimisedIfBuilt,
            },
            true,
            false,
        );
        assert_eq!(
            doc.check().command_line(),
            [
                "--config",
                "build.rustc=\"rustc\"",
                "--config",
                "build.rustdoc=\"rustdoc\"",
                "build",
                "--workspace",
                "--locked",
                "--profile",
                "test",
                "--lib",
                "--message-format=json"
            ]
        );
        let alloc = TestRun::new(
            vec!["--workspace".to_owned()],
            Invocation {
                selectors: ["--test", "allocations"].map(str::to_owned).to_vec(),
                sim: SimRule::Unoptimised,
            },
            false,
            true,
        );
        assert_eq!(
            alloc.run().command_line(),
            [
                "--config",
                "build.rustc=\"rustc\"",
                "--config",
                "build.rustdoc=\"rustdoc\"",
                "--config",
                "profile.dev.package.pharmakos-sim.opt-level=0",
                "test",
                "--workspace",
                "--test",
                "allocations",
                "--message-format=json-diagnostic-rendered-ansi"
            ]
        );
        assert_eq!(
            alloc.check().args.last().map(String::as_str),
            Some("--no-run")
        );
        // The harness's arguments go after `--`, in the run and never in the
        // check, which only builds.
        let fuzz = TestRun::new(
            vec!["--package".to_owned(), "pharmakos-verifier".to_owned()],
            Invocation {
                selectors: vec!["--test".to_owned(), "fuzz".to_owned()],
                sim: SimRule::OptimisedIfBuilt,
            },
            true,
            false,
        )
        .with_harness(vec![Harness::Ignored, Harness::Nocapture]);
        assert!(fuzz.run().args.ends_with(&[
            "--".to_owned(),
            "--ignored".to_owned(),
            "--nocapture".to_owned()
        ]));
        assert!(!fuzz.check().args.iter().any(|arg| arg == "--ignored"));
        assert_check_is_the_run(&fuzz);
        let measured = fuzz.clone().release();
        assert!(measured.run().args.contains(&"--release".to_owned()));
        assert_eq!(measured.expect(), Expect::RELEASE);
        assert_check_is_the_run(&measured);
    }

    /// What a [`Cargo`] was asked to do, in order.
    #[derive(Debug, PartialEq, Eq)]
    enum Asked {
        Report(Vec<String>, Vec<EnvChange>),
        Run(Vec<String>, Vec<EnvChange>),
    }

    /// A [`Cargo`] that records what it is asked and reports a build of the
    /// sim's library at the level the invocation requires.
    struct Recording {
        asked: Vec<Asked>,
        sim_level: &'static str,
    }

    impl Cargo for Recording {
        fn report(
            &mut self,
            line: &[String],
            env: &[EnvChange],
            stdout: &mut dyn FnMut(&str),
        ) -> Result<bool, String> {
            self.asked.push(Asked::Report(line.to_vec(), env.to_vec()));
            let report = finished(&[artifact(
                SIM_ID,
                "pharmakos_sim",
                self.sim_level,
                true,
                true,
            )]);
            for line in report.lines() {
                stdout(line);
            }
            Ok(true)
        }

        fn run(
            &mut self,
            line: &[String],
            env: &[EnvChange],
            _stdout: &mut dyn FnMut(&str),
        ) -> Result<(bool, Vec<String>), String> {
            self.asked.push(Asked::Run(line.to_vec(), env.to_vec()));
            Ok((true, Vec::new()))
        }

        fn working_dir(&self) -> Result<String, String> {
            Ok("D:/w".to_owned())
        }
    }

    /// The one door: `execute` hands cargo exactly the check, then exactly
    /// the run, for every test run the steps can make.
    #[test]
    fn execute_hands_cargo_the_check_then_the_run_and_nothing_else() {
        let graph = graph(&metadata()).expect("the graph reads");
        for test in every_test_run() {
            let mut cargo = Recording {
                asked: Vec::new(),
                sim_level: if test.invocation.sim == SimRule::Unoptimised {
                    "0"
                } else {
                    "2"
                },
            };
            execute(&test, &graph, &mut cargo, &mut |_| {}, &mut |_| {}).expect("the check holds");
            let (check, run) = (test.check(), test.run());
            assert_eq!(
                cargo.asked,
                [
                    Asked::Report(check.command_line(), check.env),
                    Asked::Run(run.command_line(), run.env)
                ],
                "{test:?}"
            );
        }
        // A check that fails runs nothing.
        let alloc = TestRun::new(
            vec!["--workspace".to_owned()],
            Invocation {
                selectors: ["--test", "allocations"].map(str::to_owned).to_vec(),
                sim: SimRule::Unoptimised,
            },
            false,
            false,
        );
        let mut cargo = Recording {
            asked: Vec::new(),
            sim_level: "2",
        };
        let error =
            execute(&alloc, &graph, &mut cargo, &mut |_| {}, &mut |_| {}).expect_err("optimised");
        assert!(error.contains("opt-level 2"), "{error}");
        assert_eq!(cargo.asked.len(), 1, "the run never started");
    }

    /// A failed build's report says why: each diagnostic's rendered text is
    /// handed on as it is read, before the failure is.
    #[test]
    fn a_failed_build_hands_on_its_diagnostics() {
        let diagnostic = r#"{"reason":"compiler-message","package_id":"x","manifest_path":"m","target":{"kind":["test"],"name":"t"},"message":{"rendered":"error[E0308]: mismatched types\n --> tests/t.rs:1:14\n","$message_type":"diagnostic","children":[],"code":{"code":"E0308","explanation":null},"level":"error","message":"mismatched types","spans":[]}}"#;
        let quiet = r#"{"reason":"compiler-message","package_id":"x","manifest_path":"m","target":{"kind":["test"],"name":"t"},"message":{"rendered":null,"message":"x"}}"#;
        let failed = r#"{"reason":"build-finished","success":false}"#;
        let mut seen: Vec<String> = Vec::new();
        let outcome = parse_messages(&format!("{diagnostic}\n{quiet}\n{failed}"), &mut |text| {
            seen.push(text.to_owned());
        });
        assert_eq!(outcome, Err(ProfileFault::BuildFailed));
        assert_eq!(
            seen,
            ["error[E0308]: mismatched types\n --> tests/t.rs:1:14\n"]
        );
        // A message without its diagnostic is not skipped.
        let bare = r#"{"reason":"compiler-message","package_id":"x"}"#;
        assert!(matches!(
            read(&format!("{bare}\n{FINISHED}")),
            Err(ProfileFault::Unreadable { line: 1, .. })
        ));
    }

    /// `release-checked` is held to its whole promise: the release profile's
    /// optimisation in every workspace unit (a build script aside), both
    /// checks, and the sim's library built (a build script of the sim alone
    /// is not the sim).
    #[test]
    fn release_checked_is_what_it_says() {
        let promise = Expect::release_checked(OptLevel::O3);
        let good = vec![
            artifact(SIM_ID, "pharmakos_sim", "3", true, true),
            artifact(PROTO_ID, "pharmakos_proto", "3", true, true),
            artifact(XXH_ID, "xxhash_rust", "3", true, true),
            // A crate the sim does not run is not the gate's to judge.
            artifact(LEXOPT_ID, "lexopt", "0", true, true),
        ];
        assert_eq!(judged(&good, promise), Vec::new());
        // The state hash's own crate, or any crate the sim runs, off the
        // release level: the gate would hash code release does not ship.
        let mut hash_crate = good.clone();
        hash_crate.push(artifact(XXH_ID, "xxhash_rust", "0", true, true));
        assert_eq!(
            judged(&hash_crate, promise),
            [ProfileFault::WrongOptLevel {
                package: "xxhash-rust".to_owned(),
                target: "xxhash_rust".to_owned(),
                found: OptLevel::O0,
                promised: OptLevel::O3
            }]
        );
        let unoptimised = vec![artifact(SIM_ID, "pharmakos_sim", "0", true, true)];
        assert_eq!(
            judged(&unoptimised, promise),
            [ProfileFault::WrongOptLevel {
                package: "pharmakos-sim".to_owned(),
                target: "pharmakos_sim".to_owned(),
                found: OptLevel::O0,
                promised: OptLevel::O3
            }]
        );
        let script = artifact(SIM_ID, "build-script-build", "0", true, true)
            .replace("\"kind\":[\"lib\"]", "\"kind\":[\"custom-build\"]");
        assert_eq!(
            judged(std::slice::from_ref(&script), promise),
            [ProfileFault::SimNotBuilt]
        );
        let mut with_lib = good;
        with_lib.push(script);
        assert_eq!(judged(&with_lib, promise), Vec::new());
    }

    /// Only the godot family may sit at opt-level 0 among third-party crates
    /// in the suite's builds; any other is a table to remove.
    #[test]
    fn a_third_party_crate_outside_the_family_at_0_is_a_table_to_remove() {
        let mut lines = good();
        lines.push(artifact(LEXOPT_ID, "lexopt", "0", true, true));
        let faults = judged(&lines, suite());
        assert_eq!(
            faults,
            [ProfileFault::StrayUnoptimised {
                package: "lexopt".to_owned(),
                target: "lexopt".to_owned()
            }]
        );
        assert!(
            faults.iter().any(|fault| fault
                .to_string()
                .contains("remove `[profile.dev.package.lexopt]`")),
            "{faults:?}"
        );
        // A release build judges no third-party optimisation.
        assert_eq!(judged(&lines, Expect::RELEASE), Vec::new());
    }

    #[test]
    fn the_exemption_is_its_owners_and_none_of_them_reaches_the_sim() {
        let graph = graph(&metadata()).expect("the graph reads");
        // In this graph no mesher exists: the table's second owner is missing.
        let fault = check_owners("allocations", &[SIM.to_owned()], &graph).expect_err("changed");
        assert!(
            matches!(fault, ProfileFault::OwnersChanged { .. }),
            "{fault:?}"
        );
        // A third owner is refused.
        let owners = [SIM, "pharmakos-mesher", "pharmakos-gateway"].map(str::to_owned);
        assert!(matches!(
            check_owners("allocations", &owners, &graph),
            Err(ProfileFault::OwnersChanged { .. })
        ));
        // An owner that depends on the sim is refused, whatever the table says.
        let mesher = metadata().replace("pharmakos-gateway", "pharmakos-mesher");
        let graph = super::graph(&mesher).expect("the graph reads");
        assert_eq!(
            check_owners(
                "allocations",
                &[SIM.to_owned(), "pharmakos-mesher".to_owned()],
                &graph
            ),
            Err(ProfileFault::OwnerReachesSim {
                test: "allocations".to_owned(),
                package: "pharmakos-mesher".to_owned()
            })
        );
        // The client stands in for an owner that does not reach the sim.
        let client = metadata().replace("pharmakos-client-gdext", "pharmakos-mesher");
        let graph = super::graph(&client).expect("the graph reads");
        assert_eq!(
            check_owners(
                "allocations",
                &[SIM.to_owned(), "pharmakos-mesher".to_owned()],
                &graph
            ),
            Ok(())
        );
    }

    /// A run that ran its test executables: each started (`Running`) and as
    /// many libtest results as starts. A runner that runs nothing leaves the
    /// starts and no result.
    #[test]
    fn a_run_whose_tests_did_not_run_is_refused() {
        let expected = vec![
            "D:\\t\\debug\\deps\\proto-1.exe".to_owned(),
            "D:\\t\\debug\\deps\\fallbacks-2.exe".to_owned(),
        ];
        let stderr = vec![
            "   Compiling pharmakos-proto v0.0.0".to_owned(),
            "     Running unittests src\\lib.rs (D:/t\\debug\\deps\\proto-1.exe)".to_owned(),
            "\u{1b}[1m\u{1b}[92m     Running\u{1b}[0m tests\\fallbacks.rs (D:/t\\debug\\deps\\fallbacks-2.exe)"
                .to_owned(),
        ];
        assert_eq!(judge_run(&expected, &stderr, 2, "D:/w", true), Ok(()));
        // The runner ran nothing: both started, no result.
        assert_eq!(
            judge_run(&expected, &stderr, 0, "D:/w", true),
            Err(ProfileFault::TestsDidNotRun {
                expected: 2,
                started: 2,
                results: 0,
                missing: Vec::new(),
                unreported: Vec::new()
            })
        );
        // An executable never started.
        let one = stderr.get(..2).map(<[String]>::to_vec).unwrap_or_default();
        assert!(matches!(
            judge_run(&expected, &one, 1, "D:/w", true),
            Err(ProfileFault::TestsDidNotRun { started: 1, ref missing, .. }) if missing.len() == 1
        ));
        // Case matters where the file system's names do, and on Windows it
        // does not: cargo spells the drive one way and the report another.
        let lower: Vec<String> = expected
            .iter()
            .map(|path| path.replace("D:", "d:"))
            .collect();
        assert_eq!(judge_run(&lower, &stderr, 2, "D:/w", true), Ok(()));
        assert!(judge_run(&lower, &stderr, 2, "D:/w", false).is_err());
        assert_eq!(
            running_path("     Running unittests src/lib.rs (/t/debug/deps/x-1)"),
            Some("/t/debug/deps/x-1".to_owned())
        );
        assert_eq!(running_path("   Compiling x v1.0.0 (/t/x)"), None);
        assert_eq!(without_ansi("\u{1b}[1mRunning\u{1b}[0m"), "Running");
    }

    /// Lines as cargo writes them when the target directory lies under its
    /// working directory (CI's in-tree `target`): the executable's path
    /// relative to that directory, while the check's report names it in full.
    fn owned(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| (*line).to_owned()).collect()
    }

    /// Linux's and macOS's relative form, as CI's legs print it.
    #[test]
    fn a_relative_running_line_names_a_unix_executable() {
        let expected = owned(&[
            "/home/runner/work/pharmakos/pharmakos/target/debug/deps/pharmakos_sim-0123abcd",
            "/home/runner/work/pharmakos/pharmakos/target/debug/deps/fallbacks-4567ef01",
        ]);
        let stderr = owned(&[
            "   Compiling pharmakos-sim v0.0.0 (/home/runner/work/pharmakos/pharmakos/crates/sim)",
            "    Finished `test` profile [optimized + debuginfo] target(s) in 1m 02s",
            "     Running unittests src/lib.rs (target/debug/deps/pharmakos_sim-0123abcd)",
            "     Running tests/fallbacks.rs (target/debug/deps/fallbacks-4567ef01)",
            "   Doc-tests pharmakos_sim",
        ]);
        assert_eq!(
            judge_run(
                &expected,
                &stderr,
                2,
                "/home/runner/work/pharmakos/pharmakos",
                false
            ),
            Ok(())
        );
        // The same lines against another working directory name other files.
        assert!(judge_run(&expected, &stderr, 2, "/home/runner/elsewhere", false).is_err());
    }

    /// Windows' relative form: backslashes and `.exe`, against a report that
    /// spells the drive another way.
    #[test]
    fn a_relative_running_line_names_a_windows_executable() {
        let expected = owned(&[
            "D:\\a\\pharmakos\\pharmakos\\target\\debug\\deps\\pharmakos_sim-0123abcd.exe",
            "D:\\a\\pharmakos\\pharmakos\\target\\debug\\deps\\fallbacks-4567ef01.exe",
        ]);
        let stderr = owned(&[
            "     Running unittests src\\lib.rs (target\\debug\\deps\\pharmakos_sim-0123abcd.exe)",
            "\u{1b}[1m\u{1b}[92m     Running\u{1b}[0m tests\\fallbacks.rs (target\\debug\\deps\\fallbacks-4567ef01.exe)",
            "   Doc-tests pharmakos_sim",
        ]);
        assert_eq!(
            judge_run(&expected, &stderr, 2, "D:\\a\\pharmakos\\pharmakos", true),
            Ok(())
        );
        assert_eq!(
            judge_run(&expected, &stderr, 2, "d:/a/pharmakos/pharmakos", true),
            Ok(())
        );
    }

    /// An absolute `Running` path, as cargo prints it for a target
    /// directory outside its working directory, on each operating system.
    #[test]
    fn an_absolute_running_line_names_its_executable() {
        let unix = owned(&["/build/citime/debug/deps/pharmakos_sim-0123abcd"]);
        let unix_lines = owned(&[
            "     Running unittests src/lib.rs (/build/citime/debug/deps/pharmakos_sim-0123abcd)",
        ]);
        assert_eq!(judge_run(&unix, &unix_lines, 1, "/home/p", false), Ok(()));
        let windows = owned(&["D:\\build\\citime\\debug\\deps\\pharmakos_sim-0123abcd.exe"]);
        let windows_lines = owned(&[
            "     Running unittests src\\lib.rs (D:\\build\\citime\\debug\\deps\\pharmakos_sim-0123abcd.exe)",
        ]);
        assert_eq!(
            judge_run(&windows, &windows_lines, 1, "C:\\Users\\p", true),
            Ok(())
        );
    }

    /// A verbatim prefix and `.` or `..` components are compared lexically,
    /// on either side.
    #[test]
    fn a_verbatim_or_dotted_path_is_compared_lexically() {
        let expected = owned(&["\\\\?\\D:\\a\\p\\target\\debug\\deps\\x-1.exe"]);
        let lines = owned(&["     Running unittests src\\lib.rs (target\\debug\\deps\\x-1.exe)"]);
        assert_eq!(judge_run(&expected, &lines, 1, "D:\\a\\p", true), Ok(()));
        assert_eq!(
            judge_run(&expected, &lines, 1, "\\\\?\\D:\\a\\p", true),
            Ok(())
        );
        // A relative `CARGO_TARGET_DIR` reaching out of the working
        // directory: cargo joins it to the working directory unnormalised.
        let dotted = owned(&["/w/p/../t/debug/deps/x-1"]);
        let lines = owned(&["     Running tests/x.rs (/w/t/debug/deps/x-1)"]);
        assert_eq!(judge_run(&dotted, &lines, 1, "/w/p", false), Ok(()));
        let lines = owned(&["     Running tests/x.rs (./t/debug/deps/x-1)"]);
        assert_eq!(
            judge_run(&owned(&["/w/t/debug/deps/x-1"]), &lines, 1, "/w", false),
            Ok(())
        );
    }

    /// The working directory is joined as cargo read it and never resolved
    /// through links: macOS's `/var` is `/private/var` only on disk.
    #[test]
    fn the_working_directory_is_joined_and_never_resolved() {
        let expected = owned(&["/private/var/folders/x/p/target/debug/deps/x-1"]);
        let lines = owned(&["     Running tests/x.rs (target/debug/deps/x-1)"]);
        assert_eq!(
            judge_run(&expected, &lines, 1, "/private/var/folders/x/p", false),
            Ok(())
        );
        assert!(judge_run(&expected, &lines, 1, "/var/folders/x/p", false).is_err());
    }

    /// An executable that started and that the check never reported fails
    /// the run, whatever else ran.
    #[test]
    fn a_start_the_check_never_reported_is_refused() {
        let expected = owned(&["/w/target/debug/deps/x-1"]);
        let lines = owned(&[
            "     Running tests/x.rs (target/debug/deps/x-1)",
            "     Running tests/y.rs (target/debug/deps/y-2)",
        ]);
        assert_eq!(
            judge_run(&expected, &lines, 2, "/w", false),
            Err(ProfileFault::TestsDidNotRun {
                expected: 1,
                started: 2,
                results: 2,
                missing: Vec::new(),
                unreported: vec!["target/debug/deps/y-2".to_owned()],
            })
        );
    }

    /// A doctest header is not a start: the doctests are held to the run's
    /// exit status.
    #[test]
    fn a_doctest_header_is_not_a_start() {
        assert_eq!(running_path("   Doc-tests pharmakos_sim"), None);
        assert_eq!(
            running_path("\u{1b}[1m\u{1b}[92m   Doc-tests\u{1b}[0m pharmakos_sim"),
            None
        );
    }

    /// A [`Cargo`] whose check and run both carry the same warning, and whose
    /// run prints a test's result and a cargo message among its output.
    struct Noisy {
        printed: Vec<String>,
    }

    const WARNING: &str = r#"{"reason":"compiler-message","package_id":"x","manifest_path":"m","target":{"kind":["lib"],"name":"x"},"message":{"rendered":"warning: unused variable: `x`\n","message":"unused"}}"#;

    impl Cargo for Noisy {
        fn report(
            &mut self,
            _line: &[String],
            _env: &[EnvChange],
            stdout: &mut dyn FnMut(&str),
        ) -> Result<bool, String> {
            for line in [
                WARNING,
                artifact(SIM_ID, "pharmakos_sim", "2", true, true)
                    .replace("\"test\":false}", "\"test\":true}")
                    .replace("\"executable\":null", "\"executable\":\"D:/t/sim-1.exe\"")
                    .as_str(),
                WARNING,
                FINISHED,
            ] {
                stdout(line);
            }
            Ok(true)
        }

        fn run(
            &mut self,
            _line: &[String],
            _env: &[EnvChange],
            stdout: &mut dyn FnMut(&str),
        ) -> Result<(bool, Vec<String>), String> {
            for line in [
                WARNING,
                FINISHED,
                "running 0 tests",
                "test result: ok. 0 passed",
            ] {
                stdout(line);
            }
            self.printed.push("ran".to_owned());
            Ok((
                true,
                vec!["     Running unittests src/lib.rs (D:/t/sim-1.exe)".to_owned()],
            ))
        }

        fn working_dir(&self) -> Result<String, String> {
            Ok("D:/w".to_owned())
        }
    }

    /// Each diagnostic is written once for the check and the run together,
    /// and what the tests print reaches the caller as it comes.
    #[test]
    fn a_diagnostic_is_written_once_and_the_tests_output_is_handed_on() {
        let graph = graph(&metadata()).expect("the graph reads");
        let test = TestRun::new(
            vec!["--package".to_owned(), "pharmakos-sim".to_owned()],
            Invocation {
                selectors: vec!["--lib".to_owned()],
                sim: SimRule::OptimisedIfBuilt,
            },
            false,
            false,
        );
        let mut cargo = Noisy {
            printed: Vec::new(),
        };
        let mut diagnostics: Vec<String> = Vec::new();
        let mut output: Vec<String> = Vec::new();
        execute(
            &test,
            &graph,
            &mut cargo,
            &mut |text| diagnostics.push(text.to_owned()),
            &mut |line| output.push(line.to_owned()),
        )
        .expect("it runs");
        assert_eq!(diagnostics, ["warning: unused variable: `x`\n"]);
        assert_eq!(output, ["running 0 tests", "test result: ok. 0 passed"]);
    }

    /// Package specs are matched as cargo matches them: by name, and by
    /// version when the spec gives one, whole or partial.
    #[test]
    fn a_package_spec_is_matched_by_name_and_version() {
        let graph = graph(&metadata()).expect("the graph reads");
        assert!(graph.has_package("godot-core"));
        assert!(graph.has_package("godot-core@0.5.5"));
        assert!(graph.has_package("godot-core@0.5"));
        assert!(!graph.has_package("godot-core@0.5.6"));
        assert!(!graph.has_package("godot-core@0.0.1"));
        assert!(graph.spec_in_family("godot-core@0.5.5"));
        assert!(!graph.spec_in_family("syn"));
        assert!(!graph.spec_in_family("godot-core@9"));
    }

    /// A build script is not the code its package's rules are about: the
    /// sim's at the build-override's level raises nothing.
    #[test]
    fn a_build_script_of_the_sim_raises_nothing() {
        let script = artifact(SIM_ID, "build-script-build", "0", true, true)
            .replace("\"kind\":[\"lib\"]", "\"kind\":[\"custom-build\"]");
        let mut lines = good();
        lines.push(script);
        assert_eq!(judged(&lines, suite()), Vec::new());
    }

    #[test]
    fn a_variable_that_changes_codegen_is_refused_by_name() {
        let pairs = |list: &[(&str, &str)]| {
            list.iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect::<Vec<_>>()
        };
        let environment = pairs(&[
            ("RUSTFLAGS", "-C debug-assertions=off"),
            ("PATH", "C:\\x"),
            ("CARGO_ENCODED_RUSTFLAGS", ""),
            ("RUSTDOCFLAGS", "-Copt-level=0"),
            (
                "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS",
                "-Ctarget-feature=+crt-static",
            ),
            ("CARGO_TARGET_DIR", "D:/build"),
            ("CARGO_BUILD_RUSTFLAGS", "x"),
            ("CARGO_ENCODED_RUSTDOCFLAGS", "x"),
            ("CARGO_BUILD_RUSTDOCFLAGS", "x"),
            ("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTDOCFLAGS", "x"),
            ("RUSTC", "C:\\other\\rustc.exe"),
            ("CARGO_BUILD_RUSTC", "x"),
            ("RUSTC_WRAPPER", "sccache"),
            ("RUSTC_WORKSPACE_WRAPPER", "x"),
            ("CARGO_BUILD_RUSTC_WRAPPER", "x"),
            ("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER", "x"),
            ("RUSTC_BOOTSTRAP", "1"),
            ("RUSTUP_TOOLCHAIN", "1.98.1"),
            ("RUSTDOC", "x"),
            ("CARGO_BUILD_RUSTDOC", "x"),
            ("CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER", "x"),
            ("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER", "x"),
        ]);
        let refused = [
            "CARGO_BUILD_RUSTC",
            "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
            "CARGO_BUILD_RUSTC_WRAPPER",
            "CARGO_BUILD_RUSTDOC",
            "CARGO_BUILD_RUSTDOCFLAGS",
            "CARGO_BUILD_RUSTFLAGS",
            "CARGO_ENCODED_RUSTDOCFLAGS",
            "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER",
            "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS",
            "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER",
            "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTDOCFLAGS",
            "RUSTC",
            "RUSTC_BOOTSTRAP",
            "RUSTC_WORKSPACE_WRAPPER",
            "RUSTC_WRAPPER",
            "RUSTDOC",
            "RUSTDOCFLAGS",
            "RUSTFLAGS",
        ];
        assert_eq!(refused_variables(environment.clone(), false), refused);
        assert_eq!(refused_variables(environment, true), refused);
        // On Windows a name is read without regard to case, and so is the
        // refusal; elsewhere `RustFlags` is another variable cargo never reads.
        let mixed = pairs(&[
            ("RustFlags", "-Cdebug-assertions=off"),
            ("rustc_wrapper", "x"),
            ("Cargo_Target_X86_64_Pc_Windows_Msvc_RustFlags", "x"),
        ]);
        assert_eq!(
            refused_variables(mixed.clone(), true),
            [
                "Cargo_Target_X86_64_Pc_Windows_Msvc_RustFlags",
                "RustFlags",
                "rustc_wrapper"
            ]
        );
        assert!(refused_variables(mixed, false).is_empty());
        assert_eq!(refuse_variables(&[]), Ok(()));
        let fault = refuse_variables(&["RUSTFLAGS".to_owned()]).expect_err("refused");
        assert!(fault.to_string().contains("RUSTFLAGS set"), "{fault}");
        // A checked build pins all of them: flags to its own, wrappers off,
        // RUSTC_BOOTSTRAP removed.
        assert_eq!(
            pinned_env("-Ctarget-feature=+crt-static"),
            [
                (
                    "CARGO_ENCODED_RUSTFLAGS".to_owned(),
                    Some("-Ctarget-feature=+crt-static".to_owned())
                ),
                ("CARGO_ENCODED_RUSTDOCFLAGS".to_owned(), Some(String::new())),
                ("RUSTC_WRAPPER".to_owned(), Some(String::new())),
                ("RUSTC_WORKSPACE_WRAPPER".to_owned(), Some(String::new())),
                ("RUSTC_BOOTSTRAP".to_owned(), None),
            ]
        );
    }
}

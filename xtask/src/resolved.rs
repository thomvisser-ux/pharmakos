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
//! xtask makes to test, to hash or to ship is checked from cargo's own answer:
//!
//! * the `test` and `test-research` steps, before each `cargo test`
//!   invocation, build the same [`TestRun`] with `--no-run
//!   --message-format=json` ([`TestRun::check`]); a doctest invocation, which
//!   cargo will not build without running, is checked through the test-profile
//!   build of the same libraries ([`doc_check`]);
//! * the `determinism` step's `release-checked` build and `cargo xtask
//!   package`'s two release builds are read from their own
//!   `--message-format=json`, as they are made.
//!
//! [`faults`] reads the profile cargo RESOLVED for every unit it built, fresh
//! units included, against an [`Expect`]: overflow checks on in every
//! workspace unit, always; debug assertions on in every workspace unit where
//! the profile promises them (the suite's and `release-checked`); and, for the
//! suite, `pharmakos-sim` optimised, every third-party crate the sim runs
//! optimised, and no crate of the godot family optimised.
//!
//! # Rustflags
//!
//! Rustflags and rustdocflags override the profile and never show in the
//! resolved JSON: `RUSTFLAGS="-C debug-assertions=off"` builds a unit the JSON
//! still reports with assertions on. So a checked build takes its flags from
//! nowhere but its own value: it carries `CARGO_ENCODED_RUSTFLAGS` (and
//! `CARGO_ENCODED_RUSTDOCFLAGS`), which cargo reads before every other source
//! (`RUSTFLAGS`, `target.*.rustflags`, `build.rustflags`, user-level config
//! files included), set to exactly what that build passes: nothing, except the
//! Windows package's `+crt-static`. And a flags variable set in the
//! environment, which the caller meant to reach the build and which this would
//! silently override, is refused by name before anything builds
//! ([`flag_variables`]).
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

/// The environment variables through which flags reach rustc or rustdoc. A
/// `CARGO_TARGET_<triple>_RUSTFLAGS` or `..._RUSTDOCFLAGS` variable is matched
/// by [`is_flag_variable`] too.
const FLAG_VARIABLES: &[&str] = &[
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
    "RUSTDOCFLAGS",
    "CARGO_ENCODED_RUSTDOCFLAGS",
    "CARGO_BUILD_RUSTDOCFLAGS",
];

/// The variable a checked build sets its rustflags through: cargo reads it
/// before every other source.
pub(crate) const ENCODED_RUSTFLAGS: &str = "CARGO_ENCODED_RUSTFLAGS";
/// The same for rustdoc, which compiles the doctests.
const ENCODED_RUSTDOCFLAGS: &str = "CARGO_ENCODED_RUSTDOCFLAGS";

/// Whether `name` is an environment variable that carries flags to rustc or
/// rustdoc.
pub(crate) fn is_flag_variable(name: &str) -> bool {
    FLAG_VARIABLES.contains(&name)
        || name
            .strip_prefix("CARGO_TARGET_")
            .is_some_and(|rest| rest.ends_with("_RUSTFLAGS") || rest.ends_with("_RUSTDOCFLAGS"))
}

/// The flags variables `environment` sets to something other than nothing,
/// sorted: each would change a checked build behind its JSON, or be silently
/// overridden by it, so each is refused by name.
pub(crate) fn flag_variables(
    environment: impl IntoIterator<Item = (String, String)>,
) -> Vec<String> {
    let mut found: Vec<String> = environment
        .into_iter()
        .filter(|(name, value)| is_flag_variable(name) && !value.is_empty())
        .map(|(name, _)| name)
        .collect();
    found.sort();
    found
}

/// The refusal for [`flag_variables`]' answer, or `Ok` when it is empty.
pub(crate) fn refuse_flag_variables(found: &[String]) -> Result<(), ProfileFault> {
    if found.is_empty() {
        Ok(())
    } else {
        Err(ProfileFault::FlagsInEnvironment {
            variables: found.to_vec(),
        })
    }
}

/// The environment every checked build carries: its rustflags exactly
/// `rustflags` (cargo's encoding: arguments separated by `\x1f`), and no
/// rustdocflags, from any source.
pub(crate) fn pinned_flags(rustflags: &str) -> Vec<(String, String)> {
    vec![
        (ENCODED_RUSTFLAGS.to_owned(), rustflags.to_owned()),
        (ENCODED_RUSTDOCFLAGS.to_owned(), String::new()),
    ]
}

/// One cargo command: its `--config` values, its environment and its
/// arguments from the subcommand on. Both the check and the run of a test
/// invocation are derived from one [`TestRun`], so they cannot differ in any
/// of the three.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CargoCall {
    pub(crate) config: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) args: Vec<String>,
}

impl CargoCall {
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

/// What a checked build requires of the sim's optimisation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SimRule {
    /// The sim optimised wherever this build builds it (a selection that
    /// reaches no sim target builds none).
    OptimisedIfBuilt,
    /// The sim built, every unit of it at opt-level 0: the invocations of
    /// [`UNOPTIMISED_TESTS`] when the sim owns the selected target.
    Unoptimised,
    /// Not judged: a release-family build, whose optimisation is the
    /// release profile's.
    Unjudged,
}

/// What [`faults`] holds a build to. Overflow checks on in every workspace
/// unit are always required.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Expect {
    /// Debug assertions on in every workspace unit.
    pub(crate) debug_assertions: bool,
    pub(crate) sim: SimRule,
    /// Every third-party crate in the sim's closure optimised.
    pub(crate) closure_optimised: bool,
    /// No crate of the godot family optimised.
    pub(crate) godot_unoptimised: bool,
}

impl Expect {
    /// A test invocation: the dev profile the suite builds with.
    pub(crate) const fn suite(sim: SimRule) -> Expect {
        Expect {
            debug_assertions: true,
            sim,
            closure_optimised: true,
            godot_unoptimised: true,
        }
    }

    /// `release-checked`, the cross-OS hash gate's build: release's
    /// optimisation and overflow checks, with debug assertions on.
    pub(crate) const RELEASE_CHECKED: Expect = Expect {
        debug_assertions: true,
        sim: SimRule::Unjudged,
        closure_optimised: false,
        godot_unoptimised: false,
    };

    /// `release` and `release-client`, what ships: overflow checks on, debug
    /// assertions off.
    pub(crate) const RELEASE: Expect = Expect {
        debug_assertions: false,
        sim: SimRule::Unjudged,
        closure_optimised: false,
        godot_unoptimised: false,
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

/// One test invocation, the single description both its check and its run
/// are derived from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TestRun {
    /// The arguments after `test` and before the selectors: the package
    /// selection and features.
    pub(crate) selection: Vec<String>,
    pub(crate) invocation: Invocation,
    pub(crate) locked: bool,
}

impl TestRun {
    /// What the check holds this invocation's build to.
    pub(crate) fn expect(&self) -> Expect {
        Expect::suite(self.invocation.sim)
    }

    /// Whether this is a doctest invocation.
    fn doc(&self) -> bool {
        self.invocation.selectors == ["--doc"]
    }

    /// The command that runs the tests.
    pub(crate) fn run(&self) -> CargoCall {
        let config = match self.invocation.sim {
            SimRule::Unoptimised => vec![SIM_UNOPTIMISED.to_owned()],
            SimRule::OptimisedIfBuilt | SimRule::Unjudged => Vec::new(),
        };
        let mut args: Vec<String> = vec!["test".to_owned()];
        args.extend(self.selection.iter().cloned());
        args.extend(self.invocation.selectors.iter().cloned());
        if self.locked {
            args.push("--locked".to_owned());
        }
        CargoCall {
            config,
            env: pinned_flags(""),
            args,
        }
    }

    /// The command that builds what [`TestRun::run`] will run, and reports
    /// it: the run itself with `--no-run --message-format=json`, or for a
    /// doctest invocation [`doc_check`] of it.
    pub(crate) fn check(&self) -> CargoCall {
        let run = self.run();
        if self.doc() {
            return doc_check(&run);
        }
        let mut check = run;
        check.args.push("--no-run".to_owned());
        check.args.push("--message-format=json".to_owned());
        check
    }
}

/// The check of a doctest run: cargo will not build doctests without running
/// them, so it builds, in the test profile and with the same `--config`,
/// environment, selection and features, the libraries the doctests link (the
/// same artefacts the doctest run then finds fresh). A dev-dependency built
/// only for doctests is not in this build; each one a package's doctests use
/// is built, and checked, for that package's unit or integration tests in
/// another invocation of the same leg. The doctest bodies are compiled by
/// rustdoc, whose flags the pinned `CARGO_ENCODED_RUSTDOCFLAGS` holds empty.
pub(crate) fn doc_check(run: &CargoCall) -> CargoCall {
    let mut args: Vec<String> = Vec::new();
    for (index, arg) in run.args.iter().enumerate() {
        match (index, arg.as_str()) {
            (0, "test") => args.push("build".to_owned()),
            (_, "--doc") => {}
            _ => args.push(arg.clone()),
        }
    }
    for extra in ["--profile", "test", "--lib", "--message-format=json"] {
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
    fn parse(text: &str) -> Option<OptLevel> {
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
    /// The executable the unit produced, when it is one.
    pub(crate) executable: Option<String>,
}

/// The packages [`faults`] judges by: the workspace's members, every
/// package's name, the sim, the third-party crates it runs and the godot
/// family.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Graph {
    members: BTreeSet<String>,
    names: BTreeMap<String, String>,
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
    #[cfg(test)]
    pub(crate) fn godot_family_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .godot_family
            .iter()
            .filter_map(|id| self.names.get(id).cloned())
            .collect();
        names.sort();
        names
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
    /// The sim at opt-level 0 in a build that runs it optimised.
    SimUnoptimised { target: String },
    /// The sim optimised in an invocation of [`UNOPTIMISED_TESTS`].
    SimOptimised { target: String, opt_level: OptLevel },
    /// An invocation of [`UNOPTIMISED_TESTS`] that built no sim unit.
    SimNotBuilt,
    /// A third-party crate the sim runs at opt-level 0.
    DependencyUnoptimised { package: String, target: String },
    /// A crate of the godot family optimised.
    GodotOptimised {
        package: String,
        target: String,
        opt_level: OptLevel,
    },
    /// Flags variables set in the environment.
    FlagsInEnvironment { variables: Vec<String> },
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
            Self::BuildFailed => f.write_str("cargo's `build-finished` says the build failed"),
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
                "the invocation of {} built no {SIM} unit, so the opt-level it exists for was \
                 never in force",
                UNOPTIMISED_TESTS.join(", ")
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
            Self::FlagsInEnvironment { variables } => write!(
                f,
                "{} set in the environment; flags change a build behind the profile cargo \
                 reports, and a checked build pins its own, so it would override them in \
                 silence. Unset {} and run it again",
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

/// The reasons cargo's `--message-format=json` writes for a build.
const REASONS: &[&str] = &[
    "compiler-artifact",
    "compiler-message",
    "build-script-executed",
    "build-finished",
];

/// Every unit cargo built, from the stdout of a `--message-format=json` run.
/// Every line must be a JSON object with a known `reason`; every
/// `compiler-artifact` must carry its package, target and resolved profile;
/// and the stream must end in exactly one `build-finished` that says the
/// build succeeded.
pub(crate) fn parse_messages(text: &str) -> Result<Vec<Built>, ProfileFault> {
    let mut built: Vec<Built> = Vec::new();
    let mut finished = false;
    let mut any = false;
    // Numbered from 1 by a range that cannot overflow: a string holds fewer
    // lines than bytes, and fewer bytes than `usize::MAX`.
    for (line, text) in (1_usize..).zip(text.lines()) {
        if text.trim().is_empty() {
            continue;
        }
        any = true;
        if finished {
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
            finished = true;
            continue;
        }
        if reason != "compiler-artifact" {
            continue;
        }
        let target = member(&message, "target").map_err(unreadable)?;
        let profile = member(&message, "profile").map_err(unreadable)?;
        let level = string_of(profile, "opt_level").map_err(unreadable)?;
        let opt_level = OptLevel::parse(level).ok_or_else(|| ProfileFault::BadOptLevel {
            line,
            value: level.to_owned(),
        })?;
        let kinds = array_of(target, "kind")
            .and_then(|kinds| {
                kinds
                    .iter()
                    .map(|kind| {
                        kind.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| "a target kind is not a string".to_owned())
                    })
                    .collect::<Result<Vec<String>, String>>()
            })
            .map_err(unreadable)?;
        let executable = match member(&message, "executable").map_err(unreadable)? {
            Json::Null => None,
            Json::Str(path) => Some(path.clone()),
            _ => {
                return Err(unreadable(
                    "`executable` is neither null nor a string".to_owned(),
                ));
            }
        };
        built.push(Built {
            package_id: string_of(&message, "package_id")
                .map_err(unreadable)?
                .to_owned(),
            target: string_of(target, "name").map_err(unreadable)?.to_owned(),
            kinds,
            opt_level,
            debug_assertions: bool_of(profile, "debug_assertions").map_err(unreadable)?,
            overflow_checks: bool_of(profile, "overflow_checks").map_err(unreadable)?,
            executable,
        });
    }
    if !any {
        return Err(ProfileFault::NothingBuilt);
    }
    if !finished {
        return Err(ProfileFault::Unfinished);
    }
    Ok(built)
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
    let mut sim_built = false;
    for unit in built {
        let Some(package) = graph.names.get(&unit.package_id) else {
            faults.push(ProfileFault::UnknownPackage {
                package_id: unit.package_id.clone(),
            });
            continue;
        };
        let target = unit.target.clone();
        if graph.members.contains(&unit.package_id) {
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
        }
        if graph.sim.as_deref() == Some(unit.package_id.as_str()) {
            sim_built = true;
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
        } else if expect.closure_optimised
            && graph.sim_closure.contains(&unit.package_id)
            && !graph.members.contains(&unit.package_id)
            && !unit.opt_level.optimised()
        {
            faults.push(ProfileFault::DependencyUnoptimised {
                package: package.clone(),
                target,
            });
        } else if expect.godot_unoptimised
            && graph.godot_family.contains(&unit.package_id)
            && unit.opt_level.optimised()
        {
            faults.push(ProfileFault::GodotOptimised {
                package: package.clone(),
                target,
                opt_level: unit.opt_level,
            });
        }
    }
    if expect.sim == SimRule::Unoptimised && !sim_built {
        faults.push(ProfileFault::SimNotBuilt);
    }
    faults
}

#[cfg(test)]
mod tests {
    use super::*;

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
            format!(
                r#"{{"id":"{id}","name":"{name}","targets":[{{"kind":["{kind}"],"name":"x"}}]}}"#
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
        let built = parse_messages(&finished(lines)).expect("the messages read");
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
        assert_eq!(judged(&checked, Expect::RELEASE_CHECKED), Vec::new());
        let unasserted = vec![artifact(SIM_ID, "determinism", "3", false, true)];
        assert_eq!(
            judged(&unasserted, Expect::RELEASE_CHECKED),
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
        // A proc-macro runs in the compiler, and what only it reaches with it;
        // a crate the sim does not run may stay at 0; so may a workspace
        // member the sim runs (only the sim is optimised among the members).
        for (id, name) in [
            (DERIVE_ID, "serde_derive"),
            (SYN_ID, "syn"),
            (LEXOPT_ID, "lexopt"),
            (PROTO_ID, "pharmakos_proto"),
        ] {
            let mut lines = good();
            lines.push(artifact(id, name, "0", true, true));
            assert_eq!(judged(&lines, suite()), Vec::new(), "{name}");
        }
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

    #[test]
    fn the_messages_are_read_strictly() {
        let fails = |text: &str| parse_messages(text).expect_err(text);
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
            assert!(parse_messages(&finished(&[line])).is_ok(), "{good}");
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
        assert_eq!(
            parse_messages(&finished(&good())).map(|built| built.len()),
            Ok(13)
        );
    }

    /// The reader never passes on nothing: the stream ends in exactly one
    /// `build-finished` that says the build succeeded.
    #[test]
    fn the_reader_never_passes_on_nothing() {
        let fails = |text: &str| parse_messages(text).expect_err(text);
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
        assert_eq!(parse_messages(FINISHED), Ok(Vec::new()));
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
            assert_eq!(check, doc_check(&run), "{test:?}");
        } else {
            let mut expected = run.clone();
            expected.args.push("--no-run".to_owned());
            expected.args.push("--message-format=json".to_owned());
            assert_eq!(check, expected, "{test:?}");
        }
        assert_eq!(check.env, pinned_flags(""), "{test:?}");
        assert_eq!(check.config, run.config, "{test:?}");
        let unoptimised = test.invocation.sim == SimRule::Unoptimised;
        assert_eq!(
            run.config == [SIM_UNOPTIMISED],
            unoptimised,
            "the run carries the sim's level: {test:?}"
        );
    }

    #[test]
    fn the_check_and_the_run_are_one_value() {
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
                            assert_check_is_the_run(&TestRun {
                                selection: selection.clone(),
                                invocation: invocation.clone(),
                                locked,
                            });
                        }
                    }
                }
            }
        }
        // The doctest route, spelled out once.
        let doc = TestRun {
            selection: vec!["--workspace".to_owned()],
            invocation: Invocation {
                selectors: vec!["--doc".to_owned()],
                sim: SimRule::OptimisedIfBuilt,
            },
            locked: true,
        };
        assert_eq!(
            doc.check().command_line(),
            [
                "build",
                "--workspace",
                "--locked",
                "--profile",
                "test",
                "--lib",
                "--message-format=json"
            ]
        );
        let alloc = TestRun {
            selection: vec!["--workspace".to_owned()],
            invocation: Invocation {
                selectors: ["--test", "allocations"].map(str::to_owned).to_vec(),
                sim: SimRule::Unoptimised,
            },
            locked: false,
        };
        assert_eq!(
            alloc.run().command_line(),
            [
                "--config",
                "profile.dev.package.pharmakos-sim.opt-level=0",
                "test",
                "--workspace",
                "--test",
                "allocations"
            ]
        );
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

    #[test]
    fn a_flags_variable_in_the_environment_is_refused_by_name() {
        let pairs = |list: &[(&str, &str)]| {
            list.iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            flag_variables(pairs(&[
                ("RUSTFLAGS", "-C debug-assertions=off"),
                ("PATH", "C:\\x"),
                ("CARGO_ENCODED_RUSTFLAGS", ""),
                ("RUSTDOCFLAGS", "-Copt-level=0"),
                (
                    "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS",
                    "-Ctarget-feature=+crt-static"
                ),
                ("CARGO_TARGET_DIR", "D:/build"),
                ("CARGO_BUILD_RUSTFLAGS", "x"),
                ("CARGO_ENCODED_RUSTDOCFLAGS", "x"),
                ("CARGO_BUILD_RUSTDOCFLAGS", "x"),
                ("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTDOCFLAGS", "x"),
            ])),
            [
                "CARGO_BUILD_RUSTDOCFLAGS",
                "CARGO_BUILD_RUSTFLAGS",
                "CARGO_ENCODED_RUSTDOCFLAGS",
                "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS",
                "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTDOCFLAGS",
                "RUSTDOCFLAGS",
                "RUSTFLAGS",
            ]
        );
        assert_eq!(refuse_flag_variables(&[]), Ok(()));
        let refused = refuse_flag_variables(&["RUSTFLAGS".to_owned()]).expect_err("refused");
        assert!(refused.to_string().contains("RUSTFLAGS set"), "{refused}");
        assert_eq!(
            pinned_flags(""),
            [
                ("CARGO_ENCODED_RUSTFLAGS".to_owned(), String::new()),
                ("CARGO_ENCODED_RUSTDOCFLAGS".to_owned(), String::new())
            ]
        );
    }
}

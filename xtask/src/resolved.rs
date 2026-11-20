// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What cargo will build for a test invocation, read from cargo itself.
//!
//! The suite runs the deterministic sim optimised, with debug assertions and
//! overflow checks on: at opt-level 0 its integer code ran 25 to 50 times
//! slower, which held CI's shards at 35 to 54 of 90 minutes. A reading of the
//! TOML cannot hold that in place: cargo also takes dotted keys, inline tables,
//! `--config` on the command line and config files outside the repository, and
//! a reader of the text would have to be cargo to see them all. So before each
//! `cargo test` invocation the `test` and `test-research` steps run the same
//! command with `--no-run --message-format=json` (a doctest invocation, which
//! cargo will not build without running, is checked through the test-profile
//! build of the same libraries), and [`faults`] reads the profile cargo
//! RESOLVED for every unit it built, fresh units included:
//!
//! * every workspace unit keeps debug assertions and overflow checks on;
//! * `pharmakos-sim` is optimised, except in the invocations of
//!   [`UNOPTIMISED_TESTS`], where it must be at opt-level 0 (below);
//! * every third-party package the sim runs (its normal-dependency closure in
//!   `cargo metadata`'s resolve graph) is optimised;
//! * no `godot` crate is optimised: they serve the walled client alone, and
//!   optimising them doubled a cold client build for no test's sake.
//!
//! # The allocation test runs the sim unoptimised
//!
//! `crates/sim/tests/allocations.rs`'s `a_tick_allocates_nothing` counts the
//! allocations a tick makes. The optimiser deletes an allocation whose value
//! is never read (a `Vec::with_capacity` dropped at once, a `Box` freed where
//! it was made), so at opt-level 2 a pull request that adds such code to a tick
//! phase passes the test, and its claim, that such a pull request fails there,
//! no longer holds. The allocation test therefore runs as an invocation of its
//! own with the sim at opt-level 0, through cargo's `--config` ([`config_for`]),
//! in whichever shard its unit falls to; the unit list, and so the split, is
//! the same as before.

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

/// The `--config` value that builds [`SIM`] at opt-level 0.
const SIM_UNOPTIMISED: &str = "profile.dev.package.pharmakos-sim.opt-level=0";

/// What an invocation requires of the sim's optimisation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SimLevel {
    /// Optimised: every invocation but [`UNOPTIMISED_TESTS`]'.
    Optimised,
    /// At opt-level 0: the invocations of [`UNOPTIMISED_TESTS`].
    Unoptimised,
}

/// One `cargo test` invocation of a step: its target selectors and what it
/// requires of the sim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub(crate) selectors: Vec<String>,
    pub(crate) sim: SimLevel,
}

/// The cargo `--config` arguments an invocation takes, placed before the
/// subcommand.
pub(crate) fn config_for(sim: SimLevel) -> Vec<String> {
    match sim {
        SimLevel::Optimised => Vec::new(),
        SimLevel::Unoptimised => vec!["--config".to_owned(), SIM_UNOPTIMISED.to_owned()],
    }
}

/// The invocations that run `units`: [`crate::shard::selectors`]' lists over
/// every unit but [`UNOPTIMISED_TESTS`], then one invocation per unoptimised
/// test the units hold. Every unit is in exactly one invocation.
pub(crate) fn invocations(units: &[Unit]) -> Vec<Invocation> {
    let unoptimised = |unit: &Unit| matches!(unit, Unit::Test(name) if UNOPTIMISED_TESTS.contains(&name.as_str()));
    let optimised: Vec<Unit> = units
        .iter()
        .filter(|unit| !unoptimised(unit))
        .cloned()
        .collect();
    let mut out: Vec<Invocation> = crate::shard::selectors(&optimised)
        .into_iter()
        .map(|selectors| Invocation {
            selectors,
            sim: SimLevel::Optimised,
        })
        .collect();
    for unit in units.iter().filter(|unit| unoptimised(unit)) {
        out.extend(
            crate::shard::selectors(std::slice::from_ref(unit))
                .into_iter()
                .map(|selectors| Invocation {
                    selectors,
                    sim: SimLevel::Unoptimised,
                }),
        );
    }
    out
}

/// One unit cargo built, with the profile it resolved for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Built {
    pub(crate) package_id: String,
    pub(crate) target: String,
    pub(crate) opt_level: String,
    pub(crate) debug_assertions: bool,
    pub(crate) overflow_checks: bool,
}

/// The packages [`faults`] judges by: the workspace's members, every
/// package's name, and the sim's normal-dependency closure.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Graph {
    members: BTreeSet<String>,
    names: BTreeMap<String, String>,
    sim: Option<String>,
    sim_closure: BTreeSet<String>,
}

/// A check that what cargo will build is what the suite runs, failed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ProfileFault {
    /// A line of cargo's JSON messages that does not read as one.
    Unreadable { line: usize, why: String },
    /// A message whose `reason` this does not know: read strictly, never
    /// skipped.
    UnknownReason { line: usize, reason: String },
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
    /// The sim at opt-level 0 in an invocation that runs it optimised.
    SimUnoptimised { target: String },
    /// The sim optimised in an invocation of [`UNOPTIMISED_TESTS`].
    SimOptimised { target: String, opt_level: String },
    /// A package the sim runs at opt-level 0.
    DependencyUnoptimised { package: String, target: String },
    /// A `godot` crate optimised.
    GodotOptimised {
        package: String,
        target: String,
        opt_level: String,
    },
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
                "{package}'s target `{target}` builds with {check} off; every workspace unit \
                 keeps them on, so optimising the suite never takes a check with it"
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
                "{package}'s target `{target}` builds at opt-level {opt_level}; the godot \
                 crates serve the walled client alone and stay at 0 (Cargo.toml names each)"
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

/// The reasons cargo's `--message-format=json` writes for a build.
const REASONS: &[&str] = &[
    "compiler-artifact",
    "compiler-message",
    "build-script-executed",
    "build-finished",
];

/// Every unit cargo built, from the stdout of a `--message-format=json` run.
/// Every line must be a JSON object with a known `reason`, and every
/// `compiler-artifact` must carry its package, target and resolved profile.
pub(crate) fn parse_messages(text: &str) -> Result<Vec<Built>, ProfileFault> {
    let mut built: Vec<Built> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line_number = index.saturating_add(1);
        let unreadable = |why: String| ProfileFault::Unreadable {
            line: line_number,
            why,
        };
        if line.trim().is_empty() {
            continue;
        }
        let message = Json::parse(line).map_err(unreadable)?;
        let reason = string_of(&message, "reason").map_err(unreadable)?;
        if !REASONS.contains(&reason) {
            return Err(ProfileFault::UnknownReason {
                line: line_number,
                reason: reason.to_owned(),
            });
        }
        if reason != "compiler-artifact" {
            continue;
        }
        let read = || -> Result<Built, String> {
            let target = member(&message, "target")?;
            let profile = member(&message, "profile")?;
            Ok(Built {
                package_id: string_of(&message, "package_id")?.to_owned(),
                target: string_of(target, "name")?.to_owned(),
                opt_level: string_of(profile, "opt_level")?.to_owned(),
                debug_assertions: bool_of(profile, "debug_assertions")?,
                overflow_checks: bool_of(profile, "overflow_checks")?,
            })
        };
        built.push(read().map_err(unreadable)?);
    }
    Ok(built)
}

/// The graph [`faults`] needs, from `cargo metadata --format-version 1`
/// (with its dependencies, so with `resolve`).
pub(crate) fn graph(metadata: &str) -> Result<Graph, ProfileFault> {
    let fault = ProfileFault::Metadata;
    let json = Json::parse(metadata).map_err(fault)?;
    let mut graph = Graph::default();
    let packages = member(&json, "packages")
        .and_then(|value| {
            value
                .as_array()
                .ok_or_else(|| "`packages` is not an array".to_owned())
        })
        .map_err(fault)?;
    for package in packages {
        let id = string_of(package, "id").map_err(fault)?;
        let name = string_of(package, "name").map_err(fault)?;
        if name == SIM {
            graph.sim = Some(id.to_owned());
        }
        graph.names.insert(id.to_owned(), name.to_owned());
    }
    let members = member(&json, "workspace_members")
        .and_then(|value| {
            value
                .as_array()
                .ok_or_else(|| "`workspace_members` is not an array".to_owned())
        })
        .map_err(fault)?;
    for id in members {
        let id = id
            .as_str()
            .ok_or_else(|| "a workspace member is not a string".to_owned())
            .map_err(fault)?;
        graph.members.insert(id.to_owned());
    }
    let nodes = member(&json, "resolve")
        .and_then(|resolve| member(resolve, "nodes"))
        .and_then(|value| {
            value
                .as_array()
                .ok_or_else(|| "`nodes` is not an array".to_owned())
        })
        .map_err(fault)?;
    // Each package's normal dependencies: a `dep_kinds` entry whose `kind` is
    // null, whatever its platform.
    let mut normal: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for node in nodes {
        let id = string_of(node, "id").map_err(fault)?;
        let deps = member(node, "deps")
            .and_then(|value| {
                value
                    .as_array()
                    .ok_or_else(|| "`deps` is not an array".to_owned())
            })
            .map_err(fault)?;
        let mut out: Vec<String> = Vec::new();
        for dep in deps {
            let pkg = string_of(dep, "pkg").map_err(fault)?;
            let kinds = member(dep, "dep_kinds")
                .and_then(|value| {
                    value
                        .as_array()
                        .ok_or_else(|| "`dep_kinds` is not an array".to_owned())
                })
                .map_err(fault)?;
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
            if is_normal {
                out.push(pkg.to_owned());
            }
        }
        normal.insert(id.to_owned(), out);
    }
    let Some(sim) = graph.sim.clone() else {
        return Err(ProfileFault::Metadata(format!(
            "no package is called {SIM}"
        )));
    };
    let mut queue: Vec<String> = normal.get(&sim).cloned().unwrap_or_default();
    while let Some(id) = queue.pop() {
        if graph.sim_closure.insert(id.clone())
            && let Some(next) = normal.get(&id)
        {
            queue.extend(next.iter().cloned());
        }
    }
    Ok(graph)
}

/// Whether `name` is one of the `godot` crates.
fn is_godot(name: &str) -> bool {
    name == "godot" || name.starts_with("godot-")
}

/// Every way `built` breaks the profile rule (the module docs), for an
/// invocation that requires `sim` of the sim. Empty when it keeps it.
pub(crate) fn faults(built: &[Built], graph: &Graph, sim: SimLevel) -> Vec<ProfileFault> {
    let mut faults: Vec<ProfileFault> = Vec::new();
    for unit in built {
        let Some(package) = graph.names.get(&unit.package_id) else {
            faults.push(ProfileFault::UnknownPackage {
                package_id: unit.package_id.clone(),
            });
            continue;
        };
        let target = unit.target.clone();
        let unoptimised = unit.opt_level == "0";
        if graph.members.contains(&unit.package_id) {
            for (on, check) in [
                (unit.debug_assertions, "debug assertions"),
                (unit.overflow_checks, "overflow checks"),
            ] {
                if !on {
                    faults.push(ProfileFault::CheckOff {
                        package: package.clone(),
                        target: target.clone(),
                        check,
                    });
                }
            }
        }
        if graph.sim.as_deref() == Some(unit.package_id.as_str()) {
            match (sim, unoptimised) {
                (SimLevel::Optimised, true) => {
                    faults.push(ProfileFault::SimUnoptimised { target });
                }
                (SimLevel::Unoptimised, false) => faults.push(ProfileFault::SimOptimised {
                    target,
                    opt_level: unit.opt_level.clone(),
                }),
                _ => {}
            }
        } else if graph.sim_closure.contains(&unit.package_id)
            && !graph.members.contains(&unit.package_id)
            && unoptimised
        {
            faults.push(ProfileFault::DependencyUnoptimised {
                package: package.clone(),
                target,
            });
        } else if is_godot(package) && !unoptimised {
            faults.push(ProfileFault::GodotOptimised {
                package: package.clone(),
                target,
                opt_level: unit.opt_level.clone(),
            });
        }
    }
    faults
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIM_ID: &str = "path+file:///w/crates/sim#pharmakos-sim@0.0.0";
    const PROTO_ID: &str = "path+file:///w/crates/proto#pharmakos-proto@0.0.0";
    const GATEWAY_ID: &str = "path+file:///w/crates/gateway#pharmakos-gateway@0.0.0";
    const XXH_ID: &str = "registry+https://github.com/rust-lang/crates.io-index#xxhash-rust@0.8.18";
    const LEXOPT_ID: &str = "registry+https://github.com/rust-lang/crates.io-index#lexopt@0.3.2";
    const GODOT_ID: &str = "registry+https://github.com/rust-lang/crates.io-index#godot-core@0.5.5";

    /// A workspace of three members: the sim (which runs proto and xxhash),
    /// the gateway (which runs lexopt, outside the sim's closure, and builds
    /// xxhash for its tests alone) and a godot crate.
    fn metadata() -> String {
        let package = |id: &str, name: &str| format!(r#"{{"id":"{id}","name":"{name}"}}"#);
        let dep = |pkg: &str, kind: &str| {
            format!(r#"{{"name":"x","pkg":"{pkg}","dep_kinds":[{{"kind":{kind},"target":null}}]}}"#)
        };
        format!(
            r#"{{"packages":[{},{},{},{},{},{}],
                "workspace_members":["{SIM_ID}","{PROTO_ID}","{GATEWAY_ID}"],
                "resolve":{{"nodes":[
                  {{"id":"{SIM_ID}","deps":[{},{}]}},
                  {{"id":"{PROTO_ID}","deps":[]}},
                  {{"id":"{GATEWAY_ID}","deps":[{},{},{}]}},
                  {{"id":"{XXH_ID}","deps":[]}},
                  {{"id":"{LEXOPT_ID}","deps":[]}},
                  {{"id":"{GODOT_ID}","deps":[]}}
                ],"root":null}}}}"#,
            package(SIM_ID, "pharmakos-sim"),
            package(PROTO_ID, "pharmakos-proto"),
            package(GATEWAY_ID, "pharmakos-gateway"),
            package(XXH_ID, "xxhash-rust"),
            package(LEXOPT_ID, "lexopt"),
            package(GODOT_ID, "godot-core"),
            dep(PROTO_ID, "null"),
            dep(XXH_ID, "null"),
            dep(SIM_ID, "null"),
            dep(LEXOPT_ID, "null"),
            dep(XXH_ID, "\"dev\""),
        )
    }

    /// One `compiler-artifact` line as cargo 1.98.1 writes it, trimmed to the
    /// members the check reads (and a few it does not).
    fn artifact(id: &str, target: &str, opt: &str, assertions: bool, overflow: bool) -> String {
        format!(
            r#"{{"reason":"compiler-artifact","package_id":"{id}","manifest_path":"C:\\w\\Cargo.toml","target":{{"kind":["lib"],"crate_types":["lib"],"name":"{target}","src_path":"C:\\w\\src\\lib.rs","edition":"2024","doc":true,"doctest":true,"test":true}},"profile":{{"opt_level":"{opt}","debuginfo":2,"debug_assertions":{assertions},"overflow_checks":{overflow},"test":false}},"features":[],"filenames":[],"executable":null,"fresh":true}}"#
        )
    }

    /// The build this lane commits: the sim and its third-party crates
    /// optimised, everything else at 0, every check on.
    fn good() -> Vec<String> {
        vec![
            artifact(XXH_ID, "xxhash_rust", "2", true, true),
            artifact(PROTO_ID, "pharmakos_proto", "0", true, true),
            artifact(SIM_ID, "pharmakos_sim", "2", true, true),
            artifact(SIM_ID, "determinism", "2", true, true),
            artifact(LEXOPT_ID, "lexopt", "2", true, true),
            artifact(GATEWAY_ID, "pharmakos_gateway", "0", true, true),
            artifact(GODOT_ID, "godot_core", "0", true, true),
            r#"{"reason":"build-finished","success":true}"#.to_owned(),
        ]
    }

    fn judged(lines: &[String], sim: SimLevel) -> Vec<ProfileFault> {
        let graph = graph(&metadata()).expect("the graph reads");
        let built = parse_messages(&lines.join("\n")).expect("the messages read");
        faults(&built, &graph, sim)
    }

    #[test]
    fn the_sims_closure_is_its_normal_dependencies_alone() {
        let graph = graph(&metadata()).expect("the graph reads");
        assert_eq!(
            graph.sim_closure,
            BTreeSet::from([PROTO_ID.to_owned(), XXH_ID.to_owned()])
        );
        assert_eq!(graph.sim.as_deref(), Some(SIM_ID));
    }

    #[test]
    fn the_committed_shape_passes() {
        assert_eq!(judged(&good(), SimLevel::Optimised), Vec::new());
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
                judged(&lines, SimLevel::Optimised),
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
        assert_eq!(judged(&lines, SimLevel::Optimised), Vec::new());
    }

    /// Whatever spelling put it there (`[profile.test]` with a dotted
    /// `package.pharmakos-sim.opt-level = 0`, a `--config`, a config file
    /// outside the repository), the sim at 0 is what cargo reports.
    #[test]
    fn the_sim_at_opt_level_0_is_refused_unless_the_invocation_asks_for_it() {
        let mut lines = good();
        lines.push(artifact(SIM_ID, "allocations", "0", true, true));
        assert_eq!(
            judged(&lines, SimLevel::Optimised),
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
        assert_eq!(judged(&lines, SimLevel::Unoptimised), Vec::new());
        let lines = vec![artifact(SIM_ID, "pharmakos_sim", "1", true, true)];
        assert_eq!(
            judged(&lines, SimLevel::Unoptimised),
            [ProfileFault::SimOptimised {
                target: "pharmakos_sim".to_owned(),
                opt_level: "1".to_owned()
            }]
        );
    }

    #[test]
    fn a_dependency_the_sim_runs_at_opt_level_0_is_refused() {
        let mut lines = good();
        lines.push(artifact(XXH_ID, "xxhash_rust", "0", true, true));
        assert_eq!(
            judged(&lines, SimLevel::Optimised),
            [ProfileFault::DependencyUnoptimised {
                package: "xxhash-rust".to_owned(),
                target: "xxhash_rust".to_owned()
            }]
        );
        // A crate the sim does not run may stay at 0, and so may a workspace
        // member the sim runs (only the sim is optimised among the members).
        let mut lines = good();
        lines.push(artifact(LEXOPT_ID, "lexopt", "0", true, true));
        assert_eq!(judged(&lines, SimLevel::Optimised), Vec::new());
    }

    #[test]
    fn an_optimised_godot_crate_is_refused() {
        let mut lines = good();
        lines.push(artifact(GODOT_ID, "godot_core", "2", true, true));
        assert_eq!(
            judged(&lines, SimLevel::Optimised),
            [ProfileFault::GodotOptimised {
                package: "godot-core".to_owned(),
                target: "godot_core".to_owned(),
                opt_level: "2".to_owned()
            }]
        );
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
            judged(&lines, SimLevel::Optimised),
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
            fails("{\"reason\":\"build-finished\",\"success\":true}\n{\"no_reason\":1}"),
            ProfileFault::Unreadable { line: 2, .. }
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
            fails(&stripped),
            ProfileFault::Unreadable { line: 1, .. }
        ));
        let stringly = artifact(SIM_ID, "pharmakos_sim", "2", true, true)
            .replace("\"debug_assertions\":true", "\"debug_assertions\":\"true\"");
        assert!(matches!(
            fails(&stringly),
            ProfileFault::Unreadable { line: 1, .. }
        ));
        assert_eq!(
            parse_messages(&good().join("\n")).map(|built| built.len()),
            Ok(7)
        );
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
    }

    #[test]
    fn the_allocation_test_runs_alone_and_every_unit_runs_once() {
        let units = vec![
            Unit::Lib,
            Unit::Test("allocations".to_owned()),
            Unit::Test("determinism".to_owned()),
            Unit::Doc,
        ];
        let runs = invocations(&units);
        assert_eq!(
            runs,
            [
                Invocation {
                    selectors: ["--lib", "--test", "determinism"]
                        .map(str::to_owned)
                        .to_vec(),
                    sim: SimLevel::Optimised
                },
                Invocation {
                    selectors: vec!["--doc".to_owned()],
                    sim: SimLevel::Optimised
                },
                Invocation {
                    selectors: ["--test", "allocations"].map(str::to_owned).to_vec(),
                    sim: SimLevel::Unoptimised
                },
            ]
        );
        assert_eq!(
            config_for(SimLevel::Unoptimised),
            ["--config", "profile.dev.package.pharmakos-sim.opt-level=0"]
        );
        assert!(config_for(SimLevel::Optimised).is_empty());
        // A part without the allocation test runs as the shards always ran it.
        let part = vec![Unit::Lib, Unit::Doc];
        assert_eq!(
            invocations(&part)
                .into_iter()
                .map(|run| run.selectors)
                .collect::<Vec<_>>(),
            crate::shard::selectors(&part)
        );
        assert!(invocations(&[]).is_empty());
    }
}

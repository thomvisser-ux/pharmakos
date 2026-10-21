// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask ci --shard N/M`: one of M parallel parts of the suite.
//!
//! CI runs each operating system's suite as M jobs (decisions-log item 148 (2),
//! the owner's ruling of 2026-10-20: the test step sharded into two or three
//! parallel jobs per OS, so that a leg comes back inside item 119 (5)'s 75
//! minutes). `cargo xtask ci` stays the one entry point (AGENTS.md section 9):
//! without `--shard` it runs every step, as it always has, and with it:
//!
//! * the three slow steps, [`SHARDED_STEPS`] (`test`, `test-research`,
//!   `scenario`), run on every shard, each over its own part of its units;
//! * `golden`, [`MERGED_STEPS`], runs on no shard: it compares every fresh
//!   output with its golden, and the outputs are spread over the shards. CI's
//!   `cargo xtask ci (<os>)` job merges its shards' outputs and runs
//!   `cargo xtask golden` over them;
//! * every other step, all of them cheap, runs on shard 1 alone.
//!
//! # Units, and which shard holds one
//!
//! A unit is what one cargo or `gamectl` invocation can select, named by a
//! key that holds no timing:
//!
//! * `test`: every test target `cargo test --workspace` runs, by kind and
//!   name — `lib` (every library's unit tests: cargo cannot select one
//!   package's library without leaving `--workspace`, which would change the
//!   feature resolution the tests are built with), `bin:<name>`,
//!   `test:<name>` (every package's integration test of that name) and `doc`
//!   (every doctest). They run as `cargo test --workspace` plus the
//!   selectors, so the build is the unsharded one: `--workspace` with target
//!   selectors resolves the same features, and the artefacts are the same;
//! * `test-research`: the same, over `pharmakos-sim`'s targets alone, with the
//!   `research` feature;
//! * `scenario`: each played scenario file, by its relative path.
//!
//! A unit falls to shard `fnv1a64("<step>/<key>") % M + 1` ([`Shard::holds`]).
//! The hash is a function of the name alone, so the split is the same on every
//! machine and every run, a new test moves no other test, and every unit falls
//! to exactly one shard; `every_unit_falls_to_exactly_one_shard` and main.rs's
//! `the_shards_of_the_real_workspace_are_the_unsharded_set` hold that. The
//! split is not balanced by measured times, on purpose: a timing table would be
//! one more thing to keep current, and a stale one would still be balanced
//! wrongly. The pull request that introduced this states what the split cost
//! per shard on each operating system.
//!
//! # What is refused
//!
//! The unit list mirrors cargo's own default selection for `cargo test`
//! (libraries, binaries and integration tests whose `test` flag is set, and
//! doctests), read from `cargo metadata`. A target this does not know how to
//! select the same way — an example, which `cargo test` always builds, a bench
//! whose `test` flag is set, or a kind it has never seen — is a typed error,
//! [`ShardError::UnsupportedTarget`], rather than a target the shards would
//! silently stop building.

use std::fmt;
use std::num::{NonZeroU32, NonZeroU64};

/// The steps that run on every shard, each over its own part of its units.
pub(crate) const SHARDED_STEPS: &[&str] = &["test", "test-research", "scenario"];

/// The steps that run on no shard, because they read every shard's outputs.
/// CI runs them after merging the shards (`cargo xtask golden`).
pub(crate) const MERGED_STEPS: &[&str] = &["golden"];

/// One of M shards: `index` is 1-based and never above `total`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shard {
    index: NonZeroU32,
    total: NonZeroU32,
}

/// Why a `--shard` value or the workspace's targets cannot be sharded.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ShardError {
    /// The value is not `N/M`.
    Shape(String),
    /// A side of `N/M` is not a decimal number that fits in 32 bits.
    NotANumber(String),
    /// A side of `N/M` is zero: shards are counted from 1.
    Zero(String),
    /// `N` is above `M`.
    OutOfRange {
        index: NonZeroU32,
        total: NonZeroU32,
    },
    /// `cargo metadata` left out a target's `test` or `doctest` flag.
    MissingFlag {
        package: String,
        target: String,
        flag: &'static str,
    },
    /// A target `cargo test --workspace` would build that no shard selects.
    UnsupportedTarget {
        package: String,
        target: String,
        kinds: Vec<String>,
    },
}

impl fmt::Display for ShardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shape(value) => write!(
                f,
                "--shard takes `N/M`, shard N of M, counted from 1 (for example `2/3`); `{value}` \
                 is not that shape"
            ),
            Self::NotANumber(value) => write!(
                f,
                "--shard `{value}`: each side of `N/M` is a decimal number of at most 32 bits"
            ),
            Self::Zero(value) => write!(
                f,
                "--shard `{value}`: shards are counted from 1, and there is at least one"
            ),
            Self::OutOfRange { index, total } => write!(
                f,
                "--shard {index}/{total}: there is no shard {index} of {total}"
            ),
            Self::MissingFlag {
                package,
                target,
                flag,
            } => write!(
                f,
                "cargo metadata gives {package}'s target `{target}` no `{flag}` flag, so whether \
                 `cargo test` runs it is unknown; the split does not guess"
            ),
            Self::UnsupportedTarget {
                package,
                target,
                kinds,
            } => write!(
                f,
                "`cargo test --workspace` builds {package}'s target `{target}` (kind {}), and \
                 no shard selects a target of that kind, so the shards would stop testing it \
                 in silence; teach xtask/src/shard.rs's `units` to select it",
                kinds.join(", ")
            ),
        }
    }
}

impl Shard {
    /// Reads `N/M`.
    pub(crate) fn parse(value: &str) -> Result<Shard, ShardError> {
        let Some((index, total)) = value.split_once('/') else {
            return Err(ShardError::Shape(value.to_owned()));
        };
        let number = |side: &str| -> Result<NonZeroU32, ShardError> {
            if side.is_empty() || !side.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(ShardError::NotANumber(value.to_owned()));
            }
            let parsed: u32 = side
                .parse()
                .map_err(|_| ShardError::NotANumber(value.to_owned()))?;
            NonZeroU32::new(parsed).ok_or_else(|| ShardError::Zero(value.to_owned()))
        };
        let index = number(index)?;
        let total = number(total)?;
        if index > total {
            return Err(ShardError::OutOfRange { index, total });
        }
        Ok(Shard { index, total })
    }

    /// Shard 1, which also runs every step that is not sharded.
    pub(crate) fn is_first(self) -> bool {
        self.index.get() == 1
    }

    /// Whether the unit `key` of `step` falls to this shard.
    pub(crate) fn holds(self, step: &str, key: &str) -> bool {
        let hash = fnv1a64(format!("{step}/{key}").as_bytes());
        let total = NonZeroU64::from(self.total);
        // `index` is 1-based and at most `total`, so `index - 1` cannot wrap.
        hash % total == u64::from(self.index.get().wrapping_sub(1))
    }
}

impl fmt::Display for Shard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.index, self.total)
    }
}

/// 64-bit FNV-1a: small, std-free of `RandomState`, and the same on every
/// platform, which is all a split needs.
pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// A target of `cargo metadata`, as far as the split needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) name: String,
    pub(crate) kinds: Vec<String>,
    /// `test`: whether `cargo test` runs it by default. `None` when the
    /// metadata left the flag out, which [`units`] refuses rather than guess.
    pub(crate) test: Option<bool>,
    /// `doctest`: whether `cargo test` runs its doctests by default; `None`
    /// as for `test`.
    pub(crate) doctest: Option<bool>,
}

/// One cargo selection of test targets.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Unit {
    /// `--lib`: every selected package's library unit tests.
    Lib,
    /// `--bin <name>`.
    Bin(String),
    /// `--test <name>`: every selected package's integration test of that name.
    Test(String),
    /// `--doc`: every doctest. Cargo refuses to mix it with another selector,
    /// so it runs as an invocation of its own.
    Doc,
}

impl Unit {
    /// The unit's name in [`Shard::holds`] and in the step's report.
    pub(crate) fn key(&self) -> String {
        match self {
            Self::Lib => "lib".to_owned(),
            Self::Bin(name) => format!("bin:{name}"),
            Self::Test(name) => format!("test:{name}"),
            Self::Doc => "doc".to_owned(),
        }
    }
}

/// The library kinds: each has unit tests (and doctests) in `cargo test`.
const LIB_KINDS: &[&str] = &["lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"];

/// Every unit `cargo test` runs over these packages' targets, sorted and
/// deduplicated, or the first target it would build that no unit selects.
pub(crate) fn units<'a>(
    targets: impl IntoIterator<Item = (&'a str, &'a Target)>,
) -> Result<Vec<Unit>, ShardError> {
    let mut found: Vec<Unit> = Vec::new();
    for (package, target) in targets {
        let has = |kind: &str| target.kinds.iter().any(|owned| owned == kind);
        let refuse = || ShardError::UnsupportedTarget {
            package: package.to_owned(),
            target: target.name.clone(),
            kinds: target.kinds.clone(),
        };
        let flag = |value: Option<bool>, flag: &'static str| {
            value.ok_or_else(|| ShardError::MissingFlag {
                package: package.to_owned(),
                target: target.name.clone(),
                flag,
            })
        };
        if has("custom-build") {
            // A build script: cargo test runs it as part of the build, which
            // every shard does for whatever it selects.
            continue;
        }
        if LIB_KINDS.iter().any(|kind| has(kind)) {
            if flag(target.test, "test")? {
                found.push(Unit::Lib);
            }
            if flag(target.doctest, "doctest")? {
                found.push(Unit::Doc);
            }
        } else if has("bin") {
            if flag(target.test, "test")? {
                found.push(Unit::Bin(target.name.clone()));
            }
        } else if has("test") {
            if flag(target.test, "test")? {
                found.push(Unit::Test(target.name.clone()));
            }
        } else if has("bench") {
            // `cargo test` builds a bench only when its `test` flag is set.
            if flag(target.test, "test")? {
                return Err(refuse());
            }
        } else {
            // An example (`cargo test` always builds them) or a kind this has
            // never seen.
            return Err(refuse());
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}

/// The cargo selector arguments for `units`: one list for every unit but
/// [`Unit::Doc`], and a second, `--doc` alone, when `units` holds it. An empty
/// `units` gives no list.
pub(crate) fn selectors(units: &[Unit]) -> Vec<Vec<String>> {
    let mut targets: Vec<String> = Vec::new();
    let mut doc = false;
    for unit in units {
        match unit {
            Unit::Lib => targets.push("--lib".to_owned()),
            Unit::Bin(name) => {
                targets.push("--bin".to_owned());
                targets.push(name.clone());
            }
            Unit::Test(name) => {
                targets.push("--test".to_owned());
                targets.push(name.clone());
            }
            Unit::Doc => doc = true,
        }
    }
    let mut lists: Vec<Vec<String>> = Vec::new();
    if !targets.is_empty() {
        lists.push(targets);
    }
    if doc {
        lists.push(vec!["--doc".to_owned()]);
    }
    lists
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(name: &str, kinds: &[&str], test: bool, doctest: bool) -> Target {
        Target {
            name: name.to_owned(),
            kinds: kinds.iter().map(|kind| (*kind).to_owned()).collect(),
            test: Some(test),
            doctest: Some(doctest),
        }
    }

    #[test]
    fn a_missing_flag_is_refused_not_guessed() {
        let mut silent = target("pharmakos_sim", &["lib"], true, true);
        silent.doctest = None;
        assert_eq!(
            units([("pharmakos-sim", &silent)]),
            Err(ShardError::MissingFlag {
                package: "pharmakos-sim".to_owned(),
                target: "pharmakos_sim".to_owned(),
                flag: "doctest",
            })
        );
    }

    fn shard(value: &str) -> Shard {
        match Shard::parse(value) {
            Ok(shard) => shard,
            Err(error) => panic!("{value}: {error}"),
        }
    }

    #[test]
    fn a_shard_is_n_of_m_counted_from_one() {
        assert_eq!(shard("2/3").to_string(), "2/3");
        assert!(shard("1/3").is_first());
        assert!(!shard("3/3").is_first());
        assert!(shard("1/1").is_first());
        for (value, expected) in [
            ("3", ShardError::Shape("3".to_owned())),
            ("", ShardError::Shape(String::new())),
            ("a/3", ShardError::NotANumber("a/3".to_owned())),
            ("1/", ShardError::NotANumber("1/".to_owned())),
            ("+1/3", ShardError::NotANumber("+1/3".to_owned())),
            ("1/3/4", ShardError::NotANumber("1/3/4".to_owned())),
            (
                "1/99999999999",
                ShardError::NotANumber("1/99999999999".to_owned()),
            ),
            ("0/3", ShardError::Zero("0/3".to_owned())),
            ("1/0", ShardError::Zero("1/0".to_owned())),
        ] {
            assert_eq!(Shard::parse(value), Err(expected), "{value}");
        }
        assert!(matches!(
            Shard::parse("4/3"),
            Err(ShardError::OutOfRange { .. })
        ));
    }

    /// The property the split exists for: over every shard count, each unit
    /// falls to exactly one shard, so the union of the shards is the whole
    /// set and no unit runs twice.
    #[test]
    fn every_unit_falls_to_exactly_one_shard() {
        let mut keys: Vec<String> = vec!["lib".to_owned(), "doc".to_owned()];
        for index in 0..200_u32 {
            keys.push(format!("test:case_{index}"));
            keys.push(format!("bin:tool_{index}"));
        }
        for step in SHARDED_STEPS {
            for total in 1..=7_u32 {
                for key in &keys {
                    let holders = (1..=total)
                        .filter(|index| shard(&format!("{index}/{total}")).holds(step, key))
                        .count();
                    assert_eq!(holders, 1, "{step}/{key} over {total} shard(s)");
                }
            }
        }
    }

    /// Pinned: the split is a function of the names, so it is the same on
    /// every machine. A change here moves every unit to a new shard.
    #[test]
    fn the_hash_is_fnv1a64() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn units_mirror_cargo_tests_default_selection() {
        let targets = [
            (
                "pharmakos-sim",
                target("pharmakos_sim", &["lib"], true, true),
            ),
            (
                "pharmakos-sim",
                target("determinism", &["bin"], true, false),
            ),
            ("pharmakos-sim", target("economy", &["test"], true, false)),
            (
                "pharmakos-gateway",
                target("economy", &["test"], true, false),
            ),
            ("pharmakos-gateway", target("off", &["test"], false, false)),
            (
                "pharmakos-client-gdext",
                target("pharmakos_client_gdext", &["cdylib", "rlib"], true, false),
            ),
            (
                "pharmakos-sim",
                target("build-script-build", &["custom-build"], false, false),
            ),
            ("pharmakos-bench", target("quiet", &["bench"], false, false)),
            ("xtask", target("xtask", &["bin"], true, false)),
        ];
        let found = units(targets.iter().map(|(package, target)| (*package, target)));
        assert_eq!(
            found,
            Ok(vec![
                Unit::Lib,
                Unit::Bin("determinism".to_owned()),
                Unit::Bin("xtask".to_owned()),
                Unit::Test("economy".to_owned()),
                Unit::Doc,
            ])
        );
    }

    #[test]
    fn a_target_no_shard_would_select_is_refused() {
        for refused in [
            target("demo", &["example"], false, false),
            target("loud", &["bench"], true, false),
            target("odd", &["some-new-kind"], true, false),
        ] {
            let found = units([("pharmakos-sim", &refused)]);
            assert!(
                matches!(found, Err(ShardError::UnsupportedTarget { ref target, .. }) if *target == refused.name),
                "{found:?}"
            );
        }
    }

    #[test]
    fn doc_runs_alone_and_nothing_runs_for_no_units() {
        assert!(selectors(&[]).is_empty());
        assert_eq!(selectors(&[Unit::Doc]), vec![vec!["--doc".to_owned()]]);
        assert_eq!(
            selectors(&[Unit::Lib, Unit::Test("save".to_owned()), Unit::Doc]),
            vec![
                vec!["--lib".to_owned(), "--test".to_owned(), "save".to_owned()],
                vec!["--doc".to_owned()],
            ]
        );
    }
}

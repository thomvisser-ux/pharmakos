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
//! A unit falls to shard `fnv1a64("<salt>/<step>/<key>") % M + 1`
//! ([`Shard::holds`], with [`SPLIT_SALT`] as the salt). The hash is a function
//! of the name alone, so the split is the same on every machine and every run,
//! a new test moves no other test, and every unit falls to exactly one shard;
//! `every_unit_falls_to_exactly_one_shard` and main.rs's
//! `the_shards_of_the_real_workspace_are_the_unsharded_set` hold that. The
//! split keeps no timing table: a table would be one more thing to keep
//! current, and a stale one would still be balanced wrongly. Timing enters in
//! one place only, the short list [`HEAVY_UNITS`] of the few units that carry
//! most of a leg's time, and through it the salt: the smallest one for which
//! those units fall to different shards at CI's count (main.rs's
//! `the_heavy_units_fall_apart_on_ci`). Every other unit falls where the hash
//! puts it. The pull requests that introduced the split and the salt state
//! what each shard cost on each operating system.
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

/// The units that carry most of a leg's test time, heaviest first, as `(step,
/// key)`: [`HEAVY_RULE`] applied to [`MEASURED`], which
/// `the_heavy_units_are_the_rule_over_the_measured_runs` holds them to.
/// `the_heavy_units_fall_apart_on_ci` (main.rs) holds [`SPLIT_SALT`] to its
/// rule over them: at CI's shard count each falls to a shard of its own, and
/// the heaviest not to shard 1, which also runs every step that is not
/// sharded.
///
/// On the two runs [`MEASURED`] records, `test`'s `test:allocations` took 199
/// to 324 s a leg (its tick tests, at opt-level 0) and `test`'s `lib` 168 to
/// 364 s (142 to 325 s of it pharmakos-plan-core's unit tests); every other
/// unit took 43 s or less, and `test-research`'s `test:allocations` under a
/// second.
///
/// History: `shard4` #217 read three heavy units from windows-latest's runs
/// of 2026-11-23 (`test`'s `lib` 491 s, `test`'s `test:allocations` 252 s,
/// `test-research`'s `test:allocations` 122 s). Then `simtest` #225 gated the
/// tick tests out of the research build, which left `test-research`'s
/// `test:allocations` one test, and `pcsplit` #224 split
/// pharmakos-plan-core's 456 s test into 16.
// PLACEHOLDER: which units are heavy, read from CI's step timings — main session, when a unit's cost moves
#[cfg(test)]
pub(crate) const HEAVY_UNITS: &[(&str, &str)] = &[("test", "test:allocations"), ("test", "lib")];

/// The rule that reads [`HEAVY_UNITS`] from [`MEASURED`]: a unit is heavy when,
/// on some operating system in some measured run, its seconds are more than
/// one [`HEAVY_RULE`]th of that leg's test time (every unit of `test` and
/// `test-research` on that operating system in that run, together). Heaviest
/// first is by the sum of its seconds over every run and operating system,
/// ties by step and key. On the measured runs each heavy unit takes 17.6 % to
/// 32.3 % of every leg, and no other unit more than 4.2 % of any, so the line
/// sits in a wide gap: the two runs differ by up to 2 to 1 in a unit's
/// seconds (windows-latest's `lib`: 183 s, then 364 s), and neither crosses it.
#[cfg(test)]
pub(crate) const HEAVY_RULE: u32 = 10;

/// The operating systems of [`Measured::leg`] and of each unit's seconds, in
/// order: ci.yml's legs.
#[cfg(test)]
pub(crate) const MEASURED_LEGS: [&str; 3] = ["windows-latest", "macos-latest", "ubuntu-latest"];

/// One `ci.yml` run's test seconds, read from its nine shard jobs' logs: each
/// test executable's libtest `finished in` figure, summed per unit.
#[cfg(test)]
pub(crate) struct Measured {
    /// The run's id.
    pub(crate) run: u64,
    /// The commit it ran.
    pub(crate) head: &'static str,
    /// Per leg ([`MEASURED_LEGS`]): every unit of `test` and `test-research`
    /// together, whole seconds.
    pub(crate) leg: [u32; 3],
    /// Every unit over 30 s on some leg, as `(step, key, seconds per leg)`,
    /// whole seconds; every unit left out is 30 s or less on every leg.
    pub(crate) units: &'static [(&'static str, &'static str, [u32; 3])],
}

/// The runs [`HEAVY_UNITS`] were read from: `main`'s push run of `bdfe7b72`
/// (the merge of `pcsplit` #224, after `simtest` #225), and the last pull
/// request run of #224, whose tree is the same.
#[cfg(test)]
pub(crate) const MEASURED: &[Measured] = &[
    Measured {
        run: 45_342_940_283,
        head: "bdfe7b72",
        leg: [1127, 1020, 988],
        units: &[
            ("test", "test:allocations", [199, 324, 229]),
            ("test", "lib", [364, 220, 227]),
            ("test", "test:operator", [43, 34, 38]),
            ("test", "test:ladder", [31, 36, 29]),
        ],
    },
    Measured {
        run: 45_332_694_060,
        head: "1a0ae693",
        leg: [907, 806, 943],
        units: &[
            ("test", "test:allocations", [259, 205, 282]),
            ("test", "lib", [183, 207, 168]),
            ("test", "test:operator", [27, 34, 30]),
            ("test", "test:fuzz", [34, 19, 28]),
            ("test", "test:ladder", [26, 31, 30]),
        ],
    },
];

/// Hashed in front of every unit's name ([`Shard::holds`]). It is the smallest
/// salt, counting from 0, for which [`HEAVY_UNITS`] fall apart at CI's shard
/// count by the rule `the_heavy_units_fall_apart_on_ci` checks. Over
/// `shard4`'s three heavy units that salt was 0, where the unsalted hash had
/// put `lib` and `test`'s `test:allocations` both on shard 2 of 3 (a
/// 20-minute windows-latest job). Over today's two it is still 0
/// (`test:allocations` on shard 2 of 3, `lib` on shard 3), so `shard5`'s
/// re-derivation moved no unit. A new salt moves every unit at once, so it
/// changes only with [`HEAVY_UNITS`] or the shard count.
// PLACEHOLDER: the split's salt, chosen from CI's step timings through HEAVY_UNITS — main session, when a unit's cost moves
pub(crate) const SPLIT_SALT: u32 = 0;

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
    /// A target `cargo test --workspace` would build that no shard selects the
    /// same way: an example, a bench with `test` set, a kind this has never
    /// seen, or a library, binary or integration test whose `test` (or a
    /// library's `doctest`) flag is off.
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
        self.holds_salted(SPLIT_SALT, step, key)
    }

    /// [`Shard::holds`] under `salt` in place of [`SPLIT_SALT`]: how the
    /// split's test shows that no smaller salt meets its rule.
    pub(crate) fn holds_salted(self, salt: u32, step: &str, key: &str) -> bool {
        let hash = fnv1a64(format!("{salt}/{step}/{key}").as_bytes());
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
        // A unit selects by kind and name across the workspace, and an explicit
        // selector runs a target whatever its flag says: so a library, binary or
        // integration test whose flag turns it off (`test = false`, or a
        // library's `doctest = false`) would run on a shard and not unsharded,
        // or run where a same-named target elsewhere is selected. Refused.
        if LIB_KINDS.iter().any(|kind| has(kind)) {
            if !flag(target.test, "test")? || !flag(target.doctest, "doctest")? {
                return Err(refuse());
            }
            found.push(Unit::Lib);
            found.push(Unit::Doc);
        } else if has("bin") {
            if !flag(target.test, "test")? {
                return Err(refuse());
            }
            found.push(Unit::Bin(target.name.clone()));
        } else if has("test") {
            if !flag(target.test, "test")? {
                return Err(refuse());
            }
            found.push(Unit::Test(target.name.clone()));
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

    /// [`HEAVY_UNITS`] is [`HEAVY_RULE`] applied to [`MEASURED`], heaviest
    /// first: a unit dropped from the list, one added by eye, or the order
    /// changed is red here, and a new run's figures that move a unit across
    /// the rule's line move the list in the same commit.
    #[test]
    fn the_heavy_units_are_the_rule_over_the_measured_runs() {
        assert!(!MEASURED.is_empty(), "the rule reads at least one run");
        let mut heavy: Vec<((&str, &str), u64)> = Vec::new();
        for measured in MEASURED {
            for (name, seconds) in MEASURED_LEGS.iter().zip(measured.leg) {
                assert!(
                    seconds > 0,
                    "run {} ({}): {name} has no test time",
                    measured.run,
                    measured.head
                );
            }
            for (step, key, seconds) in measured.units {
                assert!(
                    seconds.iter().any(|seconds| *seconds > 30),
                    "run {}: {step}/{key} is 30 s or less on every leg, so it is not recorded",
                    measured.run
                );
                let over = seconds.iter().zip(measured.leg).any(|(seconds, leg)| {
                    u64::from(*seconds) * u64::from(HEAVY_RULE) > u64::from(leg)
                });
                if over && !heavy.iter().any(|(unit, _)| *unit == (*step, *key)) {
                    heavy.push(((*step, *key), 0));
                }
            }
        }
        for (unit, total) in &mut heavy {
            *total = MEASURED
                .iter()
                .flat_map(|measured| measured.units)
                .filter(|(step, key, _)| (*step, *key) == *unit)
                .flat_map(|(_, _, seconds)| seconds)
                .map(|seconds| u64::from(*seconds))
                .sum();
        }
        heavy.sort_by(|(left, left_total), (right, right_total)| {
            right_total.cmp(left_total).then(left.cmp(right))
        });
        let derived: Vec<(&str, &str)> = heavy.into_iter().map(|(unit, _)| unit).collect();
        assert_eq!(
            derived, HEAVY_UNITS,
            "HEAVY_UNITS is HEAVY_RULE over MEASURED"
        );
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
                    // Under any salt, too: the salt only relabels the units.
                    for salt in 0..4_u32 {
                        let holders = (1..=total)
                            .filter(|index| {
                                shard(&format!("{index}/{total}")).holds_salted(salt, step, key)
                            })
                            .count();
                        assert_eq!(holders, 1, "{step}/{key} over {total}, salt {salt}");
                    }
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
            (
                "pharmakos-client-gdext",
                target("pharmakos_client_gdext", &["cdylib", "rlib"], true, true),
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
            target("off", &["test"], false, false),
            target("quiet_bin", &["bin"], false, false),
            target("pharmakos_sim", &["lib"], false, true),
            target("pharmakos_sim", &["lib"], true, false),
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

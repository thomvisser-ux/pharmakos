// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What reproduces a certifying run beside its seed and `rules_hash`: the
//! commit it was built from, and the call count it was taken at (S3's plan,
//! section 5 and task `cert3`: "the report's every figure reproducible from
//! its seed, `rules_hash` and commit").
//!
//! The commit is read from git, run in the workspace root (`git rev-parse
//! HEAD`, and `git status --porcelain --untracked-files=no` for tracked
//! changes on top of it). A run that cannot name its commit is refused with
//! [`Error::Provenance`]: a figure nobody can rebuild is not a record. The
//! WSL2 leg's mirror (`wsl-ci.sh`) is a git clone at the mirrored commit, so
//! it names its commit as the Windows leg does.
//!
//! # Certification is opted into, once, for every command
//!
//! Each certifying command (`p1_certify`, `p3_pathing_timings`,
//! `p1_decision_kinds`, `g3_raids_segments`, `g3_watch_sightings_and_breach`)
//! is also, all but the first, a `perf-alarms` step on every CI runner, whose
//! figures are notices (AGENTS.md section 9 item 11; S3's plan, section 5.1
//! item 4). So a run certifies only when the person running it says so, with
//! **one** variable for every command, [`CERTIFY_VAR`], read strictly by
//! [`certify_from`]:
//!
//! * unset: not a certifying run. Every line carries [`CUT_BANNER`],
//!   whatever its counts, and the notices speak as published figures;
//! * exactly `1`: a certifying run. A line is certifying only when every count
//!   that could be cut came from what ran and meets its plan; otherwise it
//!   carries the banner too;
//! * anything else: refused, [`Error::Plan`].
//!
//! [`CERT_CALLS_VAR`] is the other half, and only cuts: unset, P1 takes
//! [`crate::full::CERT_PLAN`]'s calls; a whole number above zero takes that
//! many, and below the plan's count every line of the run says it is cut. No
//! workflow sets either (xtask's `no_workflow_asks_a_bench_run_to_certify`
//! holds it); the owner's recorded
//! runs set [`CERTIFY_VAR`].

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use crate::error::Error;
use crate::full::CERT_PLAN;
use crate::quick::QuickPlan;

/// The environment variable that cuts P1's timed calls a figure: a whole
/// number above zero. Unset, the run takes [`CERT_PLAN`].
pub const CERT_CALLS_VAR: &str = "PHARMAKOS_BENCH_CERT_CALLS";

/// The environment variable that makes a run a certifying one: exactly `1`.
/// Named for what it asks, beside [`CERT_CALLS_VAR`]; unset is not certifying.
pub const CERTIFY_VAR: &str = "PHARMAKOS_BENCH_CERTIFY";

/// Whether a run was asked to certify.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Certify {
    /// Not asked: an alarm's or a hand run's figures, never certifying.
    Off,
    /// Asked, with [`CERTIFY_VAR`] set to `1`.
    On,
}

/// [`CERTIFY_VAR`]'s value read strictly: unset is [`Certify::Off`], exactly
/// `1` is [`Certify::On`].
///
/// # Errors
///
/// [`Error::Plan`] for any other value, so a typo never runs as one or the
/// other.
pub fn certify_from(value: Option<&OsStr>) -> Result<Certify, Error> {
    match value {
        None => Ok(Certify::Off),
        Some(value) if value == "1" => Ok(Certify::On),
        Some(value) => Err(Error::Plan(format!(
            "{CERTIFY_VAR} is `{}`; it is exactly 1 to certify, or unset",
            value.display()
        ))),
    }
}

/// [`certify_from`] on the process's environment.
///
/// # Errors
///
/// As [`certify_from`].
pub fn certify_from_env() -> Result<Certify, Error> {
    certify_from(std::env::var_os(CERTIFY_VAR).as_deref())
}

/// What a line's closing sentence says about the gate, by who prints it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// A `perf-alarms` step or a hand run that is not certifying: published,
    /// gated nowhere.
    Published,
    /// A budget command (`cargo xtask p1-budget`, `p3-budget`): gated at its
    /// bars doubled.
    Alarm,
    /// A certifying run: evidence for its gate, certified on the owner's
    /// machine.
    Certifying,
}

/// The phrase each role, and only it, prints.
pub const PUBLISHED_PHRASE: &str = "published here and gated nowhere";
/// The alarm's phrase.
pub const ALARM_PHRASE: &str = "read by its alarm, which fails at the bars doubled";
/// The certifying run's phrase, before the gate it names.
pub const CERTIFYING_PHRASE: &str = "recorded as evidence for";

impl Role {
    /// The closing words for a figure whose gate is `gate`.
    #[must_use]
    pub fn words(self, gate: &str) -> String {
        match self {
            Role::Published => format!(
                "{PUBLISHED_PHRASE} (the gate, {gate}, is certified on the owner's machine)"
            ),
            Role::Alarm => {
                format!("{ALARM_PHRASE} (the gate, {gate}, is certified on the owner's machine)")
            }
            Role::Certifying => format!("{CERTIFYING_PHRASE} {gate}, on the owner's machine"),
        }
    }

    /// The phrases this role must never print: the other roles'.
    #[must_use]
    pub fn foreign_phrases(self) -> [&'static str; 2] {
        match self {
            Role::Published => [ALARM_PHRASE, CERTIFYING_PHRASE],
            Role::Alarm => [PUBLISHED_PHRASE, CERTIFYING_PHRASE],
            Role::Certifying => [PUBLISHED_PHRASE, ALARM_PHRASE],
        }
    }
}

/// The commit a run was built from.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Commit {
    /// `HEAD`'s full hash, forty lower-case hex digits.
    pub sha: String,
    /// Whether tracked files differ from it: a run on top of uncommitted
    /// changes is not reproducible from the hash alone, and says so.
    pub dirty: bool,
}

/// Run `git` in `root` with `args`, and answer its standard output.
fn git(root: &Path, args: &[&str]) -> Result<String, Error> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| Error::Provenance(format!("running git {}: {error}", args.join(" "))))?;
    if !output.status.success() {
        return Err(Error::Provenance(format!(
            "git {} exited {}: {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout).map_err(|error| {
        Error::Provenance(format!("git {} wrote no text: {error}", args.join(" ")))
    })
}

/// `text` as a commit hash: forty lower-case hex digits and nothing else.
///
/// # Errors
///
/// [`Error::Provenance`] for anything else.
pub fn parse_sha(text: &str) -> Result<String, Error> {
    let sha = text.trim_end_matches(['\n', '\r']);
    if sha.len() == 40
        && sha
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(sha.to_owned())
    } else {
        Err(Error::Provenance(format!(
            "`{sha}` is not a full commit hash"
        )))
    }
}

/// The commit the workspace at `root` stands on.
///
/// # Errors
///
/// [`Error::Provenance`] when git cannot be run there or names no commit.
pub fn commit(root: &Path) -> Result<Commit, Error> {
    let sha = parse_sha(&git(root, &["rev-parse", "HEAD"])?)?;
    let status = git(root, &["status", "--porcelain", "--untracked-files=no"])?;
    Ok(Commit {
        sha,
        dirty: !status.trim().is_empty(),
    })
}

/// The certifying plan, or the cut one `value` (the [`CERT_CALLS_VAR`]
/// variable's value) asks for: [`CERT_PLAN`]'s warm-up, and that many timed
/// calls.
///
/// # Errors
///
/// [`Error::Plan`] for a value that is not text, not a whole number, or zero.
pub fn cert_plan_from(value: Option<&OsStr>) -> Result<QuickPlan, Error> {
    let Some(value) = value else {
        return Ok(CERT_PLAN);
    };
    let text = value
        .to_str()
        .ok_or_else(|| Error::Plan(format!("{CERT_CALLS_VAR} is not text: {}", value.display())))?;
    let calls: usize = text
        .parse()
        .map_err(|_| Error::Plan(format!("{CERT_CALLS_VAR} is `{text}`, not a whole number")))?;
    if calls == 0 {
        return Err(Error::Plan(format!(
            "{CERT_CALLS_VAR} is 0, which measures nothing"
        )));
    }
    Ok(QuickPlan {
        warmup: CERT_PLAN.warmup,
        calls,
    })
}

/// The words for `plan` against [`CERT_PLAN`]: a cut run says it is not a
/// certifying figure. Pure.
#[must_use]
pub fn plan_words(plan: QuickPlan) -> String {
    if plan.calls < CERT_PLAN.calls {
        format!(
            "{} timed calls a figure after {} untimed, CUT from the certifying {}: NOT A \
             CERTIFYING FIGURE",
            plan.calls, plan.warmup, CERT_PLAN.calls
        )
    } else {
        format!(
            "{} timed calls a figure after {} untimed, at least the certifying {}",
            plan.calls, plan.warmup, CERT_PLAN.calls
        )
    }
}

/// The banner every line of a cut run carries.
pub const CUT_BANNER: &str = "NOT A CERTIFYING FIGURE";

/// The mark every line of a run at its certifying count carries.
pub const CERTIFYING_MARK: &str = "[certifying: ";

/// One count a certifying figure rests on: how many of what ran, against how
/// many its plan certifies at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Count {
    /// What ran.
    pub ran: usize,
    /// What the certifying plan takes.
    pub planned: usize,
    /// What `count` counts, as a line prints it.
    pub unit: &'static str,
}

impl Count {
    /// Whether what ran is short of the plan.
    #[must_use]
    pub const fn is_cut(&self) -> bool {
        self.ran < self.planned
    }

    /// The count's words.
    fn words(&self) -> String {
        format!(
            "{} {} of the certifying {}",
            self.ran, self.unit, self.planned
        )
    }
}

/// How a command's figures were sampled: whether it was asked to certify,
/// and every count its figures rest on, each as it ran.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sampling {
    /// Whether the run was asked to certify.
    pub certify: Certify,
    /// Every count that could be cut.
    pub counts: Vec<Count>,
}

impl Sampling {
    /// A P1 plan's sampling: its timed calls a figure against
    /// [`CERT_PLAN`]'s.
    #[must_use]
    pub fn calls(plan: QuickPlan, certify: Certify) -> Sampling {
        Sampling {
            certify,
            counts: vec![Count {
                ran: plan.calls,
                planned: CERT_PLAN.calls,
                unit: "timed calls a figure",
            }],
        }
    }

    /// Whether the run's lines certify: asked to, and no count cut.
    #[must_use]
    pub fn certifies(&self) -> bool {
        self.certify == Certify::On && !self.counts.iter().any(Count::is_cut)
    }

    /// The role the run's notices speak in: [`Role::Certifying`] only when
    /// the run certifies.
    #[must_use]
    pub fn role(&self) -> Role {
        if self.certifies() {
            Role::Certifying
        } else {
            Role::Published
        }
    }

    /// The stamp a line of this run carries at its end.
    #[must_use]
    pub fn stamp(&self) -> String {
        let counts: Vec<String> = self.counts.iter().map(Count::words).collect();
        let counts = counts.join(", ");
        if self.certify == Certify::Off {
            format!("[{CUT_BANNER}: not a certifying run ({CERTIFY_VAR} unset); {counts}]")
        } else if self.certifies() {
            format!("{CERTIFYING_MARK}{counts}]")
        } else {
            format!("[{CUT_BANNER}: a count is cut from its plan; {counts}]")
        }
    }
}

/// `line` with `sampling`'s stamp at its end: every line a certifying command
/// prints goes through here, so a line copied alone into a record carries its
/// count and, cut, the banner.
#[must_use]
pub fn stamped(line: &str, sampling: &Sampling) -> String {
    format!("{line} {}", sampling.stamp())
}

/// `line` with the stamp of a run that ended in a refusal: whatever it was
/// asked and however whole its counts, a run that failed certifies nothing,
/// so the line carries [`CUT_BANNER`], says the run failed in the refusal's
/// own words, and keeps its counts (`cert3`, L-1).
#[must_use]
pub fn stamped_failed(line: &str, sampling: &Sampling, refusal: &str) -> String {
    let counts: Vec<String> = sampling.counts.iter().map(Count::words).collect();
    format!(
        "{line} [{CUT_BANNER}: the run failed ({refusal}); {}]",
        counts.join(", ")
    )
}

/// The one predicate for "this line is a certifying line": it carries the
/// certifying mark and not the cut banner. A line with neither, which no
/// certifying command prints, is not one either.
#[must_use]
pub fn is_certifying(line: &str) -> bool {
    line.contains(CERTIFYING_MARK) && !line.contains(CUT_BANNER)
}

/// The provenance notice a certifying run prints first: what it ran, the
/// commit, and how it was sampled. Pure.
#[must_use]
pub fn notice(what: &str, commit: &Commit, sampled: &str, os: &str) -> String {
    format!(
        "::notice title=provenance ({what})::on {os}, commit {}{}; {sampled}. The seed and \
         rules_hash of each figure are on its own line",
        commit.sha,
        if commit.dirty {
            " WITH UNCOMMITTED CHANGES to tracked files (not reproducible from the hash alone)"
        } else {
            ""
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hash_is_forty_lower_case_hex_digits() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(parse_sha(&format!("{sha}\n")), Ok(sha.to_owned()));
        assert!(parse_sha(&sha.to_uppercase()).is_err());
        assert!(parse_sha(&sha[..39]).is_err());
        assert!(parse_sha("").is_err());
        assert!(parse_sha(&format!("{sha} ")).is_err());
    }

    #[test]
    fn the_cut_plan_is_read_strictly() {
        assert_eq!(cert_plan_from(None), Ok(CERT_PLAN));
        assert_eq!(
            cert_plan_from(Some(OsStr::new("50"))),
            Ok(QuickPlan {
                warmup: CERT_PLAN.warmup,
                calls: 50
            })
        );
        for bad in ["0", "-1", "", "fifty", "5 0"] {
            assert!(
                matches!(cert_plan_from(Some(OsStr::new(bad))), Err(Error::Plan(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_cut_run_says_it_is_not_a_certifying_figure() {
        let cut = QuickPlan {
            warmup: 100,
            calls: 50,
        };
        assert_eq!(
            plan_words(cut),
            "50 timed calls a figure after 100 untimed, CUT from the certifying 2000: NOT A \
             CERTIFYING FIGURE"
        );
        assert!(!plan_words(CERT_PLAN).contains("NOT A CERTIFYING"));
        let commit = Commit {
            sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            dirty: true,
        };
        assert_eq!(
            notice("P1", &commit, &plan_words(cut), "linux"),
            "::notice title=provenance (P1)::on linux, commit \
             0123456789abcdef0123456789abcdef01234567 WITH UNCOMMITTED CHANGES to tracked files \
             (not reproducible from the hash alone); 50 timed calls a figure after 100 untimed, \
             CUT from the certifying 2000: NOT A CERTIFYING FIGURE. The seed and rules_hash of \
             each figure are on its own line"
        );
    }

    #[test]
    fn a_stamp_says_whether_a_line_certifies() {
        let cut_plan = QuickPlan {
            warmup: 100,
            calls: 50,
        };
        let line = stamped(
            "::notice title=x::a figure",
            &Sampling::calls(cut_plan, Certify::On),
        );
        assert_eq!(
            line,
            "::notice title=x::a figure [NOT A CERTIFYING FIGURE: a count is cut from its plan; \
             50 timed calls a figure of the certifying 2000]"
        );
        assert!(!is_certifying(&line));
        let whole = stamped(
            "::notice title=x::a figure",
            &Sampling::calls(CERT_PLAN, Certify::On),
        );
        assert!(is_certifying(&whole), "{whole}");
        // Opted out, a full count is still no certifying line.
        let off = stamped(
            "::notice title=x::a figure",
            &Sampling::calls(CERT_PLAN, Certify::Off),
        );
        assert_eq!(
            off,
            "::notice title=x::a figure [NOT A CERTIFYING FIGURE: not a certifying run \
             (PHARMAKOS_BENCH_CERTIFY unset); 2000 timed calls a figure of the certifying 2000]"
        );
        assert!(!is_certifying(&off));
        assert!(!is_certifying("::notice title=x::a figure"), "no stamp");
        // A cut line that also carried the mark is still not one.
        assert!(!is_certifying(&format!("{whole} {line}")));
        // Every count must meet its plan: one cut of two is cut.
        let two = Sampling {
            certify: Certify::On,
            counts: vec![
                Count {
                    ran: 3,
                    planned: 3,
                    unit: "seeds",
                },
                Count {
                    ran: 8,
                    planned: 2_000,
                    unit: "pairs a seed",
                },
            ],
        };
        assert!(!two.certifies());
        assert_eq!(two.role(), Role::Published);
    }

    #[test]
    fn certification_is_opted_into_strictly() {
        assert_eq!(certify_from(None), Ok(Certify::Off));
        assert_eq!(certify_from(Some(OsStr::new("1"))), Ok(Certify::On));
        for bad in ["0", "true", "yes", "", " 1", "1 "] {
            assert!(
                matches!(certify_from(Some(OsStr::new(bad))), Err(Error::Plan(_))),
                "{bad:?}"
            );
        }
    }

    /// No role prints another's phrase.
    #[test]
    fn each_role_says_only_its_own_words() {
        for role in [Role::Published, Role::Alarm, Role::Certifying] {
            let words = role.words("P1");
            for foreign in role.foreign_phrases() {
                assert!(!words.contains(foreign), "{role:?}: {words}");
            }
            assert!(!words.contains("never gated"), "{role:?}: {words}");
        }
    }

    /// The dirty flag reads tracked changes: a temporary repository, made
    /// with the `git` on the machine and no network, is clean at its commit
    /// and dirty once a tracked file changes.
    #[test]
    fn a_tracked_change_reads_as_dirty() {
        let dir =
            std::env::temp_dir().join(format!("pharmakos-bench-provenance-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("an old scratch repository");
        }
        std::fs::create_dir_all(&dir).expect("a scratch repository");
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .expect("git runs");
            assert!(status.status.success(), "git {args:?}: {status:?}");
        };
        run(&["init", "-q"]);
        std::fs::write(dir.join("a.txt"), "one\n").expect("a file");
        run(&["add", "a.txt"]);
        run(&[
            "-c",
            "user.name=bench",
            "-c",
            "user.email=bench@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=.git/hooks",
            "commit",
            "-q",
            "-m",
            "one",
        ]);
        let clean = commit(&dir).expect("a commit");
        assert!(!clean.dirty, "{clean:?}");
        std::fs::write(dir.join("a.txt"), "two\n").expect("a change");
        let dirty = commit(&dir).expect("a commit");
        assert!(dirty.dirty, "{dirty:?}");
        assert_eq!(dirty.sha, clean.sha);
        std::fs::remove_dir_all(&dir).expect("the scratch repository");
    }

    #[test]
    fn this_checkout_names_its_commit() {
        let commit = commit(&crate::workspace_root()).expect("a commit");
        assert_eq!(commit.sha.len(), 40);
    }
}

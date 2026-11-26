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
//! A certifying run takes [`crate::full::CERT_PLAN`]'s calls. A run on a
//! machine that cannot spare the time may cut them through
//! [`CERT_CALLS_VAR`], read by [`cert_plan_from`]; the same code path runs,
//! and every line it prints carries the count, so a cut run reads as one.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use crate::error::Error;
use crate::full::CERT_PLAN;
use crate::quick::QuickPlan;

/// The environment variable that cuts a certifying run's timed calls a
/// figure: a whole number above zero. Unset, the run takes [`CERT_PLAN`].
pub const CERT_CALLS_VAR: &str = "PHARMAKOS_BENCH_CERT_CALLS";

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

/// How a certifying command's figures were sampled: how many of what, against
/// how many its plan certifies at. A run below its plan is cut.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sampling {
    /// What was taken.
    pub count: usize,
    /// What the certifying plan takes.
    pub planned: usize,
    /// What `count` counts, as a line prints it.
    pub unit: &'static str,
}

impl Sampling {
    /// A P1 plan's sampling: its timed calls a figure against
    /// [`CERT_PLAN`]'s.
    #[must_use]
    pub const fn calls(plan: QuickPlan) -> Sampling {
        Sampling {
            count: plan.calls,
            planned: CERT_PLAN.calls,
            unit: "timed calls a figure",
        }
    }

    /// Whether the run took fewer than its plan certifies at.
    #[must_use]
    pub const fn is_cut(&self) -> bool {
        self.count < self.planned
    }

    /// The stamp a line of this run carries at its end.
    #[must_use]
    pub fn stamp(&self) -> String {
        if self.is_cut() {
            format!(
                "[{CUT_BANNER}: {} {}, cut from the certifying {}]",
                self.count, self.unit, self.planned
            )
        } else {
            format!(
                "{CERTIFYING_MARK}{} {}, the certifying plan's {}]",
                self.count, self.unit, self.planned
            )
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
        let cut = Sampling::calls(QuickPlan {
            warmup: 100,
            calls: 50,
        });
        let line = stamped("::notice title=x::a figure", &cut);
        assert_eq!(
            line,
            "::notice title=x::a figure [NOT A CERTIFYING FIGURE: 50 timed calls a figure, cut \
             from the certifying 2000]"
        );
        assert!(!is_certifying(&line));
        let whole = stamped("::notice title=x::a figure", &Sampling::calls(CERT_PLAN));
        assert!(is_certifying(&whole), "{whole}");
        assert!(!is_certifying("::notice title=x::a figure"), "no stamp");
        // A cut line that also carried the mark is still not one.
        assert!(!is_certifying(&format!("{whole} {line}")));
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

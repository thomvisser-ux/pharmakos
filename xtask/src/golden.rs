// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The golden-file convention, and the comparison the `golden` step runs.
//!
//! # The convention
//!
//! ```text
//! tests/golden/<area>/README.md          what a diff in this area means
//! tests/golden/<area>/<case>/expected.*  committed, reviewed, human-diffable
//! <target>/golden/<area>/<case>/actual.* written fresh by the test that produces it
//! ```
//!
//! The producing test writes `actual.<ext>` under Cargo's target directory as
//! it runs; this step byte-compares every `expected.*` against the `actual.*`
//! beside it. `cargo xtask golden --bless` copies actual over expected, and a
//! blessed golden has to be explained in the pull request that moves it
//! (AGENTS.md section 5: never regenerate a golden to turn a red test green
//! without saying what behaviour change moved it).
//!
//! Three rules hold for every golden in the tree, and they are enforced here
//! rather than asked for in prose:
//!
//! 1. **A fresh output that is missing is a failure, not a skip.** A golden with
//!    nothing to compare against checks nothing, and a step that reports `ok`
//!    for work it did not do is the failure mode this whole harness exists to
//!    avoid (spike G1 section 10.12).
//! 2. **Every area carries a `README.md` saying what a diff there means.** A
//!    hash chain that moved, a report hash that moved and a prose golden that
//!    moved are three different kinds of news, and the person reading the red
//!    build is usually not the person who wrote the golden.
//! 3. **Goldens are human-diffable** (AGENTS.md section 9 item 7) — text where
//!    text will do, one record per line, LF endings. The one exception the
//!    skeleton plans for is the vista PNG, which is compared by
//!    [`crate::png`] with a tolerance rather than byte for byte.
//!
//! # Areas
//!
//! The skeleton freezes the area list up front (plan section 5) so that later
//! stages add *files*, never formats: `determinism`, `pathing`, `proto`,
//! `plan-core`, `verifier`, `interpreter`, `economy`, `mapgen`, `mesher`,
//! `schema`, `docs`, `scenarios`, `vista`. Each has a README committed with
//! this task; the tasks that produce the goldens fill the directories.

use std::fs;
use std::path::{Path, PathBuf};

/// What the comparison found.
#[derive(Debug)]
pub(crate) struct Report {
    pub(crate) matched: usize,
    pub(crate) blessed: usize,
    pub(crate) areas: usize,
}

/// Compares every `tests/golden/**/expected.*` with the `actual.*` beside it
/// under `actual_root`, and checks the per-area README convention.
///
/// Pure in everything but the filesystem, so the self-test can drive it over a
/// scratch tree — the checker is tested, not just the things it checks.
pub(crate) fn compare_tree(
    golden_root: &Path,
    actual_root: &Path,
    bless: bool,
) -> Result<Report, String> {
    let mut files: Vec<PathBuf> = Vec::new();
    crate::walk(golden_root, &mut files)
        .map_err(|error| format!("reading {}: {error}", golden_root.display()))?;
    let expected_files: Vec<PathBuf> = files
        .into_iter()
        .filter(|path| crate::file_name(path).starts_with("expected."))
        .collect();

    let mut failures: Vec<String> = Vec::new();
    let mut areas: Vec<String> = Vec::new();
    let mut blessed: usize = 0;
    let mut matched: usize = 0;

    for expected_path in &expected_files {
        let relative = expected_path
            .strip_prefix(golden_root)
            .map_err(|error| format!("path outside the golden root: {error}"))?;

        if let Some(area) = relative
            .components()
            .next()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
        {
            if !areas.contains(&area) {
                areas.push(area.clone());
                let readme = golden_root.join(&area).join("README.md");
                if !readme.is_file() {
                    failures.push(format!(
                        "{area}: no tests/golden/{area}/README.md. Every golden area says what a \
                         diff in it means — which behaviour moved, whether a move is ever \
                         legitimate, and what to check before blessing it. The person reading the \
                         red build is usually not the person who wrote the golden."
                    ));
                }
            }
        }

        let name = crate::file_name(expected_path);
        let suffix = name.strip_prefix("expected.").unwrap_or("out");
        let actual_path = actual_root
            .join(relative)
            .with_file_name(format!("actual.{suffix}"));

        let Ok(actual) = fs::read(&actual_path) else {
            failures.push(format!(
                "{}: no fresh output at {} — run the test that produces it first",
                relative.display(),
                actual_path.display()
            ));
            continue;
        };
        let expected = fs::read(expected_path)
            .map_err(|error| format!("reading {}: {error}", expected_path.display()))?;

        if expected == actual {
            matched += 1;
        } else if bless {
            fs::write(expected_path, &actual)
                .map_err(|error| format!("writing {}: {error}", expected_path.display()))?;
            blessed += 1;
        } else {
            failures.push(format!(
                "{} differs from {}\n{}",
                relative.display(),
                actual_path.display(),
                crate::first_difference(&expected, &actual)
            ));
        }
    }

    if !failures.is_empty() {
        let mut report = String::from(
            "golden files do not match (`cargo xtask golden --bless` accepts them):\n",
        );
        for failure in &failures {
            report.push_str("      - ");
            report.push_str(failure);
            report.push('\n');
        }
        report.push_str(
            "      A golden that moved is a behaviour change. Say which one in the pull request; \
             tests/golden/<area>/README.md says what a diff in that area means.",
        );
        return Err(report);
    }

    Ok(Report {
        matched,
        blessed,
        areas: areas.len(),
    })
}

/// True when the tree holds at least one committed golden. Used by the step to
/// tell "nothing to check yet" from "nothing matched".
pub(crate) fn has_goldens(golden_root: &Path) -> Result<bool, String> {
    let mut files: Vec<PathBuf> = Vec::new();
    crate::walk(golden_root, &mut files)
        .map_err(|error| format!("reading {}: {error}", golden_root.display()))?;
    Ok(files
        .iter()
        .any(|path| crate::file_name(path).starts_with("expected.")))
}

// ---------------------------------------------------------------------------
// Tests — the golden-format self-test T3's acceptance asks for
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree {
        golden: PathBuf,
        actual: PathBuf,
    }

    fn scratch(name: &str) -> Tree {
        let base = std::env::temp_dir()
            .join("pharmakos-xtask-tests")
            .join("golden")
            .join(name);
        let _ = fs::remove_dir_all(&base);
        let golden = base.join("tests").join("golden");
        let actual = base.join("target").join("golden");
        fs::create_dir_all(golden.join("prose").join("case")).expect("golden dir");
        fs::create_dir_all(actual.join("prose").join("case")).expect("actual dir");
        fs::write(
            golden.join("prose").join("README.md"),
            "# prose goldens\n\nA diff here means `render_plan` says something different.\n",
        )
        .expect("area readme");
        Tree { golden, actual }
    }

    fn put(tree: &Tree, expected: &str, actual: &str) {
        fs::write(
            tree.golden.join("prose").join("case").join("expected.txt"),
            expected,
        )
        .expect("expected");
        fs::write(
            tree.actual.join("prose").join("case").join("actual.txt"),
            actual,
        )
        .expect("actual");
    }

    #[test]
    fn a_matching_pair_passes() {
        let tree = scratch("match");
        put(&tree, "alpha\nbeta\ngamma\n", "alpha\nbeta\ngamma\n");
        let report = compare_tree(&tree.golden, &tree.actual, false).expect("matches");
        assert_eq!(report.matched, 1);
        assert_eq!(report.blessed, 0);
        assert_eq!(report.areas, 1);
    }

    #[test]
    fn a_mismatched_fixture_yields_a_readable_first_difference() {
        let tree = scratch("mismatch");
        put(&tree, "alpha\nbeta\ngamma\n", "alpha\ndelta\ngamma\n");
        let report = compare_tree(&tree.golden, &tree.actual, false).expect_err("differs");

        // It names the file, the line, and both sides — the three things
        // somebody staring at a red build needs before they can act.
        assert!(report.contains("expected.txt"), "{report}");
        assert!(report.contains("first difference at line 2"), "{report}");
        assert!(report.contains("beta"), "{report}");
        assert!(report.contains("delta"), "{report}");
        assert!(report.contains("--bless"), "{report}");
        assert!(report.contains("behaviour change"), "{report}");
    }

    #[test]
    fn a_missing_fresh_output_is_a_failure_not_a_pass() {
        let tree = scratch("missing-actual");
        fs::write(
            tree.golden.join("prose").join("case").join("expected.txt"),
            "alpha\n",
        )
        .expect("expected");
        let report = compare_tree(&tree.golden, &tree.actual, false).expect_err("no fresh output");
        assert!(report.contains("no fresh output"), "{report}");
    }

    #[test]
    fn an_area_without_a_readme_is_a_failure() {
        let tree = scratch("no-readme");
        put(&tree, "alpha\n", "alpha\n");
        fs::remove_file(tree.golden.join("prose").join("README.md")).expect("remove readme");
        let report = compare_tree(&tree.golden, &tree.actual, false).expect_err("no README");
        assert!(report.contains("README.md"), "{report}");
        assert!(report.contains("what a diff in it means"), "{report}");
    }

    #[test]
    fn blessing_rewrites_the_golden_and_says_so() {
        let tree = scratch("bless");
        put(&tree, "alpha\n", "omega\n");
        let report = compare_tree(&tree.golden, &tree.actual, true).expect("blessed");
        assert_eq!(report.blessed, 1);
        let now = fs::read_to_string(tree.golden.join("prose").join("case").join("expected.txt"))
            .expect("read back");
        assert_eq!(now, "omega\n");
    }

    #[test]
    fn an_empty_tree_reports_no_goldens() {
        let base = std::env::temp_dir()
            .join("pharmakos-xtask-tests")
            .join("golden")
            .join("empty");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).expect("dir");
        assert!(!has_goldens(&base).expect("readable"));
    }

    #[test]
    fn every_committed_area_carries_its_readme() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask sits under the workspace root")
            .join("tests")
            .join("golden");
        let mut entries: Vec<PathBuf> = Vec::new();
        crate::walk(&root, &mut entries).expect("tests/golden is readable");
        let mut areas: Vec<String> = Vec::new();
        for path in &entries {
            if let Ok(relative) = path.strip_prefix(&root) {
                // Only files nested inside a directory name an area; the root
                // README.md is one component deep and is not one.
                if relative.components().count() < 2 {
                    continue;
                }
                if let Some(first) = relative.components().next() {
                    let area = first.as_os_str().to_string_lossy().into_owned();
                    if !areas.contains(&area) {
                        areas.push(area);
                    }
                }
            }
        }
        assert!(
            !areas.is_empty(),
            "the golden area layout is committed with T3"
        );
        for area in &areas {
            assert!(
                root.join(area).join("README.md").is_file(),
                "tests/golden/{area}/README.md is missing — every area says what a diff means"
            );
        }
    }
}

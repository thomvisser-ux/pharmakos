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
//! # The two areas this step does not compare
//!
//! [`SELF_COMPARED_AREAS`] names them, and it is the only place they are named.
//! Both own a comparison of their own, and running a second one here would be
//! wrong rather than redundant:
//!
//! * `determinism/` — the `determinism` step runs the sim, writes the fresh
//!   chain to `<target>/determinism/hashes.txt` and byte-compares it there,
//!   because it also has to validate the chain's *format* line by line and say
//!   which tick first diverged. Re-baseline it with
//!   `cargo xtask determinism --bless`.
//! * `vista/` — [`crate::png`] compares the render with a tolerance, because the
//!   two sides may be different rasterisers (spike G1 measured 1.14 % of pixels
//!   differing at all between a Quadro and lavapipe on identical geometry), and
//!   because the fresh render exists only on the Linux leg that produced it.
//!   Byte equality here would be permanently red on every platform.
//!
//! Rule 1 is what makes the exemption necessary rather than tidy: a missing
//! fresh output is a failure, so an area whose producer runs in a later step —
//! or on one operating system only — must not be compared here at all. Rule 2
//! still holds for both: each carries its README, and this step still fails an
//! area that does not.
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

/// Areas whose goldens are compared by a step of their own rather than byte for
/// byte here. Stated once; the module docs say why each is on the list.
///
/// Adding a name silently removes an area from this step's byte comparison, so
/// it is reviewed like any other golden-format change (AGENTS.md section 5) and
/// `tests/golden/README.md` names the same two.
pub(crate) const SELF_COMPARED_AREAS: &[&str] = &["determinism", "vista"];

/// True when `relative`'s first component is one of [`SELF_COMPARED_AREAS`].
fn self_compared(relative: &Path) -> bool {
    relative.components().next().is_some_and(|component| {
        SELF_COMPARED_AREAS
            .iter()
            .any(|area| component.as_os_str() == *area)
    })
}

/// What the comparison found.
#[derive(Debug)]
pub(crate) struct Report {
    pub(crate) matched: usize,
    pub(crate) blessed: usize,
    pub(crate) areas: usize,
    /// Goldens left to the step that owns them ([`SELF_COMPARED_AREAS`]).
    pub(crate) deferred: usize,
}

/// Rule 3's machine-checkable half: a text file under `tests/golden/` has LF
/// endings and a trailing newline.
///
/// This exists because **nothing else in the toolchain can catch it.**
/// `.gitattributes` sets `* text=auto eol=lf` for the repository and then
/// exempts this tree with `tests/golden/** -text`, deliberately: a golden is
/// byte-compared across Windows, Linux and macOS, so git must not rewrite one
/// on checkout. The cost of that exemption is that a CRLF file committed here
/// stays CRLF, in the one directory where bytes are the whole point, and
/// neither git nor `reuse` nor rustfmt has an opinion about it. It is an easy
/// mistake to make from Windows — a helper script that writes with the
/// platform default is enough — and it surfaces as a golden that is identical
/// on screen and unequal to the comparison.
///
/// Binary goldens are skipped by the only test that is always right about
/// them: a file whose bytes are not valid UTF-8 is not a text file. A PNG is
/// full of `\r` bytes and must keep every one.
///
/// READMEs are checked alongside the goldens, not exempted. They live under the
/// same `-text` exemption, and rule 3 is about the tree.
///
/// Called by [`compare_tree`] and, separately, by the step *before* the
/// "nothing to compare yet" skip — a tree whose only goldens are self-compared
/// still has bytes, and a skip that skipped this check would be the quiet pass
/// rule 1 is about.
pub(crate) fn check_endings(golden_root: &Path) -> Result<(), String> {
    let mut files: Vec<PathBuf> = Vec::new();
    crate::walk(golden_root, &mut files)
        .map_err(|error| format!("reading {}: {error}", golden_root.display()))?;

    let mut failures: Vec<String> = Vec::new();
    for path in &files {
        let bytes =
            fs::read(path).map_err(|error| format!("reading {}: {error}", path.display()))?;
        if bytes.is_empty() || std::str::from_utf8(&bytes).is_err() {
            continue;
        }
        let relative = path.strip_prefix(golden_root).unwrap_or(path);
        if bytes.contains(&b'\r') {
            failures.push(format!(
                "{}: contains a carriage return. Goldens are byte-compared across Windows, Linux \
                 and macOS, and `.gitattributes` marks tests/golden/** as `-text` so git will not \
                 rewrite this for you — that is the point. Rewrite the file with LF endings.",
                relative.display()
            ));
        }
        if bytes.last() != Some(&b'\n') {
            failures.push(format!(
                "{}: no trailing newline. A golden is one record per line, so the last record \
                 needs its ending like every other one.",
                relative.display()
            ));
        }
    }

    if failures.is_empty() {
        return Ok(());
    }
    let mut report = String::from("golden files are not byte-clean:\n");
    for failure in &failures {
        report.push_str("      - ");
        report.push_str(failure);
        report.push('\n');
    }
    report.push_str(
        "      tests/golden/README.md rule 3. `--bless` does not fix this: it copies the fresh \
         output over the golden, so a producer writing CRLF would write it again.",
    );
    Err(report)
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

    check_endings(golden_root)?;

    let mut failures: Vec<String> = Vec::new();
    let expected_files: Vec<PathBuf> = files
        .into_iter()
        .filter(|path| crate::file_name(path).starts_with("expected."))
        .collect();

    let mut areas: Vec<String> = Vec::new();
    let mut blessed: usize = 0;
    let mut matched: usize = 0;
    let mut deferred: usize = 0;

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

        // `determinism/` and `vista/` are compared by the steps that produce
        // them, and neither has produced anything by the time this step runs.
        // Comparing here would fail rule 1 ("a missing fresh output is a
        // failure") on a clean checkout, and in `vista/`'s case would also
        // replace a deliberate tolerance with byte equality.
        if self_compared(relative) {
            deferred += 1;
            continue;
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
        deferred,
    })
}

/// True when the tree holds at least one committed golden **this step compares**.
/// Used by the step to tell "nothing to check yet" from "nothing matched".
///
/// Goldens in [`SELF_COMPARED_AREAS`] do not count: a tree holding only the
/// committed hash chain and the vista PNG has nothing for this step to do, and
/// reporting `ok` for it would be the quiet pass rule 1 exists to prevent.
pub(crate) fn has_goldens(golden_root: &Path) -> Result<bool, String> {
    let mut files: Vec<PathBuf> = Vec::new();
    crate::walk(golden_root, &mut files)
        .map_err(|error| format!("reading {}: {error}", golden_root.display()))?;
    Ok(files.iter().any(|path| {
        if !crate::file_name(path).starts_with("expected.") {
            return false;
        }
        match path.strip_prefix(golden_root) {
            Ok(relative) => !self_compared(relative),
            Err(_) => false,
        }
    }))
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

    /// The mistake this catches was made for real while writing the pull
    /// request that added the check: a helper script wrote three files with the
    /// platform's default newline, and `tests/golden/** -text` in
    /// `.gitattributes` handed them to git unchanged. Nothing else in the
    /// toolchain said a word.
    #[test]
    fn a_crlf_golden_is_refused_and_bless_is_not_offered_as_the_fix() {
        let tree = scratch("crlf");
        put(&tree, "alpha\r\nbeta\r\n", "alpha\nbeta\n");
        let report = compare_tree(&tree.golden, &tree.actual, false).expect_err("refused");
        assert!(report.contains("carriage return"), "{report}");
        assert!(report.contains("expected.txt"), "{report}");
        assert!(report.contains("does not fix this"), "{report}");
    }

    #[test]
    fn a_crlf_area_readme_is_refused_too() {
        let tree = scratch("crlf-readme");
        put(&tree, "alpha\n", "alpha\n");
        fs::write(tree.golden.join("prose").join("README.md"), "# prose\r\n").expect("readme");
        let report = compare_tree(&tree.golden, &tree.actual, false).expect_err("refused");
        assert!(report.contains("carriage return"), "{report}");
        assert!(report.contains("README.md"), "{report}");
    }

    #[test]
    fn a_golden_with_no_trailing_newline_is_refused() {
        let tree = scratch("no-trailing-newline");
        put(&tree, "alpha\nbeta", "alpha\nbeta");
        let report = compare_tree(&tree.golden, &tree.actual, false).expect_err("refused");
        assert!(report.contains("no trailing newline"), "{report}");
    }

    /// A PNG is full of `\r` bytes — its signature carries one — and must keep
    /// every one of them. "Not valid UTF-8" is the test that is always right
    /// about which files these rules apply to.
    #[test]
    fn a_binary_golden_keeps_its_bytes() {
        let tree = scratch("binary");
        put(&tree, "alpha\n", "alpha\n");
        fs::create_dir_all(tree.golden.join("vista")).expect("area");
        fs::write(tree.golden.join("vista").join("README.md"), "# vista\n").expect("readme");
        fs::write(
            tree.golden.join("vista").join("expected.vista.png"),
            [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0xff, 0xfe],
        )
        .expect("png");
        let report = compare_tree(&tree.golden, &tree.actual, false).expect("binary is skipped");
        assert_eq!(report.matched, 1);
        assert_eq!(report.deferred, 1);
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

    /// Writes `tests/golden/<area>/<name>` plus that area's README, and returns
    /// the area directory. Used for the areas the step deliberately leaves to
    /// somebody else.
    fn put_area(tree: &Tree, area: &str, name: &str, body: &[u8]) -> PathBuf {
        let dir = tree.golden.join(area);
        fs::create_dir_all(&dir).expect("area dir");
        fs::write(
            dir.join("README.md"),
            format!("# {area} goldens\n\nWhat a diff here means.\n"),
        )
        .expect("area readme");
        fs::write(dir.join(name), body).expect("golden");
        dir
    }

    #[test]
    fn a_self_compared_area_is_left_to_the_step_that_owns_it() {
        // The `determinism` step runs the sim, writes its chain to
        // <target>/determinism/hashes.txt and compares it there. Nothing ever
        // lands at <target>/golden/determinism/actual.hashes.txt, so comparing
        // the committed chain here would fail rule 1 on every operating system
        // the moment T2 commits it.
        let tree = scratch("self-compared");
        put(&tree, "alpha\n", "alpha\n");
        put_area(&tree, "determinism", "expected.hashes.txt", b"0\t0000\n");
        let report = compare_tree(&tree.golden, &tree.actual, false)
            .expect("the determinism chain is not this step's to compare");
        assert_eq!(report.matched, 1);
        assert_eq!(report.deferred, 1);
        assert_eq!(
            report.areas, 2,
            "the area is still counted and still needs its README"
        );
    }

    #[test]
    fn a_committed_vista_golden_with_no_render_beside_it_does_not_fail() {
        // The vista is compared with a tolerance by `crate::png`, from the
        // `screenshot` step, which runs after this one and only on Linux. A byte
        // comparison here would be permanently red — and permanently
        // unsatisfiable on Windows and macOS, where no render is ever produced.
        let tree = scratch("vista");
        put(&tree, "alpha\n", "alpha\n");
        put_area(&tree, "vista", "expected.vista.png", b"\x89PNG\r\n\x1a\n");
        let report = compare_tree(&tree.golden, &tree.actual, false)
            .expect("the vista is png.rs's to compare, with a tolerance");
        assert_eq!(report.deferred, 1);
    }

    #[test]
    fn a_tree_of_only_self_compared_goldens_reports_nothing_to_do() {
        let base = std::env::temp_dir()
            .join("pharmakos-xtask-tests")
            .join("golden")
            .join("only-self-compared");
        let _ = fs::remove_dir_all(&base);
        let golden = base.join("tests").join("golden");
        fs::create_dir_all(&golden).expect("golden dir");
        let tree = Tree {
            golden: golden.clone(),
            actual: base.join("target").join("golden"),
        };
        put_area(&tree, "determinism", "expected.hashes.txt", b"0\t0000\n");
        put_area(&tree, "vista", "expected.vista.png", b"\x89PNG\r\n\x1a\n");
        assert!(
            !has_goldens(&golden).expect("readable"),
            "this step has nothing to compare, and must not report `ok` for work it did not do"
        );
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

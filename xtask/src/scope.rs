// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask ci-scope` — the docs-only fast path's decision
//! (decisions-log item 115 (4)).
//!
//! A pull request that changes only prose should not wait for three
//! `cargo xtask ci` legs. The ruleset's required checks still have to report
//! under their own names, so every job still runs; each one that builds asks
//! this command, right after its checkout and toolchain steps, whether anything
//! but prose changed, and skips its remaining steps when nothing did.
//!
//! The answer is `prose` only when all of this holds, and `full` otherwise:
//!
//! * the event is `pull_request` (a push to `main`, a `workflow_dispatch` and
//!   every other event run the whole suite);
//! * `HEAD` — the test merge commit `actions/checkout` checks out for a pull
//!   request, `refs/pull/N/merge` — has exactly two parents, and the second is
//!   the pull request's head (`$PR_HEAD_SHA`); the first is the base branch's
//!   tip;
//! * `git diff --no-renames --no-relative --ignore-submodules=none --name-only
//!   -z HEAD^1 HEAD` names at least one path, and every path it names is on the
//!   allow-list: `docs/**`, `AGENTS.md`, `CLAUDE.md`, `.claude/**` and a
//!   top-level `*.md`.
//!
//! Why that diff and no other. `HEAD^1..HEAD` is exactly what the pull request
//! changes against the base branch as it is now: `HEAD^2..HEAD` would be the
//! base branch's side of the merge and hide the pull request's own changes, and
//! a merge-base diff against the merge commit would count the base branch's
//! commits since the branch point, so the fast path would stop firing once
//! `main` moved. `--no-renames` lists both sides of a rename, so a file moved
//! into `docs/` from anywhere else is not prose. `-z` keeps a name with a
//! non-ASCII character, a quote, a backslash or a control character unquoted.
//! The paths come from the checkout, never from the network, so there is no
//! token, no rate limit and no file cap.
//!
//! Every error — a git command that fails, output that is not UTF-8, a missing
//! or malformed input — answers `full`, and no input can force `prose`: the
//! allow-list allows rather than denies, so a path wrongly counted as prose is
//! the only way this can skip the suite, and the tests below pin the list.
//!
//! Those tests do not guard a pull request that breaks this file, though: CI
//! runs the merge commit's own copy of the decider, and the tests that would
//! catch the break are among the steps a `prose` answer skips. So each scope
//! step in `.github/workflows/ci.yml` answers `full` in bash, before it builds
//! xtask, whenever the commit changes `xtask/`, `.github/` or `.cargo/` (the
//! `cargo xtask` alias). A change to this file therefore always runs the full
//! suite, whatever this file says about it.
//!
//! The decision ([`decide`]) is a function of the event, the pull request's
//! head and a repository directory, and runs the real git commands; only
//! [`run_from_env`] reads the environment and writes `$GITHUB_OUTPUT`, so the
//! tests (which themselves run inside CI's `cargo xtask ci` step, where those
//! variables are set) pass every input explicitly.

use std::env;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;
use std::process::Stdio;

/// The one event whose checkout can take the fast path.
const PULL_REQUEST: &str = "pull_request";

/// Directories whose every file is prose, as `git diff` spells a path.
const PROSE_DIRECTORIES: &[&str] = &["docs/", ".claude/"];

/// Top-level files that are prose by name. Every other top-level `*.md` is prose
/// too (see [`is_prose`]); these two are named because item 115 (4) names them.
const PROSE_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md"];

/// Environment variables that would point a git command at another repository
/// or index than the directory it runs in. Removed from every git command here,
/// so that a run inside a git hook, which sets some of them, still reads the
/// repository it was given.
const GIT_REDIRECTS: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
];

/// The decision, with the reason it prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Scope {
    /// Run everything: the suite, the client check, the vista.
    Full(String),
    /// Only prose changed: run the DCO walk and the `reuse` step alone.
    Prose(String),
}

impl Scope {
    /// True for [`Scope::Prose`].
    pub(crate) fn prose_only(&self) -> bool {
        matches!(self, Scope::Prose(_))
    }

    /// The line the command prints: `full: <reason>` or `prose: <reason>`.
    pub(crate) fn line(&self) -> String {
        match self {
            Scope::Full(reason) => format!("full: {reason}"),
            Scope::Prose(reason) => format!("prose: {reason}"),
        }
    }
}

/// True when a path, as `git diff --name-only` prints it from the repository
/// root, is prose: under `docs/` or `.claude/`, `AGENTS.md`, `CLAUDE.md`, or a
/// `*.md` at the top level. A `*.md` anywhere else — `godot/README.md`, a
/// golden area's `README.md` — is not prose: those files sit beside what the
/// suite checks, and some of them are inputs to it.
pub(crate) fn is_prose(path: &str) -> bool {
    for directory in PROSE_DIRECTORIES {
        if let Some(rest) = path.strip_prefix(directory) {
            return !rest.is_empty();
        }
    }
    if PROSE_FILES.contains(&path) {
        return true;
    }
    // A top-level `*.md` with a non-empty stem.
    !path.contains('/')
        && path
            .strip_suffix(".md")
            .is_some_and(|stem| !stem.is_empty())
}

/// The decision over a path list the diff produced: `prose` when the list is
/// not empty and every path is prose, `full` naming the first path that is not.
pub(crate) fn classify(paths: &[String]) -> Scope {
    if paths.is_empty() {
        return Scope::Full("the pull request's merge commit changes no path".to_owned());
    }
    if let Some(path) = paths.iter().find(|path| !is_prose(path)) {
        return Scope::Full(format!("`{path}` is not on the prose allow-list"));
    }
    let count = paths.len();
    let noun = if count == 1 { "path" } else { "paths" };
    Scope::Prose(format!(
        "{count} {noun} changed, all on the prose allow-list \
         (docs/**, AGENTS.md, CLAUDE.md, .claude/**, top-level *.md)"
    ))
}

/// The decision itself: the event, the pull request's head commit and the
/// repository to read, every one of them passed in. Runs the real git commands
/// in `repository`.
pub(crate) fn decide(event: Option<&str>, pr_head: Option<&str>, repository: &Path) -> Scope {
    match event {
        Some(PULL_REQUEST) => {}
        Some(other) => {
            return Scope::Full(format!(
                "the event is `{other}`, and only `{PULL_REQUEST}` can take the fast path"
            ));
        }
        None => return Scope::Full("no event name was given".to_owned()),
    }
    let pr_head = match pr_head.map(str::trim) {
        Some(sha) if is_object_name(sha) => sha,
        Some("") | None => {
            return Scope::Full("the pull request's head commit is not set".to_owned());
        }
        Some(other) => {
            return Scope::Full(format!(
                "the pull request's head `{other}` is not a full commit id"
            ));
        }
    };

    // Plumbing, so no `log.*` setting can add a line: HEAD's own id, then its
    // parents, first parent first, on one line.
    let listing = match git(repository, &["rev-list", "--parents", "-n", "1", "HEAD"]) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => return Scope::Full(format!("error: git's parent list: {error}")),
        },
        Err(error) => return Scope::Full(format!("error: {error}")),
    };
    let parents: Vec<&str> = listing.split_whitespace().skip(1).collect();
    let [_, second] = parents.as_slice() else {
        return Scope::Full(format!(
            "HEAD has {} parent(s), not the two of a pull request's merge commit",
            parents.len()
        ));
    };
    if *second != pr_head {
        return Scope::Full(format!(
            "HEAD's second parent {second} is not the pull request's head {pr_head}"
        ));
    }

    let diff = match git(
        repository,
        &[
            "diff",
            "--no-renames",
            "--no-relative",
            "--ignore-submodules=none",
            "--name-only",
            "-z",
            "HEAD^1",
            "HEAD",
        ],
    ) {
        Ok(bytes) => bytes,
        Err(error) => return Scope::Full(format!("error: {error}")),
    };
    match split_paths(&diff) {
        Ok(paths) => classify(&paths),
        Err(error) => Scope::Full(format!("error: {error}")),
    }
}

/// Splits `git diff -z --name-only` output into paths: NUL-terminated, never
/// quoted. A path that is not UTF-8 is an error, and so the answer is `full`.
fn split_paths(output: &[u8]) -> Result<Vec<String>, String> {
    let mut paths: Vec<String> = Vec::new();
    for raw in output.split(|byte| *byte == 0) {
        if raw.is_empty() {
            continue;
        }
        let path = std::str::from_utf8(raw)
            .map_err(|error| format!("a changed path is not UTF-8 ({error})"))?;
        paths.push(path.to_owned());
    }
    Ok(paths)
}

/// True for a full SHA-1 or SHA-256 object name in lower-case hex.
fn is_object_name(text: &str) -> bool {
    (text.len() == 40 || text.len() == 64)
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Runs git in `repository` and returns its standard output, or why it failed.
///
/// Through [`crate::child_command`], as every child of xtask is (S1-07). The
/// require-tools answer is read the way `ci` reads it; a value the reader
/// refuses is an error here, which the caller answers `full` on, like every
/// other error.
fn git(repository: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let require_tools =
        crate::require_tools_from(env::var_os(crate::REQUIRE_TOOLS_VAR).as_deref())?;
    let mut command = crate::child_command(require_tools, "git", args, repository);
    command.stdin(Stdio::null()).stderr(Stdio::piped());
    for name in GIT_REDIRECTS {
        command.env_remove(name);
    }
    let output = command
        .output()
        .map_err(|error| format!("could not run `git {}`: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "`git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

/// Appends `prose_only=true` or `prose_only=false` to a `$GITHUB_OUTPUT` file.
fn write_output(path: &Path, prose_only: bool) -> Result<(), String> {
    let line = format!("prose_only={prose_only}\n");
    OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .and_then(|mut file| file.write_all(line.as_bytes()))
        .map_err(|error| format!("cannot append to {}: {error}", path.display()))
}

/// `cargo xtask ci-scope`: reads `$GITHUB_EVENT_NAME`, `$PR_HEAD_SHA` and the
/// checkout at the workspace root, prints the answer, and appends it to
/// `$GITHUB_OUTPUT` when that is set. `extra` is every other argument on the
/// command line; any at all answer `full`, so nothing typed can force `prose`.
///
/// Returns true (exit 0) whenever it printed an answer, which is always: an
/// error is an answer, `full`.
pub(crate) fn run_from_env(extra: &[&String]) -> bool {
    let scope = if extra.is_empty() {
        match crate::workspace_root() {
            Ok(root) => decide(
                env::var("GITHUB_EVENT_NAME").ok().as_deref(),
                env::var("PR_HEAD_SHA").ok().as_deref(),
                &root,
            ),
            Err(error) => Scope::Full(format!("error: {error}")),
        }
    } else {
        let names: Vec<&str> = extra.iter().map(|arg| arg.as_str()).collect();
        Scope::Full(format!(
            "ci-scope takes no arguments, and was given `{}`",
            names.join(" ")
        ))
    };

    let mut line = scope.line();
    match env::var_os("GITHUB_OUTPUT") {
        Some(path) if !path.is_empty() => {
            if let Err(error) = write_output(Path::new(&path), scope.prose_only()) {
                // The gates read an unset output as full, so say so rather than
                // print an answer the job will not follow.
                line = format!("full: error: {error}");
            }
        }
        _ => {}
    }
    println!("{line}");
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|path| (*path).to_owned()).collect()
    }

    // --- the allow-list, over path lists ------------------------------------

    #[test]
    fn only_allow_listed_paths_are_prose() {
        let scope = classify(&paths(&[
            "docs/design/decisions-log.md",
            "docs/spikes/g4/notes.txt",
            "AGENTS.md",
            "CLAUDE.md",
            ".claude/settings.json",
            "README.md",
            "CONTRIBUTING.md",
        ]));
        assert!(scope.prose_only(), "{scope:?}");
    }

    #[test]
    fn a_top_level_readme_alone_is_prose() {
        assert!(classify(&paths(&["README.md"])).prose_only());
    }

    #[test]
    fn one_other_path_runs_everything() {
        let scope = classify(&paths(&["docs/README.md", "crates/sim/src/lib.rs"]));
        assert_eq!(
            scope,
            Scope::Full("`crates/sim/src/lib.rs` is not on the prose allow-list".to_owned())
        );
    }

    #[test]
    fn a_markdown_file_below_the_top_level_is_not_prose() {
        for path in [
            "godot/README.md",
            "tests/golden/vista/README.md",
            "xtask/tests/data/README.md",
            "crates/sim/CHANGELOG.md",
        ] {
            assert!(!is_prose(path), "{path}");
            assert!(!classify(&paths(&[path])).prose_only(), "{path}");
        }
    }

    #[test]
    fn the_harness_itself_is_never_prose() {
        for path in [
            "xtask/src/main.rs",
            "xtask/src/scope.rs",
            ".github/workflows/ci.yml",
            "Cargo.toml",
            "Cargo.lock",
            "REUSE.toml",
            "LICENSES/MIT.txt",
            "rules/rules.v1.json",
            "proto/gp/v1/rules.proto",
            "scripts/merge-train.sh",
        ] {
            assert!(!is_prose(path), "{path}");
        }
    }

    #[test]
    fn near_misses_of_the_allow_list_are_not_prose() {
        for path in [
            "",
            "docs",
            "docs/",
            ".claude",
            ".claude/",
            "docsx/a.md",
            "doc/a.md",
            "Docs/a.md",
            "a/docs/b.md",
            ".md",
            "README.MD",
            "README.markdown",
            "README.md.rs",
            "AGENTS.md/x",
            ".github/claude.md",
        ] {
            assert!(!is_prose(path), "`{path}` must not be prose");
        }
    }

    #[test]
    fn an_empty_path_list_runs_everything() {
        assert!(!classify(&[]).prose_only());
    }

    #[test]
    fn nul_separated_output_is_split_unquoted() {
        let output = "docs/caf\u{e9}.md\0docs/a \"b\"\\c.md\0".as_bytes();
        assert_eq!(
            split_paths(output).expect("splits"),
            paths(&["docs/caf\u{e9}.md", "docs/a \"b\"\\c.md"])
        );
        assert!(split_paths(b"docs/\xff.md\0").is_err());
    }

    #[test]
    fn object_names_are_full_lower_case_hex() {
        assert!(is_object_name(&"a".repeat(40)));
        assert!(is_object_name(&"0".repeat(64)));
        assert!(!is_object_name(&"A".repeat(40)));
        assert!(!is_object_name(&"a".repeat(39)));
        assert!(!is_object_name("HEAD"));
        assert!(!is_object_name(""));
    }

    #[test]
    fn the_output_line_is_appended() {
        let dir = env::temp_dir().join(format!(
            "pharmakos-xtask-ci-scope-{}-output",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("github_output");
        fs::write(&file, "earlier=1\n").expect("write");
        write_output(&file, true).expect("appends");
        write_output(&file, false).expect("appends");
        assert_eq!(
            fs::read_to_string(&file).expect("read"),
            "earlier=1\nprose_only=true\nprose_only=false\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    // --- the real command, on repositories built here ------------------------

    /// A throwaway repository under the system's temporary directory, shaped as
    /// `actions/checkout` leaves a pull request: `main` with a base commit, a
    /// branch `pr`, and `git merge --no-ff` of `pr` into `main`, so that HEAD's
    /// first parent is the base branch's tip and its second the pull request's
    /// head.
    struct Repo {
        dir: PathBuf,
    }

    impl Repo {
        /// A repository with a base commit holding one file of each kind the
        /// cases below touch.
        fn new(name: &str) -> Repo {
            let dir = env::temp_dir().join(format!(
                "pharmakos-xtask-ci-scope-{}-{name}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("temp dir");
            let repo = Repo { dir };
            repo.git(&["init", "-q", "-b", "main"]);
            for (path, text) in [
                ("README.md", "readme\n"),
                ("AGENTS.md", "agents\n"),
                ("docs/README.md", "docs\n"),
                ("docs/guide.md", "guide\n"),
                ("crates/x.rs", "fn x() {}\n"),
                ("crates/notes.md", "a note that moves unchanged\n"),
                ("godot/README.md", "godot\n"),
                ("xtask/src/main.rs", "fn main() {}\n"),
                (".github/workflows/ci.yml", "name: ci\n"),
            ] {
                repo.write(path, text);
            }
            repo.commit("base");
            repo
        }

        fn git(&self, args: &[&str]) -> String {
            let mut command = Command::new("git");
            command
                .args([
                    "-c",
                    "user.name=xtask",
                    "-c",
                    "user.email=xtask@invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.autocrlf=false",
                    // No hook of the machine's, global or otherwise, runs here.
                    "-c",
                    "core.hooksPath=.git/no-hooks",
                ])
                .args(args)
                .current_dir(&self.dir)
                .stdin(Stdio::null());
            for name in GIT_REDIRECTS {
                command.env_remove(name);
            }
            let output = command.output().expect("git runs");
            assert!(
                output.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout)
                .expect("utf-8")
                .trim()
                .to_owned()
        }

        fn write(&self, path: &str, text: &str) {
            let full = self.dir.join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).expect("parent dir");
            }
            fs::write(full, text).expect("write");
        }

        fn commit(&self, message: &str) {
            self.git(&["add", "-A"]);
            self.git(&[
                "commit",
                "-q",
                "--no-verify",
                "--allow-empty",
                "-m",
                message,
            ]);
        }

        /// Branches `pr` off `main`, lets `on_branch` change it, commits, lets
        /// `on_main` change `main` after the branch point (committing only when
        /// it did), merges `pr` into `main` with `--no-ff`, and returns the pull
        /// request's head.
        fn pull_request(
            &self,
            on_branch: impl Fn(&Repo),
            on_main: impl Fn(&Repo) -> bool,
        ) -> String {
            self.git(&["switch", "-q", "-c", "pr"]);
            on_branch(self);
            self.commit("the pull request");
            let head = self.git(&["rev-parse", "HEAD"]);
            self.git(&["switch", "-q", "main"]);
            if on_main(self) {
                self.commit("main moves on");
            }
            self.git(&["merge", "-q", "--no-ff", "--no-verify", "-m", "merge", "pr"]);
            head
        }

        fn decide(&self, head: &str) -> Scope {
            decide(Some(PULL_REQUEST), Some(head), &self.dir)
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn main_stays(_: &Repo) -> bool {
        false
    }

    #[test]
    fn a_pull_request_of_allow_listed_paths_is_prose() {
        let repo = Repo::new("allow-listed");
        let head = repo.pull_request(
            |repo| {
                repo.write("docs/guide.md", "guide, revised\n");
                repo.write("docs/new/page.md", "new\n");
                repo.write("AGENTS.md", "agents, revised\n");
                repo.write("CLAUDE.md", "claude\n");
                repo.write(".claude/settings.json", "{}\n");
            },
            main_stays,
        );
        let scope = repo.decide(&head);
        assert_eq!(
            scope,
            Scope::Prose(
                "5 paths changed, all on the prose allow-list \
                 (docs/**, AGENTS.md, CLAUDE.md, .claude/**, top-level *.md)"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_top_level_readme_is_prose() {
        let repo = Repo::new("readme");
        let head = repo.pull_request(
            |repo| repo.write("README.md", "readme, revised\n"),
            main_stays,
        );
        assert!(repo.decide(&head).prose_only());
    }

    #[test]
    fn a_deletion_inside_docs_is_prose() {
        let repo = Repo::new("delete-docs");
        let head = repo.pull_request(|repo| repo.git_rm("docs/guide.md"), main_stays);
        assert!(repo.decide(&head).prose_only());
    }

    #[test]
    fn a_non_ascii_name_in_docs_alone_is_prose() {
        // Without `-z`, git prints this name quoted with octal escapes
        // ("docs/caf\303\251.md"), which starts with a quote and is not prose.
        let repo = Repo::new("non-ascii");
        let head = repo.pull_request(
            |repo| repo.write("docs/caf\u{e9}.md", "caf\u{e9}\n"),
            main_stays,
        );
        assert!(repo.decide(&head).prose_only(), "{:?}", repo.decide(&head));
    }

    #[test]
    fn one_other_path_is_full() {
        let repo = Repo::new("other-path");
        let head = repo.pull_request(
            |repo| {
                repo.write("docs/guide.md", "guide, revised\n");
                repo.write("crates/x.rs", "fn x() { }\n");
            },
            main_stays,
        );
        assert_eq!(
            repo.decide(&head),
            Scope::Full("`crates/x.rs` is not on the prose allow-list".to_owned())
        );
    }

    #[test]
    fn a_readme_below_the_top_level_is_full() {
        let repo = Repo::new("godot-readme");
        let head = repo.pull_request(
            |repo| repo.write("godot/README.md", "godot, revised\n"),
            main_stays,
        );
        assert!(!repo.decide(&head).prose_only());
    }

    #[test]
    fn a_rename_into_docs_is_full() {
        // The file moves unchanged, so rename detection would pair the two sides
        // and `--name-only` would print only `docs/notes.md`, which is prose.
        let repo = Repo::new("rename");
        let head = repo.pull_request(
            |repo| repo.git_ok(&["mv", "crates/notes.md", "docs/notes.md"]),
            main_stays,
        );
        assert_eq!(
            repo.decide(&head),
            Scope::Full("`crates/notes.md` is not on the prose allow-list".to_owned())
        );
    }

    #[test]
    fn a_deletion_outside_the_list_is_full() {
        let repo = Repo::new("delete-other");
        let head = repo.pull_request(|repo| repo.git_rm("crates/x.rs"), main_stays);
        assert!(!repo.decide(&head).prose_only());
    }

    #[test]
    fn a_change_to_the_harness_is_full() {
        for (name, path) in [
            ("xtask", "xtask/src/main.rs"),
            ("workflows", ".github/workflows/ci.yml"),
        ] {
            let repo = Repo::new(name);
            let head = repo.pull_request(
                |repo| {
                    repo.write("docs/guide.md", "guide, revised\n");
                    repo.write(path, "changed\n");
                },
                main_stays,
            );
            assert_eq!(
                repo.decide(&head),
                Scope::Full(format!("`{path}` is not on the prose allow-list"))
            );
        }
    }

    #[test]
    fn an_empty_diff_is_full() {
        let repo = Repo::new("empty");
        let head = repo.pull_request(|_| {}, main_stays);
        assert_eq!(
            repo.decide(&head),
            Scope::Full("the pull request's merge commit changes no path".to_owned())
        );
    }

    #[test]
    fn main_moving_in_docs_does_not_hide_a_code_change() {
        // HEAD^2..HEAD would see only main's docs commit and say prose.
        let repo = Repo::new("main-docs");
        let head = repo.pull_request(
            |repo| repo.write("crates/x.rs", "fn x() { }\n"),
            |repo| {
                repo.write("docs/guide.md", "guide, from main\n");
                true
            },
        );
        assert_eq!(
            repo.decide(&head),
            Scope::Full("`crates/x.rs` is not on the prose allow-list".to_owned())
        );
    }

    #[test]
    fn main_moving_in_code_does_not_block_a_docs_change() {
        // A merge-base diff would count main's crates/ commit and say full.
        let repo = Repo::new("main-code");
        let head = repo.pull_request(
            |repo| repo.write("docs/guide.md", "guide, revised\n"),
            |repo| {
                repo.write("crates/x.rs", "fn x() { }\n");
                true
            },
        );
        assert!(repo.decide(&head).prose_only(), "{:?}", repo.decide(&head));
    }

    #[test]
    fn an_event_other_than_pull_request_is_full() {
        let repo = Repo::new("event");
        let head = repo.pull_request(|repo| repo.write("docs/guide.md", "revised\n"), main_stays);
        for event in [
            "push",
            "workflow_dispatch",
            "pull_request_target",
            "merge_group",
            "",
        ] {
            let scope = decide(Some(event), Some(&head), &repo.dir);
            assert!(!scope.prose_only(), "{event}: {scope:?}");
        }
        assert!(!decide(None, Some(&head), &repo.dir).prose_only());
    }

    #[test]
    fn a_head_with_one_parent_is_full() {
        let repo = Repo::new("one-parent");
        repo.write("docs/guide.md", "revised\n");
        repo.commit("straight onto main");
        let head = repo.git(&["rev-parse", "HEAD"]);
        let parent = repo.git(&["rev-parse", "HEAD^1"]);
        for claimed in [&head, &parent] {
            assert_eq!(
                repo.decide(claimed),
                Scope::Full(
                    "HEAD has 1 parent(s), not the two of a pull request's merge commit".to_owned()
                )
            );
        }
    }

    #[test]
    fn a_second_parent_that_is_not_the_head_is_full() {
        let repo = Repo::new("wrong-head");
        let head = repo.pull_request(|repo| repo.write("docs/guide.md", "revised\n"), main_stays);
        assert!(repo.decide(&head).prose_only());
        let base = repo.git(&["rev-parse", "HEAD^1"]);
        let merge = repo.git(&["rev-parse", "HEAD"]);
        for claimed in [base.as_str(), merge.as_str(), &"0".repeat(40)] {
            assert!(!repo.decide(claimed).prose_only(), "{claimed}");
        }
        // The parent order matters: a merge made the other way round, with the
        // pull request's head first, is not a pull request's merge commit.
        let reversed = Repo::new("reversed");
        reversed.git(&["switch", "-q", "-c", "pr"]);
        reversed.write("docs/guide.md", "revised\n");
        reversed.commit("the pull request");
        let pr_head = reversed.git(&["rev-parse", "HEAD"]);
        reversed.git(&["switch", "-q", "main"]);
        reversed.write("docs/README.md", "from main\n");
        reversed.commit("main moves on");
        reversed.git(&["switch", "-q", "pr"]);
        reversed.git(&[
            "merge",
            "-q",
            "--no-ff",
            "--no-verify",
            "-m",
            "merge",
            "main",
        ]);
        assert_eq!(reversed.git(&["rev-parse", "HEAD^1"]), pr_head);
        assert!(!reversed.decide(&pr_head).prose_only());
    }

    #[test]
    fn a_missing_or_malformed_head_is_full() {
        let repo = Repo::new("bad-head");
        let head = repo.pull_request(|repo| repo.write("docs/guide.md", "revised\n"), main_stays);
        assert!(repo.decide(&head).prose_only());
        let upper = head.to_uppercase();
        let short = head.get(..12);
        for claimed in [
            None,
            Some(""),
            Some("HEAD"),
            Some("pr"),
            Some(upper.as_str()),
            short,
        ] {
            let scope = decide(Some(PULL_REQUEST), claimed, &repo.dir);
            assert!(!scope.prose_only(), "{claimed:?}: {scope:?}");
        }
    }

    #[test]
    fn a_git_error_is_full() {
        // A repository with no commit: HEAD does not resolve.
        let unborn = Repo::new_empty("unborn");
        let scope = decide(Some(PULL_REQUEST), Some(&"a".repeat(40)), &unborn.dir);
        assert!(
            matches!(&scope, Scope::Full(reason) if reason.starts_with("error: ")),
            "{scope:?}"
        );
        // A directory that does not exist: git cannot even start there.
        let missing = env::temp_dir().join(format!(
            "pharmakos-xtask-ci-scope-{}-does-not-exist",
            std::process::id()
        ));
        let scope = decide(Some(PULL_REQUEST), Some(&"a".repeat(40)), &missing);
        assert!(
            matches!(&scope, Scope::Full(reason) if reason.starts_with("error: ")),
            "{scope:?}"
        );
    }

    impl Repo {
        /// A repository with no commit at all.
        fn new_empty(name: &str) -> Repo {
            let dir = env::temp_dir().join(format!(
                "pharmakos-xtask-ci-scope-{}-{name}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("temp dir");
            let repo = Repo { dir };
            repo.git(&["init", "-q", "-b", "main"]);
            repo
        }

        fn git_ok(&self, args: &[&str]) {
            self.git(args);
        }

        fn git_rm(&self, path: &str) {
            self.git(&["rm", "-q", path]);
        }
    }
}

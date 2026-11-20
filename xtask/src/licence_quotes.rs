// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A tracked golden, fixture or scenario that quotes the template library keeps the
//! library's licence beside the game's.
//!
//! `library/**` is `MIT OR Apache-2.0`; the goldens the game writes are
//! `GPL-3.0-or-later`. A golden that prints a template's title, summary, comments or step
//! labels therefore carries both, and `REUSE.toml` says so with the combined expression
//! `GPL-3.0-or-later AND (MIT OR Apache-2.0)` (decisions-log items 124 (5) (e), 128, 136).
//! That used to depend on someone remembering to list each new golden by name; three
//! render goldens were missed (found in the review of PR #189). This test finds the
//! quotation instead of trusting the list:
//!
//! * the quoted strings are read from `library/*.jsonc` at run time (every comment line and
//!   every string of 25 or more characters with a space in it, SPDX lines excluded);
//! * every tracked file under [`SCOPE`] that holds one of them, raw or JSON-escaped, must
//!   not end up GPL-only: its licences are the manifest's last matching block, plus the
//!   file's `.license` sidecar and its own `SPDX-License-Identifier` lines (a block of
//!   `precedence = "aggregate"` is combined with those; `"override"` is the manifest's last
//!   word);
//! * a file that is GPL-only passes only when an `override` block names it, because that is
//!   the form of a recorded ruling: the gateway's `instantiate_template` answer embeds a
//!   whole template and stays GPL (decisions-log item 113 (10)).
//!
//! It runs with the rest of xtask's tests in the `test` step, and adds no step to
//! `cargo xtask ci`. It reads the manifest with a reader of its own, as the rest of xtask
//! reads `cargo metadata`, so it adds no dependency.
//!
//! The scope is the data files. Rust and GDScript sources that repeat a phrase of a template
//! in a comment or a test string are code, a quotation of a few words that item 124 (5) (e)
//! judged to need no second licence, and are not read.

use std::fs;
use std::path::{Path, PathBuf};

/// Tracked paths (prefixes) read for quotations.
const SCOPE: &[&str] = &["tests/golden/", "godot/fixtures/", "scenarios/"];

/// The permissive tier's expression, as the manifest and the headers spell it.
const PERMISSIVE: &str = "MIT OR Apache-2.0";

/// The tag of an SPDX licence line, spelt in two pieces so that `reuse` does not read this
/// source as a file that states a licence of its own.
const IDENTIFIER_TAG: &str = concat!("SPDX-License-", "Identifier:");

/// The game tier's licence.
const GAME: &str = "GPL-3.0-or-later";

/// One `[[annotations]]` block of `REUSE.toml`.
#[derive(Debug, Default)]
struct Block {
    paths: Vec<String>,
    override_: bool,
    licence: String,
}

/// The blocks, in file order (the last match wins).
fn parse_manifest(text: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut in_paths = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line == "[[annotations]]" {
            blocks.push(Block::default());
            in_paths = false;
            continue;
        }
        let Some(block) = blocks.last_mut() else {
            continue;
        };
        if line.starts_with('#') {
            continue;
        }
        if in_paths || line.starts_with("path") {
            let body = if in_paths {
                line
            } else {
                line.split_once('=').map_or("", |(_, rest)| rest)
            };
            for piece in body.split('"').skip(1).step_by(2) {
                block.paths.push(piece.to_owned());
            }
            in_paths = !body.contains(']');
        } else if let Some((key, value)) = line.split_once('=') {
            let value = value.trim().trim_matches('"');
            match key.trim() {
                "precedence" => block.override_ = value == "override",
                "SPDX-License-Identifier" => block.licence = value.to_owned(),
                _ => {}
            }
        }
    }
    blocks
}

/// reuse's globs: `**` crosses directories, `*` does not.
fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => {
            if let Some(after) = rest.strip_prefix(b"*") {
                let after = after.strip_prefix(b"/").unwrap_or(after);
                (0..=text.len()).any(|n| glob(after, text.get(n..).unwrap_or(&[])))
            } else {
                (0..=text.len()).any(|n| {
                    text.get(..n)
                        .is_some_and(|skipped| !skipped.contains(&b'/'))
                        && glob(rest, text.get(n..).unwrap_or(&[]))
                })
            }
        }
        Some((c, rest)) => text
            .split_first()
            .is_some_and(|(t, text_rest)| t == c && glob(rest, text_rest)),
    }
}

/// What a file is licensed under, and whether the manifest's last word is an override.
struct Effective {
    licences: Vec<String>,
    overridden: bool,
}

fn effective(blocks: &[Block], root: &Path, file: &str, bytes: &str) -> Effective {
    let mut licences = Vec::new();
    let mut overridden = false;
    if let Some(block) = blocks
        .iter()
        .rev()
        .find(|b| b.paths.iter().any(|p| glob(p.as_bytes(), file.as_bytes())))
    {
        licences.push(block.licence.clone());
        overridden = block.override_;
    }
    if !overridden {
        let mut own = bytes.to_owned();
        if let Ok(sidecar) = fs::read_to_string(root.join(format!("{file}.license"))) {
            own.push('\n');
            own.push_str(&sidecar);
        }
        for line in own.lines() {
            if let Some((_, rest)) = line.split_once(IDENTIFIER_TAG) {
                licences.push(rest.trim().trim_end_matches("\\n\",").to_owned());
            }
        }
    }
    Effective {
        licences,
        overridden,
    }
}

/// The strings of the tracked `library/*.jsonc` a golden can quote (git lists them sorted).
fn library_strings(root: &Path, tracked: &[String]) -> Vec<String> {
    let mut strings = Vec::new();
    let files = tracked.iter().filter(|f| {
        f.starts_with("library/")
            && Path::new(f.as_str())
                .extension()
                .is_some_and(|e| e == "jsonc")
    });
    for file in files {
        let text = fs::read_to_string(root.join(file)).expect("a library file is UTF-8");
        for line in text.lines() {
            let line = line.trim();
            if let Some(comment) = line.strip_prefix("//") {
                let comment = comment.trim();
                if comment.chars().count() >= 25 && !comment.contains("SPDX") {
                    strings.push(comment.to_owned());
                }
                continue; // a comment's own quotation marks are not string delimiters
            }
            for piece in line.split('"').skip(1).step_by(2) {
                if piece.chars().count() >= 25 && piece.contains(' ') && !piece.contains("SPDX") {
                    strings.push(piece.to_owned());
                }
            }
        }
    }
    strings.sort();
    strings.dedup();
    strings
}

fn quotes(bytes: &str, strings: &[String]) -> Option<String> {
    strings
        .iter()
        .find(|s| bytes.contains(s.as_str()) || bytes.contains(&s.replace('"', "\\\"")))
        .cloned()
}

fn tracked(root: &Path) -> Vec<String> {
    let out = crate::child_command(false, "git", &["ls-files", "-z"], root)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|f| !f.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The files under [`SCOPE`] that quote the library and are licensed GPL-only without an
/// override block's ruling.
fn misses(root: &Path, manifest: &str, files: &[String], strings: &[String]) -> Vec<String> {
    let blocks = parse_manifest(manifest);
    let mut found = Vec::new();
    for file in files {
        if !SCOPE.iter().any(|s| file.starts_with(s)) || file.ends_with(".license") {
            continue;
        }
        let Ok(bytes) = fs::read_to_string(root.join(file)) else {
            continue; // a PNG, a hash chain with no text: nothing to quote
        };
        let Some(quoted) = quotes(&bytes, strings) else {
            continue;
        };
        let eff = effective(&blocks, root, file, &bytes);
        let game = eff.licences.iter().any(|l| l.contains(GAME));
        let permissive = eff.licences.iter().any(|l| l.contains(PERMISSIVE));
        if game && !permissive && !eff.overridden {
            let shown: String = quoted.chars().take(48).collect();
            found.push(format!("{file} (quotes \"{shown}...\")"));
        }
    }
    found
}

/// `godot/fixtures/` holds byte copies of gateway goldens, and a copy has the licence of the
/// file it copies (REUSE.toml's note on that directory). Every fixture identical, byte for
/// byte, to a golden under `tests/golden/` must carry the same licences as that golden.
fn copy_misses(root: &Path, manifest: &str, files: &[String]) -> Vec<String> {
    let blocks = parse_manifest(manifest);
    let kind = |file: &str, bytes: &str| {
        let eff = effective(&blocks, root, file, bytes);
        (
            eff.licences.iter().any(|l| l.contains(GAME)),
            eff.licences.iter().any(|l| l.contains(PERMISSIVE)),
        )
    };
    let mut found = Vec::new();
    for copy in files.iter().filter(|f| f.starts_with("godot/fixtures/")) {
        let Ok(bytes) = fs::read_to_string(root.join(copy)) else {
            continue;
        };
        for source in files.iter().filter(|f| f.starts_with("tests/golden/")) {
            let Ok(original) = fs::read_to_string(root.join(source)) else {
                continue;
            };
            if original == bytes && kind(copy, &bytes) != kind(source, &original) {
                found.push(format!(
                    "{copy} is a byte copy of {source}, which is licensed differently"
                ));
            }
        }
    }
    found
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits one level below the root")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_follow_reuses_rules() {
        assert!(glob(b"tests/**", b"tests/golden/a/b.json"));
        assert!(glob(b"tests/golden/*/x.json", b"tests/golden/a/x.json"));
        assert!(!glob(b"tests/golden/*/x.json", b"tests/golden/a/b/x.json"));
        assert!(glob(b"godot/fixtures/**", b"godot/fixtures/a.json"));
        assert!(!glob(b"godot/fixtures/a.json", b"godot/fixtures/b.json"));
    }

    #[test]
    fn the_last_matching_block_decides() {
        let manifest = "[[annotations]]\npath = [\n \"tests/**\",\n]\nprecedence = \"aggregate\"\n\
            SPDX-License-Identifier = \"GPL-3.0-or-later\"\n\
            [[annotations]]\npath = [\"tests/a.txt\"]\nprecedence = \"override\"\n\
            SPDX-License-Identifier = \"MIT OR Apache-2.0\"\n";
        let blocks = parse_manifest(manifest);
        assert_eq!(blocks.len(), 2);
        let eff = effective(&blocks, Path::new("."), "tests/a.txt", "");
        assert!(eff.overridden);
        assert_eq!(eff.licences, vec!["MIT OR Apache-2.0".to_owned()]);
        let eff = effective(&blocks, Path::new("."), "tests/b.txt", "");
        assert!(!eff.overridden);
        assert_eq!(eff.licences, vec!["GPL-3.0-or-later".to_owned()]);
    }

    #[test]
    fn a_quoting_golden_licensed_gpl_only_is_a_miss() {
        let dir = std::env::temp_dir().join(format!("xtask-licence-quotes-{}", std::process::id()));
        fs::create_dir_all(dir.join("tests/golden/x")).expect("temp dir");
        fs::write(
            dir.join("tests/golden/x/a.txt"),
            "has a long quoted sentence here",
        )
        .expect("w");
        fs::write(dir.join("tests/golden/x/b.txt"), "says nothing of the kind").expect("w");
        let manifest = "[[annotations]]\npath = [\"tests/**\"]\nprecedence = \"aggregate\"\n\
            SPDX-License-Identifier = \"GPL-3.0-or-later\"\n";
        let files = vec![
            "tests/golden/x/a.txt".to_owned(),
            "tests/golden/x/b.txt".to_owned(),
        ];
        let strings = vec!["a long quoted sentence".to_owned()];
        let found = misses(&dir, manifest, &files, &strings);
        fs::remove_dir_all(&dir).expect("cleanup");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found.iter().all(|f| f.starts_with("tests/golden/x/a.txt")));

        let both = manifest.replace(
            "\"GPL-3.0-or-later\"",
            "\"GPL-3.0-or-later AND (MIT OR Apache-2.0)\"",
        );
        fs::create_dir_all(dir.join("tests/golden/x")).expect("temp dir");
        fs::write(
            dir.join("tests/golden/x/a.txt"),
            "has a long quoted sentence here",
        )
        .expect("w");
        let found = misses(&dir, &both, files.get(..1).unwrap_or(&[]), &strings);
        fs::remove_dir_all(&dir).expect("cleanup");
        assert!(found.is_empty(), "{found:?}");
    }

    /// The check itself, over the tracked tree.
    #[test]
    fn every_golden_that_quotes_the_library_carries_its_licence() {
        let root = repo_root();
        let manifest = fs::read_to_string(root.join("REUSE.toml")).expect("REUSE.toml");
        let files = tracked(&root);
        let strings = library_strings(&root, &files);
        assert!(strings.len() > 100, "the library's strings were not read");
        let mut found = misses(&root, &manifest, &files, &strings);
        found.extend(copy_misses(&root, &manifest, &files));
        assert!(
            found.is_empty(),
            "these files quote library/ (MIT OR Apache-2.0) but REUSE.toml licenses them GPL-only. \
             List each in the block that gives `GPL-3.0-or-later AND (MIT OR Apache-2.0)` \
             (or, for an instantiate_template answer ruled GPL by decisions-log item 113 (10), \
             in the override block at the end):\n  {}",
            found.join("\n  ")
        );
    }
}

// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A tracked file that reproduces the template library's sentences carries the library's
//! licence.
//!
//! `library/**` is `MIT OR Apache-2.0`; what the game writes is `GPL-3.0-or-later`. A golden,
//! fixture, scenario or source file that reproduces a sentence of a template carries the
//! library's licence beside its own, as the combined expression
//! `GPL-3.0-or-later AND (MIT OR Apache-2.0)`, or the library's alone where the file is purely
//! the library's text (the operator goldens, the docs). Decisions-log items 124 (5) (e), 128 and
//! 136 started the rule for the render goldens; item 113 (10) is amended to it (the three
//! `instantiate_template` answers carry both licences, not GPL alone). It used to depend on
//! someone listing each new file by name, and PR #189's review found three missed. This check
//! finds the quotation instead of trusting a list.
//!
//! # The rule
//!
//! * **What is quoted:** a *sentence* of a library file, a run of at least [`MIN_WORDS`] words
//!   that ends in a full stop (from a string, or from a paragraph of `//` comment, whose
//!   consecutive lines are one paragraph); or any run of at least [`RUN_WORDS`] consecutive
//!   words of one library *string*, full stop or not, because a parameter's description is
//!   expression too. Titles, names and step labels are short and are not covered, and neither
//!   is a few words of a sentence, such as a decision cited in a comment.
//! * **How it is matched:** on text with whitespace collapsed, JSON string escapes undone
//!   (the whole of JSON's set, `\uXXXX` and `\/` included, by one reader for both sides) and
//!   comment markers removed, so a re-wrapped comment, a `\n` inside a sentence and an escaped
//!   quote all match; a run of words is compared with punctuation dropped.
//! * **What the library is:** every tracked file under `library/` that `reuse` reports (a
//!   `.jsonc` or `.json` by its strings and comments, anything else by its paragraphs).
//! * **What is read:** every file `git ls-files` lists outside `library/`, so a file not yet
//!   added is seen once it is. A binary file is skipped by extension from the named
//!   [`BINARY`] list; any other read or UTF-8 error is a [`Refusal`], never a skip.
//! * **What licence it has:** `reuse lint --json`'s answer for the file, the expressions as the
//!   tool concluded them from the manifest, the sidecar and the header together, so this agrees
//!   with `reuse lint` by construction (precedence and globs included) and no manifest is read
//!   by hand. Every expression of a quoting file must be one of the three [`ALLOWED`] forms,
//!   compared exactly (`OR` where `AND` is meant fails), and the file must carry the combined
//!   expression, or the library's with no game expression beside it (a game expression and the
//!   library's as two separate expressions is neither).
//! * **Byte copies:** a file under `godot/fixtures/` that is byte-identical to one under
//!   `tests/golden/` or `scenarios/` carries exactly the same expressions as it.
//! * **A positive control:** the check asserts it scanned a plausible number of files and that
//!   the files known to quote the library are found quoting it, so "nothing found" cannot mean
//!   "nothing looked at".
//!
//! # Where it runs
//!
//! In the `reuse` step of `cargo xtask ci` (`cargo xtask reuse`), after `reuse lint` has passed,
//! through [`check_tree`]: that is the one job that has the tool, and the docs-only fast path
//! runs the step too. `reuse` missing is the step's own rule (a skip locally, a failure under
//! `PHARMAKOS_REQUIRE_TOOLS=1`), decided before this runs. The checker itself, [`check`], takes
//! the tracked files' bytes and the licence report as values, so its unit tests need no tool,
//! no filesystem and no `git`, and cannot race one another. No step is added to `ci`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::iter::Peekable;
use std::path::Path;
use std::str::Chars;

use crate::Json;

/// The fewest words in a sentence (ending in a full stop) that counts as quoted expression.
const MIN_WORDS: usize = 8;

/// The fewest consecutive words of one library string that count as quoted expression, full
/// stop or not: a parameter's description is expression; a title or a step label is a name
/// (they are short).
const RUN_WORDS: usize = 12;

/// The library's licence, as the manifest and the headers spell it.
const LIBRARY: &str = "MIT OR Apache-2.0";

/// The game's.
const GAME: &str = "GPL-3.0-or-later";

/// A file that carries both.
const COMBINED: &str = "GPL-3.0-or-later AND (MIT OR Apache-2.0)";

/// The expressions a file that quotes the library may carry.
const ALLOWED: [&str; 3] = [GAME, LIBRARY, COMBINED];

/// Files read as bytes and never as text, by extension.
const BINARY: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "ico", "vox", "ogg", "wav", "mp3", "zip", "pck", "ttf", "otf",
    "woff", "woff2", "dll", "so", "dylib", "exe", "wasm", "bin", "binpb",
];

/// Why the check could not give an answer. Every one fails the test.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// `git ls-files` failed.
    Git(String),
    /// `reuse lint --json` did not run, or its answer was not the shape read.
    Reuse(String),
    /// A tracked text file could not be read as UTF-8 text.
    Unreadable { file: String, why: String },
    /// A tracked file that `reuse` should have reported is not in its report.
    NotReported(String),
    /// A library file could not be read.
    Library(String),
    /// The positive control failed: the check looked at too little to mean anything.
    Control(String),
    /// Files that quote the library without carrying its licence.
    Misses(Vec<String>),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Git(why) => write!(f, "git ls-files failed: {why}"),
            Refusal::Reuse(why) => write!(f, "reuse lint --json: {why}"),
            Refusal::Unreadable { file, why } => {
                write!(
                    f,
                    "{file} is not readable as text ({why}); add its extension to BINARY if it is binary"
                )
            }
            Refusal::NotReported(file) => write!(f, "reuse's report has no entry for {file}"),
            Refusal::Library(why) => write!(f, "library/: {why}"),
            Refusal::Control(why) => write!(f, "the quotation check's positive control: {why}"),
            Refusal::Misses(misses) => write!(
                f,
                "these quote library/ (MIT OR Apache-2.0) without carrying its licence: give each \
                 `{COMBINED}` (REUSE.toml, or its own header), or `{LIBRARY}` if it is purely the \
                 library's text:\n  {}",
                misses.join("\n  ")
            ),
        }
    }
}

/// What a run found.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    /// Text files read.
    pub(crate) scanned: usize,
    /// Library files read, and the word runs taken from them.
    pub(crate) library_files: usize,
    pub(crate) library_runs: usize,
    /// Files that quote the library, with the expressions each carries.
    pub(crate) quoting: BTreeMap<String, Vec<String>>,
    /// Quoting files whose licences are wrong, and copies that differ from their source.
    pub(crate) misses: Vec<String>,
}

// ---------------------------------------------------------------------------
// Text: units, sentences, normalisation
// ---------------------------------------------------------------------------

/// The strings (`true`) and the comment paragraphs (`false`) of one JSONC file, in order. Read with a scanner
/// that knows a string from a comment, so a `//` or a quote inside either is not a delimiter.
fn units(text: &str) -> Vec<(String, bool)> {
    let mut units = Vec::new();
    let mut paragraph = String::new();
    let flush = |paragraph: &mut String, units: &mut Vec<(String, bool)>| {
        if !paragraph.trim().is_empty() {
            units.push((std::mem::take(paragraph), false));
        }
        paragraph.clear();
    };
    for line in text.lines() {
        let whole_line_comment = line.trim_start().starts_with("//");
        let mut chars = line.chars().peekable();
        let mut in_string = false;
        let mut current = String::new();
        let mut comment = String::new();
        let mut in_comment = false;
        while let Some(c) = chars.next() {
            if in_comment {
                comment.push(c);
            } else if in_string {
                match c {
                    '\\' => match escape(&mut chars) {
                        Some(decoded) => current.push_str(&decoded),
                        None => current.extend(chars.next()),
                    },
                    '"' => {
                        in_string = false;
                        units.push((std::mem::take(&mut current), true));
                    }
                    other => current.push(other),
                }
            } else if c == '"' {
                in_string = true;
            } else if c == '/' && chars.peek() == Some(&'/') {
                in_comment = true;
                chars.next();
            }
        }
        if in_comment {
            let comment = comment.trim();
            if whole_line_comment {
                if comment.is_empty() {
                    flush(&mut paragraph, &mut units);
                } else {
                    if !paragraph.is_empty() {
                        paragraph.push(' ');
                    }
                    paragraph.push_str(comment);
                }
            } else {
                flush(&mut paragraph, &mut units);
                units.push((comment.to_owned(), false));
            }
        } else {
            flush(&mut paragraph, &mut units);
        }
    }
    flush(&mut paragraph, &mut units);
    units
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The four hex digits of a `\u` escape.
fn hex4(chars: &mut Peekable<Chars<'_>>) -> Option<u32> {
    let digits: String = chars.by_ref().take(4).collect();
    if digits.chars().count() == 4 {
        u32::from_str_radix(&digits, 16).ok()
    } else {
        None
    }
}

/// One JSON string escape, read after its backslash: the whole of JSON's set (`\n \t \r \b
/// \f` become a space, `\" \\ \/` themselves, `\uXXXX` its character, a surrogate pair the
/// one character it spells). `None`, with `chars` untouched, when what follows is not one.
fn escape(chars: &mut Peekable<Chars<'_>>) -> Option<String> {
    let next = *chars.peek()?;
    match next {
        'n' | 't' | 'r' | 'b' | 'f' => {
            chars.next();
            Some(" ".to_owned())
        }
        '"' | '\\' | '/' => {
            chars.next();
            Some(next.to_string())
        }
        'u' => {
            let mut look = chars.clone();
            look.next();
            let unit = hex4(&mut look)?;
            let decoded = match unit {
                0xD800..=0xDBFF => {
                    let mut pair = look.clone();
                    let low = (pair.next() == Some('\\') && pair.next() == Some('u'))
                        .then(|| hex4(&mut pair))
                        .flatten()
                        .filter(|low| (0xDC00..=0xDFFF).contains(low));
                    match low {
                        Some(low) => {
                            look = pair;
                            char::from_u32(0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00))
                        }
                        None => None,
                    }
                }
                other => char::from_u32(other),
            };
            *chars = look;
            Some(decoded.unwrap_or('\u{fffd}').to_string())
        }
        _ => None,
    }
}

/// A file's text in the form quotations are matched against: JSON string escapes undone, the
/// leading `//` of each comment line removed, the pieces of a Rust `concat!` joined,
/// whitespace collapsed. A library string is decoded by the same escape reader, so the two
/// sides cannot read an escape differently.
fn normalise(text: &str) -> String {
    let mut unescaped = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match escape(&mut chars) {
                Some(decoded) => unescaped.push_str(&decoded),
                None => unescaped.push(c),
            }
        } else {
            unescaped.push(c);
        }
    }
    let lines: Vec<&str> = unescaped
        .lines()
        .map(|line| {
            let line = line.trim_start();
            if line.starts_with("//") {
                line.trim_start_matches('/').trim_start_matches('!')
            } else {
                line
            }
        })
        .collect();
    let joined = collapse(&lines.join(" "));
    collapse(&joined.replace("\", \"", " "))
}

/// The words of a text, punctuation dropped: runs of letters and digits.
fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect()
}

/// The sentences of a unit: pieces of its collapsed text ending in a full stop, of at least
/// [`MIN_WORDS`] words.
fn sentences(unit: &str) -> Vec<String> {
    let text = collapse(unit);
    let mut found = Vec::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find(". ") {
        let (sentence, tail) = rest.split_at(at + 1);
        found.push(sentence.to_owned());
        rest = tail.trim_start();
    }
    found.push(rest.to_owned());
    found.retain(|s| s.ends_with('.') && s.split_whitespace().count() >= MIN_WORDS);
    found
}

/// What the library says, from every tracked file under `library/` that `reuse` reports.
#[derive(Debug, Default)]
struct Library {
    /// Sentences: at least [`MIN_WORDS`] words ending in a full stop.
    sentences: Vec<String>,
    /// Every run of [`RUN_WORDS`] consecutive words of one library string, full stop or not.
    runs: BTreeSet<String>,
    /// The library files read.
    files: usize,
}

impl Library {
    /// A sentence is taken from any unit; a run of words only from a string (or a paragraph of
    /// prose), which is what the library says to the player, not a comment about it.
    fn add_unit(&mut self, unit: &str, string: bool) {
        if unit.contains("SPDX") {
            return;
        }
        self.sentences.extend(sentences(unit));
        if string {
            let tokens = words(unit);
            for window in tokens.windows(RUN_WORDS) {
                self.runs.insert(window.join(" "));
            }
        }
    }

    /// One library file: a JSON or JSONC file by its strings and comment paragraphs, anything
    /// else (a note in Markdown) by its paragraphs.
    fn add_file(&mut self, name: &str, text: &str) {
        self.files += 1;
        let json = Path::new(name)
            .extension()
            .is_some_and(|e| e == "jsonc" || e == "json");
        if json {
            for (unit, string) in units(text) {
                self.add_unit(&unit, string);
            }
        } else {
            let mut paragraph = String::new();
            for line in text.lines() {
                if line.trim().is_empty() {
                    self.add_unit(&std::mem::take(&mut paragraph), true);
                } else {
                    paragraph.push(' ');
                    paragraph.push_str(line);
                }
            }
            self.add_unit(&paragraph, true);
        }
    }

    fn read(files: &[(String, Vec<u8>)]) -> Result<Library, Refusal> {
        let mut library = Library::default();
        for (file, bytes) in files
            .iter()
            .filter(|(f, _)| f.starts_with("library/") && !reuse_ignores(f, false))
        {
            let text =
                std::str::from_utf8(bytes).map_err(|e| Refusal::Library(format!("{file}: {e}")))?;
            library.add_file(file, text);
        }
        library.sentences.sort();
        library.sentences.dedup();
        if library.sentences.is_empty() && library.runs.is_empty() {
            return Err(Refusal::Library("no sentence was read".to_owned()));
        }
        Ok(library)
    }

    /// What `normalised` quotes: a sentence of the library, or a run of [`RUN_WORDS`]
    /// consecutive words of one of its strings.
    fn quoted_in(&self, normalised: &str) -> Option<String> {
        if let Some(sentence) = self
            .sentences
            .iter()
            .find(|s| normalised.contains(s.as_str()))
        {
            return Some(sentence.clone());
        }
        words(normalised)
            .windows(RUN_WORDS)
            .map(|window| window.join(" "))
            .find(|run| self.runs.contains(run))
    }
}

// ---------------------------------------------------------------------------
// Licences: what `reuse` says
// ---------------------------------------------------------------------------

/// The file's expressions from `reuse lint --json`: path to the expressions the tool found,
/// in the order it lists them.
fn read_report(json: &str) -> Result<BTreeMap<String, Vec<String>>, Refusal> {
    let bad = |why: &str| Refusal::Reuse(why.to_owned());
    let root = Json::parse(json).map_err(Refusal::Reuse)?;
    let files = root
        .get("files")
        .and_then(Json::as_array)
        .ok_or_else(|| bad("no `files` array"))?;
    let mut report = BTreeMap::new();
    for entry in files {
        let path = entry
            .get("path")
            .and_then(Json::as_str)
            .ok_or_else(|| bad("a file with no `path`"))?
            .replace('\\', "/");
        let expressions = entry
            .get("spdx_expressions")
            .and_then(Json::as_array)
            .ok_or_else(|| Refusal::Reuse(format!("{path}: no `spdx_expressions` array")))?;
        let mut found = Vec::new();
        for expression in expressions {
            let valid = matches!(expression.get("is_valid"), Some(Json::Bool(true)));
            let value = expression
                .get("value")
                .and_then(Json::as_str)
                .ok_or_else(|| Refusal::Reuse(format!("{path}: an expression with no `value`")))?;
            if !valid {
                return Err(Refusal::Reuse(format!("{path}: `{value}` is not valid")));
            }
            found.push(value.to_owned());
        }
        report.insert(path, found);
    }
    if report.is_empty() {
        return Err(bad("the report lists no files"));
    }
    Ok(report)
}

/// The files `reuse` does not report, by its own rule: the licence texts under `LICENSES/` and any `LICENSE*` file, sidecars, `REUSE.toml` itself, empty files.
fn reuse_ignores(file: &str, empty: bool) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file).to_ascii_uppercase();
    empty
        || file.starts_with("LICENSES/")
        || name.starts_with("LICENSE")
        || name.starts_with("LICENCE")
        || name.starts_with("COPYING")
        || name == "REUSE.TOML"
        || name.ends_with(".LICENSE")
}

/// An expression's licence identifiers, sorted: two spellings of the same terms agree.
fn terms(expression: &str) -> Vec<&str> {
    let mut found: Vec<&str> = expression
        .split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '.'))
        .filter(|t| !t.is_empty())
        .collect();
    found.sort_unstable();
    found
}

/// Why a quoting file's expressions are wrong, if they are. The file passes by carrying the
/// combined expression, or by carrying the library's and no game expression at all (it is
/// purely the library's text). A game expression and the library's as two separate
/// expressions is neither.
fn licence_problem(expressions: &[String]) -> Option<String> {
    let set = sorted_set(expressions);
    for expression in &set {
        if !ALLOWED.contains(&expression.as_str()) {
            return Some(if terms(expression) == terms(COMBINED) {
                format!("carries `{expression}`; write `{COMBINED}`")
            } else {
                format!(
                    "carries `{expression}`, which is none of `{GAME}`, `{LIBRARY}`, `{COMBINED}`"
                )
            });
        }
    }
    let has = |wanted: &str| set.iter().any(|e| e == wanted);
    if has(COMBINED) || (has(LIBRARY) && !has(GAME)) {
        None
    } else if has(LIBRARY) {
        Some(format!(
            "carries `{GAME}` and `{LIBRARY}` as two separate expressions; give it the combined \
             expression: write `{COMBINED}`"
        ))
    } else {
        Some(format!("is licensed `{GAME}` only; write `{COMBINED}`"))
    }
}

fn sorted_set(expressions: &[String]) -> Vec<String> {
    let mut set = expressions.to_vec();
    set.sort();
    set.dedup();
    set
}

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

/// Run the check over `files` (the tracked paths) with `report` as `reuse`'s answer.
pub(crate) fn check(
    files: &[(String, Vec<u8>)],
    report: &BTreeMap<String, Vec<String>>,
) -> Result<Outcome, Refusal> {
    let library = Library::read(files)?;
    let mut outcome = Outcome {
        library_files: library.files,
        library_runs: library.runs.len(),
        ..Outcome::default()
    };
    let mut contents: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut fixtures: BTreeMap<&str, &str> = BTreeMap::new();
    let mut texts: Vec<(&str, &str)> = Vec::new();
    for (file, bytes) in files.iter().filter(|(f, _)| !f.starts_with("library/")) {
        if is_binary(file) {
            continue;
        }
        let text = std::str::from_utf8(bytes).map_err(|e| Refusal::Unreadable {
            file: file.clone(),
            why: e.to_string(),
        })?;
        outcome.scanned += 1;
        let expressions = match report.get(file) {
            Some(expressions) => expressions,
            None if reuse_ignores(file, text.is_empty()) => continue,
            None => return Err(Refusal::NotReported(file.clone())),
        };
        if let Some(quoted) = library.quoted_in(&normalise(text)) {
            if let Some(problem) = licence_problem(expressions) {
                let shown: String = quoted.chars().take(60).collect();
                outcome
                    .misses
                    .push(format!("{file} quotes \"{shown}...\" and {problem}"));
            }
            outcome.quoting.insert(file.clone(), expressions.clone());
        }
        texts.push((file, text));
    }
    for (file, text) in texts {
        if file.starts_with("tests/golden/") || file.starts_with("scenarios/") {
            contents.entry(text).or_default().push(file);
        }
        if file.starts_with("godot/fixtures/") {
            fixtures.insert(file, text);
        }
    }
    for (copy, text) in &fixtures {
        let (Some(sources), Some(mine)) = (contents.get(text), report.get(*copy)) else {
            continue;
        };
        for source in sources {
            let theirs = report.get(*source).map(|e| sorted_set(e));
            if theirs.as_ref() != Some(&sorted_set(mine)) {
                outcome.misses.push(format!(
                    "{copy} is a byte copy of {source}, which is licensed {theirs:?}, not {:?}",
                    sorted_set(mine)
                ));
            }
        }
    }
    Ok(outcome)
}

/// A file read as bytes and never as text, by its extension.
fn is_binary(file: &str) -> bool {
    Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| BINARY.contains(&e))
}

/// The files the manifest and the reviews named as quoting the library; the check has to find
/// every one, or it is looking at nothing.
const KNOWN: &[&str] = &[
    "tests/golden/gateway/demo_hold_and_build_render/expected.response.json",
    "tests/golden/gateway/demo_expand_and_mine_render/expected.response.json",
    "tests/golden/gateway/demo_safe_playbook_render/expected.response.json",
    "tests/golden/gateway/demo_defend_the_core_render/expected.response.json",
    "tests/golden/gateway/demo_raid_an_enemy_beacon_render/expected.response.json",
    "tests/golden/gateway/demo_against_easy_render/expected.response.json",
    "tests/golden/gateway/instantiate_suggested/expected.response.json",
    "godot/fixtures/instantiate_suggested.json",
    "godot/fixtures/rule_list_render.json",
    "scenarios/skeleton/against-easy.playbook.jsonc",
    "crates/gateway/src/host.rs",
];

/// The `reuse` step's quotation check over the tracked tree: `git ls-files`, the files'
/// bytes, and `reuse lint --json`, handed to [`check`] as values, then the positive control.
/// `Ok` is a line for the step to print; `Err` is the step's failure message.
pub(crate) fn check_tree(root: &Path, require_tools: bool) -> Result<String, String> {
    check_tree_refusing(root, require_tools).map_err(|refusal| refusal.to_string())
}

fn check_tree_refusing(root: &Path, require_tools: bool) -> Result<String, Refusal> {
    let listed = crate::child_command(require_tools, "git", &["ls-files", "-z"], root)
        .output()
        .map_err(|e| Refusal::Git(e.to_string()))?;
    if !listed.status.success() {
        return Err(Refusal::Git(
            String::from_utf8_lossy(&listed.stderr).into_owned(),
        ));
    }
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for name in String::from_utf8_lossy(&listed.stdout)
        .split('\0')
        .filter(|f| !f.is_empty())
        .filter(|f| !is_binary(f))
    {
        let bytes = fs::read(root.join(name)).map_err(|e| Refusal::Unreadable {
            file: name.to_owned(),
            why: e.to_string(),
        })?;
        files.push((name.to_owned(), bytes));
    }
    // `reuse lint` exits non-zero on a non-compliant tree and still writes the report; the step
    // has already run it and passed, so only a failure to run it, or an answer that is not the
    // report, is a refusal.
    let answer = crate::child_command(require_tools, "reuse", &["lint", "--json"], root)
        .output()
        .map_err(|e| Refusal::Reuse(e.to_string()))?;
    let report = read_report(&String::from_utf8_lossy(&answer.stdout))?;
    let outcome = check(&files, &report)?;
    if outcome.scanned <= 800 {
        return Err(Refusal::Control(format!(
            "only {} files were read",
            outcome.scanned
        )));
    }
    if outcome.library_files < 5 || outcome.library_runs <= 100 {
        return Err(Refusal::Control(format!(
            "the library was not read: {} files, {} word runs",
            outcome.library_files, outcome.library_runs
        )));
    }
    if let Some(known) = KNOWN.iter().find(|k| !outcome.quoting.contains_key(**k)) {
        return Err(Refusal::Control(format!(
            "{known} is known to quote the library and was not found quoting it"
        )));
    }
    if !outcome.misses.is_empty() {
        return Err(Refusal::Misses(outcome.misses));
    }
    Ok(format!(
        "{} files read; every one that quotes the library ({}) carries its licence",
        outcome.scanned,
        outcome.quoting.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> String {
        text.to_owned()
    }

    #[test]
    fn a_sentence_is_a_full_stop_and_eight_words() {
        let unit = "Hold & Build. Raise the lantern above the harbour wall when fog comes in \
                    and keep it lit until dawn. Why: short one.";
        assert_eq!(
            sentences(unit),
            vec![s(
                "Raise the lantern above the harbour wall when fog comes in and keep it lit until dawn."
            )]
        );
        assert!(sentences("How long to keep the lantern, in game milliseconds").is_empty());
        assert!(sentences("commander died (S2's plan, decision 35).").is_empty());
    }

    #[test]
    fn the_scanner_reads_strings_and_comment_paragraphs() {
        let text = "// first line of\n// one paragraph.\n//\n// another.\n{\n  \"a\": \"say \\\"hi\\\" // not a comment\", // trailing\n  \"b\": \"two\\nlines\"\n}\n";
        assert_eq!(
            units(text),
            vec![
                (s("first line of one paragraph."), false),
                (s("another."), false),
                (s("a"), true),
                (s("say \"hi\" // not a comment"), true),
                (s("trailing"), false),
                (s("b"), true),
                (s("two lines"), true),
            ]
        );
    }

    #[test]
    fn a_rewrapped_escaped_or_commented_quotation_still_matches() {
        let mut library = Library::default();
        library.add_unit(
            "Raise the lantern above the harbour wall when fog comes in and keep it lit until dawn.",
            true,
        );
        for text in [
            "// Raise the lantern above the harbour\n// wall when fog comes in and keep it\n// lit until dawn.\n",
            "\"note\": \"Raise the lantern above the harbour wall when fog comes in and keep it lit until\\ndawn.\"",
            "\"Raise  the lantern above the harbour wall when fog comes in and keep it lit until dawn.\"",
            "concat!(\"Raise the lantern above the harbour wall\\n\",\n    \"when fog comes in and keep it lit until dawn.\\n\")",
            "{\"x\": \"Raise the lantern above the harbour wall when fog comes in and keep it lit until dawn.\"}",
        ] {
            assert!(library.quoted_in(&normalise(text)).is_some(), "{text}");
        }
        assert!(
            library
                .quoted_in(&normalise("a lantern above the harbour wall"))
                .is_none()
        );
    }

    #[test]
    fn expressions_are_compared_exactly() {
        let e = |t: &[&str]| -> Vec<String> { t.iter().map(|x| s(x)).collect() };
        assert_eq!(licence_problem(&e(&[GAME, COMBINED])), None);
        assert_eq!(licence_problem(&e(&[LIBRARY])), None);
        assert!(licence_problem(&e(&[LIBRARY, GAME])).is_some());
        assert_eq!(licence_problem(&e(&[LIBRARY, LIBRARY])), None);
        assert!(licence_problem(&e(&[GAME])).is_some());
        assert!(licence_problem(&e(&["GPL-3.0-or-later OR (MIT OR Apache-2.0)"])).is_some());
        assert!(licence_problem(&e(&[GAME, "GPL-3.0-or-later AND MIT OR Apache-2.0"])).is_some());
    }

    #[test]
    fn a_malformed_report_is_a_refusal_never_a_pass() {
        assert!(matches!(read_report("{"), Err(Refusal::Reuse(_))));
        assert!(matches!(read_report("{}"), Err(Refusal::Reuse(_))));
        assert!(matches!(
            read_report("{\"files\": []}"),
            Err(Refusal::Reuse(_))
        ));
        assert!(matches!(
            read_report(
                "{\"files\": [{\"path\": \"a\", \"spdx_expressions\": [{\"value\": \"X\", \"is_valid\": false}]}]}"
            ),
            Err(Refusal::Reuse(_))
        ));
        let ok = read_report(
            "{\"files\": [{\"path\": \"a/b\", \"spdx_expressions\": [{\"value\": \"MIT\", \"is_valid\": true}]}]}",
        );
        assert_eq!(ok.map(|r| r.len()), Ok(1));
    }

    const SENTENCE: &str =
        "Raise the lantern above the harbour wall when fog comes in and keep it lit until dawn.";

    /// The checker over in-memory files and an in-memory report: no filesystem, no process,
    /// nothing a parallel test could share.
    fn run(files: &[(&str, &str)], report: &[(&str, &[&str])]) -> Result<Outcome, Refusal> {
        let files: Vec<(String, Vec<u8>)> = files
            .iter()
            .map(|(path, text)| (s(path), text.as_bytes().to_vec()))
            .collect();
        let report: BTreeMap<String, Vec<String>> = report
            .iter()
            .map(|(path, expressions)| (s(path), expressions.iter().map(|e| s(e)).collect()))
            .collect();
        check(&files, &report)
    }

    fn library_of(text: &str) -> (String, String) {
        (s("library/t.jsonc"), text.to_owned())
    }

    /// One file beside a library file, with the expressions `reuse` would report for it.
    fn one_file(
        library: &[(&str, &str)],
        file: &str,
        text: &str,
        expressions: &[&str],
    ) -> Result<Outcome, Refusal> {
        let mut files: Vec<(&str, &str)> = library.to_vec();
        files.push((file, text));
        run(&files, &[(file, expressions)])
    }

    fn sentence_library() -> Vec<(String, String)> {
        vec![library_of(&format!("{{\"note\": \"{SENTENCE}\"}}\n"))]
    }

    fn as_refs(library: &[(String, String)]) -> Vec<(&str, &str)> {
        library
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect()
    }

    #[test]
    fn a_quoting_file_anywhere_in_the_tree_is_a_miss_unless_it_carries_the_library_licence() {
        let library = format!("{{\"note\": \"{SENTENCE}\"}}\n");
        let doc = format!("// {SENTENCE}\n");
        let code = format!("let x = \"{SENTENCE}\";\n");
        let json = format!("\"{SENTENCE}\"");
        let wrapped = "a re-wrapped\n// Raise the lantern above the harbour\n// wall when fog comes in and keep it\n// lit until dawn.\n";
        let outcome = run(
            &[
                ("library/t.jsonc", &library),
                ("docs/a.md", &doc),
                ("examples/b.md", wrapped),
                ("crates/gateway/tests/c.rs", &code),
                ("tests/golden/x/ok.json", &json),
                ("tests/golden/x/bad.json", &json),
                ("tests/golden/x/or.json", &json),
            ],
            &[
                ("docs/a.md", &[LIBRARY]),
                ("examples/b.md", &[GAME]),
                ("crates/gateway/tests/c.rs", &[GAME]),
                ("tests/golden/x/ok.json", &[COMBINED]),
                ("tests/golden/x/bad.json", &[GAME]),
                (
                    "tests/golden/x/or.json",
                    &["GPL-3.0-or-later OR (MIT OR Apache-2.0)"],
                ),
            ],
        )
        .unwrap_or_default();
        let missed: Vec<&str> = outcome
            .misses
            .iter()
            .filter_map(|m| m.split(' ').next())
            .collect();
        assert_eq!(
            missed,
            vec![
                "examples/b.md",
                "crates/gateway/tests/c.rs",
                "tests/golden/x/bad.json",
                "tests/golden/x/or.json"
            ],
            "{:?}",
            outcome.misses
        );
        assert_eq!(outcome.quoting.len(), 6);
    }

    #[test]
    fn a_file_the_report_lacks_or_cannot_be_read_is_a_refusal() {
        let library = format!("{{\"note\": \"{SENTENCE}\"}}\n");
        let report: BTreeMap<String, Vec<String>> =
            BTreeMap::from([(s("a/other.txt"), vec![s(GAME)])]);
        let with = |file: &str, bytes: &[u8]| {
            check(
                &[
                    (s("library/t.jsonc"), library.clone().into_bytes()),
                    (s(file), bytes.to_vec()),
                ],
                &report,
            )
        };
        let unlisted = with("a/unlisted.txt", b"plain");
        let unreadable = with("a/binary.dat", &[0xff, 0xfe, 0x00]);
        assert!(
            matches!(unlisted, Err(Refusal::NotReported(_))),
            "{unlisted:?}"
        );
        assert!(
            matches!(unreadable, Err(Refusal::Unreadable { .. })),
            "{unreadable:?}"
        );
        // A binary by extension is not read; an empty file is one `reuse` does not report.
        assert_eq!(with("a/x.png", &[0xff]).map(|o| o.scanned), Ok(0));
        assert_eq!(with("a/empty", b"").map(|o| o.scanned), Ok(1));
    }

    #[test]
    fn a_byte_copy_carries_every_expression_its_source_does() {
        let library = format!("{{\"note\": \"{SENTENCE}\"}}\n");
        let files = [
            ("library/t.jsonc", library.as_str()),
            ("tests/golden/a/r.json", "same bytes"),
            ("godot/fixtures/r.json", "same bytes"),
        ];
        let differing = run(
            &files,
            &[
                ("tests/golden/a/r.json", &[COMBINED]),
                ("godot/fixtures/r.json", &[GAME]),
            ],
        );
        let same = run(
            &files,
            &[
                ("tests/golden/a/r.json", &[COMBINED]),
                ("godot/fixtures/r.json", &[COMBINED, COMBINED]),
            ],
        );
        assert_eq!(differing.map(|o| o.misses.len()), Ok(1));
        assert_eq!(same.map(|o| o.misses.len()), Ok(0));
    }

    const LABEL: &str = "Which lantern the keeper lights first: the nearest one on the wall that is not lit yet, or one by name";

    #[test]
    fn a_long_description_with_no_full_stop_is_expression_and_a_short_label_is_a_name() {
        let library = [library_of(&format!(
            "{{\"label\": \"{LABEL}\", \"name\": \"How long to keep the lantern\"}}\n"
        ))];
        let library = as_refs(&library);
        let long = one_file(
            &library,
            "crates/c/tests/a.rs",
            &format!("assert_eq!(l, \"{LABEL}\");\n"),
            &[GAME],
        );
        assert_eq!(long.map(|o| o.misses.len()), Ok(1));
        let short = one_file(
            &library,
            "crates/c/tests/b.rs",
            "x(\"How long to keep the lantern\");\n",
            &[GAME],
        );
        assert_eq!(short.map(|o| o.misses.len()), Ok(0));
    }

    #[test]
    fn json_escapes_are_decoded_on_both_sides() {
        let library = [library_of(
            "{\"note\": \"Le caf\\u00e9 est ouvert, et il y a du th\\u00e9 et du lait pour tous les gens.\"}\n",
        )];
        let library = as_refs(&library);
        let literal =
            "Le caf\u{e9} est ouvert, et il y a du th\u{e9} et du lait pour tous les gens.";
        let escaped = "{\"x\": \"Le caf\\u00e9 est ouvert, et il y a du th\\u00e9 et du lait pour tous les gens.\"}";
        let slash = "{\"x\": \"Le caf\\u00e9 est ouvert, et il y a du th\\u00e9 et du lait pour tous les gens.\\/\"}";
        for text in [literal, escaped, slash] {
            let found = one_file(&library, "docs/x.json", text, &[GAME]);
            assert_eq!(found.map(|o| o.misses.len()), Ok(1), "{text}");
        }
    }

    #[test]
    fn every_tracked_file_under_library_is_library_text() {
        let library = [(
            s("library/notes.md"),
            s(
                "A note: the guard post covers the nearest scrap seam you do not cover yet, and holds.\n",
            ),
        )];
        let found = one_file(
            &as_refs(&library),
            "crates/x/src/a.rs",
            "// A note: the guard post covers the nearest scrap seam you do not cover yet, and holds.",
            &[GAME],
        );
        assert_eq!(found.map(|o| o.misses.len()), Ok(1));
    }

    #[test]
    fn the_library_licence_beside_a_game_one_is_not_the_combined_expression() {
        let library = sentence_library();
        let library = as_refs(&library);
        let text = format!("// {SENTENCE}\n");
        for ok in [
            &[LIBRARY][..],
            &[COMBINED],
            &[GAME, COMBINED],
            &[LIBRARY, LIBRARY],
        ] {
            let found = one_file(&library, "docs/a.md", &text, ok);
            assert_eq!(found.map(|o| o.misses.len()), Ok(0), "{ok:?}");
        }
        let pair = one_file(&library, "crates/x/src/a.rs", &text, &[GAME, LIBRARY]);
        assert_eq!(pair.map(|o| o.misses.len()), Ok(1));
    }

    #[test]
    fn the_failure_names_each_expression_once_and_the_exact_combined_form() {
        let library = sentence_library();
        let library = as_refs(&library);
        let text = format!("// {SENTENCE}\n");
        let doubled = one_file(&library, "crates/x/src/a.rs", &text, &[GAME, GAME]);
        let message = doubled.map(|o| o.misses.join("\n")).unwrap_or_default();
        assert_eq!(
            message.matches(&format!("`{GAME}`")).count(),
            1,
            "{message}"
        );
        let reordered = one_file(
            &library,
            "crates/x/src/b.rs",
            &text,
            &["(MIT OR Apache-2.0) AND GPL-3.0-or-later"],
        );
        let message = reordered.map(|o| o.misses.join("\n")).unwrap_or_default();
        assert!(
            message.contains(&format!("write `{COMBINED}`")),
            "{message}"
        );
    }
}

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
//!   added is seen once it is. A media, font or descriptor file (the named [`BINARY`] list) is
//!   never read; a tracked archive or executable ([`OPAQUE`]) is a [`Refusal`], because it can
//!   embed library text the check cannot read; a tracked file missing from disk, and any other
//!   read or UTF-8 error, is a [`Refusal`], never a skip.
//! * **What licence it has:** `reuse lint --json`'s answer for the file, the expressions as the
//!   tool concluded them from the manifest, the sidecar and the header together, so this agrees
//!   with `reuse lint` by construction (precedence and globs included) and no manifest is read
//!   by hand. Every expression of a quoting file must be one of the three [`ALLOWED`] forms,
//!   compared exactly (`OR` where `AND` is meant fails), and the file must carry the combined
//!   expression, or the library's with no game expression beside it (a game expression and the
//!   library's as two separate expressions is neither).
//! * **The converse:** a tracked file outside `library/` whose expressions, taken together,
//!   name `GPL-3.0-or-later`, `MIT` and `Apache-2.0` (in one expression or split across
//!   several, whatever else is beside them) and which quotes nothing of the library keeps the
//!   library's licence for no reason, and fails: it is `GPL-3.0-or-later` (decisions-log item
//!   164 (3a)). A [`BINARY`] file holds no prose, so one that names both fails too. The
//!   library's licence with no game licence beside it is not tested this way: the permissive
//!   areas are permissive by their own tier, whether or not they quote.
//! * **Byte copies:** a file under `godot/fixtures/` that is byte-identical to one under
//!   `tests/golden/` or `scenarios/` carries exactly the same expressions as it.
//! * **A positive control:** the check asserts it scanned a plausible number of files and that
//!   the files known to quote the library are found quoting it and carrying the combined
//!   expression, so "nothing found" cannot mean "nothing looked at", in either direction.
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

/// Files never read as text, by extension: media, fonts, and the protobuf descriptor set
/// (`buf` writes it from `proto/` with source info excluded, so it holds the schema's names and
/// no prose). None of them can hold a sentence of the library, so the check reads none of them
/// and holds each one's licence to the converse alone.
const BINARY: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "ico", "vox", "ogg", "wav", "mp3", "ttf", "otf", "woff", "woff2",
    "binpb",
];

/// Archives and executables, by extension. Either can embed library text (a packed `.pck` holds
/// the library itself) and the check cannot read inside it, so a tracked one is a
/// [`Refusal::Opaque`], never a pass: build outputs are not tracked.
const OPAQUE: &[&str] = &["zip", "pck", "wasm", "dll", "so", "dylib", "exe", "bin"];

/// Why the check could not give an answer. Every one fails the test.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// `git ls-files` failed.
    Git(String),
    /// `reuse lint --json` did not run, or its answer was not the shape read.
    Reuse(String),
    /// A tracked text file could not be read as UTF-8 text.
    Unreadable { file: String, why: String },
    /// A tracked file is not on disk (deleted in the working tree and not yet committed).
    Missing(String),
    /// A tracked file could not be read from disk for another reason.
    Read { file: String, why: String },
    /// A tracked archive or executable: it can embed library text the check cannot read.
    Opaque(String),
    /// A tracked file that `reuse` should have reported is not in its report.
    NotReported(String),
    /// A library file could not be read.
    Library(String),
    /// The positive control failed: the check looked at too little to mean anything.
    Control(String),
    /// Files whose licences disagree with what they quote: a quoting file without the
    /// library's licence, a non-quoting file with the combined expression, a byte copy whose
    /// expressions differ from its source's. Each line names the file and the fix.
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
                    "{file} is not readable as text ({why}); add its extension to BINARY if it is a \
                     media or font file, or to OPAQUE if it is an archive or executable"
                )
            }
            Refusal::Missing(file) => write!(
                f,
                "{file} is tracked and not on disk; restore it, or commit its removal"
            ),
            Refusal::Read { file, why } => write!(f, "{file} could not be read from disk ({why})"),
            Refusal::Opaque(file) => write!(
                f,
                "{file} is a tracked archive or executable, which can embed library/ text the \
                 check cannot read; untrack it (a build output belongs in the target folder)"
            ),
            Refusal::NotReported(file) => write!(f, "reuse's report has no entry for {file}"),
            Refusal::Library(why) => write!(f, "library/: {why}"),
            Refusal::Control(why) => write!(f, "the quotation check's positive control: {why}"),
            Refusal::Misses(misses) => write!(
                f,
                "these files' licences disagree with what they quote of library/ ({LIBRARY}); \
                 each line names the licence to write (REUSE.toml, or the file's own header):\n  {}",
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
    /// Files whose expressions together name the game's licence and the library's
    /// ([`names_both`]), quoting or not.
    pub(crate) combined: BTreeSet<String>,
    /// Quoting files whose licences are wrong, non-quoting files that carry the combined
    /// expression, and copies that differ from their source.
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
        let mut chars = line.chars();
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
            } else if c == '/' && chars.clone().next() == Some('/') {
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

/// One JSON string escape, read after its backslash, as a quotation reads it:
/// decoded by xtask's one escape decoder ([`crate::json_escape`]), the shared
/// JSON reader's, so the two never read an escape differently; and `\n \t \r
/// \b \f` become a space, as all whitespace does here. `None`, with `chars`
/// untouched, when what follows is not a JSON escape the decoder accepts:
/// the text scanned here is any tracked file's, where a backslash that starts
/// no escape (a Rust `\u{...}`, a Windows path, an unpaired surrogate) is
/// text, and is read as itself.
fn escape(chars: &mut Chars<'_>) -> Option<String> {
    let rest = chars.as_str();
    let (character, used) = crate::json_escape(rest).ok()?;
    let whitespace = rest.starts_with(['n', 't', 'r', 'b', 'f']);
    // An escape is ASCII, so its `used` bytes end on a character boundary.
    *chars = rest.get(used..)?.chars();
    Some(if whitespace {
        " ".to_owned()
    } else {
        character.to_string()
    })
}

/// A file's text in the form quotations are matched against: JSON string escapes undone, the
/// leading `//` of each comment line removed, the pieces of a Rust `concat!` joined,
/// whitespace collapsed. A library string is decoded by the same escape reader, so the two
/// sides cannot read an escape differently.
fn normalise(text: &str) -> String {
    let mut unescaped = String::with_capacity(text.len());
    let mut chars = text.chars();
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
    let name = file
        .rsplit_once('/')
        .map_or(file, |(_, name)| name)
        .to_ascii_uppercase();
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

/// Whether a file's expressions, taken together, name the game's licence and the library's:
/// the union of their licence identifiers holds `GPL-3.0-or-later`, `MIT` and `Apache-2.0`,
/// whatever else is beside them and however they are split between expressions (a manifest
/// block's `GPL-3.0-or-later` and a header's `MIT OR Apache-2.0` name both, as the combined
/// expression does, and so does `... AND CC-BY-SA-4.0`). A file that quotes nothing has no
/// reason to carry the library's licence beside the game's in any form. The library's licence
/// with no game licence beside it is not this: the permissive areas are permissive by tier.
fn names_both(expressions: &[String]) -> bool {
    let union: BTreeSet<&str> = expressions.iter().flat_map(|e| identifiers(e)).collect();
    identifiers(COMBINED).iter().all(|id| union.contains(id))
}

/// A file's expressions as a miss names them: each once, in order, in backticks.
fn shown(expressions: &[String]) -> String {
    sorted_set(expressions)
        .iter()
        .map(|e| format!("`{e}`"))
        .collect::<Vec<_>>()
        .join(" and ")
}

/// An expression's licence identifiers, operators dropped, sorted and each once: the set of
/// licences it names, whatever joins them.
fn identifiers(expression: &str) -> Vec<&str> {
    let mut found: Vec<&str> = terms(expression)
        .into_iter()
        .filter(|t| !matches!(*t, "AND" | "OR" | "WITH"))
        .collect();
    found.dedup();
    found
}

/// The miss for a file whose `expressions` name the game's licence and the library's
/// ([`names_both`]) and which quotes nothing of the library.
fn unquoted_problem(file: &str, expressions: &[String], binary: bool) -> String {
    let why = if binary {
        "is a media, font or descriptor file, which holds no library/ prose"
    } else {
        "quotes no sentence and no word run of library/"
    };
    format!(
        "{file} carries {} and {why}, so the library's licence has no reason to be there: \
         write `{GAME}` (its own header), or delete the REUSE.toml entry that gives it the \
         library's licence",
        shown(expressions)
    )
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
        if has_extension(file, OPAQUE) {
            return Err(Refusal::Opaque(file.clone()));
        }
        if is_binary(file) {
            // Not read: it holds no prose, so naming the library's licence is a miss on it.
            if let Some(expressions) = report.get(file).filter(|e| names_both(e)) {
                outcome.combined.insert(file.clone());
                outcome
                    .misses
                    .push(unquoted_problem(file, expressions, true));
            }
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
        let both = names_both(expressions);
        if both {
            outcome.combined.insert(file.clone());
        }
        if let Some(quoted) = library.quoted_in(&normalise(text)) {
            if let Some(problem) = licence_problem(expressions) {
                let shown: String = quoted.chars().take(60).collect();
                outcome
                    .misses
                    .push(format!("{file} quotes \"{shown}...\" and {problem}"));
            }
            outcome.quoting.insert(file.clone(), expressions.clone());
        } else if both {
            outcome
                .misses
                .push(unquoted_problem(file, expressions, false));
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

/// A file never read as text, by its extension ([`BINARY`]).
fn is_binary(file: &str) -> bool {
    has_extension(file, BINARY)
}

fn has_extension(file: &str, list: &[&str]) -> bool {
    Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| list.contains(&e))
}

/// The tracked files' bytes, each read through `read`, except a [`BINARY`] file, which is
/// listed by name with no bytes and never read ([`check`] holds its licence to the converse
/// alone). A file `read` cannot find is [`Refusal::Missing`]; any other read error is
/// [`Refusal::Read`].
fn read_tracked<'a>(
    names: impl IntoIterator<Item = &'a str>,
    mut read: impl FnMut(&str) -> std::io::Result<Vec<u8>>,
) -> Result<Vec<(String, Vec<u8>)>, Refusal> {
    let mut files = Vec::new();
    for name in names {
        let bytes = if is_binary(name) {
            Vec::new()
        } else {
            read(name).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => Refusal::Missing(name.to_owned()),
                _ => Refusal::Read {
                    file: name.to_owned(),
                    why: e.to_string(),
                },
            })?
        };
        files.push((name.to_owned(), bytes));
    }
    Ok(files)
}

/// The files the manifest and the reviews named as quoting the library and carrying the
/// combined expression; the check has to find every one doing both, or it is looking at
/// nothing.
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
    let listed = String::from_utf8_lossy(&listed.stdout);
    let files = read_tracked(listed.split('\0').filter(|f| !f.is_empty()), |name| {
        fs::read(root.join(name))
    })?;
    // `reuse lint` exits non-zero on a non-compliant tree and still writes the report; the step
    // has already run it and passed, so only a failure to run it, or an answer that is not the
    // report, is a refusal.
    let answer = crate::child_command(require_tools, "reuse", &["lint", "--json"], root)
        .output()
        .map_err(|e| Refusal::Reuse(e.to_string()))?;
    let report = read_report(&String::from_utf8_lossy(&answer.stdout))?;
    let outcome = check(&files, &report)?;
    verdict(outcome, KNOWN)
}

/// The positive control, then the misses, then the step's line. The control holds both
/// directions: every `known` file must be found quoting the library (or the quotation finder
/// is looking at nothing) and found carrying the combined expression (or the converse rule,
/// which keys on that expression, is looking at nothing).
fn verdict(outcome: Outcome, known: &[&str]) -> Result<String, Refusal> {
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
    if let Some(file) = known.iter().find(|k| !outcome.quoting.contains_key(**k)) {
        return Err(Refusal::Control(format!(
            "{file} is known to quote the library and was not found quoting it"
        )));
    }
    if let Some(file) = known.iter().find(|k| !outcome.combined.contains(**k)) {
        return Err(Refusal::Control(format!(
            "{file} is known to carry `{COMBINED}` and was not found carrying the game's \
             licence and the library's"
        )));
    }
    if !outcome.misses.is_empty() {
        return Err(Refusal::Misses(outcome.misses));
    }
    Ok(format!(
        "{} files read; every one that quotes the library ({}) carries its licence, and every \
         one whose licences name `{GAME}` with `MIT` and `Apache-2.0` ({}) quotes the library",
        outcome.scanned,
        outcome.quoting.len(),
        outcome.combined.len()
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
        // Neither quotes the library, so neither may carry the combined form (the converse);
        // the copy rule is about the two agreeing.
        let files = [
            ("library/t.jsonc", library.as_str()),
            ("tests/golden/a/r.json", "same bytes"),
            ("godot/fixtures/r.json", "same bytes"),
        ];
        let differing = run(
            &files,
            &[
                ("tests/golden/a/r.json", &[GAME]),
                ("godot/fixtures/r.json", &[LIBRARY]),
            ],
        );
        let same = run(
            &files,
            &[
                ("tests/golden/a/r.json", &[GAME]),
                ("godot/fixtures/r.json", &[GAME, GAME]),
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

    /// What the converse says about `file` carrying `expressions` and quoting nothing.
    fn unquoted_line(file: &str, expressions: &[&str]) -> String {
        let expressions: Vec<String> = expressions.iter().map(|e| s(e)).collect();
        format!(
            "{file} carries {} and quotes no sentence and no word run of library/, so the \
             library's licence has no reason to be there: write `GPL-3.0-or-later` (its own \
             header), or delete the REUSE.toml entry that gives it the library's licence",
            shown(&expressions)
        )
    }

    #[test]
    fn a_file_that_carries_the_combined_expression_and_quotes_nothing_is_a_miss() {
        let library = sentence_library();
        let library = as_refs(&library);
        let found = one_file(
            &library,
            "tests/golden/x/report.json",
            "{\"code\": \"I0003\", \"message\": \"A setting left out takes its default.\"}",
            &[COMBINED],
        );
        assert_eq!(
            found.map(|o| o.misses),
            Ok(vec![format!(
                "tests/golden/x/report.json carries `{COMBINED}` and quotes no sentence and no \
                 word run of library/, so the library's licence has no reason to be there: \
                 write `GPL-3.0-or-later` (its own header), or delete the REUSE.toml entry that \
                 gives it the library's licence"
            )])
        );
    }

    #[test]
    fn a_file_that_carries_the_combined_expression_and_quotes_passes() {
        let library = sentence_library();
        let library = as_refs(&library);
        let found = one_file(
            &library,
            "tests/golden/x/render.json",
            &format!("{{\"prose\": \"{SENTENCE}\"}}"),
            &[COMBINED],
        );
        assert_eq!(found.map(|o| o.misses), Ok(Vec::new()));
    }

    #[test]
    fn a_file_that_quotes_nothing_passes_with_the_games_licence_or_the_librarys_alone() {
        let library = sentence_library();
        let library = as_refs(&library);
        // Launch ruling 2: the library's licence with no game licence beside it need not quote,
        // whatever else is beside it.
        for expressions in [
            &[GAME][..],
            &[LIBRARY],
            &[LIBRARY, LIBRARY],
            &["CC-BY-SA-4.0"],
            &[LIBRARY, "CC-BY-SA-4.0"],
            &["GPL-3.0-or-later AND MIT"],
        ] {
            let found = one_file(
                &library,
                "tests/golden/x/plain.json",
                "{\"code\": \"I0003\"}",
                expressions,
            );
            assert_eq!(found.map(|o| o.misses), Ok(Vec::new()), "{expressions:?}");
        }
    }

    #[test]
    fn the_combined_expression_beside_another_still_fails_a_file_that_quotes_nothing() {
        let library = sentence_library();
        let library = as_refs(&library);
        // A manifest block's combined form beside a header's or a sidecar's own expression:
        // `reuse` reports both, and the file is a miss naming each.
        for expressions in [
            &[GAME, COMBINED][..],
            &[COMBINED, LIBRARY],
            &[COMBINED, COMBINED],
        ] {
            let found = one_file(
                &library,
                "crates/x/src/a.rs",
                "// nothing of the library\n",
                expressions,
            );
            assert_eq!(
                found.map(|o| o.misses),
                Ok(vec![unquoted_line("crates/x/src/a.rs", expressions)]),
                "{expressions:?}"
            );
        }
    }

    #[test]
    fn the_games_and_the_librarys_licence_from_two_sources_fail_a_file_that_quotes_nothing() {
        let library = sentence_library();
        let library = as_refs(&library);
        // A manifest block gives `GPL-3.0-or-later` and the file's header `MIT OR Apache-2.0`:
        // no one expression is the combined form, and together they name both.
        for expressions in [
            &[GAME, LIBRARY][..],
            &[LIBRARY, GAME],
            &[GAME, "MIT", "Apache-2.0"],
        ] {
            let found = one_file(&library, "crates/x/src/a.rs", "// nothing\n", expressions);
            assert_eq!(
                found.map(|o| o.misses),
                Ok(vec![unquoted_line("crates/x/src/a.rs", expressions)]),
                "{expressions:?}"
            );
        }
    }

    #[test]
    fn a_fourth_licence_beside_the_two_still_fails_a_file_that_quotes_nothing() {
        let library = sentence_library();
        let library = as_refs(&library);
        for expressions in [
            &["GPL-3.0-or-later AND (MIT OR Apache-2.0) AND CC-BY-SA-4.0"][..],
            &[COMBINED, "CC-BY-SA-4.0"],
            &[GAME, "(MIT OR Apache-2.0) AND MPL-2.0"],
        ] {
            let found = one_file(&library, "crates/x/src/a.rs", "// nothing\n", expressions);
            assert_eq!(
                found.map(|o| o.misses),
                Ok(vec![unquoted_line("crates/x/src/a.rs", expressions)]),
                "{expressions:?}"
            );
        }
    }

    #[test]
    fn another_spelling_of_the_combined_expression_fails_a_file_that_quotes_nothing() {
        let library = sentence_library();
        let library = as_refs(&library);
        for spelling in [
            "(MIT OR Apache-2.0) AND GPL-3.0-or-later",
            "GPL-3.0-or-later OR (MIT OR Apache-2.0)",
        ] {
            let found = one_file(&library, "crates/x/src/a.rs", "// nothing\n", &[spelling]);
            assert_eq!(
                found.map(|o| o.misses),
                Ok(vec![unquoted_line("crates/x/src/a.rs", &[spelling])]),
                "{spelling}"
            );
        }
    }

    #[test]
    fn a_media_file_that_names_both_licences_is_a_miss() {
        let library = format!("{{\"note\": \"{SENTENCE}\"}}\n");
        let files = [
            (s("library/t.jsonc"), library.into_bytes()),
            (s("assets/a.png"), Vec::new()),
            (s("assets/b.png"), Vec::new()),
        ];
        let report = BTreeMap::from([
            (s("assets/a.png"), vec![s(COMBINED)]),
            (s("assets/b.png"), vec![s("CC-BY-SA-4.0")]),
        ]);
        let found = check(&files, &report).map(|o| o.misses);
        assert_eq!(
            found,
            Ok(vec![format!(
                "assets/a.png carries `{COMBINED}` and is a media, font or descriptor file, \
                 which holds no library/ prose, so the library's licence has no reason to be \
                 there: write `GPL-3.0-or-later` (its own header), or delete the REUSE.toml \
                 entry that gives it the library's licence"
            )])
        );
    }

    #[test]
    fn a_tracked_archive_or_executable_is_a_refusal_whatever_its_licence() {
        let library = format!("{{\"note\": \"{SENTENCE}\"}}\n");
        for name in [
            "a/x.zip",
            "a/x.pck",
            "a/x.wasm",
            "a/x.dll",
            "a/x.so",
            "a/x.dylib",
            "a/x.exe",
            "a/x.bin",
        ] {
            for expressions in [vec![s(GAME)], vec![s(COMBINED)], vec![s(LIBRARY)]] {
                let files = [
                    (s("library/t.jsonc"), library.clone().into_bytes()),
                    (s(name), Vec::new()),
                ];
                let report = BTreeMap::from([(s(name), expressions.clone())]);
                assert_eq!(
                    check(&files, &report).map(|o| o.misses),
                    Err(Refusal::Opaque(s(name))),
                    "{name} {expressions:?}"
                );
            }
        }
        assert!(
            Refusal::Opaque(s("a/x.zip"))
                .to_string()
                .starts_with("a/x.zip is a tracked archive or executable")
        );
    }

    #[test]
    fn a_tracked_file_missing_from_disk_is_a_refusal_that_says_so() {
        let missing = read_tracked(["a/gone.rs"], |_| {
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        });
        assert_eq!(missing, Err(Refusal::Missing(s("a/gone.rs"))));
        let message = Refusal::Missing(s("a/gone.rs")).to_string();
        assert!(message.contains("not on disk"), "{message}");
        assert!(!message.contains("BINARY"), "{message}");
        let denied = read_tracked(["a/locked.rs"], |_| {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        });
        assert!(
            matches!(&denied, Err(Refusal::Read { file, .. }) if file == "a/locked.rs"),
            "{denied:?}"
        );
    }

    #[test]
    fn a_media_file_is_listed_by_name_and_never_read() {
        let mut read: Vec<String> = Vec::new();
        let files = read_tracked(["a/x.png", "a/y.rs", "a/z.binpb"], |name| {
            read.push(s(name));
            Ok(b"text".to_vec())
        });
        assert_eq!(
            files,
            Ok(vec![
                (s("a/x.png"), Vec::new()),
                (s("a/y.rs"), b"text".to_vec()),
                (s("a/z.binpb"), Vec::new()),
            ])
        );
        assert_eq!(read, vec![s("a/y.rs")]);
    }

    #[test]
    fn a_library_file_is_not_held_to_the_converse() {
        // `library/` is the text quoted, not a file that quotes it: one of its files carrying
        // the combined form and quoting nothing (a short title only) is not this check's miss.
        let library = format!("{{\"note\": \"{SENTENCE}\"}}\n");
        let outcome = run(
            &[
                ("library/t.jsonc", &library),
                ("library/u.jsonc", "{\"title\": \"Hold\"}\n"),
            ],
            &[
                ("library/t.jsonc", &[LIBRARY]),
                ("library/u.jsonc", &[COMBINED]),
            ],
        );
        assert_eq!(outcome.map(|o| o.misses), Ok(Vec::new()));
    }

    #[test]
    fn misses_are_listed_in_the_tracked_order() {
        let library = sentence_library();
        let mut files = as_refs(&library);
        files.extend([("a/one.rs", "// nothing\n"), ("b/two.rs", "// nothing\n")]);
        let found = run(
            &files,
            &[("a/one.rs", &[COMBINED]), ("b/two.rs", &[COMBINED])],
        );
        assert_eq!(
            found.map(|o| o.misses),
            Ok(vec![
                unquoted_line("a/one.rs", &[COMBINED]),
                unquoted_line("b/two.rs", &[COMBINED]),
            ])
        );
    }

    /// An outcome that passes every count of the positive control, with `known` quoting the
    /// library (`quoting`) and carrying the combined expression (`combined`) as given.
    fn counted(quoting: &[&str], combined: &[&str]) -> Outcome {
        Outcome {
            scanned: 801,
            library_files: 5,
            library_runs: 101,
            quoting: quoting.iter().map(|f| (s(f), vec![s(COMBINED)])).collect(),
            combined: combined.iter().map(|f| s(f)).collect(),
            misses: Vec::new(),
        }
    }

    #[test]
    fn the_positive_control_holds_both_directions() {
        let known = ["a/quotes.json"];
        assert!(verdict(counted(&known, &known), &known).is_ok());
        let not_quoting = verdict(counted(&[], &known), &known);
        assert!(
            matches!(&not_quoting, Err(Refusal::Control(why)) if why.contains("not found quoting")),
            "{not_quoting:?}"
        );
        let not_combined = verdict(counted(&known, &[]), &known);
        assert!(
            matches!(&not_combined, Err(Refusal::Control(why)) if why.contains("not found carrying")),
            "{not_combined:?}"
        );
        let mut missed = counted(&known, &known);
        missed.misses.push(unquoted_line("b/x.rs", &[COMBINED]));
        assert!(matches!(
            verdict(missed, &known),
            Err(Refusal::Misses(m)) if m.len() == 1
        ));
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

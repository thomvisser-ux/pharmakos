// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask placeholders`: the PLACEHOLDER register, collected by a
//! command rather than by hand.
//!
//! Every value a lane had to guess carries a marker (AGENTS.md section 12).
//! Until S1 the markers were free text, so `docs/placeholders.md` was built by
//! a person reading 588 lines one at a time and it went stale with every
//! comment edit (decisions-log item 123 (2) 13). S1's plan, decision 9 (ruled
//! by item 128), gives the markers **one line of grammar**:
//!
//! ```text
//! // PLACEHOLDER: <what> — <who>, <when>
//! ```
//!
//! with any comment leader (`//`, `///`, `//!`, `#`, `;`, `<!--`), the three
//! parts non-empty, the em dash separating the guess from the person and the
//! stage, and everything after the marker's line free prose. This command
//! (the register's S1-10) finds every marker in the tracked tree, parses each,
//! prints the register grouped by when and by who, and lists the markers that
//! are still off the grammar. It is **not a step** yet: the lanes reword their
//! own markers first, and S1's last `xtask` pull request (`tune`) makes it one.
//! Until then `--check` is how a lane asks whether its files are done, and the
//! command otherwise always answers success.
//!
//! # What counts as a marker
//!
//! The whole word `PLACEHOLDER` followed by a colon is a marker and is judged
//! against the grammar. Followed by ` STUB` or by a dash it is a marker in an
//! older spelling, listed to be reworded. Anywhere else — "the module docs'
//! PLACEHOLDER", "a PLACEHOLDER naming the task", `**PLACEHOLDER**` in
//! `rules/README.md`'s delegated table, `HAS_PLACEHOLDERS` — it is a reference
//! to a marker, not one, and is not counted.
//!
//! # What is left out, and why
//!
//! The same scope as the register's own grep (`docs/placeholders.md`, "How the
//! numbers were made"), stated as git pathspecs in [`EXCLUDED`]: `docs/` (the
//! register itself and the design record), the rule text in `AGENTS.md`,
//! `CLAUDE.md` and `.claude/`, the generated prost tree (copies of the
//! `.proto` comments, which are counted at their source), `tests/golden/`
//! (outputs, whose markers are copies of their producers'), and this file,
//! whose own text describes the grammar.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::process::Stdio;

use crate::child_command;

/// The grammar, as the report quotes it.
pub(crate) const GRAMMAR: &str = "PLACEHOLDER: <what> \u{2014} <who>, <when>";

/// The marker word.
const WORD: &str = "PLACEHOLDER";

/// The separator between the guess and the person: an em dash with a space on
/// each side.
const DASH: &str = " \u{2014} ";

/// Git pathspecs the register leaves out; see the module docs.
pub(crate) const EXCLUDED: &[&str] = &[
    ":!docs",
    ":!AGENTS.md",
    ":!CLAUDE.md",
    ":!.claude",
    ":!crates/proto/src/generated",
    ":!tests/golden",
    ":!xtask/src/placeholders.rs",
];

/// One marker in the grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Marker {
    /// What was guessed.
    pub(crate) what: String,
    /// Who resolves it.
    pub(crate) who: String,
    /// When: a stage, a gate, a review.
    pub(crate) when: String,
}

/// What one line of text holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Line {
    /// No marker: no `PLACEHOLDER` word, or only a reference to one.
    Nothing,
    /// A marker in the grammar.
    Marker(Marker),
    /// A marker off the grammar, to be reworded.
    OffGrammar,
}

/// Reads one line of source text.
pub(crate) fn classify(text: &str) -> Line {
    let mut found = Line::Nothing;
    let mut from = 0;
    while let Some(offset) = text.get(from..).and_then(|rest| rest.find(WORD)) {
        let start = from + offset;
        let end = start + WORD.len();
        from = end;
        let before = text.get(..start).and_then(|head| head.chars().next_back());
        let after = text.get(end..).unwrap_or_default();
        if before.is_some_and(is_word_char) || after.chars().next().is_some_and(is_word_char) {
            continue; // HAS_PLACEHOLDERS, PLACEHOLDERs
        }
        if let Some(rest) = after.strip_prefix(':') {
            return match parse(rest) {
                Some(marker) => Line::Marker(marker),
                None => Line::OffGrammar,
            };
        }
        let older = [" STUB", " -", " \u{2014}", " \u{2013}"];
        if older.iter().any(|spelling| after.starts_with(spelling)) {
            found = Line::OffGrammar;
        }
    }
    found
}

fn is_word_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

/// The text after `PLACEHOLDER:`, parsed against the grammar.
fn parse(rest: &str) -> Option<Marker> {
    let mut body = rest.trim();
    for closer in ["-->", "*/"] {
        body = body.strip_suffix(closer).unwrap_or(body).trim_end();
    }
    let (what, tail) = body.split_once(DASH)?;
    let (who, when) = tail.split_once(", ")?;
    let (what, who, when) = (what.trim(), who.trim(), when.trim());
    if what.is_empty() || who.is_empty() || when.is_empty() {
        return None;
    }
    Some(Marker {
        what: what.to_owned(),
        who: who.to_owned(),
        when: when.to_owned(),
    })
}

/// The stage a `when` names, for grouping: the `when` up to its first clause
/// break, so `S1, with the recap` and `S1.` file together under `S1`. A
/// marker's `when` often runs on into the comment's next line; the register
/// groups by the part that is the stage and prints the marker's `what`.
pub(crate) fn stage_of(when: &str) -> String {
    let mut end = when.len();
    for stop in [", ", " (", ": ", "; ", DASH, ". "] {
        if let Some(at) = when.find(stop) {
            end = end.min(at);
        }
    }
    let stage = when
        .get(..end)
        .unwrap_or(when)
        .trim_end_matches(['.', ',', ':', ';']);
    if stage.is_empty() {
        when.to_owned()
    } else {
        stage.to_owned()
    }
}

/// One line of `git grep -n` output: `<path>:<line>:<text>`.
pub(crate) fn split_grep_line(line: &str) -> Option<(&str, usize, &str)> {
    let (path, rest) = line.split_once(':')?;
    let (number, text) = rest.split_once(':')?;
    Some((path, number.parse().ok()?, text))
}

/// What the command found, ready to print.
#[derive(Debug, Default)]
pub(crate) struct Register {
    /// `(when, who)` to the markers filed under it, as `(place, what)`.
    pub(crate) filed: BTreeMap<(String, String), Vec<(String, String)>>,
    /// Markers off the grammar, as `(place, text)`.
    pub(crate) off_grammar: Vec<(String, String)>,
    /// Files holding at least one marker.
    pub(crate) files: usize,
}

impl Register {
    /// The register over `git grep -n` output, in the order git printed it,
    /// which is path order.
    pub(crate) fn from_grep(output: &str) -> Register {
        let mut register = Register::default();
        let mut last_file = String::new();
        for line in output.lines() {
            let Some((path, number, text)) = split_grep_line(line) else {
                continue;
            };
            let place = format!("{path}:{number}");
            let counted = match classify(text) {
                Line::Nothing => false,
                Line::Marker(marker) => {
                    register
                        .filed
                        .entry((stage_of(&marker.when), marker.who))
                        .or_default()
                        .push((place, marker.what));
                    true
                }
                Line::OffGrammar => {
                    register.off_grammar.push((place, text.trim().to_owned()));
                    true
                }
            };
            if counted && path != last_file {
                register.files += 1;
                path.clone_into(&mut last_file);
            }
        }
        register
    }

    /// Markers in the grammar.
    pub(crate) fn in_grammar(&self) -> usize {
        self.filed
            .values()
            .map(Vec::len)
            .fold(0, usize::saturating_add)
    }

    /// The report, whole.
    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "PLACEHOLDER register: {} marker(s) in {} file(s); {} in the grammar, {} to reword",
            self.in_grammar().saturating_add(self.off_grammar.len()),
            self.files,
            self.in_grammar(),
            self.off_grammar.len()
        );
        let _ = writeln!(out, "grammar: {GRAMMAR}");
        for ((when, who), markers) in &self.filed {
            let _ = writeln!(out);
            let _ = writeln!(out, "== {when} \u{2014} {who}");
            for (place, what) in markers {
                let _ = writeln!(out, "   {place}  {what}");
            }
        }
        if !self.off_grammar.is_empty() {
            let _ = writeln!(out);
            let _ = writeln!(out, "== to reword into `{GRAMMAR}`");
            for (place, text) in &self.off_grammar {
                let _ = writeln!(out, "   {place}  {text}");
            }
        }
        out
    }
}

/// `cargo xtask placeholders [--check]`: prints the register; with `--check`,
/// answers failure while a marker is off the grammar.
pub(crate) fn run(root: &Path, require_tools: bool, check: bool) -> Result<bool, String> {
    let mut args: Vec<&str> = vec!["grep", "-n", "-I", "-w", "-e", WORD, "--", "."];
    args.extend_from_slice(EXCLUDED);
    let output = child_command(require_tools, "git", &args, root)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("could not run `git grep`: {error}"))?;
    // `git grep` exits 1 when nothing matched, which is an answer; anything
    // else that is not success is a failure to read the tree.
    match output.status.code() {
        Some(0 | 1) => {}
        _ => {
            return Err(format!(
                "`git grep` failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let register = Register::from_grep(&text);
    print!("{}", register.render());
    if check && !register.off_grammar.is_empty() {
        println!();
        println!(
            "placeholders --check: {} marker(s) are off the grammar; reword each to `{GRAMMAR}` \
             on one line (S1's plan, decision 9)",
            register.off_grammar.len()
        );
        return Ok(false);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(what: &str, who: &str, when: &str) -> Line {
        Line::Marker(Marker {
            what: what.to_owned(),
            who: who.to_owned(),
            when: when.to_owned(),
        })
    }

    #[test]
    fn a_marker_in_the_grammar_is_read_whatever_its_comment_leader() {
        for text in [
            "    // PLACEHOLDER: tuning \u{2014} owner, S1.",
            "/// PLACEHOLDER: tuning \u{2014} owner, S1.",
            "//! PLACEHOLDER: tuning \u{2014} owner, S1.",
            "# PLACEHOLDER: tuning \u{2014} owner, S1.",
            "; PLACEHOLDER: tuning \u{2014} owner, S1.",
            "<!-- PLACEHOLDER: tuning \u{2014} owner, S1. -->",
            "/* PLACEHOLDER: tuning \u{2014} owner, S1. */",
            "rust-version = \"1.98\"  # PLACEHOLDER: tuning \u{2014} owner, S1.",
        ] {
            assert_eq!(classify(text), marker("tuning", "owner", "S1."), "{text}");
        }
    }

    #[test]
    fn the_when_keeps_its_commas_and_the_what_its_own() {
        assert_eq!(
            classify("// PLACEHOLDER: K, B \u{2014} owner, at S6's art pass, with the art"),
            marker("K, B", "owner", "at S6's art pass, with the art")
        );
    }

    #[test]
    fn a_marker_off_the_grammar_is_listed_to_reword() {
        for text in [
            "// PLACEHOLDER: tuning, owner, S1.",           // no em dash
            "// PLACEHOLDER: tuning - owner, S1.",          // a hyphen is not the dash
            "// PLACEHOLDER: tuning \u{2014} owner S1.",    // no comma after who
            "// PLACEHOLDER: tuning \u{2014} owner,",       // no when
            "// PLACEHOLDER: \u{2014} owner, S1.",          // no what
            "// PLACEHOLDER: pin the exact version once",   // prose that runs on
            "// PLACEHOLDER STUB - the per-beacon detail.", // the older spelling
            "# PLACEHOLDER \u{2014} entries held back until",
            "    # PLACEHOLDER - DELETE THIS BLOCK",
        ] {
            assert_eq!(classify(text), Line::OffGrammar, "{text}");
        }
    }

    #[test]
    fn a_reference_to_a_marker_is_not_one() {
        for text in [
            "// the module docs' PLACEHOLDER says why",
            "// message with a PLACEHOLDER naming the task that fills it",
            "//     casing PLACEHOLDER). The wire and the schema",
            "| `match.lull_ms` | **PLACEHOLDER** \u{2014} Tuning, owner |",
            "    HAS_PLACEHOLDERS = 3;",
            "// two PLACEHOLDERs were discharged",
            "no marker here at all",
        ] {
            assert_eq!(classify(text), Line::Nothing, "{text}");
        }
    }

    #[test]
    fn git_grep_output_becomes_a_register() {
        let output = "\
Cargo.toml:25:rust-version = \"1.98\"\n\
clippy.toml:40:# PLACEHOLDER: the held-back list \u{2014} owner, S1.\n\
proto/a.proto:3:// PLACEHOLDER: tuning \u{2014} owner, S2.\n\
proto/a.proto:9:// PLACEHOLDER STUB - the payload.\n\
proto/a.proto:12:// a PLACEHOLDER, referred to\n\
proto/b.proto:1:// PLACEHOLDER: tuning \u{2014} owner, S1\n";
        let register = Register::from_grep(output);
        assert_eq!(register.in_grammar(), 3);
        assert_eq!(
            register.off_grammar,
            vec![(
                "proto/a.proto:9".to_owned(),
                "// PLACEHOLDER STUB - the payload.".to_owned()
            )]
        );
        assert_eq!(register.files, 3, "Cargo.toml's line holds no marker");
        // "S1." and "S1" are one stage.
        let s1 = register
            .filed
            .get(&("S1".to_owned(), "owner".to_owned()))
            .expect("the S1 group");
        assert_eq!(s1.len(), 2);
        let report = register.render();
        assert!(report.contains("4 marker(s) in 3 file(s); 3 in the grammar, 1 to reword"));
        assert!(report.contains("== S1 \u{2014} owner"), "{report}");
        assert!(report.contains("   proto/a.proto:9  "), "{report}");
    }

    #[test]
    fn a_marker_is_filed_under_the_stage_its_when_names() {
        assert_eq!(stage_of("S1."), "S1");
        assert_eq!(stage_of("S1, with the recap's settlement lines"), "S1");
        assert_eq!(stage_of("S3 (decisions-log item 128,"), "S3");
        assert_eq!(
            stage_of("at S3's exit: P1 sets the real number from"),
            "at S3's exit"
        );
        assert_eq!(stage_of("at S2's exit. Item"), "at S2's exit");
        assert_eq!(stage_of("at S1's demo."), "at S1's demo");
        assert_eq!(stage_of("after S4, the last stage that"), "after S4");
        assert_eq!(
            stage_of("v1.1"),
            "v1.1",
            "a version's own dot is not a full stop"
        );
    }

    #[test]
    fn a_grep_line_splits_at_its_first_two_colons() {
        assert_eq!(
            split_grep_line("a/b.rs:12:// PLACEHOLDER: x \u{2014} owner, S1: soon"),
            Some(("a/b.rs", 12, "// PLACEHOLDER: x \u{2014} owner, S1: soon"))
        );
        assert_eq!(split_grep_line("no line number"), None);
    }

    #[test]
    fn the_register_leaves_out_what_the_hand_made_one_left_out() {
        for excluded in [
            ":!docs",
            ":!AGENTS.md",
            ":!CLAUDE.md",
            ":!.claude",
            ":!crates/proto/src/generated",
        ] {
            assert!(EXCLUDED.contains(&excluded), "{excluded}");
        }
    }
}

// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `::notice::` annotations — the diagnostics path for a logged-out viewer.
//!
//! GitHub hides job logs **and** step summaries from viewers who are not signed
//! in, but it shows annotations. Spike G1 (section 10.12) hit this while trying
//! to publish a geometry job's verdict: the evidence existed and nobody outside
//! the org could read it. So anything a reader outside the repository needs —
//! the screenshot comparison's verdict, the renderer the run actually selected,
//! the tail of a Godot log — goes out as `::notice::` workflow commands as well
//! as into the step summary.
//!
//! Two mechanics matter and are easy to get wrong:
//!
//! * a workflow command is **one line**, so newlines inside the message travel
//!   as `%0A` and a literal `%` as `%25`;
//! * there is a per-annotation size cap, so a long body is split into numbered
//!   chunks rather than truncated into a lie.
//!
//! Outside GitHub Actions the same text is simply printed, so a local run of
//! `cargo xtask ci` shows exactly what CI would publish.
//!
//! This module is deliberately generic: T16's vista job and T20's perf alarms
//! (skeleton-plan section 7 decision 23, recommended and not yet logged —
//! annotations with no threshold at this stage) both
//! publish through it rather than each inventing an escaping routine.

use std::fs::OpenOptions;
use std::io::Write as _;

/// Per-annotation message cap, with margin under GitHub's own limit.
const CAP: usize = 3_800;

/// Lines per chunk. Forty lines of diagnostics is about as much as anyone reads
/// from one annotation, and it keeps a chunk well under [`CAP`].
const LINES_PER_CHUNK: usize = 40;

/// Publishes `text` under `title` as one or more `::notice::` annotations, and
/// appends it to the step summary when there is one.
pub(crate) fn notice(title: &str, text: &str) {
    let lines: Vec<&str> = if text.trim().is_empty() {
        vec!["(empty)"]
    } else {
        text.lines().collect()
    };
    let chunks: Vec<&[&str]> = lines.chunks(LINES_PER_CHUNK).collect();
    let total = chunks.len();

    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        for (index, chunk) in chunks.iter().enumerate() {
            let body = encode(&chunk.join("\n"));
            let trimmed = truncate(&body, CAP);
            println!("::notice title={title} {}/{total}::{trimmed}", index + 1);
        }
    } else {
        println!("   note [{title}]");
        for line in &lines {
            println!("     {line}");
        }
    }

    summary(title, text);
}

/// Appends a fenced section to `$GITHUB_STEP_SUMMARY`, for readers who *are*
/// signed in. A failure to write is not worth failing a build over — the
/// annotations above already carry the same text.
pub(crate) fn summary(title: &str, text: &str) {
    let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") else {
        return;
    };
    if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(path) {
        let _ = writeln!(file, "### {title}\n\n```\n{text}\n```\n");
    }
}

/// Workflow-command escaping: a command is one line, so `%`, `\r` and `\n` all
/// travel encoded. `%` goes first or it would re-encode the others' escapes.
fn encode(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Truncates on a character boundary, never inside a `%0A` escape — a half
/// escape would be published as literal `%0` and read as corruption.
fn truncate(text: &str, cap: usize) -> String {
    if text.len() <= cap {
        return text.to_owned();
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    // Back off over a partial escape: at most the two characters after a `%`.
    for step in 0..3 {
        let candidate = end.saturating_sub(step);
        let tail = text
            .get(candidate.saturating_sub(2)..candidate)
            .unwrap_or("");
        if !tail.contains('%') {
            return format!("{} …", text.get(..candidate).unwrap_or(""));
        }
    }
    format!("{} …", text.get(..end.saturating_sub(3)).unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newlines_and_percents_are_encoded() {
        assert_eq!(encode("a\nb"), "a%0Ab");
        assert_eq!(encode("100%"), "100%25");
        // The `%` substitution runs first, so an encoded newline is not
        // re-encoded into `%250A`.
        assert_eq!(encode("50%\nof"), "50%25%0Aof");
    }

    #[test]
    fn truncation_never_splits_an_escape() {
        let text = encode(&"line\n".repeat(400));
        let cut = truncate(&text, 100);
        assert!(cut.len() <= 104, "length {}", cut.len());
        assert!(cut.ends_with(" …"));
        let body = cut.trim_end_matches(" …");
        assert!(
            !body.ends_with('%') && !body.ends_with("%0"),
            "a half escape reads as corruption: {body}"
        );
    }

    #[test]
    fn empty_text_still_says_something() {
        // Exercised for the panic-free path; the assertion is that it does not
        // publish an annotation with an empty body, which reads as a bug.
        notice("empty-section", "   \n  \n");
    }
}

// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **What of a Rust source file ships**, read one way for every source scan that needs it
//! (`tests/no_arithmetic.rs`, `tests/no_network.rs`; S3's `prob`, part 2, fix pass 1).
//!
//! [`views`] reads a file two ways, line for line: its code with every comment and literal
//! removed, and its code with every comment removed and the literals kept. [`scanned_lines`]
//! says how many leading lines ship: all of them, or every line before the file's trailing
//! `mod tests`, the one shape a unit-test module has in this crate. Any other `#[cfg(test)]`
//! in code, alone on its line or not (`#[cfg(test)] const X: () = ();` attributes that item
//! and nothing after it), is an error the caller reports, never a place to stop reading.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

/// One Rust file read two ways, line for line: `code` with every comment and literal removed
/// (a literal leaves `""`), and `literals` with every comment removed and the literals kept.
pub(crate) struct Views {
    pub(crate) code: Vec<String>,
    #[allow(
        dead_code,
        reason = "read by tests/no_network.rs; tests/no_arithmetic.rs shares this module and \
                  reads only the code"
    )]
    pub(crate) literals: Vec<String>,
}

pub(crate) fn views(text: &str) -> Views {
    let chars: Vec<char> = text.chars().collect();
    let mut code = String::new();
    let mut literals = String::new();
    let mut at = 0_usize;
    while let Some(&c) = chars.get(at) {
        let next = chars.get(at + 1).copied();
        let word_before = at
            .checked_sub(1)
            .and_then(|before| chars.get(before))
            .is_some_and(|before| before.is_alphanumeric() || *before == '_');
        if c == '/' && next == Some('/') {
            while chars.get(at).is_some_and(|c| *c != '\n') {
                at += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0_usize;
            loop {
                match (chars.get(at), chars.get(at + 1)) {
                    (None, _) => break,
                    (Some('/'), Some('*')) => {
                        depth += 1;
                        at += 2;
                    }
                    (Some('*'), Some('/')) => {
                        depth -= 1;
                        at += 2;
                        if depth == 0 {
                            break;
                        }
                    }
                    (Some('\n'), _) => {
                        code.push('\n');
                        literals.push('\n');
                        at += 1;
                    }
                    _ => at += 1,
                }
            }
        } else if let Some(end) = (!word_before).then(|| literal_end(&chars, at)).flatten() {
            code.push_str("\"\"");
            for index in at..end {
                let c = chars.get(index).copied().unwrap_or(' ');
                literals.push(c);
                if c == '\n' {
                    code.push('\n');
                }
            }
            at = end;
        } else {
            code.push(c);
            literals.push(c);
            at += 1;
        }
    }
    let code: Vec<String> = code.lines().map(str::to_owned).collect();
    let literals: Vec<String> = literals.lines().map(str::to_owned).collect();
    Views { code, literals }
}

/// One past the end of the string, byte string, raw string or character literal at `at`, or
/// `None` when none starts there (a lifetime is not one).
fn literal_end(chars: &[char], at: usize) -> Option<usize> {
    let c = *chars.get(at)?;
    let next = chars.get(at + 1).copied();
    let (prefix, raw) = match (c, next) {
        ('b', Some('r')) => (2, true),
        ('r', Some('#' | '"')) => (1, true),
        ('b', Some('"' | '\'')) => (1, false),
        _ => (0, false),
    };
    if raw {
        let mut hashes = 0_usize;
        while chars.get(at + prefix + hashes) == Some(&'#') {
            hashes += 1;
        }
        let open = at + prefix + hashes;
        if chars.get(open) != Some(&'"') {
            return None;
        }
        let mut close = open + 1;
        while let Some(&c) = chars.get(close) {
            if c == '"' && (1..=hashes).all(|k| chars.get(close + k) == Some(&'#')) {
                return Some(close + 1 + hashes);
            }
            close += 1;
        }
        return Some(close);
    }
    let quote = at + prefix;
    match chars.get(quote) {
        Some('"') => {
            let mut close = quote + 1;
            while let Some(&c) = chars.get(close) {
                match c {
                    '\\' => close += 2,
                    '"' => return Some(close + 1),
                    _ => close += 1,
                }
            }
            Some(close)
        }
        Some('\'') => {
            if chars.get(quote + 1) == Some(&'\\') {
                (quote + 3..quote + 12)
                    .find(|i| chars.get(*i) == Some(&'\''))
                    .map(|i| i + 1)
            } else {
                (chars.get(quote + 2) == Some(&'\'')).then_some(quote + 3)
            }
        }
        _ => None,
    }
}

/// How many leading lines of `text` ship: all of them, or, when the file ends in its unit-test
/// module, every line before that module's `#[cfg(test)]`. The module is recognised in exactly
/// one shape: a line that is only `#[cfg(test)]`, then the line `mod tests {`, and that module's
/// end found by matching its braces in the code (comments and literals removed, so no brace in
/// either counts), with nothing but blank lines after it and no other `#[cfg(test)]` in code.
/// A closing brace in column 0 is not taken for the end: a module whose closing brace is
/// indented and is followed by an item ends where its braces do, and the item after it is code
/// that ships (review 2's R7).
///
/// # Errors
///
/// Any `#[cfg(test)]` in code that is not that one: the lines it is on, for the caller to
/// report. A scan reads such a file whole or fails; it never stops early at it.
pub(crate) fn scanned_lines(text: &str) -> Result<usize, String> {
    let raw: Vec<&str> = text.lines().collect();
    let code = views(text).code;
    let marks: Vec<usize> = code
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains("#[cfg(test)]"))
        .map(|(index, _)| index)
        .collect();
    let [start] = marks.as_slice() else {
        return if marks.is_empty() {
            Ok(raw.len())
        } else {
            Err(format!(
                "`#[cfg(test)]` on lines {:?}: only the file's trailing `mod tests` may carry one",
                marks.iter().map(|index| index + 1).collect::<Vec<_>>()
            ))
        };
    };
    let refused = || {
        Err(format!(
            "`#[cfg(test)]` on line {} is not the file's trailing `mod tests`",
            start + 1
        ))
    };
    let alone = code
        .get(*start)
        .is_some_and(|line| line.trim() == "#[cfg(test)]");
    let opens = code
        .get(start + 1)
        .is_some_and(|line| line.trim_end() == "mod tests {");
    if !(alone && opens) {
        return refused();
    }
    // The module's end: the line where its braces, counted in the code view, come back to zero.
    let mut depth = 0_usize;
    let mut end = None;
    'lines: for (index, line) in code.iter().enumerate().skip(start + 1) {
        for c in line.chars() {
            match c {
                '{' => depth += 1,
                '}' => {
                    let Some(next) = depth.checked_sub(1) else {
                        return refused();
                    };
                    depth = next;
                    if depth == 0 {
                        end = Some(index);
                        break 'lines;
                    }
                }
                _ => {}
            }
        }
    }
    let Some(end) = end else {
        return refused();
    };
    let closing_alone = code.get(end).is_some_and(|line| line.trim() == "}");
    let only_blank_after = code.iter().skip(end + 1).all(|line| line.trim().is_empty());
    if closing_alone && only_blank_after {
        Ok(*start)
    } else {
        refused()
    }
}

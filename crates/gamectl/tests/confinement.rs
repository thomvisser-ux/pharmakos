// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What this crate may not do, asserted over its own source text.
//!
//! The lints catch most of it and cannot catch any of this: a crate that
//! reaches past the gateway into the sim's `Runner` compiles cleanly, and so
//! does one that quietly grows a second copy of a user-facing string. The
//! gateway keeps a file of this shape for the same reason
//! (`crates/gateway/tests/confinement.rs`), and T13b's review found that its
//! needle was the wrong spelling — so these are chosen to be the spellings
//! somebody would actually write.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::path::{Path, PathBuf};

/// Every source file of this crate, named rather than listed.
///
/// `std::fs::read_dir` is a disallowed method (`clippy.toml`: directory order
/// differs between ext4, NTFS and APFS), and a checker that walked the tree
/// would in any case pass on a crate with a new file in it. A module added
/// without a line here fails `every_module_is_checked`.
const SOURCES: &[&str] = &[
    "src/cli.rs",
    "src/docs.rs",
    "src/doctor.rs",
    "src/exit.rs",
    "src/host.rs",
    "src/lib.rs",
    "src/lobby.rs",
    "src/main.rs",
    "src/rules.rs",
    "src/scenario.rs",
    "src/scenario/nightly.rs",
    "src/scenario/run.rs",
    "src/scenario/seeds.rs",
    "src/schema.rs",
    "src/seat.rs",
    "src/strings.rs",
    "src/verify.rs",
];

/// The sim's fixture API (`World`'s block comment: "not game actions, and
/// nothing outside a test, a scenario fixture or the host's lobby layout may
/// call them"), by method name: shipped code reaches a match's state through
/// the game's own paths, never these (S3's `lic`, its fix pass).
const SIM_FIXTURES: &[(&str, &str)] = &[
    (
        "award_licence_directly",
        "a licence is earned at a settlement, never handed over (S3's `lic`)",
    ),
    (
        "place_beacon_directly",
        "a beacon is placed by a deploy, never by a fixture",
    ),
    (
        "raise_structure",
        "a structure is bought through the Quartermaster and raised by drones or a fabricator",
    ),
    (
        "field_unit",
        "a unit is fabricated by its beacon, never fielded by hand",
    ),
    (
        "field_movers_directly",
        "the movers are a test's load, never a match's",
    ),
    (
        "switch_mandate_directly",
        "a writ changes on site, by a visit",
    ),
    (
        "place_unit_directly",
        "a unit walks; it is never put somewhere",
    ),
    (
        "mark_explored_directly",
        "a Survey scout explores; nothing marks a cell by hand",
    ),
    (
        "set_treasury",
        "the treasury moves by spending and income alone",
    ),
    ("set_dormant", "dormancy is the power phase's"),
    ("send_unit_directly", "a unit is sent by its program"),
    (
        "set_build_settings_directly",
        "Build settings change on site, by a visit",
    ),
    (
        "set_site_funds_directly",
        "site funds are the Quartermaster's",
    ),
];

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn source(relative: &str) -> String {
    let path = crate_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

#[test]
fn every_module_is_checked() {
    for relative in SOURCES {
        assert!(
            crate_root().join(relative).is_file(),
            "{relative} is on the list and is not in the crate"
        );
    }
    // And the list is the whole of `mod`: a module declared in lib.rs and
    // missing from SOURCES would be unchecked.
    let lib = source("src/lib.rs");
    for line in lib.lines() {
        let Some(name) = line
            .strip_prefix("pub mod ")
            .and_then(|rest| rest.strip_suffix(';'))
        else {
            continue;
        };
        let file = format!("src/{name}.rs");
        assert!(
            SOURCES.contains(&file.as_str()),
            "`pub mod {name};` is declared and {file} is not on this file's list"
        );
    }
}

/// Decisions-log item 106 (3): "the crate map gives `gamectl` no edge to the
/// sim, so never name the sim's `Runner`".
///
/// The crate *does* take `pharmakos-sim`, and must: the gateway's public API is
/// written in the sim's types, so `Surface::new` cannot be called without
/// naming a `RulesTable` and `Host::open` without a `WorldConfig`. The line the
/// crate is held to is the one item 106 (3) draws — host through the same
/// `Surface` and `Host`, never around them — and these are the spellings that
/// would go around them.
///
/// # What this does *not* forbid, since a review asked
///
/// The crate reads the runner and the world: `crate::scenario::run` calls
/// `Host::runner()` for the tick a Lull is standing on (`tick()`), the phase a
/// refused `begin_push` was in and whether a match played to its end has ended
/// (`phase()`), the round a seal belongs to, a refusal is named in and a
/// nightly match reached (`round()`) and how the match ended (`outcome()`),
/// and `Host::world()` for the final audit's tied seats on a shared win
/// (`audit::final_audit`, a pure read) and for the Attack table's rows
/// (`attack().rows()`), which the nightly result's hunt count reads after
/// every tick (S3's `nite3`). `.runner()` is deliberately **not** a
/// needle, because the guard is against *driving* a `Runner`, not against
/// naming one — and `Host::runner` hands out a `&Runner` and `Host::world` a
/// `&World`, whose every stepping method needs `&mut`, so the reach cannot
/// grow into a second way to play a match. If `Surface` gains accessors for
/// those values, the runner takes them and `.runner()` joins the list below.
#[test]
fn nothing_here_drives_a_match_except_through_the_gateway() {
    const FORBIDDEN: &[(&str, &str)] = &[
        (
            "Runner::",
            "a match is stepped by the gateway's `Host`, which is the only place in the project \
             that drives a `Runner` (decisions-log item 103 (9), item 106 (3))",
        ),
        (
            "runner::Runner",
            "same rule, the other spelling: importing the type is how reaching for it starts",
        ),
        (
            "seal_playbook",
            "the sim's own seal. A playbook reaches a match through `submit_plan` or, for a \
             scenario the verifier refuses, through this crate's one documented harness door -- \
             and both of those go through `Surface`",
        ),
        (
            "world_mut",
            "a mutable world outside the tick is how a harness starts writing sim state",
        ),
        (
            "fork(",
            "`fork` lives behind the sim's `research` feature and nothing shipped may reach it \
             (AGENTS.md section 3 rule 1)",
        ),
    ];
    for relative in SOURCES {
        let text = source(relative);
        for (needle, why) in FORBIDDEN {
            assert!(!text.contains(needle), "{relative} names `{needle}`: {why}");
        }
    }
}

/// `gamectl`'s command runs on a stated stack (S3's `vfyq2`, fix pass 2, item
/// E): `main` starts it on `pharmakos_plan_core::stack::player_text_thread`,
/// so neither the platform's main-thread size (1 MiB on Windows) nor
/// `RUST_MIN_STACK` decides how deep a player's text may be read, and no
/// other source makes a thread any other way.
///
/// The gateway's guard, as `probw` left it (`crates/gateway/tests/
/// confinement.rs`; S3's `nite3`): an allow-list over the whole of each
/// file's shipped text, not a list of banned spellings read a line at a
/// time. Any whole-word `thread` in the shipped code, outside every string,
/// byte string, raw string (`r#"..."#`, `br#"..."#`) and character literal
/// and every line and block comment, each of which may span lines, is
/// refused unless it is written `thread::JoinHandle`, the handle
/// `player_text_thread`'s builder answers and `main` joins. So
/// `std::thread::scope`, an imported `Builder`, a braced import over several
/// lines, a renamed module, a spaced path, a local binding named `thread`,
/// and a spawn after a literal or a comment that ends on its line are all
/// refused, and a spawn written inside a string or a comment is not code.
#[test]
fn the_command_runs_on_the_stated_stack() {
    let main = source("src/main.rs");
    assert!(
        code_only(&main).contains("player_text_thread("),
        "src/main.rs starts the command on player_text_thread, in code"
    );
    let mut findings: Vec<String> = Vec::new();
    for relative in SOURCES {
        let text = source(relative);
        // The shipped code: everything before the first test module, read
        // whole, so a literal or a comment that spans lines is read as one.
        let shipped = text
            .find("#[cfg(test)]")
            .and_then(|index| text.get(..index))
            .unwrap_or(&text);
        for (number, why) in thread_offences(shipped) {
            let line = shipped.lines().nth(number.saturating_sub(1)).unwrap_or("");
            findings.push(format!("{relative}:{number}: {why}\n    {}", line.trim()));
        }
    }
    assert!(
        findings.is_empty(),
        "every thread gamectl makes is made by player_text_thread:\n{}",
        findings.join("\n")
    );
}

/// Every way round the stack a review of the gateway's guard found, each a
/// line the guard must refuse, and the spellings it must let through.
#[test]
fn the_thread_guard_refuses_every_way_round_the_stack() {
    for text in [
        "    let handle = std::thread::spawn(move || run());",
        "    thread::spawn(move || run());",
        "    let built = std::thread::Builder::new().stack_size(1);",
        "    std::thread::scope(|s| { s.spawn(|| run()); });",
        "    thread::scope(|s| { s.spawn(|| run()); });",
        "use std::thread::Builder;",
        "use std::thread::{Builder, JoinHandle};",
        "use std::thread;",
        "use std::{fs, thread};",
        "use std::thread as t;",
        "    let _ = ::std::thread::available_parallelism();",
        // The first review of #212: lines of a braced `use` written over
        // several lines, a character literal that holds a quote, a spaced
        // path.
        "use std::{\n    fs,\n    thread as t,\n};",
        "use std::{\n    fs,\n    thread,\n};",
        "    let q = '\"'; std::thread::spawn(f);",
        "    std::thread\t::spawn(f);",
        "    let q = '\\''; thread::spawn(f);",
        "    let thread = player_text_thread(\"x\");",
        "    let s = r#\"a \" quote\"#; std::thread::spawn(f);",
        // The second review: a block comment holding a quote, the end of a
        // string that began on an earlier line, byte and byte raw strings.
        "    /* \" */ std::thread::spawn(f);",
        "    let s = \"a\nline\"; std::thread::spawn(f);",
        "    let s = br#\"a \" b\"#; std::thread::spawn(f);",
        "    let s = b\"x\"; std::thread::spawn(f);",
        "    let c = b'\"'; std::thread::spawn(f);",
        "    /* one /* nested */ still */ std::thread::spawn(f);",
    ] {
        assert!(
            thread_offence(text).is_some(),
            "the guard let through: {text}"
        );
    }
    for text in [
        "use std::thread::JoinHandle;",
        "    let mut threads: Vec<JoinHandle<()>> = Vec::new();",
        "    let work = pharmakos_plan_core::stack::player_text_thread(\"gamectl\")",
        "    let outcome = match work.map(std::thread::JoinHandle::join) {",
        "    threads.push(accepter);",
        ") -> Result<std::thread::JoinHandle<()>, Error> {",
        "                \"this host could not start a connection thread: {}\",",
        "    \"a \\\"quoted\\\" thread\" ; let threads = 1;",
        "    let s = r#\"a thread\"#; // the surface thread",
        "fn f<'a>(x: &'a str) -> &'a str { x }",
        // A spawn faked inside a comment or a literal is not code.
        "    /* std::thread::spawn(f); */",
        "    /*\n    std::thread::spawn(f);\n    */",
        "    let s = \"\n    std::thread::spawn(f);\n\";",
        "    let s = br#\"\n    std::thread::spawn(f);\n\"#;",
        "    let s = b\"std::thread::spawn(f)\";",
    ] {
        assert_eq!(thread_offence(text), None, "the guard refused: {text}");
    }
}

/// Why a piece of text names `thread` other than as the stated stack's
/// handle: each finding's line, counting from 1, and its reason. The
/// gateway's guard, copied: two crates' tests cannot share a helper, and the
/// two are held to the same cases.
fn thread_offences(text: &str) -> Vec<(usize, &'static str)> {
    let code = code_only(text);
    let mut out = Vec::new();
    for (number, line) in code.lines().enumerate() {
        let mut from = 0_usize;
        while let Some(offset) = line.get(from..).and_then(|rest| find_word(rest, "thread")) {
            let start = from.saturating_add(offset);
            let end = start.saturating_add("thread".len());
            if !line.get(end..).unwrap_or("").starts_with("::JoinHandle") {
                out.push((
                    number.saturating_add(1),
                    "names `thread` other than as `thread::JoinHandle`: make every thread with \
                     player_text_thread",
                ));
                break;
            }
            from = end;
        }
    }
    out
}

/// [`thread_offences`] of one piece of text, its first finding.
fn thread_offence(text: &str) -> Option<&'static str> {
    thread_offences(text).first().map(|(_, why)| *why)
}

/// The byte offset of the first whole-word `word` in `text`: one with no
/// letter, digit or underscore on either side.
fn find_word(text: &str, word: &str) -> Option<usize> {
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let bytes = text.as_bytes();
    let mut from = 0_usize;
    while let Some(offset) = text.get(from..).and_then(|rest| rest.find(word)) {
        let start = from.saturating_add(offset);
        let end = start.saturating_add(word.len());
        let before = start
            .checked_sub(1)
            .and_then(|index| bytes.get(index))
            .copied()
            .is_some_and(is_word);
        let after = bytes.get(end).copied().is_some_and(is_word);
        if !before && !after {
            return Some(start);
        }
        from = start.saturating_add(1);
    }
    None
}

/// `text`'s code, line for line: every literal (string, byte string, raw and
/// byte raw string, character and byte character) reduced to an empty pair
/// of quotes and every comment (line, and block, nested) removed, each
/// keeping the newlines it spans so a finding names its line. A lifetime
/// (`'a`) is not a literal and is kept. An unterminated literal or comment
/// runs to the end of the text, as the compiler would read it.
fn code_only(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let at_char = |index: usize| chars.get(index).copied();
    let mut out = String::with_capacity(text.len());
    let mut at = 0_usize;
    while let Some(c) = at_char(at) {
        let next = at_char(at.saturating_add(1));
        let word_before = at
            .checked_sub(1)
            .and_then(at_char)
            .is_some_and(|before| before.is_alphanumeric() || before == '_');
        let skipped = if c == '/' && next == Some('/') {
            Some((line_comment_end(&chars, at), ""))
        } else if c == '/' && next == Some('*') {
            Some((block_comment_end(&chars, at), ""))
        } else if word_before {
            None
        } else {
            literal_end(&chars, at)
        };
        if let Some((end, kept)) = skipped {
            out.push_str(kept);
            for index in at..end {
                if at_char(index) == Some('\n') {
                    out.push('\n');
                }
            }
            at = end;
        } else {
            out.push(c);
            at = at.saturating_add(1);
        }
    }
    out
}

/// Where the line comment starting at `at` ends: at its newline, kept.
fn line_comment_end(chars: &[char], at: usize) -> usize {
    let mut end = at;
    while chars.get(end).is_some_and(|c| *c != '\n') {
        end = end.saturating_add(1);
    }
    end
}

/// One past the end of the block comment starting at `at`, nested comments
/// included; the text's end for one never closed.
fn block_comment_end(chars: &[char], at: usize) -> usize {
    let mut end = at;
    let mut depth = 0_usize;
    loop {
        match (chars.get(end), chars.get(end.saturating_add(1))) {
            (None, _) => return end,
            (Some('/'), Some('*')) => {
                depth = depth.saturating_add(1);
                end = end.saturating_add(2);
            }
            (Some('*'), Some('/')) => {
                depth = depth.saturating_sub(1);
                end = end.saturating_add(2);
                if depth == 0 {
                    return end;
                }
            }
            _ => end = end.saturating_add(1),
        }
    }
}

/// The literal starting at `at`, if one does: one past its end, and what
/// stands in for it (an empty pair of its quotes). A string, byte string,
/// raw or byte raw string, character or byte character; `None` for anything
/// else, a lifetime included.
fn literal_end(chars: &[char], at: usize) -> Option<(usize, &'static str)> {
    let at_char = |index: usize| chars.get(index).copied();
    let c = at_char(at)?;
    let next = at_char(at.saturating_add(1));
    let (prefix, raw) = match (c, next) {
        ('b', Some('r')) => (2, true),
        ('r', _) => (1, true),
        ('b', Some('"' | '\'')) => (1, false),
        _ => (0, false),
    };
    if raw {
        let mut hashes = 0_usize;
        while at_char(at.saturating_add(prefix).saturating_add(hashes)) == Some('#') {
            hashes = hashes.saturating_add(1);
        }
        let open = at.saturating_add(prefix).saturating_add(hashes);
        if at_char(open) != Some('"') {
            return None;
        }
        let mut close = open.saturating_add(1);
        while let Some(c) = at_char(close) {
            if c == '"' && (1..=hashes).all(|k| at_char(close.saturating_add(k)) == Some('#')) {
                return Some((close.saturating_add(1).saturating_add(hashes), "\"\""));
            }
            close = close.saturating_add(1);
        }
        return Some((close, "\"\""));
    }
    let quote = at.saturating_add(prefix);
    match at_char(quote) {
        Some('"') => {
            let mut close = quote.saturating_add(1);
            while let Some(c) = at_char(close) {
                match c {
                    '\\' => close = close.saturating_add(2),
                    '"' => return Some((close.saturating_add(1), "\"\"")),
                    _ => close = close.saturating_add(1),
                }
            }
            Some((close, "\"\""))
        }
        Some('\'') => {
            // A character literal: one character or one escape, then a quote.
            let close = if at_char(quote.saturating_add(1)) == Some('\\') {
                (quote.saturating_add(3)..quote.saturating_add(12))
                    .find(|i| at_char(*i) == Some('\''))
            } else {
                (at_char(quote.saturating_add(2)) == Some('\'')).then_some(quote.saturating_add(2))
            };
            close.map(|close| (close.saturating_add(1), "''"))
        }
        _ => None,
    }
}

/// `gamectl` is not a walled crate (AGENTS.md §4.9), so it reads no clock and
/// contains no float. The lints deny most of this; a `Duration` on a socket
/// option would not be denied and is not here either.
#[test]
fn there_is_no_clock_and_no_float() {
    const FORBIDDEN: &[&str] = &[
        "Instant",
        "SystemTime",
        "std::time",
        "Duration",
        "f32",
        "f64",
        "HashMap",
        "HashSet",
    ];
    for relative in SOURCES {
        let text = source(relative);
        for needle in FORBIDDEN {
            assert!(
                !text.contains(needle),
                "{relative} names `{needle}`: gamectl is a deterministic crate on the near side \
                 of the wall, and a CLI that timed its own run would be the first wall-clock read \
                 in one"
            );
        }
    }
}

/// AGENTS.md §11: no `connect`, and nothing that would become one.
#[test]
fn nothing_here_attaches_to_a_seat_from_outside() {
    const FORBIDDEN: &[(&str, &str)] = &[
        (
            "TcpStream",
            "a client connection. `seat doctor` asks whether the loopback addresses accept a \
             bind, which is a question about this machine; opening one would be `connect` by \
             another name (AGENTS.md section 11)",
        ),
        (
            "http://",
            "no shipped crate gets an HTTP client: playtest bundles are files a tester sends by \
             hand and there is no network telemetry (AGENTS.md section 7)",
        ),
        (
            "0.0.0.0",
            "the gateway binds 127.0.0.1 and ::1 and nothing else, with no flag that widens it",
        ),
    ];
    for relative in SOURCES {
        let text = source(relative);
        for (needle, why) in FORBIDDEN {
            assert!(!text.contains(needle), "{relative} names `{needle}`: {why}");
        }
    }
    // And `connect` appears in exactly one place: the sentence that says there
    // is not one.
    let table = source("src/cli.rs");
    assert!(
        !table.contains("name: \"connect\""),
        "the command table has no row for it"
    );
}

/// AGENTS.md §12, as much of it as a source-text check can hold: **no message
/// names the binary outside the string table.**
///
/// Named for what it does rather than for what one would like it to do. A
/// review found the earlier name — `every_user_facing_string_is_in_the_string_
/// table` — claiming a guarantee this cannot make: the doctor's `detail` lines
/// and the scenario format's diagnostics are written beside the conditions they
/// describe, and `src/strings.rs`'s header now says so. What *is* enforceable
/// is the tell: a message addressed to a person from this binary names the
/// binary, and the binary's name appearing in a `format!` outside `strings.rs`
/// is a message that grew somewhere else.
///
/// Both spellings are needles, because `strings::BINARY` is the other way to
/// write it and the earlier single needle let `src/cli.rs` past.
#[test]
fn no_message_names_the_binary_outside_the_string_table() {
    for relative in SOURCES {
        if *relative == "src/strings.rs" {
            continue;
        }
        let text = source(relative);
        for (number, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            // Doc comments and ordinary comments are not strings anybody
            // prints, and they talk about the binary constantly.
            if trimmed.starts_with("//") {
                continue;
            }
            let names_it = line.contains("gamectl") || line.contains("strings::BINARY");
            assert!(
                !(line.contains("format!") && names_it),
                "{relative}:{}: a message naming the binary is built outside the string table:\n  \
                 {line}",
                number.saturating_add(1)
            );
        }
    }
}

/// The scenario runner writes fresh outputs; nothing in this crate writes into
/// the repository's committed goldens.
///
/// Checked at the only place a write can happen rather than by looking for the
/// word: one module calls `std::fs::write`, and the path it is given comes from
/// `actual_for`, which strips `expected.` and produces `actual.`. A producer
/// that could write its own expectation is a producer that cannot fail
/// (`tests/golden/README.md` rule 4).
#[test]
fn nothing_here_writes_a_committed_golden() {
    let mut writers: Vec<&str> = Vec::new();
    for relative in SOURCES {
        let text = source(relative);
        for line in text.lines() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if line.contains("fs::write") || line.contains("fs::copy") {
                if !writers.contains(relative) {
                    writers.push(relative);
                }
                assert!(
                    !line.contains("expected."),
                    "{relative} writes to a committed golden:\n  {line}"
                );
            }
        }
    }
    assert_eq!(
        writers,
        vec!["src/scenario/nightly.rs", "src/scenario/run.rs"],
        "two modules write a file: `run.rs` the fresh chain the `golden` step compares, and \
         `nightly.rs` the result a run over a seed file writes to `--out`. A third writer is a \
         third place a golden could be written from"
    );
    assert!(
        source("src/lib.rs").contains("strip_prefix(\"expected.\")"),
        "`actual_for` is what turns a committed golden's path into the fresh one beside it"
    );
}

/// The research-feature ban, as far as this crate can enforce it on itself: a
/// second guard beside the research guard's walk.
///
/// `cargo xtask ci`'s research guard walks `cargo tree -e features` for every
/// crate on `GUARDED_PACKAGES` in `xtask/src/main.rs`, and `gamectl` joined that
/// list and `WALL_GUARDED_PACKAGES` at decisions-log item 109, so the walk is
/// what catches a *transitive* enablement (some dependency turning `research`
/// on further down the graph), which a manifest read cannot see. This test
/// reads the manifest text only: it keeps the sim edge in the form AGENTS.md
/// section 3 rule 1 names, and a developer running `cargo test` meets it before
/// the suite's `research-guard` step.
#[test]
fn the_crate_takes_the_sim_with_default_features_off() {
    let manifest = std::fs::read_to_string(crate_root().join("Cargo.toml")).expect("the manifest");
    assert!(
        manifest.contains("pharmakos-sim       = { workspace = true, default-features = false }"),
        "the sim is taken with `default-features = false`, in the form AGENTS.md \
         section 3 rule 1 writes it:\n{manifest}"
    );
    assert!(
        !manifest.contains("[features]") && !manifest.contains("features = [\"research\"]"),
        "and this crate declares no features of its own and never names the sim's"
    );
    let workspace = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("the workspace root")
            .join("Cargo.toml"),
    )
    .expect("the workspace manifest");
    assert!(
        workspace.contains("pharmakos-sim       = { path = \"crates/sim\", version = \"0\", default-features = false }"),
        "and that entry still has default-features off"
    );
}

/// The fallbacks that stay, each with the rule that makes its answer exact
/// (S2's plan, engineering call E7, swept for this crate by task `night`).
///
/// `(file, the line's code, why)`. The scan below finds **every** fallback
/// call in the shipped code (`unwrap_or`, `unwrap_or_default`,
/// `unwrap_or_else`, `map_or`, `map_or_else`), as the verifier's twin of this
/// test does, so each default this crate reads in place of a value is either
/// rewritten or written down here with its reason.
///
/// **Its limit:** it reads method calls, so a default written as a `match`
/// arm (`_ => Vec::new()`) is invisible to it. The sweep read those by hand:
/// the `report_hash`, a diagnostic's code and its path in `scenario/run.rs`
/// now refuse what is missing or misshapen (an absent path is proto3's empty
/// root pointer, by the wire's rule); `message_of`'s `_` arm renders the
/// whole error rather than defaulting; and the readers' `_ => None` and
/// `_ => problems.at(..)` arms each record a problem or name a refusal. Rewritten in the sweep:
/// the operands split with `split_first` rather than read past a checked
/// first, the segment's ticks read through the sim's own `Ms::to_ticks_floor`,
/// the seat and segment indices compared as `usize`, a seat past a `u8` not
/// read, the chain's reservation computed in `u64` below its cap, the first
/// `builtin` seat's index and a feed shorter than what was read each an
/// internal error, a `report_hash` that is not base64 refused (the site
/// `vfy1` found), the doctor's comment count a failed check when the document
/// does not parse, and an unreadable working directory an input error.
const KEPT_FALLBACKS: &[(&str, &str, &str)] = &[
    (
        "cli.rs",
        "let root = root.unwrap_or_else(|| cwd.to_path_buf());",
        "the documented default: without `--root` every path is read against the working \
         directory (`OPTIONS`' row says so)",
    ),
    (
        "cli.rs",
        "depth: depth.unwrap_or(Depth::Full),",
        "the documented default: FULL, which is what `submit_plan` runs (spec section 11)",
    ),
    (
        "cli.rs",
        "format: options.format.unwrap_or(Format::Text),",
        "the documented default: `verify --format text`, the report for a person (`OPTIONS`' row \
         says so)",
    ),
    (
        "cli.rs",
        "part: part.unwrap_or_default(),",
        "the documented default: no `--part` is the whole playbook, which `schema::run` reads \
         as the empty name",
    ),
    (
        "scenario.rs",
        "let name = string_at(json, \"name\", \"/name\", problems).unwrap_or_default();",
        "never read: a missing name is a problem recorded by `string_at`, and `load` returns no \
         scenario while any problem stands",
    ),
    (
        "scenario.rs",
        "round_limit.unwrap_or(default_limit)",
        "never read: an absent key is already the default, and a bad one is a problem \
         recorded above, so `load` returns no scenario",
    ),
    (
        "nightly.rs",
        "let operator = options.operator.unwrap_or(Operator::Easy);",
        "the documented default: a `builtin` seat plays Easy, the one operator this build has",
    ),
    (
        "nightly.rs",
        "let rendered = match options.format.unwrap_or(Format::Text) {",
        "the documented default: `--format text`",
    ),
    (
        "run.rs",
        ".unwrap_or_else(|| expected.len().min(actual.len()));",
        "exact: with no differing line in the common prefix, the first difference is where the \
         shorter chain ends",
    ),
    (
        "run.rs",
        "expected.get(at).copied().unwrap_or(\"<end of file>\"),",
        "a message's words: the committed chain has no line at the first difference because it \
         ended there",
    ),
    (
        "run.rs",
        "actual.get(at).copied().unwrap_or(\"<end of run>\"),",
        "as above, for the run's side",
    ),
];

/// The file's shipped code: comment-only lines removed, and everything from
/// the first `#[cfg(test)]` on, because a unit test may fall back as it likes.
fn code_lines(text: &str) -> Vec<String> {
    let shipped = match text.find("#[cfg(test)]") {
        Some(index) => text.get(..index).unwrap_or(text),
        None => text,
    };
    shipped
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .map(str::to_owned)
        .collect()
}

/// Every fallback call in the crate's shipped code, as `(file name, line)`.
fn fallbacks() -> Vec<(String, String)> {
    let mut found = Vec::new();
    for relative in SOURCES {
        let name = Path::new(relative)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        for line in code_lines(&source(relative)) {
            if line.contains("unwrap_or")
                || line.contains("map_or(")
                || line.contains("map_or_else(")
            {
                found.push((name.clone(), line.trim().to_owned()));
            }
        }
    }
    found
}

/// S2's plan, E7: no silent fallback is left in `gamectl`. A predictable
/// failure returns a typed error, a conversion or a read that cannot fail is
/// written so its failure cannot be written, and a default that is a rule is
/// listed in [`KEPT_FALLBACKS`] with the rule.
#[test]
fn no_silent_fallback_is_left() {
    let found = fallbacks();
    let unexplained: Vec<String> = found
        .iter()
        .filter(|(file, line)| {
            !KEPT_FALLBACKS
                .iter()
                .any(|(kept, code, _)| kept == file && line.contains(code))
        })
        .map(|(file, line)| format!("{file}: {line}"))
        .collect();
    assert!(
        unexplained.is_empty(),
        "a value falls back to a default instead of saying what went wrong. Return a typed \
         error, make the failure unrepresentable, or, where the default is a rule that leaves \
         the answer exact, add the line to KEPT_FALLBACKS with that rule:\n{}",
        unexplained.join("\n")
    );
    for (file, code, why) in KEPT_FALLBACKS {
        let count = found
            .iter()
            .filter(|(found_file, line)| found_file == file && line.contains(code))
            .count();
        assert_eq!(
            count, 1,
            "KEPT_FALLBACKS' entry in {file} (`{code}`, kept because {why}) matches {count} \
             lines; an entry that matches nothing is stale, and one that matches two hides one"
        );
    }
}

#[test]
fn the_fallback_scan_finds_what_it_looks_for() {
    assert_eq!(
        fallbacks().len(),
        KEPT_FALLBACKS.len(),
        "the scan sees as many fallbacks as the list keeps"
    );
    let probe = "fn f(v: &[u8]) -> u8 {\n    // v.first().copied().unwrap_or(0)\n    v.first().copied().unwrap_or(0)\n}\n#[cfg(test)]\nmod tests { fn g() { None::<u8>.unwrap_or(1); } }\n";
    let lines: Vec<String> = code_lines(probe)
        .into_iter()
        .filter(|line| line.contains("unwrap_or"))
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "the code line is seen; the comment and the test are not"
    );
}

#[test]
fn no_shipped_code_calls_a_sim_fixture() {
    for relative in SOURCES {
        let text = source(relative);
        let shipped = text
            .find("#[cfg(test)]")
            .and_then(|at| text.get(..at))
            .unwrap_or(text.as_str());
        for (needle, why) in SIM_FIXTURES {
            assert!(
                !shipped.contains(needle),
                "{relative} names `{needle}`: {why}"
            );
        }
    }
}

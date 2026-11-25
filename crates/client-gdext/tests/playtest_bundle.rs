// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **The Playtest bundle holds what it says and nothing else** (S3's `prob`, part 2; the plan's
//! E10 and R6; AGENTS.md section 7): "Export playtest bundle" writes one file the tester sends by
//! hand, holding the survey, the metrics and the client's own diagnostics log, and never a
//! playbook, a draft, the notebook, the private replay, a path or a user name.
//!
//! - **The entry list:** the zip holds exactly `playtest::BUNDLE_ENTRIES`, which Rust builds; no
//!   script packs a zip or writes the bundle itself.
//! - **Taint:** a real watch rig is driven through a Lull, a Push, a recap and into round 2 by a
//!   stand-in gateway whose every answer is laced with marker strings (a refusal quoting a path
//!   and a player's id, a notebook, a draft's label and text, a recap's prose, feed events, the
//!   verifier's pointers and messages), with the editor holding a playbook that carries markers
//!   of its own; the recorder is fed exactly as the bridge feeds it. No marker, drive-letter path,
//!   home folder or `user://` reaches any of the four files.
//! - **The type-level half:** no public function of `playtest` takes free text but the survey's
//!   answer and the readers that turn text into a closed code.
//! - **The log's grammar:** every diagnostics line has the closed shape, and the shape checker is
//!   itself tested on lines that must and must not pass it.
//! - **Opt-in and local:** off by default, nothing kept while off, switching off clears, nothing
//!   written before Export, and Export writes exactly one file, at the path chosen.
//! - **Bounded:** a recorder filled past every cap counts what it dropped, says so in the
//!   manifest, and its bundle fits `MAX_BUNDLE_BYTES`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that \
              configuration only recognises #[test] functions and #[cfg(test)] modules — \
              not an integration test's helper functions. A panic is this file's failure \
              report (clippy.toml's own wording)."
)]

use std::fs;
use std::path::{Path, PathBuf};

use pharmakos_client_gdext::pacer::{LullEnd, PushTime, SPEEDS};
use pharmakos_client_gdext::playtest::{
    Another, BUNDLE_ENTRIES, BUNDLE_FORMAT, BundleRefusal, CameraMove, Code, DropReason, Event,
    ExportFailure, MAX_BUNDLE_BYTES, MAX_CODES, MAX_LOG_LINES, MAX_LULL_RECORDS, MAX_PUSH_RECORDS,
    MAX_SURVEY_TEXT_CHARS, PushOutcome, Recorded, Recorder, SurveyRefusal, Understood,
};
use pharmakos_client_gdext::rig::{ADMIN, LullLengths, Outgoing, Phase, Rig, SEAT};
use pharmakos_proto::json::{self, Json};

// --- A reader of the zip, independent of the writer ---------------------------------------

/// The CRC-32 zip stores (IEEE 802.3, reflected), written out here so the reader checks the
/// writer rather than itself.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// `len` bytes of `bytes` from `at`; a read past the end fails the test.
fn span(bytes: &[u8], at: usize, len: usize) -> &[u8] {
    bytes
        .get(at..at + len)
        .unwrap_or_else(|| panic!("{len} bytes at {at} are past the zip's end"))
}

fn u16_at(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes(
        span(bytes, at, 2).try_into().expect("two bytes"),
    ))
}

fn u32_at(bytes: &[u8], at: usize) -> usize {
    usize::try_from(u32::from_le_bytes(
        span(bytes, at, 4).try_into().expect("four bytes"),
    ))
    .expect("fits")
}

/// The zip's entries, `(name, data)`, in central-directory order: every entry stored, its CRC,
/// sizes and name matching between its local header and the central directory, and nothing in
/// the file before, between or after them.
fn entries(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
    let end = zip.len() - 22;
    assert_eq!(u32_at(zip, end), 0x0605_4b50, "the end record is last");
    assert_eq!(u16_at(zip, end + 20), 0, "no archive comment");
    let count = u16_at(zip, end + 10);
    let central_size = u32_at(zip, end + 12);
    let central = u32_at(zip, end + 16);
    assert_eq!(
        central + central_size,
        end,
        "the central directory ends at the end record"
    );
    let mut out = Vec::new();
    let mut at = central;
    let mut local_end = 0;
    for _ in 0..count {
        assert_eq!(u32_at(zip, at), 0x0201_4b50);
        assert_eq!(u16_at(zip, at + 10), 0, "stored, not compressed");
        let crc = u32_at(zip, at + 16);
        let size = u32_at(zip, at + 20);
        assert_eq!(u32_at(zip, at + 24), size, "stored: the two sizes agree");
        let name_len = u16_at(zip, at + 28);
        assert_eq!(u16_at(zip, at + 30), 0, "no extra field");
        assert_eq!(u16_at(zip, at + 32), 0, "no entry comment");
        let offset = u32_at(zip, at + 42);
        let name = String::from_utf8(span(zip, at + 46, name_len).to_vec()).expect("a name");
        assert_eq!(offset, local_end, "the entries follow one another");
        assert_eq!(u32_at(zip, offset), 0x0403_4b50);
        assert_eq!(u16_at(zip, offset + 26), name_len);
        assert_eq!(u16_at(zip, offset + 28), 0);
        assert_eq!(span(zip, offset + 30, name_len), name.as_bytes());
        assert_eq!(u32_at(zip, offset + 14), crc);
        let data_at = offset + 30 + name_len;
        let data = span(zip, data_at, size).to_vec();
        assert_eq!(
            crc32(&data),
            u32::try_from(crc).expect("fits"),
            "{name}'s CRC"
        );
        local_end = data_at + size;
        out.push((name, data));
        at += 46 + name_len;
    }
    assert_eq!(
        local_end, central,
        "nothing between the entries and the directory"
    );
    out
}

/// The bundle's files by name, read back from its zip.
fn files_of(zip: &[u8]) -> Vec<(String, String)> {
    entries(zip)
        .into_iter()
        .map(|(name, data)| (name, String::from_utf8(data).expect("text")))
        .collect()
}

fn file<'a>(files: &'a [(String, String)], name: &str) -> &'a str {
    files
        .iter()
        .find(|(entry, _)| entry == name)
        .map_or_else(|| panic!("no {name}"), |(_, text)| text.as_str())
}

fn json_of(text: &str) -> Json {
    json::read(text).expect("JSON")
}

fn number_at(value: &Json, key: &str) -> u64 {
    match value.get(key) {
        Some(Json::Number(lexeme)) => lexeme.parse().expect("a whole number"),
        other => panic!("`{key}` is not a number: {other:?}"),
    }
}

fn array_at<'a>(value: &'a Json, key: &str) -> &'a [Json] {
    match value.get(key) {
        Some(Json::Array(items)) => items,
        other => panic!("`{key}` is not an array: {other:?}"),
    }
}

// --- The diagnostics log's grammar ----------------------------------------------------------

const PHASES: &[&str] = &["unknown", "lobby", "lull", "push", "recap", "ended"];

fn digits(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|c| c.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'))
}

fn is_code(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Whether `line` is one diagnostics line: `<seq> <phase> <event>[ <code>] t=<ms>`.
fn has_the_shape(line: &str) -> bool {
    let words: Vec<&str> = line.split(' ').collect();
    let (seq, phase, event, code, stamp) = match words.as_slice() {
        [seq, phase, event, stamp] => (*seq, *phase, *event, None, *stamp),
        [seq, phase, event, code, stamp] => (*seq, *phase, *event, Some(*code), *stamp),
        _ => return false,
    };
    digits(seq)
        && seq != "0"
        && PHASES.contains(&phase)
        && Event::ALL.iter().any(|known| known.name() == event)
        && code.is_none_or(is_code)
        && stamp.strip_prefix("t=").is_some_and(digits)
}

/// The text a bundle must never hold, outside the survey's own words.
fn path_shaped(text: &str) -> Vec<&'static str> {
    let lower = text.to_ascii_lowercase();
    let mut found = Vec::new();
    let chars: Vec<char> = lower.chars().collect();
    if chars.windows(3).any(
        |window| matches!(window, [letter, ':', '/' | '\u{5c}'] if letter.is_ascii_alphabetic()),
    ) {
        found.push("a drive letter's path");
    }
    for needle in [
        "\\users\\",
        "/home/",
        "/users/",
        "user://",
        "res://",
        "\\appdata",
    ] {
        if lower.contains(needle) {
            found.push(needle_name(needle));
        }
    }
    found
}

fn needle_name(needle: &str) -> &'static str {
    match needle {
        "\\users\\" => "\\Users\\",
        "/home/" => "/home/",
        "/users/" => "/Users/",
        "user://" => "user://",
        "res://" => "res://",
        _ => "\\AppData",
    }
}

#[test]
fn the_shape_checker_catches_what_it_is_for() {
    for line in [
        "1 lull lull_opened t=0",
        "12 push refused PHASE_CLOSED t=4031",
        "3 unknown playtest_on t=0",
        "9 ended export_failed NOT_WRITABLE t=120000",
    ] {
        assert!(has_the_shape(line), "a good line refused: {line}");
    }
    for line in [
        "",
        "0 lull lull_opened t=0",
        "01 lull lull_opened t=0",
        "1 lull lull_opened",
        "1 lull lull_opened t=",
        "1 lull lull_opened t=1 extra",
        "1 Lull lull_opened t=0",
        "1 lull opened t=0",
        "1 lull refused C:\\Users\\Marker t=0",
        "1 lull refused /home/marker t=0",
        "1 lull refused user://x t=0",
        "1 lull refused phase_closed t=0",
        "1 lull refused PHASE CLOSED t=0",
        "1 lull refused \"quoted\" t=0",
        "1 lull lull_opened t=0 C:\\Users\\Marker",
    ] {
        assert!(!has_the_shape(line), "a bad line passed: {line}");
    }
    for text in [
        "C:\\Users\\marker",
        "d:/build",
        "/home/marker",
        "/Users/marker",
        "user://logs",
    ] {
        assert!(
            !path_shaped(text).is_empty(),
            "the path check missed {text}"
        );
    }
    assert!(path_shaped("{\n  \"a\": 1\n}").is_empty());
}

// --- Off by default, local, cleared ---------------------------------------------------------

/// A fresh folder for one test, under cargo's per-target temporary folder.
fn folder(name: &str) -> PathBuf {
    let at = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("playtest_bundle")
        .join(name);
    if at.exists() {
        fs::remove_dir_all(&at).expect("an old test folder goes");
    }
    fs::create_dir_all(&at).expect("a test folder");
    at
}

/// The names in `at`, sorted.
fn listing(at: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(at)
        .expect("a folder")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn nothing_is_kept_while_off_and_switching_off_clears() {
    let mut recorder = Recorder::new();
    assert!(!recorder.is_on(), "off at every launch");
    assert_eq!(recorder.phase(0, Phase::Lull, 1, None), Recorded::Off);
    assert_eq!(recorder.ready(1), Recorded::Off);
    assert_eq!(recorder.skip(2), Recorded::Off);
    assert_eq!(recorder.speed(3, 2), Recorded::Off);
    assert_eq!(recorder.focus(4, false), Recorded::Off);
    assert_eq!(recorder.camera(CameraMove::Zoom), Recorded::Off);
    assert_eq!(
        recorder.refused(5, &Code::of_refusal("PHASE_CLOSED: x")),
        Recorded::Off
    );
    assert_eq!(
        recorder.rows(1, [Code::of_verifier("E0104")]),
        Recorded::Off
    );
    assert_eq!(
        recorder.survey(6, Another::Yes, Understood::Yes, "words"),
        Err(SurveyRefusal::Off)
    );
    assert!(recorder.summary().is_none());
    assert_eq!(recorder.bundle(), Err(BundleRefusal::Off));
    let at = folder("off");
    let chosen = at.join("pharmakos-playtest.zip");
    assert!(matches!(
        recorder.export(7, &chosen),
        Err(ExportFailure::Refused(BundleRefusal::Off))
    ));
    assert!(listing(&at).is_empty(), "nothing written while off");

    assert_eq!(
        recorder.switch(true, 10, true, Phase::Unknown, 0),
        Recorded::Taken
    );
    recorder.phase(20, Phase::Lull, 1, None);
    recorder.ready(30);
    recorder.phase(40, Phase::Push, 1, None);
    recorder.camera(CameraMove::Pan);
    recorder.phase(50, Phase::Recap, 1, None);
    let summary = recorder.summary().expect("on");
    assert_eq!((summary.pushes, summary.lulls), (1, 1));

    assert_eq!(
        recorder.switch(false, 60, true, Phase::Recap, 1),
        Recorded::Off
    );
    assert!(!recorder.is_on());
    assert!(
        recorder.summary().is_none(),
        "switched off, nothing is kept"
    );
    assert!(recorder.push_records().is_none() && recorder.lull_records().is_none());

    recorder.switch(true, 70, true, Phase::Recap, 1);
    let files = files_of(&recorder.bundle().expect("on"));
    assert_eq!(
        file(&files, "diagnostics.log"),
        "1 recap playtest_on t=0\n",
        "switched on again, it starts from nothing"
    );
    let metrics = json_of(file(&files, "metrics.json"));
    assert_eq!(number_at(&metrics, "pushes"), 0);
    assert!(array_at(&metrics, "lull_time").is_empty());
}

#[test]
fn the_export_writes_exactly_one_file_where_the_tester_chose() {
    let at = folder("export");
    let chosen = at.join("pharmakos-playtest.zip");
    let mut recorder = Recorder::new();
    recorder.switch(true, 0, true, Phase::Unknown, 0);
    recorder.phase(1_000, Phase::Lull, 1, None);
    assert!(listing(&at).is_empty(), "nothing is written before Export");
    let bundle = recorder.bundle().expect("on");
    let exported = recorder.export(5_000, &chosen).expect("written");
    assert_eq!(listing(&at), ["pharmakos-playtest.zip"]);
    assert_eq!(fs::read(&chosen).expect("the file"), bundle);
    assert_eq!(exported.bytes, bundle.len());

    // A second export replaces the file whole; the first export's line is in it.
    recorder.export(6_000, &chosen).expect("written again");
    assert_eq!(listing(&at), ["pharmakos-playtest.zip"]);
    let files = files_of(&fs::read(&chosen).expect("the file"));
    assert!(
        file(&files, "diagnostics.log").contains(" export_done t=5\n"),
        "{}",
        file(&files, "diagnostics.log")
    );

    // A folder that is not there: nothing is written anywhere, and the failure is a typed line.
    let missing = at.join("no such folder").join("bundle.zip");
    assert!(matches!(
        recorder.export(7_000, &missing),
        Err(ExportFailure::Write(_))
    ));
    assert_eq!(listing(&at), ["pharmakos-playtest.zip"]);
    let files = files_of(&recorder.bundle().expect("on"));
    let log = file(&files, "diagnostics.log");
    let failed = log
        .lines()
        .find(|line| line.contains("export_failed"))
        .expect("the failure is a line");
    assert!(has_the_shape(failed), "{failed}");
    assert!(
        path_shaped(log).is_empty(),
        "the failure's path is not in the log: {log}"
    );
}

// --- The entry list --------------------------------------------------------------------------

#[test]
fn the_bundle_holds_exactly_its_four_entries() {
    assert_eq!(
        BUNDLE_ENTRIES,
        [
            "manifest.json",
            "survey.json",
            "metrics.json",
            "diagnostics.log"
        ]
    );
    let mut recorder = Recorder::new();
    recorder.switch(true, 0, true, Phase::Unknown, 0);
    let zip = recorder.bundle().expect("on");
    let names: Vec<String> = entries(&zip).into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, BUNDLE_ENTRIES);
    let files = files_of(&zip);
    let manifest = json_of(file(&files, "manifest.json"));
    assert_eq!(
        number_at(&manifest, "bundle_format"),
        u64::from(BUNDLE_FORMAT)
    );
    assert_eq!(BUNDLE_FORMAT, 1);
    let listed: Vec<String> = array_at(&manifest, "files")
        .iter()
        .map(|name| match name {
            Json::String(name) => name.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(listed, BUNDLE_ENTRIES);
    assert_eq!(
        manifest.get("recording"),
        Some(&Json::String("absent until S7".to_owned()))
    );
    // Every reason something could be dropped is listed, at zero.
    let Some(Json::Object(dropped)) = manifest.get("dropped") else {
        panic!("no dropped");
    };
    let reasons: Vec<&str> = dropped.iter().map(|(name, _)| name.as_str()).collect();
    let all: Vec<&str> = DropReason::ALL.iter().map(|reason| reason.name()).collect();
    assert_eq!(reasons, all);
    assert!(
        dropped
            .iter()
            .all(|(_, count)| *count == Json::Number("0".to_owned()))
    );
    // No time, no path, no build id in the manifest: its only numbers are the format, the caps
    // and the counts.
    assert!(path_shaped(file(&files, "manifest.json")).is_empty());
}

fn godot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("godot")
}

fn scripts() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = fs::read_dir(godot_dir().join("scripts"))
        .expect("godot/scripts")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "gd"))
        .map(|path| {
            (
                path.file_name()
                    .expect("a name")
                    .to_string_lossy()
                    .into_owned(),
                fs::read_to_string(&path).expect("a script"),
            )
        })
        .collect();
    found.sort();
    found
}

/// The lines of a GDScript file with each `#` comment removed (outside a string).
fn gd_code(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| {
            let mut quote: Option<char> = None;
            let mut out = String::new();
            for c in line.chars() {
                match quote {
                    Some(open) if c == open => quote = None,
                    None if c == '"' || c == '\'' => quote = Some(c),
                    None if c == '#' => break,
                    Some(_) | None => {}
                }
                out.push(c);
            }
            out
        })
        .collect()
}

/// Rust builds the bundle's bytes and writes them; no script packs a zip, and the Playtest
/// script reaches the disk through the bridge's export alone.
#[test]
fn no_script_packs_or_writes_the_bundle() {
    let all = scripts();
    assert!(all.len() > 20, "the scan found the scripts: {}", all.len());
    for (name, text) in &all {
        for line in gd_code(text) {
            assert!(
                !line.contains("ZIPPacker"),
                "{name} packs a zip: the bundle's bytes are Rust's (playtest::Recorder::bundle)"
            );
        }
    }
    let (_, playtest) = all
        .iter()
        .find(|(name, _)| name == "playtest.gd")
        .expect("scripts/playtest.gd");
    let code = gd_code(playtest);
    let exports = code
        .iter()
        .filter(|line| line.contains("playtest_export("))
        .count();
    assert_eq!(exports, 1, "the bundle is exported in one place");
    for banned in [
        "file_write(",
        "file_remove(",
        "FileAccess",
        "DirAccess",
        "ZIPReader",
        "OS.",
        "user://",
    ] {
        assert!(
            !code.iter().any(|line| line.contains(banned)),
            "playtest.gd names `{banned}`: it reads and writes no file of its own"
        );
    }
}

/// `playtest.rs` writes through the client's one atomic writer, in `export` alone, and reads no
/// file: no log, no cache, no save.
#[test]
fn the_recorder_writes_only_at_export_and_reads_nothing() {
    let source = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("playtest.rs"),
    )
    .expect("src/playtest.rs");
    let code: Vec<&str> = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect();
    let writes: Vec<&&str> = code
        .iter()
        .filter(|line| line.contains("write_whole"))
        .collect();
    assert_eq!(writes.len(), 1, "{writes:?}");
    for banned in [
        "std::fs",
        "fs::",
        "File::",
        "OpenOptions",
        "read_to_string",
        "read_dir",
        "remove_whole",
        "std::env",
        "env::",
        "current_dir",
        "home_dir",
    ] {
        assert!(
            !code.iter().any(|line| line.contains(banned)),
            "playtest.rs names `{banned}`"
        );
    }
}

// --- The type-level half --------------------------------------------------------------------

/// Every public function's parameters, `(function, parameter, type)`, from the source.
fn public_parameters(source: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut rest = source;
    while let Some(at) = [rest.find("pub fn "), rest.find("pub const fn ")]
        .into_iter()
        .flatten()
        .min()
    {
        let after = &rest[at..];
        let name_from = after.find("fn ").expect("fn") + 3;
        let open = after.find('(').expect("a parameter list");
        let name = after[name_from..open]
            .split('<')
            .next()
            .expect("a name")
            .trim()
            .to_owned();
        let mut depth = 0_usize;
        let mut close = open;
        for (index, c) in after[open..].char_indices() {
            match c {
                '(' | '<' | '[' => depth += 1,
                ')' | '>' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        close = open + index;
                        break;
                    }
                }
                _ => {}
            }
        }
        let list = &after[open + 1..close];
        let mut level = 0_usize;
        let mut piece = String::new();
        let mut pieces = Vec::new();
        for c in list.chars() {
            match c {
                '<' | '(' | '[' => level += 1,
                '>' | ')' | ']' => level -= 1,
                ',' if level == 0 => {
                    pieces.push(std::mem::take(&mut piece));
                    continue;
                }
                _ => {}
            }
            piece.push(c);
        }
        pieces.push(piece);
        for piece in pieces {
            let piece = piece.trim();
            if piece.is_empty() || piece.contains("self") {
                continue;
            }
            let (parameter, kind) = piece.split_once(':').expect("name: type");
            out.push((
                name.clone(),
                parameter.trim().to_owned(),
                kind.trim().to_owned(),
            ));
        }
        rest = &after[close..];
    }
    out
}

/// The readers that may take text: each turns it into a closed value, or keeps the survey's
/// answer as written.
const TEXT_TAKERS: &[(&str, &str)] = &[
    ("of_refusal", "said"),
    ("of_verifier", "code"),
    ("of_name", "name"),
    ("survey", "text"),
];

#[test]
fn no_public_function_takes_free_text_but_the_named_readers() {
    let source = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("playtest.rs"),
    )
    .expect("src/playtest.rs");
    let parameters = public_parameters(&source);
    assert!(
        parameters
            .iter()
            .any(|(function, _, _)| function == "export"),
        "the reader found the functions: {parameters:?}"
    );
    for (function, parameter, kind) in &parameters {
        let textual = kind.contains("str")
            || kind.contains("String")
            || kind.contains("Json")
            || kind.contains("u8]")
            || kind.contains("Vec<u8>");
        if textual {
            assert!(
                TEXT_TAKERS.contains(&(function.as_str(), parameter.as_str())),
                "`{function}` takes `{parameter}: {kind}`: the recorder takes typed facts only \
                 (S3's plan, R6)"
            );
        }
    }
    // The reader is tested: it sees a text parameter where there is one.
    let sample = "pub fn leak(&mut self, now_us: u64, playbook: &str) -> Recorded {";
    assert_eq!(
        public_parameters(sample),
        [
            ("leak".to_owned(), "now_us".to_owned(), "u64".to_owned()),
            ("leak".to_owned(), "playbook".to_owned(), "&str".to_owned())
        ]
    );
}

// --- Codes --------------------------------------------------------------------------------------

#[test]
fn a_code_is_from_a_closed_set_or_unnamed() {
    assert_eq!(
        Code::of_refusal("PHASE_CLOSED: the Lull is over").as_str(),
        "PHASE_CLOSED"
    );
    assert_eq!(
        Code::of_refusal("INVALID_ARGUMENT: `C:\\Users\\Marker\\x.jsonc`: no").as_str(),
        "INVALID_ARGUMENT"
    );
    for said in [
        "MARKERUSER: shaped like a code, and not the gateway's",
        "MARKERUSER",
        "-32000: a numeric code",
        "ERROR_CODE_UNSPECIFIED: unspecified",
        "phase_closed: lower case",
        "C:\\Users\\Marker: a path",
        "",
    ] {
        assert_eq!(Code::of_refusal(said).as_str(), Code::UNNAMED, "{said}");
    }
    for code in ["E0104", "W0702", "I0003"] {
        assert_eq!(Code::of_verifier(code).as_str(), code);
    }
    for code in [
        "E010", "E01045", "X0104", "e0104", "E01O4", "MARKER", "E0104 ",
    ] {
        assert_eq!(Code::of_verifier(code).as_str(), Code::UNNAMED, "{code}");
    }
}

// --- What is recorded -----------------------------------------------------------------------

#[test]
fn a_skipped_push_carries_no_times_and_is_counted() {
    let mut recorder = Recorder::new();
    recorder.switch(true, 0, true, Phase::Unknown, 0);
    recorder.phase(0, Phase::Lull, 1, None);
    recorder.phase(10_000, Phase::Push, 1, None);
    recorder.focus(12_000, false);
    recorder.focus(15_000, true);
    recorder.phase(20_000, Phase::Recap, 1, None);
    recorder.phase(21_000, Phase::Lull, 2, None);
    recorder.ready(25_000);
    recorder.phase(26_000, Phase::Push, 2, None);
    assert_eq!(recorder.skip(27_000), Recorded::Taken);
    recorder.phase(28_000, Phase::Recap, 2, None);
    assert_eq!(
        recorder.skip(29_000),
        Recorded::NotInPhase,
        "a Skip outside a Push"
    );
    let outcomes: Vec<(u32, PushOutcome)> = recorder
        .push_records()
        .expect("on")
        .iter()
        .map(|record| (record.round, record.outcome))
        .collect();
    assert_eq!(
        outcomes,
        [
            (
                1,
                PushOutcome::Timed(PushTime {
                    focused_us: 7_000,
                    unfocused_us: 3_000
                })
            ),
            (2, PushOutcome::Skipped)
        ]
    );
    let lulls: Vec<(u32, u64, LullEnd)> = recorder
        .lull_records()
        .expect("on")
        .iter()
        .map(|record| (record.round, record.time.used_us, record.time.ended_by))
        .collect();
    assert_eq!(
        lulls,
        [(1, 10_000, LullEnd::PhaseLeft), (2, 4_000, LullEnd::Ready)]
    );

    let files = files_of(&recorder.bundle().expect("on"));
    let metrics = json_of(file(&files, "metrics.json"));
    assert_eq!(number_at(&metrics, "pushes"), 2);
    assert_eq!(number_at(&metrics, "skipped_pushes"), 1);
    let [timed, skipped] = array_at(&metrics, "push_time") else {
        panic!("two Pushes");
    };
    assert_eq!(number_at(timed, "focused_us"), 7_000);
    assert_eq!(number_at(timed, "unfocused_us"), 3_000);
    assert_eq!(skipped.get("skipped"), Some(&Json::Bool(true)));
    assert!(
        skipped.get("focused_us").is_none() && skipped.get("unfocused_us").is_none(),
        "a skipped Push carries no times: {skipped:?}"
    );
    // No threshold is written: the file records, the owner judges (item 153 (1)).
    for (_, text) in &files {
        for word in ["threshold", "target", "pass", "fail", "verdict", "majority"] {
            assert!(
                !text.to_ascii_lowercase().contains(word),
                "the bundle judges: {word}"
            );
        }
    }
}

#[test]
fn speeds_camera_rows_and_refusals_are_counted_by_kind() {
    let mut recorder = Recorder::new();
    recorder.switch(true, 0, true, Phase::Unknown, 0);
    recorder.phase(0, Phase::Push, 1, None);
    for speed in SPEEDS {
        assert_eq!(recorder.speed(1, speed), Recorded::Taken);
    }
    assert_eq!(recorder.speed(1, 4), Recorded::Taken);
    assert_eq!(recorder.speed(1, 3), Recorded::Refused, "3x is no speed");
    for kind in CameraMove::ALL {
        recorder.camera(kind);
    }
    recorder.camera(CameraMove::Zoom);
    recorder.rows(1, [Code::of_verifier("E0104"), Code::of_verifier("E0104")]);
    recorder.rows(1, [Code::of_verifier("E0104")]);
    recorder.rows(2, [Code::of_verifier("E0104"), Code::of_verifier("E0303")]);
    recorder.refused(2, &Code::of_refusal("RATE_LIMITED: slow down"));
    recorder.refused(3, &Code::of_refusal("RATE_LIMITED: slow down"));
    recorder.refused(4, &Code::of_refusal("not a code"));
    let files = files_of(&recorder.bundle().expect("on"));
    let metrics = json_of(file(&files, "metrics.json"));
    let object = |key: &str| -> Vec<(String, u64)> {
        let Some(Json::Object(entries)) = metrics.get(key) else {
            panic!("{key}");
        };
        entries
            .iter()
            .map(|(name, value)| {
                let Json::Number(lexeme) = value else {
                    panic!("{value:?}")
                };
                (name.clone(), lexeme.parse().expect("a count"))
            })
            .collect()
    };
    assert_eq!(
        object("speed_changes"),
        [
            ("1".to_owned(), 1),
            ("2".to_owned(), 1),
            ("4".to_owned(), 2)
        ]
    );
    assert_eq!(
        object("camera_moves"),
        [
            ("pan".to_owned(), 1),
            ("zoom".to_owned(), 2),
            ("turn".to_owned(), 1),
            ("follow".to_owned(), 1)
        ]
    );
    assert_eq!(
        object("verifier_error_rows"),
        [("E0104".to_owned(), 2), ("E0303".to_owned(), 1)],
        "each code once per revision it is shown in"
    );
    assert_eq!(
        object("refusals"),
        [("RATE_LIMITED".to_owned(), 2), ("UNNAMED".to_owned(), 1)]
    );
    let log = file(&files, "diagnostics.log");
    assert!(log.contains(" push speed SPEED_4 t=0\n"), "{log}");
    assert!(log.contains(" push refused UNNAMED t=0\n"), "{log}");
    for line in log.lines() {
        assert!(has_the_shape(line), "{line}");
    }
}

#[test]
fn the_survey_text_goes_as_written_and_a_long_one_is_refused_whole() {
    let mut recorder = Recorder::new();
    recorder.switch(true, 0, true, Phase::Ended, 3);
    let files = files_of(&recorder.bundle().expect("on"));
    let survey = json_of(file(&files, "survey.json"));
    assert_eq!(
        survey.get("play_another_round"),
        Some(&Json::String("unanswered".to_owned())),
        "an unanswered question says so"
    );
    assert_eq!(
        survey.get("understood_why"),
        Some(&Json::String("unanswered".to_owned()))
    );
    assert_eq!(
        survey.get("wanted_to_change"),
        Some(&json_of(r#"{"answered":false}"#))
    );

    let words = "Pause, and \"aim\" at C:\\Users\\me — ✓\nline two";
    recorder
        .survey(1, Another::Yes, Understood::Partly, words)
        .expect("taken");
    let files = files_of(&recorder.bundle().expect("on"));
    let survey = json_of(file(&files, "survey.json"));
    assert_eq!(
        survey.get("play_another_round"),
        Some(&Json::String("yes".to_owned()))
    );
    assert_eq!(
        survey.get("understood_why"),
        Some(&Json::String("partly".to_owned()))
    );
    assert_eq!(
        survey.get("wanted_to_change").and_then(|w| w.get("text")),
        Some(&Json::String(words.to_owned())),
        "as the tester wrote it"
    );
    let manifest = file(&files, "manifest.json");
    assert!(manifest.contains("sent as written"), "{manifest}");

    let long = "x".repeat(MAX_SURVEY_TEXT_CHARS + 1);
    assert_eq!(
        recorder.survey(2, Another::No, Understood::No, &long),
        Err(SurveyRefusal::TooLong)
    );
    let files = files_of(&recorder.bundle().expect("on"));
    let survey = json_of(file(&files, "survey.json"));
    assert_eq!(
        survey.get("wanted_to_change").and_then(|w| w.get("text")),
        Some(&Json::String(words.to_owned())),
        "a refused answer changes nothing, and nothing is cut"
    );
    let at_the_cap = "é".repeat(MAX_SURVEY_TEXT_CHARS);
    recorder
        .survey(3, Another::No, Understood::No, &at_the_cap)
        .expect("the cap counts characters");
}

// --- Bounded --------------------------------------------------------------------------------

/// Every cap filled and passed, with the longest numbers a reading can make: what was dropped
/// is counted and said, and the bundle fits its bound.
#[test]
fn a_full_recorder_says_what_it_dropped_and_fits_its_bound() {
    let mut recorder = Recorder::new();
    let origin = 1_u64 << 40;
    recorder.switch(true, origin, true, Phase::Unknown, 0);
    let mut now = origin;
    let mut step = || {
        now += 1_000_000_007;
        now
    };
    for round in 0..=u32::try_from(MAX_PUSH_RECORDS).expect("fits") {
        recorder.phase(step(), Phase::Lull, round, None);
        recorder.ready(step());
        recorder.phase(step(), Phase::Push, round, None);
        recorder.focus(step(), false);
        recorder.focus(step(), true);
        recorder.phase(step(), Phase::Recap, round, None);
    }
    // One code a revision, so the tally's own cap is the one reached.
    for code in 0..=MAX_CODES {
        let revision = u64::try_from(code).expect("fits");
        recorder.rows(revision, [Code::of_verifier(&format!("E{code:04}"))]);
    }
    for _ in 0..MAX_LOG_LINES {
        recorder.refused(step(), &Code::of_refusal("UNSUPPORTED_SCHEMA_VERSION: x"));
    }
    let longest = "\u{1}".repeat(MAX_SURVEY_TEXT_CHARS);
    recorder
        .survey(step(), Another::Yes, Understood::Yes, &longest)
        .expect("at the cap");

    for reason in [
        DropReason::PushRecordsFull,
        DropReason::LullRecordsFull,
        DropReason::LogFull,
        DropReason::CodesFull,
    ] {
        assert!(
            recorder.dropped(reason).expect("on") > 0,
            "{reason:?} never reached"
        );
    }
    assert_eq!(recorder.push_records().expect("on").len(), MAX_PUSH_RECORDS);
    assert_eq!(recorder.lull_records().expect("on").len(), MAX_LULL_RECORDS);
    let zip = recorder.bundle().expect("fits");
    assert!(
        zip.len() <= MAX_BUNDLE_BYTES,
        "{} bytes past {MAX_BUNDLE_BYTES}",
        zip.len()
    );
    println!(
        "a full recorder's bundle: {} bytes of {MAX_BUNDLE_BYTES}",
        zip.len()
    );
    let files = files_of(&zip);
    let manifest = json_of(file(&files, "manifest.json"));
    let dropped = manifest.get("dropped").expect("dropped");
    assert_eq!(number_at(dropped, "push_records_full"), 1);
    assert_eq!(number_at(dropped, "lull_records_full"), 1);
    assert!(number_at(dropped, "log_full") > 0);
    assert_eq!(number_at(dropped, "codes_full"), 1);
    let metrics = json_of(file(&files, "metrics.json"));
    assert_eq!(
        number_at(&metrics, "pushes"),
        u64::try_from(MAX_PUSH_RECORDS).expect("fits") + 1,
        "a Push past the record cap is still counted"
    );
    let log = file(&files, "diagnostics.log");
    assert_eq!(log.lines().count(), MAX_LOG_LINES);
    for line in log.lines() {
        assert!(has_the_shape(line), "{line}");
    }
}

/// The editor's rows are told to the recorder every frame: a code past a cap is dropped once a
/// revision, never once a frame, and a new revision counts its codes again.
#[test]
fn a_dropped_code_is_counted_once_a_revision_not_once_a_frame() {
    let mut recorder = Recorder::new();
    recorder.switch(true, 0, true, Phase::Lull, 1);
    let codes = |from: usize, count: usize| -> Vec<Code> {
        (from..from + count)
            .map(|code| Code::of_verifier(&format!("E{code:04}")))
            .collect()
    };
    // A revision showing more distinct codes than a tally keeps, told over three frames.
    for _ in 0..3 {
        recorder.rows(1, codes(0, MAX_CODES + 40));
    }
    assert_eq!(
        recorder.dropped(DropReason::CodesFull),
        Some(1),
        "one revision past the cap is one drop, whatever the frames"
    );
    // The tally is full: each new code of a new revision is one drop, once, over three frames.
    for _ in 0..3 {
        recorder.rows(2, codes(MAX_CODES + 100, 5));
    }
    assert_eq!(recorder.dropped(DropReason::CodesFull), Some(6));
    // A code the tally holds is counted again in a new revision, and once there.
    for _ in 0..3 {
        recorder.rows(3, codes(0, 1));
    }
    let files = files_of(&recorder.bundle().expect("on"));
    let metrics = json_of(file(&files, "metrics.json"));
    let rows = metrics.get("verifier_error_rows").expect("rows");
    assert_eq!(number_at(rows, "E0000"), 2, "revisions 1 and 3");
    assert_eq!(number_at(rows, "E0001"), 1);
}

// --- Taint: a real rig, a stand-in gateway, and markers everywhere --------------------------

const MARKERS: &[&str] = &[
    "MarkerPlaybook",
    "MarkerNotebook",
    "MarkerDraftLabel",
    "MarkerDraftText",
    "MarkerRecapProse",
    "MarkerFeedText",
    "MarkerPointer",
    "MarkerMessage",
    "MarkerBeginner",
    "MarkerQuotedId",
    "MarkerUser",
    "MARKERUSER",
    "MarkerMatchId",
];

/// The stand-in: answers every call the rig makes, with markers in every word it may.
struct Laced {
    phase: &'static str,
    round: u32,
    all_ready: bool,
    advances: u32,
}

impl Laced {
    fn footer(&self) -> String {
        let clock = if self.phase == "push" {
            ""
        } else {
            r#","untimed":true"#
        };
        format!(
            r#"{{"phase":"{}","round":{}{clock}}}"#,
            self.phase, self.round
        )
    }

    fn refusal(id: &str, code: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32000,"message":"MarkerMessage: the step \u201cMarkerQuotedId\u2026\u201d at C:\\Users\\MarkerUser\\AppData\\x.jsonc and /home/MarkerUser","data":{{"code":"{code}"}}}}}}"#
        )
    }

    fn serve(&mut self, frame: &Outgoing) -> String {
        let request = json::read(&frame.text).expect("a request");
        let Some(Json::Number(id)) = request.get("id") else {
            panic!("an id");
        };
        let id = id.clone();
        let Some(Json::String(method)) = request.get("method") else {
            panic!("a method");
        };
        let result = match method.as_str() {
            "save_notes" => return Self::refusal(&id, "INVALID_ARGUMENT"),
            "save_draft" | "describe_beacon" | "get_beacon" => return Self::refusal(&id, "MARKERUSER"),
            "report_host_clock" => format!(r#"{{"all_ready":{}}}"#, self.all_ready),
            "set_ready" => {
                self.all_ready = true;
                "{}".to_owned()
            }
            "end_lull" => {
                self.phase = "push";
                self.all_ready = false;
                "{}".to_owned()
            }
            "advance_push" => {
                self.advances += 1;
                if self.advances >= 4 {
                    self.phase = "recap";
                }
                let asked = match request.get("params").and_then(|p| p.get("ms")) {
                    Some(Json::Number(ms)) => ms.clone(),
                    _ => "0".to_owned(),
                };
                format!(r#"{{"advanced_ms":{asked}}}"#)
            }
            "end_recap" => {
                self.phase = "lull";
                self.round += 1;
                "{}".to_owned()
            }
            "get_status" => r#"{"preset":"custom","sealed_this_round":false}"#.to_owned(),
            "get_view" => r#"{"next_cursor":"c1","complete":true}"#.to_owned(),
            "get_segment_feed" => r#"{"events":[{"at_ms":10,"kind":"step_started","text":"MarkerFeedText at C:\\Users\\MarkerUser"}],"next_cursor":"f1"}"#.to_owned(),
            "get_recap" => r#"{"prose":"MarkerRecapProse C:\\Users\\MarkerUser"}"#.to_owned(),
            "verify_plan" | "submit_plan" => r#"{"report":{"qualifies":false,"depth":"QUICK","diagnostics":[{"code":"E0104","severity":"ERROR","path":"/MarkerPointer","message":"MarkerMessage C:\\Users\\MarkerUser","beginner":"MarkerBeginner"},{"code":"MarkerUser","severity":"ERROR","path":"/x","message":"m"}]}}"#.to_owned(),
            "get_briefing" => r#"{"notes":"MarkerNotebook C:\\Users\\MarkerUser"}"#.to_owned(),
            "list_drafts" => r#"{"drafts":[{"draft_id":"MarkerDraftLabel","label":"MarkerDraftLabel","round":1}]}"#.to_owned(),
            "get_draft" => json::write(&Json::Object(vec![
                (
                    "playbook_jsonc".to_owned(),
                    Json::String(format!("// MarkerDraftText\n{}", playbook())),
                ),
                ("label".to_owned(), Json::String("MarkerDraftLabel".to_owned())),
                ("round".to_owned(), Json::Number("1".to_owned())),
            ])),
            "render_plan" => r#"{"prose":"MarkerPlaybook prose"}"#.to_owned(),
            _ => "{}".to_owned(),
        };
        let body = result
            .trim_end()
            .strip_suffix('}')
            .expect("a result is an object");
        let separator = if body.len() > 1 { "," } else { "" };
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{body}{separator}"_status":{}}}}}"#,
            self.footer()
        )
    }
}

fn playbook() -> String {
    format!(
        "// MarkerPlaybook C:\\Users\\MarkerUser MarkerMatchId\n{}",
        include_str!("../../../examples/playbooks/expand_east.jsonc")
    )
}

/// Plays round 1 and opens round 2's Lull through a real rig against the laced stand-in,
/// feeding `recorder` exactly as the bridge does: `Recorder::answered` on each answer,
/// `Recorder::shown` each frame, and each human command beside the rig's own.
fn play_a_laced_round(recorder: &mut Recorder) {
    let mut gateway = Laced {
        phase: "lull",
        round: 1,
        all_ready: false,
        advances: 0,
    };
    let mut rig = Rig::new();
    rig.set_lull_length(LullLengths::default());
    rig.opened(ADMIN);
    rig.opened(SEAT);
    rig.editor_mut().set_seat("seat.0");
    recorder.switch(true, 0, true, rig.phase(), rig.round());
    let text = playbook();

    let mut now = 0_u64;
    let mut loaded = false;
    let mut readied = false;
    let mut sped = false;
    let mut continued = false;
    let mut answers = 0_u32;
    for _ in 0..20_000 {
        now += 16_667;
        if rig.phase() == Phase::Lull && gateway.round == 1 && !loaded {
            loaded = true;
            assert!(rig.editor_mut().load(text.as_bytes()));
            rig.editor_mut()
                .save_notes("MarkerNotebook C:\\Users\\MarkerUser");
            rig.editor_mut().save_draft("MarkerDraftLabel");
            rig.editor_mut().describe_beacon("MarkerQuotedId");
        }
        if loaded && !readied && rig.editor().rows_current() && !rig.editor().busy() {
            readied = true;
            rig.ready();
            recorder.ready(now);
        }
        if rig.phase() == Phase::Push && !sped {
            sped = true;
            assert!(rig.set_speed(2));
            recorder.speed(now, 2);
            recorder.focus(now, false);
            recorder.camera(CameraMove::Turn);
        }
        if rig.phase() == Phase::Recap && rig.recap().answers > 0 && !continued {
            continued = true;
            recorder.focus(now, true);
            rig.end_recap();
        }
        for frame in rig.poll(now) {
            let answer_text = gateway.serve(&frame);
            if let Ok(answer) = rig.receive(frame.link, &answer_text) {
                answers += 1;
                recorder.answered(now, &rig, &answer);
            }
        }
        recorder.shown(rig.editor());
        if gateway.round == 2 && rig.phase() == Phase::Lull {
            break;
        }
    }
    assert!(continued, "the rig reached the recap and went on");
    assert_eq!(rig.phase(), Phase::Lull, "round 2's Lull");
    assert!(answers > 20, "the rig was answered: {answers}");
}

/// A whole round and the next Lull through a real rig, every answer laced: the recorder, fed
/// exactly as the bridge feeds it, keeps no marker.
#[test]
fn no_marker_reaches_the_bundle() {
    let mut recorder = Recorder::new();
    play_a_laced_round(&mut recorder);

    let zip = recorder.bundle().expect("on");
    let files = files_of(&zip);
    // The recorder did record: the drive is not empty.
    let metrics = json_of(file(&files, "metrics.json"));
    assert_eq!(number_at(&metrics, "pushes"), 1);
    assert_eq!(array_at(&metrics, "lull_time").len(), 1);
    let log = file(&files, "diagnostics.log");
    for line in log.lines() {
        assert!(has_the_shape(line), "{line}");
    }
    for expected in [
        " lull lull_opened ",
        " lull refused INVALID_ARGUMENT ",
        " lull refused UNNAMED ",
        " lull ready ",
        " push push_began ",
        " recap recap_opened ",
    ] {
        assert!(log.contains(expected), "no `{expected}` in:\n{log}");
    }
    assert!(
        file(&files, "metrics.json").contains("\"E0104\""),
        "the verifier's error rows are counted by code"
    );
    for (name, text) in &files {
        for marker in MARKERS {
            assert!(
                !text.contains(marker)
                    && !text
                        .to_ascii_lowercase()
                        .contains(&marker.to_ascii_lowercase()),
                "{name} holds `{marker}`:\n{text}"
            );
        }
        assert!(
            path_shaped(text).is_empty(),
            "{name} holds {:?}:\n{text}",
            path_shaped(text)
        );
    }
    // And the bytes of the zip itself, names and headers included.
    let lowered = String::from_utf8_lossy(&zip).to_ascii_lowercase();
    for marker in MARKERS {
        assert!(!lowered.contains(&marker.to_ascii_lowercase()), "{marker}");
    }
}

// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **The event list's contents compared against `get_segment_feed`'s JSON as a golden**
//! (skeleton plan T16, acceptance; kept unchanged by the T16a amendment).
//!
//! The live event list is a view of the segment feed and of nothing else: every row is one
//! event, in the feed's order, with the gateway's own kind and words. So the golden is the
//! rows the watch rig renders from a committed `get_segment_feed` result
//! (`tests/fixtures/get_segment_feed.json`, generated through the codec by
//! `tests/fixtures.rs`), read the way the live rig reads one — the lower-case enum spelling
//! translated back, the footer set aside — and written one row a line to
//! `tests/golden/vista/expected.events.txt`.
//!
//! A row the client added, dropped, merged, reordered or reworded shows up here as a diff.
//! Like the geometry digests, this area is compared by this test rather than by the
//! `golden` step (`tests/golden/vista/README.md`), and re-blessed with
//! `PHARMAKOS_BLESS_VISTA`.
//!
//! The Ledger's memos (S3's plan, E5) are checked on the gateway's own memo feed,
//! `godot/fixtures/feed_ledger_memo.json`, a byte copy of
//! `tests/golden/gateway/feed_ledger_memo/expected.response.json` that this file keeps
//! identical: the one event whose typed payload is a memo is the Ledger's row, with the
//! memo's own text, and every other row is an ordinary event's.

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

use pharmakos_client_gdext::enums;
use pharmakos_client_gdext::rig::{EventRow, Speaker, event_rows};
use pharmakos_client_gdext::view::split_footer;
use pharmakos_proto::gp::api::v1::{GetSegmentFeedResponse, event};
use pharmakos_proto::json;

const BLESS: &str = "PHARMAKOS_BLESS_VISTA";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The cargo target directory: the parent of `CARGO_TARGET_TMPDIR`, which cargo sets
/// absolute, so a relative `CARGO_TARGET_DIR` or a user-level `build.target-dir` cannot
/// mislead it (decisions-log items 165 (13) and 172 (8) (b)).
fn target_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .expect("CARGO_TARGET_TMPDIR always has a parent")
        .to_path_buf()
}

/// A committed `get_segment_feed` answer (a whole JSON-RPC answer), read the way the live
/// rig reads one.
fn feed_of(path: &Path) -> GetSegmentFeedResponse {
    let text = fs::read_to_string(path).expect("a committed feed answer");
    let frame = json::read(&text).expect("json");
    let result = frame.get("result").expect("a result");
    let (body, _) = split_footer(result);
    json::decode_json(&enums::canonical("gp.api.v1.GetSegmentFeedResponse", &body))
        .expect("a GetSegmentFeedResponse")
}

const MEMO_GOLDEN: &str = "tests/golden/gateway/feed_ledger_memo/expected.response.json";
const MEMO_COPY: &str = "godot/fixtures/feed_ledger_memo.json";

#[test]
fn the_memo_feed_fixture_is_the_gateways_golden() {
    let golden = fs::read(root().join(MEMO_GOLDEN)).expect("the gateway's memo feed golden");
    let copy = fs::read(root().join(MEMO_COPY)).expect("the client's copy");
    assert!(
        golden == copy,
        "{MEMO_COPY} is not a byte-identical copy of {MEMO_GOLDEN}: the gateway is the \
         producer, so copy its golden over the fixture in the pull request that moved it"
    );
}

/// The Ledger's row is the event whose typed payload is a memo, and no other: drawn with the
/// memo's own text and never as an ordinary row, while every other row keeps its kind.
#[test]
fn a_memo_in_the_feed_is_the_ledgers_row_and_no_other_is() {
    let feed = feed_of(&root().join(MEMO_COPY));
    let rows = event_rows(&feed).expect("every event is at a time in the segment");
    assert_eq!(rows.len(), feed.events.len());
    let mut memos = 0_usize;
    for (row, event) in rows.iter().zip(&feed.events) {
        match &event.payload {
            Some(event::Payload::LedgerMemo(memo)) => {
                memos += 1;
                assert_eq!(row.speaker, Speaker::Ledger, "{row:?}");
                assert_eq!(row.text, memo.text, "the memo's own words");
            }
            None => {
                assert_eq!(row.speaker, Speaker::Event, "{row:?}");
                assert_eq!(row.text, event.text);
                assert_eq!(row.kind, event.kind);
            }
        }
    }
    assert_eq!(memos, 1, "the golden carries one memo");
    let memo = rows
        .iter()
        .find(|row| row.speaker == Speaker::Ledger)
        .expect("the memo's row");
    assert_eq!(
        memo,
        &EventRow {
            clock: "0:10".to_owned(),
            kind: "ledger_memo".to_owned(),
            text: "Notice of licence. You ran Generators on two vents at settlement. You may \
                   now queue a Resonance Spire at any of your beacons."
                .to_owned(),
            speaker: Speaker::Ledger,
        }
    );
}

/// The branch is on the typed payload, never on the kind's spelling: an event that calls
/// itself `ledger_memo` and carries no memo is an ordinary row, and a memo under any kind is
/// the Ledger's.
#[test]
fn the_ledgers_row_follows_the_payload_not_the_kind() {
    let text = r#"{"events":[
        {"at_ms":0,"kind":"ledger_memo","text":"no payload"},
        {"at_ms":50,"kind":"settled","text":"t","ledger_memo":{"id":"dark_stake","text":"memo"}}
    ],"next_cursor":"c"}"#;
    let feed: GetSegmentFeedResponse = json::decode_json(&enums::canonical(
        "gp.api.v1.GetSegmentFeedResponse",
        &json::read(text).expect("json"),
    ))
    .expect("a feed");
    let rows = event_rows(&feed).expect("rows");
    assert_eq!(
        rows.iter().map(|row| row.speaker).collect::<Vec<_>>(),
        [Speaker::Event, Speaker::Ledger]
    );
    assert_eq!(rows.get(1).map(|row| row.text.as_str()), Some("memo"));
}

#[test]
fn the_event_list_shows_exactly_what_get_segment_feed_said() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("get_segment_feed.json");
    let text = fs::read_to_string(&fixture).expect("the generated feed fixture");
    let (body, _) = split_footer(&json::read(&text).expect("json"));
    let feed: GetSegmentFeedResponse =
        json::decode_json(&enums::canonical("gp.api.v1.GetSegmentFeedResponse", &body))
            .expect("a GetSegmentFeedResponse");

    let rows = event_rows(&feed).expect("every event is at a time in the segment");
    assert_eq!(
        rows.len(),
        feed.events.len(),
        "one row per event, no more and no fewer"
    );
    for (row, event) in rows.iter().zip(&feed.events) {
        let line = row.line();
        assert!(
            row.speaker == Speaker::Event
                && line.contains(&event.kind)
                && line.ends_with(&event.text),
            "a row carries the event's own kind and words: {line}"
        );
    }

    let mut fresh = String::from(
        "# The live event list, rendered from crates/client-gdext/tests/fixtures/\n\
         # get_segment_feed.json: one row per event, in the feed's order - the time in the\n\
         # segment, the kind and the gateway's own words, nothing added or dropped.\n",
    );
    for row in &rows {
        fresh.push_str(&row.line());
        fresh.push('\n');
    }

    let actual = target_dir()
        .join("golden")
        .join("vista")
        .join("actual.events.txt");
    fs::create_dir_all(actual.parent().expect("a parent")).expect("the output directory");
    fs::write(&actual, &fresh).expect("the fresh output");

    let expected = root()
        .join("tests")
        .join("golden")
        .join("vista")
        .join("expected.events.txt");
    if std::env::var_os(BLESS).is_some() {
        fs::write(&expected, &fresh).expect("blessing the event list");
        return;
    }
    let committed = fs::read_to_string(&expected).unwrap_or_default();
    assert!(
        committed == fresh,
        "the event list no longer shows what the feed said; the fresh rows are at {}. \
         Read tests/golden/vista/README.md, then re-bless with {BLESS}=1 and say why in the \
         pull request.",
        actual.display()
    );
}

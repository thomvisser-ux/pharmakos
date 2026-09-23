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
use pharmakos_client_gdext::rig::event_rows;
use pharmakos_client_gdext::view::split_footer;
use pharmakos_proto::gp::api::v1::GetSegmentFeedResponse;
use pharmakos_proto::json;

const BLESS: &str = "PHARMAKOS_BLESS_VISTA";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
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

    let rows = event_rows(&feed);
    assert_eq!(
        rows.len(),
        feed.events.len(),
        "one row per event, no more and no fewer"
    );
    for (row, event) in rows.iter().zip(&feed.events) {
        assert!(
            row.contains(&event.kind) && row.ends_with(&event.text),
            "a row carries the event's own kind and words: {row}"
        );
    }

    let mut fresh = String::from(
        "# The live event list, rendered from crates/client-gdext/tests/fixtures/\n\
         # get_segment_feed.json: one row per event, in the feed's order - the time in the\n\
         # segment, the kind and the gateway's own words, nothing added or dropped.\n",
    );
    for row in &rows {
        fresh.push_str(row);
        fresh.push('\n');
    }

    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root().join("target"), PathBuf::from);
    let actual = target
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

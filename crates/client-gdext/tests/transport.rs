// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **A peer holds the largest message the gateway sends it** (S3's `fixq-c`, fix pass 2, item
//! 1a). The gateway bounds no outgoing message by a cap of its own; the largest it makes is a
//! view page, whose chunks' runs it cuts at 256 KiB (`crates/gateway/src/viewfeed.rs`,
//! `VIEW_PAGE_BYTES`: a page takes a chunk only while its runs stay under the cut, and the first
//! chunk always). This builds a page at that cut, with every chunk of the committed map (its
//! extent is the rules table's `map` row: 384 by 384 by 64 voxels, 288 chunks) and 2 000
//! entities of the committed golden's own shape, and holds it under the inbound buffer
//! `host_link.gd` gives every peer, with half to spare. It also holds the outbound buffer to
//! the message cap the rig sends nothing past.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml sets allow-expect-in-tests, which does not reach an integration \
              test's helpers; a panic is this file's failure report"
)]

use pharmakos_client_gdext::transport::{INBOUND_BUFFER_BYTES, OUTBOUND_BUFFER_BYTES};
use pharmakos_proto::json::{self, Json};
use pharmakos_proto::transport::MAX_MESSAGE_BYTES;

/// The gateway's page cut, in bytes of runs.
const PAGE_RUNS: usize = 256 * 1024;
/// The committed map's chunks: 12 by 12 by 2.
const MAP_CHUNKS: usize = 288;
/// Entities on the page: more than any committed match fields.
const ENTITIES: usize = 2_000;
/// The page's bytes, as the gateway spells every answer on the wire (`rpc::render` is the
/// canonical writer, `json::write`, which this test uses). `client_check.gd` builds the same
/// page and receives it through `host_link.gd`'s own peer, so the two are held to one number.
const WORST_PAGE_BYTES: usize = 1_230_868;

/// Base64's length for `bytes` bytes.
fn base64_len(bytes: usize) -> usize {
    bytes.div_ceil(3).saturating_mul(4)
}

fn chunk(runs: usize, at: usize) -> Json {
    Json::Object(vec![
        (
            "origin".to_owned(),
            Json::Object(vec![
                ("x".to_owned(), Json::Number(at.to_string())),
                ("y".to_owned(), Json::Number("352".to_owned())),
                ("z".to_owned(), Json::Number("32".to_owned())),
            ]),
        ),
        (
            "voxels_rle".to_owned(),
            Json::String("A".repeat(base64_len(runs))),
        ),
    ])
}

#[test]
fn a_peer_holds_the_largest_view_page_and_the_message_cap() {
    // Four chunks of 65 535 bytes of runs fill the cut; every other chunk of the map is one run.
    let mut chunks: Vec<Json> = (0..4).map(|at| chunk(65_535, at * 32)).collect();
    chunks.extend((4..MAP_CHUNKS).map(|at| chunk(3, at * 32)));
    let runs: usize = 4 * 65_535 + (MAP_CHUNKS - 4) * 3;
    assert!(runs - (MAP_CHUNKS - 4) * 3 <= PAGE_RUNS);
    let entity = json::read(
        r#"{"id": "s_4000","kind": "structure","subtype": "mortar","owner": "seat.0","at": {"x": 365,"y": 30,"z": 36},"hp": 1,"max_hp": 400,"building": true,"look": {"shape": "dish","x": 1,"y": 1,"z": 1},"title": "Mortar, going up"}"#,
    )
    .expect("an entity");
    let page = Json::Object(vec![
        ("jsonrpc".to_owned(), Json::String("2.0".to_owned())),
        ("id".to_owned(), Json::Number("999999".to_owned())),
        (
            "result".to_owned(),
            Json::Object(vec![
                ("at_ms".to_owned(), Json::Number("480000".to_owned())),
                ("chunks".to_owned(), Json::Array(chunks)),
                ("entities".to_owned(), Json::Array(vec![entity; ENTITIES])),
                ("next_cursor".to_owned(), Json::String("c".repeat(64))),
                ("complete".to_owned(), Json::Bool(true)),
            ]),
        ),
    ]);
    let bytes = json::write(&page).len();
    println!(
        "the largest view page: {bytes} bytes, against an inbound buffer of {INBOUND_BUFFER_BYTES}"
    );
    assert_eq!(
        bytes, WORST_PAGE_BYTES,
        "the page client_check.gd receives whole"
    );
    assert!(
        bytes.saturating_mul(2) <= INBOUND_BUFFER_BYTES,
        "{bytes} bytes leave less than half of the {INBOUND_BUFFER_BYTES}-byte inbound buffer"
    );
    const { assert!(OUTBOUND_BUFFER_BYTES >= MAX_MESSAGE_BYTES) };
}

// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **The client reads what a report left out as the gateway sends it** (S3's `vcap` and
//! `leftout`). Which codes a report left diagnostics out of is the report's own `left_out` field
//! (`editor::left_out_of`), so the client holds no copy of the verifier's cap. It still shows the
//! closing `I0007` rows' sentences as the player's words for the same facts, so it names that one
//! code, and this test reads the verifier's own line as text and holds the two equal: a code the
//! verifier changes alone fails here.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml sets allow-expect-in-tests, which does not reach an integration \
              test's helpers; a panic is this file's failure report"
)]

use std::path::PathBuf;

use pharmakos_client_gdext::editor::LEFT_OUT_CODE;

#[test]
fn the_clients_left_out_code_is_the_verifiers() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../verifier/src/report.rs");
    let source = std::fs::read_to_string(path).expect("the verifier's report builder");
    assert!(
        source.contains(&format!(
            "pub(crate) const LEFT_OUT: &str = \"{LEFT_OUT_CODE}\";"
        )),
        "the verifier closes a report with another code than {LEFT_OUT_CODE}"
    );
}

/// The client names no copy of the verifier's cap: the constant it once mirrored is gone from
/// its sources, and nothing in them counts rows against a number to decide what was left out.
#[test]
fn the_client_holds_no_copy_of_the_verifiers_cap() {
    let source =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/editor.rs"))
            .expect("the editor's source");
    for gone in ["REPORT_CAP_PER_CODE", "MAX_PER_CODE", "may_have_left_out"] {
        assert!(
            !source.contains(gone),
            "the editor names `{gone}` again: what a report left out is its `left_out` field"
        );
    }
}

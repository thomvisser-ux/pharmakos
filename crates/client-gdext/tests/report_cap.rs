// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **The client reads a report's cap as the verifier writes it** (S3's `vcap`). The editor
//! knows a code may have been left out of a report only once the report lists the verifier's
//! `MAX_PER_CODE` of it (`editor::may_have_left_out`). This walled crate may not depend on the
//! verifier, so it mirrors the number as `REPORT_CAP_PER_CODE`, and this test reads the
//! verifier's own line as text and holds the two equal: a cap the verifier changes alone fails
//! here.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml sets allow-expect-in-tests, which does not reach an integration \
              test's helpers; a panic is this file's failure report"
)]

use std::path::PathBuf;

use pharmakos_client_gdext::editor::{LEFT_OUT_CODE, REPORT_CAP_PER_CODE};

/// The verifier's `MAX_PER_CODE`, read from its source.
fn verifier_cap() -> usize {
    const DECLARED: &str = "pub const MAX_PER_CODE: u32 = ";
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../verifier/src/report.rs");
    let source = std::fs::read_to_string(path).expect("the verifier's report builder");
    let line = source
        .lines()
        .find(|line| line.starts_with(DECLARED))
        .expect("report.rs declares MAX_PER_CODE");
    line.trim_start_matches(DECLARED)
        .trim_end_matches(';')
        .parse()
        .expect("MAX_PER_CODE is a whole number")
}

#[test]
fn the_clients_cap_is_the_verifiers() {
    assert_eq!(REPORT_CAP_PER_CODE, verifier_cap());
}

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

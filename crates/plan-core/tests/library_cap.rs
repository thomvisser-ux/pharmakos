// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **A library file is read within the playbook's text cap** (S3's `outcap`):
//! the library is a local folder anyone can drop a file in, and the gateway
//! reads it to list and instantiate templates. A file past
//! `library::MAX_FILE_BYTES` is refused at its length, typed
//! (`Bound::LibraryFile`), before anything is read; a file within the bytes
//! but past `MAX_PLAYBOOK_CHARS` characters is refused the same way; one at
//! the cap is read.

#![allow(
    clippy::expect_used,
    reason = "clippy.toml sets allow-expect-in-tests, which does not reach an integration \
              test's helpers; a panic is this file's failure report"
)]

use pharmakos_plan_core::error::Bound;
use pharmakos_plan_core::library::{MAX_FILE_BYTES, read};
use pharmakos_verifier::MAX_PLAYBOOK_CHARS;
use std::path::PathBuf;

fn folder(name: &str) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("library-cap-{name}"));
    std::fs::create_dir_all(&folder).expect("a scratch folder");
    folder
}

#[test]
fn a_library_file_past_the_cap_is_refused_typed() {
    let folder = folder("bytes");
    std::fs::write(folder.join("big.jsonc"), vec![b' '; MAX_FILE_BYTES + 1]).expect("written");
    let refused = read(&folder, "big").expect_err("past the bytes");
    assert_eq!(refused.bound(), Some(Bound::LibraryFile), "{refused:?}");

    // Within the bytes, past the characters: four-byte characters to the
    // byte cap is a quarter of it in characters, so use one-byte ones.
    std::fs::write(
        folder.join("long.jsonc"),
        " ".repeat(MAX_PLAYBOOK_CHARS + 1),
    )
    .expect("written");
    let refused = read(&folder, "long").expect_err("past the characters");
    assert_eq!(refused.bound(), Some(Bound::LibraryFile), "{refused:?}");

    std::fs::write(folder.join("at.jsonc"), " ".repeat(MAX_PLAYBOOK_CHARS)).expect("written");
    assert_eq!(
        read(&folder, "at").expect("at the cap").chars().count(),
        MAX_PLAYBOOK_CHARS
    );

    // The bytes' cap is the characters' at four bytes each: a file of
    // four-byte characters at the character cap is read.
    std::fs::write(
        folder.join("wide.jsonc"),
        "\u{1F600}".repeat(MAX_PLAYBOOK_CHARS),
    )
    .expect("written");
    assert_eq!(
        read(&folder, "wide").expect("four bytes a character").len(),
        MAX_FILE_BYTES
    );
}

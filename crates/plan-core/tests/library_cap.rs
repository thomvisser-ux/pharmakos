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
    // Refused at its length, before it is read: the refusal names the length.
    assert!(
        refused
            .message()
            .contains(&(MAX_FILE_BYTES + 1).to_string()),
        "{}",
        refused.message()
    );

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

/// A reader that counts the bytes asked of it.
struct Counting {
    asked: std::rc::Rc<std::cell::Cell<usize>>,
}

impl std::io::Read for Counting {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.asked.set(self.asked.get() + buffer.len());
        buffer.fill(b' ');
        Ok(buffer.len())
    }
}

/// **Refused at its declared length, before anything is read** (S3's
/// `outcap`, fix pass 1, item F): a source that declares a length past the cap
/// is asked for no byte; one that declares less and goes on is asked for at
/// most one byte past the cap, and refused.
#[test]
fn a_source_past_the_cap_is_refused_before_a_byte_is_asked() {
    let asked = std::rc::Rc::new(std::cell::Cell::new(0));
    let declared = u64::try_from(MAX_FILE_BYTES + 1).expect("fits");
    let source = Counting {
        asked: std::rc::Rc::clone(&asked),
    };
    let refused = pharmakos_plan_core::library::read_within(source, declared, "big", "big")
        .expect_err("past the cap");
    assert_eq!(refused.bound(), Some(Bound::LibraryFile));
    assert_eq!(asked.get(), 0, "no byte was asked");

    let source = Counting {
        asked: std::rc::Rc::clone(&asked),
    };
    let refused = pharmakos_plan_core::library::read_within(source, 10, "grew", "grew")
        .expect_err("longer than it said");
    assert_eq!(refused.bound(), Some(Bound::LibraryFile));
    assert!(
        asked.get() <= MAX_FILE_BYTES + 1 + 64 * 1024,
        "{} bytes asked",
        asked.get()
    );
}

/// **A listing counts what it refused** (S3's `outcap`, fix pass 1, item G):
/// a file past the cap, one that is not UTF-8 and one that is not a playbook
/// are each in `refused` by reason, and the good file is listed.
#[test]
fn a_listing_counts_the_files_it_refused() {
    use pharmakos_plan_core::library::{Refusal, listing};
    let folder = folder("listing");
    let good = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../library/hold_and_build.jsonc"),
    )
    .expect("a template");
    std::fs::write(folder.join("a_good.jsonc"), good).expect("written");
    std::fs::write(folder.join("b_big.jsonc"), vec![b' '; MAX_FILE_BYTES + 1]).expect("written");
    std::fs::write(folder.join("c_bytes.jsonc"), [0xff_u8, 0xfe]).expect("written");
    std::fs::write(folder.join("d_text.jsonc"), "not a playbook").expect("written");
    let listed = listing(&folder).expect("a folder");
    assert_eq!(listed.summaries.len(), 1);
    assert_eq!(listed.refused_count(Refusal::TooLarge), 1);
    assert_eq!(listed.refused_count(Refusal::NotAPlaybook), 2);
    assert_eq!(listed.refused_count(Refusal::Unreadable), 0);
}

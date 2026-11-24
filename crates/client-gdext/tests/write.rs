// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **A file is replaced whole by the operating system, or not at all** (S3's `fixq-c`, fix pass 5,
//! item A): `crate::write`'s order of steps, its refusals, and what is on disk after any failure.
//! These run on Windows, Linux and macOS in CI's matrix. Each step is failed through a recording
//! [`Files`] that wraps the real one; on the real file system only what is the same everywhere is
//! asserted (a folder as target, a missing folder, a read-only target, a stale temporary file of
//! ours, a player's own files beside the target), and Windows' reserved names under `cfg(windows)`
//! with their twin elsewhere.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "clippy.toml sets allow-expect-in-tests, which does not reach an integration \\
              test's helpers; a panic is this file's failure report"
)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use pharmakos_client_gdext::write::{
    Files, Kind, RealFiles, Removed, WriteFailure, has_windows_bad_ending, is_windows_reserved,
    remove_whole_with, temp_of, write_whole, write_whole_with,
};

/// A fresh folder for one test, under cargo's per-target temporary folder.
fn folder(name: &str) -> PathBuf {
    let at = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("write")
        .join(name);
    if at.exists() {
        fs::remove_dir_all(&at).expect("an old test folder goes");
    }
    fs::create_dir_all(&at).expect("a test folder");
    at
}

/// The real file system, with every step recorded, and one chosen call of one step failed.
struct Recording {
    real: RealFiles,
    log: Vec<&'static str>,
    fail: Option<(&'static str, usize)>,
    seen: Vec<&'static str>,
}

impl Recording {
    fn new(fail: Option<(&'static str, usize)>) -> Self {
        Self {
            real: RealFiles::default(),
            log: Vec::new(),
            fail,
            seen: Vec::new(),
        }
    }

    /// Records `step`; fails it when it is the chosen call of the chosen step.
    fn step(&mut self, step: &'static str) -> io::Result<()> {
        self.log.push(step);
        self.seen.push(step);
        let calls = self.seen.iter().filter(|seen| **seen == step).count();
        match self.fail {
            Some((failing, at)) if failing == step && at == calls => {
                Err(io::Error::other(format!("the test failed {step}")))
            }
            _ => Ok(()),
        }
    }
}

impl Files for Recording {
    fn kind(&mut self, path: &Path) -> io::Result<Kind> {
        self.step("kind")?;
        self.real.kind(path)
    }
    fn create_new(&mut self, path: &Path) -> io::Result<()> {
        self.step("create_new")?;
        self.real.create_new(path)
    }
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.step("write_all")?;
        self.real.write_all(bytes)
    }
    fn sync_all(&mut self) -> io::Result<()> {
        self.step("sync_all")?;
        self.real.sync_all()
    }
    fn close(&mut self) {
        self.log.push("close");
        self.real.close();
    }
    fn copy_permissions(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        self.step("copy_permissions")?;
        self.real.copy_permissions(from, to)
    }
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        self.step("rename")?;
        self.real.rename(from, to)
    }
    fn remove(&mut self, path: &Path) -> io::Result<()> {
        self.step("remove")?;
        self.real.remove(path)
    }
    fn sync_folder(&mut self, folder: &Path) -> io::Result<()> {
        self.step("sync_folder")?;
        // A folder cannot be opened to sync on Windows, which is why the shipped path syncs it on
        // Unix alone (`cfg!(unix)`); here the step is recorded on every system.
        if cfg!(unix) {
            self.real.sync_folder(folder)
        } else {
            Ok(())
        }
    }
}

#[test]
fn a_write_over_an_old_file_takes_its_steps_in_order() {
    let at = folder("order");
    let target = at.join("plan.jsonc");
    fs::write(&target, b"old").expect("the old file");
    let mut files = Recording::new(None);
    write_whole_with(&mut files, &target, b"new", false, true).expect("written");
    assert_eq!(
        files.log,
        [
            "kind",
            "kind",
            "kind",
            "create_new",
            "write_all",
            "sync_all",
            "copy_permissions",
            "rename",
            "sync_folder"
        ],
        "the bytes are synced before the rename, and the rename is the only step that touches \
         the target"
    );
    assert_eq!(fs::read(&target).expect("reads"), b"new");
    assert!(!temp_of(&target).expect("a temp").exists());
}

#[test]
fn a_folder_as_target_and_a_missing_folder_are_refused_and_nothing_is_made() {
    let at = folder("refusals");
    let inner = at.join("a_folder.jsonc");
    fs::create_dir(&inner).expect("a folder");
    assert!(matches!(
        write_whole(&inner, b"x"),
        Err(WriteFailure::IsFolder { .. })
    ));
    let missing = at.join("no_such_folder").join("plan.jsonc");
    assert!(matches!(
        write_whole(&missing, b"x"),
        Err(WriteFailure::MissingFolder { .. })
    ));
    assert!(!missing.exists() && inner.is_dir());
}

#[test]
fn a_failed_rename_leaves_the_old_bytes_and_no_file_of_ours() {
    let at = folder("rename");
    let target = at.join("plan.jsonc");
    fs::write(&target, b"old").expect("the old file");
    let mut files = Recording::new(Some(("rename", 1)));
    let failed = write_whole_with(&mut files, &target, b"new", false, true);
    assert!(
        matches!(failed, Err(WriteFailure::Rename { leftover: None, .. })),
        "{failed:?}"
    );
    assert_eq!(fs::read(&target).expect("reads"), b"old");
    assert!(!temp_of(&target).expect("a temp").exists());
}

#[test]
fn a_read_only_target_is_refused_before_anything_is_made() {
    let at = folder("read_only");
    let target = at.join("plan.jsonc");
    fs::write(&target, b"old").expect("the old file");
    let mut permissions = fs::metadata(&target).expect("metadata").permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&target, permissions).expect("read-only");
    // A user the read-only mark does not stop (root on Unix) makes this test vacuous: said, and
    // skipped, never passed.
    if fs::OpenOptions::new().write(true).open(&target).is_ok() {
        println!(
            "SKIPPED: this user can write a read-only file, so the refusal is not tested here"
        );
    } else {
        let mut files = Recording::new(None);
        let failed = write_whole_with(&mut files, &target, b"new", false, true);
        assert!(
            matches!(failed, Err(WriteFailure::ReadOnly { .. })),
            "{failed:?}"
        );
        assert!(!files.log.contains(&"create_new"), "{:?}", files.log);
        assert_eq!(fs::read(&target).expect("reads"), b"old");
    }
    let mut permissions = fs::metadata(&target).expect("metadata").permissions();
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "the test's own file, made writable again to be removed"
    )]
    permissions.set_readonly(false);
    fs::set_permissions(&target, permissions).expect("writable again");
}

#[test]
fn a_stale_temporary_file_of_ours_is_removed_and_said() {
    let at = folder("stale");
    let target = at.join("plan.jsonc");
    let temp = temp_of(&target).expect("a temp");
    fs::write(&temp, b"half").expect("a stale temp");
    let written = write_whole(&target, b"new").expect("written");
    assert_eq!(written.stale_removed, Some(temp.clone()));
    assert_eq!(fs::read(&target).expect("reads"), b"new");
    assert!(!temp.exists());
}

#[test]
fn a_players_own_files_beside_the_target_are_never_touched() {
    let at = folder("players");
    let target = at.join("plan.jsonc");
    let theirs = [
        at.join("plan.jsonc.bak"),
        at.join("plan.jsonc.tmp"),
        at.join(".plan.jsonc"),
        at.join(".plan.jsonc.pharmakos-keep"),
    ];
    for (index, path) in theirs.iter().enumerate() {
        fs::write(path, format!("the player's {index}")).expect("a player's file");
    }
    fs::write(&target, b"old").expect("the old file");
    write_whole(&target, b"new").expect("written");
    let mut files = Recording::new(Some(("rename", 1)));
    let _ = write_whole_with(&mut files, &target, b"newer", false, true);
    for (index, path) in theirs.iter().enumerate() {
        assert_eq!(
            fs::read_to_string(path).expect("still there"),
            format!("the player's {index}"),
            "{}",
            path.display()
        );
    }
}

#[test]
fn windows_reserved_names_and_endings_are_refused_where_the_rule_applies() {
    let at = folder("reserved");
    for name in [
        "CON",
        "con.jsonc",
        "Prn.txt",
        "AUX",
        "nul.jsonc",
        "COM1.x",
        "com9",
        "LPT1.jsonc",
        "lpt9",
        "CON .jsonc",
    ] {
        assert!(is_windows_reserved(name), "{name}");
        let refused =
            write_whole_with(&mut RealFiles::default(), &at.join(name), b"x", true, false);
        assert!(
            matches!(refused, Err(WriteFailure::ReservedName { .. })),
            "{name}: {refused:?}"
        );
    }
    for name in [
        "CONSOLE.jsonc",
        "COM10",
        "plan.jsonc",
        "LPT",
        "aux_plan.jsonc",
    ] {
        assert!(!is_windows_reserved(name), "{name}");
    }
    for name in ["plan.", "plan "] {
        assert!(has_windows_bad_ending(name), "{name}");
        let refused =
            write_whole_with(&mut RealFiles::default(), &at.join(name), b"x", true, false);
        assert!(
            matches!(refused, Err(WriteFailure::BadEnding { .. })),
            "{name}: {refused:?}"
        );
    }
}

/// On Windows the shipped path refuses a reserved name.
#[cfg(windows)]
#[test]
fn the_shipped_path_refuses_a_reserved_name_on_windows() {
    let at = folder("reserved_shipped");
    for name in ["CON.jsonc", "nul", "LPT1.txt"] {
        assert!(matches!(
            write_whole(&at.join(name), b"x"),
            Err(WriteFailure::ReservedName { .. })
        ));
    }
}

/// Elsewhere the same names are ordinary files, and the shipped path writes them.
#[cfg(not(windows))]
#[test]
fn the_shipped_path_writes_a_windows_reserved_name_elsewhere() {
    let at = folder("reserved_shipped");
    for name in ["CON.jsonc", "nul", "LPT1.txt"] {
        write_whole(&at.join(name), b"x").expect("an ordinary name here");
        assert_eq!(fs::read(at.join(name)).expect("reads"), b"x");
    }
}

#[test]
fn a_removal_is_refused_on_a_folder_and_a_read_only_file_and_absent_is_no_failure() {
    let at = folder("remove");
    assert_eq!(
        remove_whole_with(&mut RealFiles::default(), &at.join("none")).expect("absent"),
        Removed::Absent
    );
    fs::create_dir(at.join("dir")).expect("a folder");
    assert!(matches!(
        remove_whole_with(&mut RealFiles::default(), &at.join("dir")),
        Err(WriteFailure::IsFolder { .. })
    ));
    fs::write(at.join("line"), b"x").expect("a file");
    let mut files = Recording::new(Some(("remove", 1)));
    assert!(matches!(
        remove_whole_with(&mut files, &at.join("line")),
        Err(WriteFailure::Remove { .. })
    ));
    assert!(at.join("line").exists());
    assert_eq!(
        remove_whole_with(&mut RealFiles::default(), &at.join("line")).expect("removed"),
        Removed::Removed
    );
}

/// A small seeded generator (xorshift64*), so the property test needs no dependency and replays
/// from its seed.
struct Seeded(u64);

impl Seeded {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound).expect("fits");
        usize::try_from(self.next().checked_rem(bound).expect("a bound")).expect("fits")
    }
}

/// **The property:** over generated names, old files present or absent, and every step failed in
/// turn (its first or second call), after any failure the target holds exactly the old bytes
/// (or nothing, when there was none) and never part of the new ones, and a temporary file of ours
/// left on disk is always the one the error names; after a success it holds exactly the new
/// bytes and none is left.
#[test]
fn after_any_failure_the_target_is_old_or_new_and_no_file_of_ours_is_unnamed() {
    let at = folder("property");
    let steps = [
        "kind",
        "create_new",
        "write_all",
        "sync_all",
        "copy_permissions",
        "rename",
        "remove",
        "sync_folder",
    ];
    let letters = b"abcdefghijklmnopqrstuvwxyz0123456789_-";
    let mut seed = Seeded(0x5eed_f1e5_0000_0005);
    for case in 0..400_u32 {
        let length = 1 + seed.below(12);
        let name: String = (0..length)
            .map(|_| char::from(*letters.get(seed.below(letters.len())).expect("a letter")))
            .collect();
        let target = at.join(format!("{name}.jsonc"));
        let old = format!("old {case}").into_bytes();
        let new = format!("new {case} {}", "x".repeat(seed.below(4096))).into_bytes();
        let had_old = seed.below(2) == 0;
        if had_old {
            fs::write(&target, &old).expect("the old file");
        } else if target.exists() {
            fs::remove_file(&target).expect("no old file");
        }
        let stale = seed.below(4) == 0;
        let temp = temp_of(&target).expect("a temp");
        if stale {
            fs::write(&temp, b"half").expect("a stale temp");
        }
        let step = *steps.get(seed.below(steps.len())).expect("a step");
        let nth = 1 + seed.below(2);
        let mut files = Recording::new(Some((step, nth)));
        let result = write_whole_with(&mut files, &target, &new, false, true);
        let now = fs::read(&target).ok();
        match &result {
            Ok(_) => {
                assert_eq!(
                    now.as_deref(),
                    Some(new.as_slice()),
                    "case {case}: {step} {nth}"
                );
                assert!(!temp.exists(), "case {case}: a temp left after a success");
            }
            Err(WriteFailure::SyncFolder { .. }) => {
                assert_eq!(
                    now.as_deref(),
                    Some(new.as_slice()),
                    "case {case}: in place"
                );
            }
            Err(failure) => {
                let expected = had_old.then_some(old.as_slice());
                assert_eq!(
                    now.as_deref(),
                    expected,
                    "case {case}: {step} {nth}: {failure}"
                );
                if temp.exists() {
                    let named = failure.leftover() == Some(temp.as_path())
                        || matches!(failure, WriteFailure::StaleTemp { .. })
                        || (stale
                            && matches!(failure, WriteFailure::Inspect { .. })
                            && !files.log.contains(&"create_new"));
                    assert!(
                        named,
                        "case {case}: {step} {nth}: a file of ours unnamed: {failure}"
                    );
                }
            }
        }
        for leftover in [&target, &temp] {
            if leftover.exists() {
                fs::remove_file(leftover).expect("the case's files go");
            }
        }
    }
}

/// Every variant's name is on [`WriteFailure::NAMES`], which `client_check.gd` holds the string
/// table to, row for row.
#[test]
fn every_failure_is_named_on_the_list_the_words_are_checked_against() {
    let error = || io::Error::other("e");
    let path = PathBuf::from("p");
    let every = [
        WriteFailure::NoName { path: path.clone() },
        WriteFailure::ReservedName { path: path.clone() },
        WriteFailure::BadEnding { path: path.clone() },
        WriteFailure::IsFolder { path: path.clone() },
        WriteFailure::MissingFolder {
            folder: path.clone(),
        },
        WriteFailure::ReadOnly { path: path.clone() },
        WriteFailure::Inspect {
            path: path.clone(),
            error: error(),
        },
        WriteFailure::StaleTemp {
            temp: path.clone(),
            error: error(),
        },
        WriteFailure::BeforeRename {
            step: "write",
            path: path.clone(),
            error: error(),
            leftover: None,
        },
        WriteFailure::Rename {
            path: path.clone(),
            error: error(),
            leftover: None,
        },
        WriteFailure::SyncFolder {
            path: path.clone(),
            error: error(),
        },
        WriteFailure::Remove {
            path,
            error: error(),
        },
    ];
    let names: Vec<&str> = every.iter().map(WriteFailure::name).collect();
    assert_eq!(names, WriteFailure::NAMES);
}

/// No shipped call site can make a failing [`Files`]: `src/` implements the trait once, for
/// [`RealFiles`], and only `write.rs` itself calls the `_with` functions that take one.
#[test]
fn the_shipped_path_makes_only_the_real_files() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut impls = 0;
    for name in [
        "api.rs",
        "bridge.rs",
        "chunks.rs",
        "editor.rs",
        "editor_view.rs",
        "engine.rs",
        "entry.rs",
        "enums.rs",
        "error.rs",
        "lib.rs",
        "pacer.rs",
        "panics.rs",
        "rig.rs",
        "rule_list.rs",
        "rules.rs",
        "surface.rs",
        "targeting.rs",
        "transport.rs",
        "upload.rs",
        "variant.rs",
        "view.rs",
        "vocabulary.rs",
        "wizard.rs",
        "write.rs",
    ] {
        let Ok(text) = fs::read_to_string(src.join(name)) else {
            continue;
        };
        impls += text.matches("impl Files for").count();
        if name != "write.rs" {
            assert!(
                !text.contains("write_whole_with(") && !text.contains("remove_whole_with("),
                "{name} calls a function that takes its own Files"
            );
        }
    }
    let write = fs::read_to_string(src.join("write.rs")).expect("write.rs");
    assert_eq!(impls, 1, "src/ implements Files once");
    assert!(write.contains("impl Files for RealFiles"));
}

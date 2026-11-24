// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! **A file is replaced whole by the operating system, or not at all** (S3's `fixq-c`, fix passes
//! 5 and 6, item A): `crate::write`'s order of steps, its refusals, and what is on disk after any
//! failure. These run on Windows, Linux and macOS in CI's matrix. Each step is failed through a
//! recording [`Files`] that wraps the real one and names its temporary file deterministically; on
//! the real file system only what is the same everywhere is asserted, plus Windows' reserved
//! names and illegal characters under `cfg(windows)` with their twins elsewhere, and links on
//! Unix.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "clippy.toml sets allow-expect-in-tests, which does not reach an integration \
              test's helpers; a panic is this file's failure report"
)]

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use pharmakos_client_gdext::write::{
    Files, Kind, Platform, RealFiles, Removed, WriteFailure, has_windows_bad_ending,
    has_windows_illegal_character, is_windows_reserved, remove_whole_with, temp_of, write_whole,
    write_whole_with,
};

/// The rules the tests write under: Windows' names refused or not, and the folder synced.
const PLAIN: Platform = Platform {
    windows_names: false,
    sync_folder: true,
};
const WINDOWS_NAMES: Platform = Platform {
    windows_names: true,
    sync_folder: false,
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

/// The temporary file a [`Recording`] writes through for `path`: write number 0.
fn our_temp(path: &Path) -> PathBuf {
    temp_of(path, 0).expect("a temp")
}

/// The real file system, with every step recorded, one chosen call of one step failed, and its
/// temporary file named deterministically ([`our_temp`]).
struct Recording {
    real: RealFiles,
    log: Vec<&'static str>,
    fail: Option<(&'static str, usize)>,
    seen: Vec<&'static str>,
    /// The system's own rename made to fail: the source is removed just before it.
    vanish_before_rename: bool,
    /// A removal of our temporary file answers that it worked and leaves it there.
    pretend_remove: bool,
    /// The temporary file's identity (Unix inode) just before the rename.
    temp_inode: Option<u64>,
    /// A stand-in link: the path asked for, and the real file it leads to.
    link: Option<(PathBuf, PathBuf)>,
}

impl Recording {
    fn new(fail: Option<(&'static str, usize)>) -> Self {
        Self {
            real: RealFiles::default(),
            log: Vec::new(),
            fail,
            seen: Vec::new(),
            vanish_before_rename: false,
            pretend_remove: false,
            temp_inode: None,
            link: None,
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

#[cfg(unix)]
fn inode(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|meta| meta.ino())
}

#[cfg(not(unix))]
fn inode(_path: &Path) -> Option<u64> {
    None
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
        self.temp_inode = inode(from);
        if self.vanish_before_rename {
            fs::remove_file(from)?;
        }
        self.real.rename(from, to)
    }
    fn remove(&mut self, path: &Path) -> io::Result<()> {
        self.step("remove")?;
        if self.pretend_remove {
            return Ok(());
        }
        self.real.remove(path)
    }
    fn sync_folder(&mut self, folder: &Path) -> io::Result<()> {
        self.step("sync_folder")?;
        // A folder cannot be opened to sync on Windows, which is why the shipped path syncs it on
        // Unix alone (`Platform::THIS`); here the step is recorded on every system.
        if cfg!(unix) {
            self.real.sync_folder(folder)
        } else {
            Ok(())
        }
    }
    fn resolve(&mut self, path: &Path) -> io::Result<PathBuf> {
        match &self.link {
            Some((link, real)) if link == path => Ok(real.clone()),
            _ => self.real.resolve(path),
        }
    }
    fn temp_for(&mut self, path: &Path) -> Option<PathBuf> {
        temp_of(path, 0)
    }
}

#[test]
fn a_write_over_an_old_file_takes_its_steps_in_order() {
    let at = folder("order");
    let target = at.join("plan.jsonc");
    fs::write(&target, b"old").expect("the old file");
    let mut files = Recording::new(None);
    write_whole_with(&mut files, &target, b"new", PLAIN).expect("written");
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
    assert!(!our_temp(&target).exists());
}

/// The rename is the replace: on Unix the target IS the temporary file afterwards (its inode),
/// and on every system a reader that opened the old file before the save still reads the old
/// bytes whole (a copy over the old file, then a removal, would hand it the new ones).
#[test]
fn the_replace_is_a_rename_and_a_reader_of_the_old_file_keeps_it() {
    let at = folder("rename_is_replace");
    let target = at.join("plan.jsonc");
    fs::write(&target, b"the old bytes").expect("the old file");
    let mut reader = fs::File::open(&target).expect("a reader of the old file");
    let mut files = Recording::new(None);
    write_whole_with(&mut files, &target, b"the new bytes", PLAIN).expect("written");
    let mut held = Vec::new();
    reader.read_to_end(&mut held).expect("the reader reads");
    assert_eq!(
        held, b"the old bytes",
        "a reader of the old file saw the new bytes"
    );
    assert_eq!(fs::read(&target).expect("reads"), b"the new bytes");
    if cfg!(unix) {
        assert_eq!(
            inode(&target),
            files.temp_inode,
            "the target is not the temporary file renamed"
        );
    }
}

/// The platform's rules the shipped path writes with: Windows' names on Windows, the folder's
/// sync on Unix, and `write_whole` passing exactly these.
#[test]
fn the_shipped_path_writes_with_this_platforms_rules() {
    assert_eq!(
        Platform::THIS,
        Platform {
            windows_names: cfg!(windows),
            sync_folder: cfg!(unix),
        }
    );
    let src = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/write.rs"))
        .expect("write.rs");
    // Word for word, so a flag turned off is seen on the system where it changes nothing too.
    assert!(
        src.contains(
            "        windows_names: cfg!(windows),
        sync_folder: cfg!(unix),
"
        ),
        "Platform::THIS is not this platform's rules"
    );
    assert!(
        src.contains("write_whole_with(&mut RealFiles::default(), path, bytes, Platform::THIS)"),
        "write_whole does not write with Platform::THIS"
    );
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
    let failed = write_whole_with(&mut files, &target, b"new", PLAIN);
    assert!(
        matches!(failed, Err(WriteFailure::Rename { leftover: None, .. })),
        "{failed:?}"
    );
    assert_eq!(fs::read(&target).expect("reads"), b"old");
    assert!(!our_temp(&target).exists());
}

/// The system's own rename failing (here: its source gone) leaves the target's old bytes: the
/// replace is one atomic rename, never a removal of the target and then a rename.
#[test]
fn a_rename_that_fails_in_the_system_leaves_the_old_file() {
    let at = folder("system_rename");
    let target = at.join("plan.jsonc");
    fs::write(&target, b"old").expect("the old file");
    let mut files = Recording::new(None);
    files.vanish_before_rename = true;
    let failed = write_whole_with(&mut files, &target, b"new", PLAIN);
    assert!(
        matches!(failed, Err(WriteFailure::Rename { .. })),
        "{failed:?}"
    );
    assert_eq!(
        fs::read(&target).expect("the old file is still there"),
        b"old"
    );
}

/// Whether this user can write a file whose metadata says read-only (root on Unix can).
fn can_write_read_only(target: &Path) -> bool {
    fs::OpenOptions::new().write(true).open(target).is_ok()
}

/// A read-only target is refused before anything is made. Where this user can write a read-only
/// file anyway (root on Unix), the refusal cannot be tested here: the test FAILS, saying so,
/// unless `PHARMAKOS_WRITE_TESTS_AS_ROOT` is set, when it prints that it was skipped. A root run
/// is never a quiet pass.
#[test]
fn a_read_only_target_is_refused_before_anything_is_made() {
    let at = folder("read_only");
    let target = at.join("plan.jsonc");
    fs::write(&target, b"old").expect("the old file");
    let mut permissions = fs::metadata(&target).expect("metadata").permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&target, permissions).expect("read-only");
    if can_write_read_only(&target) {
        let allowed = std::env::var_os("PHARMAKOS_WRITE_TESTS_AS_ROOT").is_some();
        let mut permissions = fs::metadata(&target).expect("metadata").permissions();
        #[allow(
            clippy::permissions_set_readonly_false,
            reason = "the test's own file, made writable again to be removed"
        )]
        permissions.set_readonly(false);
        fs::set_permissions(&target, permissions).expect("writable again");
        assert!(
            allowed,
            "this user can write a read-only file, so the read-only refusal is not tested: run \
             as another user, or set PHARMAKOS_WRITE_TESTS_AS_ROOT to skip it knowingly"
        );
        println!("SKIPPED: this user can write a read-only file (PHARMAKOS_WRITE_TESTS_AS_ROOT)");
        return;
    }
    let mut files = Recording::new(None);
    let failed = write_whole_with(&mut files, &target, b"new", PLAIN);
    assert!(
        matches!(failed, Err(WriteFailure::ReadOnly { .. })),
        "{failed:?}"
    );
    assert!(!files.log.contains(&"create_new"), "{:?}", files.log);
    assert_eq!(fs::read(&target).expect("reads"), b"old");
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
    let temp = our_temp(&target);
    fs::write(&temp, b"half").expect("a stale temp");
    let written =
        write_whole_with(&mut Recording::new(None), &target, b"new", PLAIN).expect("written");
    assert_eq!(written.stale_removed, Some(temp.clone()));
    assert_eq!(fs::read(&target).expect("reads"), b"new");
    assert!(!temp.exists());
}

/// The temporary file is made with `create_new`: a stale one the removal missed is never
/// written into; the write fails before anything touches the target.
#[test]
fn a_stale_temporary_file_the_removal_missed_is_never_written_into() {
    let at = folder("stale_missed");
    let target = at.join("plan.jsonc");
    fs::write(&target, b"old").expect("the old file");
    let temp = our_temp(&target);
    fs::write(&temp, b"someone's half").expect("a stale temp");
    let mut files = Recording::new(None);
    files.pretend_remove = true;
    let failed = write_whole_with(&mut files, &target, b"new", PLAIN);
    assert!(
        matches!(
            &failed,
            Err(WriteFailure::BeforeRename { step: "create", .. })
        ),
        "{failed:?}"
    );
    assert_eq!(fs::read(&temp).expect("still there"), b"someone's half");
    assert_eq!(fs::read(&target).expect("reads"), b"old");
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
    let _ = write_whole_with(&mut files, &target, b"newer", PLAIN);
    for (index, path) in theirs.iter().enumerate() {
        assert_eq!(
            fs::read_to_string(path).expect("still there"),
            format!("the player's {index}"),
            "{}",
            path.display()
        );
    }
}

/// Every name Windows reserves, in its cases, written out by hand: the reserved list may lose
/// none of them.
const RESERVED_TABLE: &[&str] = &[
    "CON",
    "PRN",
    "AUX",
    "NUL",
    "COM1",
    "COM2",
    "COM3",
    "COM4",
    "COM5",
    "COM6",
    "COM7",
    "COM8",
    "COM9",
    "COM\u{b9}",
    "COM\u{b2}",
    "COM\u{b3}",
    "LPT1",
    "LPT2",
    "LPT3",
    "LPT4",
    "LPT5",
    "LPT6",
    "LPT7",
    "LPT8",
    "LPT9",
    "LPT\u{b9}",
    "LPT\u{b2}",
    "LPT\u{b3}",
    "CONIN$",
    "CONOUT$",
];

#[test]
fn windows_reserved_names_illegal_characters_and_endings_are_refused_where_the_rule_applies() {
    let at = folder("reserved");
    for reserved in RESERVED_TABLE {
        for name in [
            (*reserved).to_owned(),
            reserved.to_ascii_lowercase(),
            format!("{reserved}.jsonc"),
            format!("{} .jsonc", reserved.to_ascii_lowercase()),
        ] {
            assert!(is_windows_reserved(&name), "{name}");
            let refused = write_whole_with(
                &mut RealFiles::default(),
                &at.join(&name),
                b"x",
                WINDOWS_NAMES,
            );
            assert!(
                matches!(refused, Err(WriteFailure::ReservedName { .. })),
                "{name}: {refused:?}"
            );
        }
    }
    for name in [
        "CONSOLE.jsonc",
        "COM10",
        "plan.jsonc",
        "LPT",
        "aux_plan.jsonc",
        "COM0",
    ] {
        assert!(!is_windows_reserved(name), "{name}");
    }
    for name in ["plan.", "plan "] {
        assert!(has_windows_bad_ending(name), "{name}");
        let refused = write_whole_with(
            &mut RealFiles::default(),
            &at.join(name),
            b"x",
            WINDOWS_NAMES,
        );
        assert!(
            matches!(refused, Err(WriteFailure::BadEnding { .. })),
            "{name}: {refused:?}"
        );
    }
    for illegal in ['<', '>', ':', '"', '|', '?', '*', '\u{1}', '\u{1f}'] {
        let name = format!("plan.jsonc{illegal}ads");
        assert!(has_windows_illegal_character(&name), "{name:?}");
        let refused = write_whole_with(
            &mut RealFiles::default(),
            &at.join(&name),
            b"x",
            WINDOWS_NAMES,
        );
        assert!(
            matches!(refused, Err(WriteFailure::IllegalCharacter { .. })),
            "{name:?}: {refused:?}"
        );
    }
    assert!(
        !at.join(".plan.jsonc").exists() && !at.join("plan.jsonc").exists(),
        "a refused name made a file"
    );
}

/// On Windows the shipped path refuses a reserved name and an alternate data stream's name, and
/// makes no file for either (`plan.jsonc:ads` once made an empty `.plan.jsonc`).
#[cfg(windows)]
#[test]
fn the_shipped_path_refuses_reserved_and_illegal_names_on_windows() {
    let at = folder("reserved_shipped");
    for name in ["CON.jsonc", "nul", "LPT1.txt", "CONIN$"] {
        assert!(matches!(
            write_whole(&at.join(name), b"x"),
            Err(WriteFailure::ReservedName { .. })
        ));
    }
    assert!(matches!(
        write_whole(&at.join("plan.jsonc:ads"), b"x"),
        Err(WriteFailure::IllegalCharacter { .. })
    ));
    assert!(!at.join(".plan.jsonc").exists() && !at.join("plan.jsonc").exists());
}

/// Elsewhere the same names are ordinary files, and the shipped path writes them.
#[cfg(not(windows))]
#[test]
fn the_shipped_path_writes_windows_names_elsewhere() {
    let at = folder("reserved_shipped");
    for name in ["CON.jsonc", "nul", "LPT1.txt", "plan:ads.jsonc"] {
        write_whole(&at.join(name), b"x").expect("an ordinary name here");
        assert_eq!(fs::read(at.join(name)).expect("reads"), b"x");
    }
}

/// The file a link leads to is the one replaced, on every system: a stand-in link (resolved by
/// the recording `Files`) is left as it is, and the real file gets the bytes, its temporary file
/// beside it.
#[test]
fn the_resolved_file_is_the_one_replaced() {
    let at = folder("resolved");
    let link = at.join("link.jsonc");
    let real_folder = at.join("elsewhere");
    fs::create_dir(&real_folder).expect("a folder");
    let real = real_folder.join("real.jsonc");
    fs::write(&link, b"the stand-in link").expect("a stand-in");
    fs::write(&real, b"old").expect("the real file");
    let mut files = Recording::new(None);
    files.link = Some((link.clone(), real.clone()));
    write_whole_with(&mut files, &link, b"new", PLAIN).expect("written");
    assert_eq!(fs::read(&real).expect("the real file"), b"new");
    assert_eq!(fs::read(&link).expect("the stand-in"), b"the stand-in link");
    assert!(!our_temp(&link).exists() && !our_temp(&real).exists());
}

/// Makes `link` a symbolic link to the file `real`, as this system makes one.
#[cfg(unix)]
fn symlink_file(real: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(real, link)
}

/// Makes `link` a symbolic link to the file `real`, as this system makes one.
#[cfg(windows)]
fn symlink_file(real: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(real, link)
}

/// Windows' `ERROR_PRIVILEGE_NOT_HELD`: this user may not make a symbolic link (no Developer
/// Mode and no administrator's right).
#[cfg(windows)]
const PRIVILEGE_NOT_HELD: i32 = 1314;

/// A target that is a symbolic link, on every system: the real file gets the bytes, and the link
/// stays a link (fix pass 7, item 6: on Windows it was tested only through the stand-in). Where
/// this Windows user may not make a symbolic link, the test FAILS, saying so, unless
/// `PHARMAKOS_WRITE_TESTS_WITHOUT_SYMLINKS` is set, when it prints that it was skipped: as the
/// read-only test does, never a quiet pass.
#[test]
fn a_symbolic_link_is_followed_and_stays_a_link() {
    let at = folder("symlink");
    let real = at.join("real.jsonc");
    let link = at.join("link.jsonc");
    fs::write(&real, b"old").expect("the real file");
    if let Err(error) = symlink_file(&real, &link) {
        #[cfg(windows)]
        if error.raw_os_error() == Some(PRIVILEGE_NOT_HELD) {
            assert!(
                std::env::var_os("PHARMAKOS_WRITE_TESTS_WITHOUT_SYMLINKS").is_some(),
                "this user may not make a symbolic link ({error}), so the link rule is not \
                 tested: turn on Developer Mode or run as an administrator, or set \
                 PHARMAKOS_WRITE_TESTS_WITHOUT_SYMLINKS to skip it knowingly"
            );
            println!(
                "SKIPPED: this user may not make a symbolic link \
                 (PHARMAKOS_WRITE_TESTS_WITHOUT_SYMLINKS)"
            );
            return;
        }
        panic!("a link: {error}");
    }
    write_whole(&link, b"new").expect("written");
    assert!(
        fs::symlink_metadata(&link)
            .expect("the link")
            .file_type()
            .is_symlink(),
        "the link was replaced by a file"
    );
    assert_eq!(fs::read(&real).expect("the real file"), b"new");
}

/// A target with a second hard link: this name gets the new bytes, the other keeps the old
/// ones; an atomic replace swaps the name, not the file's content.
#[cfg(unix)]
#[test]
fn a_hard_linked_target_keeps_its_old_bytes_under_the_other_name() {
    let at = folder("hardlink");
    let target = at.join("plan.jsonc");
    let other = at.join("other.jsonc");
    fs::write(&target, b"old").expect("the file");
    fs::hard_link(&target, &other).expect("a hard link");
    write_whole(&target, b"new").expect("written");
    assert_eq!(fs::read(&target).expect("reads"), b"new");
    assert_eq!(fs::read(&other).expect("reads"), b"old");
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

/// The shipped temporary name is unique per process and per call: two writes in a row go through
/// two names.
#[test]
fn each_write_goes_through_a_temporary_name_of_its_own() {
    let at = folder("unique");
    let target = at.join("plan.jsonc");
    let mut real = RealFiles::default();
    let first = real.temp_for(&target).expect("a temp");
    let second = real.temp_for(&target).expect("a temp");
    assert_ne!(first, second);
    let pid = std::process::id().to_string();
    assert!(first.to_string_lossy().contains(&pid));
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
        let temp = our_temp(&target);
        if stale {
            fs::write(&temp, b"half").expect("a stale temp");
        }
        let step = *steps.get(seed.below(steps.len())).expect("a step");
        let nth = 1 + seed.below(2);
        let mut files = Recording::new(Some((step, nth)));
        let result = write_whole_with(&mut files, &target, &new, PLAIN);
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
        WriteFailure::IllegalCharacter { path: path.clone() },
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
/// [`RealFiles`], and only `write.rs` itself calls the `_with` functions that take one. Every
/// Rust file under `src/` is read, by walking the folder.
#[test]
fn the_shipped_path_makes_only_the_real_files() {
    fn walk(folder: &Path, found: &mut Vec<PathBuf>) {
        #[allow(
            clippy::disallowed_methods,
            reason = "the listing is collected and sorted before it is used"
        )]
        let entries = fs::read_dir(folder).expect("a folder");
        for entry in entries {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&src, &mut files);
    files.sort();
    assert!(files.len() > 20, "the walk found {} files", files.len());
    let mut impls = 0;
    for path in &files {
        let text = fs::read_to_string(path).expect("reads");
        // Test modules may implement it (the rig's fault test does not); count the shipped part.
        let shipped = text
            .split("#[cfg(test)]\nmod tests {")
            .next()
            .expect("a first part");
        impls += shipped.matches("impl Files for").count();
        if !path.ends_with("write.rs") {
            assert!(
                !shipped.contains("write_whole_with(") && !shipped.contains("remove_whole_with("),
                "{} calls a function that takes its own Files",
                path.display()
            );
        }
    }
    let write = fs::read_to_string(src.join("write.rs")).expect("write.rs");
    assert_eq!(impls, 1, "src/ implements Files once");
    assert!(write.contains("impl Files for RealFiles"));
}

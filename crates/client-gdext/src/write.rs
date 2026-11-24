// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A file replaced whole by the operating system, atomically (S3's `fixq-c`, fix pass 5): Save and
//! Save as... (`editor.gd`), and the lobby's remembered config line and its forgetting
//! (`lobby.gd`).
//!
//! GDScript cannot replace a file atomically (measured on Godot 4.7.2: its rename over an
//! existing file removes the target first, and loses it when the source is held open), so the
//! script-side helper of fix passes 3 and 4 grew a keep-file, a recovery pass and words for nine
//! states, and each had a hole. `std::fs::rename` replaces a file atomically on Windows, Linux
//! and macOS: the name holds the whole old file until the rename, and the whole new file after.
//! So [`write_whole`]:
//! 1. refuses a name this platform cannot hold as a file (on Windows a reserved device name, a
//!    name ending in a dot or a space, or one holding `< > : " | ? *` or a control character),
//!    before anything looks at the disk; then follows a symbolic link to the real file; and
//!    refuses a target that is a folder and a missing folder, each its own [`WriteFailure`];
//! 2. refuses an existing target its metadata marks read-only, before anything is created;
//! 3. creates a temporary file in the same folder with `create_new` (a dot-prefixed name with the
//!    process id, a process-wide count of writes and a suffix no player file has, [`temp_of`]; if
//!    that exact name exists it is ours, from a write that stopped: it is removed, and said once;
//!    no other file is ever touched), writes every byte, `sync_all`s it, and copies the old
//!    file's permissions onto it;
//! 4. renames it over the target; on any failure it removes the temporary file (a leftover is
//!    said by its full name), and the target is untouched, true by construction: the only step
//!    that touches it is the atomic rename;
//! 5. on Unix, syncs the folder, so the rename itself survives a crash; a failure is its own
//!    [`WriteFailure::SyncFolder`], said with the file already in place.
//!
//! Every step goes through [`Files`], so a test can fail each one. The shipped path makes only
//! [`RealFiles`] (`tests/write.rs` holds the source to that: no other `impl Files` is in `src/`).
//!
//! Writing the bytes the player asked to save, where they asked, decides nothing about the game
//! (AGENTS.md section 3 rule 4); no playbook can name a path (section 7).

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The temporary file's suffix: no player file ends in it.
pub const TEMP_SUFFIX: &str = ".pharmakos-write";

/// What is at a path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Nothing.
    Absent,
    /// A file, and whether its metadata marks it read-only.
    File {
        /// `Permissions::readonly`.
        read_only: bool,
    },
    /// A folder.
    Folder,
}

/// The file operations [`write_whole`] and [`remove_whole`] make, one at a time, in order.
pub trait Files {
    /// What is at `path`.
    ///
    /// # Errors
    ///
    /// The metadata could not be read for a reason other than the path being absent.
    fn kind(&mut self, path: &Path) -> io::Result<Kind>;
    /// A new file at `path`, which must not exist, open for writing.
    ///
    /// # Errors
    ///
    /// The file could not be created.
    fn create_new(&mut self, path: &Path) -> io::Result<()>;
    /// Every byte of `bytes` to the file `create_new` opened.
    ///
    /// # Errors
    ///
    /// The bytes could not all be written.
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// The open file's data and metadata to the disk, and the file closed.
    ///
    /// # Errors
    ///
    /// The disk did not take them.
    fn sync_all(&mut self) -> io::Result<()>;
    /// The open file closed, if one is.
    fn close(&mut self);
    /// `from`'s permissions copied onto `to`.
    ///
    /// # Errors
    ///
    /// They could not be read or set.
    fn copy_permissions(&mut self, from: &Path, to: &Path) -> io::Result<()>;
    /// `from` renamed over `to`, atomically.
    ///
    /// # Errors
    ///
    /// The rename did not happen.
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()>;
    /// The file `path` removed.
    ///
    /// # Errors
    ///
    /// It was not removed.
    fn remove(&mut self, path: &Path) -> io::Result<()>;
    /// The folder `folder`'s entries to the disk (Unix only).
    ///
    /// # Errors
    ///
    /// The disk did not take them.
    fn sync_folder(&mut self, folder: &Path) -> io::Result<()>;
    /// The file `path` names: itself, or, when it is a symbolic link, the real file the link
    /// leads to, so the real file is replaced and the link stays a link.
    ///
    /// # Errors
    ///
    /// The link could not be followed.
    fn resolve(&mut self, path: &Path) -> io::Result<PathBuf>;
    /// The temporary file beside `path` this write goes through: unique per process and per call
    /// ([`temp_of`] with the process-wide counter). A test's `Files` may name its own.
    fn temp_for(&mut self, path: &Path) -> Option<PathBuf> {
        temp_of(path, WRITES.fetch_add(1, Ordering::Relaxed))
    }
}

/// The writes this process has made: the counter in each temporary file's name.
static WRITES: AtomicU64 = AtomicU64::new(0);

/// The real file system: the only [`Files`] a shipped call site makes.
#[derive(Debug, Default)]
pub struct RealFiles {
    open: Option<File>,
}

impl Files for RealFiles {
    fn kind(&mut self, path: &Path) -> io::Result<Kind> {
        match fs::metadata(path) {
            Ok(meta) if meta.is_dir() => Ok(Kind::Folder),
            Ok(meta) => Ok(Kind::File {
                read_only: meta.permissions().readonly(),
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Kind::Absent),
            Err(error) => Err(error),
        }
    }

    fn create_new(&mut self, path: &Path) -> io::Result<()> {
        self.open = Some(OpenOptions::new().write(true).create_new(true).open(path)?);
        Ok(())
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self.open.as_mut() {
            Some(file) => file.write_all(bytes),
            None => Err(io::Error::other("no file is open to write to")),
        }
    }

    fn sync_all(&mut self) -> io::Result<()> {
        let synced = match self.open.as_mut() {
            Some(file) => file.sync_all(),
            None => Err(io::Error::other("no file is open to sync")),
        };
        self.open = None;
        synced
    }

    fn close(&mut self) {
        self.open = None;
    }

    fn copy_permissions(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        fs::set_permissions(to, fs::metadata(from)?.permissions())
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn remove(&mut self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn sync_folder(&mut self, folder: &Path) -> io::Result<()> {
        File::open(folder)?.sync_all()
    }

    fn resolve(&mut self, path: &Path) -> io::Result<PathBuf> {
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => fs::canonicalize(path),
            Ok(_) => Ok(path.to_path_buf()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path.to_path_buf()),
            Err(error) => Err(error),
        }
    }
}

/// The rules of the platform a write runs on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Platform {
    /// Refuse the names Windows cannot hold as a file (its reserved device names, a name ending
    /// in a dot or a space, a name with one of `< > : " | ? *` or a control character).
    pub windows_names: bool,
    /// Sync the folder after the rename, so the rename itself survives a crash (Unix).
    pub sync_folder: bool,
}

impl Platform {
    /// This build's platform: what [`write_whole`] runs with.
    pub const THIS: Self = Self {
        windows_names: cfg!(windows),
        sync_folder: cfg!(unix),
    };
}

/// Why a write or a removal did not happen, or did with something to say. Each variant says what
/// is true of the disk.
#[derive(Debug)]
pub enum WriteFailure {
    /// The path names no file (it ends in a separator or `..`). Nothing was touched.
    NoName {
        /// The path.
        path: PathBuf,
    },
    /// The name is one this platform reserves for a device (Windows: CON, PRN, AUX, NUL, COM1 to
    /// COM9, LPT1 to LPT9, before its first dot, any case). Nothing was touched.
    ReservedName {
        /// The path.
        path: PathBuf,
    },
    /// The name ends in a dot or a space, which Windows drops from a file name. Nothing was
    /// touched.
    BadEnding {
        /// The path.
        path: PathBuf,
    },
    /// The name holds a character Windows cannot hold in a file name (`< > : " | ? *` or a
    /// control character; a `:` would name an alternate data stream of another file). Nothing
    /// was touched.
    IllegalCharacter {
        /// The path.
        path: PathBuf,
    },
    /// The path is a folder. Nothing was touched.
    IsFolder {
        /// The path.
        path: PathBuf,
    },
    /// The folder the file would go in does not exist. Nothing was touched.
    MissingFolder {
        /// The folder.
        folder: PathBuf,
    },
    /// The file is read-only. Nothing was touched.
    ReadOnly {
        /// The path.
        path: PathBuf,
    },
    /// What is at the path could not be read. Nothing was touched.
    Inspect {
        /// The path looked at.
        path: PathBuf,
        /// The operating system's words.
        error: io::Error,
    },
    /// A temporary file of ours from a stopped write is in the way and could not be removed;
    /// the target is untouched.
    StaleTemp {
        /// The temporary file.
        temp: PathBuf,
        /// The operating system's words.
        error: io::Error,
    },
    /// A step before the rename failed: the target is untouched.
    BeforeRename {
        /// The step: `create`, `write`, `sync` or `permissions`.
        step: &'static str,
        /// The target.
        path: PathBuf,
        /// The operating system's words.
        error: io::Error,
        /// The temporary file, when it could not be removed.
        leftover: Option<PathBuf>,
    },
    /// The rename failed: the target is untouched.
    Rename {
        /// The target.
        path: PathBuf,
        /// The operating system's words.
        error: io::Error,
        /// The temporary file, when it could not be removed.
        leftover: Option<PathBuf>,
    },
    /// The new file is in place, and the folder could not be synced, so a crash now could still
    /// lose the rename.
    SyncFolder {
        /// The target, written.
        path: PathBuf,
        /// The operating system's words.
        error: io::Error,
    },
    /// The file could not be removed: it is as it was.
    Remove {
        /// The path.
        path: PathBuf,
        /// The operating system's words.
        error: io::Error,
    },
}

impl WriteFailure {
    /// Every variant's [`WriteFailure::name`], for the check that each has words.
    pub const NAMES: &[&str] = &[
        "no_name",
        "reserved_name",
        "bad_ending",
        "illegal_character",
        "is_folder",
        "missing_folder",
        "read_only",
        "inspect",
        "stale_temp",
        "before_rename",
        "rename",
        "sync_folder",
        "remove",
    ];

    /// The name the string table's `write_<name>` row goes by.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::NoName { .. } => "no_name",
            Self::ReservedName { .. } => "reserved_name",
            Self::BadEnding { .. } => "bad_ending",
            Self::IllegalCharacter { .. } => "illegal_character",
            Self::IsFolder { .. } => "is_folder",
            Self::MissingFolder { .. } => "missing_folder",
            Self::ReadOnly { .. } => "read_only",
            Self::Inspect { .. } => "inspect",
            Self::StaleTemp { .. } => "stale_temp",
            Self::BeforeRename { .. } => "before_rename",
            Self::Rename { .. } => "rename",
            Self::SyncFolder { .. } => "sync_folder",
            Self::Remove { .. } => "remove",
        }
    }

    /// The path the words name first: the target, the folder, or the temporary file.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::NoName { path }
            | Self::ReservedName { path }
            | Self::BadEnding { path }
            | Self::IllegalCharacter { path }
            | Self::IsFolder { path }
            | Self::ReadOnly { path }
            | Self::Inspect { path, .. }
            | Self::BeforeRename { path, .. }
            | Self::Rename { path, .. }
            | Self::SyncFolder { path, .. }
            | Self::Remove { path, .. } => path,
            Self::MissingFolder { folder } => folder,
            Self::StaleTemp { temp, .. } => temp,
        }
    }

    /// The operating system's words, when a step failed.
    #[must_use]
    pub fn error(&self) -> Option<&io::Error> {
        match self {
            Self::Inspect { error, .. }
            | Self::StaleTemp { error, .. }
            | Self::BeforeRename { error, .. }
            | Self::Rename { error, .. }
            | Self::SyncFolder { error, .. }
            | Self::Remove { error, .. } => Some(error),
            Self::NoName { .. }
            | Self::ReservedName { .. }
            | Self::BadEnding { .. }
            | Self::IllegalCharacter { .. }
            | Self::IsFolder { .. }
            | Self::MissingFolder { .. }
            | Self::ReadOnly { .. } => None,
        }
    }

    /// The step that failed before the rename, if that is what failed.
    #[must_use]
    pub const fn step(&self) -> Option<&'static str> {
        match self {
            Self::BeforeRename { step, .. } => Some(step),
            _ => None,
        }
    }

    /// A temporary file of ours left behind, named so the player can remove it.
    #[must_use]
    pub fn leftover(&self) -> Option<&Path> {
        match self {
            Self::BeforeRename { leftover, .. } | Self::Rename { leftover, .. } => {
                leftover.as_deref()
            }
            _ => None,
        }
    }
}

impl fmt::Display for WriteFailure {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path().display();
        match self {
            Self::NoName { .. } => write!(out, "{path} names no file; nothing was written"),
            Self::ReservedName { .. } => {
                write!(
                    out,
                    "{path} is a name this system keeps for a device; nothing was written"
                )
            }
            Self::BadEnding { .. } => {
                write!(out, "{path} ends in a dot or a space; nothing was written")
            }
            Self::IllegalCharacter { .. } => write!(
                out,
                "{path} holds a character this system cannot hold in a name; nothing was written"
            ),
            Self::IsFolder { .. } => write!(out, "{path} is a folder; nothing was written"),
            Self::MissingFolder { .. } => {
                write!(out, "the folder {path} does not exist; nothing was written")
            }
            Self::ReadOnly { .. } => write!(out, "{path} is read-only; it is as it was"),
            Self::Inspect { error, .. } => {
                write!(
                    out,
                    "{path} could not be looked at ({error}); nothing was written"
                )
            }
            Self::StaleTemp { error, .. } => write!(
                out,
                "{path}, left by a write that stopped, could not be removed ({error}); nothing was \
                 written"
            ),
            Self::BeforeRename { step, error, .. } => {
                write!(out, "the {step} step failed ({error}); {path} is as it was")
            }
            Self::Rename { error, .. } => {
                write!(
                    out,
                    "the new file could not replace {path} ({error}); it is as it was"
                )
            }
            Self::SyncFolder { error, .. } => write!(
                out,
                "{path} is written, and its folder could not be synced ({error})"
            ),
            Self::Remove { error, .. } => {
                write!(
                    out,
                    "{path} could not be removed ({error}); it is as it was"
                )
            }
        }
    }
}

impl std::error::Error for WriteFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.error()
            .map(|error| error as &(dyn std::error::Error + 'static))
    }
}

/// A write that happened, and what it has to say.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Written {
    /// A temporary file of ours from a stopped write, found and removed first.
    pub stale_removed: Option<PathBuf>,
}

/// Whether a removal found a file to remove.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Removed {
    /// It was there, and is gone.
    Removed,
    /// Nothing was there.
    Absent,
}

/// The device names Windows reserves, matched against a file name's part before its first dot
/// (and its trailing spaces), in any case.
const RESERVED: &[&str] = &[
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

/// The characters Windows cannot hold in a file name, besides the control characters.
const ILLEGAL: &[char] = &['<', '>', ':', '"', '|', '?', '*'];

/// Whether `name` holds a character Windows cannot hold in a file name, whatever system this is.
#[must_use]
pub fn has_windows_illegal_character(name: &str) -> bool {
    name.chars().any(|c| ILLEGAL.contains(&c) || c.is_control())
}

/// Whether `name` is one Windows reserves for a device, whatever system this is.
#[must_use]
pub fn is_windows_reserved(name: &str) -> bool {
    let base = match name.split_once('.') {
        Some((base, _)) => base,
        None => name,
    }
    .trim_end_matches(' ');
    RESERVED
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(base))
}

/// Whether `name` ends in a dot or a space, which Windows drops from a file name.
#[must_use]
pub fn has_windows_bad_ending(name: &str) -> bool {
    name.ends_with('.') || name.ends_with(' ')
}

/// The temporary file beside `path` for this process's write number `count`:
/// `<folder>/.<name>.<pid>.<count>` and [`TEMP_SUFFIX`]. If that exact name exists, it is ours,
/// from a write of this process (or of an earlier one with the same id) that stopped.
#[must_use]
pub fn temp_of(path: &Path, count: u64) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy();
    let folder = path.parent()?;
    Some(folder.join(format!(
        ".{name}.{}.{count}{TEMP_SUFFIX}",
        std::process::id()
    )))
}

/// `bytes` written to `path` whole, by an atomic replace, through the real file system.
///
/// # Errors
///
/// [`WriteFailure`], each variant saying what is on disk.
pub fn write_whole(path: &Path, bytes: &[u8]) -> Result<Written, WriteFailure> {
    write_whole_with(&mut RealFiles::default(), path, bytes, Platform::THIS)
}

/// The file `path` removed, through the real file system: a forgotten remembered line.
///
/// # Errors
///
/// [`WriteFailure`]: a folder, a read-only file, or a removal that failed.
pub fn remove_whole(path: &Path) -> Result<Removed, WriteFailure> {
    remove_whole_with(&mut RealFiles::default(), path)
}

/// [`write_whole`] through `files`, with the platform's rules given ([`Platform`]).
///
/// A target that is a symbolic link is resolved first, and the real file is replaced: the
/// temporary file is written beside it, so the link stays a link and the player's file gets the
/// bytes. A target with a second hard link is replaced under this name alone: the other name
/// keeps the old bytes, which is what an atomic replace means (`tests/write.rs` shows it on
/// Unix).
///
/// # Errors
///
/// [`WriteFailure`].
pub fn write_whole_with(
    files: &mut dyn Files,
    asked: &Path,
    bytes: &[u8],
    platform: Platform,
) -> Result<Written, WriteFailure> {
    // The name asked for is checked before anything looks at the disk: Windows answers a look
    // at a reserved device name with an error of its own. The resolved name is checked again.
    names(asked, platform.windows_names)?;
    let path = &files
        .resolve(asked)
        .map_err(|error| WriteFailure::Inspect {
            path: asked.to_path_buf(),
            error,
        })?;
    let owned = || path.clone();
    let (target, folder) = refusals(files, path, platform.windows_names)?;
    let Some(temp) = files.temp_for(path) else {
        return Err(WriteFailure::NoName { path: owned() });
    };
    let stale_removed = clear_stale(files, &temp)?;
    let before = |files: &mut dyn Files, step: &'static str, error: io::Error| {
        files.close();
        let leftover = files.remove(&temp).err().map(|_| temp.clone());
        WriteFailure::BeforeRename {
            step,
            path: owned(),
            error,
            leftover,
        }
    };
    if let Err(error) = files.create_new(&temp) {
        // Nothing of ours was made: there is nothing to remove.
        files.close();
        return Err(WriteFailure::BeforeRename {
            step: "create",
            path: owned(),
            error,
            leftover: None,
        });
    }
    if let Err(error) = files.write_all(bytes) {
        return Err(before(files, "write", error));
    }
    if let Err(error) = files.sync_all() {
        return Err(before(files, "sync", error));
    }
    if target != Kind::Absent
        && let Err(error) = files.copy_permissions(path, &temp)
    {
        return Err(before(files, "permissions", error));
    }
    if let Err(error) = files.rename(&temp, path) {
        let leftover = files.remove(&temp).err().map(|_| temp.clone());
        return Err(WriteFailure::Rename {
            path: owned(),
            error,
            leftover,
        });
    }
    if platform.sync_folder
        && let Err(error) = files.sync_folder(&folder)
    {
        return Err(WriteFailure::SyncFolder {
            path: owned(),
            error,
        });
    }
    Ok(Written { stale_removed })
}

/// Steps 1 and 2 of [`write_whole_with`]: the refusals made before anything is created. Answers
/// what is at the target and the folder it goes in.
fn refusals(
    files: &mut dyn Files,
    path: &Path,
    windows_names: bool,
) -> Result<(Kind, PathBuf), WriteFailure> {
    let owned = || path.to_path_buf();
    names(path, windows_names)?;
    let target = inspect(files, path)?;
    match target {
        Kind::Folder => return Err(WriteFailure::IsFolder { path: owned() }),
        Kind::File { read_only: true } => return Err(WriteFailure::ReadOnly { path: owned() }),
        Kind::Absent | Kind::File { read_only: false } => {}
    }
    // A path with no folder part is refused rather than read as the current folder: the client
    // always hands an absolute path.
    let folder = match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder.to_path_buf(),
        _ => return Err(WriteFailure::MissingFolder { folder: owned() }),
    };
    if inspect(files, &folder)? != Kind::Folder {
        return Err(WriteFailure::MissingFolder { folder });
    }
    Ok((target, folder))
}

/// The refusals of a name alone, made before anything looks at the disk.
fn names(path: &Path, windows_names: bool) -> Result<(), WriteFailure> {
    let owned = || path.to_path_buf();
    let Some(name) = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
    else {
        return Err(WriteFailure::NoName { path: owned() });
    };
    if windows_names && is_windows_reserved(&name) {
        return Err(WriteFailure::ReservedName { path: owned() });
    }
    if windows_names && has_windows_bad_ending(&name) {
        return Err(WriteFailure::BadEnding { path: owned() });
    }
    if windows_names && has_windows_illegal_character(&name) {
        return Err(WriteFailure::IllegalCharacter { path: owned() });
    }
    Ok(())
}

/// What is at `at`, or why that could not be read.
fn inspect(files: &mut dyn Files, at: &Path) -> Result<Kind, WriteFailure> {
    files.kind(at).map_err(|error| WriteFailure::Inspect {
        path: at.to_path_buf(),
        error,
    })
}

/// A temporary file of ours at `temp`, from a write that stopped, removed: answers its path when
/// there was one.
fn clear_stale(files: &mut dyn Files, temp: &Path) -> Result<Option<PathBuf>, WriteFailure> {
    match inspect(files, temp)? {
        Kind::Absent => Ok(None),
        Kind::File { .. } => {
            files
                .remove(temp)
                .map_err(|error| WriteFailure::StaleTemp {
                    temp: temp.to_path_buf(),
                    error,
                })?;
            Ok(Some(temp.to_path_buf()))
        }
        Kind::Folder => Err(WriteFailure::StaleTemp {
            temp: temp.to_path_buf(),
            error: io::Error::other("a folder has the temporary file's name"),
        }),
    }
}

/// [`remove_whole`] through `files`.
///
/// # Errors
///
/// [`WriteFailure`].
pub fn remove_whole_with(files: &mut dyn Files, path: &Path) -> Result<Removed, WriteFailure> {
    match files.kind(path).map_err(|error| WriteFailure::Inspect {
        path: path.to_path_buf(),
        error,
    })? {
        Kind::Absent => Ok(Removed::Absent),
        Kind::Folder => Err(WriteFailure::IsFolder {
            path: path.to_path_buf(),
        }),
        Kind::File { read_only: true } => Err(WriteFailure::ReadOnly {
            path: path.to_path_buf(),
        }),
        Kind::File { read_only: false } => {
            files.remove(path).map_err(|error| WriteFailure::Remove {
                path: path.to_path_buf(),
                error,
            })?;
            Ok(Removed::Removed)
        }
    }
}

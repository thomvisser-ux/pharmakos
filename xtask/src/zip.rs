// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A std-only zip writer, and a reader of the central directory it writes
//! (decisions-log item 117 (9)).
//!
//! `cargo xtask package` writes the two unsigned zips with this rather than
//! with `7z`, `zip` or `tar`, which would be a different tool per runner with
//! mode bits that depend on it. The format is the smallest subset of APPNOTE
//! that `unzip`, `bsdtar`, Windows' Expand-Archive and the file managers read:
//!
//! * every entry **stored** (method 0, no compression; compression is a
//!   PLACEHOLDER for hardening, item 117 (14)), with its CRC-32;
//! * local headers, a central directory and the end record, no data
//!   descriptors, no extra fields, no comments and **no zip64**: an entry, the
//!   archive or the entry count past the classic format's limits is an error
//!   that says so;
//! * "version made by" `0x0314` (Unix, 2.0), so extractors read the Unix mode
//!   from the high half of the external attributes; "version needed" 10 for a
//!   stored file and 20 for a directory; general-purpose flags 0;
//! * the Unix mode `0o100755` for the game executable and `gamectl`,
//!   `0o100644` for every other file, and `0o040755` plus the MS-DOS directory
//!   bit for every directory entry (a directory stored as `0644` extracts under
//!   `unzip` without its search bit, so nothing inside it can be opened);
//! * the fixed MS-DOS date `0x0021` (1980-01-01) and time `0x0000` on every
//!   entry, and the entries in byte order of their names, with forward slashes
//!   and every directory as its own entry — so a zip's bytes depend only on its
//!   files, and two writes of one tree are byte-identical.
//!
//! The reader reads exactly this subset back: the manifest golden
//! (`tests/golden/package/`) is the list of names in the central directory, and
//! the reuse check extracts the zip with it rather than with a tool that could
//! read it more forgivingly than a tester's would.

use std::fs;
use std::io::Write;
use std::path::Path;

/// The Unix mode of a file the tester runs: the game executable and `gamectl`.
pub(crate) const MODE_EXECUTABLE: u32 = 0o100_755;
/// The Unix mode of every other file.
pub(crate) const MODE_FILE: u32 = 0o100_644;
/// The Unix mode of a directory: searchable, so its contents can be opened.
pub(crate) const MODE_DIRECTORY: u32 = 0o040_755;
/// The MS-DOS attribute bit that marks a directory.
const DOS_DIRECTORY: u32 = 0x10;

/// "Version made by": Unix (3) in the high byte, specification 2.0 in the low.
const MADE_BY: u16 = 0x0314;
/// "Version needed to extract" for a stored file, and for a directory.
const NEEDED_FILE: u16 = 10;
const NEEDED_DIRECTORY: u16 = 20;
/// 1980-01-01, the MS-DOS epoch, and midnight: the same on every entry.
const DOS_DATE: u16 = 0x0021;
const DOS_TIME: u16 = 0x0000;

const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const END_SIGNATURE: u32 = 0x0605_4b50;
const LOCAL_HEADER_LEN: usize = 30;
const CENTRAL_HEADER_LEN: usize = 46;
const END_RECORD_LEN: usize = 22;

/// What an entry is, which decides its mode and its "version needed".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A file the tester runs.
    Executable,
    /// Any other file.
    File,
    /// A directory, stored as its own entry with a name ending in `/`.
    Directory,
}

/// The external attributes an entry of `kind` carries: the Unix mode in the
/// high 16 bits, and for a directory the MS-DOS directory bit as well.
pub(crate) const fn external_attributes(kind: Kind) -> u32 {
    match kind {
        Kind::Executable => MODE_EXECUTABLE << 16,
        Kind::File => MODE_FILE << 16,
        Kind::Directory => (MODE_DIRECTORY << 16) | DOS_DIRECTORY,
    }
}

/// The CRC-32 lookup table (IEEE 802.3, reflected polynomial `0xEDB88320`).
fn crc_table() -> [u32; 256] {
    std::array::from_fn(|slot| {
        let mut value = u32::try_from(slot).unwrap_or_default();
        for _ in 0..8 {
            value = if value & 1 == 1 {
                (value >> 1) ^ 0xEDB8_8320
            } else {
                value >> 1
            };
        }
        value
    })
}

/// The CRC-32 of `bytes`, as zip stores it.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let table = crc_table();
    let mut crc: u32 = 0xFFFF_FFFF;
    for byte in bytes {
        let low = (crc ^ u32::from(*byte)) & 0xFF;
        let slot = usize::try_from(low).unwrap_or_default();
        let entry = table.get(slot).copied().unwrap_or_default();
        crc = (crc >> 8) ^ entry;
    }
    !crc
}

/// One entry to write: its name in the zip and where its bytes come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Source {
    /// The entry's name: forward slashes, a directory ending in `/`.
    pub(crate) name: String,
    /// What it is.
    pub(crate) kind: Kind,
    /// The file to read, for a file entry; `None` for a directory.
    pub(crate) path: Option<std::path::PathBuf>,
}

/// Every entry of the tree under `root`, named `<prefix>/<relative path>`:
/// every directory (the prefix itself included) and every file, sorted in byte
/// order. `executable` says which relative paths are [`Kind::Executable`].
pub(crate) fn tree(root: &Path, prefix: &str, executable: &[&str]) -> Result<Vec<Source>, String> {
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    crate::walk(root, &mut files)
        .map_err(|error| format!("reading {}: {error}", root.display()))?;
    let mut entries: Vec<Source> = Vec::new();
    let mut directories: Vec<String> = vec![format!("{prefix}/")];
    for file in files {
        let relative = file
            .strip_prefix(root)
            .map_err(|error| format!("{}: {error}", file.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        let mut parent = relative.as_str();
        while let Some((above, _)) = parent.rsplit_once('/') {
            directories.push(format!("{prefix}/{above}/"));
            parent = above;
        }
        let kind = if executable.contains(&relative.as_str()) {
            Kind::Executable
        } else {
            Kind::File
        };
        entries.push(Source {
            name: format!("{prefix}/{relative}"),
            kind,
            path: Some(file),
        });
    }
    directories.sort();
    directories.dedup();
    for name in directories {
        entries.push(Source {
            name,
            kind: Kind::Directory,
            path: None,
        });
    }
    entries.sort_by(|left, right| left.name.as_bytes().cmp(right.name.as_bytes()));
    Ok(entries)
}

/// The central-directory record of one written entry.
struct Written {
    name: Vec<u8>,
    kind: Kind,
    crc: u32,
    size: u32,
    offset: u32,
}

fn u16_of(value: usize, what: &str) -> Result<u16, String> {
    u16::try_from(value).map_err(|_| {
        format!("{what} is {value}, past the classic zip format's 65 535; this writer has no zip64")
    })
}

fn u32_of(value: usize, what: &str) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| {
        format!(
            "{what} is {value} bytes, past the classic zip format's 4 GiB; this writer has no zip64"
        )
    })
}

/// Writes `entries`, in the order given, as a zip's bytes. Pure over the bytes
/// it is handed, so two calls over one tree produce the same output.
pub(crate) fn write(entries: &[(String, Kind, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::new();
    let mut written: Vec<Written> = Vec::new();
    for (name, kind, data) in entries {
        let named = if *kind == Kind::Directory && !name.ends_with('/') {
            format!("{name}/")
        } else {
            name.clone()
        };
        if named.contains('\\') || named.starts_with('/') {
            return Err(format!(
                "zip entry `{named}` must be relative, with forward slashes"
            ));
        }
        let data: &[u8] = if *kind == Kind::Directory { &[] } else { data };
        let offset = u32_of(out.len(), "the archive")?;
        let size = u32_of(data.len(), &format!("`{named}`"))?;
        let crc = crc32(data);
        let name_bytes = named.into_bytes();
        let needed = if *kind == Kind::Directory {
            NEEDED_DIRECTORY
        } else {
            NEEDED_FILE
        };
        push_u32(&mut out, LOCAL_SIGNATURE);
        push_u16(&mut out, needed);
        push_u16(&mut out, 0); // flags
        push_u16(&mut out, 0); // stored
        push_u16(&mut out, DOS_TIME);
        push_u16(&mut out, DOS_DATE);
        push_u32(&mut out, crc);
        push_u32(&mut out, size);
        push_u32(&mut out, size);
        push_u16(
            &mut out,
            u16_of(name_bytes.len(), "an entry name's length")?,
        );
        push_u16(&mut out, 0); // extra
        out.extend_from_slice(&name_bytes);
        out.extend_from_slice(data);
        written.push(Written {
            name: name_bytes,
            kind: *kind,
            crc,
            size,
            offset,
        });
    }
    let directory_start = out.len();
    for entry in &written {
        let needed = if entry.kind == Kind::Directory {
            NEEDED_DIRECTORY
        } else {
            NEEDED_FILE
        };
        push_u32(&mut out, CENTRAL_SIGNATURE);
        push_u16(&mut out, MADE_BY);
        push_u16(&mut out, needed);
        push_u16(&mut out, 0); // flags
        push_u16(&mut out, 0); // stored
        push_u16(&mut out, DOS_TIME);
        push_u16(&mut out, DOS_DATE);
        push_u32(&mut out, entry.crc);
        push_u32(&mut out, entry.size);
        push_u32(&mut out, entry.size);
        push_u16(
            &mut out,
            u16_of(entry.name.len(), "an entry name's length")?,
        );
        push_u16(&mut out, 0); // extra
        push_u16(&mut out, 0); // comment
        push_u16(&mut out, 0); // disk
        push_u16(&mut out, 0); // internal attributes
        push_u32(&mut out, external_attributes(entry.kind));
        push_u32(&mut out, entry.offset);
        out.extend_from_slice(&entry.name);
    }
    let directory_len = out.len() - directory_start;
    let count = u16_of(written.len(), "the entry count")?;
    push_u32(&mut out, END_SIGNATURE);
    push_u16(&mut out, 0); // this disk
    push_u16(&mut out, 0); // the directory's disk
    push_u16(&mut out, count);
    push_u16(&mut out, count);
    push_u32(&mut out, u32_of(directory_len, "the central directory")?);
    push_u32(&mut out, u32_of(directory_start, "the archive")?);
    push_u16(&mut out, 0); // comment
    u32_of(out.len(), "the archive")?;
    Ok(out)
}

/// Reads every source's bytes and writes the zip to `destination`.
pub(crate) fn write_file(entries: &[Source], destination: &Path) -> Result<u64, String> {
    let mut loaded: Vec<(String, Kind, Vec<u8>)> = Vec::new();
    for entry in entries {
        let data = match &entry.path {
            Some(path) => {
                fs::read(path).map_err(|error| format!("reading {}: {error}", path.display()))?
            }
            None => Vec::new(),
        };
        loaded.push((entry.name.clone(), entry.kind, data));
    }
    let bytes = write(&loaded)?;
    let mut file = fs::File::create(destination)
        .map_err(|error| format!("creating {}: {error}", destination.display()))?;
    file.write_all(&bytes)
        .map_err(|error| format!("writing {}: {error}", destination.display()))?;
    u64::try_from(bytes.len()).map_err(|error| format!("the zip's size: {error}"))
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// One entry, as the central directory records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Listed {
    /// The entry's name.
    pub(crate) name: String,
    /// The external attributes: the Unix mode in the high half.
    pub(crate) external: u32,
    /// The CRC-32 the directory records.
    pub(crate) crc: u32,
    /// The stored size.
    pub(crate) size: u32,
    /// Where the local header is.
    pub(crate) offset: u32,
}

impl Listed {
    /// Whether the entry is a directory.
    pub(crate) fn is_directory(&self) -> bool {
        self.name.ends_with('/')
    }
}

fn read_u16(bytes: &[u8], at: usize) -> Result<u16, String> {
    let slice = bytes
        .get(at..at + 2)
        .ok_or_else(|| format!("the zip ends inside a field at byte {at}"))?;
    let mut array = [0_u8; 2];
    array.copy_from_slice(slice);
    Ok(u16::from_le_bytes(array))
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, String> {
    let slice = bytes
        .get(at..at + 4)
        .ok_or_else(|| format!("the zip ends inside a field at byte {at}"))?;
    let mut array = [0_u8; 4];
    array.copy_from_slice(slice);
    Ok(u32::from_le_bytes(array))
}

fn index_of(value: u32) -> Result<usize, String> {
    usize::try_from(value).map_err(|error| format!("a zip offset does not fit: {error}"))
}

/// The central directory of a zip this module wrote: every entry, in the
/// order the directory lists them. Only the subset [`write`] produces is
/// accepted — no comment, stored entries only.
pub(crate) fn list(bytes: &[u8]) -> Result<Vec<Listed>, String> {
    let end = bytes
        .len()
        .checked_sub(END_RECORD_LEN)
        .ok_or_else(|| "the file is too short to be a zip".to_owned())?;
    if read_u32(bytes, end)? != END_SIGNATURE {
        return Err(
            "no end-of-central-directory record where this writer puts it (a zip \
                    comment, or not one of ours)"
                .to_owned(),
        );
    }
    let count = usize::from(read_u16(bytes, end + 10)?);
    let mut at = index_of(read_u32(bytes, end + 16)?)?;
    let mut listed: Vec<Listed> = Vec::new();
    for _ in 0..count {
        if read_u32(bytes, at)? != CENTRAL_SIGNATURE {
            return Err(format!("no central-directory header at byte {at}"));
        }
        let method = read_u16(bytes, at + 10)?;
        if method != 0 {
            return Err(format!(
                "entry at byte {at} is compressed (method {method})"
            ));
        }
        let crc = read_u32(bytes, at + 16)?;
        let size = read_u32(bytes, at + 24)?;
        let name_len = usize::from(read_u16(bytes, at + 28)?);
        let extra_len = usize::from(read_u16(bytes, at + 30)?);
        let comment_len = usize::from(read_u16(bytes, at + 32)?);
        let external = read_u32(bytes, at + 38)?;
        let offset = read_u32(bytes, at + 42)?;
        let name_start = at + CENTRAL_HEADER_LEN;
        let name = bytes
            .get(name_start..name_start + name_len)
            .ok_or_else(|| format!("entry name past the end at byte {name_start}"))?;
        let name = String::from_utf8(name.to_vec())
            .map_err(|error| format!("entry name is not UTF-8: {error}"))?;
        listed.push(Listed {
            name,
            external,
            crc,
            size,
            offset,
        });
        at = name_start + name_len + extra_len + comment_len;
    }
    Ok(listed)
}

/// The bytes of one listed file entry, checked against its CRC-32.
pub(crate) fn data<'a>(bytes: &'a [u8], entry: &Listed) -> Result<&'a [u8], String> {
    let at = index_of(entry.offset)?;
    if read_u32(bytes, at)? != LOCAL_SIGNATURE {
        return Err(format!("no local header for `{}` at byte {at}", entry.name));
    }
    let name_len = usize::from(read_u16(bytes, at + 26)?);
    let extra_len = usize::from(read_u16(bytes, at + 28)?);
    let start = at + LOCAL_HEADER_LEN + name_len + extra_len;
    let size = index_of(entry.size)?;
    let data = bytes
        .get(start..start + size)
        .ok_or_else(|| format!("`{}` runs past the end of the zip", entry.name))?;
    if crc32(data) != entry.crc {
        return Err(format!("`{}` fails its CRC-32", entry.name));
    }
    Ok(data)
}

/// Whether an entry's name stays inside the folder it is extracted into: a
/// relative path of plain parts, with no backslash, no drive or root, and no
/// empty, `.` or `..` part. A colon is refused on every platform, because on
/// Windows `C:x` joined onto a folder replaces it.
fn stays_inside(name: &str) -> bool {
    use std::path::Component;
    let trimmed = name.strip_suffix('/').unwrap_or(name);
    !trimmed.is_empty()
        && !name.contains('\\')
        && !name.contains(':')
        && trimmed
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && Path::new(trimmed)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// Extracts every entry of `bytes` under `destination`, which must exist.
/// Names are checked to stay inside it ([`stays_inside`]).
pub(crate) fn extract(bytes: &[u8], destination: &Path) -> Result<usize, String> {
    let entries = list(bytes)?;
    for entry in &entries {
        if !stays_inside(&entry.name) {
            return Err(format!("zip entry `{}` would leave the folder", entry.name));
        }
        let target = destination.join(entry.name.trim_end_matches('/'));
        if entry.is_directory() {
            fs::create_dir_all(&target)
                .map_err(|error| format!("creating {}: {error}", target.display()))?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("creating {}: {error}", parent.display()))?;
            }
            fs::write(&target, data(bytes, entry)?)
                .map_err(|error| format!("writing {}: {error}", target.display()))?;
        }
    }
    Ok(entries.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crc_has_its_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn the_three_attribute_values_are_the_unix_modes() {
        assert_eq!(external_attributes(Kind::Executable), 0o100_755 << 16);
        assert_eq!(external_attributes(Kind::File), 0o100_644 << 16);
        assert_eq!(
            external_attributes(Kind::Directory),
            (0o040_755 << 16) | 0x10
        );
        assert_eq!(external_attributes(Kind::Executable), 0x81ED_0000);
        assert_eq!(external_attributes(Kind::File), 0x81A4_0000);
        assert_eq!(external_attributes(Kind::Directory), 0x41ED_0010);
    }

    fn sample() -> Vec<(String, Kind, Vec<u8>)> {
        vec![
            ("Pharmakos/".to_owned(), Kind::Directory, Vec::new()),
            (
                "Pharmakos/README.txt".to_owned(),
                Kind::File,
                b"hello\n".to_vec(),
            ),
            (
                "Pharmakos/gamectl".to_owned(),
                Kind::Executable,
                vec![0, 1, 2, 3, 255],
            ),
            ("Pharmakos/rules/".to_owned(), Kind::Directory, Vec::new()),
        ]
    }

    #[test]
    fn a_zip_round_trips_through_the_reader() {
        let bytes = write(&sample()).expect("writes");
        let listed = list(&bytes).expect("lists");
        let names: Vec<&str> = listed.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Pharmakos/",
                "Pharmakos/README.txt",
                "Pharmakos/gamectl",
                "Pharmakos/rules/"
            ]
        );
        let modes: Vec<u32> = listed.iter().map(|entry| entry.external).collect();
        assert_eq!(
            modes,
            [
                external_attributes(Kind::Directory),
                external_attributes(Kind::File),
                external_attributes(Kind::Executable),
                external_attributes(Kind::Directory)
            ]
        );
        let readme = listed.get(1).expect("an entry");
        assert_eq!(data(&bytes, readme).expect("reads"), b"hello\n");
        let binary = listed.get(2).expect("an entry");
        assert_eq!(data(&bytes, binary).expect("reads"), [0, 1, 2, 3, 255]);

        // A flipped byte fails its CRC.
        let mut broken = bytes.clone();
        let at =
            usize::try_from(readme.offset).expect("fits") + LOCAL_HEADER_LEN + readme.name.len();
        if let Some(byte) = broken.get_mut(at) {
            *byte ^= 1;
        }
        assert!(data(&broken, readme).is_err());
    }

    #[test]
    fn two_writes_of_one_tree_are_byte_identical() {
        let first = write(&sample()).expect("writes");
        let second = write(&sample()).expect("writes");
        assert_eq!(first, second);
        // The fixed date and time are in every header.
        let listed = list(&first).expect("lists");
        for entry in &listed {
            let at = usize::try_from(entry.offset).expect("fits");
            assert_eq!(read_u16(&first, at + 10).expect("time"), DOS_TIME);
            assert_eq!(read_u16(&first, at + 12).expect("date"), DOS_DATE);
        }
    }

    #[test]
    fn a_tree_is_listed_with_its_directories_in_byte_order() {
        let root = std::env::temp_dir().join(format!("pharmakos-xtask-zip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("rules")).expect("dir");
        fs::create_dir_all(root.join("LICENSES")).expect("dir");
        fs::write(root.join("gamectl"), b"x").expect("write");
        fs::write(root.join("rules").join("rules.v1.json"), b"{}").expect("write");
        fs::write(root.join("LICENSES").join("MIT.txt"), b"m").expect("write");
        let entries = tree(&root, "Pharmakos", &["gamectl"]).expect("tree");
        let _ = fs::remove_dir_all(&root);
        let names: Vec<(&str, Kind)> = entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind))
            .collect();
        assert_eq!(
            names,
            [
                ("Pharmakos/", Kind::Directory),
                ("Pharmakos/LICENSES/", Kind::Directory),
                ("Pharmakos/LICENSES/MIT.txt", Kind::File),
                ("Pharmakos/gamectl", Kind::Executable),
                ("Pharmakos/rules/", Kind::Directory),
                ("Pharmakos/rules/rules.v1.json", Kind::File),
            ]
        );
    }

    #[test]
    fn an_entry_that_would_leave_the_folder_is_refused() {
        for name in [
            "../evil",
            "Pharmakos/../../evil",
            "/evil",
            "C:evil",
            "C:/evil",
            "Pharmakos/C:/evil",
            "Pharmakos\\evil",
            "Pharmakos/./evil",
            "Pharmakos//evil",
            "",
            "/",
        ] {
            assert!(!stays_inside(name), "{name}");
        }
        for name in [
            "Pharmakos/",
            "Pharmakos/rules/rules.v1.json",
            "Pharmakos/gamectl.exe",
        ] {
            assert!(stays_inside(name), "{name}");
        }
        let bytes = write(&[("../evil".to_owned(), Kind::File, b"x".to_vec())]).expect("writes");
        let root =
            std::env::temp_dir().join(format!("pharmakos-xtask-unzip-{}", std::process::id()));
        fs::create_dir_all(&root).expect("dir");
        assert!(extract(&bytes, &root).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}

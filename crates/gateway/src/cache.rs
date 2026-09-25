// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The private match cache: plain local state, in the OS-standard per-user data
//! directory.
//!
//! Decisions-log item 98 settles where it goes, with the standard library alone
//! because three environment variables cover it and a `directories`-style crate
//! is off the approved list (AGENTS.md section 3 rule 5):
//!
//! | Platform | Root |
//! |---|---|
//! | Windows | `%LOCALAPPDATA%\Pharmakos\matches\<match-id>\` |
//! | Linux | `$XDG_DATA_HOME/pharmakos/matches/<match-id>/`, default `~/.local/share` |
//! | macOS | `~/Library/Application Support/Pharmakos/matches/<match-id>/` |
//!
//! **A missing variable is an error the lobby shows, never a fallback beside the
//! executable.** An unsigned zip writing into its own folder fails on a
//! read-only install path, which is the failure mode item 98 rejected the
//! alternative for.
//!
//! # This is enforcement, not cryptography, and it says so
//!
//! Spec section 12: "On a local host this is enforcement, not cryptography: the
//! private match cache is plain local state." Anyone with filesystem access can
//! read this folder. Nothing here encrypts, nothing here claims to, and
//! [`README_TEXT`] -- written into every match folder -- says so in words a
//! tester will read before sending a folder to somebody. The secrecy guarantee
//! spec section 12 makes is that *the gateway* never hands one seat another
//! seat's playbook, draft, notebook or replay; it is not a guarantee about the
//! disk, and the two must not be confused.
//!
//! # The layout
//!
//! ```text
//! <root>/Pharmakos/matches/<match-id>/
//!   README.txt                       what this folder is, and that it is unencrypted
//!   match.json                       the match header: id, seed, gateway version
//!   audit.log                        the access log (crate::audit), header plus one record a line
//!   seats/<seat>/notebook.txt        the private seat notebook, 4,000 characters   (T13)
//!   seats/<seat>/drafts/             saved drafts, one JSONC file each             (T13)
//!   seats/<seat>/sealed/<round>.jsonc the playbook each seat sealed, per round   (T17)
//!   replay/<round>.hashes.txt        the segment's per-tick hash chain             (T17)
//!   save.json                        the latest save, at a Lull boundary           (T17)
//! ```
//!
//! T9 creates the match folder, writes `README.txt` and `match.json`, and
//! appends to `audit.log`. `seats/` and `replay/` are created empty at
//! [`MatchCache::open`] so the layout is visible from the first match rather
//! than appearing a stage at a time; a seat's own `drafts/` and `sealed/` are
//! made by [`MatchCache::seat_directory`], when that seat first has state.
//!
//! T17 fills the rest (decisions-log item 84; the wave-6 notes, decisions C8
//! to C11). `match.json` gains the settings and the rules hash, so that with
//! the sealed files and the hash chains the folder holds the private replay's
//! inputs -- seed, playbooks, hashes -- and a test re-hosts a match from them
//! alone ([`crate::save`] says what the replay is not). `save.json` is written
//! by the host loop through [`MatchCache::write_save`], atomically, and read
//! back only by a resume, through [`MatchCache::reopen`] and
//! [`MatchCache::read_save`]. A seat's drafts and notebook are not written as
//! files of their own: they live in the save, which is the one place a resume
//! reads them from, and `drafts/` stays empty.
//!
//! **A save outlives its process.** [`MatchCache::open`] refuses a match id
//! whose folder already holds `save.json` (the wave-6 notes, H16): a new match
//! never overwrites a saved one, and the only way back into that folder is a
//! resume.
//!
//! # Readers
//!
//! [`MatchCache::read_save`], [`MatchCache::reopen`] and
//! [`MatchCache::audit_text`] are host-side. No method handler reaches any of
//! them (`no_handler_reads_the_save_or_the_replay` in `tests/confinement.rs`),
//! and no wire method returns a save or a replay in v1, which is what "no
//! other seat's token can read it" comes to on a local host.
//!
//! # The one place the gateway touches the filesystem
//!
//! Deliberately. `tests/confinement.rs` asserts that `std::fs` appears in this
//! module and nowhere else in the crate, which is what makes "no filesystem
//! access through playbooks" (AGENTS.md section 7) checkable rather than
//! asserted: every path this crate opens is built from a
//! [`MatchCache::valid_match_id`] match id and a fixed name, and no caller-
//! supplied text ever reaches a path component.

use crate::audit::Entry;
use crate::error::Error;
use pharmakos_sim::tables::SeatId;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// The application directory's name on Windows and macOS, where convention is
/// title case.
pub const APP_DIR: &str = "Pharmakos";

/// The application directory's name on Linux, where convention is lower case.
pub const APP_DIR_LOWER: &str = "pharmakos";

/// The folder holding one folder per match.
pub const MATCHES_DIR: &str = "matches";

/// The longest a match id may be.
pub const MAX_MATCH_ID: usize = 64;

/// The save's file name inside a match folder.
pub const SAVE_FILE: &str = "save.json";

/// The name a save is written under before it is renamed into place.
const SAVE_TEMPORARY: &str = "save.json.partial";

/// What `match.json` says about a match: everything needed to host it again
/// from nothing, the playbooks apart.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Header {
    /// The match seed, which is also the map seed.
    pub match_seed: u64,
    /// How many seats.
    pub seats: u32,
    /// The Push lengths in game milliseconds, one per round; empty is the
    /// rules table's own ladder.
    pub segment_lengths_ms: Vec<i32>,
    /// How many rounds at most.
    pub round_limit: u32,
    /// The rules table's hash.
    pub rules_hash: u64,
}

/// What every match folder says about itself.
///
/// Written once, when the folder is made. A tester who zips this folder up and
/// sends it to somebody has been told what is in it.
pub const README_TEXT: &str = "\
This folder is one Pharmakos match's private cache.

It is PLAIN LOCAL STATE and it is NOT ENCRYPTED. Anyone who can read this
directory can read everything in it: the seats' playbooks, their drafts, their
notebooks and the private replay.

The secrecy the game promises is that the Seat Gateway never hands one seat
another seat's playbook, draft, notebook or replay while a match is running.
That is enforcement inside one process on one machine. It is not a claim about
this folder, and nothing here is protected by cryptography.

Do not send this folder to anyone you would not show every seat's orders to.
The playtest bundle is the thing to send instead: it carries the survey, the
metrics, the recording and the diagnostics log, and none of the above.
";

/// Which operating system's convention to follow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Os {
    /// `%LOCALAPPDATA%`.
    Windows,
    /// `$XDG_DATA_HOME`, or `$HOME/.local/share`.
    Linux,
    /// `$HOME/Library/Application Support`.
    Macos,
}

/// The operating system this build runs on.
#[must_use]
pub const fn host_os() -> Os {
    #[cfg(target_os = "windows")]
    {
        Os::Windows
    }
    #[cfg(target_os = "macos")]
    {
        Os::Macos
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Os::Linux
    }
}

/// The per-user data root, resolved from environment variables.
///
/// `get` is the lookup, so the rule can be tested for all three operating
/// systems on any one of them -- which matters, because the failure this
/// function has to get right is the one that only happens on somebody else's
/// machine.
///
/// # Errors
///
/// [`crate::error::Code::Internal`] naming the variable that is missing. There
/// is no fallback: item 98 says a missing variable is an error the lobby shows.
pub fn data_root(get: impl Fn(&str) -> Option<String>, os: Os) -> Result<PathBuf, Error> {
    let missing = |name: &str| {
        Error::internal(format!(
            "the environment variable {name} is not set, so there is nowhere standard to keep \
             this match's private cache. Pharmakos will not write beside its own executable: on \
             a read-only install path that fails, and on a shared one it puts one player's \
             orders in another's reach."
        ))
    };
    match os {
        Os::Windows => {
            let local = get("LOCALAPPDATA").ok_or_else(|| missing("LOCALAPPDATA"))?;
            Ok(PathBuf::from(local).join(APP_DIR))
        }
        Os::Linux => {
            if let Some(xdg) = get("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
                return Ok(PathBuf::from(xdg).join(APP_DIR_LOWER));
            }
            let home = get("HOME").ok_or_else(|| missing("HOME"))?;
            Ok(PathBuf::from(home)
                .join(".local")
                .join("share")
                .join(APP_DIR_LOWER))
        }
        Os::Macos => {
            let home = get("HOME").ok_or_else(|| missing("HOME"))?;
            Ok(PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join(APP_DIR))
        }
    }
}

/// The per-user data root on this machine.
///
/// # Errors
///
/// As [`data_root`].
pub fn locate() -> Result<PathBuf, Error> {
    data_root(|name| std::env::var(name).ok(), host_os())
}

/// One match's folder on disk.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MatchCache {
    directory: PathBuf,
    match_id: String,
}

impl MatchCache {
    /// True when `id` is a match id this gateway will build a path from.
    ///
    /// Lower-case ASCII letters, digits and hyphens, between 1 and
    /// [`MAX_MATCH_ID`] characters. No dot, no separator, no colon, no drive
    /// letter, nothing that a filesystem reads as a traversal or a device.
    ///
    /// This is the narrowest gate in the crate, and it is narrow on purpose:
    /// it is the only place a string decides part of a path.
    #[must_use]
    pub fn valid_match_id(id: &str) -> bool {
        !id.is_empty()
            && id.len() <= MAX_MATCH_ID
            && id.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
            })
    }

    /// Open -- and if necessary create -- the folder for one **new** match
    /// under `data_root`.
    ///
    /// Writes [`README_TEXT`] and a `match.json` header, and creates `seats/`
    /// and `replay/` so the layout is visible from the start. The per-seat
    /// `drafts/` and `sealed/` are made by [`MatchCache::seat_directory`], when
    /// a seat first has state.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a match id
    /// [`MatchCache::valid_match_id`] refuses, and for one whose folder holds a
    /// save: a new match never overwrites a saved one, and a lobby that meant
    /// to go on with it asks for a resume. [`crate::error::Code::Internal`] for
    /// anything the filesystem refuses.
    pub fn open(data_root: &Path, match_id: &str, header: &Header) -> Result<MatchCache, Error> {
        let cache = MatchCache::at(data_root, match_id)?;
        if cache.has_save() {
            return Err(Error::invalid(format!(
                "match `{match_id}` has a save in the private match cache, and a new match does \
                 not overwrite a saved one: resume it, or start a match with another id"
            )));
        }

        for path in [
            cache.directory.clone(),
            cache.directory.join("seats"),
            cache.directory.join("replay"),
        ] {
            fs::create_dir_all(&path).map_err(|error| {
                Error::internal(format!("creating {}: {error}", path.display()))
            })?;
        }

        cache.write(Path::new("README.txt"), README_TEXT.as_bytes())?;
        let lengths = header
            .segment_lengths_ms
            .iter()
            .map(i32::to_string)
            .collect::<Vec<String>>()
            .join(", ");
        let text = format!(
            "{{\n  \"match_id\": \"{}\",\n  \"match_seed\": \"0x{:016x}\",\n  \
             \"seats\": {},\n  \"segment_lengths_ms\": [{lengths}],\n  \
             \"round_limit\": {},\n  \"rules_hash\": \"{}\",\n  \
             \"gateway_version\": {},\n  \"encrypted\": false\n}}\n",
            cache.match_id,
            header.match_seed,
            header.seats,
            header.round_limit,
            pharmakos_sim::hex(header.rules_hash),
            crate::GATEWAY_VERSION
        );
        cache.write(Path::new("match.json"), text.as_bytes())?;
        Ok(cache)
    }

    /// Open the folder of a **saved** match, to resume it.
    ///
    /// Writes nothing: `match.json` is the match's, from the day it was
    /// opened, and a resume does not rewrite it (the wave-6 notes, H16). The
    /// host loop appends a `resumed` line to `audit.log` once the match is up.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a match id
    /// [`MatchCache::valid_match_id`] refuses, and for one with no save to
    /// resume: a lobby's resume line naming a match that was never saved is
    /// the lobby's mistake to show, not the host's failure.
    pub fn reopen(data_root: &Path, match_id: &str) -> Result<MatchCache, Error> {
        let cache = MatchCache::at(data_root, match_id)?;
        if !cache.has_save() {
            return Err(Error::invalid(format!(
                "there is no saved match `{match_id}` to resume"
            )));
        }
        Ok(cache)
    }

    /// The cache for a match id, validated, without touching the disk.
    fn at(data_root: &Path, match_id: &str) -> Result<MatchCache, Error> {
        if !MatchCache::valid_match_id(match_id) {
            return Err(Error::invalid(format!(
                "`{match_id}` is not a match id: lower-case letters, digits and hyphens only, \
                 at most {MAX_MATCH_ID} characters"
            )));
        }
        Ok(MatchCache {
            directory: data_root.join(MATCHES_DIR).join(match_id),
            match_id: match_id.to_owned(),
        })
    }

    /// True when the match folder holds a save.
    #[must_use]
    pub fn has_save(&self) -> bool {
        self.directory.join(SAVE_FILE).is_file()
    }

    /// Write the save, replacing any earlier one **atomically**: the text goes
    /// to a temporary file beside it, which is then renamed over `save.json`,
    /// so a host that dies mid-write leaves the previous save whole rather
    /// than half of a new one.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for anything the filesystem refuses.
    pub fn write_save(&self, text: &str) -> Result<(), Error> {
        let temporary = self.directory.join(SAVE_TEMPORARY);
        let path = self.directory.join(SAVE_FILE);
        fs::write(&temporary, text.as_bytes()).map_err(|error| {
            Error::internal(format!("writing {}: {error}", temporary.display()))
        })?;
        fs::rename(&temporary, &path).map_err(|error| {
            Error::internal(format!(
                "moving {} into place as {}: {error}",
                temporary.display(),
                path.display()
            ))
        })
    }

    /// The save's text, for a resume.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] when there is no save, or it is
    /// not text.
    pub fn read_save(&self) -> Result<String, Error> {
        let path = self.directory.join(SAVE_FILE);
        fs::read_to_string(&path).map_err(|error| {
            Error::invalid(format!(
                "match `{}` has no save this host can read: {error}",
                self.match_id
            ))
        })
    }

    /// One seat's sealed playbook for one round, into
    /// `seats/<seat>/sealed/<round>.jsonc`. Both parts of the name are
    /// numbers from the sim, never a caller's text.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for anything the filesystem refuses.
    pub fn write_sealed(
        &self,
        seat: SeatId,
        round: u32,
        playbook_jsonc: &str,
    ) -> Result<(), Error> {
        let path = self
            .seat_directory(seat)?
            .join("sealed")
            .join(format!("{round}.jsonc"));
        fs::write(&path, playbook_jsonc.as_bytes())
            .map_err(|error| Error::internal(format!("writing {}: {error}", path.display())))
    }

    /// One segment's per-tick hash chain, into `replay/<round>.hashes.txt`.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for anything the filesystem refuses.
    pub fn write_replay(&self, round: u32, hashes: &str) -> Result<(), Error> {
        let folder = self.directory.join("replay");
        fs::create_dir_all(&folder)
            .map_err(|error| Error::internal(format!("creating {}: {error}", folder.display())))?;
        let path = folder.join(format!("{round}.hashes.txt"));
        fs::write(&path, hashes.as_bytes())
            .map_err(|error| Error::internal(format!("writing {}: {error}", path.display())))
    }

    /// The sequence number of the last line `audit.log` holds, or zero.
    ///
    /// A host that resumes a match, or opens a folder an earlier process
    /// wrote into, numbers its own lines after it: an audit entry's sequence
    /// number is "never reused" ([`crate::audit::Entry::seq`]).
    #[must_use]
    pub fn last_audit_seq(&self) -> u64 {
        let Ok(text) = fs::read_to_string(self.directory.join("audit.log")) else {
            return 0;
        };
        text.lines()
            .filter_map(|line| line.split('\t').next()?.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
    }

    /// The folder itself.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The match this cache belongs to.
    #[must_use]
    pub fn match_id(&self) -> &str {
        &self.match_id
    }

    /// One seat's private subtree, created if it is not there yet.
    ///
    /// The seat's number is the only thing that varies, and it is a `u8` from
    /// the sim rather than a string from a caller.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for anything the filesystem refuses.
    pub fn seat_directory(&self, seat: SeatId) -> Result<PathBuf, Error> {
        let directory = self.directory.join("seats").join(seat.raw().to_string());
        for path in [
            directory.clone(),
            directory.join("drafts"),
            directory.join("sealed"),
        ] {
            fs::create_dir_all(&path).map_err(|error| {
                Error::internal(format!("creating {}: {error}", path.display()))
            })?;
        }
        Ok(directory)
    }

    /// Append audit entries to `audit.log`, writing the header if the file is
    /// new.
    ///
    /// LF endings on every platform: the log is read beside a golden and
    /// compared byte for byte (`tests/golden/README.md` rule 3).
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] for anything the filesystem refuses.
    pub fn append_audit(&self, entries: &[Entry]) -> Result<(), Error> {
        if entries.is_empty() {
            return Ok(());
        }
        let path = self.directory.join("audit.log");
        let fresh = !path.exists();
        let mut text = String::new();
        if fresh {
            text.push_str("seq\ttick\tsubject\thandle\taction\toutcome\n");
        }
        for entry in entries {
            text.push_str(&entry.render());
            text.push('\n');
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| Error::internal(format!("opening {}: {error}", path.display())))?;
        file.write_all(text.as_bytes())
            .map_err(|error| Error::internal(format!("writing {}: {error}", path.display())))?;
        Ok(())
    }

    /// The audit log's text, for a host or a test that wants to read it back.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::NotFound`] when no log has been written yet.
    pub fn audit_text(&self) -> Result<String, Error> {
        let path = self.directory.join("audit.log");
        fs::read_to_string(&path)
            .map_err(|error| Error::not_found(format!("reading {}: {error}", path.display())))
    }

    /// Write one file inside the match folder. `name` is a fixed literal at
    /// every call site; nothing caller-supplied reaches it.
    fn write(&self, name: &Path, bytes: &[u8]) -> Result<(), Error> {
        let path = self.directory.join(name);
        fs::write(&path, bytes)
            .map_err(|error| Error::internal(format!("writing {}: {error}", path.display())))
    }
}

#[cfg(test)]
mod tests {
    use super::{APP_DIR, APP_DIR_LOWER, Header, MatchCache, Os, data_root};
    use crate::audit::{AuditLog, Outcome};
    use crate::error::Code;
    use crate::token::Subject;
    use pharmakos_sim::math::quantity::Tick;
    use pharmakos_sim::tables::SeatId;
    use std::path::PathBuf;

    /// A scratch directory of this test's own.
    ///
    /// `CARGO_TARGET_TMPDIR` is set for integration tests only, and these are
    /// unit tests inside the crate, so this is the system temporary directory
    /// with a name nothing else uses -- the same thing `xtask`'s own self-tests
    /// do.
    fn scratch(name: &str) -> PathBuf {
        let base = std::env::temp_dir()
            .join("pharmakos-gateway-tests")
            .join("cache")
            .join(name);
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("scratch");
        base
    }

    fn header(seed: u64) -> Header {
        Header {
            match_seed: seed,
            seats: 2,
            segment_lengths_ms: vec![1_000],
            round_limit: 2,
            rules_hash: 0x0123_4567_89ab_cdef,
        }
    }

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        move |name: &str| {
            owned
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn windows_uses_local_appdata() {
        let root = data_root(
            env(&[("LOCALAPPDATA", "C:/Users/x/AppData/Local")]),
            Os::Windows,
        )
        .expect("resolved");
        assert!(root.ends_with(APP_DIR), "{}", root.display());
        assert!(root.starts_with("C:/Users/x/AppData/Local"));
    }

    #[test]
    fn linux_prefers_xdg_data_home_and_falls_back_to_the_default_inside_home() {
        let root =
            data_root(env(&[("XDG_DATA_HOME", "/home/x/.data")]), Os::Linux).expect("resolved");
        assert_eq!(root, PathBuf::from("/home/x/.data").join(APP_DIR_LOWER));

        let root = data_root(env(&[("HOME", "/home/x")]), Os::Linux).expect("resolved");
        assert_eq!(
            root,
            PathBuf::from("/home/x")
                .join(".local")
                .join("share")
                .join(APP_DIR_LOWER)
        );
    }

    #[test]
    fn macos_uses_application_support() {
        let root = data_root(env(&[("HOME", "/Users/x")]), Os::Macos).expect("resolved");
        assert_eq!(
            root,
            PathBuf::from("/Users/x")
                .join("Library")
                .join("Application Support")
                .join(APP_DIR)
        );
    }

    /// Item 98: never a fallback beside the executable.
    #[test]
    fn a_missing_variable_is_an_error_and_never_a_fallback() {
        for os in [Os::Windows, Os::Linux, Os::Macos] {
            let error = data_root(env(&[]), os).expect_err("no variable");
            assert_eq!(error.code, Code::Internal);
            assert!(
                error.message.contains("beside its own executable"),
                "{}",
                error.message
            );
        }
    }

    #[test]
    fn a_match_folder_says_it_is_not_encrypted() {
        let root = scratch("readme");
        let cache = MatchCache::open(&root, "m-0001", &header(0xca5c_aded)).expect("opened");
        let readme = std::fs::read_to_string(cache.directory().join("README.txt")).expect("readme");
        assert!(readme.contains("NOT ENCRYPTED"), "{readme}");
        assert!(readme.contains("playtest bundle"), "{readme}");
        let header = std::fs::read_to_string(cache.directory().join("match.json")).expect("header");
        assert!(header.contains("\"encrypted\": false"), "{header}");
        assert!(header.contains("0x00000000ca5caded"), "{header}");
        assert!(
            header.contains("\"rules_hash\": \"0123456789abcdef\""),
            "the replay's third input: {header}"
        );
        assert!(header.contains("\"round_limit\": 2"), "{header}");
    }

    /// The wave-6 notes, H16: a save outlives its process, and a new match
    /// under the same id is refused rather than written over it.
    #[test]
    fn a_folder_with_a_save_is_reopened_and_never_opened_new() {
        let root = scratch("reopen");
        let error = MatchCache::reopen(&root, "m-0004").expect_err("nothing saved yet");
        assert_eq!(error.code, Code::InvalidArgument);

        let cache = MatchCache::open(&root, "m-0004", &header(4)).expect("opened");
        assert!(!cache.has_save());
        cache.write_save("first\n").expect("saved");
        cache.write_save("second\n").expect("saved again");
        assert_eq!(cache.read_save().expect("read"), "second\n", "latest wins");
        assert!(
            !cache.directory().join("save.json.partial").exists(),
            "the temporary file was renamed into place"
        );
        let match_json =
            std::fs::read_to_string(cache.directory().join("match.json")).expect("header");

        let error = MatchCache::open(&root, "m-0004", &header(4)).expect_err("saved");
        assert_eq!(error.code, Code::InvalidArgument);
        assert!(error.message.contains("resume"), "{}", error.message);

        let again = MatchCache::reopen(&root, "m-0004").expect("reopened");
        assert_eq!(again.read_save().expect("read"), "second\n");
        assert_eq!(
            std::fs::read_to_string(again.directory().join("match.json")).expect("header"),
            match_json,
            "a resume does not rewrite match.json"
        );
    }

    #[test]
    fn the_replay_inputs_land_in_the_reserved_layout() {
        let root = scratch("replay");
        let cache = MatchCache::open(&root, "m-0005", &header(5)).expect("opened");
        cache
            .write_sealed(SeatId::new(1), 2, "{}\n")
            .expect("a sealed file");
        cache
            .write_replay(2, "21\t00000000000000ab\n")
            .expect("a chain");
        assert_eq!(
            std::fs::read_to_string(
                cache
                    .directory()
                    .join("seats")
                    .join("1")
                    .join("sealed")
                    .join("2.jsonc")
            )
            .expect("sealed"),
            "{}\n"
        );
        assert!(
            cache
                .directory()
                .join("replay")
                .join("2.hashes.txt")
                .is_file()
        );
    }

    #[test]
    fn the_layout_is_visible_from_the_first_match() {
        let root = scratch("layout");
        let cache = MatchCache::open(&root, "m-0002", &header(1)).expect("opened");
        assert!(cache.directory().join("seats").is_dir());
        assert!(cache.directory().join("replay").is_dir());
        let seat = cache.seat_directory(SeatId::new(1)).expect("seat");
        assert!(seat.join("drafts").is_dir());
        assert!(seat.join("sealed").is_dir());
        assert!(seat.ends_with("1"), "{}", seat.display());
    }

    /// The one gate between a string and a path.
    #[test]
    fn a_match_id_that_could_reach_out_of_the_folder_is_refused() {
        let root = scratch("traversal");
        for id in [
            "..",
            "../escape",
            "a/b",
            "a\\b",
            "C:",
            ".hidden",
            "m 0001",
            "M-0001",
            "",
            "m-\u{00e9}",
        ] {
            assert!(!MatchCache::valid_match_id(id), "`{id}` should be refused");
            let error = MatchCache::open(&root, id, &header(0)).expect_err("refused");
            assert_eq!(error.code, Code::InvalidArgument, "`{id}`");
        }
        assert!(MatchCache::valid_match_id("m-0001"));
        assert!(!MatchCache::valid_match_id(&"m".repeat(65)));
    }

    #[test]
    fn the_audit_log_is_appended_with_its_header_once() {
        let root = scratch("audit");
        let cache = MatchCache::open(&root, "m-0003", &header(3)).expect("opened");
        let mut log = AuditLog::new();
        log.record(Tick::ZERO, None, None, "upgrade", Outcome::Ok);
        cache.append_audit(&log.take()).expect("appended");
        log.record(
            Tick::new(1),
            Some(Subject::Seat(SeatId::new(0))),
            None,
            "call get_status",
            Outcome::Ok,
        );
        cache.append_audit(&log.take()).expect("appended");

        let text = cache.audit_text().expect("read back");
        assert_eq!(cache.last_audit_seq(), 2, "the last line's sequence number");
        assert_eq!(
            text.matches("seq\ttick").count(),
            1,
            "the header is written once, not once per flush"
        );
        assert!(text.contains("1\t0\t-\t-\tupgrade\tok\n"), "{text}");
        assert!(
            text.contains("2\t1\tseat.0\t-\tcall get_status\tok\n"),
            "{text}"
        );
        assert!(!text.contains('\r'), "LF endings on every platform");
        cache
            .append_audit(&[])
            .expect("nothing to append is not an error");
    }
}

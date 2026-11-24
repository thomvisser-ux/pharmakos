// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The save container, and the private replay's inputs.
//!
//! Spec section 3: "the host can save during any Lull, from the frozen
//! segment-end snapshot, and resume from the lobby. Seat tokens are reissued.
//! Saves are stamped with the rules hash and verifier version, and won't load
//! on a mismatch. They contain every playbook, draft and notebook, so they
//! live in the private match cache. No saving mid-Push." Decisions-log item
//! 84 builds it at the skeleton; the wave-6 notes' decisions C8 to C11, adopted
//! by item 111, give it its shape. This module is the **format** and nothing
//! else: it reads and writes text, and it touches no file (`std::fs` is
//! [`crate::cache`]'s alone, `tests/confinement.rs`).
//!
//! # When a save is written (decision C8)
//!
//! Automatically, at the two Lull boundaries item 84 names, and never mid-Push:
//!
//! * **`sealed`**, at [`crate::surface::Surface::begin_push`], once every seal
//!   is final and the safe playbooks are filed, before the first tick. A
//!   `sealed` save resumes **straight into that Push** with those seals and no
//!   Lull, so a client killed mid-Push replays the same Push to the same chain
//!   and cannot re-plan what it watched (AGENTS.md section 1: orders you
//!   cannot take back).
//! * **`lull`**, when the control pipe reaches end of file during a Lull
//!   ([`crate::serve`]). It resumes into an ordinary Lull with the seats'
//!   notebooks, drafts and any verified submission, which the spec lets a seat
//!   replace until the timer ends.
//!
//! One file per match, `save.json`, replaced atomically (write, then rename).
//! PLACEHOLDER: one slot, latest wins — owner, at S6, with a save browser, where a `save_match` method stays additive.
//!
//! # What it holds (decision C10)
//!
//! * the **stamp**: [`SAVE_FORMAT_VERSION`], the rules hash, the verifier
//!   version and [`pharmakos_sim::snapshot::SNAPSHOT_VERSION`]. A mismatch in
//!   any of them is refused as [`crate::error::Code::InvalidArgument`].
//!   PLACEHOLDER: a save from an older build is refused, with no converter — owner, at hardening;
//! * the **config line's** seven values, the preset among them, which a
//!   resume line must repeat exactly (decisions-log item 112 (8));
//! * `lull_offset`, so the gateway's tick and the audit log's stamps stay
//!   monotonic across a resume;
//! * the **frozen planning snapshot**'s bytes, base64 (the sim's postcard
//!   inside the gateway's JSON: two formats in one file, said plainly);
//! * the **boundary** kind, `sealed` or `lull`;
//! * per seat, the **notebook**, the **drafts**, and the **seal**: canonical
//!   JSONC as submitted, its round, `filed_by_the_gateway`, its `report_hash`
//!   and its **plan fingerprint** (decisions-log items 77, 103 (5), 104 (4));
//! * per seat, from format version 3, the **handle table** -- every `e_NN`
//!   the seat holds and the beacon id it names, in the order they were
//!   minted (S3's plan, E1) -- and the **memos** issued to it (E5);
//! * from format version 4, the **last Push** (`last_push`,
//!   [`crate::surface::last_push`]): the segment feed a Lull holds, in a
//!   `lull` save, and the recap's record, in every save, so a resumed Lull
//!   reads the Push before it as the live one did. A version-3 save, without
//!   it, still loads.
//!
//! A third boundary, **`ended`**, is written when the match ends, in place of
//! the final round's `sealed` save, and is never resumed: [`Save::check`]
//! refuses it ([`refuse_ended`]).
//!
//! # Why the handle table is saved (S3's plan, E1)
//!
//! A seat's `e_NN` for another seat's beacon is the gateway's per-viewer name,
//! minted where a Lull opens and never in the sim
//! ([`crate::surface::Surface`]'s `name_sightings`). Rebuilt on a resume from
//! the sim's sighting table alone, the names came back in the table's order,
//! which is not the order they were minted in across several Lulls: a match
//! resumed at round 3's Lull named round 2's `e_01` `e_02`, and an Attack a
//! seat wrote against `e_01` would have hit another beacon
//! (`tests/handles.rs`). The table in the save reproduces the names exactly
//! across any number of resumes, and the Lull's own minting then names only
//! what is new. Putting the order in the sim instead would have made a
//! presentation detail hashed state and moved every chain.
//!
//! `SNAPSHOT_VERSION` does **not** move for any of this. The fingerprint lives
//! beside the snapshot, never inside it, so no verifier `report_hash` golden
//! moves (item 109 (7)).
//!
//! # What the fingerprint check catches, said plainly
//!
//! On a resume every seal is recompiled from its saved text and its
//! fingerprint recomputed and compared with the saved one. **Both come from
//! `save.json`**, so the check catches a damaged file and a canonical form that
//! changed between builds, and nothing more: the private match cache is plain
//! local state (AGENTS.md section 7), and a person who can edit the file can
//! edit both. It is not tamper-evidence and does not claim to be.
//!
//! # The private replay (decision C11)
//!
//! Inputs only, and no new sim format. At every `begin_push` each seat's seal
//! goes to `seats/<seat>/sealed/<round>.jsonc` ([`SealedFile`]); at every
//! segment end the segment's per-tick chain goes to
//! `replay/<round>.hashes.txt` ([`SegmentChain`]), in the text format of the
//! committed scenario hash chains -- `<tick>\t<16 lowercase hex digits>`, one
//! line per tick, the sim's own tick numbers, LF endings and a trailing
//! newline -- which makes that format a contract on disk as well as under
//! `tests/golden/`. The seed, the settings and the rules hash are in
//! `match.json`. Nothing reads the replay in v1 but a test; spec section 15's
//! third input, the event **log**, is not written.
//! PLACEHOLDER: the replay's log, reader, scrub and recap use — owner, at S7, with the recordings.

use pharmakos_proto::json::grammar::Said;
use pharmakos_proto::said;
use std::fmt::Write as _;

use pharmakos_proto::json::{self, Json};
use pharmakos_sim::math::quantity::Tick;
use pharmakos_sim::tables::SeatId;

use crate::error::Error;
use crate::probation::{Memo, Preset};
use crate::serve::Config;
use crate::surface::Draft;
use crate::surface::last_push::LastPush;

/// The format's name, the first thing in the file.
pub const SAVE_FORMAT: &str = "pharmakos.save";

/// The format's version, in the stamp. A save of another version is refused.
///
/// **2 is S1's targeting** (decisions-log item 127 (13);
/// `docs/design/targeting.md`, contract pull request 2): beacon names became
/// per seat, so a version-1 save's sealed playbooks, drafts and snapshot name
/// beacons by numbers that counted every seat's, and resumed under per-seat
/// names they would point a seat's orders at other beacons. Such a save is
/// refused by its version, with [`PRE_S1_SAVE`] saying why.
///
/// **3 is S3's `gwp`** (S3's plan, E1, E5 and E6): each seat's `e_NN` handle
/// table and the memos issued to it, and the config line's preset. A
/// version-2 save holds no handle table, so a resume would rename the beacons
/// its seats were shown (the module doc's "Why the handle table is saved");
/// it is refused by its version, naming both, with no converter (S2's saves
/// are development saves on the owner's disk only; S3's plan, risk R7).
///
/// **4 is S3's `opsave`** (decisions-log item 177 (5)): the last Push, what a
/// Lull holds of the Push before it that lives only in the running gateway
/// (`last_push`, [`crate::surface::last_push`]), required in a version-4
/// save. The field is **additive**: a version-3 save, which holds none, still
/// loads ([`LAST_PUSH_ABSENT`]) and resumes as it always did, into a Lull
/// whose feed shows no Push, which the operator reads as unread
/// (`PushUnread::Resumed`). A reader of a later version that adds a member
/// does the same: it reads the older version with the member absent and
/// says so in a typed state, never by a default it cannot tell from a value.
pub const SAVE_FORMAT_VERSION: u32 = 4;

/// The last format version written before the save carried the last Push:
/// still read, with [`SavedMatch::last_push`] `None`.
pub const LAST_PUSH_ABSENT: u32 = 3;

/// The last format version written before S1's per-seat beacon names.
pub const PRE_S1_SAVE: u32 = 1;

/// Which Lull boundary a save was made at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Boundary {
    /// At `begin_push`, with every seal final: resumes into that Push.
    Sealed,
    /// On control end of file during a Lull: resumes into that Lull.
    Lull,
    /// When the match ended (lane `opsave`, from `fixq-c`'s finding): the
    /// match as it ended, which **never resumes**. An ended match cannot be
    /// continued (the lobby forgets its line for that reason, decisions-log
    /// item 114), and without this boundary the last save on disk was the
    /// final round's `sealed` one, which replayed the final Push and served
    /// a match that had ended. [`Save::check`] refuses it, naming why.
    Ended,
}

impl Boundary {
    /// The spelling in the file.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Boundary::Sealed => "sealed",
            Boundary::Lull => "lull",
            Boundary::Ended => "ended",
        }
    }

    fn from_name(name: &str) -> Option<Boundary> {
        match name {
            "sealed" => Some(Boundary::Sealed),
            "lull" => Some(Boundary::Lull),
            "ended" => Some(Boundary::Ended),
            _ => None,
        }
    }
}

/// What a save was written by, and what it will only load into.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stamp {
    /// [`SAVE_FORMAT_VERSION`] when it was written.
    pub format_version: u32,
    /// The rules table's hash.
    pub rules_hash: u64,
    /// `pharmakos_verifier::VERIFIER_VERSION`.
    pub verifier_version: String,
    /// `pharmakos_sim::snapshot::SNAPSHOT_VERSION`.
    pub snapshot_version: u32,
}

impl Stamp {
    /// This build's stamp, for a match under a rules table of this hash.
    #[must_use]
    pub fn current(rules_hash: u64) -> Stamp {
        Stamp {
            format_version: SAVE_FORMAT_VERSION,
            rules_hash,
            verifier_version: pharmakos_verifier::VERIFIER_VERSION.to_owned(),
            snapshot_version: pharmakos_sim::snapshot::SNAPSHOT_VERSION,
        }
    }
}

/// One seat's seal, as a save carries it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SavedSeal {
    /// The playbook as it was sealed: JSONC, comments and all.
    pub playbook_jsonc: String,
    /// The round it was sealed for.
    pub round: u32,
    /// True when the gateway filed it (a safe playbook).
    pub filed_by_the_gateway: bool,
    /// `report_hash` of the report that accepted it; empty for the gateway's
    /// own fallback, which no report accepted.
    pub report_hash: Vec<u8>,
    /// The plan fingerprint of its canonical form (decisions-log item 77).
    pub plan_fingerprint: u64,
}

/// One seat's private store, as a save carries it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SavedSeat {
    /// Which seat.
    pub seat: u8,
    /// The private seat notebook.
    pub notebook: String,
    /// The seat's drafts, in the order they were saved.
    pub drafts: Vec<Draft>,
    /// The seat's latest verified submission, if it has one.
    pub seal: Option<SavedSeal>,
    /// Every `e_NN` handle the seat holds: the beacon id it names and the
    /// handle, in the order they were minted, so `e_01` first (S3's plan,
    /// E1).
    pub handles: Vec<(u32, String)>,
    /// The Ledger's memos issued to the seat, in the order they were issued
    /// (S3's plan, E5).
    pub memos: Vec<Memo>,
}

/// Everything the surface saves: the save less its stamp and config line,
/// which the host loop adds ([`Save`]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SavedMatch {
    /// Which boundary.
    pub boundary: Boundary,
    /// The round the match is in.
    pub round: u32,
    /// The ticks of host time no sim tick covered, carried so the gateway's
    /// tick stays monotonic across a resume. A `sealed` save's Push is played
    /// again from its first tick, so for that one the resume also lifts the
    /// offset to the audit log's last tick ([`Continuation::last_tick`]).
    pub lull_offset: u32,
    /// The frozen planning snapshot, as the sim encodes it.
    pub snapshot: Vec<u8>,
    /// Every seat of the match, ascending.
    pub seats: Vec<SavedSeat>,
    /// What the Lull holds of the Push before it ([`LastPush`]): always
    /// written; `None` only for a save of format version
    /// [`LAST_PUSH_ABSENT`], written before the save carried it.
    pub last_push: Option<LastPush>,
}

/// What a resume needs from the processes that ran this match before it, read
/// off the audit log by the host loop before the surface exists
/// ([`crate::cache::MatchCache::continuation`]).
///
/// Not part of the save: the save is written at a Lull boundary, and a
/// process that was killed mid-Push went on past it. The audit log is the one
/// file every process of the match appends to as it runs, so it is the one
/// that knows how far they got.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Continuation {
    /// The last audit line's sequence number: a value no earlier process of
    /// this match minted a view under, folded into the resumed view's id
    /// ([`crate::viewfeed::ViewFeed::resumed_after`]).
    pub generation: u64,
    /// The largest gateway tick the audit log holds. A resumed surface's tick
    /// starts at no less, so the gateway's tick and the log's stamps stay
    /// monotonic across a resume -- a `sealed` save's included, whose Push the
    /// dead process had already played part of.
    pub last_tick: u32,
}

/// One seat's seal, for `seats/<seat>/sealed/<round>.jsonc`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SealedFile {
    /// Which seat.
    pub seat: SeatId,
    /// Which round.
    pub round: u32,
    /// The playbook as sealed.
    pub playbook_jsonc: String,
}

/// One segment's per-tick chain, for `replay/<round>.hashes.txt`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SegmentChain {
    /// The round the segment was.
    pub round: u32,
    /// `(tick, state hash)`, one per tick the segment played.
    pub ticks: Vec<(Tick, u64)>,
}

impl SegmentChain {
    /// The chain as a hash file: `<tick>\t<16 lowercase hex digits>` per line,
    /// LF endings, a trailing newline.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = String::with_capacity(self.ticks.len().saturating_mul(24));
        for (tick, hash) in &self.ticks {
            text.push_str(&tick.raw().to_string());
            text.push('\t');
            text.push_str(&pharmakos_sim::hex(*hash));
            text.push('\n');
        }
        text
    }
}

/// What the host loop has to write into the private match cache, taken from
/// the surface after every job ([`crate::surface::Surface::take_persistence`]).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Persistence {
    /// A `sealed` save, when a Push has just begun.
    pub save: Option<SavedMatch>,
    /// Every seat's seal, when a Push has just begun.
    pub sealed: Vec<SealedFile>,
    /// Every segment that has ended since the last take.
    pub chains: Vec<SegmentChain>,
    /// A save the disk needed and the surface could not build: the match's
    /// end or a Push's beginning (`Surface::persist_end`,
    /// `Surface::persist_push`). The host loop stops serving on it and says
    /// it, rather than leave a save on disk that a resume would misread.
    pub unsaved: Option<Error>,
}

impl Persistence {
    /// True when there is nothing to write.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.save.is_none()
            && self.sealed.is_empty()
            && self.chains.is_empty()
            && self.unsaved.is_none()
    }
}

/// A whole save: the stamp, the config line and the match.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Save {
    /// What wrote it.
    pub stamp: Stamp,
    /// The config line the match was hosted with.
    pub config: Config,
    /// The match itself.
    pub state: SavedMatch,
}

impl Save {
    /// The file's text: canonical JSON, two-space indent, LF endings.
    #[must_use]
    pub fn render(&self) -> String {
        let stamp = object(vec![
            ("format_version", number(self.stamp.format_version)),
            ("rules_hash", hex64(self.stamp.rules_hash)),
            (
                "verifier_version",
                Json::String(self.stamp.verifier_version.clone()),
            ),
            ("snapshot_version", number(self.stamp.snapshot_version)),
        ]);
        let config = object(vec![
            ("match_id", Json::String(self.config.match_id.clone())),
            (
                "seed",
                Json::String(crate::view::seed_text(self.config.seed)),
            ),
            ("seats", number(self.config.seats)),
            (
                "human_seat",
                self.config.human_seat.map_or(Json::Null, number),
            ),
            (
                "segment_lengths_ms",
                Json::Array(
                    self.config
                        .segment_lengths_ms
                        .iter()
                        .map(|length| number(*length))
                        .collect(),
                ),
            ),
            ("round_limit", number(self.config.round_limit)),
            (
                "preset",
                Json::String(String::from(self.config.preset.name())),
            ),
        ]);
        let seats: Vec<Json> = self.state.seats.iter().map(seat_json).collect();
        let mut entries = vec![
            ("format", Json::String(String::from(SAVE_FORMAT))),
            ("stamp", stamp),
            ("config", config),
            (
                "boundary",
                Json::String(String::from(self.state.boundary.name())),
            ),
            ("round", number(self.state.round)),
            ("lull_offset", number(self.state.lull_offset)),
            (
                "snapshot",
                Json::String(json::base64::encode(&self.state.snapshot)),
            ),
            ("seats", Json::Array(seats)),
        ];
        // A version-3 save, read back, is written back as it was read: no
        // member. Every save this build makes holds one.
        if let Some(last_push) = &self.state.last_push {
            entries.push(("last_push", last_push.render()));
        }
        json::write(&object(entries))
    }

    /// Read a save's text.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for text that is not a save
    /// this build reads: not JSON, not this format, another format version, or
    /// a field missing or of the wrong shape. The format version is checked
    /// before anything else is read, so a save from a later build is refused
    /// by its version rather than by whichever field it renamed.
    pub fn parse(text: &str) -> Result<Save, Error> {
        let root = json::read(text)
            .map_err(|error| damaged(said!("it is not JSON ({})", error.said())))?;
        if string_at(&root, "format")? != SAVE_FORMAT {
            return Err(damaged("it does not say it is a Pharmakos save"));
        }
        let stamp_json = member(&root, "stamp")?;
        let format_version = u32_at(stamp_json, "format_version")?;
        if format_version <= PRE_S1_SAVE {
            return Err(Error::invalid(said!(
                "this save is format version {}, written before S1 made beacon \
                 names per seat: its playbooks and its snapshot name beacons by numbers that \
                 counted every seat's, so this build (format version {}) \
                 refuses it rather than resume orders that would reach other beacons; start a \
                 new match",
                format_version,
                SAVE_FORMAT_VERSION
            )));
        }
        if format_version != SAVE_FORMAT_VERSION && format_version != LAST_PUSH_ABSENT {
            return Err(Error::invalid(said!(
                "this save is format version {} and this build reads version \
                 {}; a save from another build is refused rather than \
                 converted: start a new match",
                format_version,
                SAVE_FORMAT_VERSION
            )));
        }
        let stamp = Stamp {
            format_version,
            rules_hash: hex64_at(stamp_json, "rules_hash")?,
            verifier_version: string_at(stamp_json, "verifier_version")?.to_owned(),
            snapshot_version: u32_at(stamp_json, "snapshot_version")?,
        };

        let config_json = member(&root, "config")?;
        let seed_text = string_at(config_json, "seed")?;
        let seed = seed_text
            .strip_prefix("0x")
            .and_then(|digits| u64::from_str_radix(digits, 16).ok())
            .ok_or_else(|| damaged("its seed is not a `0x` seed"))?;
        let human_seat = match member(config_json, "human_seat")? {
            Json::Null => None,
            other => Some(
                u8::try_from(integer(other, "human_seat")?)
                    .map_err(|_| damaged("its human seat is not a seat"))?,
            ),
        };
        let mut segment_lengths_ms: Vec<i32> = Vec::new();
        for length in array_at(config_json, "segment_lengths_ms")? {
            segment_lengths_ms.push(
                i32::try_from(integer(length, "segment_lengths_ms")?)
                    .map_err(|_| damaged("a segment length does not fit"))?,
            );
        }
        let config = Config {
            match_id: string_at(config_json, "match_id")?.to_owned(),
            seed,
            seats: u32_at(config_json, "seats")?,
            human_seat,
            segment_lengths_ms,
            round_limit: u32_at(config_json, "round_limit")?,
            preset: Preset::parse(string_at(config_json, "preset")?)
                .map_err(|error| damaged(error.said()))?,
        };

        let boundary = Boundary::from_name(string_at(&root, "boundary")?)
            .ok_or_else(|| damaged("its boundary is not `sealed`, `lull` or `ended`"))?;
        let snapshot = json::base64::decode(string_at(&root, "snapshot")?)
            .ok_or_else(|| damaged("its snapshot is not base64"))?;
        let mut seats: Vec<SavedSeat> = Vec::new();
        for seat in array_at(&root, "seats")? {
            seats.push(parse_seat(seat)?);
        }
        // Required from version 4, and absent before it: a version-4 save
        // without it, or an older one with it, is not one this build wrote.
        let last_push = match (format_version, root.get("last_push")) {
            (LAST_PUSH_ABSENT, None) => None,
            (SAVE_FORMAT_VERSION, Some(member)) => Some(LastPush::parse(member)?),
            (LAST_PUSH_ABSENT, Some(_)) => {
                return Err(damaged(said!(
                    "it is format version {} and holds a last Push, which that version never wrote",
                    LAST_PUSH_ABSENT
                )));
            }
            (_, _) => return Err(damaged("it has no `last_push`")),
        };
        Ok(Save {
            stamp,
            config,
            state: SavedMatch {
                boundary,
                round: u32_at(&root, "round")?,
                lull_offset: u32_at(&root, "lull_offset")?,
                snapshot,
                seats,
                last_push,
            },
        })
    }

    /// Refuse a save this host will not resume: another rules table,
    /// verifier or snapshot format, or a config line that is not the saved
    /// one.
    ///
    /// All seven of the config line's values must be equal, the round limit
    /// and the preset included (decisions-log item 112 (8)): nothing is
    /// silently defaulted,
    /// and a lobby that asked for a different match would get a different
    /// match than it asked for.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`], naming what differs.
    pub fn check(&self, config: &Config, rules_hash: u64) -> Result<(), Error> {
        self.check_stamp(rules_hash)?;
        self.check_config(config)?;
        refuse_ended(self.state.boundary)
    }

    /// The stamp's half of [`SavedMatch::check`]: the rules table, the
    /// verifier and the snapshot format this build runs.
    fn check_stamp(&self, rules_hash: u64) -> Result<(), Error> {
        let here = Stamp::current(rules_hash);
        if self.stamp.rules_hash != here.rules_hash {
            return Err(Error::invalid(said!(
                "this save was made under the rules table {} and this build runs {}: a save \
                 does not load under another rules table (spec section 3)",
                pharmakos_proto::json::grammar::quoted(&pharmakos_sim::hex(self.stamp.rules_hash)),
                pharmakos_proto::json::grammar::quoted(&pharmakos_sim::hex(here.rules_hash))
            )));
        }
        if self.stamp.verifier_version != here.verifier_version {
            return Err(Error::invalid(said!(
                "this save was verified by verifier {} and this build's is {}: a save does not \
                 load under another verifier (spec section 3)",
                pharmakos_proto::json::grammar::quoted(&self.stamp.verifier_version),
                pharmakos_proto::json::grammar::quoted(&here.verifier_version)
            )));
        }
        if self.stamp.snapshot_version != here.snapshot_version {
            return Err(Error::invalid(said!(
                "this save's snapshot is version {} and this build reads version {}",
                self.stamp.snapshot_version,
                here.snapshot_version
            )));
        }
        Ok(())
    }

    /// The config's half of [`SavedMatch::check`]: the resume line repeats
    /// the saved match's config, and the memos are where a Probation match
    /// puts them.
    fn check_config(&self, config: &Config) -> Result<(), Error> {
        let saved = &self.config;
        let differs = |field: &str, was: String, now: String| {
            Error::invalid(said!(
                "the resume line asks for {} {} and the saved match has {}: a resume \
                 line repeats the saved match's config exactly",
                pharmakos_proto::json::grammar::quoted(field),
                pharmakos_proto::json::grammar::quoted(&now),
                pharmakos_proto::json::grammar::quoted(&was)
            ))
        };
        if saved.match_id != config.match_id {
            return Err(differs(
                "match id",
                saved.match_id.clone(),
                config.match_id.clone(),
            ));
        }
        if saved.seed != config.seed {
            return Err(differs(
                "seed",
                crate::view::seed_text(saved.seed),
                crate::view::seed_text(config.seed),
            ));
        }
        if saved.seats != config.seats {
            return Err(differs(
                "seat count",
                saved.seats.to_string(),
                config.seats.to_string(),
            ));
        }
        if saved.human_seat != config.human_seat {
            let show =
                |seat: Option<u8>| seat.map_or_else(|| String::from("-"), |raw| raw.to_string());
            return Err(differs(
                "human seat",
                show(saved.human_seat),
                show(config.human_seat),
            ));
        }
        if saved.segment_lengths_ms != config.segment_lengths_ms {
            return Err(differs(
                "segment lengths",
                format!("{:?}", saved.segment_lengths_ms),
                format!("{:?}", config.segment_lengths_ms),
            ));
        }
        if saved.round_limit != config.round_limit {
            return Err(differs(
                "round limit",
                saved.round_limit.to_string(),
                config.round_limit.to_string(),
            ));
        }
        if saved.preset != config.preset {
            return Err(differs(
                "preset",
                saved.preset.name().to_owned(),
                config.preset.name().to_owned(),
            ));
        }
        // The Ledger's memos go to the person's seat of a Probation match and
        // to nobody else (S3's plan, E5): a save holding one anywhere else is
        // not one this gateway wrote.
        for seat in &self.state.seats {
            if seat.memos.is_empty() {
                continue;
            }
            if saved.preset != Preset::Probation {
                return Err(damaged(said!(
                    "seat {} holds the Ledger's memos in a `{}` match, which issues none",
                    seat.seat,
                    saved.preset.name()
                )));
            }
            if saved.human_seat != Some(seat.seat) {
                return Err(damaged(said!(
                    "seat {} holds the Ledger's memos and is not the person's seat",
                    seat.seat
                )));
            }
        }
        Ok(())
    }
}

fn seat_json(seat: &SavedSeat) -> Json {
    let drafts: Vec<Json> = seat
        .drafts
        .iter()
        .map(|draft| {
            object(vec![
                ("draft_id", Json::String(draft.draft_id.clone())),
                ("label", Json::String(draft.label.clone())),
                ("round", number(draft.round)),
                ("playbook_jsonc", Json::String(draft.playbook_jsonc.clone())),
            ])
        })
        .collect();
    let sealed = seat.seal.as_ref().map_or(Json::Null, |held| {
        object(vec![
            ("playbook_jsonc", Json::String(held.playbook_jsonc.clone())),
            ("round", number(held.round)),
            (
                "filed_by_the_gateway",
                Json::Bool(held.filed_by_the_gateway),
            ),
            ("report_hash", Json::String(hex_bytes(&held.report_hash))),
            ("plan_fingerprint", hex64(held.plan_fingerprint)),
        ])
    });
    let handles: Vec<Json> = seat
        .handles
        .iter()
        .map(|(beacon, handle)| {
            object(vec![
                ("beacon", number(*beacon)),
                ("handle", Json::String(handle.clone())),
            ])
        })
        .collect();
    let memos: Vec<Json> = seat
        .memos
        .iter()
        .map(|memo| Json::String(String::from(memo.name())))
        .collect();
    object(vec![
        ("seat", number(seat.seat)),
        ("notebook", Json::String(seat.notebook.clone())),
        ("drafts", Json::Array(drafts)),
        ("seal", sealed),
        ("handles", Json::Array(handles)),
        ("memos", Json::Array(memos)),
    ])
}

fn parse_seat(value: &Json) -> Result<SavedSeat, Error> {
    let seat = u8::try_from(integer(member(value, "seat")?, "seat")?)
        .map_err(|_| damaged("a seat number does not fit"))?;
    let listed = array_at(value, "drafts")?;
    // Its own and the carried one, at most: refused before a draft is read.
    if listed.len() > crate::surface::planning::MAX_HELD_DRAFTS {
        return Err(damaged(said!(
            "seat {} holds {} drafts, past the {} of its own and the carried one a seat may",
            seat,
            listed.len(),
            crate::surface::planning::MAX_DRAFTS
        )));
    }
    let mut drafts: Vec<Draft> = Vec::with_capacity(listed.len());
    for draft in listed {
        drafts.push(parse_draft(seat, draft)?);
    }
    crate::surface::planning::drafts_hold(&drafts)
        .map_err(|broken| damaged(said!("seat {}'s drafts: {}", seat, broken.said())))?;
    let sealed = match member(value, "seal")? {
        Json::Null => None,
        held => Some(SavedSeal {
            playbook_jsonc: string_at(held, "playbook_jsonc")?.to_owned(),
            round: u32_at(held, "round")?,
            filed_by_the_gateway: match member(held, "filed_by_the_gateway")? {
                Json::Bool(flag) => *flag,
                _ => return Err(damaged("`filed_by_the_gateway` is not true or false")),
            },
            report_hash: bytes_of_hex(string_at(held, "report_hash")?)
                .ok_or_else(|| damaged("a report hash is not hex"))?,
            plan_fingerprint: hex64_at(held, "plan_fingerprint")?,
        }),
    };
    let listed = array_at(value, "handles")?;
    if listed.len() > crate::viewfeed::MAX_FOREIGN_HANDLES {
        return Err(damaged(said!(
            "seat {}'s handle table holds {} handles, past the {} a seat may",
            seat,
            listed.len(),
            crate::viewfeed::MAX_FOREIGN_HANDLES
        )));
    }
    let mut handles: Vec<(u32, String)> = Vec::with_capacity(listed.len());
    let mut seen: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for held in listed {
        let beacon = u32_at(held, "beacon")?;
        let handle = string_at(held, "handle")?.to_owned();
        // Handles are minted `e_01` up, one a beacon: anything else is not a
        // table this gateway wrote.
        let number = u32::try_from(handles.len())
            .ok()
            .and_then(|held| held.checked_add(1))
            .ok_or_else(|| damaged("its handle table is longer than a u32 counts"))?;
        if handle != pharmakos_sim::tables::foreign_beacon_name(number) || !seen.insert(beacon) {
            return Err(damaged(said!(
                "seat {}'s handle table is not `e_01` up, one handle a beacon",
                seat
            )));
        }
        handles.push((beacon, handle));
    }
    let mut memos: Vec<Memo> = Vec::new();
    for memo in array_at(value, "memos")? {
        let Json::String(name) = memo else {
            return Err(damaged("a memo is not a string"));
        };
        let memo = Memo::parse(name).map_err(|error| damaged(error.said()))?;
        if memos.contains(&memo) {
            return Err(damaged(said!(
                "seat {}'s memo `{}` is issued twice, and a memo is issued once a match",
                seat,
                memo.name()
            )));
        }
        memos.push(memo);
    }
    Ok(SavedSeat {
        seat,
        notebook: string_at(value, "notebook")?.to_owned(),
        drafts,
        seal: sealed,
        handles,
        memos,
    })
}

/// One saved draft, held to the bounds `save_draft` holds a draft to when a
/// seat saves it (`crate::surface::planning`): its id in the draft id
/// grammar, its label at most [`crate::surface::planning::MAX_DRAFT_LABEL_CHARS`]
/// characters and its body at most [`pharmakos_verifier::MAX_PLAYBOOK_CHARS`],
/// so `get_draft` after a resume hands back no draft a seat could not have
/// saved.
fn parse_draft(seat: u8, draft: &Json) -> Result<Draft, Error> {
    let draft_id = string_at(draft, "draft_id")?;
    let label = string_at(draft, "label")?;
    let playbook_jsonc = string_at(draft, "playbook_jsonc")?;
    if !crate::surface::planning::is_draft_id(draft_id) {
        return Err(damaged(said!(
            "seat {} holds a draft whose id is not a draft id",
            seat
        )));
    }
    if label.chars().count() > crate::surface::planning::MAX_DRAFT_LABEL_CHARS {
        return Err(damaged(said!(
            "seat {}'s draft `{}` has a label past the {} characters a label may have",
            seat,
            pharmakos_proto::json::grammar::quoted(draft_id),
            crate::surface::planning::MAX_DRAFT_LABEL_CHARS
        )));
    }
    if pharmakos_verifier::is_too_long(playbook_jsonc) {
        return Err(damaged(said!(
            "seat {}'s draft `{}` is past the {} characters a playbook may have",
            seat,
            pharmakos_proto::json::grammar::quoted(draft_id),
            pharmakos_verifier::MAX_PLAYBOOK_CHARS
        )));
    }
    Ok(Draft {
        draft_id: draft_id.to_owned(),
        label: label.to_owned(),
        round: u32_at(draft, "round")?,
        playbook_jsonc: playbook_jsonc.to_owned(),
    })
}

/// The refusal of a save made when its match ended: an ended match is not
/// resumed, whichever door it is resumed by ([`Save::check`],
/// `Surface::resume`).
///
/// # Errors
///
/// [`crate::error::Code::InvalidArgument`] for [`Boundary::Ended`], in the
/// words the host says on its announce path (`gamectl host`'s refusal).
pub fn refuse_ended(boundary: Boundary) -> Result<(), Error> {
    match boundary {
        Boundary::Ended => Err(match_ended()),
        Boundary::Sealed | Boundary::Lull => Ok(()),
    }
}

/// The refusal of a resume of an ended match, in the words the host says on
/// its announce path ([`refuse_ended`]). A save of another boundary whose
/// world has already ended is refused by the one restore, `Host::resume`,
/// which lands a save in a Lull only.
#[must_use]
pub fn match_ended() -> Error {
    Error::invalid(
        "this match has ended, and an ended match cannot be continued: start a new match",
    )
}

/// The refusal for a file that is not a save this build reads.
fn damaged(why: impl Into<Said>) -> Error {
    Error::invalid(said!(
        "this is not a save this build can resume: {}",
        why.into()
    ))
}

fn object(entries: Vec<(&str, Json)>) -> Json {
    Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn number(value: impl Into<i64>) -> Json {
    Json::Number(value.into().to_string())
}

fn hex64(value: u64) -> Json {
    Json::String(pharmakos_sim::hex(value))
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn bytes_of_hex(text: &str) -> Option<Vec<u8>> {
    if text.len().checked_rem(2) != Some(0) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len());
    let mut index = 0_usize;
    while index < text.len() {
        let pair = text.get(index..index.saturating_add(2))?;
        out.push(u8::from_str_radix(pair, 16).ok()?);
        index = index.saturating_add(2);
    }
    Some(out)
}

fn member<'a>(value: &'a Json, key: &str) -> Result<&'a Json, Error> {
    value.get(key).ok_or_else(|| {
        damaged(said!(
            "it has no `{}`",
            pharmakos_proto::json::grammar::quoted(key)
        ))
    })
}

fn string_at<'a>(value: &'a Json, key: &str) -> Result<&'a str, Error> {
    match member(value, key)? {
        Json::String(text) => Ok(text.as_str()),
        _ => Err(damaged(said!(
            "`{}` is not a string",
            pharmakos_proto::json::grammar::quoted(key)
        ))),
    }
}

fn array_at<'a>(value: &'a Json, key: &str) -> Result<&'a [Json], Error> {
    match member(value, key)? {
        Json::Array(items) => Ok(items.as_slice()),
        _ => Err(damaged(said!(
            "`{}` is not an array",
            pharmakos_proto::json::grammar::quoted(key)
        ))),
    }
}

fn integer(value: &Json, key: &str) -> Result<i64, Error> {
    match value {
        Json::Number(lexeme) => lexeme.parse::<i64>().map_err(|_| {
            damaged(said!(
                "`{}` is not a whole number",
                pharmakos_proto::json::grammar::quoted(key)
            ))
        }),
        _ => Err(damaged(said!(
            "`{}` is not a number",
            pharmakos_proto::json::grammar::quoted(key)
        ))),
    }
}

fn u32_at(value: &Json, key: &str) -> Result<u32, Error> {
    u32::try_from(integer(member(value, key)?, key)?).map_err(|_| {
        damaged(said!(
            "`{}` does not fit",
            pharmakos_proto::json::grammar::quoted(key)
        ))
    })
}

fn hex64_at(value: &Json, key: &str) -> Result<u64, Error> {
    let text = string_at(value, key)?;
    if text.len() != 16 {
        return Err(damaged(said!(
            "`{}` is not 16 hex digits",
            pharmakos_proto::json::grammar::quoted(key)
        )));
    }
    u64::from_str_radix(text, 16).map_err(|_| {
        damaged(said!(
            "`{}` is not hex",
            pharmakos_proto::json::grammar::quoted(key)
        ))
    })
}

/// The most bytes one character of a save's text can take: the writer spells
/// a control character as a six-byte `\u` escape, and nothing it writes
/// takes more.
const BYTES_PER_CHAR: u64 = 6;

/// The most bytes one encoded voxel of the planning snapshot is taken to
/// take, base64 included.
///
/// PLACEHOLDER: a snapshot taken at 16 bytes a map voxel, the sim giving no bound of its encoding — owner, at hardening, with the sim's snapshot ceiling.
const SNAPSHOT_BYTES_PER_VOXEL: u64 = 16;

/// The fixed part of a save: its keys, stamp, config line and counts.
const FIXED_BYTES: u64 = 64 * 1024;

impl Save {
    /// The largest a save of the match `config` names can be, from the
    /// format's own bounds, so a resume refuses a file past it before it
    /// reads it (`crate::cache::MatchCache::read_save`):
    ///
    /// * each seat's notebook (`verifier.notebook_max_chars`), its drafts (at
    ///   most `MAX_DRAFTS` of its own and the carried one, each a playbook of
    ///   at most `MAX_PLAYBOOK_CHARS` with a label and an id), its seal, its
    ///   handle table (`MAX_FOREIGN_HANDLES`) and its memos;
    /// * the last Push's feed, which is what the live feed retains: every line
    ///   of every drain of the sim's bus, at most `EVENT_BUS_CAPACITY` a drain
    ///   (the bus drops and counts the rest), one drain a tick of the
    ///   segment and `DRAINS_BEYOND_TICKS` more, for the longest segment of
    ///   this match's **own** ladder over the rounds it plays
    ///   ([`longest_segment_ticks`]), each line at most `MAX_LINE_CHARS`, and
    ///   the recap's record beside it;
    /// * the planning snapshot, at [`SNAPSHOT_BYTES_PER_VOXEL`] a voxel of
    ///   the rules' map.
    ///
    /// Every character counts at [`BYTES_PER_CHAR`].
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for match settings a host
    /// refuses, and [`crate::error::Code::Internal`] when the rules table has
    /// no verifier block, or a term does not fit a `u64`.
    pub fn max_bytes(
        config: &Config,
        rules: &pharmakos_sim::rules::RulesTable,
    ) -> Result<u64, Error> {
        let past = || Error::internal("a save's largest size does not fit a count");
        let count = |value: usize| u64::try_from(value).map_err(|_| past());
        let mul = |a: u64, b: u64| a.checked_mul(b).ok_or_else(past);
        let add = |a: u64, b: u64| a.checked_add(b).ok_or_else(past);
        let notebook = rules
            .message()
            .verifier
            .as_ref()
            .map(|verifier| u64::from(verifier.notebook_max_chars))
            .ok_or_else(|| {
                Error::internal(
                    "this rules table has no verifier block, so the notebook has no size",
                )
            })?;
        let playbook = count(pharmakos_verifier::MAX_PLAYBOOK_CHARS)?;
        let draft = add(
            add(
                playbook,
                count(crate::surface::planning::MAX_DRAFT_LABEL_CHARS)?,
            )?,
            64,
        )?;
        let drafts = mul(count(crate::surface::planning::MAX_HELD_DRAFTS)?, draft)?;
        let handles = mul(count(crate::viewfeed::MAX_FOREIGN_HANDLES)?, 64)?;
        let seat_chars = add(add(add(notebook, drafts)?, playbook)?, add(handles, 1024)?)?;
        let seats = mul(u64::from(config.seats), mul(seat_chars, BYTES_PER_CHAR)?)?;

        let ticks = u64::from(longest_segment_ticks(config, rules)?);
        let lines = add(
            mul(
                add(ticks, crate::surface::last_push::DRAINS_BEYOND_TICKS)?,
                count(pharmakos_sim::EVENT_BUS_CAPACITY)?,
            )?,
            add(
                u64::from(config.seats),
                count(crate::probation::Memo::ALL.len())?,
            )?,
        )?;
        let line = add(
            mul(count(crate::feed::MAX_LINE_CHARS)?, BYTES_PER_CHAR)?,
            512,
        )?;
        // The feed's lines, and as many failed steps in the record beside it.
        let feed = mul(lines, add(line, 64)?)?;

        let [x, y, z] = rules.map_size_voxels();
        let mut voxels: u64 = 1;
        for side in [x, y, z] {
            let side = u64::try_from(side).map_err(|_| past())?;
            voxels = mul(voxels, side)?;
        }
        let snapshot = mul(voxels, SNAPSHOT_BYTES_PER_VOXEL)?;
        add(add(add(FIXED_BYTES, seats)?, feed)?, snapshot)
    }
}

/// The most ticks any Push of the match `config` names plays: the longest of
/// its rounds' lengths on its own ladder (the config line's, or the rules'
/// when the line gives none, as the match is hosted: `MatchSettings::validate`),
/// over the rounds its round limit lets it play, in whole ticks rounded up.
/// Rounds past the ladder's end take its last entry, so the rounds read are
/// at most one past the ladder.
///
/// # Errors
///
/// [`crate::error::Code::InvalidArgument`] for settings the sim refuses, as
/// a host refuses them.
fn longest_segment_ticks(
    config: &Config,
    rules: &pharmakos_sim::rules::RulesTable,
) -> Result<u32, Error> {
    let settings = pharmakos_sim::runner::MatchSettings {
        segment_lengths_ms: config.segment_lengths_ms.clone(),
        round_limit: config.round_limit,
    };
    // The lobby's refusals first, in the host's own words; then the sim's, of
    // the ladder the rules table would give.
    crate::host::Host::check_settings(&settings)?;
    let checked = settings.validate(rules).map_err(|_| {
        Error::invalid("the rules table's own segment ladder is not one a host plays")
    })?;
    let state = pharmakos_sim::runner::MatchState::new(&checked);
    let ladder = u32::try_from(state.segment_lengths_ms().len())
        .map_err(|_| Error::internal("a segment ladder longer than a count holds"))?;
    let past_the_ladder = ladder
        .checked_add(1)
        .ok_or_else(|| Error::internal("a segment ladder longer than a count holds"))?;
    let rounds = config.round_limit.min(past_the_ladder);
    (1..=rounds)
        .map(|round| state.length_for_round(round).to_ticks_ceil())
        .max()
        .ok_or_else(|| Error::internal("a match that plays no round has no segment"))
}

#[cfg(test)]
mod tests {
    use super::{Boundary, Save, SavedMatch, SavedSeal, SavedSeat, SegmentChain, Stamp};
    use crate::error::Code;
    use crate::probation::{Memo, Preset};
    use crate::serve::Config;
    use crate::surface::Draft;
    use pharmakos_sim::math::quantity::Tick;

    fn config() -> Config {
        Config {
            match_id: String::from("m-save"),
            seed: 0xca5c_aded,
            seats: 2,
            human_seat: Some(0),
            segment_lengths_ms: vec![1_000, 2_000],
            round_limit: 3,
            preset: Preset::Probation,
        }
    }

    fn save() -> Save {
        Save {
            stamp: Stamp::current(0x0123_4567_89ab_cdef),
            config: config(),
            state: SavedMatch {
                boundary: Boundary::Sealed,
                round: 2,
                lull_offset: 3_600,
                snapshot: vec![0, 1, 2, 250, 251, 252],
                seats: vec![
                    SavedSeat {
                        seat: 0,
                        notebook: String::from("line one\n\"quoted\"\ttabbed"),
                        drafts: vec![Draft {
                            draft_id: String::from("carried"),
                            label: String::from("carried from round 1"),
                            round: 2,
                            playbook_jsonc: String::from("// a comment\n{}\n"),
                        }],
                        seal: Some(SavedSeal {
                            playbook_jsonc: String::from("{}"),
                            round: 2,
                            filed_by_the_gateway: false,
                            report_hash: vec![0xde, 0xad, 0xbe, 0xef],
                            plan_fingerprint: 0xfeed_f00d,
                        }),
                        handles: vec![
                            (0x0001_0003, String::from("e_01")),
                            (1, String::from("e_02")),
                        ],
                        memos: vec![Memo::FirstDeath, Memo::PowerShortfall],
                    },
                    SavedSeat {
                        seat: 1,
                        notebook: String::new(),
                        drafts: Vec::new(),
                        seal: None,
                        handles: Vec::new(),
                        memos: Vec::new(),
                    },
                ],
                last_push: Some(last_push(r#"{"feed": null, "recap": {"none": true}}"#)),
            },
        }
    }

    /// A last Push read from its member's text.
    fn last_push(text: &str) -> crate::surface::last_push::LastPush {
        crate::surface::last_push::LastPush::parse(
            &pharmakos_proto::json::read(text).expect("JSON"),
        )
        .expect("a last Push")
    }

    #[test]
    fn a_save_reads_back_exactly_as_it_was_written() {
        let written = save();
        let text = written.render();
        assert!(!text.contains('\r'), "LF endings");
        assert!(text.ends_with('\n'));
        assert_eq!(Save::parse(&text).expect("its own file"), written);
    }

    /// S3's plan, E1 and risk R7: a version-2 save, which holds no handle
    /// table, is refused by its version, naming both versions, and says to
    /// start a new match; so is one from a later build.
    #[test]
    fn a_save_from_another_format_version_is_refused_naming_both_versions() {
        assert_eq!(super::SAVE_FORMAT_VERSION, 4);
        for other in [2_u32, 5] {
            let text = save().render().replace(
                "\"format_version\": 4",
                &format!("\"format_version\": {other}"),
            );
            let error = Save::parse(&text).expect_err("another version");
            assert_eq!(error.code(), Code::InvalidArgument);
            assert!(
                error.message().contains(&format!("format version {other}"))
                    && error.message().contains("reads version 4")
                    && error.message().contains("start a new match"),
                "{}",
                error.message()
            );
        }
    }

    /// The handle table is bounded like the notebook and the drafts: a table
    /// past `MAX_FOREIGN_HANDLES` is refused as damaged.
    #[test]
    fn a_handle_table_past_its_bound_is_refused() {
        let mut written = save();
        let most = crate::viewfeed::MAX_FOREIGN_HANDLES;
        let table = |count: usize| -> Vec<(u32, String)> {
            (0..count)
                .map(|index| {
                    let number = u32::try_from(index).expect("small").saturating_add(1);
                    (
                        number.saturating_add(100),
                        pharmakos_sim::tables::foreign_beacon_name(number),
                    )
                })
                .collect()
        };
        if let Some(seat) = written.state.seats.first_mut() {
            seat.handles = table(most);
        }
        assert_eq!(
            Save::parse(&written.render()).expect("at the bound").state,
            written.state
        );
        if let Some(seat) = written.state.seats.first_mut() {
            seat.handles = table(most.saturating_add(1));
        }
        let error = Save::parse(&written.render()).expect_err("past the bound");
        assert_eq!(error.code(), Code::InvalidArgument);
        assert!(error.message().contains("handles"), "{}", error.message());
    }

    /// The Ledger's memos are the person's seat's in a Probation match: a save
    /// holding one on another seat, or in a `custom` match, is refused.
    #[test]
    fn memos_off_the_persons_seat_or_in_a_custom_save_are_refused() {
        let hash = 0x0123_4567_89ab_cdef;
        let mut elsewhere = save();
        if let Some(seat) = elsewhere.state.seats.get_mut(1) {
            seat.memos = vec![Memo::DarkStake];
        }
        let error = elsewhere
            .check(&config(), hash)
            .expect_err("seat 1 is no one's");
        assert_eq!(error.code(), Code::InvalidArgument);
        assert!(error.message().contains("seat 1"), "{}", error.message());

        let mut custom = save();
        custom.config.preset = Preset::Custom;
        let asked = Config {
            preset: Preset::Custom,
            ..config()
        };
        let error = custom
            .check(&asked, hash)
            .expect_err("a custom match issues none");
        assert_eq!(error.code(), Code::InvalidArgument);
        assert!(error.message().contains("custom"), "{}", error.message());

        let mut quiet = save();
        quiet.config.preset = Preset::Custom;
        for seat in &mut quiet.state.seats {
            seat.memos.clear();
        }
        assert!(
            quiet.check(&asked, hash).is_ok(),
            "a custom save with no memos"
        );
    }

    /// The handle table and the memos read back exactly, and a table that is
    /// not `e_01` up, one handle a beacon, or a memo issued twice, is not a
    /// save this build wrote.
    #[test]
    fn a_damaged_handle_table_or_memo_list_is_refused() {
        let text = save().render();
        assert!(text.contains("\"handle\": \"e_02\""), "{text}");
        assert!(text.contains("\"first_death\""), "{text}");
        assert!(text.contains("\"preset\": \"probation\""), "{text}");
        for (what, damaged) in [
            ("a gap", text.replace("\"e_02\"", "\"e_03\"")),
            (
                "one beacon twice",
                text.replace("\"beacon\": 1,", "\"beacon\": 65539,"),
            ),
            (
                "a memo twice",
                text.replace("\"power_shortfall\"", "\"first_death\""),
            ),
            (
                "an unknown memo",
                text.replace("\"power_shortfall\"", "\"first_licences\""),
            ),
            (
                "an unknown preset",
                text.replace("\"probation\"", "\"casual\""),
            ),
        ] {
            assert_ne!(damaged, text, "{what}: the fixture holds what is damaged");
            let error = Save::parse(&damaged).expect_err(what);
            assert_eq!(error.code(), Code::InvalidArgument, "{what}");
        }
    }

    #[test]
    fn a_pre_s1_save_is_refused() {
        let text = save()
            .render()
            .replace("\"format_version\": 4", "\"format_version\": 1");
        let error = Save::parse(&text).expect_err("a pre-S1 save");
        assert_eq!(error.code(), Code::InvalidArgument);
        assert!(
            error.message().contains("format version 1")
                && error.message().contains("per seat")
                && error.message().contains("start a new match"),
            "{}",
            error.message()
        );
    }

    #[test]
    fn text_that_is_not_a_save_is_refused_as_an_argument() {
        for text in ["", "{}", "[]", "{\"format\": \"something else\"}"] {
            let error = Save::parse(text).expect_err("not a save");
            assert_eq!(error.code(), Code::InvalidArgument, "{text}");
        }
    }

    #[test]
    fn every_one_of_the_seven_config_values_must_match() {
        let saved = save();
        assert!(saved.check(&config(), 0x0123_4567_89ab_cdef).is_ok());
        let variants: [(&str, Config); 7] = [
            (
                "match id",
                Config {
                    match_id: String::from("m-other"),
                    ..config()
                },
            ),
            (
                "seed",
                Config {
                    seed: 1,
                    ..config()
                },
            ),
            (
                "seat count",
                Config {
                    seats: 3,
                    ..config()
                },
            ),
            (
                "human seat",
                Config {
                    human_seat: None,
                    ..config()
                },
            ),
            (
                "segment lengths",
                Config {
                    segment_lengths_ms: vec![1_000],
                    ..config()
                },
            ),
            (
                "round limit",
                Config {
                    round_limit: 4,
                    ..config()
                },
            ),
            (
                "preset",
                Config {
                    preset: Preset::Custom,
                    ..config()
                },
            ),
        ];
        for (field, asked) in variants {
            let error = saved.check(&asked, 0x0123_4567_89ab_cdef).expect_err(field);
            assert_eq!(error.code(), Code::InvalidArgument, "{field}");
            assert!(
                error.message().contains(field),
                "{field}: {}",
                error.message()
            );
        }
    }

    #[test]
    fn a_segment_chain_is_written_in_the_hash_file_format() {
        let chain = SegmentChain {
            round: 2,
            ticks: vec![(Tick::new(21), 0xab), (Tick::new(22), u64::MAX)],
        };
        assert_eq!(
            chain.render(),
            "21\t00000000000000ab\n22\tffffffffffffffff\n"
        );
    }

    /// The last Push is additive (lane `opsave`): a version-3 save, which
    /// holds none, loads with it absent and writes back as it was read; a
    /// version-4 save without one, or a version-3 save with one, is not a
    /// save this build wrote.
    #[test]
    fn a_version_3_save_loads_without_the_last_push_and_no_other_mix_does() {
        let mut old = save();
        old.stamp.format_version = super::LAST_PUSH_ABSENT;
        old.state.last_push = None;
        let text = old.render();
        assert!(!text.contains("last_push"), "{text}");
        let read = Save::parse(&text).expect("a version-3 save loads");
        assert_eq!(read, old);
        assert_eq!(read.state.last_push, None);

        let mut current = save();
        current.state.last_push = None;
        let error = Save::parse(&current.render()).expect_err("version 4 carries it");
        assert_eq!(error.code(), Code::InvalidArgument);
        assert!(error.message().contains("last_push"), "{}", error.message());

        let mut mixed = save();
        mixed.stamp.format_version = super::LAST_PUSH_ABSENT;
        let error = Save::parse(&mixed.render()).expect_err("version 3 never carried it");
        assert_eq!(error.code(), Code::InvalidArgument);
    }

    /// A saved draft is held to `save_draft`'s bounds at load: its id, its
    /// label and its body.
    #[test]
    fn a_saved_draft_past_save_drafts_bounds_is_refused() {
        let at = |mutate: &dyn Fn(&mut Draft)| {
            let mut written = save();
            if let Some(draft) = written
                .state
                .seats
                .first_mut()
                .and_then(|seat| seat.drafts.first_mut())
            {
                mutate(draft);
            }
            Save::parse(&written.render())
        };
        let most = pharmakos_verifier::MAX_PLAYBOOK_CHARS;
        assert!(at(&|draft| draft.playbook_jsonc = "x".repeat(most)).is_ok());
        assert!(at(&|draft| draft.playbook_jsonc = String::new()).is_ok());
        for (what, outcome) in [
            (
                "a body past the bound",
                at(&|draft| draft.playbook_jsonc = "x".repeat(most + 1)),
            ),
            (
                "a label past the bound",
                at(&|draft| {
                    draft.label = "l".repeat(crate::surface::planning::MAX_DRAFT_LABEL_CHARS + 1);
                }),
            ),
            (
                "an id off the grammar",
                at(&|draft| draft.draft_id = String::from("Not An Id")),
            ),
        ] {
            let error = outcome.expect_err(what);
            assert_eq!(error.code(), Code::InvalidArgument, "{what}");
        }
        let mut many = save();
        if let Some(seat) = many.state.seats.first_mut() {
            let one = seat.drafts.first().cloned().expect("a draft");
            seat.drafts = (0..=crate::surface::planning::MAX_DRAFTS)
                .map(|index| Draft {
                    draft_id: format!("d{index}"),
                    ..one.clone()
                })
                .collect();
        }
        assert_eq!(
            Save::parse(&many.render())
                .expect_err("past the count")
                .code(),
            Code::InvalidArgument
        );
    }

    /// The draft bodies a save holds, under damage: oversized, empty, a lone
    /// surrogate escape (text that is not UTF-8), a control byte, a body of
    /// another type. Seeded; only the typed error escapes.
    #[test]
    fn a_damaged_draft_body_is_refused_only_as_an_argument() {
        let text = save().render();
        let needle = "\"playbook_jsonc\": \"// a comment\\n{}\\n\"";
        assert!(text.contains(needle), "{text}");
        let most = pharmakos_verifier::MAX_PLAYBOOK_CHARS;
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let (mut taken, mut refused) = (0_u32, 0_u32);
        for case in 0..400_u32 {
            let body = match next() % 8 {
                0 => format!(
                    "\"{}\"",
                    "x".repeat(most + 1 + usize::try_from(next() % 64).expect("small"))
                ),
                1 => String::from("\"\""),
                2 => String::from("\"\\udc80\""),
                3 => String::from("\"\\ud800x\""),
                4 => String::from("\"a\u{0}b\""),
                5 => String::from("null"),
                6 => format!(
                    "\"{}\"",
                    "é".repeat(usize::try_from(next() % 4).expect("small") + most - 2)
                ),
                _ => String::from("12"),
            };
            let damaged = text.replace(needle, &format!("\"playbook_jsonc\": {body}"));
            match Save::parse(&damaged) {
                Ok(_) => taken = taken.saturating_add(1),
                Err(error) => {
                    assert_eq!(
                        error.code(),
                        Code::InvalidArgument,
                        "case {case}: {body:.40}"
                    );
                    refused = refused.saturating_add(1);
                }
            }
        }
        assert!(taken > 0 && refused > 0, "{taken} taken, {refused} refused");
    }
}

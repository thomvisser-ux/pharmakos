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
//! PLACEHOLDER: one slot, latest wins -- **OWNER**, at **S6** with a save
//! browser, where a `save_match` method stays additive.
//!
//! # What it holds (decision C10)
//!
//! * the **stamp**: [`SAVE_FORMAT_VERSION`], the rules hash, the verifier
//!   version and [`pharmakos_sim::snapshot::SNAPSHOT_VERSION`]. A mismatch in
//!   any of them is refused as [`crate::error::Code::InvalidArgument`].
//!   PLACEHOLDER: a save from an older build is refused, with no converter --
//!   **OWNER**, at hardening;
//! * the **config line's** six values, which a resume line must repeat
//!   exactly (decisions-log item 112 (8));
//! * `lull_offset`, so the gateway's tick and the audit log's stamps stay
//!   monotonic across a resume;
//! * the **frozen planning snapshot**'s bytes, base64 (the sim's postcard
//!   inside the gateway's JSON: two formats in one file, said plainly);
//! * the **boundary** kind, `sealed` or `lull`;
//! * per seat, the **notebook**, the **drafts**, and the **seal**: canonical
//!   JSONC as submitted, its round, `filed_by_the_gateway`, its `report_hash`
//!   and its **plan fingerprint** (decisions-log items 77, 103 (5), 104 (4)).
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
//! third input, the event **log**, is not written. PLACEHOLDER: the replay's
//! log, reader, scrub and recap use -- **OWNER**, at **S7** with the
//! recordings.

use std::fmt::Write as _;

use pharmakos_proto::json::{self, Json};
use pharmakos_sim::math::quantity::Tick;
use pharmakos_sim::tables::SeatId;

use crate::error::Error;
use crate::serve::Config;
use crate::surface::Draft;

/// The format's name, the first thing in the file.
pub const SAVE_FORMAT: &str = "pharmakos.save";

/// The format's version, in the stamp. A save of another version is refused.
pub const SAVE_FORMAT_VERSION: u32 = 1;

/// Which Lull boundary a save was made at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Boundary {
    /// At `begin_push`, with every seal final: resumes into that Push.
    Sealed,
    /// On control end of file during a Lull: resumes into that Lull.
    Lull,
}

impl Boundary {
    /// The spelling in the file.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Boundary::Sealed => "sealed",
            Boundary::Lull => "lull",
        }
    }

    fn from_name(name: &str) -> Option<Boundary> {
        match name {
            "sealed" => Some(Boundary::Sealed),
            "lull" => Some(Boundary::Lull),
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
    /// tick stays monotonic across a resume.
    pub lull_offset: u32,
    /// The frozen planning snapshot, as the sim encodes it.
    pub snapshot: Vec<u8>,
    /// Every seat of the match, ascending.
    pub seats: Vec<SavedSeat>,
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
}

impl Persistence {
    /// True when there is nothing to write.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.save.is_none() && self.sealed.is_empty() && self.chains.is_empty()
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
        ]);
        let seats: Vec<Json> = self.state.seats.iter().map(seat_json).collect();
        json::write(&object(vec![
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
        ]))
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
        let root =
            json::read(text).map_err(|error| damaged(&format!("it is not JSON ({error})")))?;
        if string_at(&root, "format")? != SAVE_FORMAT {
            return Err(damaged("it does not say it is a Pharmakos save"));
        }
        let stamp_json = member(&root, "stamp")?;
        let format_version = u32_at(stamp_json, "format_version")?;
        if format_version != SAVE_FORMAT_VERSION {
            return Err(Error::invalid(format!(
                "this save is format version {format_version} and this build reads version \
                 {SAVE_FORMAT_VERSION}; a save from another build is refused rather than \
                 converted"
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
        };

        let boundary = Boundary::from_name(string_at(&root, "boundary")?)
            .ok_or_else(|| damaged("its boundary is neither `sealed` nor `lull`"))?;
        let snapshot = json::base64::decode(string_at(&root, "snapshot")?)
            .ok_or_else(|| damaged("its snapshot is not base64"))?;
        let mut seats: Vec<SavedSeat> = Vec::new();
        for seat in array_at(&root, "seats")? {
            seats.push(parse_seat(seat)?);
        }
        Ok(Save {
            stamp,
            config,
            state: SavedMatch {
                boundary,
                round: u32_at(&root, "round")?,
                lull_offset: u32_at(&root, "lull_offset")?,
                snapshot,
                seats,
            },
        })
    }

    /// Refuse a save this host will not resume: another rules table,
    /// verifier or snapshot format, or a config line that is not the saved
    /// one.
    ///
    /// All six of the config line's values must be equal, the round limit
    /// included (decisions-log item 112 (8)): nothing is silently defaulted,
    /// and a lobby that asked for a different match would get a different
    /// match than it asked for.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`], naming what differs.
    pub fn check(&self, config: &Config, rules_hash: u64) -> Result<(), Error> {
        let here = Stamp::current(rules_hash);
        if self.stamp.rules_hash != here.rules_hash {
            return Err(Error::invalid(format!(
                "this save was made under the rules table {} and this build runs {}: a save \
                 does not load under another rules table (spec section 3)",
                pharmakos_sim::hex(self.stamp.rules_hash),
                pharmakos_sim::hex(here.rules_hash)
            )));
        }
        if self.stamp.verifier_version != here.verifier_version {
            return Err(Error::invalid(format!(
                "this save was verified by verifier {} and this build's is {}: a save does not \
                 load under another verifier (spec section 3)",
                self.stamp.verifier_version, here.verifier_version
            )));
        }
        if self.stamp.snapshot_version != here.snapshot_version {
            return Err(Error::invalid(format!(
                "this save's snapshot is version {} and this build reads version {}",
                self.stamp.snapshot_version, here.snapshot_version
            )));
        }
        let saved = &self.config;
        let differs = |field: &str, was: String, now: String| {
            Error::invalid(format!(
                "the resume line asks for {field} {now} and the saved match has {was}: a resume \
                 line repeats the saved match's config exactly"
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
    object(vec![
        ("seat", number(seat.seat)),
        ("notebook", Json::String(seat.notebook.clone())),
        ("drafts", Json::Array(drafts)),
        ("seal", sealed),
    ])
}

fn parse_seat(value: &Json) -> Result<SavedSeat, Error> {
    let seat = u8::try_from(integer(member(value, "seat")?, "seat")?)
        .map_err(|_| damaged("a seat number does not fit"))?;
    let mut drafts: Vec<Draft> = Vec::new();
    for draft in array_at(value, "drafts")? {
        drafts.push(Draft {
            draft_id: string_at(draft, "draft_id")?.to_owned(),
            label: string_at(draft, "label")?.to_owned(),
            round: u32_at(draft, "round")?,
            playbook_jsonc: string_at(draft, "playbook_jsonc")?.to_owned(),
        });
    }
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
    Ok(SavedSeat {
        seat,
        notebook: string_at(value, "notebook")?.to_owned(),
        drafts,
        seal: sealed,
    })
}

/// The refusal for a file that is not a save this build reads.
fn damaged(why: &str) -> Error {
    Error::invalid(format!("this is not a save this build can resume: {why}"))
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
    value
        .get(key)
        .ok_or_else(|| damaged(&format!("it has no `{key}`")))
}

fn string_at<'a>(value: &'a Json, key: &str) -> Result<&'a str, Error> {
    match member(value, key)? {
        Json::String(text) => Ok(text.as_str()),
        _ => Err(damaged(&format!("`{key}` is not a string"))),
    }
}

fn array_at<'a>(value: &'a Json, key: &str) -> Result<&'a [Json], Error> {
    match member(value, key)? {
        Json::Array(items) => Ok(items.as_slice()),
        _ => Err(damaged(&format!("`{key}` is not an array"))),
    }
}

fn integer(value: &Json, key: &str) -> Result<i64, Error> {
    match value {
        Json::Number(lexeme) => lexeme
            .parse::<i64>()
            .map_err(|_| damaged(&format!("`{key}` is not a whole number"))),
        _ => Err(damaged(&format!("`{key}` is not a number"))),
    }
}

fn u32_at(value: &Json, key: &str) -> Result<u32, Error> {
    u32::try_from(integer(member(value, key)?, key)?)
        .map_err(|_| damaged(&format!("`{key}` does not fit")))
}

fn hex64_at(value: &Json, key: &str) -> Result<u64, Error> {
    let text = string_at(value, key)?;
    if text.len() != 16 {
        return Err(damaged(&format!("`{key}` is not 16 hex digits")));
    }
    u64::from_str_radix(text, 16).map_err(|_| damaged(&format!("`{key}` is not hex")))
}

#[cfg(test)]
mod tests {
    use super::{Boundary, Save, SavedMatch, SavedSeal, SavedSeat, SegmentChain, Stamp};
    use crate::error::Code;
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
                    },
                    SavedSeat {
                        seat: 1,
                        notebook: String::new(),
                        drafts: Vec::new(),
                        seal: None,
                    },
                ],
            },
        }
    }

    #[test]
    fn a_save_reads_back_exactly_as_it_was_written() {
        let written = save();
        let text = written.render();
        assert!(!text.contains('\r'), "LF endings");
        assert!(text.ends_with('\n'));
        assert_eq!(Save::parse(&text).expect("its own file"), written);
    }

    #[test]
    fn a_save_from_another_format_version_is_refused_by_its_version() {
        let text = save()
            .render()
            .replace("\"format_version\": 1", "\"format_version\": 2");
        let error = Save::parse(&text).expect_err("another version");
        assert_eq!(error.code, Code::InvalidArgument);
        assert!(
            error.message.contains("format version 2"),
            "{}",
            error.message
        );
    }

    #[test]
    fn text_that_is_not_a_save_is_refused_as_an_argument() {
        for text in ["", "{}", "[]", "{\"format\": \"something else\"}"] {
            let error = Save::parse(text).expect_err("not a save");
            assert_eq!(error.code, Code::InvalidArgument, "{text}");
        }
    }

    #[test]
    fn every_one_of_the_six_config_values_must_match() {
        let saved = save();
        assert!(saved.check(&config(), 0x0123_4567_89ab_cdef).is_ok());
        let variants: [(&str, Config); 6] = [
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
        ];
        for (field, asked) in variants {
            let error = saved.check(&asked, 0x0123_4567_89ab_cdef).expect_err(field);
            assert_eq!(error.code, Code::InvalidArgument, "{field}");
            assert!(error.message.contains(field), "{field}: {}", error.message);
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
}

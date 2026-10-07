// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Time, as the gateway is allowed to know it.
//!
//! **The gateway is not a walled crate, so it reads no clock at all** (AGENTS.md
//! section 4.5, and decisions-log item 99's closing paragraph). There is no
//! `Instant::now` in this crate, no `SystemTime::now`, and no per-module escape
//! hatch. Time arrives from the host as [`MatchTime`] -- a tick, a phase, two
//! durations in game milliseconds and a round number -- and every part of the
//! gateway that needs to know "when" reads it from there:
//!
//! * the rate limiter counts calls per tick and per window of ticks
//!   ([`crate::limit`]);
//! * the audit log stamps a tick and a sequence number ([`crate::audit`]);
//! * a token's expiry is a tick comparison ([`crate::token`]);
//! * the segment feed's 60-second digests are cut on game milliseconds
//!   ([`crate::feed`]).
//!
//! The Lull's timer is a host and UI concern that the sim never reads, so it
//! lives on the walled side of the wall like every other clock; what reaches the
//! gateway is the host's *answer* -- how many game milliseconds are left -- and
//! not the clock it worked that out from.
//!
//! The one `std::time` type this crate does use is [`std::time::Duration`], on a
//! socket read timeout in [`crate::session`]. A socket option is not a clock
//! read: nothing in the gateway's behaviour depends on its value, and no result
//! changes if the operating system honours it a millisecond late.

use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::{Ms, Tick};

/// The `_status` footer's contents, which is also everything the gateway knows
/// about time.
///
/// `segment_length_ms` is the **coming** segment's length and comes from the
/// frozen snapshot rather than from a constant (spec section 10, "Segment end";
/// skeleton plan T10). The gateway carries the number the host put in the
/// snapshot and never derives one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatchTime {
    /// The tick the host is on.
    pub tick: Tick,
    /// Which phase the match is in.
    pub phase: Phase,
    /// Game milliseconds left in this phase.
    pub phase_remaining_ms: Ms,
    /// The coming segment's length, from the frozen snapshot.
    pub segment_length_ms: Ms,
    /// 1-based. The round the match is on.
    pub round: u32,
    /// True when the current phase has no countdown, so `phase_remaining_ms`
    /// means nothing and a client must not show it as a timer that ran out
    /// (`gp.api.v1.Status.untimed`; S1's plan, decision 12, the register's
    /// S1-11). [`crate::surface::Surface`] decides it in one place: the lobby,
    /// a recap and an ended match are untimed, a Push never is, and a Lull is
    /// untimed until the host reports a countdown for it.
    pub untimed: bool,
}

impl MatchTime {
    /// A match sitting in the lobby at tick zero: the state a gateway is in
    /// before a host has told it anything.
    #[must_use]
    pub const fn lobby() -> MatchTime {
        MatchTime {
            tick: Tick::ZERO,
            phase: Phase::Lobby,
            phase_remaining_ms: Ms::ZERO,
            segment_length_ms: Ms::ZERO,
            round: 0,
            untimed: true,
        }
    }

    /// True when planning methods may run.
    ///
    /// Spec section 12: "Planning is closed during play and the recap." So the
    /// Lull and nothing else -- not the lobby, where there is no snapshot to
    /// plan against, and not `ENDED`.
    #[must_use]
    pub fn planning_open(self) -> bool {
        self.phase == Phase::Lull
    }

    /// The `_status` footer every result carries (spec section 12, "Budgets").
    ///
    /// Written under the key `_status`, which is not a legal Protobuf identifier
    /// -- `gp.api.v1.Status` is the footer's *shape* and never a field of a
    /// response message, and `gateway.proto` says so in its transport note.
    ///
    /// `untimed` is written only when it is true, as proto3's JSON mapping
    /// leaves a `bool` at its default out: a client decoding the footer as a
    /// `gp.api.v1.Status` reads an absent `untimed` as false, which is a timer.
    /// So a timed footer -- every Lull with a countdown, every Push -- keeps
    /// the four fields it had before S1 byte for byte, and the client
    /// fixtures copied from the gateway's goldens with it (S1's plan, task
    /// `econ`).
    #[must_use]
    pub fn footer(self) -> Json {
        let mut entries = vec![
            (
                String::from("phase"),
                Json::String(phase_wire_name(self.phase)),
            ),
            (
                String::from("phase_remaining_ms"),
                Json::Number(self.phase_remaining_ms.raw().to_string()),
            ),
            (
                String::from("segment_length_ms"),
                Json::Number(self.segment_length_ms.raw().to_string()),
            ),
            (String::from("round"), Json::Number(self.round.to_string())),
        ];
        if self.untimed {
            entries.push((String::from("untimed"), Json::Bool(true)));
        }
        Json::Object(entries)
    }
}

/// How long each Lull is declared to be: `rules.match.first_lull_ms` for round
/// 1's and `rules.match.lull_ms` for every later one (spec section 3's ten and
/// five minutes; decisions-log item 127 (2), S1's plan, decision 10).
///
/// Read once, from the rules table the surface is built with, and **typed**:
/// a table with no `match` block or a Lull that is not a positive number of
/// game milliseconds is refused with a [`LullRowError`] rather than read as a
/// zero-length Lull. A Lull's length is what bounds the countdown a host may
/// report ([`crate::surface::Surface::set_host_clock`]) and what the Lull's
/// elapsed time is derived from when a host reports only the countdown, so a
/// zero read in its place would refuse every report or move the gateway's
/// clock by nothing. Both are tuning values, data and not constants of this
/// crate's (AGENTS.md section 12).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LullLengths {
    first: Ms,
    later: Ms,
}

/// Why a rules table declares no usable Lull length.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LullRowError {
    /// The table carries no `match` block at all.
    NoMatchBlock,
    /// A Lull row is zero or negative.
    NotPositive {
        /// The row, as the rules table spells it.
        row: &'static str,
        /// What it holds.
        value: i32,
    },
}

impl core::fmt::Display for LullRowError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LullRowError::NoMatchBlock => formatter.write_str(
                "the rules table carries no `match` block, so nothing says how long a Lull is",
            ),
            LullRowError::NotPositive { row, value } => write!(
                formatter,
                "the rules table's `match.{row}` is {value} ms, and a Lull lasts a positive \
                 number of game milliseconds"
            ),
        }
    }
}

impl std::error::Error for LullRowError {}

impl LullLengths {
    /// The two Lull rows of `rules`.
    ///
    /// # Errors
    ///
    /// [`LullRowError::NoMatchBlock`] for a table with no `match` block, and
    /// [`LullRowError::NotPositive`] for a row that is not a positive length.
    pub fn from_rules(
        rules: &pharmakos_sim::rules::RulesTable,
    ) -> Result<LullLengths, LullRowError> {
        let block = rules
            .message()
            .r#match
            .as_ref()
            .ok_or(LullRowError::NoMatchBlock)?;
        let positive = |row: &'static str, value: i32| {
            if value > 0 {
                Ok(Ms::new(value))
            } else {
                Err(LullRowError::NotPositive { row, value })
            }
        };
        Ok(LullLengths {
            first: positive("first_lull_ms", block.first_lull_ms)?,
            later: positive("lull_ms", block.lull_ms)?,
        })
    }

    /// The Lull that opens round `round`: the first Lull's length for round 1,
    /// and the later Lulls' for every round after it. Round 0 is the lobby's,
    /// before any Lull, and reads as the first, which is the next to open.
    #[must_use]
    pub const fn for_round(self, round: u32) -> Ms {
        if round <= 1 { self.first } else { self.later }
    }

    /// `rules.match.first_lull_ms`.
    #[must_use]
    pub const fn first(self) -> Ms {
        self.first
    }

    /// `rules.match.lull_ms`.
    #[must_use]
    pub const fn later(self) -> Ms {
        self.later
    }
}

/// An opaque mark on where the match is, for `wait_for{trigger:"phase_change"}`.
///
/// **Tied to the match, never to the segment**, and that is the whole reason it
/// is not a [`crate::feed::Cursor`]. A feed cursor is snapshot-tied and a new
/// segment makes every one of them stale — which is correct for a feed, and
/// exactly wrong here: the moment the phase changes from Lull to Push is the
/// moment a new segment opens, so a snapshot-tied mark would answer
/// `STALE_SNAPSHOT` at the one moment this trigger exists to report.
///
/// Sixteen lower-case hex digits: eight of the mark and eight of a check over
/// the mark and the match seed. Half the length of a feed cursor, so a client
/// that hands one where the other belongs is told rather than resumed at a
/// place nobody meant. The check is not a signature and does not claim to be —
/// the token authenticates a caller, and this says a mangled mark is mangled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PhaseMark {
    mark: u32,
    match_seed: u64,
}

impl PhaseMark {
    /// How many characters a rendered mark has.
    pub const CHARS: usize = 16;

    /// The mark for where a match is now.
    ///
    /// The phase and the round, and nothing else: those are the two things a
    /// `phase_change` trigger is about, and a mark carrying the tick would fire
    /// twenty times a second.
    #[must_use]
    pub fn of(match_seed: u64, time: MatchTime) -> PhaseMark {
        let phase = u32::try_from(i32::from(time.phase)).unwrap_or(0);
        let round = time.round & 0x00ff_ffff;
        PhaseMark {
            mark: (phase << 24) | round,
            match_seed,
        }
    }

    /// The opaque text a client receives and hands back.
    #[must_use]
    pub fn render(self) -> String {
        format!("{:08x}{:08x}", self.mark, self.check())
    }

    /// Read a mark a client handed back.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] when the text is not a mark this
    /// gateway wrote. There is no stale case: a mark is a match's, and a match
    /// is what the token is tied to.
    pub fn parse(text: &str, match_seed: u64) -> Result<PhaseMark, crate::error::Error> {
        let refuse =
            || crate::error::Error::invalid("that is not a phase mark this gateway issued");
        if text.len() != PhaseMark::CHARS
            || !text
                .bytes()
                .all(|digit| matches!(digit, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(refuse());
        }
        let mark = text
            .get(..8)
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .ok_or_else(refuse)?;
        let check = text
            .get(8..16)
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .ok_or_else(refuse)?;
        let parsed = PhaseMark { mark, match_seed };
        if parsed.check() != check {
            return Err(crate::error::Error::invalid("that phase mark is damaged"));
        }
        Ok(parsed)
    }

    /// The check value: the low 32 bits of the project's one hash function over
    /// the mark **and the match seed**, so a mark from another match is refused
    /// as damaged rather than read as this match's opening Lull.
    fn check(self) -> u32 {
        let mut bytes: Vec<u8> = Vec::with_capacity(12);
        bytes.extend_from_slice(&self.mark.to_le_bytes());
        bytes.extend_from_slice(&self.match_seed.to_le_bytes());
        let hash = pharmakos_sim::digest(&bytes);
        u32::try_from(hash & 0xffff_ffff).unwrap_or(0)
    }
}

/// A phase's spelling on the wire: `lull`, `push`, `recap`.
///
/// Lower case, by decisions-log item 80's rule for every enum-valued parameter
/// and result field the gateway writes.
#[must_use]
pub fn phase_wire_name(phase: Phase) -> String {
    pharmakos_proto::scope::wire_name("gp.api.v1.Status.Phase", phase.as_str_name())
}

#[cfg(test)]
mod tests {
    use super::{MatchTime, Phase, phase_wire_name};
    use pharmakos_proto::json::Json;
    use pharmakos_sim::math::quantity::{Ms, Tick};

    fn lull() -> MatchTime {
        MatchTime {
            tick: Tick::new(40),
            phase: Phase::Lull,
            phase_remaining_ms: Ms::new(174_000),
            segment_length_ms: Ms::new(180_000),
            round: 1,
            untimed: false,
        }
    }

    #[test]
    fn the_footer_is_the_fields_of_the_schema_in_order() {
        let footer = lull().footer();
        let Json::Object(entries) = &footer else {
            panic!("the footer is an object");
        };
        let keys: Vec<&str> = entries.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["phase", "phase_remaining_ms", "segment_length_ms", "round"],
            "a timed footer leaves `untimed` out, at proto3's default"
        );
        let lobby = MatchTime::lobby().footer();
        let Json::Object(entries) = &lobby else {
            panic!("the footer is an object");
        };
        assert_eq!(
            entries.last(),
            Some(&(String::from("untimed"), Json::Bool(true))),
            "the lobby has no countdown, and says so last, in the schema's order"
        );
        assert_eq!(
            footer.get("phase"),
            Some(&Json::String(String::from("lull")))
        );
        assert_eq!(
            footer.get("segment_length_ms"),
            Some(&Json::Number(String::from("180000")))
        );
    }

    #[test]
    fn the_first_lull_is_round_ones_and_every_later_lull_is_the_other_row() {
        use super::{LullLengths, LullRowError};
        let rules = pharmakos_sim::rules::RulesTable::load(std::path::Path::new(
            "../../rules/rules.v1.json",
        ))
        .expect("the committed rules table");
        let lulls = LullLengths::from_rules(&rules).expect("the shipped table has both rows");
        assert_eq!(lulls.first(), Ms::new(600_000));
        assert_eq!(lulls.later(), Ms::new(300_000));
        assert_eq!(lulls.for_round(0), lulls.first(), "the lobby's next Lull");
        assert_eq!(lulls.for_round(1), lulls.first());
        assert_eq!(lulls.for_round(2), lulls.later());
        assert_eq!(lulls.for_round(6), lulls.later());

        let mut json = std::fs::read_to_string("../../rules/rules.v1.json").expect("the table");
        json = json.replace("\"first_lull_ms\": 600000", "\"first_lull_ms\": 0");
        let zeroed = pharmakos_sim::rules::RulesTable::from_canonical_json(&json)
            .expect("a table with a zero first Lull still loads");
        assert_eq!(
            LullLengths::from_rules(&zeroed),
            Err(LullRowError::NotPositive {
                row: "first_lull_ms",
                value: 0
            }),
            "a zero is refused, never read as a Lull of no length"
        );
    }

    #[test]
    fn the_phase_names_are_lower_case_on_the_wire() {
        assert_eq!(phase_wire_name(Phase::Lull), "lull");
        assert_eq!(phase_wire_name(Phase::Push), "push");
        assert_eq!(phase_wire_name(Phase::Recap), "recap");
        assert_eq!(phase_wire_name(Phase::Ended), "ended");
        assert_eq!(phase_wire_name(Phase::Lobby), "lobby");
    }

    #[test]
    fn planning_is_open_in_the_lull_and_nowhere_else() {
        assert!(lull().planning_open());
        for phase in [Phase::Lobby, Phase::Push, Phase::Recap, Phase::Ended] {
            let mut time = lull();
            time.phase = phase;
            assert!(!time.planning_open(), "{phase:?}");
        }
    }
}

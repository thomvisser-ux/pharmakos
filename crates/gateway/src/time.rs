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
    #[must_use]
    pub fn footer(self) -> Json {
        Json::Object(vec![
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
        ])
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
        }
    }

    #[test]
    fn the_footer_is_the_four_fields_of_the_schema_in_order() {
        let footer = lull().footer();
        let Json::Object(entries) = &footer else {
            panic!("the footer is an object");
        };
        let keys: Vec<&str> = entries.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["phase", "phase_remaining_ms", "segment_length_ms", "round"]
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

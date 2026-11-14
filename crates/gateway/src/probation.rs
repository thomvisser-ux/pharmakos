// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The match's preset, and the Ledger's memos a Probation match issues.
//!
//! Spec section 3's Probation paragraph and S3's plan (decisions 4, 5 and 13;
//! E5, E6, E7 and E25): a newcomer's first match is a named preset, carried on
//! the config line ([`crate::serve::Config::preset`]) so that the gateway, and
//! never the client, decides what it changes. What it changes, all of it the
//! gateway's:
//!
//! * **Untimed Lulls** (E7). The rules table's `match.lull_ms` rows are left as
//!   they are; a Probation match's Lulls simply carry no countdown, so the
//!   `_status` footer answers `untimed`, and a host that reports one is refused
//!   ([`crate::surface::Surface::set_host_clock`]), never clamped or ignored.
//! * **The Ledger's memos** (E5): for the seat the person plays, the first
//!   power shortfall, the first beacon lost, the first death and the first
//!   licence each put one memo on that seat's feed, and on its recap, once a
//!   match ([`Memo`]).
//! * **The guided first pick** (E25): round 1's template list marks the
//!   template the advisor says to start with, with its reason.
//!
//! A `custom` match is every match that is not Probation: the scenarios, the
//! tests, the watch check, and S6's settings screen when it comes. It changes
//! nothing above.

use pharmakos_proto::json::Json;
use pharmakos_sim::events::{Event, EventKind};
use pharmakos_sim::tables::SeatId;

use crate::error::Error;

/// Which preset a match is played under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preset {
    /// The newcomer's first match: untimed Lulls, the memos and the guided
    /// first pick (spec section 3; S3's plan, decisions 4, 5 and 13).
    Probation,
    /// Any other match, which the preset changes nothing about.
    Custom,
}

impl Preset {
    /// The spelling on the config line and in a save.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Preset::Probation => "probation",
            Preset::Custom => "custom",
        }
    }

    /// The preset a config line or a save names.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for any other word, naming the
    /// two it may be.
    pub fn parse(text: &str) -> Result<Preset, Error> {
        match text {
            "probation" => Ok(Preset::Probation),
            "custom" => Ok(Preset::Custom),
            other => Err(Error::invalid(format!(
                "`{other}` is not a preset: a match is `probation` or `custom`"
            ))),
        }
    }

    /// True for a match whose Lulls carry no countdown.
    #[must_use]
    pub const fn untimed_lulls(self) -> bool {
        matches!(self, Preset::Probation)
    }
}

/// One of the Ledger's memos (`gp.api.v1.LedgerMemo.Id`).
///
/// The fourth, the first licence (`FIRST_LICENCE`), keys on the sim's
/// `licence_awarded`, which S3's `lic` added; `gwr` issues it (S3's plan, E5).
/// A save spells each by [`Memo::name`], so a variant is added at the end and
/// a name never changes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Memo {
    /// The seat's first brownout that sheds a load.
    PowerShortfall,
    /// One of the seat's beacons first destroyed. A powered-down beacon is the
    /// shortfall's, so the two never fire on one event.
    DarkStake,
    /// The seat's commander's first death.
    FirstDeath,
    /// The seat's first licence awarded.
    FirstLicence,
}

impl Memo {
    /// Every memo this build issues, in their wire order.
    pub const ALL: [Memo; 4] = [
        Memo::PowerShortfall,
        Memo::DarkStake,
        Memo::FirstDeath,
        Memo::FirstLicence,
    ];

    /// The wire's spelling: `gp.api.v1.LedgerMemo.Id` in lower case, as every
    /// enum this surface answers with is (decisions-log item 80), and the
    /// spelling a save keeps.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Memo::PowerShortfall => "power_shortfall",
            Memo::DarkStake => "dark_stake",
            Memo::FirstDeath => "first_death",
            Memo::FirstLicence => "first_licence",
        }
    }

    /// The memo a save or the wire names.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::InvalidArgument`] for a name this build does not
    /// issue.
    pub fn parse(text: &str) -> Result<Memo, Error> {
        Memo::ALL
            .into_iter()
            .find(|memo| memo.name() == text)
            .ok_or_else(|| Error::invalid(format!("`{text}` is not a memo this build issues")))
    }

    /// The memo's text, from the one string table; the first licence's
    /// reads the rules table's vent count.
    ///
    /// # Errors
    ///
    /// [`crate::error::Code::Internal`] when the string table has no name
    /// for the Resonance Spire's licence, which a test pins never happens.
    pub fn text(self, rules: &pharmakos_sim::rules::RulesTable) -> Result<String, Error> {
        Ok(match self {
            Memo::PowerShortfall => String::from(crate::strings::MEMO_POWER_SHORTFALL),
            Memo::DarkStake => String::from(crate::strings::MEMO_DARK_STAKE),
            Memo::FirstDeath => String::from(crate::strings::MEMO_FIRST_DEATH),
            Memo::FirstLicence => {
                let title =
                    crate::strings::licence_title(pharmakos_proto::gp::v1::Licence::ResonanceSpire)
                        .ok_or_else(|| {
                            Error::internal(
                                "the string table has no name for the Resonance Spire's licence",
                            )
                        })?;
                crate::strings::memo_first_licence(rules.resonance_vents(), title)
            }
        })
    }

    /// The memo as the wire carries it, `gp.api.v1.LedgerMemo`: which memo,
    /// and its text ([`Memo::text`]).
    #[must_use]
    pub fn json(self, text: &str) -> Json {
        Json::Object(vec![
            (String::from("id"), Json::String(self.name().to_owned())),
            (String::from("text"), Json::String(text.to_owned())),
        ])
    }

    /// The memo a sim event is the trigger of for `seat`, or `None`.
    ///
    /// Decision 4's readings: a beacon going dark (`beacon_browned_out`, the
    /// brownout order shedding it) is the shortfall; a beacon reaching zero hit
    /// points (`beacon_destroyed`) is the dark Stake, a different event, so the
    /// two never fire on one; the commander's death (`commander_died`) is the
    /// first death; and a licence the Ledger awards (`licence_awarded`) is the
    /// first licence. Each is the seat's own event, by the event's `seat`.
    #[must_use]
    pub fn triggered_by(event: &Event, seat: SeatId) -> Option<Memo> {
        if event.seat != Some(seat) {
            return None;
        }
        match event.kind {
            EventKind::BeaconBrownedOut => Some(Memo::PowerShortfall),
            EventKind::BeaconDestroyed => Some(Memo::DarkStake),
            EventKind::CommanderDied => Some(Memo::FirstDeath),
            EventKind::LicenceAwarded => Some(Memo::FirstLicence),
            _ => None,
        }
    }
}

/// The feed kind a memo travels as (`gp.api.v1.Event.kind`).
pub const LEDGER_MEMO_KIND: &str = "ledger_memo";

#[cfg(test)]
mod tests {
    use super::{Memo, Preset};
    use crate::error::Code;

    #[test]
    fn a_preset_reads_back_as_it_is_spelt_and_nothing_else_is_one() {
        for preset in [Preset::Probation, Preset::Custom] {
            assert_eq!(Preset::parse(preset.name()).expect("its own name"), preset);
        }
        for other in ["", "Probation", "casual", "resume", "-"] {
            let error = Preset::parse(other).expect_err("not a preset");
            assert_eq!(error.code, Code::InvalidArgument, "{other}");
            assert!(error.message.contains("probation"), "{}", error.message);
        }
        assert!(Preset::Probation.untimed_lulls());
        assert!(!Preset::Custom.untimed_lulls());
    }

    #[test]
    fn a_memo_reads_back_as_it_is_spelt() {
        for memo in Memo::ALL {
            assert_eq!(Memo::parse(memo.name()).expect("its own name"), memo);
            let rules = pharmakos_sim::rules::RulesTable::load(
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules/rules.v1.json"),
            )
            .expect("the shipped rules table");
            assert!(!memo.text(&rules).expect("its text").is_empty());
        }
        assert_eq!(
            Memo::parse("first_licence").expect("gwr's"),
            Memo::FirstLicence
        );
        for other in ["", "FIRST_LICENCE", "first_licences", "licence"] {
            assert_eq!(
                Memo::parse(other).expect_err("not a memo").code,
                Code::InvalidArgument,
                "{other}"
            );
        }
    }
}

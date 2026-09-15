// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The diagnostic catalogue: one data table of every code the verifier can
//! carry, what family it belongs to, how severe it is, and which stage emits it.
//!
//! # Code numbers are append-only from the day they ship
//!
//! A code is an identity a client branches on, a saved report records and a
//! piece of documentation quotes. **Renaming or renumbering one is a breaking
//! change to all three**, so it is not done: a code is added at the end of its
//! family and an obsolete one keeps its number for ever. The whole table is
//! committed as `tests/golden/verifier/expected.catalogue.json`, which is what
//! makes that promise checkable rather than remembered — a diff there is the
//! review (decisions-log item 93).
//!
//! # A complete table, an honest coverage
//!
//! Item 93 settles what the walking skeleton implements: **every code the v1
//! vocabulary can actually produce in the QUICK stages**, with the rest entered
//! as rows that have no emitter yet. So the table is complete and the coverage
//! is recorded in the same file. FULL's families — `E06xx`, `W06xx`, `W07xx`
//! and the information codes — are entered and unemitted, consistent with item
//! 82's empty estimate and lint stages, and S1 and S3 fill them in without
//! renumbering anything.
//!
//! [`Emitter::NoneYet`] carries the task or stage that fills the row, so
//! "unemitted" is never a mystery: the catalogue says who owes it.
//!
//! # Families
//!
//! Spec section 11 names them, and their first digits are the family:
//!
//! | Prefix | Family |
//! |---|---|
//! | `E000x` | decode and version |
//! | `E01xx` | limits and required blocks |
//! | `E02xx` | conditions |
//! | `E03xx` | control flow |
//! | `E04xx` | references and placement |
//! | `E05xx` | settings and recycle |
//! | `E06xx`, `W06xx` | economy, power and messaging |
//! | `W07xx` | schedule, conflicts and staleness |
//! | `I...` | information |
//!
//! ## The letter is part of the identity, not a copy of the severity
//!
//! Read the prefix as spelling, not as data. It follows the spec's own usage —
//! the spec writes the families it introduces as warnings with a `W` and the
//! rest with an `E` — and that usage is a good guide but not a law: `E0008` (a
//! stale fingerprint) and `E0404` (a filter whose tags can never match an enemy
//! beacon) are both **warnings**, because the proto says so in as many words,
//! and both keep the `E` they were numbered with.
//!
//! A client branches on [`Entry::severity`], which `gp.api.v1.Diagnostic`
//! carries on every diagnostic, never on the first letter of the code. The
//! letter cannot be corrected later — codes are append-only from the day they
//! ship, and that includes their spelling — so this is written down here rather
//! than left for the next reader to infer.

use pharmakos_proto::gp::api::v1::diagnostic::Severity;
use pharmakos_proto::json::Json;

use crate::strings;

/// One stage of the pipeline (spec section 11).
///
/// `decode → structure → resolve → semantics` is QUICK; `estimate → lint`
/// completes FULL.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Stage {
    /// JSON text to a `gp.v1.Playbook`, strictly, at a supported version.
    Decode,
    /// Shape and limits: required blocks, names, durations, the size budget.
    Structure,
    /// Names to the things they name: labels, handler ids, beacon ids.
    Resolve,
    /// The rules that need meaning: jumps forward, placement, settings.
    Semantics,
    /// Travel, interface time, `$` and `kW` projection. Empty at the skeleton.
    Estimate,
    /// Schedule, conflicts and staleness. Empty at the skeleton.
    Lint,
}

impl Stage {
    /// The stage's name, as the catalogue golden spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Stage::Decode => "decode",
            Stage::Structure => "structure",
            Stage::Resolve => "resolve",
            Stage::Semantics => "semantics",
            Stage::Estimate => "estimate",
            Stage::Lint => "lint",
        }
    }

    /// Whether QUICK runs this stage. FULL runs all six.
    #[must_use]
    pub const fn in_quick(self) -> bool {
        matches!(
            self,
            Stage::Decode | Stage::Structure | Stage::Resolve | Stage::Semantics
        )
    }

    /// Every stage, in pipeline order.
    pub const ALL: [Stage; 6] = [
        Stage::Decode,
        Stage::Structure,
        Stage::Resolve,
        Stage::Semantics,
        Stage::Estimate,
        Stage::Lint,
    ];
}

/// Which stage raises a code, or which task will.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Emitter {
    /// The stage that raises it today.
    Stage(Stage),
    /// Nothing raises it at the walking skeleton. The string names the task or
    /// the stage that owes it, and it is printed into the catalogue golden.
    NoneYet(&'static str),
}

impl Emitter {
    /// The emitter as the catalogue golden spells it.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Emitter::Stage(stage) => stage.name().to_owned(),
            Emitter::NoneYet(owed) => format!("none at the skeleton ({owed})"),
        }
    }
}

/// The families of spec section 11.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Family {
    /// `E000x`.
    DecodeAndVersion,
    /// `E01xx`.
    LimitsAndRequiredBlocks,
    /// `E02xx`.
    Conditions,
    /// `E03xx`.
    ControlFlow,
    /// `E04xx`.
    ReferencesAndPlacement,
    /// `E05xx`.
    SettingsAndRecycle,
    /// `E06xx` and `W06xx`.
    EconomyPowerAndMessaging,
    /// `W07xx`.
    ScheduleConflictsAndStaleness,
    /// `I...`.
    Information,
}

impl Family {
    /// The family's name, as the catalogue golden spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Family::DecodeAndVersion => "decode and version",
            Family::LimitsAndRequiredBlocks => "limits and required blocks",
            Family::Conditions => "conditions",
            Family::ControlFlow => "control flow",
            Family::ReferencesAndPlacement => "references and placement",
            Family::SettingsAndRecycle => "settings and recycle",
            Family::EconomyPowerAndMessaging => "economy, power and messaging",
            Family::ScheduleConflictsAndStaleness => "schedule, conflicts and staleness",
            Family::Information => "information",
        }
    }
}

/// One row of the catalogue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    /// The code. Language-neutral, append-only, never reused.
    pub code: &'static str,
    /// Which family of spec section 11 it belongs to.
    pub family: Family,
    /// How severe it is. Only an `ERROR` stops a playbook qualifying.
    pub severity: Severity,
    /// Which stage raises it, or which task owes it.
    pub emitter: Emitter,
}

impl Entry {
    /// The message template, from the one string table.
    ///
    /// Empty only if the string table has lost the row, which
    /// `the_two_tables_name_the_same_codes` fails on.
    #[must_use]
    pub fn message(&self) -> &'static str {
        strings::text(self.code).map_or("", |row| row.message)
    }

    /// The plain-language sentence, from the one string table.
    #[must_use]
    pub fn beginner(&self) -> &'static str {
        strings::text(self.code).map_or("", |row| row.beginner)
    }
}

/// The severity name the catalogue golden spells.
#[must_use]
pub const fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Unspecified => "SEVERITY_UNSPECIFIED",
        Severity::Error => "ERROR",
        Severity::Warning => "WARNING",
        Severity::Info => "INFO",
    }
}

/// Every diagnostic code, in code order.
///
/// Read this table as the contract it is: the order is the golden's order, the
/// numbers never move, and a row with [`Emitter::NoneYet`] is a number held
/// deliberately rather than a gap (item 93).
pub const CATALOGUE: &[Entry] = &[
    // --- E000x: decode and version ------------------------------------------
    Entry {
        code: "E0001",
        family: Family::DecodeAndVersion,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Decode),
    },
    Entry {
        code: "E0002",
        family: Family::DecodeAndVersion,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Decode),
    },
    Entry {
        code: "E0003",
        family: Family::DecodeAndVersion,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Decode),
    },
    Entry {
        code: "E0004",
        family: Family::DecodeAndVersion,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Decode),
    },
    Entry {
        code: "E0005",
        family: Family::DecodeAndVersion,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Decode),
    },
    Entry {
        code: "E0006",
        family: Family::DecodeAndVersion,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Decode),
    },
    Entry {
        code: "E0007",
        family: Family::DecodeAndVersion,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Decode),
    },
    Entry {
        code: "E0008",
        family: Family::DecodeAndVersion,
        severity: Severity::Warning,
        emitter: Emitter::Stage(Stage::Decode),
    },
    // --- E01xx: limits and required blocks -----------------------------------
    Entry {
        code: "E0101",
        family: Family::LimitsAndRequiredBlocks,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0102",
        family: Family::LimitsAndRequiredBlocks,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0103",
        family: Family::LimitsAndRequiredBlocks,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0104",
        family: Family::LimitsAndRequiredBlocks,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0105",
        family: Family::LimitsAndRequiredBlocks,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0106",
        family: Family::LimitsAndRequiredBlocks,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0107",
        family: Family::LimitsAndRequiredBlocks,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0108",
        family: Family::LimitsAndRequiredBlocks,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0109",
        family: Family::LimitsAndRequiredBlocks,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0110",
        family: Family::LimitsAndRequiredBlocks,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0111",
        family: Family::LimitsAndRequiredBlocks,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0112",
        family: Family::LimitsAndRequiredBlocks,
        severity: Severity::Error,
        // The seat notebook is gateway state, not part of the playbook file, so
        // the limit is real but nothing here has the text to measure.
        emitter: Emitter::NoneYet("T9, the gateway owns the seat notebook"),
    },
    // --- E02xx: conditions ---------------------------------------------------
    Entry {
        code: "E0201",
        family: Family::Conditions,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0202",
        family: Family::Conditions,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0203",
        family: Family::Conditions,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0204",
        family: Family::Conditions,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0205",
        family: Family::Conditions,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0206",
        family: Family::Conditions,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Structure),
    },
    Entry {
        code: "E0207",
        family: Family::Conditions,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Resolve),
    },
    Entry {
        code: "E0208",
        family: Family::Conditions,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Resolve),
    },
    Entry {
        code: "E0209",
        family: Family::Conditions,
        severity: Severity::Error,
        // The enemy-knowledge predicate block (60-69) is reserved in the schema;
        // nothing in the v1 vocabulary can carry a Freshness yet.
        emitter: Emitter::NoneYet("S2/S3, with the enemy-knowledge predicates"),
    },
    // --- E03xx: control flow -------------------------------------------------
    Entry {
        code: "E0301",
        family: Family::ControlFlow,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Semantics),
    },
    Entry {
        code: "E0302",
        family: Family::ControlFlow,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Semantics),
    },
    Entry {
        code: "E0303",
        family: Family::ControlFlow,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Semantics),
    },
    Entry {
        code: "E0304",
        family: Family::ControlFlow,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Semantics),
    },
    Entry {
        code: "E0305",
        family: Family::ControlFlow,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Resolve),
    },
    // --- E04xx: references and placement -------------------------------------
    Entry {
        code: "E0401",
        family: Family::ReferencesAndPlacement,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Resolve),
    },
    Entry {
        code: "E0402",
        family: Family::ReferencesAndPlacement,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Semantics),
    },
    Entry {
        code: "E0403",
        family: Family::ReferencesAndPlacement,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Semantics),
    },
    Entry {
        code: "E0404",
        family: Family::ReferencesAndPlacement,
        severity: Severity::Warning,
        emitter: Emitter::Stage(Stage::Semantics),
    },
    Entry {
        code: "E0405",
        family: Family::ReferencesAndPlacement,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Semantics),
    },
    Entry {
        code: "E0406",
        family: Family::ReferencesAndPlacement,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Semantics),
    },
    // --- E05xx: settings and recycle -----------------------------------------
    Entry {
        code: "E0501",
        family: Family::SettingsAndRecycle,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Semantics),
    },
    Entry {
        code: "E0502",
        family: Family::SettingsAndRecycle,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Semantics),
    },
    Entry {
        code: "E0503",
        family: Family::SettingsAndRecycle,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Semantics),
    },
    Entry {
        code: "E0504",
        family: Family::SettingsAndRecycle,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Semantics),
    },
    Entry {
        code: "E0505",
        family: Family::SettingsAndRecycle,
        severity: Severity::Error,
        emitter: Emitter::Stage(Stage::Semantics),
    },
    Entry {
        code: "E0506",
        family: Family::SettingsAndRecycle,
        severity: Severity::Error,
        emitter: Emitter::NoneYet("S4, with the capability catalogue"),
    },
    Entry {
        code: "E0507",
        family: Family::SettingsAndRecycle,
        severity: Severity::Error,
        emitter: Emitter::NoneYet("S4, with licences"),
    },
    // --- E06xx and W06xx: economy, power and messaging -----------------------
    Entry {
        code: "E0601",
        family: Family::EconomyPowerAndMessaging,
        severity: Severity::Error,
        emitter: Emitter::NoneYet("S1, the estimate stage"),
    },
    Entry {
        code: "E0602",
        family: Family::EconomyPowerAndMessaging,
        severity: Severity::Error,
        emitter: Emitter::NoneYet("S4, with radio"),
    },
    Entry {
        code: "W0601",
        family: Family::EconomyPowerAndMessaging,
        severity: Severity::Warning,
        emitter: Emitter::NoneYet("S1, the estimate stage"),
    },
    Entry {
        code: "W0602",
        family: Family::EconomyPowerAndMessaging,
        severity: Severity::Warning,
        emitter: Emitter::NoneYet("S1, the estimate stage"),
    },
    // `proto/gp/v1/playbook.proto` names this warning by hand and leaves it
    // unnumbered: "When false, a playbook that adds draw beyond supply is an
    // error (E0601). When true it is allowed, and the shortfall is a warning
    // (W06xx)." It is a different sentence to `W0601`, which is the shortfall
    // the grid is *already* carrying before this playbook runs, so it gets a row
    // of its own rather than being folded in.
    Entry {
        code: "W0603",
        family: Family::EconomyPowerAndMessaging,
        severity: Severity::Warning,
        emitter: Emitter::NoneYet("S1, the estimate stage"),
    },
    // --- W07xx: schedule, conflicts and staleness ----------------------------
    Entry {
        code: "W0701",
        family: Family::ScheduleConflictsAndStaleness,
        severity: Severity::Warning,
        emitter: Emitter::NoneYet("S1, the estimate stage"),
    },
    Entry {
        code: "W0702",
        family: Family::ScheduleConflictsAndStaleness,
        severity: Severity::Warning,
        emitter: Emitter::NoneYet("S3, the lint stage"),
    },
    Entry {
        code: "W0703",
        family: Family::ScheduleConflictsAndStaleness,
        severity: Severity::Warning,
        emitter: Emitter::NoneYet("S2/S3, with the reach window"),
    },
    // --- I...: information ---------------------------------------------------
    Entry {
        code: "I0001",
        family: Family::Information,
        severity: Severity::Info,
        emitter: Emitter::NoneYet("T7/S1, the estimate stage"),
    },
    Entry {
        code: "I0002",
        family: Family::Information,
        severity: Severity::Info,
        emitter: Emitter::NoneYet("S3, the lint stage"),
    },
];

/// The row for one code, or `None` if the catalogue does not have it.
#[must_use]
pub fn entry(code: &str) -> Option<&'static Entry> {
    CATALOGUE.iter().find(|row| row.code == code)
}

/// The whole catalogue as one JSON value, ready for the golden file.
///
/// Deterministic by construction: the table's own order, one object per row,
/// and no map anywhere in it.
#[must_use]
pub fn catalogue_json() -> Json {
    Json::Array(
        CATALOGUE
            .iter()
            .map(|row| {
                Json::Object(vec![
                    ("code".to_owned(), Json::String(row.code.to_owned())),
                    (
                        "family".to_owned(),
                        Json::String(row.family.name().to_owned()),
                    ),
                    (
                        "severity".to_owned(),
                        Json::String(severity_name(row.severity).to_owned()),
                    ),
                    ("emitter".to_owned(), Json::String(row.emitter.label())),
                    ("message".to_owned(), Json::String(row.message().to_owned())),
                    (
                        "beginner".to_owned(),
                        Json::String(row.beginner().to_owned()),
                    ),
                ])
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::{CATALOGUE, Emitter, Entry, Family, Stage, entry};
    use crate::strings::VERIFIER_STRINGS;

    #[test]
    fn the_two_tables_name_the_same_codes() {
        assert_eq!(
            CATALOGUE.len(),
            VERIFIER_STRINGS.len(),
            "the catalogue and the string table are different lengths"
        );
        for (row, text) in CATALOGUE.iter().zip(VERIFIER_STRINGS) {
            assert_eq!(
                row.code, text.code,
                "the catalogue and the string table are in different orders"
            );
            assert!(!row.message().is_empty(), "{} has no message", row.code);
            assert!(!row.beginner().is_empty(), "{} has no beginner", row.code);
        }
    }

    #[test]
    fn the_codes_ascend_inside_a_family_and_never_repeat() {
        // The table is in the order spec section 11 lists the families, not in
        // ASCII order: `I...` sorts before `W07xx` as text and comes after it as
        // a family. What must hold is that a family's codes ascend — so a new
        // one lands at the end of its own block — and that no number is ever
        // used twice, which is what "append-only from the day they ship" means.
        let mut previous: Option<&Entry> = None;
        for row in CATALOGUE {
            if let Some(before) = previous {
                if before.family == row.family {
                    assert!(
                        before.code < row.code,
                        "{} follows {} inside one family; a family's codes ascend",
                        row.code,
                        before.code
                    );
                }
            }
            let count = CATALOGUE
                .iter()
                .filter(|other| other.code == row.code)
                .count();
            assert_eq!(count, 1, "{} appears {count} times", row.code);
            previous = Some(row);
        }
    }

    #[test]
    fn the_families_are_in_the_order_the_spec_lists_them() {
        let mut families: Vec<Family> = Vec::new();
        for row in CATALOGUE {
            if families.last() != Some(&row.family) {
                assert!(
                    !families.contains(&row.family),
                    "{} reopens the {} family after it closed; a family is one block",
                    row.code,
                    row.family.name()
                );
                families.push(row.family);
            }
        }
        assert_eq!(
            families,
            [
                Family::DecodeAndVersion,
                Family::LimitsAndRequiredBlocks,
                Family::Conditions,
                Family::ControlFlow,
                Family::ReferencesAndPlacement,
                Family::SettingsAndRecycle,
                Family::EconomyPowerAndMessaging,
                Family::ScheduleConflictsAndStaleness,
                Family::Information,
            ]
        );
    }

    #[test]
    fn every_code_is_reachable_by_lookup() {
        for row in CATALOGUE {
            assert_eq!(entry(row.code).map(|found| found.code), Some(row.code));
        }
        assert!(entry("E9999").is_none());
    }

    #[test]
    fn nothing_the_skeleton_emits_hangs_off_an_empty_full_stage() {
        // Item 82: estimate and lint are present and empty, so no row may claim
        // one as its emitter until S1 and S3 fill them.
        for row in CATALOGUE {
            if let Emitter::Stage(stage) = row.emitter {
                assert!(
                    stage.in_quick(),
                    "{} names {} as its emitter, but that stage is empty at the skeleton",
                    row.code,
                    stage.name()
                );
            }
        }
    }

    #[test]
    fn quick_is_the_first_four_stages_and_full_is_all_six() {
        let quick: Vec<&str> = Stage::ALL
            .iter()
            .filter(|stage| stage.in_quick())
            .map(|stage| stage.name())
            .collect();
        assert_eq!(quick, ["decode", "structure", "resolve", "semantics"]);
        assert_eq!(Stage::ALL.len(), 6);
    }
}

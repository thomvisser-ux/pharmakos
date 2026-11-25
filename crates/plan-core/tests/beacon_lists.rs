// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A beacon's lists as `get_beacon` names them (S3's `edlists`): each list's
//! pointer and its clear's are the schema's, paired one to one, and each entry
//! line is said in the string table's words.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "clippy.toml sets allow-expect-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules, not an integration test's helpers. A panic is this file's failure report: these are panic lints, not determinism lints (AGENTS.md section 5)."
)]

use pharmakos_plan_core::render::{
    BeaconList, area_line, protected_beacon_line, protected_commander_line,
};
use pharmakos_plan_core::strings as s;
use pharmakos_proto::descriptor::{ScalarKind, schema};
use pharmakos_sim::seams::MandateKind;

/// The field a pointer inside `gp.v1.MandateSettings` names, by the schema.
fn field_at(pointer: &str) -> pharmakos_proto::descriptor::Field {
    let mut message = String::from("gp.v1.MandateSettings");
    let mut found = None;
    for segment in pointer.trim_start_matches('/').split('/') {
        let field = schema()
            .message(&message)
            .and_then(|held| held.field_by_name(segment))
            .unwrap_or_else(|| panic!("{pointer}: {message} has no `{segment}`"))
            .clone();
        message.clone_from(&field.type_name);
        found = Some(field);
    }
    found.unwrap_or_else(|| panic!("{pointer} names nothing"))
}

/// Each list's pointer is a repeated field of its writ's arm, and its clear's
/// is the bool beside it named `clear_<list>`: no list is paired with another's
/// clear.
#[test]
fn each_list_and_its_clear_are_the_schemas_pair() {
    for list in BeaconList::ALL {
        let held = field_at(list.pointer());
        assert!(held.repeated, "{list:?}: a list");
        let clear = field_at(list.clear_pointer());
        assert!(
            !clear.repeated && clear.kind == ScalarKind::Bool,
            "{list:?}"
        );
        let (arm, name) = list.pointer().rsplit_once('/').expect("an arm and a field");
        assert_eq!(
            list.clear_pointer(),
            format!("{arm}/clear_{name}"),
            "{list:?}"
        );
    }
    let clears: Vec<&str> = BeaconList::ALL
        .iter()
        .map(|list| list.clear_pointer())
        .collect();
    let mut distinct = clears.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(clears.len(), distinct.len());
}

/// Each writ holds the list of its arm, and Mine and no writ hold none.
#[test]
fn each_writ_holds_its_arms_list() {
    for (writ, arm, held) in [
        (
            MandateKind::Build,
            "build",
            Some(BeaconList::ProtectedAreas),
        ),
        (MandateKind::Defend, "defend", Some(BeaconList::Protect)),
        (MandateKind::Attack, "attack", Some(BeaconList::AvoidAreas)),
        (MandateKind::Survey, "survey", Some(BeaconList::ProbeAreas)),
        (MandateKind::Mine, "mine", None),
        (MandateKind::None, "", None),
    ] {
        assert_eq!(BeaconList::of(writ), held, "{writ:?}");
        if let Some(list) = held {
            assert!(list.pointer().starts_with(&format!("/{arm}/")), "{list:?}");
        }
    }
}

/// The lines are the string table's words, and the list's words differ list by
/// list.
#[test]
fn the_lines_are_the_string_tables() {
    assert_eq!(
        area_line([30, -4], [38, 44]),
        format!(
            "{} {} 30, -4 {} {} 38, 44",
            s::LIST_AREA_FROM,
            s::COLUMN_NOUN,
            s::LIST_AREA_TO,
            s::COLUMN_NOUN
        )
    );
    assert_eq!(
        protected_beacon_line("b_03"),
        format!("{} \"b_03\"", s::BEACON_NOUN)
    );
    assert_eq!(protected_commander_line(), s::DEFEND_MY_COMMANDER);
    let mut titles: Vec<&str> = BeaconList::ALL.iter().map(|list| list.title()).collect();
    titles.extend(BeaconList::ALL.iter().map(|list| list.clear_title()));
    let mut distinct = titles.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), titles.len(), "{titles:?}");
}

/// Fix pass 1, T3: an area's line pinned by its literal words, its lowest
/// column first (the review's R07, the corners swapped, is red here).
#[test]
fn an_area_is_said_lowest_column_first_in_these_words() {
    assert_eq!(
        area_line([10, 10], [11, 11]),
        "the ground from column 10, 10 to column 11, 11"
    );
    assert_eq!(
        area_line([-4, -2], [-2, 1]),
        "the ground from column -4, -2 to column -2, 1"
    );
}

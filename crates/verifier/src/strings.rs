// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The string table's verifier section: every user-facing string this crate can
//! produce, in one place.
//!
//! Diagnostic codes are language-neutral and messages are not (spec section
//! 11). A client branches on the code; a player reads the message. So the codes
//! live in [`crate::catalogue`], which is a data table of *identity*, and the
//! words live here, which is a data table of *text*, and the two are joined by
//! the code. `tests/golden/verifier/expected.catalogue.json` pins the join, so
//! a code cannot lose its words and a set of words cannot lose its code.
//!
//! **PLACEHOLDER — the one project-wide string table.** Spec section 11 says
//! every user-facing string in v1 is English and lives in *one* table: the user
//! interface, this catalogue, and `render_plan`'s plain-language rendering.
//! This module is that table's verifier section and nothing more. Who assembles
//! the single table, where it lives and what its wording is settles with the
//! editor at **S6** and is the **owner's** call; the skeleton plan already
//! carries the same PLACEHOLDER against T8. Until then a message is written
//! here, once, and read by everyone through [`text`].
//!
//! # Templates, not sentences
//!
//! A [`Text::message`] is a template: `{name}` is substituted by [`fill`] at the
//! moment the diagnostic is emitted. The template is what the catalogue golden
//! records, so the golden pins the *wording* rather than one rendering of it,
//! and there is exactly one string per code rather than one for the catalogue
//! and another at the call site that can drift away from it.
//!
//! A [`Text::beginner`] takes no substitutions. It is the plain-language
//! sentence, and it says what the rule is for rather than what this file did
//! wrong — a number in it would make it a second copy of the message.

/// One row of the string table: a code and the two strings it shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Text {
    /// The diagnostic code these strings belong to.
    pub code: &'static str,
    /// The precise message, as a template. `{name}` is filled by [`fill`].
    pub message: &'static str,
    /// The plain-language sentence. Never templated.
    pub beginner: &'static str,
}

/// The verifier's rows of the one English string table, in code order.
///
/// Code order is the same order [`crate::catalogue::CATALOGUE`] is in, and a
/// test asserts that the two tables name exactly the same codes.
pub const VERIFIER_STRINGS: &[Text] = &[
    // --- E000x: decode and version ------------------------------------------
    Text {
        code: "E0001",
        message: "the playbook did not decode: {detail}",
        beginner: "This file is not a playbook the game can read. The message says where reading \
                   stopped.",
    },
    Text {
        code: "E0002",
        message: "`{field}` is not a field the schema declares. Unknown fields are rejected, \
                  never stripped.",
        beginner: "There is a name here that the playbook vocabulary does not have. Nothing is \
                   removed for you: correct the name or take the line out yourself.",
    },
    Text {
        code: "E0003",
        message: "`{field}` is held back to a later version and is not part of the v1 vocabulary.",
        beginner: "This is a word the game keeps for later. It is refused rather than quietly \
                   ignored, so nothing you wrote disappears without you seeing it.",
    },
    Text {
        code: "E0004",
        message: "schema version {found} is not supported; this build speaks {supported}.",
        beginner: "The file says it was written for a different version of the game.",
    },
    Text {
        code: "E0005",
        message: "`kind` is unset; a playbook must say whether it is a PLAYBOOK, a TEMPLATE or a \
                  SAMPLE.",
        beginner: "The file does not say what it is.",
    },
    Text {
        code: "E0006",
        message: "`author_kind` is SCRIPT, which is reserved for a later version and is rejected \
                  in v1.",
        beginner: "Scripts are not part of this version of the game.",
    },
    Text {
        code: "E0007",
        message: "`author_kind` is unset; an unset tag is an error, never a default.",
        beginner: "The file does not say who wrote it.",
    },
    Text {
        code: "E0008",
        message: "the recorded fingerprint does not match these bytes; the verifier has \
                  recomputed it.",
        beginner: "The stamp on this file was out of date. It has been worked out again for you.",
    },
    // --- E01xx: limits and required blocks -----------------------------------
    Text {
        code: "E0101",
        message: "{what} is required and is missing.",
        beginner: "A part that every playbook must have is not here.",
    },
    Text {
        code: "E0102",
        message: "the route has no steps.",
        beginner: "A playbook needs at least one step for the commander to walk.",
    },
    Text {
        code: "E0103",
        message: "the step names no action.",
        beginner: "This step does not say what to do.",
    },
    Text {
        code: "E0104",
        message: "{what} has no name.",
        beginner: "Give this a name, so the rest of the playbook can refer to it.",
    },
    Text {
        code: "E0105",
        message: "`{name}` is used twice; a name is unique {scope}.",
        beginner: "Two things share a name, so a reference to it would be ambiguous.",
    },
    Text {
        code: "E0106",
        message: "the handler has no condition.",
        beginner: "A rule needs something to watch for.",
    },
    Text {
        code: "E0107",
        message: "`max_fires` is {found}; it must be between 1 and {max}.",
        beginner: "A rule fires between once and a few times in a segment.",
    },
    Text {
        code: "E0108",
        message: "`cooldown_ms` is {found} ms; the minimum is {min} ms.",
        beginner: "A rule waits a while between firings, so put at least the minimum here.",
    },
    Text {
        code: "E0109",
        message: "`{field}` is {found} ms; a duration is never negative.",
        beginner: "Time does not run backwards. Use nothing, or more.",
    },
    Text {
        code: "E0110",
        message: "the playbook is {found} size units; the budget is {budget}.",
        beginner: "This playbook is larger than the seal allows. Take something out of it.",
    },
    Text {
        code: "E0111",
        message: "`{field}` is unset; an unset choice is an error, never a default.",
        beginner: "One of the choices here has been left blank.",
    },
    Text {
        code: "E0112",
        message: "the notebook is {found} characters; the maximum is {max}.",
        beginner: "The notebook is longer than a seat may keep.",
    },
    // --- E02xx: conditions ---------------------------------------------------
    Text {
        code: "E0201",
        message: "the comparison names no operator.",
        beginner: "Say how to compare: at most, equal to, more than, and so on.",
    },
    Text {
        code: "E0202",
        message: "the percentage is {found}; it must be between 0 and 100.",
        beginner: "A percentage runs from none of it to all of it.",
    },
    Text {
        code: "E0203",
        message: "`{node}` has no items.",
        beginner: "This condition groups other conditions, and there are none inside it.",
    },
    Text {
        code: "E0204",
        message: "the condition is {found} levels deep; the limit is {max}.",
        beginner: "This condition is nested too deeply to be read at a glance. Flatten it.",
    },
    Text {
        code: "E0205",
        message: "the condition has {found} nodes; the limit is {max}.",
        beginner: "This condition has too many parts.",
    },
    Text {
        code: "E0206",
        message: "the condition names no predicate.",
        beginner: "This condition is empty.",
    },
    Text {
        code: "E0207",
        message: "`rule_fired` names `{id}`, which this playbook does not declare.",
        beginner: "There is no rule by that name in this playbook.",
    },
    Text {
        code: "E0208",
        message: "`step_reached` names `{label}`, which this playbook's route does not declare.",
        beginner: "There is no step by that name on the route.",
    },
    Text {
        code: "E0209",
        message: "an enemy predicate must declare its freshness.",
        beginner: "Say how recently you must have seen the thing you are asking about.",
    },
    // --- E03xx: control flow -------------------------------------------------
    Text {
        code: "E0301",
        message: "`{label}` is at or before this step; a jump only ever goes forward.",
        beginner: "A jump can send the commander onward, never back. That is part of why a \
                   playbook always finishes.",
    },
    Text {
        code: "E0302",
        message: "`wait_until` has no timeout, and every wait has one.",
        beginner: "A wait needs a time limit, or the commander could stand there all segment.",
    },
    Text {
        code: "E0303",
        message: "`hold` is {found} ms; a hold lasts a positive number of milliseconds.",
        beginner: "Say how long to hold for.",
    },
    Text {
        code: "E0304",
        message: "the jump names no target label.",
        beginner: "This says to jump, but not where to.",
    },
    Text {
        code: "E0305",
        message: "`{label}` is not a label this {scope} declares.",
        beginner: "There is no step by that name to jump to.",
    },
    // --- E04xx: references and placement -------------------------------------
    Text {
        code: "E0401",
        message: "`{id}` is not a beacon this seat's snapshot holds.",
        beginner: "The seat does not know a beacon by that name.",
    },
    Text {
        code: "E0402",
        message: "the voxel ({x}, {y}, {z}) lies outside the map, which is {sx} by {sy} by {sz}.",
        beginner: "That place is off the edge of the world.",
    },
    Text {
        code: "E0403",
        message: "the site ({x}, {y}, {z}) lies outside every one of this seat's spheres.",
        beginner: "A beacon goes up inside ground you already hold.",
    },
    Text {
        code: "E0404",
        message: "an enemy-side filter and a tag filter select nothing together: an enemy beacon \
                  carries none of your tags.",
        beginner: "Nothing can match both of these at once, so this selector never finds \
                   anything.",
    },
    Text {
        code: "E0405",
        message: "`{id}` is a beacon this seat does not own.",
        beginner: "A commander interfaces with your own beacons.",
    },
    Text {
        code: "E0406",
        message: "the area is inside out: min ({minx}, {miny}, {minz}) is not at or below max \
                  ({maxx}, {maxy}, {maxz}) on every axis.",
        beginner: "The two corners of this box are the wrong way round, so the box holds nothing.",
    },
    // --- E05xx: settings and recycle -----------------------------------------
    Text {
        code: "E0501",
        message: "`{field}` is {found}; it must be between 0 and 100.",
        beginner: "A percentage runs from none of it to all of it.",
    },
    Text {
        code: "E0502",
        message: "the mandate settings name no mandate.",
        beginner: "Say which writ the beacon is on.",
    },
    Text {
        code: "E0503",
        message: "`{id}` is on the {mandate} writ; a build target is only added to a beacon on \
                  BUILD.",
        beginner: "Only a beacon told to build takes a build target.",
    },
    Text {
        code: "E0504",
        message: "`{id}` is the core, and the core cannot be recycled.",
        beginner: "The core is where the seat begins, and it cannot be taken apart.",
    },
    Text {
        code: "E0505",
        message: "`rotation_quarter_turns` is {found}; it must be between 0 and 3.",
        beginner: "A building turns in quarter turns: none, one, two or three.",
    },
    Text {
        code: "E0506",
        message: "`{id}` is not a blueprint the catalogue holds.",
        beginner: "There is no such thing to build.",
    },
    Text {
        code: "E0507",
        message: "`{id}` needs a licence this seat does not hold.",
        beginner: "The seat has not earned the right to build this yet.",
    },
    // --- E06xx and W06xx: economy, power and messaging -----------------------
    Text {
        code: "E0601",
        message: "the playbook adds {found} kW of draw beyond supply; set \
                  `allow_dormant_beacons` to accept the shortfall instead.",
        beginner: "There is not enough power for everything this playbook switches on.",
    },
    Text {
        code: "E0602",
        message: "a broadcast needs a Radio Mast, and coverage at the place it is sent from.",
        beginner: "Nothing can be sent without a mast.",
    },
    Text {
        code: "W0601",
        message: "the grid is already {found} kW short.",
        beginner: "The power the seat has is already less than the power it is using.",
    },
    Text {
        code: "W0602",
        message: "committed spending of {found} outruns the projected treasury of {available}.",
        beginner: "This playbook spends more than the seat is likely to have.",
    },
    // --- W07xx: schedule, conflicts and staleness ----------------------------
    Text {
        code: "W0701",
        message: "the route needs about {found} ms and the coming segment is {segment} ms.",
        beginner: "There may not be time to walk this whole route this round.",
    },
    Text {
        code: "W0702",
        message: "`{id}` can never fire: `{winner}` is earlier and its condition is true first.",
        beginner: "An earlier rule always wins, so this one never gets a turn.",
    },
    Text {
        code: "W0703",
        message: "the sighting this rests on is {found} ms old; the reach window is {max} ms.",
        beginner: "What the seat remembers here may be too old to act on.",
    },
    // --- I...: information ---------------------------------------------------
    Text {
        code: "I0001",
        message: "the route is about {found} ms of walking.",
        beginner: "Roughly how long the walking takes.",
    },
    Text {
        code: "I0002",
        message: "`{selector}` would resolve to `{id}` right now.",
        beginner: "What this selector picks if the round started now.",
    },
];

/// The row for one code, or `None` if the table does not have it.
#[must_use]
pub fn text(code: &str) -> Option<&'static Text> {
    VERIFIER_STRINGS.iter().find(|row| row.code == code)
}

/// A template with every `{name}` replaced by its argument.
///
/// A placeholder with no argument is left exactly as written rather than
/// blanked: a message that reads `{found}` is a bug report, and a message with a
/// silent hole in it is not.
#[must_use]
pub fn fill(template: &str, args: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let (head, tail) = rest.split_at(open);
        out.push_str(head);
        let Some(close) = tail.find('}') else {
            out.push_str(tail);
            return out;
        };
        let name = tail.get(1..close).unwrap_or_default();
        match args.iter().find(|(key, _)| *key == name) {
            Some((_, value)) => out.push_str(value),
            None => out.push_str(tail.get(..close.saturating_add(1)).unwrap_or_default()),
        }
        rest = tail.get(close.saturating_add(1)..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::{VERIFIER_STRINGS, fill, text};

    #[test]
    fn a_template_is_filled_by_name() {
        assert_eq!(
            fill(
                "`max_fires` is {found}; the limit is {max}.",
                &[("found", "9".to_owned()), ("max", "8".to_owned()),]
            ),
            "`max_fires` is 9; the limit is 8."
        );
    }

    #[test]
    fn an_unsupplied_placeholder_is_left_visible() {
        assert_eq!(fill("a {missing} b", &[]), "a {missing} b");
    }

    #[test]
    fn every_row_is_reachable_and_no_code_appears_twice() {
        for row in VERIFIER_STRINGS {
            assert!(text(row.code).is_some(), "{} is unreachable", row.code);
            let count = VERIFIER_STRINGS
                .iter()
                .filter(|other| other.code == row.code)
                .count();
            assert_eq!(count, 1, "{} appears {count} times", row.code);
        }
    }

    #[test]
    fn no_beginner_sentence_is_templated() {
        for row in VERIFIER_STRINGS {
            assert!(
                !row.beginner.contains('{'),
                "{}'s beginner sentence is templated; it must read the same every time",
                row.code
            );
        }
    }
}

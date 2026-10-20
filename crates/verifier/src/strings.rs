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
//! PLACEHOLDER: the one project-wide string table — owner, S6, with the editor.
//!
//! Spec section 11 says
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
        message: "the schema version is {found}, and this build speaks {supported}.",
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
    // E0009 to E0017: constructs in the v1 vocabulary that the sim refuses at
    // the seal because their effect needs a later stage (S2's plan, E8). Each
    // message says what was written and when it starts working.
    Text {
        code: "E0009",
        message: "a `queue_structure` row has no effect until S4, with the capability catalogue \
                  and its licences; it is refused rather than ignored.",
        beginner: "Queuing a capability building comes later in the game. Nothing you wrote is \
                   dropped: take the row out yourself.",
    },
    Text {
        code: "E0010",
        message: "a Build target names `{id}`; beside `generator` and S2's base kit, nothing can \
                  be built until S4, with the capability catalogue and its licences.",
        beginner: "This building cannot be made by a Build beacon in this build.",
    },
    Text {
        code: "E0011",
        message: "a Build target is anchored on `{arm}`; until S3 a target stands on a voxel or \
                  `on` a vent.",
        beginner: "A building is placed on a spot on the map, not on whichever beacon a selector \
                   picks. Pick the spot.",
    },
    Text {
        code: "E0012",
        message: "Defend mandate settings have no effect until S2's Defend mandate lands; they \
                  are refused rather than ignored.",
        beginner: "A Defend beacon cannot be given settings in this build yet.",
    },
    Text {
        code: "E0013",
        message: "Attack mandate settings have no effect until S2's Attack mandate lands; they \
                  are refused rather than ignored.",
        beginner: "An Attack beacon cannot be given settings in this build yet.",
    },
    Text {
        code: "E0014",
        message: "selector `most_threatened` has no effect until S2's threat model, which is what \
                  it ranks by; it is refused rather than ignored.",
        beginner: "\"Most threatened\" needs threats to weigh, and this build has none yet. Name \
                   the beacon, or pick it another way.",
    },
    Text {
        code: "E0015",
        message: "selector filter `side: ENEMY_KNOWN` has no effect until S3, with the seat's \
                  knowledge store; it is refused rather than ignored.",
        beginner: "Picking among enemy beacons comes later. For now a selector picks among your \
                   own.",
    },
    Text {
        code: "E0016",
        message: "selector filter `tags` has no effect until S3: a beacon has no tag column to \
                  read yet, so writing tags is legal and filtering on them is not.",
        beginner: "Tags on a beacon are kept, but nothing can pick a beacon by its tags yet.",
    },
    Text {
        code: "E0017",
        message: "predicate `beacon_under_attack` has no effect until S2's combat gives a beacon \
                  a damage history to ask about; it is refused rather than read as true or false.",
        beginner: "Asking whether a beacon is under attack comes with combat. Until then the \
                   question has no answer, so it cannot be asked.",
    },
    Text {
        code: "E0018",
        message: "`{field}` carries {value}, which is not a value this build names; it is refused \
                  rather than read as a default.",
        beginner: "This setting holds a number the game does not know. Nothing is guessed for \
                   you: write one of the setting's names instead.",
    },
    Text {
        code: "E0019",
        message: "a Build target names `{id}`, one of S2's base kit, which has no effect until \
                  S2's base kit lands; it is refused rather than ignored.",
        beginner: "Autocannons, mortars and walls cannot be built in this build yet.",
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
    Text {
        code: "E0113",
        message: "the playbook has {found} handlers; this build compiles at most {max}.",
        beginner: "A playbook can carry only so many rules, however small each one is. Take \
                   some out.",
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
    Text {
        code: "E0407",
        message: "`covering` is legal only in a place_beacon's `at`, which is where a new beacon \
                  goes; here it names no site a beacon is placed at.",
        beginner: "\"A site covering a vent\" says where a new beacon goes, so it belongs in a \
                   place-beacon step and nowhere else.",
    },
    Text {
        code: "E0408",
        message: "`on` is legal only in a Build target's anchor, and as a `feature_id` in \
                  remove_build_target; here there is no structure for it to stand.",
        beginner: "\"On a vent\" says where a building goes, so it belongs in a Build target.",
    },
    Text {
        code: "E0409",
        message: "`covered` is legal only under `on` in a Build target inside the `initial` of a \
                  place_beacon whose `at` is `covering`: it means the feature that beacon was \
                  placed to cover.",
        beginner: "\"The vent this beacon was placed to cover\" only means something for a beacon \
                   placed to cover one, in the settings it starts with.",
    },
    Text {
        code: "E0410",
        message: "remove_build_target names the target it removes by its feature's `feature_id`; \
                  a description names no one target to remove.",
        beginner: "To take a building off the list, name the vent it stands on.",
    },
    Text {
        code: "E0411",
        message: "`coverage` must be omitted under `on`: the pick ranks the vents inside the \
                  target beacon's sphere, and coverage is not one of its filters there.",
        beginner: "Under \"on\", leave the coverage out.",
    },
    Text {
        code: "E0412",
        message: "`{id}` is not a feature this seat's snapshot holds.",
        beginner: "The seat does not know a vent or seam by that name.",
    },
    Text {
        code: "E0413",
        message: "`on` stands a structure on a heat vent, and {what} names a scrap seam.",
        beginner: "Buildings stand on vents. A seam is dug, not built on.",
    },
    Text {
        code: "E0414",
        message: "`{id}` is {side}; {role} names {wanted}.",
        beginner: "A Defend beacon protects your own beacons, and an Attack beacon attacks an \
                   enemy's.",
    },
    Text {
        code: "E0415",
        message: "{role} is a selector that ranks {found}; it must rank {wanted}.",
        beginner: "A Defend beacon protects your own beacons, and an Attack beacon attacks an \
                   enemy's, so a selector there has to look among the right side's beacons.",
    },
    Text {
        code: "E0416",
        message: "{what} column ({x}, {y}) lies outside the sphere of the beacon whose mandate \
                  holds the target.",
        beginner: "A beacon builds only inside its own sphere.",
    },
    Text {
        code: "E0417",
        message: "a wall's anchor names the wall and must be a voxel inside its box; {found}.",
        beginner: "A wall is found by its anchor, so the anchor has to be one of the wall's own \
                   voxels.",
    },
    Text {
        code: "E0418",
        message: "{what}; an Attack target must have been seen within the reach window, {max} \
                  ms, when the playbook is sealed.",
        beginner: "An Attack beacon attacks what the seat has seen recently. Look again with a \
                   scout, or choose a target the seat has seen.",
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
    Text {
        code: "E0508",
        message: "`{field}` is {found}; a beacon holds at most {most} scouts.",
        beginner: "A beacon can field only so many scouts. Ask for fewer.",
    },
    Text {
        code: "E0509",
        message: "a `{id}` target carries a `wall_box`; only a `wall` target fills a box, and \
                  every other structure's footprint is its blueprint's.",
        beginner: "Only a wall is built as a box of voxels. Leave the box out, or make this \
                   target a wall.",
    },
    Text {
        code: "E0510",
        message: "`{field}` is {found}; it must be {range}.",
        beginner: "This setting has a range, and the value written is outside it.",
    },
    Text {
        code: "E0511",
        message: "a wall is not turned: `rotation_quarter_turns` is {found}, and a wall's is 0.",
        beginner: "A wall's box already says where every voxel goes, so it has no rotation.",
    },
    Text {
        code: "E0512",
        message: "the wall box is {found} voxels from `min.z` to `max.z`; a wall stands at least \
                  {min} voxels above the ground, and `min.z` lies at or below the ground, so \
                  this box cannot.",
        beginner: "A wall lower than this is climbable and blocks nothing. Raise the top of the \
                   box.",
    },
    Text {
        code: "W0501",
        message: "`seam_choice` SAFEST reads as NEAREST until S2's threat model arrives.",
        beginner: "\"Safest\" needs threats to weigh, and they come later. Until then this beacon \
                   digs the nearest seam.",
    },
    Text {
        code: "W0502",
        message: "`{name}` is on this protect list more than once; its guards escort it once, \
                  and the repeat only adds a field to the edit.",
        beginner: "This asset is already on the list. Naming it again changes nothing.",
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
        code: "E0603",
        message: "the route orders up to {found}, past what the Ledger can price a seal at \
                  ({most}); order less in one playbook.",
        beginner: "This playbook asks for more than a seal can hold. The game would refuse it \
                   when you submit, so it is refused here first.",
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
    Text {
        code: "W0603",
        message: "the playbook adds {found} kW of draw beyond supply; `allow_dormant_beacons` \
                  accepts it, and the beacons past the supply stay dormant.",
        beginner: "Some of what this playbook switches on will have no power and will sit idle.",
    },
    Text {
        code: "W0604",
        message: "the playbook may add up to {found} kW of draw beyond supply: an edit of a beacon \
                  that may already field what it orders is counted in full, because the view \
                  cannot see what the beacon fields.",
        beginner: "This playbook might switch on more than the seat's power can carry; it depends \
                   on what the beacons it edits already have.",
    },
    // --- W07xx: schedule, conflicts and staleness ----------------------------
    Text {
        code: "W0701",
        message: "the route needs at least {found} ms if every step runs, and the coming segment \
                  is {segment} ms.",
        beginner: "There is not time for this whole route this round.",
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
    Text {
        code: "W0704",
        message: "{what} matches nothing on the map right now: {why}.",
        beginner: "Unless the map changes before this step starts, it finds nothing and fails.",
    },
    Text {
        code: "W0705",
        message: "an earlier step places a beacon, so {what} may read differently when this \
                  step starts than it does now.",
        beginner: "A description is read when its step starts, and a beacon placed before then \
                   can change what it picks.",
    },
    Text {
        code: "W0706",
        message: "every vent a beacon at this fixed site could cover is already covered: {feature} \
                  by your `{beacon}`.",
        beginner: "A fixed site puts its beacon in the same place every time. A description such \
                   as \"the nearest vent you do not cover\" reads the map again each round.",
    },
    Text {
        code: "W0707",
        message: "the snapshot this report is about does not decode, so whether the route fits \
                  the coming segment was not checked.",
        beginner: "The game could not check this route's timing. That is a fault in the game, not \
                   in the playbook.",
    },
    // --- I...: information ---------------------------------------------------
    Text {
        code: "I0001",
        message: "the route is at least {found} ms of walking, between the places it names.",
        beginner: "The least time the walking can take; going round anything in the way takes \
                   longer.",
    },
    Text {
        code: "I0002",
        message: "`{selector}` would resolve to `{id}` right now.",
        beginner: "What this selector picks if the round started now.",
    },
    Text {
        code: "I0003",
        message: "`{field}` is omitted and reads as {value}.",
        beginner: "A setting left out takes its named default.",
    },
    Text {
        code: "I0004",
        message: "`repair_threshold_pct` is 0, so this Build beacon's drones never repair a \
                  structure; its units and the commander are still repaired.",
        beginner: "A repair threshold of 0 means never. Write a percentage to have damaged \
                   structures repaired.",
    },
];

// --- Words the templates' arguments use --------------------------------------
//
// A template's `{what}`, `{why}`, `{feature}` and `{value}` are filled at the
// call site, and the English they are filled with lives here rather than there,
// so the one table holds every word a player reads.

/// A heat vent's display name, before its anchor: "Heat vent (120, 88)"
/// (docs/design/targeting.md, "Names").
pub const VENT_NAME: &str = "Heat vent";
/// A scrap seam's display name, before its anchor.
pub const SEAM_NAME: &str = "Scrap seam";
/// A vent pick with no coverage filter: "the nearest vent".
pub const PICK_NEAREST_VENT: &str = "the nearest vent";
/// A seam pick with no coverage filter.
pub const PICK_NEAREST_SEAM: &str = "the nearest seam";
/// A vent pick under UNCOVERED.
pub const PICK_NEAREST_UNCOVERED_VENT: &str = "the nearest vent you do not cover";
/// A seam pick under UNCOVERED.
pub const PICK_NEAREST_UNCOVERED_SEAM: &str = "the nearest seam you do not cover";
/// W0704's reason when no feature of the kind is live.
pub const WHY_NONE_LIVE: &str = "none of that kind is live";
/// W0704's reason when every live one lies inside one of the seat's spheres.
pub const WHY_ALL_COVERED: &str = "every live one is inside one of your spheres";
/// W0704's reason for a name whose feature is lost.
pub const WHY_LOST: &str = "it is lost";
/// E0413's `{what}` for a seam pick.
pub const SEAM_PICK: &str = "the `seam` pick";
/// E0413's `{what}` for `covered` under a deploy that covers a seam.
pub const COVERED_SEAM: &str = "`covered`, whose deploy covers a seam,";
/// I0003's `{value}` for an omitted `pillar_spacing`.
pub const NO_PILLARS: &str = "0, no pillars";
/// W0706's Fix: swap the fixed site for the description it should have been.
pub const FIX_COVER_NEAREST_VENT: &str = "Cover the nearest vent you do not cover yet";
/// I0003's Fix: write the named default out.
pub const FIX_WRITE_DEFAULT: &str = "Write the default out";
/// E0111's Fix on a pick with no rank.
pub const FIX_RANK_NEAREST: &str = "Rank it by nearest";
/// E0111's Fix on a `covering` pick with no coverage filter.
pub const FIX_COVERAGE_UNCOVERED: &str = "Pick only what you do not cover yet";
/// E0411's Fix.
pub const FIX_DROP_COVERAGE: &str = "Leave the coverage out";
/// `E0509`'s Fix: a box only a `wall` target fills.
pub const FIX_DROP_WALL_BOX: &str = "Leave the box out";
/// E0601's Fix: accept the shortfall, so the beacons past the supply stay
/// dormant.
pub const FIX_ALLOW_DORMANT: &str = "Allow dormant beacons";

// S2's Defend and Attack (task `vfy2`).

/// E0414's `{side}` for one of the seat's own beacons.
pub const SIDE_OWN: &str = "one of your own beacons";
/// E0414's `{side}` for an enemy beacon.
pub const SIDE_ENEMY: &str = "an enemy beacon";
/// E0414's and E0415's `{role}` for a protect-list entry.
pub const ROLE_PROTECT: &str = "a Defend beacon's protect list";
/// E0414's and E0415's `{role}` for an Attack target.
pub const ROLE_ATTACK_TARGET: &str = "an Attack target";
/// E0414's `{wanted}` under a protect list.
pub const WANTED_OWN: &str = "your own beacons";
/// E0414's `{wanted}` under an Attack target.
pub const WANTED_ENEMY: &str = "enemy beacons";
/// E0415's `{found}` and `{wanted}`: the seat's own beacons (`OWN`, or a side
/// left unset, which reads as `OWN`).
pub const RANKS_OWN: &str = "your own beacons (`side: OWN`, or no side)";
/// E0415's `{found}` and `{wanted}`: the beacons the seat knows of an enemy.
pub const RANKS_ENEMY: &str = "known enemy beacons (`side: ENEMY_KNOWN`)";
/// E0415's `{found}` for `safest`, which ranks only the seat's own.
pub const RANKS_SAFEST: &str = "your own beacons by safety (`safest`)";
/// E0510's `{range}` for a percentage that cannot be 0.
pub const RANGE_ONE_TO_HUNDRED: &str = "from 1 to 100";
/// E0510's `{range}` for a count of at least one.
pub const RANGE_ONE_OR_MORE: &str = "1 or more";
/// E0417's `{found}` when the anchor is a voxel outside the box.
pub const ANCHOR_OUTSIDE_BOX: &str = "this anchor is a voxel outside the box";
/// E0417's `{found}` when the anchor is not a voxel at all.
pub const ANCHOR_NOT_A_VOXEL: &str = "this anchor is not a voxel";
/// E0416's `{what}` for a wall box's corner.
pub const WALL_BOX_COLUMN: &str = "the wall box's";
/// E0416's `{what}` for a Build target's anchor.
pub const ANCHOR_COLUMN: &str = "the anchor's";
/// E0418's `{what}`: no enemy commander has been sighted.
pub const NO_COMMANDER_SIGHTED: &str = "the seat has no sighting of an enemy commander";
/// W0502's `{name}` for a repeated `my_commander`.
pub const MY_COMMANDER: &str = "my_commander";
/// E0101's `{what}` for a launch with no condition.
pub const A_LAUNCH_CONDITION: &str = "a launch condition (`force_size` or `segment_ms`)";
/// E0511's Fix.
pub const FIX_DROP_ROTATION: &str = "Leave the rotation out";

/// E0418's `{what}` for a beacon the view holds no sighting of.
#[must_use]
pub fn never_sighted(beacon_id: &str) -> String {
    format!("the seat has no sighting of `{beacon_id}`")
}

/// E0418's `{what}` for a sighting older than the window.
#[must_use]
pub fn sighted_ago(what: &str, age_ms: u32) -> String {
    format!("{what} was last seen {age_ms} ms ago")
}

/// The enemy commander, as E0418's `{what}` names it.
pub const THE_ENEMY_COMMANDER: &str = "the enemy commander";

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
    // Split at the braces themselves rather than slicing by index, so there is
    // no slice that could fall outside the text and nothing to fall back on.
    while let Some((head, tail)) = rest.split_once('{') {
        out.push_str(head);
        let Some((name, after)) = tail.split_once('}') else {
            out.push('{');
            out.push_str(tail);
            return out;
        };
        if let Some((_, value)) = args.iter().find(|(key, _)| *key == name) {
            out.push_str(value);
        } else {
            out.push('{');
            out.push_str(name);
            out.push('}');
        }
        rest = after;
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

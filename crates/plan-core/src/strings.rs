// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The string table's `plan-core` section: every English word `render_plan`
//! can put in front of a player, in one place.
//!
//! **PLACEHOLDER — the one project-wide string table.** Spec section 11 says
//! every user-facing string in v1 is English and lives in *one* table: the
//! user interface, the diagnostic catalogue, and "the plain-language playbook
//! rendering", which is this crate's. `pharmakos-verifier`'s `strings` module
//! carries the same note and the same PLACEHOLDER. Who assembles the single
//! table, where it lives and what its wording is settles with the editor at
//! **S6** and is the **owner's** call; the skeleton plan carries this
//! PLACEHOLDER against T8 by name. Until then a phrase is written here once
//! and read by everyone from here, so there is one wording to move rather than
//! forty call sites to find.
//!
//! # Why the wording is a golden
//!
//! `tests/golden/plan-core/expand_east/expected.prose.txt` byte-compares the
//! rendering. Its README says what a diff means: "Either a template string
//! changed (say so; it is the one the player reads) or the plan itself renders
//! differently, which is a semantic change wearing a typographic disguise."
//! Changing a constant in this file is the first of those, and it is a
//! one-line explanation in the pull request rather than a mystery.
//!
//! # What these sentences may claim
//!
//! Only what the file and the rules table say. Decisions-log item 97 is
//! explicit that T8 "claims nothing in `render_plan` that a scenario would
//! have to assert differently later", so the prose states the playbook's own
//! content, the interface-time arithmetic, and the size meter — and says
//! nothing about travel, arrival, outcomes, or the segment's length, none of
//! which is arithmetic over the file.

// --- Headings ---------------------------------------------------------------

/// The route section.
pub const HEADING_ROUTE: &str = "Route";
/// The handlers section. "Rules" is the player-facing word (spec section 13's
/// rule list); `handlers` is the schema's.
pub const HEADING_RULES: &str = "Rules";
/// The `on_death` block.
pub const HEADING_ON_DEATH: &str = "If the commander dies";
/// The `fallback` block.
pub const HEADING_FALLBACK: &str = "Fallback";
/// The per-playbook switches.
pub const HEADING_OPTIONS: &str = "Options";
/// The size meter and the interface-time total.
pub const HEADING_BUDGET: &str = "Budget";

// --- The envelope -----------------------------------------------------------

/// Shown when a playbook has no title.
pub const UNTITLED: &str = "Untitled playbook";
/// `meta.author_kind` = HUMAN.
pub const AUTHOR_HUMAN: &str = "Written by hand.";
/// `meta.author_kind` = BUILTIN.
pub const AUTHOR_BUILTIN: &str = "Filed by the built-in operator.";
/// `meta.author_kind` = SCRIPT, which the verifier rejects in v1.
pub const AUTHOR_SCRIPT: &str = "Written by a script, which v1 does not accept.";
/// `meta.author_kind` unset, which the verifier rejects.
pub const AUTHOR_UNKNOWN: &str = "Written by nobody the file names.";
/// `kind` = TEMPLATE.
pub const KIND_TEMPLATE: &str = "A template, not a playbook: instantiate it before sealing it.";
/// `kind` = SAMPLE.
pub const KIND_SAMPLE: &str = "A sample, shipped to be read and copied.";

// --- Steps ------------------------------------------------------------------

/// A step whose `kind` oneof is unset.
pub const STEP_NOTHING: &str = "do nothing, which is not a step the verifier accepts";
/// `move`.
pub const STEP_MOVE: &str = "walk to";
/// `move` with `pace` = DIRECT.
pub const PACE_DIRECT: &str = ", directly";
/// `move` with `pace` = `AVOID_KNOWN_THREATS`.
pub const PACE_AVOID: &str = ", avoiding known threats";
/// `interface`.
pub const STEP_INTERFACE: &str = "interface with";
/// `place_beacon`.
pub const STEP_PLACE_BEACON: &str = "place a beacon at";
/// `wait_until`.
pub const STEP_WAIT_UNTIL: &str = "wait until";
/// `hold`.
pub const STEP_HOLD: &str = "hold for";
/// `broadcast`, which needs a Radio Mast and arrives at S4.
pub const STEP_BROADCAST: &str = "broadcast over the radio";

// --- Guards -----------------------------------------------------------------

/// `skip_if`.
pub const GUARD_SKIP_IF: &str = "Skip this step if";
/// `timeout_ms`.
pub const GUARD_TIMEOUT: &str = "Give up after";
/// `on_fail` = SKIP.
pub const ON_FAIL_SKIP: &str = "On failure, move on to the next step.";
/// `on_fail` = `ABORT_ROUTE`.
pub const ON_FAIL_ABORT: &str = "On failure, abandon the route.";
/// `on_fail` = `JUMP_FORWARD`.
pub const ON_FAIL_JUMP: &str = "On failure, jump forward to";

// --- Interface rows ---------------------------------------------------------

/// `set_mandate`.
pub const ROW_SET_MANDATE: &str = "switch it to a";
/// `set_mandate_settings`.
pub const ROW_SET_SETTINGS: &str = "change its mandate settings";
/// `set_priority`.
pub const ROW_SET_PRIORITY: &str = "set its Quartermaster priority to";
/// `recycle`.
pub const ROW_RECYCLE: &str = "recycle it";
/// `add_build_target`.
pub const ROW_ADD_TARGET: &str = "add a build target:";
/// `remove_build_target`.
pub const ROW_REMOVE_TARGET: &str = "remove the build target at";
/// `queue_structure`.
pub const ROW_QUEUE: &str = "queue a";
/// A row whose oneof is unset.
pub const ROW_NOTHING: &str = "change nothing, which is not a row the verifier accepts";

// --- Places and selectors ---------------------------------------------------

/// A fixed voxel.
pub const PLACE_VOXEL: &str = "voxel";
/// `Location` with no `place` set.
pub const PLACE_NOWHERE: &str = "nowhere the file names";
/// `BeaconRef.safest`, and `Location.safest`.
pub const BEACON_SAFEST: &str = "the safest own beacon";
/// `BeaconRef.nearest`.
pub const BEACON_NEAREST: &str = "the nearest";
/// `BeaconRef.weakest`.
pub const BEACON_WEAKEST: &str = "the weakest";
/// `BeaconRef.most_threatened`.
pub const BEACON_MOST_THREATENED: &str = "the most threatened";
/// A selector's own beacons.
pub const SIDE_OWN: &str = "own";
/// A selector's known enemy beacons.
pub const SIDE_ENEMY: &str = "known enemy";
/// The noun a selector ends on.
pub const BEACON_NOUN: &str = "beacon";
/// A selector that filters on tags.
pub const TAGGED: &str = "tagged";
/// `BeaconRef` with no `ref` set.
pub const BEACON_NONE: &str = "no beacon the file names";
/// Reminds the reader when a selector resolves (spec section 13's chip text).
pub const SELECTOR_NOTE: &str = "resolves when the step starts";

// --- Conditions -------------------------------------------------------------

/// `all`.
pub const AND: &str = " and ";
/// `any`.
pub const OR: &str = " or ";
/// `not`.
pub const NOT: &str = "it is not the case that";
/// An empty `all`/`any` group, which the verifier rejects.
pub const EMPTY_GROUP: &str = "nothing";
/// A `Condition` with no node set.
pub const CONDITION_NOTHING: &str = "nothing the file names";
/// `LT`.
pub const CMP_LT: &str = "below";
/// `LE`.
pub const CMP_LE: &str = "at or below";
/// `EQ`.
pub const CMP_EQ: &str = "exactly";
/// `NE`.
pub const CMP_NE: &str = "anything but";
/// `GE`.
pub const CMP_GE: &str = "at or above";
/// `GT`.
pub const CMP_GT: &str = "above";
/// An unset comparison operator, which the verifier rejects.
pub const CMP_NONE: &str = "compared somehow to";

// --- on_death, fallback, options -------------------------------------------

/// `on_respawn` = CONTINUE.
pub const RESPAWN_CONTINUE: &str = "On respawn, carry on from the step the route was on.";
/// `on_respawn` unset.
pub const RESPAWN_UNSET: &str = "On respawn, do what the file does not say.";
/// `fallback.hold`.
pub const FALLBACK_HOLD: &str = "Hold at";
/// `fallback.shadow`.
pub const FALLBACK_SHADOW: &str = "Stay with";
/// `fallback.patrol`.
pub const FALLBACK_PATROL: &str = "Patrol";
/// `fallback` with no posture, which the verifier rejects.
pub const FALLBACK_NONE: &str = "No fallback, which the verifier rejects.";
/// `options.allow_dormant_beacons`.
pub const ALLOW_DORMANT: &str =
    "Beacons may go dormant: a power shortfall is a warning, not an error.";

// --- Travel -----------------------------------------------------------------

/// A fogged leg, and any total that contains one. Item 61: never an ETA.
pub const AT_MOST: &str = "at most";
/// The connectivity oracle's answer.
pub const NO_ROUTE: &str = "no route the seat knows of";

// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The string table's `plan-core` section: every English word `render_plan`
//! can put in front of a player, in one place.
//!
//! PLACEHOLDER: the one project-wide string table — owner, S6, with the editor.
//!
//! Spec section 11 says
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
/// `meta.author_kind` = BUILTIN: every playbook instantiated from the
/// library, the player's included, and the safe playbook the gateway files.
/// It used to read "Filed by the built-in operator.", which showed on a
/// playbook the player chose and edited (the demo's F5); decision 8 of S1's
/// plan, ruled by item 128, is this sentence, true of Easy's too.
pub const AUTHOR_BUILTIN: &str = "Written from a built-in template.";
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
pub const ROW_SET_MANDATE: &str = "switch it to";
/// `set_mandate_settings`.
pub const ROW_SET_SETTINGS: &str = "change its mandate settings";
/// `set_priority`.
pub const ROW_SET_PRIORITY: &str = "set its Quartermaster priority to";
/// `recycle`.
pub const ROW_RECYCLE: &str = "recycle it";
/// `add_build_target`.
pub const ROW_ADD_TARGET: &str = "add a build target:";
/// A `wall` target's box, before its first corner (`BuildTarget.wall_box`).
pub const WALL_BOX_FROM: &str = "filling the box from";
/// A `wall` target's box, before its second corner.
pub const WALL_BOX_TO: &str = "to";
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
/// The same for an Attack target's selector, which the sim reads when the
/// force is ready to launch (S2's `atk2`).
pub const ATTACK_SELECTOR_NOTE: &str = "chosen when the force launches";

// --- Targeting's sites and features -----------------------------------------
//
// docs/design/targeting.md, "Names", "Descriptions (S1)" and "Sites". These
// replace the two placeholder phrases S1's targeting proto rendered the arms
// with ("... (not in this build yet)"), which lived at the call site until S1's
// targeting verifier (task `tgtv`) moved them here.

/// `Location.covering`: "a site covering the nearest heat vent ...".
pub const SITE_COVERING: &str = "a site covering";
/// `Location.toward`: "a site toward the nearest heat vent ...".
pub const SITE_TOWARD: &str = "a site toward";
/// `Location.on`: "a spot on Heat vent (120, 88)".
pub const SITE_ON: &str = "a spot on";
/// A vent's display name, before its anchor: "Heat vent (120, 88)".
pub const VENT_NAME: &str = "Heat vent";
/// A seam's display name, before its anchor.
pub const SEAM_NAME: &str = "Scrap seam";
/// A vent in a pick: "the nearest heat vent".
pub const VENT_NOUN: &str = "heat vent";
/// A seam in a pick.
pub const SEAM_NOUN: &str = "scrap seam";
/// A `feature_id` that is not `vent_<x>_<y>` or `seam_<x>_<y>`, quoted after.
pub const FEATURE_NOUN: &str = "feature";
/// `FeatureRef.covered`.
pub const FEATURE_COVERED: &str = "the feature this beacon was placed to cover";
/// A `FeatureRef` with no arm set, which the verifier rejects.
pub const FEATURE_NONE: &str = "no feature the file names";
/// A pick whose rank is unset, which the verifier rejects.
pub const RANK_UNSET: &str = "an unranked";
/// `FeatureRef.Coverage.UNCOVERED`: outside every sphere of the seat's own
/// living beacons.
pub const NOT_COVERED: &str = " you do not cover yet";
/// Reminds the reader how and when a pick reads (targeting.md's chip:
/// "nearest by travel ..., read when the step starts").
pub const PICK_NOTE: &str = "nearest by travel, read when the step starts";

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

// --- Mandates ---------------------------------------------------------------
//
// The five mandate names, written once. They are proper nouns in the design
// docs and in the spec's section 6, so they are capitalised here and nowhere
// else, and the beacon selector and the mandate-settings row read the same
// five words rather than spelling their own.

/// `gp.v1.MandateSettings.build` and `gp.v1.BeaconFilter.mandate = BUILD`.
pub const MANDATE_BUILD: &str = "Build";
/// `DEFEND`.
pub const MANDATE_DEFEND: &str = "Defend";
/// `ATTACK`.
pub const MANDATE_ATTACK: &str = "Attack";
/// `SURVEY`.
pub const MANDATE_SURVEY: &str = "Survey";
/// `MINE`.
pub const MANDATE_MINE: &str = "Mine";
/// A `mandate_settings` with no arm set, which the verifier rejects.
pub const MANDATE_NONE: &str = "no mandate the file names";

// --- Quartermaster priority -------------------------------------------------

/// `QUARTERMASTER_PRIORITY_LOW`.
pub const PRIORITY_LOW: &str = "low";
/// `QUARTERMASTER_PRIORITY_NORMAL`.
pub const PRIORITY_NORMAL: &str = "normal";
/// `QUARTERMASTER_PRIORITY_HIGH`.
pub const PRIORITY_HIGH: &str = "high";
/// The priority left unspecified, which the verifier rejects.
pub const PRIORITY_UNSET: &str = "unset";

// --- Conditions -------------------------------------------------------------

/// The unit `cmdr_deaths` counts in.
pub const DEATHS_UNIT: &str = "times";
/// `beacon_powered` true.
pub const POWERED: &str = "powered";
/// `beacon_powered` false.
pub const DORMANT: &str = "dormant";
/// The unit `enemy_units_near` counts in.
pub const ENEMY_UNITS_UNIT: &str = "enemy units";
/// `enemy_structure_near`'s `any_kind`.
pub const ANY_STRUCTURE: &str = "structure";
/// `enemy_structure_near` with neither a blueprint nor `any_kind`, which is
/// refused.
pub const STRUCTURE_NONE: &str = "structure of no kind the file names";
/// `beacon_units_by_role`'s `BUILD_DRONE`.
pub const ROLE_BUILD_DRONE: &str = "build drones";
/// `beacon_units_by_role`'s `MINING_DRONE`.
pub const ROLE_MINING_DRONE: &str = "mining drones";
/// `beacon_units_by_role`'s `REPAIR_DRONE`.
pub const ROLE_REPAIR_DRONE: &str = "repair drones";
/// `beacon_units_by_role`'s `RAIDER`.
pub const ROLE_RAIDER: &str = "raiders";
/// `beacon_units_by_role`'s `SCOUT`.
pub const ROLE_SCOUT: &str = "scouts";
/// A role left unspecified, which S3's verifier rejects.
pub const ROLE_NONE: &str = "units of no role the file names";
/// `Freshness.VISIBLE_NOW`.
pub const FRESH_NOW: &str = "in sight now";
/// `Freshness.KNOWN_WITHIN`, before its window: "seen in the last 30 s".
pub const FRESH_WITHIN: &str = "seen in the last";
/// A freshness left out, which `E0209` rejects.
pub const FRESH_NONE: &str = "at no freshness the file names";

// --- S2's Defend and Attack settings (task `vfy2`) ---------------------------

/// Before a Defend arm's protect list.
pub const DEFEND_PROTECTING: &str = "protecting";
/// A protect-list entry naming the seat's own commander.
pub const DEFEND_MY_COMMANDER: &str = "your commander";
/// A protect-list entry with no asset.
pub const DEFEND_NOTHING: &str = "nothing the file names";
/// Before the engagement radius.
pub const DEFEND_ENGAGING: &str = "engaging within";
/// After the engagement radius's percentage.
pub const DEFEND_OF_SPHERE: &str = "of the sphere";
/// `pursue: NEVER`.
pub const PURSUE_NEVER: &str = "never pursuing";
/// `pursue: SPHERE_EDGE`.
pub const PURSUE_SPHERE_EDGE: &str = "pursuing to the sphere edge";
/// `pursue: WHILE_IN_REACH`.
pub const PURSUE_WHILE_IN_REACH: &str = "pursuing while in reach";
/// A pursue value the file does not set.
pub const PURSUE_UNSET: &str = "pursuing as the file does not say";
/// Before a picket's scout count.
pub const DEFEND_PICKET: &str = "with a picket of";
/// After a picket's scout count.
pub const DEFEND_PICKET_UNIT: &str = "scouts";
/// Before an Attack target.
pub const ATTACK_TARGETING: &str = "attacking";
/// `enemy_commander`.
pub const ATTACK_ENEMY_COMMANDER: &str = "the enemy commander";
/// An Attack target with no arm.
pub const ATTACK_NOTHING: &str = "nothing the file names";
/// `retarget: NEAREST_KNOWN`.
pub const RETARGET_NEAREST: &str = "retargeting the nearest known enemy beacon";
/// `retarget: NONE`.
pub const RETARGET_NONE: &str = "keeping its target";
/// A retarget value the file does not set.
pub const RETARGET_UNSET: &str = "retargeting as the file does not say";
/// Before the staging point.
pub const ATTACK_STAGING: &str = "gathering at";
/// Before the launch conditions.
pub const ATTACK_LAUNCH: &str = "launching once";
/// A launch's force size, before the count.
pub const LAUNCH_FORCE: &str = "raiders stand there";
/// A launch's segment time, after the duration.
pub const LAUNCH_SEGMENT: &str = "of the segment have passed";
/// A launch with no condition.
pub const LAUNCH_NEVER: &str = "never, for no launch condition is written";
/// Before the retreat conditions.
pub const ATTACK_RETREAT: &str = "breaking off once";
/// A retreat's force threshold, before the percentage.
pub const RETREAT_FORCE: &str = "fewer than";
/// A retreat's force threshold, after the percentage.
pub const RETREAT_FORCE_UNIT: &str = "of the force remain";
/// A retreat's time, after the duration.
pub const RETREAT_TIME: &str = "after launch";
/// `after_action: REGROUP_AT_STAGING`.
pub const AFTER_REGROUP: &str = "then regrouping at the staging point";
/// `after_action: RETURN_HOME`.
pub const AFTER_HOME: &str = "then returning home";
/// An after-action the file does not set.
pub const AFTER_UNSET: &str = "then doing what the file does not say";
/// Before the count of avoid areas.
pub const ATTACK_AVOIDING: &str = "avoiding";
/// One avoid area.
pub const AREA_ONE: &str = "area";
/// More than one avoid area.
pub const AREA_MANY: &str = "areas";
/// `breach_allowed: true`.
pub const BREACH_ALLOWED: &str = "breaching walls if it must";
/// `breach_allowed: false`.
pub const BREACH_REFUSED: &str = "never breaching walls";
/// One wave.
pub const WAVE_ONE: &str = "wave";
/// More than one wave.
pub const WAVE_MANY: &str = "waves";
/// Before the count of waves.
pub const WAVES_IN: &str = "in";
/// The article before a writ's name that begins with a consonant sound.
pub const ARTICLE_A: &str = "a";
/// The article before "Attack".
pub const ARTICLE_AN: &str = "an";

// ---------------------------------------------------------------------------
// The vocabulary (`get_vocabulary`; S3's plan, E8)
// ---------------------------------------------------------------------------

/// A lock's lead before the stage: "from S6".
pub const VOCABULARY_FROM: &str = "from";
/// The Resonance Spire's lock while the seat holds no licence.
pub const VOCABULARY_NEEDS_SPIRE_LICENCE: &str = "needs the Resonance Spire licence, which the Ledger awards at a settlement where your \
     Generators run on two vents";

/// The clock family.
pub const FAMILY_CLOCK: &str = "Clock";
/// The commander family.
pub const FAMILY_COMMANDER: &str = "Commander";
/// The own-beacon family.
pub const FAMILY_OWN_BEACON: &str = "Own beacon";
/// The aggregates family.
pub const FAMILY_AGGREGATES: &str = "Aggregates";
/// The enemy-knowledge family.
pub const FAMILY_ENEMY_KNOWLEDGE: &str = "Enemy knowledge";
/// The economy family.
pub const FAMILY_ECONOMY: &str = "Economy";
/// The capability family.
pub const FAMILY_CAPABILITY: &str = "Capability licence and structure state";
/// The mast-coverage family.
pub const FAMILY_MAST_COVERAGE: &str = "Mast coverage";
/// The playbook-state family.
pub const FAMILY_PLAYBOOK_STATE: &str = "Playbook state";

/// `segment_elapsed`.
pub const PREDICATE_SEGMENT_ELAPSED: &str = "Time into the segment";
/// `cmdr_hp_pct`.
pub const PREDICATE_CMDR_HP_PCT: &str = "Commander's HP";
/// `cmdr_took_damage_within`.
pub const PREDICATE_CMDR_TOOK_DAMAGE: &str = "Commander took damage recently";
/// `cmdr_deaths`.
pub const PREDICATE_CMDR_DEATHS: &str = "Commander's deaths";
/// `beacon_hp_pct`.
pub const PREDICATE_BEACON_HP_PCT: &str = "A beacon's HP";
/// `beacon_under_attack`.
pub const PREDICATE_BEACON_UNDER_ATTACK: &str = "A beacon under attack";
/// `beacon_powered`.
pub const PREDICATE_BEACON_POWERED: &str = "A beacon powered or dormant";
/// `beacon_units_by_role`.
pub const PREDICATE_BEACON_UNITS_BY_ROLE: &str = "A beacon's units by role";
/// `beacon_mandate`.
pub const PREDICATE_BEACON_MANDATE: &str = "A beacon's mandate";
/// `beacon_has_structure`.
pub const PREDICATE_BEACON_HAS_STRUCTURE: &str = "A beacon's finished structure";
/// `enemy_units_near`.
pub const PREDICATE_ENEMY_UNITS_NEAR: &str = "Enemy units near";
/// `enemy_beacon_known`.
pub const PREDICATE_ENEMY_BEACON_KNOWN: &str = "An enemy beacon known";
/// `enemy_structure_near`.
pub const PREDICATE_ENEMY_STRUCTURE_NEAR: &str = "An enemy structure near";
/// `treasury`.
pub const PREDICATE_TREASURY: &str = "Treasury";
/// `kw_headroom`.
pub const PREDICATE_KW_HEADROOM: &str = "Grid headroom";
/// `kw_supply`.
pub const PREDICATE_KW_SUPPLY: &str = "Grid supply";
/// `kw_draw`.
pub const PREDICATE_KW_DRAW: &str = "Grid draw";
/// `step_reached`.
pub const PREDICATE_STEP_REACHED: &str = "The route reached a step";
/// `rule_fired`.
pub const PREDICATE_RULE_FIRED: &str = "A rule fired";

/// The clock family's reserved rest.
pub const RESERVED_CLOCK: &str = "The rest of the clock";
/// The commander family's reserved rest.
pub const RESERVED_COMMANDER: &str = "Commander's position";
/// The own-beacon family's reserved rest.
pub const RESERVED_OWN_BEACON: &str = "A beacon's reports since";
/// The enemy-knowledge family's reserved rest.
pub const RESERVED_ENEMY_KNOWLEDGE: &str = "The rest of enemy knowledge";
/// The economy family's reserved rest.
pub const RESERVED_ECONOMY: &str = "Income forecast";
/// The playbook-state family's reserved rest.
pub const RESERVED_PLAYBOOK_STATE: &str = "Posture";

/// A comparison's operator.
pub const VOCABULARY_COMPARED: &str = "Compared";
/// A duration.
pub const VOCABULARY_TIME: &str = "Time";
/// A recent window.
pub const VOCABULARY_WITHIN: &str = "Within";
/// A percentage.
pub const VOCABULARY_PERCENT: &str = "Percent";
/// A count.
pub const VOCABULARY_COUNT: &str = "Count";
/// A beacon.
pub const VOCABULARY_BEACON: &str = "Beacon";
/// Powered or dormant.
pub const VOCABULARY_POWERED: &str = "Powered";
/// A unit role.
pub const VOCABULARY_ROLE: &str = "Role";
/// A mandate.
pub const VOCABULARY_MANDATE: &str = "Mandate";
/// The place an enemy predicate measures from.
pub const VOCABULARY_NEAR: &str = "Near";
/// A radius.
pub const VOCABULARY_RADIUS: &str = "Radius";
/// A freshness kind.
pub const VOCABULARY_FRESHNESS: &str = "Freshness";
/// A freshness window.
pub const VOCABULARY_WINDOW: &str = "Seen within";
/// An amount of money.
pub const VOCABULARY_AMOUNT: &str = "Amount";
/// An amount of power.
pub const VOCABULARY_POWER: &str = "Power";
/// A step label.
pub const VOCABULARY_STEP: &str = "Step";
/// A rule id.
pub const VOCABULARY_RULE: &str = "Rule";
/// The rules of engagement every writ shares.
pub const VOCABULARY_ROE: &str = "Rules of engagement";
/// The retreat threshold every writ shares.
pub const VOCABULARY_RETREAT_HP: &str = "Retreat below HP";
/// Build's repair threshold.
pub const VOCABULARY_REPAIR: &str = "Repair below";
/// Build's rebuild flag.
pub const VOCABULARY_REBUILD: &str = "Rebuild destroyed";
/// Build's terraform setting.
pub const VOCABULARY_TERRAFORM: &str = "Terraform";
/// Defend's engagement radius.
pub const VOCABULARY_ENGAGEMENT: &str = "Engagement radius";
/// Defend's pursue setting.
pub const VOCABULARY_PURSUE: &str = "Pursue";
/// Defend's picket.
pub const VOCABULARY_PICKET: &str = "Picket scouts";
/// Attack's target.
pub const VOCABULARY_TARGET: &str = "Target";
/// Attack's retarget rule.
pub const VOCABULARY_RETARGET: &str = "Retarget";
/// Attack's launch force.
pub const VOCABULARY_FORCE: &str = "Launch at force";
/// Attack's launch time.
pub const VOCABULARY_LAUNCH: &str = "Launch at time";
/// Attack's retreat force.
pub const VOCABULARY_RETREAT_FORCE: &str = "Break off below";
/// Attack's retreat time.
pub const VOCABULARY_RETREAT_TIME: &str = "Break off after";
/// Attack's after-action.
pub const VOCABULARY_AFTER: &str = "After the attack";
/// Attack's breach permission.
pub const VOCABULARY_BREACH: &str = "Breach walls";
/// Attack's waves.
pub const VOCABULARY_WAVES: &str = "Waves";
/// Survey's scouts.
pub const VOCABULARY_SCOUTS: &str = "Scouts";
/// Mine's dig depth.
pub const VOCABULARY_DIG: &str = "Dig depth";
/// Mine's flight.
pub const VOCABULARY_FLEE: &str = "Flee on threat";
/// Mine's seam choice.
pub const VOCABULARY_SEAM: &str = "Seam";
/// Mine's pillars.
pub const VOCABULARY_PILLARS: &str = "Pillar spacing";
/// A move's destination.
pub const VOCABULARY_TO: &str = "To";
/// A move's pace.
pub const VOCABULARY_PACE: &str = "Pace";
/// A placement's site.
pub const VOCABULARY_AT: &str = "At";

/// The `move` step.
pub const STEP_TITLE_MOVE: &str = "Walk";
/// The `interface` step.
pub const STEP_TITLE_INTERFACE: &str = "Interface with a beacon";
/// The `place_beacon` step.
pub const STEP_TITLE_PLACE_BEACON: &str = "Place a beacon";
/// The `wait_until` step.
pub const STEP_TITLE_WAIT_UNTIL: &str = "Wait until";
/// The `hold` step.
pub const STEP_TITLE_HOLD: &str = "Hold";
/// The `broadcast` step.
pub const STEP_TITLE_BROADCAST: &str = "Broadcast";

/// A blueprint parameter.
pub const VOCABULARY_STRUCTURE: &str = "Structure";

/// The Generator.
pub const BLUEPRINT_GENERATOR: &str = "Generator";
/// The autocannon.
pub const BLUEPRINT_AUTOCANNON: &str = "Autocannon";
/// The mortar.
pub const BLUEPRINT_MORTAR: &str = "Mortar";
/// The Survey post.
pub const BLUEPRINT_SURVEY_POST: &str = "Survey post";
/// The Resonance Spire.
pub const BLUEPRINT_RESONANCE_SPIRE: &str = "Resonance Spire";
/// A wall.
pub const BLUEPRINT_WALL: &str = "Wall";

/// A rendering whose line, character or chip count passed what it is
/// counted in (E22): refused rather than saturated.
pub const RENDER_COUNT_PAST_RANGE: &str =
    "the rendering's line, character or chip count passed what it is counted in";
/// A rules-table bound past what a chip's `int32` bound holds (E22).
pub const RENDER_BOUND_PAST_RANGE: &str =
    "the rules table bounds a value past what a chip's bound holds";

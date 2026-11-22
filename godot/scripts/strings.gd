# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The client's string table: every sentence the lobby and the editor show that the
# gateway did not write.
#
# Spec section 12: "Every user-facing string in v1 lives in one string table; translation
# is roadmap." What the verifier and the gateway say - a diagnostic's sentence, a beacon's
# description, a refusal - arrives already written, from their half of that table, and is
# shown as it came. What is left for the client is its own chrome: button labels, headings,
# and the frames the gateway's words and numbers are put into. Those live here and nowhere
# else, so a script says `Strings.text("save")` rather than "Save".
#
# A frame is filled with String.format, never with arithmetic: a number the gateway sent is
# put in as it came (a travel time is game milliseconds, spelt as such), because converting
# it would be time maths of the editor's own (AGENTS.md section 3 rule 4;
# `crates/client-gdext/tests/no_arithmetic.rs`).
#
# PLACEHOLDER: the string table's file and format, and every wording below — OWNER, at S6.
# With the one English string table (skeleton plan T19, PLACEHOLDERs).

extends RefCounted

const TEXT := {
	# --- The lobby ---------------------------------------------------------------
	"lobby_starting": "Starting the match host...",
	"lobby_connected": "Connected. Waiting for the view...",
	"lobby_failed": "The match host failed: {reason}",
	"lobby_status": "Round {round} - {phase}",
	"lobby_skipping": "(skipping)",
	"lobby_all_ready": "all ready",
	"lobby_speed": "speed {speed}x",
	"lobby_skip": "Skip",
	"lobby_ready": "Ready — end the Lull",
	# Under the button, in a Lull (decisions-log item 143 (4)). Ready ends the Lull once every
	# seat is ready, and with nothing sealed the gateway files the safe playbook.
	"lobby_ready_help": "With nothing sealed, Ready files the safe playbook for you.",
	"lobby_continue": "Continue",
	"lobby_follow": "Follow",
	"lobby_whole_map": "Whole map",
	"lobby_speed_button": "{speed}x",
	"lobby_new_match": "New match",
	"lobby_resume": "Resume last match",
	"lobby_choose": "Start a new match, or go on with the last one.",
	"lobby_resumed_failed": "The match host would not resume the last match: {reason}",
	"lobby_forgotten": "The last match has ended, so there is nothing to resume.",
	"lobby_about": "Credits",
	# The recap: the gateway's own prose - the settlement, the shortfall and why a step
	# found nothing are its sentences (S1's plan, task `ui`) - under this heading.
	"lobby_recap": "Recap of round {round}: {prose}",
	# A refused get_recap: the gateway's refusal, code and message, as it came.
	"lobby_recap_refused": "The recap could not be read: {refusal}",
	# Every seat's tally once the match is decided (item 145 (7)), one frame a seat, each number
	# as the gateway counted it.
	"lobby_recap_tallies": "Every seat's tally:",
	"lobby_recap_tally": "{seat}: destroyed {kills}, {shared_kills} of them shared, worth ${value}; lost {losses}.",

	# --- The credits overlay (scripts/credits.gd; decisions-log item 117 (11)) ------
	# The game's licences by area are written here, never read from files at run time.
	"about_back": "Back",
	"about_lockup": "PHARMAKOS: THE SEALED ORDER",
	"about_licences_heading": "Licences",
	"about_licence_game": "The game - its scripts, the client library and gamectl: GPL-3.0-or-later.",
	"about_licence_data": "The rules table (rules/) and the template library (library/): MIT OR Apache-2.0.",
	"about_licence_art": "Art and audio: CC-BY-SA-4.0.",
	"about_notices_file": "The notices of the Rust crates compiled into the client library and gamectl are in THIRD-PARTY-NOTICES.txt beside the game.",
	"about_attribution_heading": "Attributions (CC BY)",
	"about_attribution_none": "None yet: this build ships no third-party art or audio.",
	"about_godot_heading": "Godot Engine",
	"about_godot_intro": "This game runs on the Godot Engine. Its licence, and the notices of the third-party components it contains, follow as the engine reports them.",
	"about_godot_component": "{name}",
	"about_godot_part": "  {copyright} - {license}",
	"about_godot_licence": "--- {name} ---",

	# --- The editor's panel ------------------------------------------------------
	"editor_title": "Orders",
	"new": "New",
	"load": "Load...",
	"save": "Save",
	"save_as": "Save as...",
	"undo": "Undo",
	"submit": "Submit",
	"checks_heading": "Checks",
	"route_heading": "Route",
	"notes_heading": "Notebook",
	"notes_hint": "Your private notebook. Only you see it.",
	"save_notes": "Save notes",
	"drafts_heading": "Drafts",
	"save_draft": "Save draft",
	"draft_label": "Saved from the editor",
	"draft_row": "{label} (round {round})",
	"no_drafts": "No drafts yet.",
	"file_filter": "*.jsonc ; Playbooks",

	# --- The wizard, the rule list and the meter (T19, pull request 2) -------------
	# A value is shown and typed as the raw JSON the gateway wrote (a duration is game
	# milliseconds): unit display is S6's (skeleton-plan-w6-notes.md section A4 item 5).
	"templates_heading": "Start from a template",
	"no_templates": "No templates.",
	"template_button": "{title}",
	"wizard_heading": "Template: {title}",
	"wizard_waiting": "Asking the gateway...",
	"wizard_page_value": "{label}",
	"wizard_mark": "suggested by the built-in operator",
	"wizard_why": "Why: {why}",
	"wizard_send": "Send this value",
	"wizard_use": "Use this playbook",
	"wizard_close": "Close",
	"rules_heading": "What the playbook says",
	"rules_waiting": "Rendering...",
	# The rule list's chips, drag and connector (S3's `ed3b`). Every value is the gateway's as
	# sent; a refusal is its words.
	"rule_number_heading": "Change {text}:",
	"rule_number_send": "Set this value",
	# The gateway's bounds for the value, as it sent them ("-" where it set none): the field applies
	# none of them, and a value past them goes to the gateway, whose answer is drawn (S3's `fixp`).
	"rule_number_bounds": "The gateway's range here: {min} to {max}.",
	"rule_number_waiting": "Sending...",
	"rule_conflict": "Never fires while the rule joined above it is true: {sentence}",
	"rule_refused": "The gateway refused that edit: {detail}",
	"rule_chip_read_only_ms": "This duration is shown as the gateway wrote it. Editing it here waits on a unit the gateway can send.",
	"rule_chip_read_only_no_shape": "The gateway sent no way to edit this value.",
	# The file's value lies outside the gateway's bounds: shown as it is, never clamped (L1).
	"rule_number_outside": "The file holds {now}, outside the range the gateway gives here ({min} to {max}). It is shown as it is; the checks say what it costs.",
	# Why the rule list took no edit, by the bridge's reason (`RuleEditRefusal`), never in silence.
	"status_rule_refused_shut": "The rule list takes no edit while planning is closed.",
	"status_rule_refused_no_text": "Open a playbook first.",
	"status_rule_refused_stale": "The rule list is being redrawn for the text on screen; try again when it is.",
	"status_rule_refused_busy": "An edit is on its way; try again when it lands.",
	"status_rule_refused_changed": "The rule list was redrawn since you opened that value; open it again.",
	"status_rule_refused_no_such_chip": "That value is not in the rule list any more.",
	"status_rule_refused_not_this_editor": "That value is not edited that way.",
	"status_rule_refused_no_such_value": "That is not one of the values the gateway offers here.",
	"status_rule_refused_not_json": "The gateway offered a value this client cannot read.",
	"status_rule_refused_unchanged": "That is the value it already has: nothing was sent.",
	"status_rule_refused_no_such_rule": "That rule is not in the rule list any more.",
	"status_rule_refused_onto_itself": "A rule dropped on itself stays where it is.",
	"status_rule_refused_other_list": "A step moves among the route's steps and a rule among the rules, not from one list to the other.",
	"status_rule_refused_not_picking": "No value is being picked on the map.",
	"status_rule_refused_no_target": "That click named nothing on the map to pick; click again, or Escape.",
	"status_rule_refused_rendering_out_of_range": "The rule list cannot be edited until it is drawn again.",
	"status_rule_refused_not_this_kind": "That is not a {detail}. Click one on the map, or Escape.",
	"status_rule_refused_arm_not_here": "That cannot go here: the gateway takes no {detail} at this point. Click something else, or Escape.",
	"status_rule_refused_not_an_integer": "That is not a whole number (digits only, with a minus sign if it is negative), so nothing was sent.",
	"status_rule_refused_no_vocabulary": "The words to pick from have not come yet this Lull (or were refused); see the picker's line.",
	"status_rule_refused_not_in_rule": "That condition is not in the rule the picker was opened on.",
	"status_rule_refused_no_such_entry": "That is not in the list the gateway sent any more.",
	"status_rule_refused_locked": "That is locked: its reason is shown beside it.",
	"status_rule_refused_no_detail": "This beacon's detail has not come yet (or was refused); open its menu again.",
	"status_rule_refused_no_pointer": "The gateway sent a setting this client cannot place.",
	"status_rule_refused_no_such_slot": "That condition is not in the rule list any more.",
	"status_rule_refused_group_slot": "That condition is a group: pick one of its items instead.",
	"status_rule_refused_no_pick_op": "The gateway named no way to put a condition there, so nothing was sent.",
	"rule_slot": "Condition: {title}",
	"status_rule_refused_unnamed": "The gateway's answer named nothing to write for that entry, so nothing was sent.",
	"rule_chip_read_only_unreadable": "This value does not fit what the gateway says it holds, so it is shown as it came and not edited.",
	"picker_parameter_unknown": "{title}: the gateway sent no way to edit this value",
	"vocabulary_ask_again": "Ask again",
	"menu_setting_unreadable": "{said} ({cause})",
	"menu_setting_not_here": "This value is not changed from this menu.",
	# Part 2 (S3's `ed3b`): the condition picker, the name picks and Visit & change.
	"picker_heading": "Pick a condition",
	"picker_family_locked": "{title} (locked: {locked})",
	"picker_entry": "{title}",
	"picker_entry_locked": "{title} (locked: {locked})",
	"picker_parameter": "{title}",
	"picker_parameter_number": "{title}: a number{bounds}{unit}",
	"picker_parameter_bounds": " from {min} to {max}",
	"picker_parameter_ms": "{title}: a time, edited where the gateway can send its unit",
	"picker_parameter_choice": "{title}: one of {values}",
	"picker_parameter_reference": "{title}: picked ({kind})",
	"picker_use": "Use this condition",
	"vocabulary_waiting": "Reading the words to pick from...",
	"vocabulary_refused": "The gateway refused the words to pick from: {detail}",
	"vocabulary_unreadable": "The words to pick from could not be read: {detail}",
	"name_none": "The playbook names none to pick from.",
	"menu_settings": "Change a setting",
	"menu_setting": "{title}: {value}",
	"menu_setting_value": "{value}",
	"menu_mandate": "Switch mandate",
	"menu_add_target": "Add a build target (then click the ground)",
	"menu_remove_target": "Remove a build target",
	"menu_target_row": "{blueprint} at {anchor}",
	"menu_queue": "Queue a structure",
	"menu_entry_locked": "{title} (locked: {locked})",
	"menu_detail_waiting": "Reading this beacon...",
	"menu_detail_refused": "This beacon's detail: {detail}",
	"menu_detail_unreadable": "This beacon's detail could not be read: {detail}",
	"meter_heading": "Your economy",
	# The four numbers as the gateway sent them. Headroom is the gateway's own figure,
	# never supply minus draw: nothing here adds or subtracts anything.
	"meter": "${treasury}   supply {supply} kW   draw {draw} kW   headroom {headroom} kW",
	"meter_waiting": "Not read yet.",
	# The size meter (S3's plan, E9): the verifier's two numbers, as they came. Over budget the
	# panel shows the verifier's own sentence instead of this frame.
	"size_meter": "{units} of {budget} size units",
	"forecast_heading": "Your next settlement",
	# The next BMI is a prediction, and says so (decisions-log item 135 (2) (g)): the BMI at
	# the band held now, ignoring committed spend. Each line is left out when the answer
	# left its figure out, which is not 0.
	"meter_bmi": "Next BMI, at the band you hold now: ${bmi}. A prediction: it ignores the spend your orders have committed, and spending in the Push can move your band.",
	"meter_committed": "Committed by your sealed orders and not yet paid: up to ${committed}.",
	# The register's S1-31: the key-core is netted out of draw, and the meter says so.
	"meter_key_core": "Supply and draw leave out each beacon's own base draw, which its key-core supplies.",

	# --- What the checks said ----------------------------------------------------
	"verdict_none": "Not checked yet.",
	"verdict_quick": "Quick check",
	"verdict_full": "Full check",
	"verdict_submitted": "Checked at submission",
	"verdict_refused": "Load refused",
	"verdict_checking": "Checking...",
	# What follows the verdict, as the bridge's editor names it (`SealLine`): whether the
	# text can be sealed is said only in an open Lull (S1's stage demo, finding J).
	"seal_ready": "Ready to seal.",
	"seal_not_ready": "Cannot be sealed yet.",
	"seal_filed": "Not sealed: the safe playbook was filed for you instead.",
	# A check that will not come, in place of "Checking...", "Estimating...",
	# "Rendering..." and "Reading the map..." (finding K): the gateway refused it, with the
	# gateway's words, or the planning door is shut.
	"unchecked_refused": "Not checked: the gateway refused ({detail}).",
	# The gateway answered, and this client could not read the answer (M4).
	"unchecked_unreadable": "Not shown: this client could not read the gateway's answer ({detail}).",
	"unchecked_ready": "Not checked: after Ready this client sends no planning call for the rest of this Lull.",
	"unchecked_push": "Not checked: planning is closed during the Push.",
	"unchecked_recap": "Not checked: planning is closed during the recap.",
	"unchecked_ended": "Not checked: the match has ended.",
	"no_rows": "Nothing to fix.",
	# E0601's checkbox: the option is the player's choice, and the verifier's next report
	# says what it costs (E0601 becomes W0603).
	"dormant_option": "Allow dormant beacons: let this route draw more power than you supply, so some beacons go dark.",
	"severity_error": "Error",
	"severity_warning": "Warning",
	"severity_info": "Note",
	"row_where": "{code} at {pointer}",
	"fix": "Fix: {title}",

	# --- The status line: keys the bridge's editor names ---------------------------
	"status_": "",
	"status_load_not_text": "That file is not text, so it cannot be a playbook.",
	"status_load_refused": "Load refused: {detail}. Nothing was opened and nothing in the file was changed.",
	"status_load_refused_by_gateway": "The gateway would not check that file: {detail}",
	"status_loaded": "Opened.",
	"status_undone": "Undone.",
	"status_no_playbook": "Open a playbook first.",
	"status_gateway_refused": "The gateway refused: {detail}",
	"status_submitting": "Submitting...",
	"status_submitted": "Submitted. This is now your sealed order.",
	"status_fixing": "Fixing...",
	"status_fixed": "Fix applied.",
	"status_submit_refused": "Not submitted: the check found errors.",
	"status_notes_saved": "Notebook saved ({detail} characters).",
	"status_draft_saved": "Draft saved.",
	"status_carried": "Last round's orders are loaded again and checked against the new map ({detail}).",
	"status_not_yours": "That beacon is not yours.",
	"status_saved_file": "Saved to {detail}.",
	"status_save_failed": "Could not write {path}.",
	"status_open_failed": "Could not read {path}.",
	"status_new_opened": "A new playbook is in the editor; the checks say what it still needs. Undo takes it back.",
	"status_picking": "Click the map for the new value; Escape stops.",
	"lobby_zero_refused": "The Lull ran out, and the gateway refused to be told so: {detail}",
	"status_anchoring": "Click the ground where the target goes; Escape stops.",
	"status_visit_changing": "Adding the visit...",
	"status_rule_editing": "Editing...",
	"status_wizard_used": "The template's playbook is in the editor. Undo takes it back.",
	"status_wizard_refused": "The gateway refused that value: {detail}",
	"status_dormant_allowing": "Allowing dormant beacons...",
	"status_dormant_allowed": "Dormant beacons allowed.",
	"status_dormant_disallowing": "No longer allowing dormant beacons...",
	"status_dormant_disallowed": "Dormant beacons no longer allowed.",
	# An ask the editor did not send, because the planning door is shut (finding K).
	"status_closed_ready": "You pressed Ready: this client sends no planning call for the rest of this Lull.",
	# Neither promises a next Lull: the last Push and the final recap have none (item 143 (3)).
	"status_closed_push": "Planning is closed during the Push. If another round follows, its Lull opens it again.",
	"status_closed_recap": "Planning is closed during the recap. If another round follows, its Lull opens it again.",
	"status_closed_ended": "The match has ended: there is nothing left to plan.",
	# The gateway's own line from this seat's feed, as it came, and what it means for the
	# text on screen (finding J).
	"status_safe_filed": "{detail} The orders on screen were not sealed.",

	# --- The route -------------------------------------------------------------
	# Travel is the gateway's rendered figure (`estimate_route`'s `travel_text`, the register's
	# S3-18; decisions-log items 57 and 61), as it came, a fogged leg already labelled a bound.
	# A figure it sent no text for is the raw game milliseconds it did send, and says so.
	"leg_text": "{text}",
	"route_whole_text": "Travel, as estimated: {text}",
	"leg": "{ms} ms (raw)",
	"route_whole": "Travel, as estimated: {ms} ms (the gateway sent no rendered figure)",
	"route_none": "No route: the commander cannot get there.",
	# The gateway's sentence when a waypoint resolves to nothing (a `covering` that covers
	# nothing), shown as it came: its answer about the route, not a refused call.
	"route_found_nothing": "No route: {why}",
	"route_empty": "No step moves the commander yet.",
	"route_waiting": "Estimating...",

	# --- Targeting: the Lull's sentence and the chips (S1's plan, task `ui`) -----
	# "This round" is the gateway's own sentence, shown as it came under this heading; with
	# no sentence the heading is hidden, never replaced by a claim of the client's own.
	"this_round_heading": "This round",
	"chips_heading": "What the map reads now",
	"chips_waiting": "Reading the map...",
	"chips_none": "No step names a vent or a seam.",
	# One chip per vent or seam the playbook names or describes. Every number is the
	# gateway's, as it came; a travel time is game milliseconds (strings.gd's `leg`).
	"chip_now": "Step {step}: now {feature}, {ms} ms",
	"chip_now_unreachable": "Step {step}: now {feature}, which no route reaches from where it is read",
	"attack_heading": "Attack targets",
	"attack_none": "No Attack beacon in the text.",
	"attack_mark": "Attack (step {step})",
	"attack_voxel": "Step {step}: attacks the ground at ({x}, {y}, {z})",
	"attack_beacon": "Step {step}: attacks {id}, marked on the map while it is in sight",
	"attack_selector": "Step {step}: attacks the enemy beacon its selector picks when the attack runs",
	"attack_enemy_commander": "Step {step}: hunts an enemy commander your side has seen recently, chosen when the force launches; it waits until there is one",
	"attack_unknown": "Step {step}: an attack target this panel cannot read; the checks say what is wrong",
	"chip_next": "; next {feature}, {ms} ms",
	"chip_how_description_covering": " - nearest by travel from the commander, read when the step starts",
	"chip_how_description_on": " - nearest by travel from its beacon, read when the step starts",
	"chip_how_description_other": " - nearest by travel, read when the step starts",
	"chip_how_name_covering": " - named, read when the step starts",
	"chip_how_name_on": " - named, read when the step starts",
	"chip_how_name_other": " - named, read when the step starts",
	"chip_how_covered_on": " - the feature its beacon was placed to cover, bound when it deploys",
	"chip_nothing": "Step {step}: reads nothing now ({failure}; {matched} matched)",
	"feature_vent": "Heat vent ({x}, {y})",
	"feature_seam": "Scrap seam ({x}, {y})",
	"feature_": "{id}",

	# --- The map's menu --------------------------------------------------------
	"beacon_heading": "Beacon {id}",
	"menu_visit": "Visit & change",
	"menu_priority_low": "Set priority low",
	"menu_priority_normal": "Set priority normal",
	"menu_priority_high": "Set priority high",
	"menu_go": "Go here",
	"menu_recycle": "Recycle",
	"menu_place": "Place beacon",
	# A click on a vent offers both: its name, and the description (docs/design/targeting.md).
	# Alt held puts the description first.
	"menu_cover": "Place beacon covering this vent",
	"menu_cover_nearest": "Place beacon covering the nearest vent you can cover",
	"menu_target": "Target (a selector picks when the step starts):",
	"selector_this": "this beacon",
	"selector_nearest": "the nearest own beacon",
	"selector_weakest": "the weakest own beacon",
	"selector_safest": "the safest own beacon",
	"selector_most_threatened": "the most threatened own beacon",

	# --- The placement ghost ---------------------------------------------------
	"ghost_waiting": "Checking this spot...",
	"ghost_legal": "A beacon can go here.",
	"ghost_illegal": "Not here: {sentence}",
}


## The sentence `key` names, with `args` put into its frame. An unknown key comes back as
## itself, so a missing row shows rather than hides.
static func text(key: String, args: Dictionary = {}) -> String:
	if not TEXT.has(key):
		return key
	return String(TEXT[key]).format(args)

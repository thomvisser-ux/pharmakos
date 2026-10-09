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
	"lobby_ready": "Ready",
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
	"meter_heading": "Your economy",
	# The four numbers as the gateway sent them. Headroom is the gateway's own figure,
	# never supply minus draw: nothing here adds or subtracts anything.
	"meter": "${treasury}   supply {supply} kW   draw {draw} kW   headroom {headroom} kW",
	"meter_waiting": "Not read yet.",
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
	"qualifies_yes": "Ready to seal.",
	"qualifies_no": "Cannot be sealed yet.",
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
	"status_saved_file": "Saved to {path}.",
	"status_save_failed": "Could not write {path}.",
	"status_open_failed": "Could not read {path}.",
	"status_wizard_used": "The template's playbook is in the editor. Undo takes it back.",
	"status_wizard_refused": "The gateway refused that value: {detail}",
	"status_dormant_allowing": "Allowing dormant beacons...",
	"status_dormant_allowed": "Dormant beacons allowed.",
	"status_dormant_disallowing": "No longer allowing dormant beacons...",
	"status_dormant_disallowed": "Dormant beacons no longer allowed.",

	# --- The route -------------------------------------------------------------
	# PLACEHOLDER: travel times as raw game milliseconds — OWNER, S3, with plan-core/T18a.
	# Travel times are shown as the raw game milliseconds the estimator
	# answered, until the gateway answers a rendered figure (decisions-log items 57 and 61:
	# a short route's ETA rounded generously or shown in whole seconds). OWNER, with
	# plan-core/T18a, S3.
	"leg": "{ms} ms",
	"route_whole": "Travel, as estimated: {ms} ms",
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
	# A click on a vent names it; Alt-click describes it (docs/design/targeting.md).
	"menu_cover": "Place beacon covering this vent",
	"menu_cover_nearest": "Place beacon covering the nearest vent you can cover",
	"menu_target": "Target, chosen when the step starts:",
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

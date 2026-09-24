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
# PLACEHOLDER: the string table's file and format, and every wording below. OWNER, at S6,
# with the one English string table (skeleton plan T19, PLACEHOLDERs).

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
	"status_submitted": "Submitted. This is now your sealed order.",
	"status_submit_refused": "Not submitted: the check found errors.",
	"status_notes_saved": "Notebook saved ({detail} characters).",
	"status_draft_saved": "Draft saved.",
	"status_carried": "Last round's orders are loaded again and checked against the new map ({detail}).",
	"status_not_yours": "That beacon is not yours.",
	"status_saved_file": "Saved to {path}.",
	"status_save_failed": "Could not write {path}.",
	"status_open_failed": "Could not read {path}.",

	# --- The route -------------------------------------------------------------
	# PLACEHOLDER: travel times are shown as the raw game milliseconds the estimator
	# answered, until the gateway answers a rendered figure (decisions-log items 57 and 61:
	# a short route's ETA rounded generously or shown in whole seconds). OWNER, with
	# plan-core/T18a, S3.
	"leg": "{ms} ms",
	"route_whole": "Travel, as estimated: {ms} ms",
	"route_none": "No route: the commander cannot get there.",
	"route_empty": "No step moves the commander yet.",
	"route_waiting": "Estimating...",

	# --- The map's menu --------------------------------------------------------
	"beacon_heading": "Beacon {id}",
	"menu_visit": "Visit & change",
	"menu_priority_low": "Set priority low",
	"menu_priority_normal": "Set priority normal",
	"menu_priority_high": "Set priority high",
	"menu_go": "Go here",
	"menu_recycle": "Recycle",
	"menu_place": "Place beacon",
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

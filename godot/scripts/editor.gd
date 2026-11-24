# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The playbook editor, T19 pull requests 1 and 2: the map route surface, the validation rows,
# the notebook, draft continuity, Load, Save, Undo and Submit (skeleton plan T19, amended by
# `docs/design/skeleton-plan-w6-notes.md` section A4).
#
# THE EDITOR IS ONE MORE CLIENT OF THE GATEWAY, AND A THIN ONE (spec section 12; AGENTS.md
# section 3 rule 4). This script draws and forwards: a click on the map becomes a request to
# the bridge (`crates/client-gdext/src/editor.rs`), which turns it into a JSON Patch the
# gateway applies; every verdict on the rows is the verifier's, every travel time on the
# route is the estimator's, and the placement ghost's colour is QUICK's answer about the
# draft with the beacon patched in. Nothing here validates, prices or times anything:
# numbers are shown exactly as they came back (a travel time in game milliseconds), and the
# editor's one timer, FULL after 600 ms idle, is the bridge's pacer's.
#
# The map route surface (spec section 13):
#   * click one of your beacons: Visit & change (a priority row), Go here, or Recycle;
#   * click the ground: Go here, or Place beacon - the ghost shows QUICK's verdict for this
#     click (per click at the skeleton, not live on hover: plan T19 amendment).
#     PLACEHOLDER: the ghost per click, not live on hover — OWNER, S6 (w6 notes A4 item 5);
#   * the beacon menu's target choice: this beacon, or a selector - nearest, weakest, safest
#     or most threatened own beacon, chosen when the step starts; Alt held pre-selects the
#     nearest (S3's `ed3b`: the choice is in the menu every time, because at P6a a real
#     Windows window never delivered Alt with the click);
#   * click a heat vent: "Place beacon covering this vent", a place-beacon step whose site
#     is `covering` the vent by its name, and "the nearest vent you can cover", the same
#     step with a description the sim ranks when the step starts, both every time; Alt held
#     puts the description first (docs/design/targeting.md, "Surfaces"; S1's plan, task
#     `ui`; S3's `ed3b`). `--log-input` logs every map click's modifiers. The vents are the ones
#     `get_map_summary` lists this Lull, drawn where the view holds their anchor column,
#     and picked by screen distance as the beacons are;
#   * the route is drawn as a polyline from the gateway's `estimate_route` legs, each leg
#     labelled with its travel time. There are NO dashed legs at the skeleton: the estimator
#     prices every leg over the whole generated map and the view agrees
#     (skeleton-plan-t16a-notes.md section B, "T19" (2)).
#
# Files: Load and Save read and write the player's own JSONC file, byte for byte; Save writes
# the file the text on screen came from (finding G, below). The only
# files the client writes at all are that JSONC file, the lobby's one remembered config
# line (`user://`, no token; scripts/lobby.gd), each instance's own log (`user://logs/`;
# scripts/instance_log.gd) and the checks' own `user://` files. A file is opened only after QUICK has seen it; an out-of-vocabulary construct is
# refused with the verifier's code and pointer and nothing is stripped (spec section 13).
#
# Pull request 2 (skeleton-plan-w6-notes.md section A4, as decisions-log item 112 amends
# it) adds, all drawn as the gateway answered:
#   * the TEMPLATE WIZARD (scripts/wizard.gd): the templates `list_templates` offers, and
#     for the one opened, one page per parameter `instantiate_template{suggested: true}`
#     lists - label, raw value, the operator's mark and its why. Use puts the gateway's
#     playbook into the editor byte for byte, as one edit Undo takes back;
#   * the RULE LIST (scripts/rule_list.gd): `render_plan`'s lines for the text on screen;
#     from S3's `ed3b`, with its chips (each edit one `replace`: the value field, with the gateway's
#     bounds said beside it, an enum's values as sent, a pick on the map), drag reordering (one `move`)
#     and W0702's conflict connector, every patch composed by the bridge from the click;
#   * the own $/kW METER: `get_economy_forecast`'s four numbers, as they came, through
#     strings.gd's frame. Headroom is the gateway's, never supply minus draw. When it is
#     read is the bridge's rig's scheduling (crates/client-gdext/src/rig.rs).
#     PLACEHOLDER: the meter's layout and refresh cadence — OWNER, S6, Tuning.
#   * draft continuity through `get_draft`: the carried draft is fetched and opened every
#     Lull (the bridge's editor).
#
# S1 (S1's plan, task `ui`) adds, every one the gateway's words or numbers as they came:
#   * the Lull's "THIS ROUND" sentence: the gateway's own sentence off the briefing's prose
#     (decisions-log item 133 (3) (i)), read every Lull and after every accepted seal;
#   * the CHIPS: for each vent or seam the playbook names or describes, what it reads now,
#     how far, and what comes next, from `resolve_refs` for the text on screen;
#   * the meter's NEXT BMI, labelled as the BMI at the band held now, ignoring committed
#     spend (item 135 (2) (g)), the committed spend, and the key-core's netting (S1-31);
#   * E0601's ALLOW DORMANT BEACONS checkbox: ticking it applies the verifier's own patch,
#     clearing it removes the option, and the next report says what it costs.
#
# THE FIRST CLICK LANDS (F1, decisions-log item 126 (3); S1's plan, task `fixc`). At the
# demo the first click on Fix or Submit after the panel had changed or scrolled did
# nothing, and the second worked. Four things keep a click on the panel whole:
#   * the rows are rebuilt only when they change (scripts/rows.gd, `fill`), so a redraw
#     that changed nothing never frees the Fix button a press landed on before its release;
#   * a press on the panel closes the map menus first (`_input`), so an open menu is never
#     left behind the click that meant to leave it;
#   * a click the map surface takes releases the GUI focus (`_take_map_click`,
#     decisions-log item 124 (5) (g)), so no text field keeps the keys after it;
#   * Submit and Fix say at once that they were taken ("Submitting...", "Fixing..."), from
#     the bridge's status line, so a click whose answer is still on its way is not taken
#     for a click that was lost.
# S1'S STAGE DEMO (decisions-log item 139 (2); S1's plan, task `fixd`):
#   * G, THE TEXT'S OWN FILE: Save writes, and Save as... offers, the file the text on
#     screen was opened from or last saved to, which the bridge's editor keeps (`text_file`);
#     after a template's playbook or a carried draft there is none, so Save opens Save as...
#     with no name and one Enter cannot overwrite another round's file. Saving over a file
#     that exists asks first: the save dialog's own confirmation.
#   * J, AFTER A TIMEOUT: when the gateway files this seat's safe playbook (its
#     `safe_playbook_filed` line in the seat's own feed), the status line says so with the
#     gateway's words and that the orders on screen were not sealed; "Ready to seal" is the
#     bridge's `seal`, said only in an open Lull. The filed playbook is not shown: no read
#     the seat's token holds returns it once the Push has begun.
#   * K, A SHUT DOOR: once the match has ended the map offers no menu (`map_open`, a branch
#     on the bridge's phase string) and a click says why; in a Push, a recap and after Ready
#     the map offers what it always did (item 139 (2): "What the map offers in a Push is
#     unchanged"). An ask the bridge will not send - the door is shut outside an open Lull -
#     is refused at once with the reason, and a check that will not come - refused by the
#     gateway, PHASE_CLOSED included, or shut out - says why on its own line in place of
#     "Checking...", "Estimating...", "Rendering..." or "Reading the map...".
#
# S2'S STAGE DEMO AND THE REST OF S1'S (decisions-log items 139 (2), 143 (3), 155 (3)):
#   * D, THE PANEL STAYS ON SCREEN: every sentence wraps inside the panel, and should its
#     contents ever ask for more than its width it widens over the map, never off the window;
#   * H, THE STATUS LINE IS NOT STALE: Undo says "Undone." (the bridge's editor), a map action
#     clears the line, and a line about a shut door or a filing goes when the door opens or
#     the round turns;
#   * I, ESCAPE closes the map menus (`close_menu_on_escape`), both ways a key can arrive: to
#     the window, and to a menu that holds the focus;
#   * M, THE UNTICKED BOX IS DRAWN: Godot's default icon is a near-black square at half alpha
#     (measured on 4.7.2), which hardly shows on the panel, so the checkbox has a box of its own
#     (`unticked_box`);
#   * THE SIZE METER (S3's plan, E9), on the title's row: the verifier's `size_units` over
#     `size_budget`, from the last report on the text on screen; the bar's fill is the
#     ProgressBar's own ratio, over budget the line beside it is the verifier's refusal sentence,
#     never one of the client's, and while the report describes an older text it says what the
#     verdict's line says.
#
# Every button on the panel takes no keyboard focus. Run the game with `-- --log-input` and
# the editor prints every press (button_down) and every handled click (pressed) on Submit,
# Fix and Save, and the lobby every mouse button it sees in `_input`: the logging the owner
# reproduces F1 by hand with (godot/README.md). `watch_check.gd`'s `_first_click_lands`
# injects the clicks headless.
#
# PLACEHOLDER: the panel's layout, sizes and colours, wording and look — OWNER, S6. The
# panel's, the menu's wording and the ghost's look are the skeleton's; the real editor's
# layout is S6's (skeleton plan T19).
#
# PLACEHOLDER: the segment clock and the "fits" pill — OWNER, S6.
# Neither is built (skeleton plan T19, PLACEHOLDERs line; S3's plan's register row S3-16). The
# rendered travel time is the gateway's from S3 (`estimate_route`'s `travel_text`, the
# register's S3-18): the route and each leg say it as it came, and only a figure the gateway
# sent no text for is drawn as its raw game milliseconds, saying so.

extends CanvasLayer

const Strings := preload("res://scripts/strings.gd")
const Rows := preload("res://scripts/rows.gd")
const Wizard := preload("res://scripts/wizard.gd")
const RuleList := preload("res://scripts/rule_list.gd")
const ConditionPicker := preload("res://scripts/condition_picker.gd")
const ValueField := preload("res://scripts/value_field.gd")
const Values := preload("res://scripts/values.gd")

## How close, in screen pixels, a click must land to a beacon to pick it.
## PLACEHOLDER: the pick distance — OWNER, at S6.
const PICK_PIXELS := 28.0
## The panel's width in pixels. PLACEHOLDER: the panel's width — OWNER, at S6.
const PANEL_WIDTH := 380.0
## The route's colour, and the ghost's by verdict. PLACEHOLDER: the colours — OWNER, at S6.
const ROUTE_COLOUR := Color(0.95, 0.85, 0.35)
const GHOST_COLOURS := {
	"waiting": Color(0.8, 0.8, 0.8, 0.45),
	"legal": Color(0.3, 0.9, 0.4, 0.5),
	"illegal": Color(0.95, 0.25, 0.2, 0.5),
}
## The size meter's bar, in pixels, and how far it fades while its numbers describe an older text.
## PLACEHOLDER: the size meter's look — OWNER, at S6.
const SIZE_BAR := Vector2(64, 10)
const SIZE_STALE_ALPHA := 0.35
## The verdict line's and the size meter's two looks (the P6a finding): CHECKING while a check
## of the text on screen is on its way, ANSWERED from its answer until the player's next input.
## Still colours behind the words, never a flash: there is no motion, and no clock. Part of the
## panel's look, whose marker heads this file.
const CHECKING_LOOK := Color(0.55, 0.6, 0.7, 0.22)
const ANSWERED_LOOK := Color(0.95, 0.78, 0.25, 0.38)
## The guided first pick's look: the marked template's button is tinted, a mark with no words.
## PLACEHOLDER: the guided pick's look — OWNER, at S6.
const GUIDED_LOOK := Color(1.0, 0.85, 0.45)
## The unticked box: its side in pixels, its outline's width, and its colour, shown or disabled.
## PLACEHOLDER: the checkbox's box — OWNER, at S6's art pass.
const BOX_SIDE := 16
const BOX_LINE := 2
const BOX_COLOUR := Color(0.92, 0.92, 0.92)
const BOX_COLOUR_DISABLED := Color(0.92, 0.92, 0.92, 0.4)
## How far above the ground the route is drawn, in voxels. PLACEHOLDER: the lift — OWNER, at S6.
const ROUTE_LIFT := 1.2
## An Attack target's mark: its colour, and how far over a voxel target and over a named
## beacon's marker it floats, in voxels. PLACEHOLDER: the attack marks — OWNER, at S6.
const ATTACK_COLOUR := Color(1.0, 0.35, 0.25)
const ATTACK_MARK_LIFT := 2.0
const ATTACK_BEACON_LIFT := 11.0
## A Build target's mark: its colour, and how far over its anchor it floats, in voxels.
## PLACEHOLDER: the Build target marks — OWNER, at S6.
const BUILD_COLOUR := Color(0.45, 0.9, 1.0)
const BUILD_MARK_LIFT := 2.5

## Menu item ids.
const ITEM_GO := 1
const ITEM_RECYCLE := 2
const ITEM_PLACE := 3
const ITEM_COVER := 4
const ITEM_COVER_NEAREST := 5
## The ground menu's first line: the ghost's words, a line and not an action.
const ITEM_GHOST := 6
const ITEM_LOW := 11
const ITEM_NORMAL := 12
const ITEM_HIGH := 13
## "This beacon": the clicked beacon itself, the first of the beacon menu's target choices.
const ITEM_THIS := 19
const ITEM_SELECTOR := 20
const SELECTORS := ["nearest", "weakest", "safest", "most_threatened"]
## Visit & change's rows that open an entry list (`_open_entry_list`): Switch mandate, Add a build
## target, Queue a structure, each the bridge's row of that name on the entry picked.
const ITEM_LIST_MANDATE := 31
const ITEM_LIST_ADD_TARGET := 32
const ITEM_LIST_QUEUE := 33
const ENTRY_LISTS := {ITEM_LIST_MANDATE: ["mandate", "mandates", "menu_mandate"], ITEM_LIST_ADD_TARGET: ["add_target", "blueprints", "menu_add_target"], ITEM_LIST_QUEUE: ["queue", "queue_blueprints", "menu_queue"]}
## A locked entry's sentence colour. PLACEHOLDER: the entry list's look — OWNER, at S6.
const LOCKED_COLOUR := Color(0.65, 0.65, 0.65)
## How much of the window's height the entry list leaves free, in pixels: its panel's margins.
const ENTRY_MARGIN := 24.0
## Every popup the editor owns draws this panel (S3's `fixp`, P6b finding 6): opaque. Measured on
## Godot 4.7.2, the default theme's popup panel is a StyleBoxFlat drawing its centre at
## Color(0.1, 0.1, 0.1, 0.6), so the map showed through the value field's popup and the picker.
## PLACEHOLDER: the popups' panel colour — OWNER, at S6.
const POPUP_COLOUR := Color(0.12, 0.12, 0.14, 1.0)
## The user argument (after `--`) that turns on the click logging.
const LOG_INPUT_ARG := "--log-input"
## What New puts on screen: the empty playbook the client ships, one data file (the P6a
## finding). The checks, not the client, say what it still needs.
const NEW_PLAYBOOK := "res://playbooks/new.jsonc"
## The panel buttons whose clicks are counted and, with `--log-input`, logged.
const COUNTED := ["submit", "fix", "save"]

var bridge: Node = null
var vista: Node3D = null

## Whether `--log-input` was given: every press and handled click on Submit, Fix and Save
## is printed, and the lobby prints the mouse buttons it sees.
var log_input := false

var _changes := -1
var _panel: PanelContainer
var _status: Label
var _verdict: Label
var _rows: VBoxContainer
var _route_label: Label
var _beacon_label: Label
var _ghost_label: Label
var _notes: TextEdit
var _notes_status: Label
var _drafts: Label
var _undo_button: Button
## Whether a check of the text on screen was on its way at the last redraw, and whether its
## answer's highlight is showing (`_mark_check`).
var _was_checking := false
var _highlit := false
var _new_button: Button
var _size_box: HBoxContainer
var _size_bar: ProgressBar
var _size_line: Label
var _load_dialog: FileDialog
var _save_dialog: FileDialog
var _menu: PopupMenu
var _visit_menu: PopupMenu
var _menu_target := {}
## The target each menu entry acts on, by its item id, where entries differ in target (the
## vent menu's name and description).
var _menu_targets := {}
var _selector := ""
var _route_mesh: MeshInstance3D
var _route_labels: Node3D
var _ghost: MeshInstance3D
var _attack_label: Label
## The map's marks for the Attack targets the text names: a mark per voxel target, and a
## mark over each named beacon the vista draws, placed every frame because the vista
## redraws its markers with every view. id -> Label3D for the named beacons.
var _attack_marks: Node3D
var _attack_beacons := {}
var _build_label: Label
## The map's marks for the Build targets the text writes: a mark per target whose anchor can be
## drawn, and a wall's box as the outline of its two corners as written (S3's `fixp`).
var _build_marks: Node3D
var _build_boxes: MeshInstance3D
var _notes_loaded := false
var _meter: Label
var _meter_answers := -1
var _forecast: Label
var _this_round: Label
var _this_round_box: VBoxContainer
var _chips: Label
var _dormant: CheckBox
## Why the box's last click was refused, beside it (S3's `fixp`, P6b finding 9).
var _dormant_said: Label
var _chips_drawn := ""
var _templates_box: VBoxContainer
var _wizard_box: VBoxContainer
var _rules_status: Label
var _rules_box: VBoxContainer
## The rule list's chips as the bridge last sent them, for a click on one to find its editor.
var _rule_chips: Array = []
## The rendering id of the rule list on screen (the bridge's `rules_rendering`), and why
## it takes no edit now (`rules_blocked`, "" when it takes one).
var _rules_rendering := -1
var _rules_blocked := ""
## A number's editor (a chip's, or a Visit & change setting's): the one value field
## (scripts/value_field.gd), the gateway's bounds as sent in its note, and the chip it is for,
## named as the bridge checks it: its index, the rendering and its pointer (M1). Its last line
## says what became of the value it sent, the bridge's `field`.
var _number_popup: PopupPanel
var _number_heading: Label
var _number_note: Label
var _number_said: Label
var _number_field: ValueField
var _number_chip := {}
## An enum chip's editor: its values as sent, and the chip it is for, named the same way.
var _choice_menu: PopupMenu
var _choice_chip := {}
## A name chip's editor (a step label or a rule id): the names the bridge read out of the text on
## screen, in file order, and the chip it is for, named the same way (S3's `ed3b`, part 2).
var _name_menu: PopupMenu
var _name_chip := {}
var _step_labels := PackedStringArray()
var _rule_ids := PackedStringArray()
## The condition picker: the bridge's `vocabulary` as last drawn, and the slot it is open for,
## named as the bridge checks it (the rule's index and pointer, the rendering, the slot's pointer
## and whether a condition stands there).
var _vocabulary := {}
var _picker_popup: PopupPanel
var _picker_line: Label
var _picker_again: Button
var _picker_tree: Tree
var _picker_slot := {}
## The rule list's condition slots as the bridge last sent them.
var _rule_slots: Array = []
## Visit & change (S3's `ed3b`, part 2): the beacon the menu is open for, the detail it was last
## drawn from, the submenus built from it, and the setting a number field is open for.
var _visit_beacon := ""
var _visit_drawn := ""
var _visit_submenus: Array[PopupMenu] = []
## The entry list (S3's `fixp`, P6b finding 5): the vocabulary's mandates or blueprints, as the
## Visit & change row that opened it lists them, for the beacon the menu was open for.
var _entry_popup: PopupPanel
var _entry_box: VBoxContainer
var _entry_scroll: ScrollContainer
var _entry_beacon := ""
## The entries the open list draws, and the bridge's row name a pick makes.
var _entry_row := ""
## Emitted with the row name and the entry's index when an entry is picked (a check listens).
signal entry_picked(row: String, which: int)
var _number_setting := {}
## The voxel whose ghost the ground menu's first line was filled from, or null: the menu closes
## when that ghost goes, and not for a ghost that was never asked (fix pass 2, item 1).
var _menu_ghost_at: Variant = null
var _templates_drawn := ""
var _wizard_drawn := ""
## Per counted button: how many presses (`button_down`) and handled clicks (`pressed`) it
## has had, for `watch_check.gd`'s `_first_click_lands`.
var _clicks := {}
## The panel's own buttons, by their string key.
var _named := {}


## Builds the panel, the menus and the map's drawing nodes. `vista` is the scene's vista,
## whose bridge and camera the editor uses.
func setup(the_vista: Node3D) -> void:
	vista = the_vista
	bridge = vista.bridge
	# The gateway's message cap, passed through for the words that name it (fix pass 2).
	var cap: Variant = Values.int_of(bridge.watch_transport_limits().get("message_bytes"))
	Strings.message_limit = str(cap) if cap is int else Values.unreadable_text(Strings.text("value_limit"), cap)
	log_input = LOG_INPUT_ARG in OS.get_cmdline_user_args()
	for name in COUNTED:
		_clicks[name] = {"down": 0, "pressed": 0}
	_build_panel()
	_build_menus()
	_build_map_nodes()


## Starts the editor for `seat` (spelt as the gateway spells a seat). Call after the host
## link has started the bridge's watch rig.
func begin(seat: String) -> void:
	bridge.editor_seat(seat)


## Opens the file at `path`: its bytes go to QUICK first, and the editor takes it only if
## the verifier found nothing out of vocabulary. Once it is taken, `path` is the text's
## file (`text_file`).
func load_file(path: String) -> bool:
	var bytes := FileAccess.get_file_as_bytes(path)
	if bytes.is_empty() and FileAccess.get_open_error() != OK:
		_say_local("status_open_failed", {"path": path})
		return false
	return bridge.editor_load_file(bytes, path)


## New: puts the empty playbook the client ships on screen, as one edit Undo takes back
## (the P6a finding). Its checks follow as for any edit.
func new_playbook() -> bool:
	var bytes := FileAccess.get_file_as_bytes(NEW_PLAYBOOK)
	if bytes.is_empty():
		_say_local("status_open_failed", {"path": NEW_PLAYBOOK})
		return false
	return bridge.editor_new(bytes)


## Opens a playbook from its bytes, which came from no file of the player's.
func load_bytes(bytes: PackedByteArray) -> bool:
	return bridge.editor_load(bytes)


## Writes the playbook on screen to `path`, byte for byte; from then on `path` is the text's
## file.
func save_file(path: String) -> bool:
	var bytes: PackedByteArray = bridge.editor_bytes()
	var file := FileAccess.open(path, FileAccess.WRITE)
	if file == null:
		_say_local("status_save_failed", {"path": path})
		return false
	file.store_buffer(bytes)
	file.close()
	# The bridge's editor keeps the file and says so; a line said here alone would be gone at the
	# next redraw, which that call causes.
	bridge.editor_saved_to(path)
	return true


## The file the text on screen was opened from or last saved to, as the bridge's editor
## keeps it, or "" when the text came from a template, a carried draft or no file (S1's
## stage demo, finding G).
func text_file() -> String:
	return String(bridge.editor_state().get("file", ""))


## Opens Save as..., offering the text's own file and no other (finding G): after a
## template's playbook or a carried draft it offers no name, so one Enter cannot overwrite
## another round's file. Saving over a file that exists asks first: the dialog's own
## overwrite confirmation (FILE_MODE_SAVE_FILE).
func open_save_as() -> void:
	var file := text_file()
	if file == "":
		_save_dialog.current_file = ""
	else:
		_save_dialog.current_path = file
	_save_dialog.popup_centered_ratio(0.6)


## Opens Load, with no name in its box (item 143 (3): it once showed a stray word), in the folder
## of the text's own file, or else of the last file Loaded or Saved this match (S3's `fixp`:
## it opened in the launch folder). The native dialog is the operating system's.
func open_load() -> void:
	aim_load(bridge.editor_state())
	_load_dialog.popup_centered_ratio(0.6)


## Sets the Load dialog's folder from the bridge's editor state (`load_folder`) and empties its
## box.
func aim_load(state: Dictionary) -> void:
	var folder := load_folder(state)
	if folder != "":
		_load_dialog.current_dir = folder
	_load_dialog.current_file = ""


## The folder Load opens in: the text's own file's (`file`), else the last file Loaded or Saved
## this match (`last_file`), else none, which leaves the dialog where it is.
static func load_folder(state: Dictionary) -> String:
	var file := String(state.get("file", ""))
	if file == "":
		file = String(state.get("last_file", ""))
	return file.get_base_dir()


## A map action on a target: `go`, `visit_low`, `visit_normal`, `visit_high`, `recycle` or
## `place`, at `{"beacon": id}`, `{"selector": name}` or `{"voxel": Vector3i}`.
func act(action: String, target: Dictionary) -> bool:
	return bridge.editor_action(action, target)


## Submits the playbook on screen.
func submit() -> bool:
	return bridge.editor_submit()


## Presses the `fix`th Fix button of row `row`.
func fix(row: int, which: int) -> bool:
	return bridge.editor_fix(row, which)


## The rows on screen, for a check to read their accessible names.
func row_controls() -> Array[Control]:
	return Rows.row_controls(_rows)


## The status line as shown.
func status_text() -> String:
	return _status.text


## Closes the map menus, every level of them, as a click elsewhere would. Every action pick
## calls it (S3's `fixp`, P6b finding 3): Godot hides a submenu's parents on a checkable pick only
## while each parent hides on one, and the beacon menu does not (its target choice, radio rows,
## stays open on a pick, the P6a fix), so a priority row or an enum setting's value left it open
## and the next map click only closed it. A target choice is the one pick that leaves it open.
func close_menu() -> void:
	for menu in _map_menus():
		menu.hide()


## Whether a map menu is open, at any level.
func menu_open() -> bool:
	for menu in _map_menus():
		if menu.visible:
			return true
	return false


## The map menus: the beacon, vent or ground menu, Visit & change, its submenus and theirs.
func _map_menus() -> Array[PopupMenu]:
	var out: Array[PopupMenu] = [_menu, _visit_menu]
	for submenu in _visit_submenus:
		out.append(submenu)
		for child in submenu.get_children():
			if child is PopupMenu:
				out.append(child)
	return out


## How many presses and handled clicks each counted panel button has had:
## `{"submit": {"down": n, "pressed": n}, "fix": ..., "save": ...}`.
func click_counts() -> Dictionary:
	return _clicks.duplicate(true)


## The panel's Submit or Save button, or the first Fix button drawn (or null), for a check
## to click.
func panel_button(name: String) -> Button:
	if name == "fix":
		var fixes: Array[Button] = Rows.fix_buttons(_rows)
		return null if fixes.is_empty() else fixes[0]
	return _named.get(name)


## The panel's rectangle on screen.
func panel_rect() -> Rect2:
	return _panel.get_global_rect()


## The panel's scroll container, for a check to read how far it scrolled.
func panel_scroll() -> ScrollContainer:
	return _panel.get_child(0)


## The Save as... dialog, for a check to see that Save with no file opened it, and to close
## it.
func save_dialog() -> FileDialog:
	return _save_dialog


## The wizard's pages as drawn, in order (see scripts/wizard.gd, `page_controls`).
func wizard_page_controls() -> Array[Control]:
	return Wizard.page_controls(_wizard_box)


## The wizard's Use button as drawn, or null.
func wizard_use_button() -> Button:
	return Wizard.use_button(_wizard_box)


## The wizard's why as drawn.
func wizard_why_text() -> String:
	return Wizard.why_text(_wizard_box)


## The wizard's refusal as drawn.
func wizard_refusal_text() -> String:
	return Wizard.refusal_text(_wizard_box)


## The rule list's rows as drawn, in order.
func rule_controls() -> Array[Control]:
	return RuleList.row_controls(_rules_box)


## The template buttons as drawn, in order: each carries the template's `id` as a meta.
func template_buttons() -> Array[Button]:
	var out: Array[Button] = []
	for child in _templates_box.get_children():
		if child is Button and child.has_meta("id"):
			out.append(child)
	return out


## The guided first pick as drawn on the panel, for a check to read (`guided_in`).
func guided_drawn() -> Array:
	return guided_in(_templates_box)


## The meter as drawn.
func meter_text() -> String:
	return _meter.text


## The meter's forecast lines as drawn: the next BMI, the committed spend, the key-core.
func forecast_text() -> String:
	return _forecast.text


## The Lull's "this round" line as drawn.
func this_round_text() -> String:
	return _this_round.text


## The chips as drawn, one line each.
func chips_text() -> String:
	return _chips.text


## Opens the wizard on template `id`, as its button does.
func open_template(id: String) -> void:
	bridge.editor_wizard_open(id)


## Sends `text` for the wizard page at `pointer`, as its field and Send button do.
func wizard_send(pointer: String, text: String) -> void:
	bridge.editor_wizard_set(pointer, text)


## Called by the lobby every frame: redraws when the bridge says something changed.
func refresh() -> void:
	_draw_meter(bridge.watch_meter())
	var state: Dictionary = bridge.editor_state()
	if state.is_empty():
		return
	# Every count the bridge sends is read through Values.int_of: one past `int` is its refusal,
	# said as unreadable on its line, never a script error (S3's `fixp`).
	var changes: Variant = Values.int_of(state.get("changes", 0))
	if changes is int and changes == _changes:
		return
	_changes = changes if changes is int else -1
	_draw_state(state)
	if not (changes is int):
		_status.text = Values.unreadable_text(Strings.text("value_changes"), changes)


func _process(_delta: float) -> void:
	if bridge != null:
		refresh()
	_place_attack_marks()


## A press on the panel closes the map menus before any control takes it, so a menu is
## never left open behind the click that meant to leave it (F1). Escape closes them too.
func _input(event: InputEvent) -> void:
	if _panel == null:
		return
	if next_input(event):
		_clear_highlight()
	if close_menu_on_escape(event):
		get_viewport().set_input_as_handled()
		return
	if event is InputEventKey and event.pressed and event.keycode == KEY_ESCAPE and _a_pick_is_on():
		bridge.editor_cancel_pick()
		get_viewport().set_input_as_handled()
		return
	if not (event is InputEventMouseButton) or not event.pressed:
		return
	if menu_open() and _panel.get_global_rect().has_point(event.position):
		close_menu()


## Whether a map pick is on: a rule-list chip's, or a Build target's anchor. A picking index
## that cannot be read counts as a pick: the bridge, which holds the pick, answers the click.
func _a_pick_is_on() -> bool:
	return picking_on(bridge.editor_state())


## Whether the editor state says a map pick is on (see `_a_pick_is_on`).
static func picking_on(state: Dictionary) -> bool:
	var picking: Variant = Values.int_of(state.get("picking", -1))
	return not (picking is int) or picking >= 0 or state.get("anchoring", false)


## Closes a map menu that is open when `event` is Escape going down, and says whether it did
## (finding I). Called from the window's `_input` and from each menu's own `window_input`, since
## a key reaches the one or the other as the menu holds the focus or does not.
func close_menu_on_escape(event: InputEvent) -> bool:
	if not (event is InputEventKey) or not event.pressed or event.keycode != KEY_ESCAPE or not menu_open():
		return false
	close_menu()
	return true


func _unhandled_input(event: InputEvent) -> void:
	if vista == null or not visible or not (event is InputEventMouseButton):
		return
	if not event.pressed or event.button_index != MOUSE_BUTTON_LEFT:
		return
	if log_input:
		# The P6a hand check: what the window delivered with this click, both ways Alt can be read.
		print("[editor] map click at %s: alt_pressed %s, KEY_ALT held %s, shift %s, ctrl %s" % [event.position, event.alt_pressed, Input.is_key_pressed(KEY_ALT), event.shift_pressed, event.ctrl_pressed])
	map_click(event.position, alt_held(event))


## Whether Alt is held for a map click: the event's own flag, or the key's state as the engine
## reads it. At P6a, on a real Windows window, Alt-click never reached the map as an Alt-click;
## Alt is a shortcut only, and every choice it makes is also in the menu without it.
static func alt_held(event: InputEventWithModifiers) -> bool:
	return event.alt_pressed or Input.is_key_pressed(KEY_ALT)


## A left-click on the map at `at` (screen pixels), Alt held or not: the map surface's menus
## for the beacon, the vent or the ground under it. Returns whether the map took the click.
## `_unhandled_input` calls it for every left press, and the watch check calls it where a
## headless window, all editor panel, would not route an injected click to the map.
func map_click(at: Vector2, alt: bool) -> bool:
	if not map_open():
		# No menu once the match has ended (finding K): say why, and leave the click alone.
		_say_local("status_closed_ended", {})
		return false
	var camera: Camera3D = vista.rig.camera
	if picking_on(bridge.editor_state()):
		return _pick_at(camera, at)
	var beacon := _beacon_under(camera, at)
	if not beacon.is_empty():
		_take_map_click()
		_on_beacon_clicked(beacon, alt, at)
		return true
	var vent := _vent_under(camera, at)
	if not vent.is_empty():
		_take_map_click()
		_on_vent_clicked(vent, alt, at)
		return true
	var picked: Dictionary = bridge.view_pick(camera.project_ray_origin(at), camera.project_ray_normal(at))
	if picked.get("hit", false):
		_take_map_click()
		_on_ground_clicked(picked["at"], at)
		return true
	return false


## A map click while a rule-list chip's pick is on: what is under it - a beacon (any seat's), a
## vent or the ground - goes to the bridge, which writes the arm the click names when the
## gateway lists it there, or says why not and keeps the pick on. No menu opens.
func _pick_at(camera: Camera3D, at: Vector2) -> bool:
	var target := {}
	var beacon := _beacon_under(camera, at)
	var vent := _vent_under(camera, at)
	if not beacon.is_empty():
		target = {"beacon": String(beacon["id"])}
	elif not vent.is_empty():
		target = {"feature": String(vent["id"])}
	else:
		var picked: Dictionary = bridge.view_pick(camera.project_ray_origin(at), camera.project_ray_normal(at))
		# A click on nothing (the sky) is still the pick's: the bridge refuses it by its reason,
		# `no_target`, rather than the click passing in silence.
		if picked.get("hit", false):
			target = {"voxel": picked["at"]}
	_take_map_click()
	bridge.editor_pick(target)
	return true


## Whether the map offers its menus: until the match has ended, by the bridge's phase string
## (S1's stage demo, finding K: after the match ended, a click still offered "Place beacon"
## and the panel then waited on "Checking..." for a call the rig would never send). In a
## Push, a recap or after Ready the menus open as before, and what they ask for is refused
## at once by the bridge's editor with the shut door's reason (item 139 (2)).
func map_open() -> bool:
	return String(bridge.watch_state().get("phase", "")) != "ended"


## The map surface took a left-click: it is handled here, so the camera rig never sees it,
## and the GUI focus is released as the rig would release it, so a text field does not keep
## the keys (decisions-log item 124 (5) (g)).
func _take_map_click() -> void:
	get_viewport().set_input_as_handled()
	get_viewport().gui_release_focus()


# --- The map --------------------------------------------------------------------------

func _beacon_under(camera: Camera3D, at: Vector2) -> Dictionary:
	var best := {}
	var best_distance := PICK_PIXELS
	for beacon in vista.beacons():
		var where: Vector3 = beacon["position"]
		if camera.is_position_behind(where):
			continue
		var distance := camera.unproject_position(where).distance_to(at)
		if distance <= best_distance:
			best_distance = distance
			best = beacon
	return best


## The heat vent nearest the click on screen, within PICK_PIXELS, as the bridge lists the
## map's features this Lull; empty when there is none.
func _vent_under(camera: Camera3D, at: Vector2) -> Dictionary:
	var best := {}
	var best_distance := PICK_PIXELS
	for feature in bridge.editor_features():
		if String(feature.get("kind", "")) != "vent":
			continue
		var where := _world(feature["at"])
		if camera.is_position_behind(where):
			continue
		var distance := camera.unproject_position(where).distance_to(at)
		if distance <= best_distance:
			best_distance = distance
			best = feature
	return best


## A click on a vent offers a beacon covering it by its name AND by the description, "the
## nearest vent you can cover" (docs/design/targeting.md), every time: no modifier is needed
## (the P6a finding). Alt only puts the description first.
func _on_vent_clicked(vent: Dictionary, alt: bool, at: Vector2) -> void:
	_selector = ""
	_beacon_label.text = Strings.text("feature_vent", {"x": vent.get("x", ""), "y": vent.get("y", "")})
	_menu_target = {}
	_menu_targets = {}
	_menu.clear()
	for choice in vent_choices(String(vent["id"]), alt):
		_menu_targets[choice["id"]] = choice["target"]
		_menu.add_item(Strings.text(choice["key"]), choice["id"])
	_menu.position = Vector2i(at)
	_menu.popup()


## The vent menu's two entries, in the order shown: the vent by its name, and the description
## of the nearest vent you can cover, each with the target its step names. Both are there every
## time; Alt held puts the description first.
static func vent_choices(vent_id: String, alt: bool) -> Array:
	var named := {"key": "menu_cover", "id": ITEM_COVER, "target": {"feature": vent_id}}
	var described := {"key": "menu_cover_nearest", "id": ITEM_COVER_NEAREST, "target": {"nearest_uncovered": "vent"}}
	return [described, named] if alt else [named, described]


func _on_beacon_clicked(beacon: Dictionary, alt: bool, at: Vector2) -> void:
	# Menu routing, not a rule: the owner is the gateway's own field on the view, and the
	# map route surface's menu is for the seat's own beacons (spec section 13). A visit
	# to another seat's beacon is the verifier's E0401 if it is ever written by hand.
	if String(beacon["owner"]) != vista.my_seat:
		_say_local("status_not_yours", {})
		return
	var id := String(beacon["id"])
	# The click maps onto get_beacon through the beacon's `b_NN` id (t16a notes section B,
	# "T19" (4)); its description heads the panel while the menu is open.
	bridge.editor_describe_beacon(id)
	_beacon_label.text = Strings.text("beacon_heading", {"id": id})
	# The target choice is in the menu every time (the P6a finding): this beacon, or a
	# selector chosen when the step starts. Alt held pre-selects the nearest own beacon.
	_selector = SELECTORS[0] if alt else ""
	_menu_target = {"beacon": id}
	_menu_targets = {}
	_menu.clear()
	_menu.add_separator(Strings.text("menu_target"))
	_menu.add_radio_check_item(Strings.text("selector_this"), ITEM_THIS)
	_menu.set_item_checked(_menu.get_item_index(ITEM_THIS), _selector == "")
	for index in SELECTORS.size():
		var name: String = SELECTORS[index]
		_menu.add_radio_check_item(Strings.text("selector_" + name), ITEM_SELECTOR + index)
		_menu.set_item_checked(_menu.get_item_index(ITEM_SELECTOR + index), name == _selector)
	_menu.add_separator()
	_visit_beacon = id
	_visit_drawn = ""
	_fill_visit_menu(bridge.editor_state())
	_menu.add_submenu_node_item(Strings.text("menu_visit"), _visit_menu)
	_menu.add_item(Strings.text("menu_go"), ITEM_GO)
	_menu.add_item(Strings.text("menu_recycle"), ITEM_RECYCLE)
	_menu.position = Vector2i(at)
	_menu.popup()


func _on_ground_clicked(at: Vector3i, where: Vector2) -> void:
	_menu_target = {"voxel": at}
	_menu_targets = {}
	_menu_ghost_at = null
	_selector = ""
	_beacon_label.text = ""
	# The ghost for this click: the draft with a beacon patched in here, checked by QUICK.
	var asked: bool = bridge.editor_preview_place(at)
	_menu.clear()
	# The ghost's words head the menu the click opened (the P6a finding: they showed only in
	# the side panel, away from the click); `_ghost_in_menu` fills them in as QUICK answers.
	_menu.add_item(Strings.text("ghost_waiting"), ITEM_GHOST)
	_menu.set_item_disabled(_menu.get_item_index(ITEM_GHOST), true)
	if not asked:
		# Refused at once (a Push, a recap, after Ready, no playbook): the line says why, in the
		# bridge's own words, and the menu stays open as it always did (item 139 (2)).
		var state: Dictionary = bridge.editor_state()
		_menu.set_item_text(_menu.get_item_index(ITEM_GHOST), Strings.text("status_" + String(state.get("status_key", "")), {"detail": state.get("status_detail", "")}))
	_menu.add_item(Strings.text("menu_go"), ITEM_GO)
	_menu.add_item(Strings.text("menu_place"), ITEM_PLACE)
	_menu.position = Vector2i(where)
	_menu.popup()


## The open ground menu, when it is the one for the ghost's voxel: its first line says what
## the panel's ghost line says, and "Place beacon" is greyed while the gateway says the spot is
## illegal. Nothing is decided here: the words and the verdict are QUICK's.
func _ghost_in_menu(ghost: Dictionary) -> void:
	if not _menu.visible or _menu.get_item_index(ITEM_GHOST) < 0 or ghost.is_empty():
		return
	if not _menu_target.has("voxel") or _menu_target["voxel"] != ghost.get("at"):
		return
	_menu.set_item_text(_menu.get_item_index(ITEM_GHOST), _ghost_label.text)
	_menu_ghost_at = ghost.get("at")
	_menu.set_item_disabled(_menu.get_item_index(ITEM_PLACE), String(ghost.get("state", "")) == "illegal")
	_menu.reset_size()


func _on_menu(id: int) -> void:
	if id == ITEM_THIS or (id >= ITEM_SELECTOR and id < ITEM_SELECTOR + SELECTORS.size()):
		_selector = "" if id == ITEM_THIS else SELECTORS[id - ITEM_SELECTOR]
		_menu.set_item_checked(_menu.get_item_index(ITEM_THIS), _selector == "")
		for index in SELECTORS.size():
			_menu.set_item_checked(_menu.get_item_index(ITEM_SELECTOR + index), SELECTORS[index] == _selector)
		return
	# Every other entry is an action: it closes the map menus, whichever level it was picked in.
	var beside := Rect2(Vector2(_visit_menu.position), Vector2(_visit_menu.size))
	close_menu()
	if ENTRY_LISTS.has(id):
		var vocabulary: Dictionary = bridge.editor_state().get("vocabulary", {})
		open_entry_list(id, vocabulary.get(String(ENTRY_LISTS[id][1]), []), beside)
		return
	var target := _menu_target.duplicate()
	if _selector != "":
		target = {"selector": _selector}
	match id:
		ITEM_GO:
			act("go", target)
		ITEM_RECYCLE:
			act("recycle", target)
		ITEM_PLACE:
			act("place", target)
		ITEM_COVER, ITEM_COVER_NEAREST:
			act("place_covering", _menu_targets.get(id, {}))
		ITEM_LOW:
			act("visit_low", target)
		ITEM_NORMAL:
			act("visit_normal", target)
		ITEM_HIGH:
			act("visit_high", target)


# --- Drawing ---------------------------------------------------------------------------

func _draw_state(state: Dictionary) -> void:
	_status.text = Strings.text("status_" + String(state.get("status_key", "")), {"detail": state.get("status_detail", "")})
	var verdict := String(state.get("verdict", "none"))
	var line := Strings.text("verdict_" + verdict)
	# A check that will not come says why, on its own line, in place of the words that wait
	# for it.
	var unchecked := unchecked_text(state, "rows")
	var checking := false
	if state.get("busy", false) or (state.get("has_text", false) and not state.get("rows_current", false) and verdict != "refused"):
		checking = unchecked == "" or state.get("busy", false)
		line = Strings.text("verdict_checking") if checking else unchecked
	elif String(state.get("seal", "")) != "":
		# Whether the text can be sealed is the bridge's `seal`: said only in an open Lull,
		# and outside one only that the gateway filed the safe playbook (finding J).
		line += " - " + Strings.text("seal_" + String(state["seal"]))
	_mark_check(checking, line)
	_verdict.text = line
	var open: bool = String(state.get("door", "open")) == "open"
	Rows.fill(_rows, state.get("rows", []), Callable(self, "_on_fix_pressed"), state.get("rows_current", false) and not state.get("busy", false) and open, Callable(self, "_on_fix_down"))
	# A depth that cannot be read leaves Undo to the bridge, which says why when there is none.
	var depth: Variant = Values.int_of(state.get("undo_depth", 0))
	_undo_button.disabled = depth is int and depth == 0
	_undo_button.tooltip_text = "" if depth is int else Values.unreadable_text(Strings.text("value_undo_depth"), depth)
	_new_button.disabled = not open
	if state.get("beacon_prose", "") != "":
		_beacon_label.text = String(state["beacon_prose"])
	_draw_notes(state)
	_draw_drafts(state.get("drafts", []))
	_draw_route(state.get("route", {}), state.get("has_text", false), unchecked_text(state, "route"))
	_draw_ghost(state.get("ghost", {}))
	_draw_templates(state.get("templates", []), state.get("templates_known", false), state.get("templates_read", {}), state.get("templates_unchecked", {}))
	_draw_wizard(state.get("wizard", {}), state.get("templates", []))
	_draw_rules(state, unchecked_text(state, "prose"))
	_draw_number_field(state.get("field", {}))
	_fill_visit_menu(state)
	_draw_this_round(String(state.get("this_round", "")))
	_draw_chips(state.get("chips", []), state.get("chips_current", false), state.get("has_text", false), unchecked_text(state, "chips"))
	_draw_attack(state.get("attack_targets", []))
	_draw_build(state.get("build_targets", []))
	_draw_dormant(state.get("dormant", {}))
	# The report the meter draws describes the text on screen only while the rows do. A Load the
	# verifier refused opened nothing, so the text on screen is still the one it counted.
	var counted: bool = state.get("rows_current", false) or verdict == "refused"
	_draw_size(state.get("size", {}), counted, unchecked if unchecked != "" else Strings.text("verdict_checking"))


## Whether a ground menu closes because its ghost went: it is open, it has the ghost's line,
## and that line was filled from a ghost (`filled_from`, the voxel, or null when no ghost ever
## came for it, as when the bridge refused the preview at the click).
static func ghost_went(open: bool, has_line: bool, filled_from: Variant) -> bool:
	return open and has_line and filled_from != null


## The re-check made visible (the P6a finding: "i did not see the re-check and redraw"). While
## a check is on its way the verdict line and the size meter's line wear CHECKING_LOOK; when its
## answer arrives, or the verdict line changes, both wear ANSWERED_LOOK until the player's next
## input (`_clear_highlight`). `after` is the verdict line about to be drawn.
func _mark_check(checking: bool, after: String) -> void:
	if answer_arrived(_was_checking, checking, _verdict.text, after):
		_highlit = true
	_was_checking = checking
	if checking:
		_highlit = false
	var colour := CHECKING_LOOK if checking else (ANSWERED_LOOK if _highlit else Color(0, 0, 0, 0))
	_look(_verdict, colour)
	_look(_size_line, colour)


## Whether an answer is to be highlighted: no check is on its way now, and one was at the last
## redraw, or the verdict line changes.
static func answer_arrived(was_checking: bool, checking: bool, before: String, after: String) -> bool:
	return not checking and (was_checking or before != after)


## The player's next input took the answer's highlight down.
func _clear_highlight() -> void:
	if not _highlit:
		return
	_highlit = false
	if not _was_checking:
		_look(_verdict, Color(0, 0, 0, 0))
		_look(_size_line, Color(0, 0, 0, 0))


## Whether `event` is the player's next input: a key going down, or a mouse button (a click
## or a wheel notch) going down. A mouse move is not, nor a bare modifier key.
static func next_input(event: InputEvent) -> bool:
	if event is InputEventKey:
		# A modifier held for a click (Alt before an Alt-click) is not an input of its own.
		if event.keycode in [KEY_ALT, KEY_SHIFT, KEY_CTRL, KEY_META]:
			return false
		return event.pressed and not event.echo
	return event is InputEventMouseButton and event.pressed


## Paints `colour` behind `label`'s words, or nothing when it is clear.
static func _look(label: Label, colour: Color) -> void:
	if colour.a <= 0.0:
		label.remove_theme_stylebox_override("normal")
		return
	var box := StyleBoxFlat.new()
	box.bg_color = colour
	label.add_theme_stylebox_override("normal", box)


## Why `check` (`rows`, `route`, `prose` or `chips`) of the text on screen will not come, as
## the bridge's editor says (`unchecked[check]`: `key` is `refused`, with the gateway's
## words in `detail`, or the shut door's name), or "" when it is on its way (finding K).
## Shown in place of that check's "Checking...", "Estimating...", "Rendering..." or "Reading
## the map...", only where it is not current; one check's refusal is not another's.
static func unchecked_text(state: Dictionary, check: String) -> String:
	var why: Dictionary = (state.get("unchecked", {}) as Dictionary).get(check, {})
	var key := String(why.get("key", ""))
	if key == "":
		return ""
	return Strings.text("unchecked_" + key, {"detail": why.get("detail", "")})


## The meter: the gateway's four numbers put into strings.gd's frame as they came. Redrawn
## only when a new answer has come.
func _draw_meter(meter: Dictionary) -> void:
	if meter.is_empty():
		return
	var answers: Variant = Values.int_of(meter.get("answers", 0))
	if not (answers is int):
		_meter_answers = -1
		_meter.text = Values.unreadable_text(Strings.text("value_meter_answers"), answers)
		_meter.accessibility_name = _meter.text
		return
	if answers == _meter_answers:
		return
	_meter_answers = answers
	if _meter_answers == 0:
		_meter.text = Strings.text("meter_waiting")
	else:
		_meter.text = Strings.text("meter", {"treasury": meter.get("treasury_now"), "supply": meter.get("supply_kw_now"), "draw": meter.get("draw_kw_now"), "headroom": meter.get("headroom_kw_now")})
	_meter.accessibility_name = _meter.text
	var lines: PackedStringArray = []
	if meter.get("has_bmi_next_dollars", false):
		lines.append(Strings.text("meter_bmi", {"bmi": meter.get("bmi_next_dollars")}))
	if meter.get("has_committed_dollars", false):
		lines.append(Strings.text("meter_committed", {"committed": meter.get("committed_dollars")}))
	if _meter_answers > 0:
		lines.append(Strings.text("meter_key_core"))
	_forecast.text = "\n".join(lines)
	_forecast.accessibility_name = _forecast.text


## The Lull's "this round" sentence, as the gateway wrote it, or nothing at all: with no
## sentence (no seal yet, no briefing read yet, or a seal whose route reads the map nowhere)
## the panel hides the heading rather than word a claim about the orders the gateway did
## not make.
func _draw_this_round(sentence: String) -> void:
	_this_round.text = sentence
	_this_round.accessibility_name = sentence
	_this_round_box.visible = sentence != ""


## One line per chip, each put together from the gateway's answer through strings.gd's
## frames. Redrawn only when the chips changed.
func _draw_chips(chips: Array, current: bool, has_text: bool, unchecked: String) -> void:
	var drawn := var_to_str([chips, current, has_text, unchecked])
	if drawn == _chips_drawn:
		return
	_chips_drawn = drawn
	if has_text and not current:
		_chips.text = unchecked if unchecked != "" else Strings.text("chips_waiting")
	elif chips.is_empty():
		_chips.text = Strings.text("chips_none") if has_text else ""
	else:
		var lines: PackedStringArray = []
		for chip in chips:
			lines.append(chip_line(chip))
		_chips.text = "\n".join(lines)
	_chips.accessibility_name = _chips.text


## One chip's line: what it reads now and next, as the gateway answered, and how it reads.
static func chip_line(chip: Dictionary) -> String:
	var now: Dictionary = chip.get("now", {})
	if now.is_empty():
		return Strings.text("chip_nothing", {"step": Values.said(Strings.text("value_step"), chip.get("step", 0)), "failure": chip.get("failure", ""), "matched": chip.get("matched", 0)})
	# The parts are joined, never added: a travel time is put into its frame as it came.
	# A pick no route reaches (the gateway's `reachable`) has a travel time that means
	# nothing, so the line says it is out of reach instead of drawing one.
	var parts: PackedStringArray = []
	if chip.get("reachable", false):
		parts.append(Strings.text("chip_now", {"step": Values.said(Strings.text("value_step"), chip.get("step", 0)), "feature": feature_text(now), "travel": now.get("travel_text", "")}))
	else:
		parts.append(Strings.text("chip_now_unreachable", {"step": Values.said(Strings.text("value_step"), chip.get("step", 0)), "feature": feature_text(now)}))
	var next: Dictionary = chip.get("next", {})
	if not next.is_empty():
		parts.append(Strings.text("chip_next", {"feature": feature_text(next), "travel": next.get("travel_text", "")}))
	var how := "chip_how_%s_%s" % [chip.get("form", ""), chip.get("arm", "")]
	if Strings.TEXT.has(how):
		parts.append(Strings.text(how))
	return "".join(parts)


## A feature's name as the panel says it: "Heat vent (120, 88)", or its id when the bridge
## could not spell it.
static func feature_text(feature: Dictionary) -> String:
	return Strings.text("feature_" + String(feature.get("kind", "")), {"x": feature.get("x", ""), "y": feature.get("y", ""), "id": feature.get("id", "")})


## E0601's checkbox: shown when the rows name the option or the text sets it, ticked when
## the text sets it. It is disabled only while the planning door is shut, which the status line
## says already; every other click reaches the bridge, which takes it or refuses it with its
## reason, on the status line and beside the box (S3's `fixp`, P6b finding 9: Godot drops a press
## on a disabled box without a word, and the box was disabled while each check ran).
func _draw_dormant(dormant: Dictionary) -> void:
	_dormant.visible = dormant.get("shown", false)
	_dormant.set_pressed_no_signal(dormant.get("checked", false))
	_dormant.disabled = not dormant.get("enabled", false)
	var refused := String(dormant.get("refused", ""))
	_dormant_said.visible = _dormant.visible and refused != "" and refused != "shut"
	_dormant_said.text = Strings.text("status_dormant_refused_" + refused) if _dormant_said.visible else ""
	_dormant_said.accessibility_name = _dormant_said.text


## With `--log-input`, a press on the box is logged.
func _on_dormant_down() -> void:
	if log_input:
		print("[editor] allow dormant: button_down")


## The box's click, with `--log-input` logged: whether it was taken.
func _on_dormant_toggled(on: bool) -> void:
	var taken: bool = bridge.editor_set_allow_dormant(on)
	if log_input:
		print("[editor] allow dormant: toggled %s, taken %s" % [on, taken])


## With `--log-input`, the lobby's line for a press at `at`: whether it fell inside the dormant
## box, and the box's state, so a real-window run tells a click the box took, one it refused, one
## it was disabled for and one the window never delivered apart.
func dormant_log(at: Vector2) -> String:
	return "dormant box: inside %s, shown %s, disabled %s, pressed %s" % [_dormant.is_visible_in_tree() and _dormant.get_global_rect().has_point(at), _dormant.is_visible_in_tree(), _dormant.disabled, _dormant.button_pressed]


## The size meter: the verifier's two numbers as they came, and its refusal in its own words
## over budget (S3's plan, E9). The bar's fill is a display ratio, the ProgressBar's own, and a
## count past the budget draws it full. Hidden until a report names a budget. While the report
## describes an older text (`current` false) the bar fades and the line says `waiting`, the
## words the verdict's line says for the same wait, never the old count.
func _draw_size(size: Dictionary, current: bool, waiting: String) -> void:
	_size_box.visible = not size.is_empty()
	if size.is_empty():
		return
	_size_bar.max_value = size["budget"]
	_size_bar.value = size["units"]
	_size_bar.modulate.a = 1.0 if current else SIZE_STALE_ALPHA
	var refusal := String(size.get("refusal", ""))
	if not current:
		_size_line.text = waiting
	elif refusal != "":
		_size_line.text = refusal
	else:
		_size_line.text = Strings.text("size_meter", {"units": size["units"], "budget": size["budget"]})
	_size_line.accessibility_name = _size_line.text


## The size meter's line as drawn, for a check to read.
func size_text() -> String:
	return _size_line.text


## The size meter on screen, for a check to see it stays in sight.
func size_rect() -> Rect2:
	return _size_box.get_global_rect()


## Redrawn only when the list, its state or its why changed (`fill_templates`).
func _draw_templates(templates: Array, known: bool, read: Dictionary, unchecked: Dictionary) -> void:
	var drawn := var_to_str([templates, known, read, unchecked])
	if drawn == _templates_drawn:
		return
	_templates_drawn = drawn
	fill_templates(_templates_box, templates, known, read, unchecked, open_template)


## The template list as the gateway sent it this Lull (the bridge re-asks at every Lull's
## opening), drawn into `box`: one button per template, pressing which calls `open` with its
## id, and the one the gateway marked as the guided first pick (`guided`, S3's plan, decision 5)
## drawn in GUIDED_LOOK with the gateway's own sentence under it (`guided_reason`), as sent: the
## client adds no words to the mark. Which template is marked, and why, are the gateway's;
## nothing here names one. `known` false is a list not read yet this Lull; `read` says why it did
## not come, when it did not (`failed`, `detail`); `unchecked` why it will not come, the door
## being shut (`key`, in the checks' own words), so no "Reading..." waits on a shut door. Static,
## so the hostless template-list shot draws the panel's own picture.
static func fill_templates(box: Container, templates: Array, known: bool, read: Dictionary, unchecked: Dictionary, open: Callable) -> void:
	var failed := String(read.get("failed", ""))
	var shut := String(unchecked.get("key", ""))
	for child in box.get_children():
		box.remove_child(child)
		child.queue_free()
	if templates.is_empty():
		var none := Label.new()
		none.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		if failed != "":
			none.text = Strings.text("templates_" + failed, {"detail": read.get("detail", "")})
		elif shut != "":
			none.text = Strings.text("unchecked_" + shut, {"detail": unchecked.get("detail", "")})
		elif not known:
			none.text = Strings.text("templates_waiting")
		else:
			none.text = Strings.text("no_templates")
		none.accessibility_name = none.text
		box.add_child(none)
		return
	for template in templates:
		var id := String(template.get("id", ""))
		var guided: bool = template.get("guided", false)
		var button := Button.new()
		button.focus_mode = Control.FOCUS_NONE
		button.pressed.connect(func() -> void: open.call(id))
		box.add_child(button)
		button.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		button.text = Strings.text("template_button", {"title": template.get("title", "")})
		button.accessibility_name = button.text
		button.tooltip_text = String(template.get("summary", ""))
		button.set_meta("id", id)
		if guided:
			# The mark is a look, not words: the gateway's own sentence under the button says
			# why, and the client adds none of its own.
			button.set_meta("guided", true)
			button.self_modulate = GUIDED_LOOK
			var why := Label.new()
			why.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
			why.text = String(template.get("guided_reason", ""))
			why.accessibility_name = why.text
			why.name = "GuidedReason"
			box.add_child(why)


## The guided first pick as drawn in `box`, for a check to read: the id of the template button
## marked (its GUIDED_LOOK) and the gateway's sentence drawn under it, or an empty array when no
## template is marked.
static func guided_in(box: Container) -> Array:
	var children := box.get_children()
	for index in children.size():
		var button: Node = children[index]
		if not (button is Button and button.has_meta("guided")):
			continue
		var why: Node = children[index + 1] if index + 1 < children.size() else null
		return [String(button.get_meta("id", "")), (why as Label).text if why is Label and why.name == "GuidedReason" else ""]
	return []


## Redrawn only when the wizard's answer or refusal changed, so a value being typed is not
## wiped by an unrelated redraw.
func _draw_wizard(wizard: Dictionary, templates: Array) -> void:
	var drawn := var_to_str(wizard)
	if drawn == _wizard_drawn:
		return
	_wizard_drawn = drawn
	var title := String(wizard.get("template_id", ""))
	for template in templates:
		if template.get("id", "") == title:
			title = String(template.get("title", title))
	Wizard.fill(_wizard_box, title, wizard, Callable(self, "wizard_send"), func() -> void: bridge.editor_wizard_use(), func() -> void: bridge.editor_wizard_close())


## The open number field's chip in the rendering drawn now: the chip at the same pointer, when it
## is still a number, so the field stays open with what was typed in it and names the new
## rendering (fix pass 1, item 1: another field's edit landing re-renders the list). Returns
## whether it found one; with none, the field closes, as M1 asks.
func _rebind_number() -> bool:
	if not _number_popup.visible or _number_chip.is_empty():
		return false
	var pointer := String(_number_chip.get("pointer", ""))
	for index in _rule_chips.size():
		var chip: Dictionary = _rule_chips[index]
		if String(chip.get("pointer", "")) == pointer and String(chip.get("editor", "")) == "number":
			_number_chip = {"index": index, "revision": _rules_rendering, "pointer": pointer}
			return true
	return false


## The rule list: `render_plan`'s lines, cut at its chips by the bridge, with the conflict
## connectors and the gateway's refusal of the last edit (S3's `ed3b`; scripts/rule_list.gd).
func _draw_rules(state: Dictionary, unchecked: String) -> void:
	if state.get("has_text", false) and not state.get("prose_current", false):
		_rules_status.text = unchecked if unchecked != "" else Strings.text("rules_waiting")
	else:
		_rules_status.text = ""
	var list: Dictionary = state.get("rule_list", {})
	_rule_chips = list.get("chips", [])
	var rendered := int(state.get("rules_rendering", -1))
	var rendering_moved := rendered != _rules_rendering
	_rules_rendering = rendered
	_rules_blocked = String(state.get("rules_blocked", ""))
	_step_labels = state.get("step_labels", PackedStringArray())
	_rule_ids = state.get("rule_ids", PackedStringArray())
	_vocabulary = state.get("vocabulary", {})
	_picker_line.text = ConditionPicker.status_text(_vocabulary)
	# A refused or unreadable vocabulary can be asked for once more, from the picker's line.
	_picker_again.visible = String(_vocabulary.get("failed", "")) != ""
	_rule_slots = list.get("slots", [])
	# An unreadable picking index highlights no chip; the map click still goes to the bridge.
	var picking: Variant = Values.int_of(state.get("picking", -1))
	var redrawn: bool = RuleList.fill(_rules_box, list, _rules_rendering, state.get("rules_editable", false), picking if picking is int else -1, state.get("conflicts", []), state.get("rule_refusal", {}), Callable(self, "_on_rule_chip"), Callable(self, "_on_rule_move"), Callable(self, "open_condition_picker"), Callable(self, "_on_rule_remove"), Callable(self, "_on_rule_add"), state.get("has_text", false))
	# An editor opened on the rendering drawn before names chips that may not be there any more
	# (M1): it closes with that rendering, and the bridge would refuse its edit anyway. A redraw
	# of the same rendering (an edit on its way, a refusal on a rule) keeps it, so its popup can
	# say what became of the value it sent (S3's `fixp`). The Visit & change field names no
	# rendering, and closes only on its own answer.
	if redrawn and rendering_moved:
		if _number_setting.is_empty() and not _rebind_number():
			_number_popup.hide()
		_choice_menu.hide()
		_name_menu.hide()
		_picker_popup.hide()


## A click on rule-list chip `index` opens its editor, by the shape the gateway sent: the value
## field, with the gateway's bounds said beside it, the enum's values as sent, or a pick on the map; a chip the
## rule list does not edit says why. The edit itself is the bridge's one JSON Patch `replace`.
func _on_rule_chip(index: int) -> void:
	# No editor opens while the list takes no edit (M1): the bridge's reason is said instead.
	if _rules_blocked != "":
		_say_local("status_rule_refused_" + _rules_blocked, {})
		return
	if index < 0 or index >= _rule_chips.size():
		_say_local("status_rule_refused_no_such_chip", {})
		return
	var chip: Dictionary = _rule_chips[index]
	var named := {"index": index, "revision": _rules_rendering, "pointer": String(chip.get("pointer", ""))}
	match String(chip.get("editor", "")):
		"number":
			_open_number(named, chip)
		"choice":
			_choice_chip = named
			_choice_menu.clear()
			# Drawn by the gateway's titles; the value sent is its own JSON, beside the title.
			var values: PackedStringArray = chip.get("values", PackedStringArray())
			var titles: PackedStringArray = chip.get("titles", PackedStringArray())
			for which in values.size():
				_choice_menu.add_radio_check_item(titles[which], which)
				_choice_menu.set_item_checked(which, values[which] == String(chip.get("value_json", "")))
			_choice_menu.position = Vector2i(get_viewport().get_mouse_position())
			_choice_menu.popup()
		"pick":
			bridge.editor_begin_pick(index, _rules_rendering, named["pointer"])
		"name":
			_open_names(named, chip)
		_:
			_say_local("rule_chip_read_only_" + String(chip.get("why", "")), {})


## The names a step-label or rule-id chip is picked from: the labels or rule ids the bridge read
## out of the text on screen, in file order. Listed, never judged: whether a pick is legal there
## is the gateway's check, on the rows.
func _open_names(named: Dictionary, chip: Dictionary) -> void:
	var names := _step_labels if String(chip.get("kind", "")) == "step_label" else _rule_ids
	if names.is_empty():
		_say_local("name_none", {})
		return
	_name_chip = named
	_name_menu.clear()
	for which in names.size():
		_name_menu.add_radio_check_item(names[which], which)
		_name_menu.set_item_checked(which, JSON.stringify(names[which]) == String(chip.get("value_json", "")))
	_name_menu.position = Vector2i(get_viewport().get_mouse_position())
	_name_menu.popup()


func _on_name(which: int) -> void:
	bridge.editor_choose_name(int(_name_chip.get("index", -1)), int(_name_chip.get("revision", -1)), String(_name_chip.get("pointer", "")), which)


## Opens the condition picker for condition slot `slot` of the rule list on screen (its index in
## the bridge's `slots`): family, predicate, parameters, as the bridge's `vocabulary` lists them,
## locked ones greyed with their reason. The slot, its pointer and the op a pick there sends are
## the gateway's: this client composes no pointer and chooses no op.
func open_condition_picker(slot: int) -> void:
	if _rules_blocked != "":
		_say_local("status_rule_refused_" + _rules_blocked, {})
		return
	if slot < 0 or slot >= _rule_slots.size():
		_say_local("status_rule_refused_no_such_slot", {})
		return
	_picker_slot = {"slot": slot, "pointer": String(_rule_slots[slot].get("pointer", "")), "revision": _rules_rendering, "title": String(_rule_slots[slot].get("title", ""))}
	ConditionPicker.fill(_picker_tree, _vocabulary)
	_picker_line.text = ConditionPicker.status_text(_vocabulary)
	_picker_popup.popup_centered()


func _on_picker_use() -> void:
	var picked := ConditionPicker.chosen(_picker_tree)
	if picked.is_empty():
		return
	_picker_popup.hide()
	var slot := int(_picker_slot.get("slot", -1))
	var pointer := String(_picker_slot.get("pointer", ""))
	var revision := int(_picker_slot.get("revision", -1))
	if picked.has("group"):
		bridge.editor_choose_group(slot, pointer, revision, int(picked["group"]))
	else:
		bridge.editor_choose_predicate(slot, pointer, revision, int(picked["family"]), int(picked["entry"]))


## The condition picker's rows as drawn, `[text, selectable]` each, for a check to read.
func picker_rows() -> Array:
	return ConditionPicker.rows(_picker_tree)


# --- Visit & change (S3's `ed3b`, part 2; register row S3-01) ---------------------------

## The Visit & change submenu for the beacon the map menu is open for: the priority rows (S1's),
## then, from the bridge's `beacon_detail` and `vocabulary`, one row per setting (its title and
## value now, its editor by its shape), Switch mandate, Add and Remove a build target, and Queue a
## structure, every entry as sent and locked ones greyed with their reason. Each row the player
## picks is one route step with one row, composed by the bridge. Redrawn only when what it draws
## from changed.
func _fill_visit_menu(state: Dictionary) -> void:
	if _visit_beacon == "":
		return
	var detail: Dictionary = state.get("beacon_detail", {})
	var vocabulary: Dictionary = state.get("vocabulary", {})
	var drawn := var_to_str([_visit_beacon, detail, vocabulary.get("mandates", []), vocabulary.get("blueprints", []), vocabulary.get("queue_blueprints", [])])
	if drawn == _visit_drawn:
		return
	_visit_drawn = drawn
	for submenu in _visit_submenus:
		_visit_menu.remove_child(submenu)
		submenu.queue_free()
	_visit_submenus.clear()
	_visit_menu.clear()
	# The priority rows, the one the beacon's summary sends marked as the value now.
	var now := String(detail.get("priority", "")) if String(detail.get("id", "")) == _visit_beacon else ""
	for row in [[ITEM_LOW, "low"], [ITEM_NORMAL, "normal"], [ITEM_HIGH, "high"]]:
		_visit_menu.add_radio_check_item(Strings.text("menu_priority_" + String(row[1])), int(row[0]))
		_visit_menu.set_item_checked(_visit_menu.item_count - 1, now == String(row[1]))
	_visit_menu.add_separator()
	if String(detail.get("id", "")) != _visit_beacon:
		var failed := String(detail.get("failed", ""))
		var line := Strings.text("menu_detail_" + failed, {"detail": detail.get("detail", "")}) if failed != "" else Strings.text("menu_detail_waiting")
		_visit_menu.add_item(line)
		_visit_menu.set_item_disabled(_visit_menu.item_count - 1, true)
		return
	var settings := _visit_submenu(Strings.text("menu_settings"))
	settings.id_pressed.connect(_on_setting)
	var rows: Array = detail.get("settings", [])
	for which in rows.size():
		_setting_row(settings, which, rows[which])
	# The vocabulary's lists open as the editor's own entry list, which wraps a lock's sentence
	# under its entry and stays inside the window: a PopupMenu does not wrap, so a submenu with
	# the gateway's long sentences ran wider than the window (P6b finding 5).
	_visit_menu.add_item(Strings.text("menu_mandate"), ITEM_LIST_MANDATE)
	_visit_menu.add_item(Strings.text("menu_add_target"), ITEM_LIST_ADD_TARGET)
	var remove := _visit_submenu(Strings.text("menu_remove_target"))
	var targets: Array = detail.get("targets", [])
	for which in targets.size():
		# The target in the gateway's own words (`build_target_lines`), never its anchor's JSON.
		remove.add_item(Strings.text("menu_target_row", {"line": targets[which].get("line", "")}), which)
	remove.id_pressed.connect(func(which: int) -> void: _on_visit_row("remove_target", which))
	_visit_menu.add_item(Strings.text("menu_queue"), ITEM_LIST_QUEUE)


## A submenu of the Visit & change menu, titled `title`.
func _visit_submenu(title: String) -> PopupMenu:
	var submenu := PopupMenu.new()
	opaque(submenu)
	submenu.window_input.connect(close_menu_on_escape)
	_visit_menu.add_child(submenu)
	_visit_menu.add_submenu_node_item(title, submenu)
	_visit_submenus.append(submenu)
	return submenu


## One setting's row: an enum's values as sent in a submenu of their own, a number opening the
## value field, with the gateway's bounds said beside it, a value not edited here greyed with its reason.
func _setting_row(settings: PopupMenu, which: int, setting: Dictionary) -> void:
	var title := Strings.text("menu_setting", {"title": setting.get("title", ""), "value": setting_value_text(setting)})
	match String(setting.get("editor", "")):
		"choice":
			var values := PopupMenu.new()
			opaque(values)
			settings.add_child(values)
			settings.add_submenu_node_item(title, values)
			var listed: PackedStringArray = setting.get("values", PackedStringArray())
			var titles: PackedStringArray = setting.get("titles", PackedStringArray())
			for value in listed.size():
				values.add_radio_check_item(Strings.text("menu_setting_value", {"value": titles[value]}), value)
				values.set_item_checked(value, listed[value] == String(setting.get("value_json", "")))
			var pointer := String(setting.get("pointer", ""))
			values.id_pressed.connect(func(value: int) -> void: _on_setting_choice(which, pointer, value))
		"number":
			settings.add_item(title, which)
		_:
			settings.add_item(title, which)
			settings.set_item_disabled(settings.item_count - 1, true)
			var why := String(setting.get("why", ""))
			var said := Strings.text("rule_chip_read_only_" + why) if why != "" else Strings.text("menu_setting_not_here")
			# A value the bridge could not read against its shape says what could not be read.
			var cause := String(setting.get("unreadable", ""))
			if cause != "":
				said = Strings.text("menu_setting_unreadable", {"said": said, "cause": cause})
			settings.set_item_tooltip(settings.item_count - 1, said)


## A setting's value now as the menu says it, in the gateway's words: a number's `value_text` in
## its display unit, and a choice's title for the value it holds, or that it holds a value the
## gateway does not list. A row the bridge could not read (`read_only`) says so in its own words
## beside it, and draws no value; no value's JSON is drawn in place of the gateway's words.
static func setting_value_text(setting: Dictionary) -> String:
	match String(setting.get("editor", "")):
		"number":
			var unit := String(setting.get("display_unit", ""))
			return String(setting.get("value_text", "")) + ((" " + unit) if unit != "" else "")
		"choice":
			var values: PackedStringArray = setting.get("values", PackedStringArray())
			var titles: PackedStringArray = setting.get("titles", PackedStringArray())
			var at := values.find(String(setting.get("value_json", "")))
			if at >= 0:
				return titles[at]
			return Strings.text("menu_setting_unlisted")
	return Strings.text("menu_setting_unread")


## An enum setting took its `value`th value: the bridge's one row, and the map menus close.
func _on_setting_choice(which: int, pointer: String, value: int) -> void:
	close_menu()
	bridge.editor_change_setting_choice(_visit_beacon, which, pointer, value)


## A Visit & change row on entry `which` of what `row` names: the bridge's one row, and the map
## menus close.
func _on_visit_row(row: String, which: int) -> void:
	close_menu()
	bridge.editor_visit_row(_visit_beacon, row, which)


## A number setting's row was picked: the number field opens on it, as on a number chip.
func _on_setting(which: int) -> void:
	close_menu()
	var rows: Array = bridge.editor_state().get("beacon_detail", {}).get("settings", [])
	if which < 0 or which >= rows.size() or String(rows[which].get("editor", "")) != "number":
		return
	_number_setting = {"beacon": _visit_beacon, "index": which, "pointer": String(rows[which].get("pointer", ""))}
	_open_number({}, rows[which])


## Opens the entry list for Visit & change's row `item` (one of ENTRY_LISTS) on the beacon the
## menu is open for: the vocabulary's entries as sent, beside `beside` (the menu it was picked
## from, in the window's pixels) and inside the window, PANEL_WIDTH wide. Each entry is a row
## with its title; a locked one is greyed, takes no click, and has the gateway's sentence wrapped
## under it (the reason beside the entry, S3's `ed3b` part 2). A pick is the bridge's row.
func open_entry_list(item: int, entries: Array, beside: Rect2) -> void:
	var list: Array = ENTRY_LISTS.get(item, [])
	if list.is_empty():
		return
	_entry_beacon = _visit_beacon
	_entry_row = String(list[0])
	for child in _entry_box.get_children():
		_entry_box.remove_child(child)
		child.queue_free()
	var heading := Label.new()
	heading.text = Strings.text(String(list[2]))
	_entry_box.add_child(heading)
	for which in entries.size():
		_entry_box.add_child(_entry(entries[which], which))
	# Taller than the window, the list scrolls inside a frame the window's height (fix pass 1):
	# the frame is as tall as the entries, and never taller than the window allows.
	var window := get_viewport().get_visible_rect()
	var content := _entry_box.get_combined_minimum_size()
	_entry_scroll.custom_minimum_size = Vector2(PANEL_WIDTH - 12.0, minf(content.y, window.size.y - ENTRY_MARGIN))
	_entry_popup.size = Vector2i(int(PANEL_WIDTH), 0)
	_entry_popup.reset_size()
	_entry_popup.size.x = int(PANEL_WIDTH)
	# Placed beside the menu it was picked from; Godot's Popup keeps itself inside the window
	# (measured: placed past the window's right edge, it came back inside), so no clamp here.
	_entry_popup.popup(Rect2i(Vector2i(int(beside.end.x), int(beside.position.y)), _entry_popup.size))


## One entry's row: a flat button with its title, or, locked, a greyed button that takes no click
## and the gateway's sentence wrapped under it.
func _entry(entry: Dictionary, which: int) -> Control:
	var row := VBoxContainer.new()
	row.set_meta("entry", which)
	var pick := Button.new()
	pick.flat = true
	pick.focus_mode = Control.FOCUS_NONE
	pick.alignment = HORIZONTAL_ALIGNMENT_LEFT
	pick.text = String(entry.get("title", ""))
	pick.accessibility_name = pick.text
	pick.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	pick.custom_minimum_size = Vector2(PANEL_WIDTH - 40.0, 0)
	row.add_child(pick)
	var locked := String(entry.get("locked", ""))
	if locked != "":
		pick.disabled = true
		var why := Label.new()
		why.text = Strings.text("menu_entry_locked", {"locked": locked})
		why.accessibility_name = why.text
		why.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		why.custom_minimum_size = Vector2(PANEL_WIDTH - 40.0, 0)
		why.add_theme_color_override("font_color", LOCKED_COLOUR)
		row.add_child(why)
		row.set_meta("locked", why.text)
	else:
		pick.pressed.connect(func() -> void: _on_entry(which))
	return row


## Entry `which` of the open list was picked: the list closes, and the bridge composes the row.
func _on_entry(which: int) -> void:
	_entry_popup.hide()
	entry_picked.emit(_entry_row, which)
	bridge.editor_visit_row(_entry_beacon, _entry_row, which)


## The entry list's rows as drawn, for a check: each a VBoxContainer with the meta `entry`, and
## `locked` (its sentence as drawn) on a locked one.
func entry_rows() -> Array[Control]:
	var out: Array[Control] = []
	for child in _entry_box.get_children():
		if child.has_meta("entry"):
			out.append(child)
	return out


## The value field for the chip `named` (or, with `named` empty, the Visit & change setting
## `_number_setting` names): the value now as the gateway wrote it, its `value_text` in its
## `display_unit`, never clamped (L1), and the gateway's bounds in the note as sent, in the unit
## they were sent in (`unit`), none where it set none. A value outside its shape is said where the
## gateway's `outside` says so. The field applies no bound: a value past one goes to the gateway,
## whose answer is drawn. Each open releases the field's focus, so the first click selects the old
## value.
func _open_number(named: Dictionary, chip: Dictionary) -> void:
	_number_chip = named
	if not named.is_empty():
		_number_setting = {}
	bridge.editor_field_opened()
	# `outside` is the gateway's word, sent on a rule-list chip; a setting row carries none.
	var outside: bool = chip.get("outside", false)
	var unit := String(chip.get("unit", ""))
	var shown_unit := String(chip.get("display_unit", ""))
	var bounds := {"now": chip.get("value_text", ""), "display_unit": (" " + shown_unit) if shown_unit != "" else "", "unit": (" " + unit) if unit != "" else "", "min": chip.get("min", "-") if chip.get("has_min", false) else "-", "max": chip.get("max", "-") if chip.get("has_max", false) else "-"}
	_number_heading.text = Strings.text("rule_number_heading", {"text": chip.get("text", "")})
	var bounded: bool = chip.get("has_min", false) or chip.get("has_max", false)
	_number_note.visible = outside or bounded
	if outside:
		_number_note.text = Strings.text("rule_number_outside", bounds)
	elif bounded:
		_number_note.text = Strings.text("rule_number_bounds", bounds)
	else:
		_number_note.text = ""
	_number_said.text = ""
	_number_said.visible = false
	# Pre-filled with the gateway's own text of the value, in the unit it is typed in (seconds for
	# a duration): the field converts nothing, and what is typed goes to the gateway as typed.
	_number_field.open(String(chip.get("value_text", "")), shown_unit, String(chip.get("text", "")))
	_number_popup.position = Vector2i(get_viewport().get_mouse_position())
	_number_popup.popup()


## The value field sent `text`, exactly as typed: the bridge sends it to the gateway's
## `set_value_text` (or refuses a text too large to send). The popup stays open and says what became of it
## (`_draw_number_field`), closing once the edit lands.
func _on_number_send(text: String) -> void:
	if not _number_setting.is_empty():
		bridge.editor_change_setting_number_text(String(_number_setting.get("beacon", "")), int(_number_setting.get("index", -1)), String(_number_setting.get("pointer", "")), text)
	else:
		bridge.editor_set_number_text(int(_number_chip.get("index", -1)), int(_number_chip.get("revision", -1)), String(_number_chip.get("pointer", "")), text)
	_draw_number_field(bridge.editor_state().get("field", {}))


## The open value field's last line, from the bridge's `field`: "Sending..." while its edit is on
## its way, the refusal in its own words (this client's or the gateway's), and closed once the edit
## landed. A refusal is drawn where the player is looking, and on the chip's rule after it closes.
func _draw_number_field(field: Dictionary) -> void:
	if not _number_popup.visible:
		return
	match String(field.get("state", "nothing")):
		"waiting":
			_number_said.text = Strings.text("rule_number_waiting")
		"refused":
			_number_said.text = Strings.text(String(field.get("key", "")), {"detail": field.get("detail", "")})
		"landed":
			_number_popup.hide()
			_number_setting = {}
			return
		_:
			_number_said.text = ""
	_number_said.visible = _number_said.text != ""
	_number_said.accessibility_name = _number_said.text


func _on_choice(which: int) -> void:
	bridge.editor_choose(int(_choice_chip.get("index", -1)), int(_choice_chip.get("revision", -1)), String(_choice_chip.get("pointer", "")), which)


## Remove on rule `rule` (its index and pointer in the rendering drawn): the bridge's one `remove`.
func _on_rule_remove(rule: int, pointer: String) -> void:
	bridge.editor_remove_rule(rule, pointer, _rules_rendering)


## Add a rule: the gateway writes an empty one with a fresh id, whose condition the picker fills.
func _on_rule_add() -> void:
	bridge.editor_new_rule()


## A rule dropped on another of its list, each named by its index and pointer, in the
## rendering the drag began on: the bridge's one JSON Patch `move`.
func _on_rule_move(from: int, from_pointer: String, to: int, to_pointer: String, rendering: int) -> void:
	bridge.editor_move_rule(from, from_pointer, to, to_pointer, rendering)


func _draw_notes(state: Dictionary) -> void:
	if state.get("notes_known", false) and not _notes_loaded:
		_notes_loaded = true
		_notes.text = String(state.get("notes", ""))
	var stored := int(state.get("notes_saved", -1))
	if stored >= 0:
		_notes_status.text = Strings.text("status_notes_saved", {"detail": stored})


func _draw_drafts(drafts: Array) -> void:
	if drafts.is_empty():
		_drafts.text = Strings.text("no_drafts")
		return
	var lines: PackedStringArray = []
	for draft in drafts:
		lines.append(Strings.text("draft_row", {"label": draft.get("label", ""), "round": draft.get("round", 0)}))
	_drafts.text = "\n".join(lines)


func _draw_route(route: Dictionary, has_text: bool, unchecked: String) -> void:
	for child in _route_labels.get_children():
		child.queue_free()
	var mesh: ImmediateMesh = _route_mesh.mesh
	mesh.clear_surfaces()
	if not has_text:
		_route_label.text = ""
		return
	if not route.get("current", false):
		_route_label.text = unchecked if unchecked != "" else Strings.text("route_waiting")
	elif String(route.get("found_nothing", "")) != "":
		_route_label.text = Strings.text("route_found_nothing", {"why": route.get("found_nothing", "")})
	elif not route.get("reachable", true):
		_route_label.text = Strings.text("route_none")
	elif (route.get("legs", PackedInt64Array()) as PackedInt64Array).is_empty():
		_route_label.text = Strings.text("route_empty")
	elif String(route.get("whole_text", "")) != "":
		# The gateway's own rendered figure (S3-18): the client formats nothing.
		_route_label.text = Strings.text("route_whole_text", {"text": route.get("whole_text", "")})
	else:
		_route_label.text = Strings.text("route_whole", {"ms": route.get("whole", 0)})
	var points: Array = route.get("points", [])
	var legs: PackedInt64Array = route.get("legs", PackedInt64Array())
	var texts: PackedStringArray = route.get("leg_texts", PackedStringArray())
	if points.size() < 2:
		return
	mesh.surface_begin(Mesh.PRIMITIVE_LINE_STRIP)
	mesh.surface_set_color(ROUTE_COLOUR)
	for point in points:
		mesh.surface_add_vertex(_world(point))
	mesh.surface_end()
	for index in legs.size():
		if index + 1 >= points.size():
			break
		var label := Label3D.new()
		label.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		label.no_depth_test = true
		label.pixel_size = 0.05
		var said := texts[index] if index < texts.size() else ""
		label.text = Strings.text("leg_text", {"text": said}) if said != "" else Strings.text("leg", {"ms": legs[index]})
		label.position = _world(points[index]).lerp(_world(points[index + 1]), 0.5)
		_route_labels.add_child(label)


func _draw_ghost(ghost: Dictionary) -> void:
	if ghost.is_empty():
		_ghost.visible = false
		_ghost_label.text = ""
		# The ghost the ground menu's first line was filled from is gone (an edit landed under it,
		# or the gateway refused it later): the menu goes with it rather than say "waiting" for
		# ever (L4). A menu whose ghost was refused from the start stays, saying why.
		if ghost_went(_menu.visible, _menu.get_item_index(ITEM_GHOST) >= 0, _menu_ghost_at):
			_menu.hide()
			_menu_ghost_at = null
		return
	var at: Vector3i = ghost["at"]
	var state := String(ghost.get("state", "waiting"))
	_ghost.visible = true
	_ghost.position = Vector3(at.x + 0.5, at.z + 4.5, at.y + 0.5)
	var material: StandardMaterial3D = _ghost.material_override
	material.albedo_color = GHOST_COLOURS.get(state, GHOST_COLOURS["waiting"])
	if state == "illegal":
		_ghost_label.text = Strings.text("ghost_illegal", {"sentence": ghost.get("sentence", "")})
	else:
		_ghost_label.text = Strings.text("ghost_" + state)
	_ghost_in_menu(ghost)


## The Attack beacons' targets the text on screen names (S2's plan, task `view`), read by
## the bridge's lax walk of the player's own file: one line each in the panel, a mark on a
## voxel target, and a mark over a named beacon whenever the vista draws one under that
## id. A selector and "the enemy commander" are words only: where they point is the sim's
## to read when the attack runs, never the editor's to guess. Drawing, not deciding: the
## verdict on each target is the verifier's, on the rows.
func _draw_attack(targets: Array) -> void:
	for child in _attack_marks.get_children():
		child.queue_free()
	_attack_beacons = {}
	var lines: PackedStringArray = []
	for target in targets:
		var aim := String(target.get("aim", "unknown"))
		var step := Values.said(Strings.text("value_step"), target.get("step", 0))
		if aim == "voxel":
			var at: Vector3i = target["at"]
			lines.append(Strings.text("attack_voxel", {"step": step, "x": at.x, "y": at.y, "z": at.z}))
			var mark := _attack_mark(Strings.text("attack_mark", {"step": step}))
			mark.position = _world(at) + Vector3(0.0, ATTACK_MARK_LIFT, 0.0)
			_attack_marks.add_child(mark)
		elif aim == "beacon":
			var id := String(target.get("id", ""))
			lines.append(Strings.text("attack_beacon", {"step": step, "id": id}))
			var mark := _attack_mark(Strings.text("attack_mark", {"step": step}))
			mark.visible = false
			_attack_marks.add_child(mark)
			_attack_beacons[id] = mark
		else:
			lines.append(Strings.text("attack_" + aim, {"step": step}))
	_attack_label.text = "\n".join(lines) if not lines.is_empty() else Strings.text("attack_none")
	_attack_label.accessibility_name = _attack_label.text
	_place_attack_marks()


## The Build targets the text on screen writes (S3's `fixp`, P6b finding 7), read by the bridge's
## lax walk of the player's own file: one line each in the panel, "step N: <blueprint as
## written>", a mark at a voxel anchor or over the vent an `on` anchor names (where
## `get_map_summary` lists it this Lull), and a wall's box drawn as the outline of its two corners
## as written. It decides nothing: an anchor it cannot read, or a vent it does not list, is not
## drawn, and its line says so. The verdict on each target is the verifier's, on the rows.
func _draw_build(targets: Array) -> void:
	for child in _build_marks.get_children():
		child.queue_free()
	var boxes: ImmediateMesh = _build_boxes.mesh
	boxes.clear_surfaces()
	var vents := {}
	for feature in bridge.editor_features():
		vents[String(feature.get("id", ""))] = feature["at"]
	var lines: PackedStringArray = []
	for target in targets:
		var words := {"step": Values.said(Strings.text("value_step"), target.get("step", 0)), "blueprint": target.get("blueprint", "")}
		var anchor := String(target.get("anchor", "unknown"))
		var at: Variant = null
		if anchor == "voxel":
			at = target["at"]
			lines.append(Strings.text("build_voxel", words.merged({"x": at.x, "y": at.y, "z": at.z})))
		elif anchor == "on" and vents.has(String(target.get("feature", ""))):
			at = vents[String(target["feature"])]
			lines.append(Strings.text("build_on", words.merged({"feature": target.get("feature", "")})))
		elif anchor == "on":
			lines.append(Strings.text("build_on_unlisted", words.merged({"feature": target.get("feature", "")})))
		else:
			lines.append(Strings.text("build_unknown", words))
		if at != null:
			var mark := _attack_mark(Strings.text("build_mark", words))
			mark.modulate = BUILD_COLOUR
			mark.position = _world(at) + Vector3(0.0, BUILD_MARK_LIFT, 0.0)
			_build_marks.add_child(mark)
		if target.get("has_box", false):
			_box_outline(boxes, target["box_min"], target["box_max"])
	_build_label.text = "\n".join(lines) if not lines.is_empty() else Strings.text("build_none")
	_build_label.accessibility_name = _build_label.text


## The outline of the box whose corners are the voxels `low` and `high` (inclusive, as written,
## sim axes): its twelve edges, around the voxels' faces. A drawing of what the file says.
static func _box_outline(mesh: ImmediateMesh, low: Vector3i, high: Vector3i) -> void:
	var a := Vector3(mini(low.x, high.x), mini(low.z, high.z), mini(low.y, high.y))
	var b := Vector3(maxi(low.x, high.x) + 1, maxi(low.z, high.z) + 1, maxi(low.y, high.y) + 1)
	var corners := [Vector3(a.x, a.y, a.z), Vector3(b.x, a.y, a.z), Vector3(b.x, a.y, b.z), Vector3(a.x, a.y, b.z), Vector3(a.x, b.y, a.z), Vector3(b.x, b.y, a.z), Vector3(b.x, b.y, b.z), Vector3(a.x, b.y, b.z)]
	mesh.surface_begin(Mesh.PRIMITIVE_LINES)
	mesh.surface_set_color(BUILD_COLOUR)
	for edge in [[0, 1], [1, 2], [2, 3], [3, 0], [4, 5], [5, 6], [6, 7], [7, 4], [0, 4], [1, 5], [2, 6], [3, 7]]:
		mesh.surface_add_vertex(corners[edge[0]])
		mesh.surface_add_vertex(corners[edge[1]])
	mesh.surface_end()


## The Build targets' marks as drawn, for a check to count.
func build_marks() -> Array[Node]:
	return _build_marks.get_children().filter(func(mark: Node) -> bool: return not mark.is_queued_for_deletion())


## The Build targets' lines as drawn.
func build_text() -> String:
	return _build_label.text


## Puts each named beacon's mark over the vista's marker for that id, and hides it while
## the vista draws none: the view is what says the beacon is in sight.
func _place_attack_marks() -> void:
	if vista == null:
		return
	for id in _attack_beacons:
		var mark: Label3D = _attack_beacons[id]
		var marker: Node3D = vista.marker_node(String(id))
		mark.visible = marker != null
		if marker != null:
			mark.global_position = marker.global_position + Vector3(0.0, ATTACK_BEACON_LIFT, 0.0)


func _attack_mark(text: String) -> Label3D:
	var mark := Label3D.new()
	mark.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	mark.no_depth_test = true
	mark.pixel_size = 0.06
	mark.modulate = ATTACK_COLOUR
	mark.text = text
	return mark


## A voxel in the sim's axes (x east, y north, z up) as a point in the world's (x, y up,
## z north), at the middle of the voxel and lifted clear of the ground.
func _world(voxel: Vector3i) -> Vector3:
	return Vector3(voxel.x + 0.5, voxel.z + ROUTE_LIFT, voxel.y + 0.5)


# --- Building ----------------------------------------------------------------------------

func _build_panel() -> void:
	_panel = PanelContainer.new()
	_panel.anchor_left = 1.0
	_panel.anchor_right = 1.0
	_panel.anchor_bottom = 1.0
	_panel.offset_left = -PANEL_WIDTH
	# Should its contents ever ask for more than the panel's width, it widens to the left, over
	# the map, and never off the window's right edge (items 139 (2) D, 155 (3) 1).
	_panel.grow_horizontal = Control.GROW_DIRECTION_BEGIN
	_panel.mouse_filter = Control.MOUSE_FILTER_STOP
	add_child(_panel)
	var scroll := ScrollContainer.new()
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	_panel.add_child(scroll)
	var column := VBoxContainer.new()
	column.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	scroll.add_child(column)

	# The title's row holds the size meter, so the meter stays in sight however many rows the
	# checks open, and adds no height, so the Fix buttons keep their place.
	var title_row := HBoxContainer.new()
	column.add_child(title_row)
	_heading(title_row, "editor_title")
	_size_box = HBoxContainer.new()
	_size_box.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_size_box.visible = false
	title_row.add_child(_size_box)
	_size_bar = ProgressBar.new()
	_size_bar.min_value = 0.0
	_size_bar.step = 1.0
	_size_bar.show_percentage = false
	_size_bar.custom_minimum_size = SIZE_BAR
	_size_bar.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	_size_box.add_child(_size_bar)
	_size_line = Label.new()
	_size_line.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_size_line.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_size_box.add_child(_size_line)
	var buttons := HBoxContainer.new()
	column.add_child(buttons)
	_new_button = _button(buttons, "new", new_playbook)
	_button(buttons, "load", open_load)
	_count(_button(buttons, "save", _save), "save")
	_button(buttons, "save_as", open_save_as)
	_undo_button = _button(buttons, "undo", func() -> void: bridge.editor_undo())
	_count(_button(buttons, "submit", _on_submit_pressed), "submit")
	_status = _label(column)
	_verdict = _label(column)
	_verdict.text = Strings.text("verdict_none")
	_beacon_label = _label(column)
	_ghost_label = _label(column)

	_heading(column, "meter_heading")
	_meter = _label(column)
	_meter.text = Strings.text("meter_waiting")

	_heading(column, "templates_heading")
	_templates_box = VBoxContainer.new()
	column.add_child(_templates_box)
	_wizard_box = VBoxContainer.new()
	column.add_child(_wizard_box)

	_heading(column, "checks_heading")
	_rows = VBoxContainer.new()
	column.add_child(_rows)
	_dormant = CheckBox.new()
	_dormant.text = Strings.text("dormant_option")
	_dormant.accessibility_name = _dormant.text
	_dormant.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_dormant.custom_minimum_size = Vector2(PANEL_WIDTH - 40.0, 0)
	_dormant.focus_mode = Control.FOCUS_NONE
	_dormant.add_theme_icon_override("unchecked", ImageTexture.create_from_image(unticked_box(BOX_COLOUR)))
	_dormant.add_theme_icon_override("unchecked_disabled", ImageTexture.create_from_image(unticked_box(BOX_COLOUR_DISABLED)))
	_dormant.visible = false
	_dormant.toggled.connect(_on_dormant_toggled)
	_dormant.button_down.connect(_on_dormant_down)
	column.add_child(_dormant)
	_dormant_said = _label(column)
	_dormant_said.visible = false

	_heading(column, "rules_heading")
	_rules_status = _label(column)
	_rules_box = VBoxContainer.new()
	_rules_box.draw.connect(func() -> void: RuleList.draw_connectors(_rules_box))
	column.add_child(_rules_box)

	_heading(column, "route_heading")
	_route_label = _label(column)

	_heading(column, "attack_heading")
	_attack_label = _label(column)

	_heading(column, "build_heading")
	_build_label = _label(column)

	_heading(column, "chips_heading")
	_chips = _label(column)

	# Below the checks and the route, so the rows and their Fix buttons keep their place at
	# the panel's top (F1's first-click check clicks them unscrolled).
	_this_round_box = VBoxContainer.new()
	_this_round_box.visible = false
	column.add_child(_this_round_box)
	_heading(_this_round_box, "this_round_heading")
	_this_round = _label(_this_round_box)

	_heading(column, "forecast_heading")
	_forecast = _label(column)

	_heading(column, "notes_heading")
	_notes = TextEdit.new()
	_notes.placeholder_text = Strings.text("notes_hint")
	_notes.accessibility_name = Strings.text("notes_heading")
	_notes.custom_minimum_size = Vector2(0, 90)
	_notes.wrap_mode = TextEdit.LINE_WRAPPING_BOUNDARY
	column.add_child(_notes)
	_button(column, "save_notes", func() -> void: bridge.editor_save_notes(_notes.text))
	# Under its button: a text field's full width beside a button is wider than the panel, which
	# is what pushed the whole panel off the window's right edge (finding D).
	_notes_status = _label(column)

	_heading(column, "drafts_heading")
	_drafts = _label(column)
	_button(column, "save_draft", func() -> void: bridge.editor_save_draft(Strings.text("draft_label")))

	_load_dialog = _file_dialog(FileDialog.FILE_MODE_OPEN_FILE)
	_load_dialog.file_selected.connect(func(path: String) -> void: load_file(path))
	_save_dialog = _file_dialog(FileDialog.FILE_MODE_SAVE_FILE)
	_save_dialog.file_selected.connect(func(path: String) -> void: save_file(path))


func _build_menus() -> void:
	_menu = PopupMenu.new()
	opaque(_menu)
	_menu.hide_on_checkable_item_selection = false
	_menu.id_pressed.connect(_on_menu)
	_menu.window_input.connect(close_menu_on_escape)
	add_child(_menu)
	_visit_menu = PopupMenu.new()
	opaque(_visit_menu)
	# Filled per beacon (`_fill_visit_menu`): the priority rows, then the detail's.
	_visit_menu.add_item(Strings.text("menu_priority_low"), ITEM_LOW)
	_visit_menu.add_item(Strings.text("menu_priority_normal"), ITEM_NORMAL)
	_visit_menu.add_item(Strings.text("menu_priority_high"), ITEM_HIGH)
	_visit_menu.id_pressed.connect(_on_menu)
	_visit_menu.window_input.connect(close_menu_on_escape)
	_menu.add_child(_visit_menu)
	# The rule list's chip editors (S3's `ed3b`).
	_choice_menu = PopupMenu.new()
	opaque(_choice_menu)
	_choice_menu.id_pressed.connect(_on_choice)
	add_child(_choice_menu)
	_name_menu = PopupMenu.new()
	opaque(_name_menu)
	_name_menu.id_pressed.connect(_on_name)
	add_child(_name_menu)
	_build_picker()
	_entry_popup = PopupPanel.new()
	opaque(_entry_popup)
	_entry_scroll = ScrollContainer.new()
	_entry_scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	_entry_popup.add_child(_entry_scroll)
	_entry_box = VBoxContainer.new()
	_entry_box.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_entry_scroll.add_child(_entry_box)
	add_child(_entry_popup)
	_number_popup = PopupPanel.new()
	opaque(_number_popup)
	var box := VBoxContainer.new()
	_number_popup.add_child(box)
	_number_heading = Label.new()
	box.add_child(_number_heading)
	_number_note = Label.new()
	_number_note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_number_note.custom_minimum_size = Vector2(PANEL_WIDTH - 40.0, 0)
	_number_note.visible = false
	box.add_child(_number_note)
	_number_field = ValueField.new(Strings.text("rule_number_send"))
	_number_field.sent.connect(_on_number_send)
	box.add_child(_number_field)
	_number_said = Label.new()
	_number_said.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_number_said.custom_minimum_size = Vector2(PANEL_WIDTH - 40.0, 0)
	_number_said.visible = false
	box.add_child(_number_said)
	add_child(_number_popup)


## The condition picker's window: a heading, the line that says why the words are not on hand,
## the tree of families, predicates and parameters, and Use.
func _build_picker() -> void:
	_picker_popup = PopupPanel.new()
	opaque(_picker_popup)
	var box := VBoxContainer.new()
	_picker_popup.add_child(box)
	var heading := Label.new()
	heading.text = Strings.text("picker_heading")
	box.add_child(heading)
	_picker_line = Label.new()
	_picker_line.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_picker_line.custom_minimum_size = Vector2(PANEL_WIDTH - 40.0, 0)
	box.add_child(_picker_line)
	_picker_again = Button.new()
	_picker_again.text = Strings.text("vocabulary_ask_again")
	_picker_again.accessibility_name = _picker_again.text
	_picker_again.visible = false
	_picker_again.pressed.connect(func() -> void: bridge.editor_vocabulary_again())
	box.add_child(_picker_again)
	_picker_tree = Tree.new()
	_picker_tree.custom_minimum_size = Vector2(PANEL_WIDTH, 320)
	box.add_child(_picker_tree)
	var use := Button.new()
	use.text = Strings.text("picker_use")
	use.accessibility_name = use.text
	use.pressed.connect(_on_picker_use)
	box.add_child(use)
	add_child(_picker_popup)


## Gives `popup` the editor's one popup panel: opaque, POPUP_COLOUR.
static func opaque(popup: Window) -> void:
	var panel := StyleBoxFlat.new()
	panel.bg_color = POPUP_COLOUR
	panel.draw_center = true
	panel.set_content_margin_all(6.0)
	popup.add_theme_stylebox_override("panel", panel)


## The popups the editor owns, for a check to read their panels.
func popups() -> Array[Window]:
	var out: Array[Window] = [_number_popup, _picker_popup, _entry_popup, _choice_menu, _name_menu]
	out.append_array(_map_menus())
	return out


## An unticked checkbox's box: a square outline in `colour`, clear inside.
static func unticked_box(colour: Color) -> Image:
	var box := Image.create_empty(BOX_SIDE, BOX_SIDE, false, Image.FORMAT_RGBA8)
	box.fill_rect(Rect2i(0, 0, BOX_SIDE, BOX_SIDE), colour)
	box.fill_rect(Rect2i(BOX_LINE, BOX_LINE, BOX_SIDE - BOX_LINE * 2, BOX_SIDE - BOX_LINE * 2), Color(0.0, 0.0, 0.0, 0.0))
	return box


func _build_map_nodes() -> void:
	_route_mesh = MeshInstance3D.new()
	_route_mesh.mesh = ImmediateMesh.new()
	var line := StandardMaterial3D.new()
	line.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	line.vertex_color_use_as_albedo = true
	line.no_depth_test = true
	_route_mesh.material_override = line
	vista.add_child(_route_mesh)
	_route_labels = Node3D.new()
	vista.add_child(_route_labels)
	_attack_marks = Node3D.new()
	vista.add_child(_attack_marks)
	_build_marks = Node3D.new()
	vista.add_child(_build_marks)
	_build_boxes = MeshInstance3D.new()
	_build_boxes.mesh = ImmediateMesh.new()
	_build_boxes.material_override = line
	vista.add_child(_build_boxes)
	_ghost = MeshInstance3D.new()
	var box := BoxMesh.new()
	box.size = Vector3(2.0, 9.0, 2.0)
	_ghost.mesh = box
	var glass := StandardMaterial3D.new()
	glass.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	glass.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	_ghost.material_override = glass
	_ghost.visible = false
	vista.add_child(_ghost)


## Save: to the file the text on screen was opened from or last saved to, or, when it came
## from no file (a template, a carried draft), through the Save as... dialog (finding G: Save
## used to write the last file saved, whatever the text on screen was).
func _save() -> void:
	_counted("save")
	var file := text_file()
	if file == "":
		open_save_as()
	else:
		save_file(file)


func _on_submit_pressed() -> void:
	_counted("submit")
	submit()


func _on_fix_pressed(row: int, which: int) -> void:
	_counted("fix")
	fix(row, which)


func _on_fix_down(row: int, which: int) -> void:
	_pressed_down("fix")
	if log_input:
		print("[editor] Fix (row %d, fix %d): button_down" % [row, which])


## Counts, and with `--log-input` logs, a press on `button`, which the editor knows as `name`.
func _count(button: Button, name: String) -> void:
	_named[name] = button
	button.button_down.connect(func() -> void: _pressed_down(name))


func _pressed_down(name: String) -> void:
	_clicks[name]["down"] += 1
	if log_input and name != "fix":
		print("[editor] %s: button_down" % name)


func _counted(name: String) -> void:
	_clicks[name]["pressed"] += 1
	if log_input:
		print("[editor] %s: pressed" % name)


func _say_local(key: String, args: Dictionary) -> void:
	_status.text = Strings.text(key, args)


func _heading(parent: Node, key: String) -> void:
	var label := Label.new()
	label.text = Strings.text(key)
	label.add_theme_font_size_override("font_size", 17)
	parent.add_child(label)


func _label(parent: Node) -> Label:
	var label := Label.new()
	label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	label.custom_minimum_size = Vector2(PANEL_WIDTH - 40.0, 0)
	parent.add_child(label)
	return label


func _button(parent: Node, key: String, pressed: Callable) -> Button:
	var button := Button.new()
	button.text = Strings.text(key)
	button.accessibility_name = button.text
	button.focus_mode = Control.FOCUS_NONE
	button.pressed.connect(pressed)
	parent.add_child(button)
	return button


func _file_dialog(mode: FileDialog.FileMode) -> FileDialog:
	var dialog := FileDialog.new()
	dialog.file_mode = mode
	dialog.access = FileDialog.ACCESS_FILESYSTEM
	dialog.filters = PackedStringArray([Strings.text("file_filter")])
	dialog.use_native_dialog = true
	add_child(dialog)
	return dialog

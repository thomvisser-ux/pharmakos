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
#     PLACEHOLDER: a live-on-hover legality ghost is S3/S6's (w6 notes A4 item 5), OWNER;
#   * Alt-click a beacon: the step's target becomes a selector - nearest, weakest, safest
#     or most threatened own beacon, chosen when the step starts;
#   * the route is drawn as a polyline from the gateway's `estimate_route` legs, each leg
#     labelled with its travel time. There are NO dashed legs at the skeleton: the estimator
#     prices every leg over the whole generated map and the view agrees
#     (skeleton-plan-t16a-notes.md section B, "T19" (2)).
#
# Files: Load and Save read and write the player's own JSONC file, byte for byte. The only
# files the client writes at all are that JSONC file, the lobby's one remembered config
# line (`user://`, no token; scripts/lobby.gd) and the headless watch check's own `user://`
# files. A file is opened only after QUICK has seen it; an out-of-vocabulary construct is
# refused with the verifier's code and pointer and nothing is stripped (spec section 13).
#
# Pull request 2 (skeleton-plan-w6-notes.md section A4, as decisions-log item 112 amends
# it) adds, all drawn as the gateway answered:
#   * the TEMPLATE WIZARD (scripts/wizard.gd): the templates `list_templates` offers, and
#     for the one opened, one page per parameter `instantiate_template{suggested: true}`
#     lists - label, raw value, the operator's mark and its why. Use puts the gateway's
#     playbook into the editor byte for byte, as one edit Undo takes back;
#   * the RULE LIST (scripts/rule_list.gd): `render_plan`'s lines for the text on screen;
#   * the own $/kW METER: `get_economy_forecast`'s four numbers, as they came, through
#     strings.gd's frame. Headroom is the gateway's, never supply minus draw. When it is
#     read is the bridge's rig's scheduling (crates/client-gdext/src/rig.rs).
#     PLACEHOLDER: the meter's layout and refresh cadence, Tuning, OWNER.
#   * draft continuity through `get_draft`: the carried draft is fetched and opened every
#     Lull (the bridge's editor).
#
# PLACEHOLDER: the panel's layout, sizes and colours, the menu's wording and the ghost's
# look are the skeleton's; the real editor's layout is S6's, OWNER (skeleton plan T19).
#
# PLACEHOLDER: the segment clock and the "fits" pill are S3/S6's and are not built
# (skeleton plan T19, PLACEHOLDERs line), OWNER. So is a rendered travel time: legs and the
# whole route are shown as the raw game milliseconds the estimator answered, not the
# generously rounded or whole-second figure decisions-log items 57 and 61 ask for, until the
# gateway answers one (see strings.gd's `leg`), OWNER with plan-core/T18a, S3.

extends CanvasLayer

const Strings := preload("res://scripts/strings.gd")
const Rows := preload("res://scripts/rows.gd")
const Wizard := preload("res://scripts/wizard.gd")
const RuleList := preload("res://scripts/rule_list.gd")

## How close, in screen pixels, a click must land to a beacon to pick it.
## PLACEHOLDER: UI, OWNER at S6.
const PICK_PIXELS := 28.0
## The panel's width in pixels. PLACEHOLDER: layout, OWNER at S6.
const PANEL_WIDTH := 380.0
## The route's colour, and the ghost's by verdict. PLACEHOLDER: art, OWNER at S6.
const ROUTE_COLOUR := Color(0.95, 0.85, 0.35)
const GHOST_COLOURS := {
	"waiting": Color(0.8, 0.8, 0.8, 0.45),
	"legal": Color(0.3, 0.9, 0.4, 0.5),
	"illegal": Color(0.95, 0.25, 0.2, 0.5),
}
## How far above the ground the route is drawn, in voxels. PLACEHOLDER: art, OWNER at S6.
const ROUTE_LIFT := 1.2

## Menu item ids.
const ITEM_GO := 1
const ITEM_RECYCLE := 2
const ITEM_PLACE := 3
const ITEM_LOW := 11
const ITEM_NORMAL := 12
const ITEM_HIGH := 13
const ITEM_SELECTOR := 20
const SELECTORS := ["nearest", "weakest", "safest", "most_threatened"]

var bridge: Node = null
var vista: Node3D = null

## The file the player opened or last saved, if any.
var file_path := ""

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
var _load_dialog: FileDialog
var _save_dialog: FileDialog
var _menu: PopupMenu
var _visit_menu: PopupMenu
var _menu_target := {}
var _selector := ""
var _route_mesh: MeshInstance3D
var _route_labels: Node3D
var _ghost: MeshInstance3D
var _notes_loaded := false
var _meter: Label
var _meter_answers := -1
var _templates_box: VBoxContainer
var _wizard_box: VBoxContainer
var _rules_status: Label
var _rules_box: VBoxContainer
var _templates_drawn := ""
var _wizard_drawn := ""
var _rules_drawn := ""


## Builds the panel, the menus and the map's drawing nodes. `vista` is the scene's vista,
## whose bridge and camera the editor uses.
func setup(the_vista: Node3D) -> void:
	vista = the_vista
	bridge = vista.bridge
	_build_panel()
	_build_menus()
	_build_map_nodes()


## Starts the editor for `seat` (spelt as the gateway spells a seat). Call after the host
## link has started the bridge's watch rig.
func begin(seat: String) -> void:
	bridge.editor_seat(seat)


## Opens the file at `path`: its bytes go to QUICK first, and the editor takes it only if
## the verifier found nothing out of vocabulary.
func load_file(path: String) -> bool:
	var bytes := FileAccess.get_file_as_bytes(path)
	if bytes.is_empty() and FileAccess.get_open_error() != OK:
		_say_local("status_open_failed", {"path": path})
		return false
	file_path = path
	return load_bytes(bytes)


## Opens a playbook from its bytes.
func load_bytes(bytes: PackedByteArray) -> bool:
	return bridge.editor_load(bytes)


## Writes the playbook on screen to `path`, byte for byte.
func save_file(path: String) -> bool:
	var bytes: PackedByteArray = bridge.editor_bytes()
	var file := FileAccess.open(path, FileAccess.WRITE)
	if file == null:
		_say_local("status_save_failed", {"path": path})
		return false
	file.store_buffer(bytes)
	file.close()
	file_path = path
	_say_local("status_saved_file", {"path": path})
	return true


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


## Closes the map menu, as a click elsewhere would.
func close_menu() -> void:
	_menu.hide()
	_visit_menu.hide()


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


## The meter as drawn.
func meter_text() -> String:
	return _meter.text


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
	if state.is_empty() or int(state.get("changes", 0)) == _changes:
		return
	_changes = int(state.get("changes", 0))
	_draw_state(state)


func _process(_delta: float) -> void:
	if bridge != null:
		refresh()


func _unhandled_input(event: InputEvent) -> void:
	if vista == null or not (event is InputEventMouseButton):
		return
	if not event.pressed or event.button_index != MOUSE_BUTTON_LEFT:
		return
	var camera: Camera3D = vista.rig.camera
	var beacon := _beacon_under(camera, event.position)
	if not beacon.is_empty():
		_on_beacon_clicked(beacon, event.alt_pressed, event.position)
		get_viewport().set_input_as_handled()
		return
	var picked: Dictionary = bridge.view_pick(camera.project_ray_origin(event.position), camera.project_ray_normal(event.position))
	if picked.get("hit", false):
		_on_ground_clicked(picked["at"], event.position)
		get_viewport().set_input_as_handled()


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
	_selector = SELECTORS[0] if alt else ""
	_menu_target = {"beacon": id}
	_menu.clear()
	if alt:
		_menu.add_separator(Strings.text("menu_target"))
		for index in SELECTORS.size():
			var name: String = SELECTORS[index]
			_menu.add_radio_check_item(Strings.text("selector_" + name), ITEM_SELECTOR + index)
			_menu.set_item_checked(_menu.get_item_index(ITEM_SELECTOR + index), index == 0)
		_menu.add_separator()
	_menu.add_submenu_node_item(Strings.text("menu_visit"), _visit_menu)
	_menu.add_item(Strings.text("menu_go"), ITEM_GO)
	_menu.add_item(Strings.text("menu_recycle"), ITEM_RECYCLE)
	_menu.position = Vector2i(at)
	_menu.popup()


func _on_ground_clicked(at: Vector3i, where: Vector2) -> void:
	_menu_target = {"voxel": at}
	_selector = ""
	_beacon_label.text = ""
	# The ghost for this click: the draft with a beacon patched in here, checked by QUICK.
	bridge.editor_preview_place(at)
	_menu.clear()
	_menu.add_item(Strings.text("menu_go"), ITEM_GO)
	_menu.add_item(Strings.text("menu_place"), ITEM_PLACE)
	_menu.position = Vector2i(where)
	_menu.popup()


func _on_menu(id: int) -> void:
	if id >= ITEM_SELECTOR and id < ITEM_SELECTOR + SELECTORS.size():
		_selector = SELECTORS[id - ITEM_SELECTOR]
		for index in SELECTORS.size():
			_menu.set_item_checked(_menu.get_item_index(ITEM_SELECTOR + index), SELECTORS[index] == _selector)
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
	if state.get("busy", false) or (state.get("has_text", false) and not state.get("rows_current", false) and verdict != "refused"):
		line = Strings.text("verdict_checking")
	elif verdict != "none" and verdict != "refused":
		line += " - " + Strings.text("qualifies_yes" if state.get("qualifies", false) else "qualifies_no")
	_verdict.text = line
	Rows.fill(_rows, state.get("rows", []), Callable(self, "fix"), state.get("rows_current", false) and not state.get("busy", false))
	_undo_button.disabled = int(state.get("undo_depth", 0)) == 0
	if state.get("beacon_prose", "") != "":
		_beacon_label.text = String(state["beacon_prose"])
	_draw_notes(state)
	_draw_drafts(state.get("drafts", []))
	_draw_route(state.get("route", {}), state.get("has_text", false))
	_draw_ghost(state.get("ghost", {}))
	_draw_templates(state.get("templates", []))
	_draw_wizard(state.get("wizard", {}), state.get("templates", []))
	_draw_rules(state.get("prose", PackedStringArray()), state.get("prose_current", false), state.get("has_text", false))


## The meter: the gateway's four numbers put into strings.gd's frame as they came. Redrawn
## only when a new answer has come.
func _draw_meter(meter: Dictionary) -> void:
	if meter.is_empty() or int(meter.get("answers", 0)) == _meter_answers:
		return
	_meter_answers = int(meter.get("answers", 0))
	if _meter_answers == 0:
		_meter.text = Strings.text("meter_waiting")
	else:
		_meter.text = Strings.text("meter", {"treasury": meter.get("treasury_now"), "supply": meter.get("supply_kw_now"), "draw": meter.get("draw_kw_now"), "headroom": meter.get("headroom_kw_now")})
	_meter.accessibility_name = _meter.text


func _draw_templates(templates: Array) -> void:
	var drawn := var_to_str(templates)
	if drawn == _templates_drawn:
		return
	_templates_drawn = drawn
	for child in _templates_box.get_children():
		_templates_box.remove_child(child)
		child.queue_free()
	if templates.is_empty():
		var none := Label.new()
		none.text = Strings.text("no_templates")
		_templates_box.add_child(none)
		return
	for template in templates:
		var id := String(template.get("id", ""))
		var button := _button(_templates_box, "", func() -> void: open_template(id))
		button.text = Strings.text("template_button", {"title": template.get("title", "")})
		button.accessibility_name = button.text
		button.tooltip_text = String(template.get("summary", ""))
		button.set_meta("id", id)


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


func _draw_rules(lines: PackedStringArray, current: bool, has_text: bool) -> void:
	_rules_status.text = Strings.text("rules_waiting") if has_text and not current else ""
	var drawn := var_to_str(lines)
	if drawn == _rules_drawn:
		return
	_rules_drawn = drawn
	RuleList.fill(_rules_box, lines)


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


func _draw_route(route: Dictionary, has_text: bool) -> void:
	for child in _route_labels.get_children():
		child.queue_free()
	var mesh: ImmediateMesh = _route_mesh.mesh
	mesh.clear_surfaces()
	if not has_text:
		_route_label.text = ""
		return
	if not route.get("current", false):
		_route_label.text = Strings.text("route_waiting")
	elif not route.get("reachable", true):
		_route_label.text = Strings.text("route_none")
	elif (route.get("legs", PackedInt64Array()) as PackedInt64Array).is_empty():
		_route_label.text = Strings.text("route_empty")
	else:
		_route_label.text = Strings.text("route_whole", {"ms": route.get("whole", 0)})
	var points: Array = route.get("points", [])
	var legs: PackedInt64Array = route.get("legs", PackedInt64Array())
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
		label.text = Strings.text("leg", {"ms": legs[index]})
		label.position = _world(points[index]).lerp(_world(points[index + 1]), 0.5)
		_route_labels.add_child(label)


func _draw_ghost(ghost: Dictionary) -> void:
	if ghost.is_empty():
		_ghost.visible = false
		_ghost_label.text = ""
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
	_panel.mouse_filter = Control.MOUSE_FILTER_STOP
	add_child(_panel)
	var scroll := ScrollContainer.new()
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	_panel.add_child(scroll)
	var column := VBoxContainer.new()
	column.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	scroll.add_child(column)

	_heading(column, "editor_title")
	var buttons := HBoxContainer.new()
	column.add_child(buttons)
	_button(buttons, "load", func() -> void: _load_dialog.popup_centered_ratio(0.6))
	_button(buttons, "save", _save)
	_button(buttons, "save_as", func() -> void: _save_dialog.popup_centered_ratio(0.6))
	_undo_button = _button(buttons, "undo", func() -> void: bridge.editor_undo())
	_button(buttons, "submit", submit)
	_status = _label(column)
	_verdict = _label(column)
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

	_heading(column, "rules_heading")
	_rules_status = _label(column)
	_rules_box = VBoxContainer.new()
	column.add_child(_rules_box)

	_heading(column, "route_heading")
	_route_label = _label(column)

	_heading(column, "notes_heading")
	_notes = TextEdit.new()
	_notes.placeholder_text = Strings.text("notes_hint")
	_notes.accessibility_name = Strings.text("notes_heading")
	_notes.custom_minimum_size = Vector2(0, 90)
	_notes.wrap_mode = TextEdit.LINE_WRAPPING_BOUNDARY
	column.add_child(_notes)
	var notes_row := HBoxContainer.new()
	column.add_child(notes_row)
	_button(notes_row, "save_notes", func() -> void: bridge.editor_save_notes(_notes.text))
	_notes_status = _label(notes_row)

	_heading(column, "drafts_heading")
	_drafts = _label(column)
	_button(column, "save_draft", func() -> void: bridge.editor_save_draft(Strings.text("draft_label")))

	_load_dialog = _file_dialog(FileDialog.FILE_MODE_OPEN_FILE)
	_load_dialog.file_selected.connect(func(path: String) -> void: load_file(path))
	_save_dialog = _file_dialog(FileDialog.FILE_MODE_SAVE_FILE)
	_save_dialog.file_selected.connect(func(path: String) -> void: save_file(path))


func _build_menus() -> void:
	_menu = PopupMenu.new()
	_menu.hide_on_checkable_item_selection = false
	_menu.id_pressed.connect(_on_menu)
	add_child(_menu)
	_visit_menu = PopupMenu.new()
	_visit_menu.add_item(Strings.text("menu_priority_low"), ITEM_LOW)
	_visit_menu.add_item(Strings.text("menu_priority_normal"), ITEM_NORMAL)
	_visit_menu.add_item(Strings.text("menu_priority_high"), ITEM_HIGH)
	_visit_menu.id_pressed.connect(_on_menu)
	_menu.add_child(_visit_menu)


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


func _save() -> void:
	if file_path == "":
		_save_dialog.popup_centered_ratio(0.6)
	else:
		save_file(file_path)


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

# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The rule list (T19 pull request 2; S3's `ed3b`, spec section 13): `render_plan`'s prose for
# the playbook on screen, one row per line, each drawn exactly as it came, its accessible
# name its line (spec section 13's "accessible names generated from the rendered sentences").
#
# NO SENTENCE IS PARSED HERE. The line layout is the gateway's contract
# (`gp.api.v1.RenderPlanResponse`: one rule, step or block per line, the indent saying what a
# line is), and so are the chips: each line arrives from the bridge already cut into runs of
# plain text and chips, at the spans `render_plan` placed (crates/client-gdext/src/rule_list.rs).
# This script draws the runs and forwards the clicks:
#   * a CHIP is drawn as a link in its sentence; a click on it asks the editor for its editor,
#     which the chip's shape names (the value field with the bounds sent said beside it, an enum's values
#     as sent, a pick on the map, or, for a step label or a rule id, the names the playbook itself
#     holds, in file order). A chip the rule list does not edit (a duration, which waits on a
#     display unit from the wire) is drawn dim, and a click says why;
#   * a rule's or a step's sentence row can be DRAGGED onto another of the same list: the drop
#     asks the bridge for one JSON Patch `move` of it to that rule's place;
#   * the CONFLICT CONNECTOR joins the two rules a W0702 row names, a line in the list's left
#     margin from the earlier rule to the later one, with the verifier's sentence under the
#     later rule;
#   * a refusal of a rule-list edit is shown under the rule it was of: the gateway's in its
#     words, this client's (a value field's text that is no whole number, a value it already
#     holds) in the status line's (S3's `fixp`).
# The controls take a click only while the bridge says the rule list describes the text on
# screen, no edit is on its way and the door is open (`rules_editable`), as Fix buttons do.
#
# PLACEHOLDER: the rule list's look — OWNER, at S6.

extends RefCounted

const Strings := preload("res://scripts/strings.gd")
const Values := preload("res://scripts/values.gd")

## The width a line wraps at. PLACEHOLDER: the wrap width — OWNER, at S6.
const LINE_WIDTH := 340.0
## The left margin the connector is drawn in, its line's x in it, and its colour and width.
const MARGIN := 12.0
const CONNECTOR_X := 4.0
const CONNECTOR_COLOUR := Color(1.0, 0.62, 0.2)
const CONNECTOR_WIDTH := 2.0
## A chip's colour, editable or not, and the colour of a chip whose pick is on.
const CHIP_COLOUR := Color(0.55, 0.85, 1.0)
const CHIP_DIM := Color(0.62, 0.62, 0.62)
const CHIP_PICKING := Color(1.0, 0.85, 0.35)
## The meta a row's drawn state is kept under, so an unchanged redraw keeps its controls.
const DRAWN := "drawn"


## Replaces the rows in `container` with the rule list `list` (the bridge's `rule_list`),
## rendered as `revision` (the bridge's `rules_rendering`, the rendering id), unless that is
## what is drawn already; returns whether it redrew. `editable` is the bridge's `rules_editable`, `picking`
## the chip a pick is on for (-1 none), `conflicts` and `refusal` the bridge's. `on_chip` is
## called with a chip's index when it is clicked, `on_move` with the dragged rule's index and
## pointer, the dropped-on rule's index and pointer, and the rendering, when one is dropped on
## the other.
static func fill(container: Container, list: Dictionary, revision: int, editable: bool, picking: int, conflicts: Array, refusal: Dictionary, on_chip: Callable, on_move: Callable, on_slot: Callable = Callable()) -> bool:
	var drawn := var_to_str([list, revision, editable, picking, conflicts, refusal])
	if container.has_meta(DRAWN) and String(container.get_meta(DRAWN)) == drawn:
		return false
	container.set_meta(DRAWN, drawn)
	container.set_meta("conflicts", conflicts)
	for child in container.get_children():
		container.remove_child(child)
		child.queue_free()
	var chips: Array = list.get("chips", [])
	var rules: Array = list.get("rules", [])
	var lines: Array = list.get("lines", [])
	var said := {}
	# Every index the bridge sends is read through Values.int_of; one it could not send as an `int`
	# names nothing here, and the list says so under its last line (S3's `fixp`).
	var unread: Array = []
	for conflict in conflicts:
		var later := _index(conflict.get("later", -1), unread)
		var earlier := _index(conflict.get("earlier", -1), unread)
		if later >= 0 and earlier >= 0:
			said[later] = String(conflict.get("sentence", ""))
	for number in lines.size():
		var line: Dictionary = lines[number]
		var rule := _index(line.get("rule", -1), unread)
		var row := _row(line, chips, editable, picking, on_chip, unread)
		row.set_meta("rule", rule)
		if rule >= 0 and rule < rules.size():
			_draggable(row, rule, String(rules[rule].get("pointer", "")), String(rules[rule].get("parent", "")), revision, editable, on_move)
		container.add_child(row)
		if rule >= 0 and on_slot.is_valid():
			_slots(container, list.get("slots", []), rule, editable, on_slot, unread)
		if rule >= 0 and said.has(rule):
			container.add_child(_note(Strings.text("rule_conflict", {"sentence": said[rule]})))
		if rule >= 0 and _index(refusal.get("rule", -1), unread) == rule:
			# The gateway's refusal (`rule_refused`) or this client's, by the key the bridge names.
			container.add_child(_note(Strings.text(String(refusal.get("key", "rule_refused")), {"detail": refusal.get("detail", "")})))
	if not unread.is_empty():
		container.add_child(_note(Values.unreadable_text(Strings.text("value_index"), unread[0])))
	container.queue_redraw()
	return true


## An index the bridge sent, or -1, which names nothing, when it sent its refusal; the refusal is
## kept in `unread` for the list to say.
static func _index(value: Variant, unread: Array) -> int:
	var read: Variant = Values.int_of(value)
	if read is int:
		return read
	unread.append(read)
	return -1


## One line: its runs drawn in order, the chips as links whose meta is the chip's index.
static func _row(line: Dictionary, chips: Array, editable: bool, picking: int, on_chip: Callable, unread: Array) -> RichTextLabel:
	var row := RichTextLabel.new()
	row.fit_content = true
	row.scroll_active = false
	row.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	row.custom_minimum_size = Vector2(LINE_WIDTH, 0)
	var margin := StyleBoxEmpty.new()
	margin.content_margin_left = MARGIN
	row.add_theme_stylebox_override("normal", margin)
	var text := ""
	for segment in line.get("segments", []):
		var piece := String(segment.get("text", ""))
		text += piece
		var chip := _index(segment.get("chip", -1), unread)
		if chip < 0 or chip >= chips.size():
			row.add_text(piece)
			continue
		var shown := String(chips[chip].get("editor", "")) != "read_only" and editable
		row.push_color(CHIP_PICKING if chip == picking else (CHIP_COLOUR if shown else CHIP_DIM))
		row.push_meta(chip)
		row.add_text(piece)
		row.pop()
		row.pop()
	row.meta_clicked.connect(func(meta: Variant) -> void: on_chip.call(int(meta)))
	row.accessibility_name = text
	row.set_meta("line", text)
	return row


## The condition slots the gateway sent for rule `rule` that take a pick (an empty or a leaf
## slot, with the op the gateway named), each a button in the gateway's words ("when, item 1
## of all", "step 1, skip if") that opens the condition picker on it. A group slot is drawn as
## no button: its items are the pick targets. The titles are the gateway's; none is composed.
static func _slots(container: Container, slots: Array, rule: int, editable: bool, on_slot: Callable, unread: Array) -> void:
	for index in slots.size():
		var slot: Dictionary = slots[index]
		if _index(slot.get("rule", -1), unread) != rule or not slot.get("pickable", false):
			continue
		var button := Button.new()
		button.flat = true
		button.focus_mode = Control.FOCUS_NONE
		button.alignment = HORIZONTAL_ALIGNMENT_LEFT
		button.text = Strings.text("rule_slot", {"title": slot.get("title", "")})
		button.accessibility_name = button.text
		button.disabled = not editable
		button.custom_minimum_size = Vector2(LINE_WIDTH, 0)
		button.set_meta("slot", index)
		button.pressed.connect(func() -> void: on_slot.call(index))
		container.add_child(button)


## A rule's sentence row can be dragged onto another rule of the same list, `parent`, of the
## same rendering, `revision`; the drop asks for the move. A drag down lands the rule after the
## one it is dropped on, and a drag up before it: an RFC 6902 `move` takes the dropped-on
## rule's place. Nothing is dragged while the rule list takes no click.
static func _draggable(row: Control, rule: int, pointer: String, parent: String, revision: int, editable: bool, on_move: Callable) -> void:
	row.set_drag_forwarding(
		func(_at: Vector2) -> Variant:
			if not editable:
				return null
			var preview := Label.new()
			preview.text = String(row.get_meta("line")).strip_edges()
			row.set_drag_preview(preview)
			return {"rule": rule, "pointer": pointer, "parent": parent, "revision": revision},
		func(_at: Vector2, data: Variant) -> bool:
			return editable and data is Dictionary and String(data.get("parent", "")) == parent and int(data.get("revision", -1)) == revision and int(data.get("rule", -1)) != rule,
		func(_at: Vector2, data: Variant) -> void:
			on_move.call(int(data["rule"]), String(data["pointer"]), rule, pointer, int(data["revision"]))
	)


## A line under a rule that is not one of the prose's lines: no `line` meta, so the rows a
## check reads are the prose's alone.
static func _note(text: String) -> Label:
	var note := Label.new()
	note.text = text
	note.accessibility_name = text
	note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	note.custom_minimum_size = Vector2(LINE_WIDTH, 0)
	note.add_theme_color_override("font_color", CONNECTOR_COLOUR)
	var margin := StyleBoxEmpty.new()
	margin.content_margin_left = MARGIN
	note.add_theme_stylebox_override("normal", margin)
	return note


## Draws the conflict connectors on `container` (connect its `draw` signal here): for each
## conflict, a line in the left margin from the earlier rule's sentence to the later one's,
## with a short tick into each.
static func draw_connectors(container: Container) -> void:
	if not container.has_meta("conflicts"):
		return
	for conflict in container.get_meta("conflicts"):
		# An unreadable index draws no connector; fill said so under the list.
		var earlier := _rule_row(container, _index(conflict.get("earlier", -1), []))
		var later := _rule_row(container, _index(conflict.get("later", -1), []))
		if earlier == null or later == null:
			continue
		var top := earlier.position.y + earlier.size.y * 0.5
		var bottom := later.position.y + later.size.y * 0.5
		container.draw_line(Vector2(CONNECTOR_X, top), Vector2(CONNECTOR_X, bottom), CONNECTOR_COLOUR, CONNECTOR_WIDTH)
		container.draw_line(Vector2(CONNECTOR_X, top), Vector2(MARGIN, top), CONNECTOR_COLOUR, CONNECTOR_WIDTH)
		container.draw_line(Vector2(CONNECTOR_X, bottom), Vector2(MARGIN, bottom), CONNECTOR_COLOUR, CONNECTOR_WIDTH)


## The sentence row of rule `rule` in `container`, or null.
static func _rule_row(container: Container, rule: int) -> Control:
	if rule < 0:
		return null
	for child in container.get_children():
		if child.has_meta("line") and int(child.get_meta("rule", -1)) == rule:
			return child
	return null


## The rows `container` holds, in order: what a check reads the accessible names from.
static func row_controls(container: Container) -> Array[Control]:
	var out: Array[Control] = []
	for child in container.get_children():
		if child.has_meta("line"):
			out.append(child)
	return out


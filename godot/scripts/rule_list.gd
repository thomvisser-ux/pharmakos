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
#     which the chip's shape names (a number field within the bounds sent, an enum's values as
#     sent, a pick on the map). A chip the rule list does not edit (a duration, which waits on a
#     typed unit hint from the wire; a step label or a rule id, part 2's picker) is drawn dim,
#     and a click says why;
#   * a rule's or a step's sentence row can be DRAGGED onto another of the same list: the drop
#     asks the bridge for one JSON Patch `move` of it to that rule's place;
#   * the CONFLICT CONNECTOR joins the two rules a W0702 row names, a line in the list's left
#     margin from the earlier rule to the later one, with the verifier's sentence under the
#     later rule;
#   * the gateway's refusal of a rule-list edit is shown under the rule it was of, in the
#     gateway's words.
# The controls take a click only while the bridge says the rule list describes the text on
# screen, no edit is on its way and the door is open (`rules_editable`), as Fix buttons do.
#
# PLACEHOLDER: the rule list's look — OWNER, at S6.

extends RefCounted

const Strings := preload("res://scripts/strings.gd")

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
## unless what `drawn` describes is what is drawn already. `editable` is the bridge's
## `rules_editable`, `picking` the chip a pick is on for (-1 none), `conflicts` and `refusal`
## the bridge's. `on_chip` is called with a chip's index when it is clicked, `on_move` with
## two rule indices when one is dropped on the other.
static func fill(container: Container, list: Dictionary, editable: bool, picking: int, conflicts: Array, refusal: Dictionary, on_chip: Callable, on_move: Callable) -> void:
	var drawn := var_to_str([list, editable, picking, conflicts, refusal])
	if container.has_meta(DRAWN) and String(container.get_meta(DRAWN)) == drawn:
		return
	container.set_meta(DRAWN, drawn)
	container.set_meta("conflicts", conflicts)
	for child in container.get_children():
		container.remove_child(child)
		child.queue_free()
	var chips: Array = list.get("chips", [])
	var rules: Array = list.get("rules", [])
	var lines: Array = list.get("lines", [])
	var said := {}
	for conflict in conflicts:
		said[int(conflict.get("later", -1))] = String(conflict.get("sentence", ""))
	for number in lines.size():
		var line: Dictionary = lines[number]
		var rule := int(line.get("rule", -1))
		var row := _row(line, chips, editable, picking, on_chip)
		row.set_meta("rule", rule)
		if rule >= 0 and rule < rules.size():
			_draggable(row, rule, String(rules[rule].get("parent", "")), editable, on_move)
		container.add_child(row)
		if rule >= 0 and said.has(rule):
			container.add_child(_note(Strings.text("rule_conflict", {"sentence": said[rule]})))
		if rule >= 0 and int(refusal.get("rule", -1)) == rule:
			container.add_child(_note(Strings.text("rule_refused", {"detail": refusal.get("detail", "")})))
	container.queue_redraw()


## One line: its runs drawn in order, the chips as links whose meta is the chip's index.
static func _row(line: Dictionary, chips: Array, editable: bool, picking: int, on_chip: Callable) -> RichTextLabel:
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
		var chip := int(segment.get("chip", -1))
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


## A rule's sentence row can be dragged onto another rule of the same list, `parent`; the drop
## asks for the move. Nothing is dragged while the rule list takes no click.
static func _draggable(row: Control, rule: int, parent: String, editable: bool, on_move: Callable) -> void:
	row.set_drag_forwarding(
		func(_at: Vector2) -> Variant:
			if not editable:
				return null
			var preview := Label.new()
			preview.text = String(row.get_meta("line")).strip_edges()
			row.set_drag_preview(preview)
			return {"rule": rule, "parent": parent},
		func(_at: Vector2, data: Variant) -> bool:
			return editable and data is Dictionary and String(data.get("parent", "")) == parent and int(data.get("rule", -1)) != rule,
		func(_at: Vector2, data: Variant) -> void:
			on_move.call(int(data["rule"]), rule)
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
		var earlier := _rule_row(container, int(conflict.get("earlier", -1)))
		var later := _rule_row(container, int(conflict.get("later", -1)))
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


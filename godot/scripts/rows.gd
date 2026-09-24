# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The validation rows: one per diagnostic the verifier returned, drawn as an icon, the
# severity in words, the plain sentence, the code and the JSON Pointer, and a Fix button for
# each machine-applicable patch the verifier offered (spec section 13, "Validation").
#
# Every word on a row but the severity comes from the verifier, through the bridge
# (`crates/client-gdext/src/editor.rs`, `rows_of`); nothing here judges a playbook. A Fix
# button hands the row and fix index back to the editor, which asks the gateway to apply
# the verifier's own patch.
#
# Accessibility (spec section 13): each row's accessible name is its plain sentence, the
# icon's is its severity word, and a Fix button's is its label - names generated from the
# rendered text, not written beside it. Icons plus text: the colour is never the only
# signal. PLACEHOLDER: accessibility polish beyond generated names (font scaling, reduced
# motion, focus order) is S6's, OWNER (skeleton plan T19, PLACEHOLDERs).

extends RefCounted

const Strings := preload("res://scripts/strings.gd")

## The icon colours by severity. PLACEHOLDER: art, OWNER at S6's art pass.
const ICON_COLOURS := {
	"error": Color(0.86, 0.26, 0.22),
	"warning": Color(0.93, 0.66, 0.18),
	"info": Color(0.35, 0.62, 0.9),
}
## The icon's size in pixels. PLACEHOLDER: art, OWNER at S6.
const ICON_SIZE := Vector2(14, 14)
## The width a row's sentence wraps at. PLACEHOLDER: layout, OWNER at S6.
const SENTENCE_WIDTH := 300.0
## The gap between two rows, in pixels. PLACEHOLDER: layout, OWNER at S6.
const ROW_GAP := 12


## Replaces the rows in `container` with `rows` (the bridge's row dictionaries). `on_fix`
## is called with the row index and the fix index when a Fix button is pressed; the
## buttons are disabled when `fixes_enabled` is false (the rows describe an older text).
static func fill(container: Container, rows: Array, on_fix: Callable, fixes_enabled: bool) -> void:
	container.add_theme_constant_override("separation", ROW_GAP)
	for child in container.get_children():
		container.remove_child(child)
		child.queue_free()
	if rows.is_empty():
		var nothing := Label.new()
		nothing.text = Strings.text("no_rows")
		nothing.accessibility_name = nothing.text
		container.add_child(nothing)
		return
	for index in rows.size():
		container.add_child(_row(rows[index], index, on_fix, fixes_enabled))


## The rows `container` holds, in order: what a check reads the accessible names from.
static func row_controls(container: Container) -> Array[Control]:
	var out: Array[Control] = []
	for child in container.get_children():
		if child.has_meta("sentence"):
			out.append(child)
	return out


static func _row(row: Dictionary, index: int, on_fix: Callable, fixes_enabled: bool) -> Control:
	var severity := String(row.get("severity", "error"))
	var sentence := String(row.get("sentence", ""))
	var line := HBoxContainer.new()
	line.set_meta("sentence", sentence)
	line.set_meta("code", String(row.get("code", "")))
	line.set_meta("pointer", String(row.get("pointer", "")))
	line.accessibility_name = sentence

	var icon := ColorRect.new()
	icon.custom_minimum_size = ICON_SIZE
	icon.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	icon.color = ICON_COLOURS.get(severity, ICON_COLOURS["error"])
	icon.accessibility_name = Strings.text("severity_" + severity)
	line.add_child(icon)

	var body := VBoxContainer.new()
	body.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	line.add_child(body)

	var word := Label.new()
	word.text = Strings.text("severity_" + severity)
	word.add_theme_color_override("font_color", icon.color)
	body.add_child(word)

	var said := Label.new()
	said.text = sentence
	said.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	said.custom_minimum_size = Vector2(SENTENCE_WIDTH, 0)
	said.accessibility_name = sentence
	body.add_child(said)

	var where := Label.new()
	where.text = Strings.text("row_where", {"code": row.get("code", ""), "pointer": row.get("pointer", "")})
	where.modulate = Color(1, 1, 1, 0.65)
	where.autowrap_mode = TextServer.AUTOWRAP_ARBITRARY
	where.custom_minimum_size = Vector2(SENTENCE_WIDTH, 0)
	body.add_child(where)

	for fix in row.get("fixes", []):
		var button := Button.new()
		button.text = Strings.text("fix", {"title": fix.get("title", "")})
		button.accessibility_name = button.text
		button.disabled = not fixes_enabled
		var which := int(fix.get("index", 0))
		button.pressed.connect(func() -> void: on_fix.call(index, which))
		body.add_child(button)
	return line

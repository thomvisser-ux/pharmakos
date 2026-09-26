# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The template wizard's pages (T19 pull request 2; skeleton-plan-w6-notes.md section A4 as
# decisions-log item 112 amends it): one page per parameter the gateway's
# `instantiate_template{suggested: true}` answer lists, in its order, each drawn as it came -
# the template's label, the value as the raw JSON text the gateway wrote (a duration stays
# game milliseconds), a mark where the value is the built-in operator's suggestion, and the
# answer's "why" as it came.
#
# GENERIC OVER WHAT EACH TEMPLATE DECLARES: nothing here names a template, a pointer or a
# label, so a template added to `library/` needs no change here
# (`crates/client-gdext/tests/wizard.rs`, `the_client_names_no_template`).
#
# THIN (AGENTS.md section 3 rule 4): a value is never parsed, checked or converted here. What
# the player types goes to the bridge exactly as typed (`on_send` with the page's pointer
# and the text), which sends it to the gateway as an explicit value; a refusal is shown as
# the gateway wrote it. Use hands the gateway's playbook to the editor through the bridge.
#
# Accessibility (spec section 13): each page's accessible name is its label, the value field's
# too, the mark's is its words, and the why's is its sentence - names generated from the
# rendered text. PLACEHOLDER: accessibility polish beyond generated names is S6's, OWNER.
#
# PLACEHOLDER: the page layout (all pages in one column rather than one page at a time),
# and a value's unit display, are S6's with the typed parameter catalogue, OWNER.

extends RefCounted

const Strings := preload("res://scripts/strings.gd")

## The width a label wraps at. PLACEHOLDER: layout, OWNER at S6.
const TEXT_WIDTH := 320.0
## The mark's colour. PLACEHOLDER: art, OWNER at S6's art pass.
const MARK_COLOUR := Color(0.55, 0.8, 1.0)
## The gap between two pages, in pixels. PLACEHOLDER: layout, OWNER at S6.
const PAGE_GAP := 10


## Replaces what `container` shows with `wizard` (the bridge's wizard dictionary: `pages`,
## `why`, `current`, `refusal`, `answered`). `title`, when there is one, heads it.
## `on_send` is called with a page's pointer and the text in its field when that page's
## Send is pressed; `on_use` and `on_close` when Use and Close are.
static func fill(container: Container, title: String, wizard: Dictionary, on_send: Callable, on_use: Callable, on_close: Callable) -> void:
	container.add_theme_constant_override("separation", PAGE_GAP)
	for child in container.get_children():
		container.remove_child(child)
		child.queue_free()
	if wizard.is_empty():
		return
	if title != "":
		var heading := Label.new()
		heading.text = Strings.text("wizard_heading", {"title": title})
		heading.accessibility_name = heading.text
		container.add_child(heading)
	if not wizard.get("answered", false):
		var waiting := Label.new()
		waiting.text = Strings.text("wizard_waiting")
		waiting.accessibility_name = waiting.text
		container.add_child(waiting)
	var pages: Array = wizard.get("pages", [])
	for index in pages.size():
		container.add_child(_page(pages[index], on_send))
	var why := String(wizard.get("why", ""))
	if why != "":
		var said := Label.new()
		said.text = Strings.text("wizard_why", {"why": why})
		said.accessibility_name = said.text
		said.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		said.custom_minimum_size = Vector2(TEXT_WIDTH, 0)
		said.set_meta("why", why)
		container.add_child(said)
	var refusal := String(wizard.get("refusal", ""))
	if refusal != "":
		var refused := Label.new()
		refused.text = refusal
		refused.accessibility_name = refusal
		refused.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		refused.custom_minimum_size = Vector2(TEXT_WIDTH, 0)
		refused.add_theme_color_override("font_color", Color(0.95, 0.4, 0.35))
		refused.set_meta("refusal", refusal)
		container.add_child(refused)
	var buttons := HBoxContainer.new()
	container.add_child(buttons)
	var use := _button(buttons, Strings.text("wizard_use"), on_use)
	use.disabled = not wizard.get("current", false)
	use.set_meta("use", true)
	_button(buttons, Strings.text("wizard_close"), on_close)


## The pages `container` holds, in order: each a Control with the metas `pointer`, `label`,
## `value` and `suggested`, and the children `field` (the value as shown) and `mark`.
static func page_controls(container: Container) -> Array[Control]:
	var out: Array[Control] = []
	for child in container.get_children():
		if child.has_meta("pointer"):
			out.append(child)
	return out


## The Use button as drawn, or null when the wizard is closed.
static func use_button(container: Container) -> Button:
	for child in container.get_children():
		if child is HBoxContainer:
			for button in child.get_children():
				if button.has_meta("use"):
					return button
	return null


## The why as drawn, or "" when none is.
static func why_text(container: Container) -> String:
	for child in container.get_children():
		if child.has_meta("why"):
			return (child as Label).text
	return ""


## The refusal as drawn, or "" when none is.
static func refusal_text(container: Container) -> String:
	for child in container.get_children():
		if child.has_meta("refusal"):
			return (child as Label).text
	return ""


static func _page(page: Dictionary, on_send: Callable) -> Control:
	var label := String(page.get("label", ""))
	var box := VBoxContainer.new()
	box.accessibility_name = label
	box.set_meta("pointer", String(page.get("pointer", "")))
	box.set_meta("label", label)
	box.set_meta("value", String(page.get("value", "")))
	box.set_meta("suggested", bool(page.get("suggested", false)))

	var named := Label.new()
	named.text = Strings.text("wizard_page_value", {"label": label})
	named.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	named.custom_minimum_size = Vector2(TEXT_WIDTH, 0)
	named.accessibility_name = label
	box.add_child(named)

	var row := HBoxContainer.new()
	box.add_child(row)
	var field := LineEdit.new()
	field.name = "field"
	field.text = String(page.get("value", ""))
	field.accessibility_name = label
	field.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	row.add_child(field)
	var pointer := String(page.get("pointer", ""))
	var send := Button.new()
	send.text = Strings.text("wizard_send")
	send.accessibility_name = send.text
	send.focus_mode = Control.FOCUS_NONE
	send.pressed.connect(func() -> void: on_send.call(pointer, field.text))
	field.text_submitted.connect(func(typed: String) -> void: on_send.call(pointer, typed))
	row.add_child(send)
	box.set_meta("field", field)
	box.set_meta("send", send)

	var mark := Label.new()
	mark.name = "mark"
	mark.text = Strings.text("wizard_mark")
	mark.accessibility_name = mark.text
	mark.add_theme_color_override("font_color", MARK_COLOUR)
	mark.visible = bool(page.get("suggested", false))
	box.add_child(mark)
	box.set_meta("mark", mark)
	return box


static func _button(parent: Node, text: String, pressed: Callable) -> Button:
	var button := Button.new()
	button.text = text
	button.accessibility_name = text
	button.focus_mode = Control.FOCUS_NONE
	button.pressed.connect(pressed)
	parent.add_child(button)
	return button

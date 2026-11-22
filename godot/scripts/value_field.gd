# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# ONE VALUE FIELD FOR EVERY TYPED VALUE (S3's `fixp`, P6b findings 1 and 2): the rule list's
# number chip, the Visit & change number setting and every template wizard page type into this
# one component, and nothing else.
#
# What it replaced, as measured on Godot 4.7.2 (`fixp`'s pull request): a `SpinBox` copies its
# text into its `value` only on Enter or a focus change, so a click on the popup's Send button
# sent the old number; a click into it left the caret after the old text with nothing selected,
# so typed digits landed beside it ("40 %30"); a `Range` clamps a typed value to the bounds and
# holds a float, which rounds a whole number past 2^53; and a `SpinBox` evaluates its text as an
# expression ("5*2" is 10).
#
# So this field is a `LineEdit` and holds no value but its text: there is no uncommitted state
# for a button to ignore. Enter and Send both emit `sent` with the text exactly as typed, which
# goes to the bridge; the bridge reads it (a whole number, for a number field: optional sign and
# digits, nothing else) and refuses one it cannot read, by its reason. The field has no bounds,
# no arrows and no arithmetic: the bounds are shown in the gateway's words beside it, a value past
# them goes to the gateway, and stepping would be client arithmetic over a step the gateway never
# sends (decisions-log rulings on `fixp`, TODO 2).
#
# THE FIRST CLICK SELECTS THE OLD VALUE. Each `open` releases the focus, and the first press on
# the field after it gains the focus selects the whole text and is taken there with its release,
# so neither can put the caret back after it; a focus gained without a click (Tab) selects it too. The
# next character typed replaces the old value, whatever the field did before.
#
# PLACEHOLDER: the value field's look — OWNER, at S6.

extends HBoxContainer

## The text as typed, on Enter or on Send.
signal sent(text: String)

## The field's least width, in pixels. PLACEHOLDER: the field's width — OWNER, at S6.
const FIELD_WIDTH := 140.0

## The text field, the unit drawn beside it as the gateway sent it, and Send.
var field: LineEdit
var unit: Label
var send: Button

## Whether the next press on the field is the first since it opened or gained the focus.
var _fresh := true
## Whether the release of that first press is still to come.
var _holding := false


## `send_text` is Send's label.
func _init(send_text: String = "") -> void:
	field = LineEdit.new()
	field.name = "field"
	field.select_all_on_focus = false
	field.custom_minimum_size = Vector2(FIELD_WIDTH, 0)
	field.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	field.focus_entered.connect(_on_focus)
	field.gui_input.connect(_on_field_input)
	field.text_submitted.connect(func(typed: String) -> void: sent.emit(typed))
	add_child(field)
	unit = Label.new()
	unit.name = "unit"
	unit.visible = false
	add_child(unit)
	send = Button.new()
	send.name = "send"
	send.text = send_text
	send.accessibility_name = send_text
	# Send takes no focus, so a click on it leaves the text field as it was.
	send.focus_mode = Control.FOCUS_NONE
	send.pressed.connect(func() -> void: sent.emit(field.text))
	add_child(send)


## Shows `text` (the value as the file or the gateway holds it) with `unit_text` beside it, and
## releases the focus, so the player's first click selects `text` whole.
func open(text: String, unit_text: String, name_text: String = "") -> void:
	field.text = text
	field.accessibility_name = name_text
	unit.text = unit_text
	unit.visible = unit_text != ""
	if field.has_focus():
		field.release_focus()
	field.deselect()
	_fresh = true


## The focus came: the whole text is selected, and the next press, if one brought the focus,
## keeps it so.
func _on_focus() -> void:
	_fresh = true
	field.select_all()


## The first left press after the field opened or gained the focus takes the focus when the window
## has not given it, selects the whole text, and is taken here with its release, so the field's own
## handling of either cannot put the caret back off the selection (measured on Godot 4.7.2: the
## release alone clears a selection the press made).
func _on_field_input(event: InputEvent) -> void:
	if not (event is InputEventMouseButton) or event.button_index != MOUSE_BUTTON_LEFT:
		return
	if event.pressed and _fresh:
		if not field.has_focus():
			field.grab_focus()
		_fresh = false
		_holding = true
		field.select_all()
		field.accept_event()
	elif not event.pressed and _holding:
		_holding = false
		field.select_all()
		field.accept_event()

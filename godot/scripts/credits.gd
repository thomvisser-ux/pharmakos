# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The credits overlay (decisions-log item 117 (11)): the lobby adds it from the Credits
# button in its pre-match chooser and it removes itself on Back. It is an overlay, never a
# scene change, because freeing the lobby ends its host; and the button is only in the
# chooser, which is hidden once a match has started, so it never opens while a host runs.
#
# It only draws:
#   * the lockup;
#   * the game's licences by area, as strings from `scripts/strings.gd` (never read from
#     files at run time);
#   * one line pointing at the notices file beside the game, which lists the Rust crates
#     compiled into the client library and `gamectl` (`cargo xtask package` writes it);
#   * the CC BY attributions, of which there are none yet;
#   * Godot's own licence and the notices of its third-party components, as the engine
#     reports them (`Engine.get_license_text`, `get_copyright_info`, `get_license_info`),
#     which is Godot's own compliance route and is the same in an export as in the editor.
#
# No in-game crate list (item 117 (11)): that is the notices file's. Nothing here computes
# anything; text is joined, never summed (AGENTS.md section 3 rule 4).

extends CanvasLayer

## Emitted once Back is pressed, just before the overlay frees itself.
signal closed

const Strings := preload("res://scripts/strings.gd")

## Above the lobby's own layer, so the chooser is covered.
const OVERLAY_LAYER := 10

## The Back button; the smoke check presses it through its own `pressed` signal.
var back: Button


func _ready() -> void:
	layer = OVERLAY_LAYER
	var panel := PanelContainer.new()
	panel.set_anchors_preset(Control.PRESET_FULL_RECT)
	panel.mouse_filter = Control.MOUSE_FILTER_STOP
	add_child(panel)
	var column := VBoxContainer.new()
	panel.add_child(column)
	back = Button.new()
	back.text = Strings.text("about_back")
	back.accessibility_name = back.text
	back.focus_mode = Control.FOCUS_NONE
	back.pressed.connect(_on_back)
	column.add_child(back)
	var scroll := ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	column.add_child(scroll)
	var body := Label.new()
	body.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	body.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	body.text = "\n".join(_lines())
	scroll.add_child(body)


## Every line the overlay draws, in order.
func _lines() -> PackedStringArray:
	var lines := PackedStringArray()
	lines.append(Strings.text("about_lockup"))
	lines.append("")
	lines.append(Strings.text("about_licences_heading"))
	lines.append(Strings.text("about_licence_game"))
	lines.append(Strings.text("about_licence_data"))
	lines.append(Strings.text("about_licence_art"))
	lines.append(Strings.text("about_notices_file"))
	lines.append("")
	lines.append(Strings.text("about_attribution_heading"))
	lines.append(Strings.text("about_attribution_none"))
	lines.append("")
	lines.append(Strings.text("about_godot_heading"))
	lines.append(Strings.text("about_godot_intro"))
	lines.append("")
	lines.append(Engine.get_license_text())
	for component in Engine.get_copyright_info():
		lines.append(Strings.text("about_godot_component", {"name": component.get("name", "")}))
		for part in component.get("parts", []):
			var holders := "; ".join(PackedStringArray(part.get("copyright", PackedStringArray())))
			lines.append(Strings.text("about_godot_part", {"copyright": holders, "license": part.get("license", "")}))
	var texts: Dictionary = Engine.get_license_info()
	var names := texts.keys()
	names.sort()
	for name in names:
		lines.append("")
		lines.append(Strings.text("about_godot_licence", {"name": name}))
		lines.append(String(texts[name]))
	return lines


func _on_back() -> void:
	closed.emit()
	queue_free()

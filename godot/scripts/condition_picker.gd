# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The condition picker (S3's `ed3b`, part 2; spec section 13: "Conditions are picked through
# family -> predicate -> parameters"): a tree of the families `get_vocabulary` sent, each
# family's predicates under it, and each predicate's parameters under that, every one drawn as
# sent and in the order sent.
#
# NOTHING HERE DECIDES WHAT MAY BE PICKED. A family or a predicate the gateway locked is drawn
# greyed with the gateway's own reason beside it and cannot be selected; nothing is hidden. The
# client holds no family, predicate, parameter title, bound or default: the words are the
# bridge's `vocabulary` (crates/client-gdext/src/vocabulary.rs), and the pick is the bridge's one
# JSON Patch (`editor_choose_predicate`), the predicate's skeleton wrapped under its name. A
# predicate's parameters are listed for reading: once it is in the file, they are its chips in
# the rule list, edited there. A duration is shown with no field (the owner's ruling of
# 2026-11-15: the player is never asked for milliseconds).
#
# PLACEHOLDER: the picker's look — OWNER, at S6.

extends RefCounted

const Strings := preload("res://scripts/strings.gd")

## The colour a locked row is drawn in.
const LOCKED_COLOUR := Color(0.6, 0.6, 0.6)


## Fills `tree` with the vocabulary `vocabulary` (the bridge's `vocabulary`): one row per family,
## its predicates under it, their parameters under each. A predicate row carries the family's
## and the predicate's indices as its metadata, `[family, entry]`; it is selectable only when
## neither it nor its family is locked.
static func fill(tree: Tree, vocabulary: Dictionary) -> void:
	tree.clear()
	tree.hide_root = true
	var root := tree.create_item()
	var families: Array = vocabulary.get("families", [])
	for family_index in families.size():
		var family: Dictionary = families[family_index]
		var family_locked := String(family.get("locked", ""))
		var row := tree.create_item(root)
		row.set_selectable(0, false)
		if family_locked != "":
			row.set_text(0, Strings.text("picker_family_locked", {"title": family.get("title", ""), "locked": family_locked}))
			row.set_custom_color(0, LOCKED_COLOUR)
			row.collapsed = true
		else:
			row.set_text(0, String(family.get("title", "")))
		var predicates: Array = family.get("predicates", [])
		for entry_index in predicates.size():
			var entry: Dictionary = predicates[entry_index]
			var locked := String(entry.get("locked", ""))
			var item := tree.create_item(row)
			item.set_metadata(0, [family_index, entry_index])
			if locked != "":
				item.set_text(0, Strings.text("picker_entry_locked", {"title": entry.get("title", ""), "locked": locked}))
				item.set_custom_color(0, LOCKED_COLOUR)
			else:
				item.set_text(0, Strings.text("picker_entry", {"title": entry.get("title", "")}))
			item.set_selectable(0, locked == "" and family_locked == "")
			for parameter in entry.get("parameters", []):
				var leaf := tree.create_item(item)
				leaf.set_text(0, parameter_text(parameter))
				leaf.set_selectable(0, false)
			item.collapsed = true


## A parameter's line: its title and what it holds, as the gateway sent them.
static func parameter_text(parameter: Dictionary) -> String:
	var title: Variant = parameter.get("title", "")
	match String(parameter.get("shape", "")):
		"number":
			var bounds := ""
			if parameter.get("has_min", false) and parameter.get("has_max", false):
				bounds = Strings.text("picker_parameter_bounds", {"min": parameter.get("min"), "max": parameter.get("max")})
			var unit := String(parameter.get("unit", ""))
			return Strings.text("picker_parameter_number", {"title": title, "bounds": bounds, "unit": (" " + unit) if unit != "" else ""})
		"ms":
			return Strings.text("picker_parameter_ms", {"title": title})
		"choice":
			return Strings.text("picker_parameter_choice", {"title": title, "values": ", ".join(parameter.get("values", PackedStringArray()))})
		"reference":
			return Strings.text("picker_parameter_reference", {"title": title, "kind": parameter.get("kind", "")})
		_:
			return Strings.text("picker_parameter", {"title": title})


## The selected predicate as `[family, entry]`, or an empty array when none is selected.
static func chosen(tree: Tree) -> Array:
	var item := tree.get_selected()
	if item == null:
		return []
	var picked: Variant = item.get_metadata(0)
	return picked if picked is Array else []


## What the picker's line says about the vocabulary: waiting, the gateway's refusal, an answer
## this client could not read, or nothing when the words are on hand.
static func status_text(vocabulary: Dictionary) -> String:
	var failed := String(vocabulary.get("failed", ""))
	if failed != "":
		return Strings.text("vocabulary_" + failed, {"detail": vocabulary.get("detail", "")})
	if not vocabulary.get("known", false):
		return Strings.text("vocabulary_waiting")
	return ""


## The tree's rows as drawn, depth first, for a check to read: each `[text, selectable]`.
static func rows(tree: Tree) -> Array:
	var out: Array = []
	var root := tree.get_root()
	if root == null:
		return out
	_walk(root, out)
	return out


static func _walk(item: TreeItem, out: Array) -> void:
	for child in item.get_children():
		out.append([child.get_text(0), child.is_selectable(0)])
		_walk(child, out)

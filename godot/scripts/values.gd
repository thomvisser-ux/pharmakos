# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# THE ONE READER OF A COUNT THE BRIDGE HANDS OVER (S3's `fixp`, the `godot_int` sweep's GDScript
# half). The bridge never clamps a count GDScript's `int` cannot hold: it sends it as the typed
# refusal `{"not_an_int": "<its digits>"}` in the value's own place
# (crates/client-gdext/src/variant.rs, `int_variant`). `int(...)` on that raises a script error,
# so every script reads such a key through `int_of` and says the refusal on its line as unreadable.
#
# Not arithmetic and not a rule: a decoding of the bridge's two shapes for one value.

extends RefCounted

const Strings := preload("res://scripts/strings.gd")

## The bridge's key for a count past `int`.
const NOT_AN_INT := "not_an_int"


## `value` as the bridge sent it: the `int` it holds, or, when it is the bridge's refusal (or
## anything else that is no `int`), that refusal as `{"unreadable": "<what it held>"}`. A caller
## tells them apart with `is int`, and says a refusal with `unreadable_text`.
static func int_of(value: Variant) -> Variant:
	if typeof(value) == TYPE_INT:
		return value
	var held := ""
	if typeof(value) == TYPE_DICTIONARY and (value as Dictionary).has(NOT_AN_INT):
		held = String(value[NOT_AN_INT])
	else:
		held = var_to_str(value)
	return {"unreadable": held}


## The words for a count that could not be read: `what` it is, and the digits it held.
static func unreadable_text(what: String, refused: Variant) -> String:
	var held := String((refused as Dictionary).get("unreadable", "")) if typeof(refused) == TYPE_DICTIONARY else var_to_str(refused)
	return Strings.text("value_unreadable", {"what": what, "held": held})


## A count put into a sentence: its digits as sent, or the words for a count that could not be
## read.
static func said(what: String, value: Variant) -> String:
	var read: Variant = int_of(value)
	if read is int:
		return str(read)
	return unreadable_text(what, read)

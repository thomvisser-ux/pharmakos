# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The rule list (T19 pull request 2): `render_plan`'s prose for the playbook on screen,
# split at its line ends by the bridge, one read-only row per line, each drawn exactly as it
# came, its accessible name its line (spec section 13's "accessible names generated from
# the rendered sentences").
#
# NO SENTENCE IS PARSED HERE. The line layout is the gateway's contract
# (`gp.api.v1.RenderPlanResponse`: one rule, step or block per line, the indent saying what
# a line is), so a row needs nothing but its text. Chips, drag reordering and the pickers
# are S3's (decisions-log item 111, decision C3). PLACEHOLDER: chip editing and drag
# reordering, OWNER at S3.

extends RefCounted

## The width a line wraps at. PLACEHOLDER: layout, OWNER at S6.
const LINE_WIDTH := 340.0


## Replaces the rows in `container` with one read-only row per line of `lines`.
static func fill(container: Container, lines: PackedStringArray) -> void:
	for child in container.get_children():
		container.remove_child(child)
		child.queue_free()
	for line in lines:
		var row := Label.new()
		row.text = line
		row.accessibility_name = line
		row.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		row.custom_minimum_size = Vector2(LINE_WIDTH, 0)
		row.set_meta("line", line)
		container.add_child(row)


## The rows `container` holds, in order: what a check reads the accessible names from.
static func row_controls(container: Container) -> Array[Control]:
	var out: Array[Control] = []
	for child in container.get_children():
		if child.has_meta("line"):
			out.append(child)
	return out

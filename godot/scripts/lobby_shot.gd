# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The lobby's Probation words, drawn with NO host running (S3's `prob`): the New match button's
# label, the status line's preset as `get_status` names it, the event list with one of the
# Ledger's memos, and a Probation round's recap with its memo and licence sentence. Every word
# is the lobby's own drawing (`scripts/lobby.gd`'s static functions) of the bridge's reading of
# a byte copy of a gateway golden, each kept identical by a test:
#   * fixtures/get_status_probation.json  - tests/golden/gateway/get_status_probation/
#   * fixtures/feed_ledger_memo.json      - tests/golden/gateway/feed_ledger_memo/
#   * fixtures/recap_probation_round.json - tests/golden/gateway/recap_probation_round/
#
# Without `--shot=` it is a check: the memo row is the Ledger's, the recap carries the memo and
# the prose whole, and the scene quits 0. NOT YET COMPARED: `cargo xtask screenshot` renders only
# the shots `xtask`'s SHOTS lists, and its row is an `xtask` change this lane does not own (S3's
# `cert3` adds it); until then it is run by hand (godot/README.md):
#
#     godot --path godot --resolution 1280x720 -- --scene=res://scenes/lobby_shot.tscn --shot=<png>
#
# Exit codes: 0 when everything was drawn as sent (and shot, when asked) with no caught panic,
# 1 when anything failed.

extends Control

const Lobby := preload("res://scripts/lobby.gd")
const Strings := preload("res://scripts/strings.gd")
const STATUS := "res://fixtures/get_status_probation.json"
const FEED := "res://fixtures/feed_ledger_memo.json"
const RECAP := "res://fixtures/recap_probation_round.json"
const SETTLE_DRAWS := 4

@onready var bridge: Node = $Bridge
@onready var column: VBoxContainer = $Column


func _ready() -> void:
	var shot := ""
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--shot="):
			shot = arg.trim_prefix("--shot=")
	if shot != "" and DisplayServer.get_name() == "headless":
		_fail("a screenshot needs a window; run this windowed, not with --headless")
		return
	var new_match := Button.new()
	new_match.text = Lobby.new_match_text()
	column.add_child(new_match)
	# The status line as the lobby draws it once `get_status` has named the match it asked for.
	var served := _served_preset()
	if served == "":
		return
	var status := Label.new()
	status.text = Lobby.preset_line({"preset": served, "preset_check": "agrees"})
	column.add_child(status)
	var rows: Array = bridge.watch_rows_of_response(FileAccess.get_file_as_string(FEED))
	var memo_rows := 0
	var lines: PackedStringArray = []
	for row in rows:
		lines.append(Lobby.event_text_of(row))
		if row.get("ledger", false):
			memo_rows += 1
			if not Lobby.event_text_of(row).contains(String(row.get("text", ""))):
				_fail("the memo's row does not carry the memo's words")
				return
	if rows.is_empty() or memo_rows != 1:
		_fail("%d event rows, %d of them the Ledger's: the fixture carries one memo" % [rows.size(), memo_rows])
		return
	var events := Label.new()
	events.text = "\n".join(lines)
	column.add_child(events)
	var recap: Dictionary = bridge.watch_recap_of_response(FileAccess.get_file_as_string(RECAP))
	var memos: Array = recap.get("memos", [])
	if recap.is_empty() or memos.size() != 1:
		_fail("the recap did not read with its one memo: %s" % recap)
		return
	var recap_label := Label.new()
	recap_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	recap_label.custom_minimum_size = Vector2(560, 0)
	recap_label.text = Lobby.recap_text_of(recap)
	if not recap_label.text.contains(String(recap.get("prose", ""))) or not recap_label.text.contains(String(memos[0].get("text", ""))):
		_fail("the recap is not drawn whole with its memo: %s" % recap_label.text)
		return
	column.add_child(recap_label)
	for i in SETTLE_DRAWS:
		await get_tree().process_frame
	var caught: Variant = bridge.caught_panics()
	if not (caught is int) or caught != 0:
		_fail("panic(s) were caught at the bridge boundary: %s" % str(caught))
		return
	if shot != "":
		await RenderingServer.frame_post_draw
		var image := get_viewport().get_texture().get_image()
		var error := image.save_png(shot)
		if error != OK:
			_fail("the PNG could not be written to %s (error %d)" % [shot, error])
			return
		print("[lobby-shot] %dx%d written to %s" % [image.get_width(), image.get_height(), shot])
	print("[lobby-shot] %s; %s; %d event rows, one the Ledger's; the recap with its memo; OK" % [new_match.text, status.text, rows.size()])
	get_tree().quit(0)


## The preset the committed `get_status` answer names, as the bridge's rig reads it; "" after a
## failure.
func _served_preset() -> String:
	var read: Dictionary = bridge.watch_status_of_response(FileAccess.get_file_as_string(STATUS))
	var preset := String(read.get("preset", ""))
	if preset == "":
		_fail("%s did not read; the reason is in the log" % STATUS)
		return ""
	if Strings.text("preset_" + preset) == "preset_" + preset:
		_fail("the string table has no row for the preset `%s`" % preset)
		return ""
	return preset


func _fail(message: String) -> void:
	printerr("[lobby-shot] FAILED: %s" % message)
	get_tree().quit(1)

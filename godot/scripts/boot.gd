# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The main scene: send the run where its command line says, or to the lobby.
#
# `cargo xtask screenshot` runs the project windowed under xvfb with its arguments after
# `--` (xtask/src/main.rs, `step_screenshot`):
#
#     godot --path godot --resolution 1280x720 -- --scene=res://scenes/vista_shot.tscn --shot=<png>
#
# so the main scene is this one, which honours `--scene=` and leaves `--shot=` to the scene
# that takes it. With no `--scene=`, the run is the game, and the game starts at the lobby.

extends Node

const LOBBY := "res://scenes/lobby.tscn"


func _ready() -> void:
	var target := LOBBY
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--scene="):
			target = arg.trim_prefix("--scene=")
	get_tree().call_deferred("change_scene_to_file", target)

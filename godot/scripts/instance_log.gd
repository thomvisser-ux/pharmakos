# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Each running client's own log file (S1's stage demo, finding N; decisions-log item 139 (2)).
#
# Godot's file logging has one path per project. A second instance's start rotates the first's
# `user://logs/godot.log` under it and both then write the new one, so a tester who runs two
# instances loses the first one's log. No setting names a path per process, and the rotation
# happens before any script runs, so the main scene (`scripts/boot.gd`) adds this logger beside
# the engine's: every print and every error also goes to
# `user://logs/client-<process id>.log`, and no two running instances share that file.
#
# What it does not hold: whatever the engine says before the main scene runs. That is the
# engine's own `godot.log`, which stays on because on a double-clicked Windows game it is the only
# record of a start that fails that early. This logger decides nothing and reaches nothing of
# the match. `smoke_check.gd` asserts it writes in a real run.
#
# PLACEHOLDER: how many instance logs are kept — OWNER, at S6.

extends Logger

## Where the logs live, and how many a start leaves behind.
const FOLDER := "user://logs"
const KEPT := 5

var _file: FileAccess = null
## A logger is called from whichever thread printed.
var _lock := Mutex.new()
## Whether this thread is already writing: an error the write itself raises comes back here, and
## must not write again.
var _writing := false


## This process's log file.
static func path_of(process_id: int) -> String:
	return "%s/client-%d.log" % [FOLDER, process_id]


## Opens `path` for this instance. A file that cannot be opened logs nothing, and says so on the
## standard error stream: it is the one place left to say it.
func _init(path: String) -> void:
	DirAccess.make_dir_recursive_absolute(path.get_base_dir())
	_file = FileAccess.open(path, FileAccess.WRITE)
	if _file == null:
		printerr("[log] %s could not be opened (error %d), so this instance keeps no log file" % [path, FileAccess.get_open_error()])


func _log_message(message: String, _error: bool) -> void:
	_write(message.trim_suffix("\n"))


func _log_error(function: String, file: String, line: int, code: String, rationale: String, _editor_notify: bool, _error_type: int, _script_backtraces: Array[ScriptBacktrace]) -> void:
	_write("ERROR: %s\n   at: %s (%s:%d)" % [rationale if rationale != "" else code, function, file, line])


func _write(text: String) -> void:
	if _file == null:
		return
	_lock.lock()
	if not _writing:
		_writing = true
		_file.store_line(text)
		_file.flush()
		_writing = false
	_lock.unlock()


## Removes all but the KEPT newest logs in `folder`, oldest first. On Windows a log another
## instance still holds open cannot be removed, and that removal fails, which is harmless. On
## Linux and macOS `remove_absolute` unlinks it, and that instance writes on into a file nobody
## can read. That is accepted: it takes KEPT later starts while the instance still runs, and
## its log is the oldest of them.
static func prune(folder: String) -> void:
	var logs: Array = []
	for name in DirAccess.get_files_at(folder):
		if name.begins_with("client-") and name.ends_with(".log"):
			var path := "%s/%s" % [folder, name]
			logs.append([FileAccess.get_modified_time(path), path])
	logs.sort()
	while logs.size() > KEPT:
		var oldest: Array = logs.pop_front()
		DirAccess.remove_absolute(oldest[1])

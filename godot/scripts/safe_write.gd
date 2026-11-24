# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# A file written whole or not at all (S3's `fixq-c`, fix passes 3 and 4): Save and Save as...
# (`editor.gd`), and the lobby's remembered config line and its forgetting (`lobby.gd`).
#
# A file written in place and failing part-way leaves half of it under its own name, and a
# half playbook or a half config line is worse than the old one. So every write:
#   1. RECOVERS what an earlier write that stopped left: the name absent and our backup present
#      (it stopped between moving the old file aside and moving the new one in): the backup,
#      the last good copy, is moved back, and the caller says so; the name present and our
#      backup present (it landed, and stopped before removing the backup): the backup is
#      removed; our temporary file present: removed. A recovery step that fails is its own
#      failure, whose words name the file that holds the last good copy;
#   2. refuses a target this user may not write BEFORE anything moves, by a probe that changes
#      nothing: the existing file opened for read and write, without truncation, and closed.
#      Read-write means the same on the three operating systems (a read-only file, a file the
#      user lacks write permission on, and a file another program holds locked all refuse it),
#      where moving a file aside does not: a read-only file can be renamed on all three, so a
#      write that only moved it aside would get past a file the player protected;
#   3. writes the bytes to its temporary file in the same folder, every Error checked;
#   4. moves the old file, if there is one, aside to its backup, moves the new file into place,
#      and removes the backup. No step renames over an existing file.
# At every moment the whole old file or the whole new one is under a name, and each failure
# says which name holds what.
#
# The beside-files are this helper's own and cannot be a player's file: a dot-prefixed name
# with a suffix no player file has (`<folder>/.<name>.pharmakos-write` and
# `.<name>.pharmakos-keep`). A player's own `plan.jsonc.bak` is never touched.
#
# Every file operation goes through ONE seam, a `Disk` (`exists`, `is_folder`, `read`,
# `probe_writable`, `write`, `rename`, `remove`): the real one for the shipped path, and in
# `client_check.gd` one that fails a chosen step, so each failure point is provoked the same
# way on Windows, Linux and macOS, never through one operating system's file behaviour.
#
# This decides nothing about the game: it writes bytes the caller hands it, and says why it
# could not.

extends RefCounted

const Strings := preload("res://scripts/strings.gd")

## Why a write did not happen. NONE is written (`restored` and `leftover` may still have words).
enum Failure {
	NONE,
	## The path is a folder.
	IS_FOLDER,
	## The file exists and this user may not write it (read-only, no permission, or locked):
	## nothing was moved.
	NOT_WRITABLE,
	## An earlier write left the last good copy in the backup, and it could not be moved back.
	RECOVER_RESTORE,
	## An earlier write left a backup beside a whole file, and it could not be removed.
	RECOVER_CLEAR,
	## An earlier write left its temporary file, and it could not be removed.
	RECOVER_TEMP,
	## The temporary file could not be opened: the name cannot be used here, or the folder
	## cannot be written.
	OPEN,
	## The bytes could not all be written to the temporary file.
	STORE,
	## The old file could not be moved aside: it is where it was.
	BACKUP,
	## The new file could not be moved into place: the old one is back where it was.
	PLACE,
	## The new file could not be moved into place, nor the old one back: `temp` holds the
	## new text and `backup` the old one.
	PLACE_LOST,
}

## The beside-files' names: `<folder>/<PREFIX><name><suffix>`.
const PREFIX := "."
const TEMP_SUFFIX := ".pharmakos-write"
const BACKUP_SUFFIX := ".pharmakos-keep"


## Every file operation this helper makes. Each answers what it did: a bool for a question, a
## Godot Error for a step. A check passes its own, which fails a chosen step.
class Disk:
	extends RefCounted

	## Whether `path` is a file.
	func exists(path: String) -> bool:
		return FileAccess.file_exists(path)

	## Whether `path` is a folder.
	func is_folder(path: String) -> bool:
		return DirAccess.dir_exists_absolute(path)

	## `path`'s text: `error` OK and `text`, or the Error that stopped the read.
	func read(path: String) -> Dictionary:
		var file := FileAccess.open(path, FileAccess.READ)
		if file == null:
			return {"error": FileAccess.get_open_error(), "text": ""}
		var text := file.get_as_text()
		var error := file.get_error()
		file.close()
		# Reading to the end sets ERR_FILE_EOF, which is the read having worked.
		return {"error": OK if error == ERR_FILE_EOF else error, "text": text}

	## Whether this user may write the existing file `path`, by opening it for read and write
	## without truncation and closing it: OK, or the Error that refused it. Changes nothing.
	func probe_writable(path: String) -> int:
		var file := FileAccess.open(path, FileAccess.READ_WRITE)
		if file == null:
			var refused := FileAccess.get_open_error()
			return refused if refused != OK else ERR_FILE_NO_PERMISSION
		file.close()
		return OK

	## `bytes` written to a new file at `path`: `failure` (Failure.OPEN or Failure.STORE, or
	## Failure.NONE) and its `error`.
	func write(path: String, bytes: PackedByteArray) -> Dictionary:
		var file := FileAccess.open(path, FileAccess.WRITE)
		if file == null:
			var opened := FileAccess.get_open_error()
			return {"failure": Failure.OPEN, "error": opened if opened != OK else ERR_CANT_OPEN}
		var stored := file.store_buffer(bytes)
		file.flush()
		var error := file.get_error()
		file.close()
		if not stored or error != OK:
			return {"failure": Failure.STORE, "error": error if error != OK else ERR_FILE_CANT_WRITE}
		return {"failure": Failure.NONE, "error": OK}

	## `from` moved to `to`, which must not exist: OK or the Error.
	func rename(from: String, to: String) -> int:
		return DirAccess.rename_absolute(from, to)

	## `path` removed: OK or the Error.
	func remove(path: String) -> int:
		return DirAccess.remove_absolute(path)


## The temporary file beside `path`.
static func temp_of(path: String) -> String:
	return path.get_base_dir().path_join(PREFIX + path.get_file() + TEMP_SUFFIX)


## The backup beside `path`.
static func backup_of(path: String) -> String:
	return path.get_base_dir().path_join(PREFIX + path.get_file() + BACKUP_SUFFIX)


## Writes `bytes` to `target`, whole or not at all, through `disk` (the real one when none is
## given). Answers `failure` (a Failure), `error` (the Godot Error of the step that failed, OK
## when written), `path`, `temp` and `backup` (the paths, absolute), `restored` (whether an
## earlier write's last good copy was moved back first, which the caller says) and `leftover`
## (a path a step could not remove, said by the caller; "" when none).
static func write(target: String, bytes: PackedByteArray, disk: Disk = null) -> Dictionary:
	if disk == null:
		disk = Disk.new()
	# One spelling of the path for every step (`user://` and `res://` made absolute).
	var path := ProjectSettings.globalize_path(target)
	var answer := {"failure": Failure.NONE, "error": OK, "path": path, "temp": temp_of(path), "backup": backup_of(path), "restored": false, "leftover": ""}
	if disk.is_folder(path):
		return _failed(answer, Failure.IS_FOLDER, ERR_ALREADY_EXISTS)
	var recovered := _recover(path, answer, disk)
	if recovered != Failure.NONE:
		return answer
	var had_old := disk.exists(path)
	if had_old:
		var writable := disk.probe_writable(path)
		if writable != OK:
			return _failed(answer, Failure.NOT_WRITABLE, writable)
	var written := disk.write(answer["temp"], bytes)
	if written["failure"] != Failure.NONE:
		answer["leftover"] = _removed(answer["temp"], disk)
		return _failed(answer, written["failure"], written["error"])
	if not had_old:
		var placed := disk.rename(answer["temp"], path)
		if placed != OK:
			answer["leftover"] = _removed(answer["temp"], disk)
			return _failed(answer, Failure.PLACE, placed)
		return answer
	var moved := disk.rename(path, answer["backup"])
	if moved != OK:
		answer["leftover"] = _removed(answer["temp"], disk)
		return _failed(answer, Failure.BACKUP, moved)
	var put := disk.rename(answer["temp"], path)
	if put != OK:
		var back := disk.rename(answer["backup"], path)
		if back != OK:
			return _failed(answer, Failure.PLACE_LOST, put)
		answer["leftover"] = _removed(answer["temp"], disk)
		return _failed(answer, Failure.PLACE, put)
	answer["leftover"] = _removed(answer["backup"], disk)
	return answer


## Step 1: what an earlier write that stopped left beside `path`, put right. Answers NONE, or
## the recovery failure it set on `answer`.
static func _recover(path: String, answer: Dictionary, disk: Disk) -> int:
	if disk.exists(answer["backup"]):
		if not disk.exists(path):
			# It stopped between moving the old file aside and moving the new one in: the
			# backup is the last good copy.
			var back := disk.rename(answer["backup"], path)
			if back != OK:
				_failed(answer, Failure.RECOVER_RESTORE, back)
				return Failure.RECOVER_RESTORE
			answer["restored"] = true
		else:
			# It landed, and stopped before removing the backup.
			var cleared := disk.remove(answer["backup"])
			if cleared != OK:
				_failed(answer, Failure.RECOVER_CLEAR, cleared)
				return Failure.RECOVER_CLEAR
	if disk.exists(answer["temp"]):
		var removed := disk.remove(answer["temp"])
		if removed != OK:
			_failed(answer, Failure.RECOVER_TEMP, removed)
			return Failure.RECOVER_TEMP
	return Failure.NONE


## A file the caller wrote whole: `path`'s text through `disk` (the real one when none is
## given): `error` OK and `text`, or the Error that stopped the read.
static func read(target: String, disk: Disk = null) -> Dictionary:
	if disk == null:
		disk = Disk.new()
	return disk.read(ProjectSettings.globalize_path(target))


## The name a failure goes by in the string table's keys (`write_<name>`), lower case.
static func failure_name(failure: int) -> String:
	return String(Failure.find_key(failure)).to_lower()


## The answer of `write` in the string table's words: the restore first (`write_restored`),
## then the failure (`write_<failure>`), then a leftover (`write_leftover`); "" when there is
## nothing to say. Each is true of what is on disk in that state.
static func why(answer: Dictionary) -> String:
	var said: PackedStringArray = []
	if answer.get("restored", false):
		said.append(Strings.text("write_restored", answer))
	var failure: int = answer.get("failure", Failure.NONE)
	if failure != Failure.NONE:
		said.append(Strings.text("write_" + failure_name(failure), answer))
	if String(answer.get("leftover", "")) != "":
		said.append(Strings.text("write_leftover", answer))
	return " ".join(said)


## `path` removed through `disk`, or `path` itself when it could not be: the caller says a
## leftover.
static func _removed(path: String, disk: Disk) -> String:
	if not disk.exists(path):
		return ""
	return "" if disk.remove(path) == OK else path


static func _failed(answer: Dictionary, failure: int, error: int) -> Dictionary:
	answer["failure"] = failure
	answer["error"] = error
	return answer

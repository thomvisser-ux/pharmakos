# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# A file written whole or not at all (S3's `fixq-c`, fix pass 3): Save and Save as...
# (`editor.gd`), and the lobby's remembered config line and its forgetting (`lobby.gd`).
#
# A file written in place and failing part-way leaves half of it under its own name, and a
# half playbook or a half config line is worse than the old one. So the bytes go to
# `<path>.tmp` in the same folder first, every Error checked; then the old file, if there is
# one, is moved aside to `<path>.bak`, the new one is moved into its place, and the old one
# is removed. No step renames over an existing file: Godot's `DirAccess.rename` over one
# succeeds on Windows (measured on 4.7.2), but this sequence does not lean on how it does it.
# At every moment the whole old file or the whole new one is under a name, and a failure says
# which name holds what: a failure before the move leaves the old file where it was and
# removes the temporary one; a failure after the old file was moved aside moves it back; and
# if even that fails, the answer names where the new text and the old one are (`PLACE_LOST`).
# A `.tmp` or `.bak` already beside the file (left by a failure that was said) is never
# overwritten or removed here: it may hold the only whole copy of something, so the write
# refuses and names it.
#
# This decides nothing about the game: it writes bytes the caller hands it, and says why it
# could not.

extends RefCounted

const Strings := preload("res://scripts/strings.gd")

## Why a write did not happen, or did with something left behind. NONE is written.
enum Failure {
	NONE,
	## The path is a folder.
	IS_FOLDER,
	## `<path>.tmp` is already there.
	TEMP_IN_WAY,
	## `<path>.bak` is already there.
	BACKUP_IN_WAY,
	## The temporary file could not be opened (the folder cannot be written, or is missing).
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

## The suffixes beside the target, in its own folder.
const TEMP := ".tmp"
const BACKUP := ".bak"


## Writes `bytes` to `path`, whole or not at all. Answers `failure` (a Failure), `error` (the
## Godot Error of the step that failed, OK when written), `temp` and `backup` (the paths
## beside the target), and `leftover`: a path a step could not remove, said by the caller
## ("" when none), which a written file can also leave (an old copy at `backup`).
static func write(target: String, bytes: PackedByteArray) -> Dictionary:
	# One spelling of the path for every step (`user://` and `res://` made absolute), as the
	# lobby's removal always spelt it.
	var path := ProjectSettings.globalize_path(target)
	var temp := path + TEMP
	var backup := path + BACKUP
	if DirAccess.dir_exists_absolute(path):
		return _answer(Failure.IS_FOLDER, ERR_ALREADY_EXISTS, path, "")
	if FileAccess.file_exists(temp) or DirAccess.dir_exists_absolute(temp):
		return _answer(Failure.TEMP_IN_WAY, ERR_ALREADY_EXISTS, path, "")
	if FileAccess.file_exists(backup) or DirAccess.dir_exists_absolute(backup):
		return _answer(Failure.BACKUP_IN_WAY, ERR_ALREADY_EXISTS, path, "")
	var file := FileAccess.open(temp, FileAccess.WRITE)
	if file == null:
		var opened := FileAccess.get_open_error()
		return _answer(Failure.OPEN, opened if opened != OK else ERR_CANT_OPEN, path, _removed(temp))
	var stored := file.store_buffer(bytes)
	file.flush()
	var error := file.get_error()
	file.close()
	if not stored or error != OK:
		return _answer(Failure.STORE, error if error != OK else ERR_FILE_CANT_WRITE, path, _removed(temp))
	if not FileAccess.file_exists(path):
		var placed := DirAccess.rename_absolute(temp, path)
		if placed != OK:
			return _answer(Failure.PLACE, placed, path, _removed(temp))
		return _answer(Failure.NONE, OK, path, "")
	var moved := DirAccess.rename_absolute(path, backup)
	if moved != OK:
		return _answer(Failure.BACKUP, moved, path, _removed(temp))
	var put := DirAccess.rename_absolute(temp, path)
	if put != OK:
		var back := DirAccess.rename_absolute(backup, path)
		if back != OK:
			return _answer(Failure.PLACE_LOST, put, path, "")
		return _answer(Failure.PLACE, put, path, _removed(temp))
	return _answer(Failure.NONE, OK, path, _removed(backup))


## The name a failure goes by in the string table's keys (`<prefix>_<name>`), lower case.
static func failure_name(failure: int) -> String:
	return String(Failure.find_key(failure)).to_lower()


## The answer of `write` in the string table's words (`write_<failure>`, then `write_leftover`
## when a step left a file it could not remove): what was written where, never more.
static func why(answer: Dictionary) -> String:
	var said := ""
	var failure: int = answer.get("failure", Failure.NONE)
	if failure != Failure.NONE:
		said = Strings.text("write_" + failure_name(failure), answer)
	if String(answer.get("leftover", "")) != "":
		said = (said + " " if said != "" else "") + Strings.text("write_leftover", answer)
	return said


## `path` removed, or `path` itself when it could not be: the caller says a leftover.
static func _removed(path: String) -> String:
	if not FileAccess.file_exists(path):
		return ""
	return "" if DirAccess.remove_absolute(path) == OK else path


static func _answer(failure: int, error: int, path: String, leftover: String) -> Dictionary:
	return {"failure": failure, "error": error, "path": path, "temp": path + TEMP, "backup": path + BACKUP, "leftover": leftover}

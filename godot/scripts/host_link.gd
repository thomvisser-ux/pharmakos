# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The lobby's link to its match host: the child process, its two pipe ends, and the two
# WebSocket connections.
#
# Decisions-log item 107 (2), confirmed by the owner as item 108 (2): the lobby spawns
# `gamectl host` beside the executable with OS.execute_with_pipe, writes ONE config line
# on its standard input, reads ONE announce line (port, match id, admin token, seat token)
# from its standard output, keeps standard input open for the life of the match and closes
# it on quit - which is the whole of the host's shutdown protocol.
#
# Then two connections, both to 127.0.0.1 at the announced port, each with its own token
# in an `Authorization: Bearer` handshake header: the ADMIN connection, which controls the
# match, and the SEAT connection, which watches it. Before connecting, each peer's inbound
# buffer is raised to 1 MiB, because Godot's default of about 64 KiB is smaller than one
# page of the view.
#
# THE TOKENS LIVE IN THIS SCRIPT'S VARIABLES AND NOWHERE ELSE: never printed, never
# logged, never written to a file, a scene or a resource, never put on the config line or
# the resume line. The announce line is parsed and dropped; the error paths below print the
# host's standard error and hand it to the lobby to show, which is safe because the host
# keeps it free of tokens (it writes them to standard output only, and only once).
#
# THE MATCH ID (T19 pull request 2; decisions-log item 113 (12) and (13)): `match_id`
# below is the one helper that names a match, `<prefix>-<unix seconds>-<pid>`, so a new
# match never reuses the id of an old one whose save is still in the private match cache
# (a new match under an id with a save is refused). It reads the clock ONCE, to name the
# match, and computes nothing with it: the seconds are a name, not a time.
#
# What each connection SENDS, and when, is not decided here: this script moves text
# between the sockets and the bridge's watch rig (crates/client-gdext/src/rig.rs), which
# holds the pacer, the host clock and the keep-alive. GDScript is views and plumbing
# (AGENTS.md section 3 rule 4).

extends Node

## Emitted once the host has announced itself and both connections have been started.
signal announced(match_id: String)
## Emitted when the host could not be started or has gone away.
signal failed(reason: String)
## Emitted for every answer the bridge read: what `watch_receive` returned.
signal received(answer: Dictionary)

## The admin connection's index in the bridge's watch rig.
const ADMIN := 0
## The seat connection's index in the bridge's watch rig.
const SEAT := 1

## One view page is cut at 256 KiB of encoded runs, which base64 and JSON grow by about a
## third; 1 MiB holds a page with room to spare (skeleton-plan-t16a-notes.md section A (2)).
const INBOUND_BYTES := 1048576

## How many times a dropped connection is reopened before the link gives up.
##
## PLACEHOLDER: a local host that drops a connection more often than this has gone wrong
## in a way a retry will not fix. OWNER, at hardening, with the transport's other numbers.
const RECONNECTS := 5

## The match the lobby hosts until it has a match-settings screen.
##
## PLACEHOLDER: the golden seed (so the live vista is the fixture's map), two seats with
## the human at seat 0 and the other played by the built-in operator (Easy, since T18:
## it plans, submits and says ready for its seat every round), the rules table's own
## segment ladder, and the spec's default round limit of six. The lobby's settings screen
## is OWNER's, with T22's stage demo and S1's Probation preset.
const MATCH_SEED := "0x00000000ca5caded"
const MATCH_SEATS := 2
const HUMAN_SEAT := 0
const ROUND_LIMIT := 6
const LADDER := "-"

var bridge: Node = null

var _pid := -1
var _stdio: FileAccess = null
var _stderr: FileAccess = null
## What the host wrote on its standard error before it announced, which it keeps free of
## tokens.
var _stderr_text := ""
var _announce := PackedByteArray()
var _port := 0
var _tokens := ["", ""]
var _peers: Array = [null, null]
var _open := [false, false]
var _reconnects := 0
var _done := false


## The `gamectl` binary: `--gamectl=` after `--` on the command line, then the
## PHARMAKOS_GAMECTL environment variable, then beside the executable - which is where a
## packaged build keeps it (skeleton-plan-t16a-notes.md section B, T21).
static func find_gamectl() -> String:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--gamectl="):
			return arg.trim_prefix("--gamectl=")
	var named := OS.get_environment("PHARMAKOS_GAMECTL")
	if named != "":
		return named
	var binary := "gamectl.exe" if OS.get_name() == "Windows" else "gamectl"
	return OS.get_executable_path().get_base_dir().path_join(binary)


## The repository root the host reads `rules/rules.v1.json` from: `--root=` after `--`,
## then PHARMAKOS_ROOT, then the folder above this project.
##
## PLACEHOLDER: a packaged build has no repository; where the host finds its rules file
## there is packaging's decision, OWNER at T21 (the gamectl `host` command's own note).
static func find_root() -> String:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--root="):
			return arg.trim_prefix("--root=")
	var named := OS.get_environment("PHARMAKOS_ROOT")
	if named != "":
		return named
	return ProjectSettings.globalize_path("res://").trim_suffix("/").get_base_dir()


## The one config line `pharmakos_gateway::serve::Config` reads: match id, seed, seats,
## the human seat, the segment ladder and the round limit, tab separated. `seats` is the
## lobby's two unless a caller names another (the headless watch check hosts one, so that
## its own Ready is every seat's).
static func config_line(match_id: String, ladder: String, seats: int = MATCH_SEATS) -> String:
	return "%s\t%s\t%d\t%d\t%s\t%d\n" % [match_id, MATCH_SEED, seats, HUMAN_SEAT, ladder, ROUND_LIMIT]


## The resume line for a remembered six-field `line`: the same six fields, a tab and
## `resume` (decisions-log item 111, decision C9). Nothing else changes, so a refusal says
## the save and this line disagree, and nothing is retried with other values.
static func resume_line(line: String) -> String:
	return line.strip_edges(false, true) + "\tresume\n"


## A match id no earlier run can have used: `<prefix>-<unix seconds>-<pid>`, lower-case
## letters, digits and hyphens, within the gateway's 64 characters. THE ONE CLOCK READ in
## this script: the seconds name the match and nothing is computed with them (AGENTS.md
## section 3 rule 4, as decisions-log item 113 (12) words it).
static func match_id(prefix: String) -> String:
	return "%s-%d-%d" % [prefix, int(Time.get_unix_time_from_system()), OS.get_process_id()]


## The seat the human plays, spelt as the gateway spells a seat.
static func my_seat() -> String:
	return "seat.%d" % HUMAN_SEAT


## Starts the host and, once it announces itself, both connections.
##
## `gamectl` is the path of the binary, `root` the repository root it reads the rules
## table from, and `config_line` the one line `pharmakos_gateway::serve::Config` reads.
func start(gamectl: String, root: String, config_line: String) -> bool:
	if bridge == null:
		failed.emit("the host link has no bridge")
		return false
	var spawned: Dictionary = OS.execute_with_pipe(gamectl, PackedStringArray(["host", "--root", root]), false)
	if spawned.is_empty():
		failed.emit("could not start %s" % gamectl)
		return false
	_pid = int(spawned.get("pid", -1))
	_stdio = spawned.get("stdio")
	_stderr = spawned.get("stderr")
	_stdio.store_string(config_line)
	_stdio.flush()
	bridge.watch_begin()
	return true


## The host's process id, or -1 before it was started.
func host_pid() -> int:
	return _pid


## What the host wrote on its standard error before it announced: its own words,
## token-free, which the lobby shows when the host refuses a line.
func host_error() -> String:
	_drain_stderr()
	return _stderr_text.strip_edges()


## Whether the host process is still running.
func host_running() -> bool:
	return _pid >= 0 and OS.is_process_running(_pid)


## One frame of plumbing: read the pipes until the announce line is in, then move text
## between the two sockets and the bridge.
func pump() -> void:
	if _done:
		return
	_drain_stderr()
	if _port == 0:
		_read_announce()
		return
	for link in [ADMIN, SEAT]:
		_pump_link(link)
	for pair in bridge.watch_outbox():
		var peer: WebSocketPeer = _peers[int(pair[0])]
		if peer != null and peer.get_ready_state() == WebSocketPeer.STATE_OPEN:
			peer.send_text(String(pair[1]))


## Closes both connections and the host's standard input, which ends the host.
func quit() -> void:
	_done = true
	for peer in _peers:
		if peer != null:
			peer.close()
	if _stdio != null:
		_stdio.close()
		_stdio = null
	_tokens = ["", ""]


func _read_announce() -> void:
	var chunk := _stdio.get_buffer(4096)
	if chunk.size() > 0:
		_announce.append_array(chunk)
	var text := _announce.get_string_from_utf8()
	var end := text.find("\n")
	if end < 0:
		if not host_running():
			_done = true
			# The host's own words (a refused resume line, a taken match id), as it wrote
			# them; the generic sentence only when it wrote nothing.
			var said := host_error()
			failed.emit(said if said != "" else "the host exited before it announced a match")
		return
	var fields := text.substr(0, end).strip_edges().split("\t")
	_announce = PackedByteArray()
	if fields.size() < 4 or not fields[0].is_valid_int():
		_done = true
		failed.emit("the host's first line was not an announce line")
		return
	_port = int(fields[0])
	_tokens = [fields[2], fields[3]]
	for link in [ADMIN, SEAT]:
		_connect(link)
	announced.emit(fields[1])


func _connect(link: int) -> void:
	var peer := WebSocketPeer.new()
	peer.inbound_buffer_size = INBOUND_BYTES
	peer.handshake_headers = PackedStringArray(["Authorization: Bearer " + String(_tokens[link])])
	var error := peer.connect_to_url("ws://127.0.0.1:%d/" % _port)
	if error != OK:
		failed.emit("could not open connection %d" % link)
	_peers[link] = peer
	_open[link] = false


func _pump_link(link: int) -> void:
	var peer: WebSocketPeer = _peers[link]
	if peer == null:
		return
	peer.poll()
	var state := peer.get_ready_state()
	if state == WebSocketPeer.STATE_OPEN:
		if not _open[link]:
			_open[link] = true
			bridge.watch_opened(link)
		while peer.get_available_packet_count() > 0:
			var text := peer.get_packet().get_string_from_utf8()
			var answer: Dictionary = bridge.watch_receive(link, text)
			if not answer.is_empty():
				received.emit(answer)
	elif state == WebSocketPeer.STATE_CLOSED and not _done:
		if _open[link]:
			bridge.watch_dropped(link)
		_open[link] = false
		if _reconnects >= RECONNECTS or not host_running():
			_done = true
			failed.emit("connection %d closed (%d) and the host is gone or will not take it back" % [link, peer.get_close_code()])
			return
		_reconnects += 1
		_connect(link)


func _drain_stderr() -> void:
	if _stderr == null:
		return
	while true:
		var chunk := _stderr.get_buffer(4096)
		if chunk.size() == 0:
			return
		var text := chunk.get_string_from_utf8()
		# Kept only until the host announces: it is read on the pre-announce failure path
		# alone, so a long match does not grow it. Everything is still printed.
		if _port == 0:
			_stderr_text += text
		printerr("[host] ", text.strip_edges())

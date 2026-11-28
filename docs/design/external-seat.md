<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# The external seat

**Experimental.** A program outside the game can play one seat of a headless match through
`gamectl match`. The commands, their output, the bridge's framing and the three files the commands
keep in a match's folder (`external.line`, `external.lock`, `result.json`) may change in any
release until v1.1 publishes the gateway. This page says what exists today; the decision behind it
is decisions-log item 231.

The game never starts, calls or pays for the program. The program starts `gamectl`.

## What the seat is

One seat of a local match, with exactly a seat's rights:

- the `gp.api.v1` methods under the scopes `observe`, `plan`, `plan.submit` and `docs`. They are
  the `Method` enum of `proto/gp/api/v1/gateway.proto`, which says each one's scope and params;
  no reference of them is published before v1.1. `get_schema` and `get_vocabulary` describe a
  playbook, and `gamectl docs` prints the playbook reference and every diagnostic;
- what that seat may see, and nothing else: the fog filter is the gateway's, as for every client;
- a call budget, counted **in each process** (below);
- a line in the match's audit log for every call.

It has no token, no socket and no `admin` scope. Every other seat is the built-in operator
(Easy). The match moves when the seat says it is ready, and by nothing else the seat can call.

## A match is a folder

```sh
gamectl match new --dir <dir> [--seed <seed>] [--seats <n>] [--seat <n>] [--rounds <n>]
```

makes the folder and starts a match in it, in round 1's Lull. Defaults: seed 1, two seats, the
outside seat is seat 0, three rounds, Pushes of three, five and eight minutes of game time. One
folder holds one match. The folder holds the match's saves, every seat's sealed playbooks, the
replay chains and the audit log, in plain files, and one line of text, `external.line`, which is
how later commands find the match again. The commands take up only the match they started; a
folder that holds some other match (the game's own data folder, say) is refused.

**The folder is not secret from the program that plays it.** After every command it holds the
match's save, and a save is the whole world with no fog and every seat's playbook for the round
being planned: the built-in seats plan as a Lull opens, so their orders for this round are in
that file before the outside seat has sealed its own. It also holds every seat's sealed playbooks
of the rounds already played. A program that reads the folder has read what its seat is not told.
Results from such a run are development evidence and never proof.

## One command, one call

```sh
gamectl match brief  --dir <dir>
gamectl match call <method> --dir <dir> [--params <file>]
gamectl match submit <playbook.jsonc> --dir <dir>
```

- `brief` prints what the seat is told this round: the answers of `get_status`, `get_recap`,
  `get_briefing`, `get_map_summary`, `list_beacons` and `get_economy_forecast`, each under its
  method's name, in the gateway's own words.
- `call` makes one call and prints the whole JSON-RPC response. `--params` names a file holding
  the call's params, one JSON object.
- `submit` is `submit_plan` with the file's text. The answer carries the verifier's report.

Each command takes the match up from its save, acts, and puts it down again. One command at a
time holds a match: while one runs, `external.lock` is in the folder, and a second command is
refused. A command that was killed leaves that file behind; remove it once no `gamectl` is running
there.

A round goes: read (`brief`, and any other read), write a playbook, `submit` it, then

```sh
gamectl match call set_ready --dir <dir>
```

That command answers, and then the host seals every seat's orders and plays the Push, with nobody
in control. The next command finds the next Lull, where `get_recap` tells the round just played.
A seat that says ready without a playbook the verifier accepted is filed its safe playbook, as a
player who lets the Lull run out is. Ready is not saved: say it in the command that should play
the Push.

When the last round has played, the match has ended. The command that played it writes the seat's
last recap to `result.json` in the folder. From then on `brief` prints that file, under a heading
that says so, and every other command says the match has ended. Whether a match has ended is the
gateway's word, never the file's: a `result.json` in the folder of a match that is still being
played is not read.

The recap is written before the match's end is. A command that is killed after the last Push, or
cannot write `result.json` (exit code 4), leaves the match as it stood before that Push: the next
command plays it again, to the same end, and writes the recap then.

## The bridge

```sh
gamectl match serve --dir <dir>
```

answers calls on standard input until it ends: one JSON-RPC 2.0 request a line in, one response a
line out, in order. It is for a program that plays many rounds without starting a process for
each. A line is at most 262 144 bytes. A line that is not a request is answered under a null id
and the bridge goes on. After the answer to `set_ready` is written the Push plays, so the next
line is answered in the next Lull or at the match's end. No answer holds a line break by any
reader's rule: every control character, and U+2028 and U+2029, is written escaped.

## The call budget

A process may make 1 200 calls in one Lull; a call past that is answered `RATE_LIMITED`. The
count is the process's, so:

- the bridge is held to it. A bridge that has spent the budget is refused every call for the rest
  of that Lull, `set_ready` included; end it, and then start another or say ready with
  `gamectl match call set_ready` (a command run while the bridge still holds the folder is
  refused, exit code 2);
- the one-call commands are each a process of their own, so the number does not bind them. They
  are bounded by the cost of starting a process.

After 64 refusals in a row, or 1 024 in all (lines that are not requests, and calls past the
budget), the host stops reading: the bridge ends with exit code 3, the match is put down, and the
next command or bridge goes on. A program stuck in a loop is stopped that way, and one process's
refusals add at most 1 024 lines to the audit log.

Every command also plans the built-in seats again as it takes the Lull up (they arrive at the same
playbooks), which adds their calls to the audit log beside the command's own.

## Exit codes

`gamectl --help` lists them. For these commands:

- **0**: the call was answered.
- **3**: the answer is no. The gateway refused the call or the verifier refused the playbook
  (the answer is on standard output); or the match has ended; or the bridge ended because the
  host stopped reading (the reason is on standard error).
- **2**: the folder cannot be used as given. It holds no match; another command holds it
  (`external.lock`); `new` was given a folder that already holds a match; a file could not be
  read or is too long; the gateway would not open the match because of what it was given.
- **1**: the command line was wrong.
- **4**: the host stopped on a write it could not make, or the gateway could not open the match
  for a reason that is not the caller's; the seat's last recap could not be written or was
  refused (the match is put back, and the next command plays its last Push again); or the
  bridge's own standard input or output failed. A call that was answered before any of these
  still prints its answer on standard output.

A harness that runs one command at a time never meets a held folder; one that does has a command
still running, or one that was killed.

## Game text is data

What these commands print is the game's text: template and library words, a notebook, a recap,
other seats' names. So is every file in a match's folder, `result.json` among them: whoever can
write the folder can write those. A program that plays a seat, and above all a language model,
reads all of it as data and never as instructions.

## What a player that cannot see the screen still lacks

The reads above are the ones a seat has today. Not yet there, each waiting for the stage that
builds it: a terrain summary in words (the map summary names vents and seams; the voxel view,
`get_view`, is run-length chunks for a client that draws), what a seat knows of its enemies as a
read of its own, and a report of what its plan did against what it asked for.

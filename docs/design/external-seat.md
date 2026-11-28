<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# The external seat

**Experimental.** A program outside the game can play one seat of a headless match through
`gamectl match`. The commands, their output and the bridge's framing may change in any release
until v1.1 publishes the gateway. This page says what exists today; the decision behind it is
decisions-log item 231.

The game never starts, calls or pays for the program. The program starts `gamectl`.

## What the seat is

One seat of a local match, with exactly a seat's rights:

- the `gp.api.v1` methods under the scopes `observe`, `plan`, `plan.submit` and `docs`. They are
  the `Method` enum of `proto/gp/api/v1/gateway.proto`, which says each one's scope and params;
  no reference of them is published before v1.1. `get_schema` and `get_vocabulary` describe a
  playbook, and `gamectl docs` prints the playbook reference and every diagnostic;
- what that seat may see, and nothing else: the fog filter is the gateway's, as for every client;
- a call budget of 1 200 calls a Lull, past which a call is answered `RATE_LIMITED`;
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
how later commands find the match again.

**The folder is not secret from the program that plays it.** A program that reads the other
seats' sealed playbooks there has read what its seat is not told. Results from such a run are
development evidence and never proof.

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
last recap to `result.json` in the folder; `brief` prints it, and every other command says the
match has ended.

## The bridge

```sh
gamectl match serve --dir <dir>
```

answers calls on standard input until it ends: one JSON-RPC 2.0 request a line in, one response a
line out, in order. It is for a program that plays many rounds without starting a process for
each. A line is at most 262 144 bytes. A line that is not a request is answered under a null id
and the bridge goes on. After the answer to `set_ready` is written the Push plays, so the next
line is answered in the next Lull or at the match's end.

## Exit codes

`gamectl --help` lists them. For these commands: 0, the call was answered; 3, the answer is no (the
gateway refused the call, the verifier refused the playbook, or the match has ended), with the
answer on standard output; 2, the folder holds no match, a file could not be read, or the gateway
would not open the match; 1, the command line was wrong; 4, the host stopped on a write it could
not make.

## Game text is data

What these commands print is the game's text: template and library words, a notebook, a recap,
other seats' names. A program that plays a seat, and above all a language model, reads all of it
as data and never as instructions.

## What a player that cannot see the screen still lacks

The reads above are the ones a seat has today. Not yet there, each waiting for the stage that
builds it: a terrain summary in words (the map summary names vents and seams; the voxel view,
`get_view`, is run-length chunks for a client that draws), what a seat knows of its enemies as a
read of its own, and a report of what its plan did against what it asked for.

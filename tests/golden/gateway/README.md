<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Gateway goldens

What the Seat Gateway's **security surface** does, written down: who sees which
event, what a 60-second digest says, what the audit log records, and which
opening handshakes the gateway accepts. Produced by
`crates/gateway/tests/goldens.rs`; compared by `cargo xtask ci`'s `golden` step;
re-blessed with `cargo xtask golden --bless`, which a pull request then has to
explain (AGENTS.md §5).

These four exist because the surface is the thing the whole roadmap inherits
(skeleton plan T9, risk R8). A unit test says a rule holds; a golden says what
the rule *does*, in a table a reviewer can read without running anything — and
it is the shape of that table, not any one line of it, that v1.1 publishes.

## The four cases

| Case | File | What it pins |
| --- | --- | --- |
| `fog_one_tick/` | `expected.fog.txt` | One tick's events through every viewer's fog filter, under every policy |
| `segment_digest/` | `expected.digest.txt` | `get_segment_feed`'s 60-second digests and their per-kind counts |
| `audit_log/` | `expected.audit.txt` | One scripted session's access log |
| `handshake/` | `expected.handshake.txt` | Every opening handshake the gateway will and will not accept |

## What a diff means, case by case

### `fog_one_tick/expected.fog.txt`

One line per (policy, viewer, event): `shown` or `hidden`. Seat 0 has seen voxel
(12, 8, 33) and nothing else; seat 1 has seen nothing at all.

**A `hidden` that became `shown` is a fog leak until proved otherwise** — spec
§12 and decisions-log item 26, and the most expensive kind of bug this project
can ship, because a seat that learns something it should not have cannot unlearn
it and the match is already spoiled. Before blessing one, say which of the five
rules moved:

* a seat sees its **own** assets wherever they are (that is ownership, not fog);
* a seat sees a **place it has seen** (that is [`Vision`], the host's answer);
* a **casual** match has no fog for any seat, chosen when the match is made;
* an **eliminated** seat, and every seat at **match end**, sees everything — and
  keeps the token it already had, because no token is reissued mid-match;
* `spectate.nofog` lifts **fog** and never **secrecy**: the two `Private` rows
  are `hidden` for every viewer but the seat they belong to, and that is the one
  column in the file that no policy may ever change.

A `shown` that became `hidden` is less dangerous and still a behaviour change:
the watch rig and the live event list read this feed, so a seat that stops being
told about its own commander has lost a feature.

### `segment_digest/expected.digest.txt`

One line per 60 seconds of game time: the window, the total, the per-kind counts
and the deterministic prose.

A diff here means one of three things. **The cadence moved** — the windows are
60 000 ms from spec §12 and are not tuning, so a change to the boundaries is a
spec question. **The counting moved** — decisions-log item 97 owes the scenario
format a per-kind count that `event_count_in_range` can assert on at T15, so a
kind that stopped being counted, or started being counted twice, breaks an
assertion nobody has written yet. **The prose moved** — harmless to the machine,
visible to the player, and still a change worth a sentence.

The empty window (`1:00-2:00: nothing.`) is deliberate and load-bearing: a
minute in which nothing happened is news, and a client rendering a timeline needs
the gap to be there.

### `audit_log/expected.audit.txt`

`seq`, `tick`, `subject`, `handle`, `action`, `outcome` — one line per attempt,
whatever the answer was.

Read a diff here in two directions.

* **A line that disappeared** is worse than a line that changed: the log is the
  record of who asked for what, and a call that stops being logged is a call
  nobody can review. Every refusal is logged too, including one that never
  authenticated (`-` in both the subject and the handle columns).
* **A column that gained information** may be a leak. This is an *access* log:
  no token, no parameters and no results reach it, because a playbook, a draft,
  a notebook and a seat's knowledge are exactly what spec §12 says never leave
  the gateway — and on a local host anyone can read this file
  (`crates/gateway/src/cache.rs`). A handle (`t1`) names a token without being
  one; it is a counter, not a fingerprint of the secret.

The sequence numbers order attempts inside a tick, and they never restart: the
gateway buffers and the host flushes, and a restarted sequence would make the
file on disk unreadable.

### `handshake/expected.handshake.txt`

One line per case: the HTTP status and either the `Sec-WebSocket-Accept` value
or the reason for the refusal.

The `101` rows are the ones to look at first. `conforming`, `ipv6-loopback`,
`localhost-name` and `connection-list` are the four shapes a legitimate client
takes; **every other row in the file is a refusal, and a refusal that became a
101 is a hole in the surface** (AGENTS.md §7: localhost only, `Host` and
`Origin` checked, and no flag that widens it). In particular:

* `foreign-host` and `other-port` are the DNS-rebinding case — a name that
  resolves to `127.0.0.1` reaches this port legitimately at the IP layer, and the
  `Host` check is the only thing that turns it away;
* `browser-origin` and `loopback-origin` are both 403 because v1's allow-list is
  **empty**: every v1 client is a native process and sends no `Origin`, so an
  `Origin` at all means a web page;
* `no-authorization`, `basic-authorization` and `short-token` are 401 because the
  seat token rides the upgrade — an unauthenticated connection would be a
  connection the rate limiter cannot count.

The accept value `s3pPLMBiTxaQ9kYGzzhZRbK+xOo=` is RFC 6455 §1.3's own worked
example, reached through a SHA-1 this project owns (decisions-log item 99). **If
that value moves, the SHA-1 or the base64 is wrong and every WebSocket client in
the world will disagree with this server** — it is not a behaviour change to
bless, it is a bug to fix. `crates/gateway/src/sha1.rs` also pins the three FIPS
180-4 vectors for the same reason.

## Conventions

Tab-separated, one record per line, LF endings, a trailing newline, no `\r`
(`tests/golden/README.md` rule 3 — enforced, because `.gitattributes` marks this
tree `-text` so git will not normalise it for you). Comment lines start with `#`
and are part of the file: they are what a person reading a red build sees first.

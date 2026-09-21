<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Gateway goldens

What the Seat Gateway does, written down: who sees which event, what a
60-second digest says, what the audit log records, which opening handshakes the
gateway accepts, and what the spec's own worked session answers call by call.
Produced by `crates/gateway/tests/goldens.rs` and
`crates/gateway/tests/methods.rs`; compared by `cargo xtask ci`'s `golden` step;
re-blessed with `cargo xtask golden --bless`, which a pull request then has to
explain (AGENTS.md §5).

The first four exist because the surface is the thing the whole roadmap inherits
(skeleton plan T9, risk R8). A unit test says a rule holds; a golden says what
the rule *does*, in a table a reviewer can read without running anything — and
it is the shape of that table, not any one line of it, that v1.1 publishes. The
fifth is T13's, and is the method slice answering the session spec §12 prints.

## The five cases

| Case | File | What it pins |
| --- | --- | --- |
| `fog_one_tick/` | `expected.fog.txt` | One tick's events through every viewer's fog filter, under every policy |
| `segment_digest/` | `expected.digest.txt` | `get_segment_feed`'s 60-second digests and their per-kind counts |
| `audit_log/` | `expected.audit.txt` | One scripted session's access log |
| `handshake/` | `expected.handshake.txt` | Every opening handshake the gateway will and will not accept |
| `walkthrough/` | `expected.walkthrough.txt` | Spec §12's 14-call session, call by call, against a live gateway |

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

A digest covers the **whole segment as its viewer may see it**, never one page of
it: `get_segment_feed`'s `limit` and its `detail` budget cut the events, and the
counts stay put. A count that moved with the caller's page size would not be a
count `event_count_in_range` could assert on at T15, which is the whole of what
item 97 owes.

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

**No caller writes a line of this file.** The `action` column holds a method
name resolved against `gp.api.v1`, or the fixed literal `call <unknown>` for a
name the schema does not have — never the string the client sent. The file is
tab-separated and one record to a line, so a method name carrying a tab and a
newline would be a *forged record*: a seat with nothing but `observe` writing a
line that says another seat submitted a plan. `crates/gateway/src/audit.rs`
sanitises and bounds every action on top of that. If a row here ever grows a
column, loses one, or shows text a caller chose, that is the log failing at the
one thing it is for, not a formatting change.

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
  connection the rate limiter cannot count;
* `unpadded-key` and `url-safe-key` are 400 for a reason the file would not
  otherwise show: both **decode** to sixteen bytes, because the base64 reader
  this crate shares with the proto3 JSON mapping is required to accept unpadded
  text and the URL-safe alphabet. RFC 6455 §4.1 lets a client send neither, so
  the shape check in `handshake::is_sixteen_base64_bytes` is the only thing that
  turns them away. A 400 that became a 101 there means the strictness was
  dropped.

The accept value `s3pPLMBiTxaQ9kYGzzhZRbK+xOo=` is RFC 6455 §1.3's own worked
example, reached through a SHA-1 this project owns (decisions-log item 99). **If
that value moves, the SHA-1 or the base64 is wrong and every WebSocket client in
the world will disagree with this server** — it is not a behaviour change to
bless, it is a bug to fix. `crates/gateway/src/sha1.rs` also pins the three FIPS
180-4 vectors for the same reason.

### `walkthrough/expected.walkthrough.txt`

One line per call: the call's number, the method, and one phrase saying what
came back. Spec §12 prints this session and the skeleton plan's T13 acceptance
line asks for it end to end, so the file is the session's *shape* rather than
its contents — the contents are asserted in
`crates/gateway/tests/methods.rs`, which is where a reader should look for what
each line means.

Three lines are load-bearing and a diff in any of them is a behaviour change
rather than a wording one.

* **Line 2, `segment_length_ms=1000`.** The coming segment's length comes from
  the frozen snapshot and never from a constant (spec §10, "Segment end"; T10).
  A number here that matched the rules table's ladder instead would mean the
  gateway had started deriving it.
* **Line 11, `report_hash matches call 9`.** `submit_plan` always runs FULL, so
  the seal a seat gets is the report it was shown by its own pre-check (spec
  §11; decisions-log item 82). If this line ever stops saying it, the promise
  that makes a seal inspectable has gone, and no amount of blessing fixes that.
* **Line 7, `qualifies=false`.** An invalid playbook is a full report and not a
  method error. A `qualifies=true` here would mean the verifier stopped seeing
  the two errors the walkthrough puts in front of it; a refusal in its place
  would mean the gateway had started treating a bad playbook as a bad call.

Line 4 says `0 templates (no folder configured)` because no template folder is
set in the test, which is the plan's own T13 PLACEHOLDER (the folder's location
on each platform is the owner's, with packaging at T21). When that lands, this
line moves and the move is the feature arriving.

## Conventions

Tab-separated, one record per line, LF endings, a trailing newline, no `\r`
(`tests/golden/README.md` rule 3 — enforced, because `.gitattributes` marks this
tree `-text` so git will not normalise it for you). Comment lines start with `#`
and are part of the file: they are what a person reading a red build sees first.

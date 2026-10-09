<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Gateway goldens

What the Seat Gateway does, written down: who sees which event, what a
60-second digest says, what the audit log records, which opening handshakes the
gateway accepts, what the spec's own worked session answers call by call, and —
since T16a — what a **camera** is shown, since T18a what the editor's
**wizard** is handed, since T22a what the stage demo's own playbooks verify
and render to, since S1's `tgtw` what a seat is told about the map's vents
and seams, and since S1's `econ` what a seat is told about its own economy.
Produced by `crates/gateway/tests/goldens.rs`,
`crates/gateway/tests/methods.rs`, `crates/gateway/tests/view.rs`,
`crates/gateway/tests/advice.rs`, `crates/gateway/tests/demo_playbooks.rs`,
`crates/gateway/tests/targeting.rs` and `crates/gateway/tests/economy.rs`;
compared by `cargo xtask ci`'s `golden` step;
re-blessed with `cargo xtask golden --bless`, which a pull request then has to
explain (AGENTS.md §5).

The first four exist because the surface is the thing the whole roadmap inherits
(skeleton plan T9, risk R8). A unit test says a rule holds; a golden says what
the rule *does*, in a table a reviewer can read without running anything — and
it is the shape of that table, not any one line of it, that v1.1 publishes. The
fifth is T13's, and is the method slice answering the session spec §12 prints.
The next three are T16a's, the ninth is T18a's, the next ten are T22a's, the
next two are S1's targeting surfaces (task `tgtw`), and the last three are S1's
economy surfaces (task `econ`).

## The twenty-four cases

| Case | File | What it pins |
| --- | --- | --- |
| `fog_one_tick/` | `expected.fog.txt` | One tick's events through every viewer's fog filter, under every policy |
| `segment_digest/` | `expected.digest.txt` | `get_segment_feed`'s 60-second digests and their per-kind counts |
| `audit_log/` | `expected.audit.txt` | One scripted session's access log |
| `handshake/` | `expected.handshake.txt` | Every opening handshake the gateway will and will not accept |
| `walkthrough/` | `expected.walkthrough.txt` | Spec §12's 14-call session, call by call, against a live gateway |
| `view_one_tick/` | `expected.view.txt` | One tick's terrain and entities through every viewer's `get_view` |
| `view_keyframe/` | `expected.keyframe.jsonl`, `expected.keyframe.txt` | Seat 0's whole-map keyframe on the golden seed — **the fixture T16 renders from** |
| `walkthrough_watch/` | `expected.watch.txt` | A watch session: keyframe, clock, `end_lull`, speed, skip, `end_recap` |
| `instantiate_suggested/` | `expected.response.json` | One `instantiate_template{suggested: true}` answer on the wire — **the fixture T19's wizard reads** |
| `demo_hold_and_build_verify/` | `expected.response.json` | Seat 0's `verify_plan{depth: "full"}` answer for Hold & Build with its own values (nothing suggested), in the lobby's round 1 Lull — its `report_hash` |
| `demo_hold_and_build_render/` | `expected.response.json` | The same playbook's `render_plan` answer — its prose |
| `demo_expand_and_mine_verify/` | `expected.response.json` | As above, for Expand & Mine |
| `demo_expand_and_mine_render/` | `expected.response.json` | As above, for Expand & Mine |
| `demo_safe_playbook_verify/` | `expected.response.json` | As above, for the Safe Playbook template |
| `demo_safe_playbook_render/` | `expected.response.json` | As above, for the Safe Playbook template |
| `demo_against_easy_verify/` | `expected.response.json` | As above, for `scenarios/skeleton/against-easy.playbook.jsonc` as committed |
| `demo_against_easy_render/` | `expected.response.json` | As above, for `scenarios/skeleton/against-easy.playbook.jsonc` as committed |
| `demo_deploy_and_visit_verify/` | `expected.response.json` | As above, for `scenarios/skeleton/deploy-and-visit.playbook.jsonc` as committed |
| `demo_deploy_and_visit_render/` | `expected.response.json` | As above, for `scenarios/skeleton/deploy-and-visit.playbook.jsonc` as committed |
| `map_summary/` | `expected.response.json` | Seat 0's `get_map_summary` in the opening Lull on the golden seed: every vent and seam, its grade, liveness, coverage by seat 0 and travel from its commander |
| `resolve_refs/` | `expected.response.json` | Seat 0's `resolve_refs` in the same Lull for `scenarios/s1/cover-nearest-vent.playbook.jsonc`: what its `covering` and its `on {covered {}}` read, with every candidate in rank order |
| `briefing/` | `expected.response.json` | Seat 0's `get_briefing` in the opening Lull on the golden seed: its own standing (score and displayed rank on the final audit's terms) and the prose |
| `economy_forecast/` | `expected.response.json` | Seat 0's `get_economy_forecast` in the same Lull once it has sealed `scenarios/s1/cover-nearest-vent.playbook.jsonc`: the four present-state figures and the committed spend |
| `recap/` | `expected.response.json` | Seat 0's `get_recap` in round 1's recap of the same match: the round's tick count and the seat's own settlement line |

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
* **Line 7, `qualifies=false, 2 errors`.** An invalid playbook is a full report
  and not a method error. A `qualifies=true` here would mean the verifier
  stopped seeing the two errors the walkthrough puts in front of it; a count
  other than 2 would mean one broken step had started producing a different
  number of diagnostics, which is the spec's own `(2 errors, with fixes)`
  moving; and a refusal in its place would mean the gateway had started
  treating a bad playbook as a bad call.

**Fifteen rows, fourteen calls, and the file says so in its header.** Call 13 is
made twice with the host ending the Lull between the two, because `wait_for`
polls and never blocks — the gateway reads no clock, so "the seat waits for the
phase to change" *is* two calls. A file with one row 13 would be describing a
gateway that blocks.

Line 8 says the fixes came from the report itself: call 8's patch is built from
the diagnostics' own `json_patch` suggestions, which is the loop the editor runs
and the other half of the spec's `with fixes`.

Line 3 says what an **own** beacon carries since T18a (decisions-log item 111,
decision C7): `owner`, and for the seat's own beacons only `core`, `priority`
and `powered`. A diff here is the wire telling a seat something new or no longer
telling it something; another seat's beacon carries its `owner` and nothing
more, which `crates/gateway/tests/advice.rs` pins under both fog policies.

Line 4 reads the committed `library/` folder, as `gamectl host` does since
T18a, and counts it: `0 templates tagged attack, of 3 in the library`. The
skeleton's three templates are Hold & Build, Expand & Mine and the Safe Playbook
(item 81), none of them attack-shaped. The count moves when the library does,
and the first number moves when an attack template lands (S3). Before T18a the
line said `0 templates (no folder configured)`, because no host passed a
folder.

### `view_one_tick/expected.view.txt`

One line per (policy, viewer): how many chunks that viewer's keyframe carried,
what its **own bytes** say at the two voxels that were turned to air in the same
tick, and which entities it was listed. One of the two voxels is inside seat 0's
core sphere and the other is outside it, in the *same chunk* — which is the case
the fog rule is actually about.

Read it the way `fog_one_tick` is read, and with the same warning: **a `dirt` or
a `stone` that became `air` is a fog leak until proved otherwise**. Four rules
decide every cell in the table:

* the **generated map is served whole** to every viewer that gets a view at all
  (decisions-log item 107 (1)), so the chunk count is the whole map or zero;
* an **edit** is fogged voxel by voxel: the current byte where the viewer is
  unfogged or its `Vision` reaches that voxel, and the generated byte everywhere
  else;
* `admin` and a spectator **without** `spectate.nofog` get an empty view — no
  terrain, no entities — which is `fog_one_tick`'s own table said once more;
* an entity id is a **handle this viewer was given**, in the order it first saw
  the thing. Two seats each seeing one unit of their own both read `u_1`, and
  that is correct: a dense row id would be a count of everything the match has
  ever made. A `u_` number that jumped is that leak arriving.

The `chunks` column moving from 288 to something smaller means the *keyframe*
stopped being the whole map, which is item 107 (1) being reversed and is the
owner's sentence rather than a lane's.

### `view_keyframe/`

`expected.keyframe.jsonl` is seat 0's `get_view` keyframe at the opening Lull of
the golden seed: **one served `GetViewResponse` per line, in page order**, which
is the fixture T16's hostless screenshot and `client-gdext`'s decode tests load.
`expected.keyframe.txt` is its human-diffable twin — per chunk the origin, the
run count and the sim's own digest, then the entity list.

It is produced and drift-checked **here**, by the gateway, because the gateway is
the producer; a fixture made by the consumer would test nothing but the consumer.
T16 commits a byte-identical copy under `godot/fixtures/`, guarded by a test that
compares the two.

A diff here means one of six things, and the first is the only cheap one:

* **the map generator moved.** `tests/golden/mapgen/` will have moved too, and
  that is where the explanation belongs.
* **the run-length encoding moved.** `pharmakos_proto::chunk_rle` is a byte
  format inside a `bytes` field, which is a contract `buf breaking` cannot see —
  so this golden and `ViewChunk`'s own comment are the only things guarding it.
* **the view's shape moved.** A field added, removed or renamed in
  `GetViewResponse`, `ViewChunk` or `ViewEntity` is an AGENTS.md §5 change.
* **what seat 0 is entitled to moved.** At the opening Lull nothing has been
  written yet, so every chunk here is the generated one; a diff that is *only* in
  some chunks means the fog rule changed under a keyframe.
* **only `next_cursor` moved.** Then what changed is what the view's id is
  minted from (`ViewFeed::attach`), and nothing a client draws moved at all.
  The id is a digest of the match id, the match seed, the chunk count and how
  many times the feed has been attached — deterministic on purpose, which is
  what lets a rendered cursor sit in a golden.
* **only the `_status` footer's `phase_remaining_ms` moved.** The footer is the
  opening Lull's, with the whole first Lull, 600 000 ms
  (`match.first_lull_ms`), reported left. Its history: S1's first contract pull
  request moved it from 180 000 to 300 000 when it re-ruled the register's D-01
  (decisions-log item 127 (2)), and `godot/fixtures/view_keyframe.jsonl` was
  re-copied with it; since S1's `econ` the opening Lull is
  `match.first_lull_ms`, and the producer pinned the report at 300 000 until
  S1's `build` lane dropped the pin (decisions-log item 135 (2) (h)), when
  `godot/fixtures/` was re-copied in the same commit, byte for byte. A timed footer carries no
  `untimed` member: it is written only when true.

The `.txt` header carries **two** byte figures and they are different numbers:
`run bytes` is what the encoder produced and what `VIEW_PAGE_BYTES` budgets,
and `json bytes` is the served response once `voxels_rle` has become base64.

Measured at 288 chunks, about 197 KiB of encoded runs and 279 KiB of JSON, which
is **one page** at `VIEW_PAGE_BYTES` = 256 KiB. Paging is therefore not exercised
by this fixture and is covered by `viewfeed`'s own
`a_keyframe_wider_than_a_page_is_paged_in_ascending_chunk_order` instead. If the
file ever passes 1 MiB it is cut to the chunks near seat 0's core and this
paragraph says so (skeleton-plan T16a notes, decision 5).

### `walkthrough_watch/expected.watch.txt`

One line per call of a watch session, across the **two** connections the Godot
lobby holds: the camera's seat connection and the lobby's admin connection.

Three lines are load-bearing.

* **`report_host_clock` moving the tick.** The gateway reads no clock and a
  recap spends no sim tick, so without this call every token holds
  `CALLS_PER_TICK` calls for the whole recap and for ever after match end —
  which is where the full-map unlock lands. A `tick moved by 0` here is that
  hole re-opening.
* **`advanced_ms`.** `advance_push` takes and answers **game milliseconds** and
  stops at segment end; a client carries `asked - advanced_ms` and never learns
  how long a tick is. A tick or a hash appearing in this column would be the
  first time either reached the wire (decisions-log item 107 (3)).
* **the skip's call count.** A skip is `advance_push {60 000}` repeated until the
  `_status` footer leaves `PUSH`. One call covers a one-second segment; a number
  that grew means the cap or the flooring moved.

The `0 chunks` deltas are honest and not a bug: at the skeleton nothing in a
Push writes a voxel — mining is T14's and craters are S2's — so a watched Push
changes entity positions and no terrain at all.

### `instantiate_suggested/expected.response.json`

One whole JSON-RPC response, exactly as the wire carries it: seat 0's
`instantiate_template{template_id: "hold_and_build", suggested: true}` on the
golden seed, in the opening Lull, after a **scripted** advisor filed a suggestion
for the vent the new beacon covers, by name, and none for the hold
(decisions-log item 111, decision C2; one name since S1's operator lane rewrote
Hold & Build with a description, item 127 (12)). Produced by
`a_suggested_instantiation_reports_every_declared_parameter_in_order`.

It is a fixture as well as a golden: T19's second pull request copies it byte
for byte to `godot/fixtures/instantiate_suggested.json`, with a guard that keeps
the two identical (`the_wizard_fixture_is_the_gateways_golden`). The real
operator runs live in the client job's watch check; the copy feeds the wizard
shot, which `cargo xtask screenshot` compares with
`tests/golden/vista/expected.wizard.png`, the page pins in
`crates/client-gdext/tests/wizard.rs`, and the watch check's Hold & Build byte
pin. So a diff here is **also a diff in the editor's input**, and the pull
request that moves it re-copies the fixture, brings those pins up to date,
re-renders the PNG golden from its own CI run (`tests/golden/vista/README.md`)
when something the wizard's first page draws moved (its label, value, mark or
why; a comment in the template is none of them), and names which of four
things moved:

* **The `parameters` list** — its order is the template's declaration order,
  one entry per declared parameter, with `suggested: true` exactly where the
  value is the operator's. An entry missing, reordered or mis-flagged is the
  wizard showing the wrong page.
* **`value`** — compact JSON text, raw (a duration in game milliseconds; the
  wire carries no unit, because the editor may not convert one).
* **`why`** — the scripted advisor's own sentence, passed through untouched.
* **`playbook_jsonc`** — `library/hold_and_build.jsonc` itself, instantiated:
  `kind` is `PLAYBOOK`, `meta.parameters` is gone and every comment survives.
  **It moves whenever the template does**: T18b moved it, with every entry of
  `parameters` and the scripted `why`, when it reshaped Hold & Build to walk to
  the edge of the core's sphere and place a Build beacon there (decisions-log
  item 113 (6)); S1's `oper` moved it again, with `parameters`, when it
  rewrote the template with a `covering` description and walk-in, so the walk,
  the site and the anchor became one vent named by the suggestion.

The `_status` footer is the opening Lull's, with the whole first Lull, 600 000
ms (`match.first_lull_ms`), reported left. Its history: S1's first contract
pull request moved it from 180 000 to 300 000, with the `godot/fixtures/` copy;
since S1's `econ` the opening Lull is `match.first_lull_ms`, and the producer
pinned the report at 300 000 until S1's `build` lane dropped the pin
(decisions-log item 135 (2) (h)), when the `godot/fixtures/` copy was re-copied
in the same commit. Any other change in it means the footer's
shape did; a timed footer carries no `untimed` member, which is written only when
true.

### `demo_<playbook>_verify/` and `demo_<playbook>_render/`

AGENTS.md section 10 item 3 for the stage demo's own playbooks (decisions-log
item 123 (2) 10): each one verifies identically (the same `report_hash`) and
renders identically (the same prose). Five playbooks, two cases each, produced
by `crates/gateway/tests/demo_playbooks.rs`: the three templates in `library/`,
each instantiated for seat 0 with `template_id` alone (its own values, nothing
suggested, as `templates.rs` asks), and the two scenario playbooks under
`scenarios/skeleton/` as committed. The wizard itself always asks with
`suggested: true`, and in the lobby the live Easy advisor fills those values;
that answer has no report or prose golden, because the real operator is
reachable only from `crates/gamectl/tests` (`instantiate_suggested` pins the
page's shape with a scripted advisor). `demo_against_easy_*` is Hold & Build
as Easy fills it. The same test asserts each text's byte
round trip through plan-core (`Document::parse(text).to_text() == text`) and
that each FULL report qualifies; neither has a file here.

**The verification context is the game's**: seat 0's frozen snapshot and scope
in round 1's Lull of a match opened as `gamectl host` opens the lobby's
(`godot/scripts/host_link.gd`: seed `0x00000000ca5caded`, two seats, the rules
table's own segment ladder, three rounds), through the `Host::open_from`,
fogged `Surface::new`, `attach` and `open_lull` that `serve.rs` takes, with the
client's first clock report (the whole first Lull left, `match.first_lull_ms`).
No in-process seat is
registered, because neither the built-in operator nor the advisor steps
anything, so seat 0's frozen snapshot is the same without them. That is why
`segment_length_ms` in the footer is the ladder's first Push, not the one-second
segment of `instantiate_suggested`'s footer, and why these are not the
verifier's fixture scope (`crates/verifier/tests/verifier.rs`), which holds none of the voxels the
scenario playbooks name.

What moves a demo case, and what a diff means:

* **The playbook moved.** A template in `library/` or a scenario playbook was
  edited: its `plan_fingerprint`, `report_hash` and `size_units` move, and its
  prose moves with any change a player would read. Say which file and why; a
  comment-only edit to a template moves nothing here, because neither answer
  carries a comment.
* **The instantiation moved** (the three template cases only): the gateway's
  `instantiate_template`, or the template's own values.
* **The context moved** (item 109): the golden seed's map, the rules table (its
  `rules_hash` is in every report, so a tuning change moves every `_verify`
  case, and the footer's `phase_remaining_ms` is `match.first_lull_ms`, so a
  first-Lull re-tune moves every `_verify` and every `_render` case: S1's `econ`
  moved it from 300 000 to 600 000 when the gateway began reading round 1's
  Lull from that row), the segment ladder
  the footer names, or the frozen snapshot's shape. A
  widened snapshot moves every `report_hash` here with the verifier's own
  goldens, and the pull request re-blesses both and says why. So does a
  widened seat view (`pharmakos_verifier::Scope`'s encoding): S1's targeting
  verifier added the map's features and the commander's position to it, which
  moved every `report_hash` here while the gateway still filled neither, and
  S1's task `tgtw` moved them again when it filled them (decisions-log item
  131 (4) (d): each feature's id, kind, grade, anchor, live bit and the seat's
  own covering beacon, and the commander's voxel). That second move changed
  the view's content and not its encoding, and no diagnostic moved with it:
  only the `report_hash` line of each `_verify` case. S1's `build` lane moved
  that line again in all five, and nothing else: the snapshot's version went
  from 8 to 9 with each Build target's order and rotation and each
  structure's rotation, and the version is the snapshot's first field.
* **The verifier or the renderer moved.** A new diagnostic, a changed lint or a
  changed `verifier_version` moves the `_verify` cases; a template string in
  `render_plan` moves every `_render` case. A report that stops qualifying is
  not re-blessed: the test asserts `qualifies: true`, and a demo playbook the
  game refuses is a demo that cannot be played.

The answers carry no file's text but what the renderer quotes: a report names
codes, pointers and hashes, and the prose quotes the playbook's own `meta`
title and summary and its step labels. No licence header or comment reaches
them. The `_verify` cases and the two scenario cases take this area's
`GPL-3.0-or-later`; the three template `_render` cases quote `library/`'s
permissive titles, summaries and labels, so `REUSE.toml` records them as
`GPL-3.0-or-later AND (MIT OR Apache-2.0)` (decisions-log item 124 (5) (e),
taken by S1's plan's decision 9).

### `map_summary/` and `resolve_refs/`

S1's targeting surfaces (`docs/design/targeting.md`, "Surfaces"; S1's plan, task
`tgtw`), each one whole JSON-RPC answer, produced by
`crates/gateway/tests/targeting.rs` on the golden seed in the opening Lull of a
two-seat match with a 60-second segment. Every number in them is the sim's: the
features are the map generator's table, a feature's liveness is the sim's
footprint scan, its coverage is the seat's own living beacons' spheres, and every
travel time is the item-61 estimator's cost from the commander's column, ranked
by the sim's own `Ranker` and turned into game milliseconds the way
`estimate_route`'s legs are. The pick in `resolve_refs` is the sim's own
`cover` and `on_vent`, and `targeting.rs`'s
`resolve_refs_answers_as_the_sim_would_at_step_start` holds it to what the sim
then binds when the step starts.

What a diff means:

* **The map moved** (`tests/golden/mapgen/` moves with it): features appear,
  disappear or change grade, anchors move, and every travel figure with them.
* **The estimator or the step costs moved** (`tests/golden/pathing/`, or a
  rules row in `locomotion.*`): the `travel_ms` figures move, and when they
  reorder, so do the candidates and possibly the pick. A pick that changes is a
  change in which vent a carried "cover the nearest vent" covers, and the
  `cover-nearest-vent` scenario's chain moves with it.
* **The resolver moved** (`crates/sim/src/targeting.rs`): the pick, the
  `on` column it implies or `matched` move with nothing else. That is
  determinism code, and its pull request explains it.
* **The answer's shape moved**: a field added or renamed in `MapFeature` or
  `ResolvedRef` is a `gateway.proto` change (AGENTS.md §5).
* **What a non-seat is told** is not in these files (it is asserted in
  `targeting.rs`): `admin` and a spectator are told each feature's id, kind,
  grade, anchor and liveness, and none of `covered`, `travel_ms` and
  `reachable`, which are a seat's own.

The `_status` footer is the opening Lull's after the client frames the test
reported before the call (`map_summary`'s is its second call, 599 900 of the
first Lull's 600 000 ms left; `resolve_refs`' its first, 599 950), with the
test's 60-second segment; a first-Lull re-tune moves it. S1's `econ` moved both
by 300 000 ms when the tests began counting round 1 down from
`match.first_lull_ms` rather than `match.lull_ms`.

### `briefing/`, `economy_forecast/` and `recap/`

S1's economy surfaces (S1's plan, task `econ`; the register's X-03, X-16,
S1-11, S1-12 and S1-46), each one whole JSON-RPC answer to seat 0, produced by
`crates/gateway/tests/economy.rs` on the golden seed in a two-seat, three-round
match with a one-second segment: the briefing in the opening Lull, the forecast
after the seat has sealed the cover-and-build playbook, and the recap once
round 1's segment has ended (twenty ticks, which the Push spends walking, so
nothing is paid but the settlement). Every number in them is the sim's or the
verifier's:

* **`standing`** is the seat's own audit line (`pharmakos_sim::audit`) and its
  place among the audited seats on spec section 3's tie-break terms. At the
  opening Lull both seats hold the same, so they tie and both read rank 1,
  which `economy.rs`'s `the_displayed_rank_agrees_with_the_final_audit` holds
  to the audit's own winners. A diff here is the economy's values (the
  starting treasury, a structure's or unit's cost, value follows condition)
  or the ranking.
* **`committed_dollars`** is the verifier's projection over the seal
  (`pharmakos_verifier::projection::project`): the beacon and the Generator in
  its initial settings, at the rules table's prices. It moves with a price row,
  with the projection's rules, or with the scenario playbook. It is written in
  a Lull only.
* **`bmi_next_dollars`** is the sim's `economy::bmi_for` at the sim's
  `economy::ladder_place` over the living seats' held values as the world
  stands: the BMI at the band the seat holds now, a prediction and not a
  promise, because spending in the Push can move the ladder before the Ledger
  reads it (`economy.rs`'s
  `the_next_bmi_is_the_band_held_now_and_spending_in_the_push_can_move_it`).
  Seat 0 leads the opening tie, so it is the leader's BMI; this recap's
  `bmi_dollars` below happens to match it, because the twenty ticks spend
  nothing. It moves with `economy.bmi_dollars` or the scaling rows.
* **`settlement.bmi_dollars`** is the `settled` event's own value: the BMI
  after the band's adjustment. Seat 0 leads a tie on held value (the lower
  seat index wins a tie), so it draws the leader's malus. `award_dollars` is 0
  until S4. `band_rank` (from 1, the leader first) and `band_percent` are the
  sim's `economy::ladder_place` and `economy::band_percent` over the ladder as
  the Ledger read it (decisions-log item 134 (2) (c)): band 1 at the leader's
  malus. They move with the scaling rows or with the ladder's tie-break.
  There is no `shortfall` here: the shipped grid carries the starting force.
* **The prose** is `crates/gateway/src/strings.rs`'s; a wording change moves it
  and nothing else.

The forecast's and the briefing's `_status` footers are the opening Lull's
after the client frames the test reported before each call, counted from the
first Lull's 600 000 ms (`rules.match.first_lull_ms`); the recap's carries
`"untimed": true`, which a recap always does, since it has no countdown.

## Conventions

Tab-separated, one record per line, LF endings, a trailing newline, no `\r`
(`tests/golden/README.md` rule 3 — enforced, because `.gitattributes` marks this
tree `-text` so git will not normalise it for you). Comment lines start with `#`
and are part of the file: they are what a person reading a red build sees first.

# Skeleton plan: wave 6 notes (item 111, 2026-09-23)

Produced by the wave-6 design step against `main` = `87375b1` (wave 5 closed, item 110), read-only: one designer, two
adversarial critics (the rules; buildability, cost and scope) and this revision. Every critique item was checked against
the code or the log before it was taken; the ones not taken, or taken in another form, are in section F. Where this file and
`skeleton-plan.md` section 3's T17, T18 and T19 lines disagree, this file wins once the item adopting it is logged.
Precedence followed: decisions-log section 2.7 > spec v0.6 > co-design doc. Nothing here is a rule the spec does not
state; every silence is a PLACEHOLDER with an owner and a stage (section D).

Adopted by decisions-log item 111 without change. Section A1 is also `skeleton-plan.md` section 3's `### T18a`
section, and the T17, T18 and T19 sections there carry an amendment line pointing here.

## 0. The holes, verified against main

Each hole the main session named, checked in the code. Line numbers are `main` at `87375b1`.

- **H1 holds, and is wider than stated.** `gp.api.v1.InstantiateTemplateResponse` is one field, `playbook_jsonc`
  (`proto/gp/api/v1/gateway.proto:770-773`); `Parameter` is a PLACEHOLDER STUB `{name, value}` whose comment already
  assumes "the template declares what it accepts" (`:761-768`), yet `gp.v1` has no parameter message anywhere and
  `pharmakos_plan_core::library::instantiate_template` reads `name` as an RFC 6901 pointer (`crates/plan-core/src/library.rs:203-240`).
  `RenderPlanResponse` is one `prose` string (`gateway.proto:888-892`); `render_plan` returns `String`
  (`crates/plan-core/src/render.rs:75`). No path carries an operator suggestion or a "why". **Wider:** `gamectl host`
  passes `library: None` (`crates/gamectl/src/host.rs`, `Setup { .., library: None, .. }`), so `list_templates` answers
  an empty list in every hosted match today (`crates/gateway/src/surface/planning.rs:121-128`), and no template file
  exists in the tree (H7).
- **H2 holds.** `BuiltInSeat::plan(&mut self, seat: u8, call: &mut dyn FnMut(&Request) -> Json)`
  (`crates/gateway/src/serve.rs:129-146`) takes `pharmakos_gateway::rpc::Request` (`crates/gateway/src/rpc.rs:42-52`).
  **No "gp.api.v1 service traits" exist:** `gateway.proto:40-42` says "There is still no `service` block", and
  `crates/proto/src` defines no trait. The only `Operator` trait is `pharmakos_sim::seams::Operator`
  (`crates/sim/src/seams.rs:311`), a per-tick decide seam over `LocalView`/`IntentSink` inside the sim; it is the
  execution arm's seam, not the planning client, and T18 must not implement it. `crates/operator` has no dependencies
  and a placeholder `lib.rs`. `gamectl host` passes `built_in: Vec::new()`.
- **H3 holds, and the constant is per host, not per seat.** `SAFE_PLAYBOOK` (`crates/gateway/src/host.rs:66-107`,
  PLACEHOLDER naming T18) is held once per `Host` (`host.rs:157`, `safe_playbook()` / `set_safe_playbook()`
  `:282-290`), filed for every stale seat by `Surface::file_safe_playbooks` (`crates/gateway/src/surface.rs:1228-1265`)
  from `begin_push` (`surface.rs:706-729`, not ~1219-1280, which is the filing helper), and returned verbatim by
  `get_safe_plan` (`surface/planning.rs:394-404`). The spec's safe playbook depends on the seat's own power and
  beacons (spec section 14), so a per-host string cannot carry it even with `set_safe_playbook`.
- **H4 holds, with one factual correction.** Save trigger undecided (t16a notes section B "T17", section D); `cache.rs`
  is the one `std::fs` module (`crates/gateway/src/cache.rs:52-58`); `Surface::snapshot_bytes` exists
  (`surface.rs:886`); no restore path exists in the gateway (`plans_for_round`'s PLACEHOLDER, `surface.rs:740-763`);
  the sim side exists (`Runner::restore`, `crates/sim/src/runner.rs:956`; `Snapshot::restore_into`,
  `snapshot.rs:780`; `Interpreter::restore` with the fingerprint PLACEHOLDER, `interpreter/state.rs:466-474`;
  `SNAPSHOT_VERSION = 6`, `snapshot.rs:117`). **Correction: sealed playbooks are NOT written to disk.** The layout
  reserves `seats/<seat>/sealed/`, `drafts/`, `notebook.txt` and `replay/` (`cache.rs:36-50`), but
  `MatchCache::seat_directory` has no caller outside `cache.rs`; the only files a hosted match writes are
  `README.txt`, `match.json` and `audit.log`. Notebooks, drafts and seals live in `Surface` memory only, so a save
  must carry them itself. **Decision 13 is answered:** item 84, "build it (T17, 4 agent-days)", save at Lull boundaries
  into the private match cache. Item 84 does **not** mention the private replay; it rides T17's plan line only.
  The crate map already says the gateway does "saves" (AGENTS.md section 3, gateway row), so "T17 owns crates/sim
  only" contradicts the crate map too.
- **H5 holds.** `SphereVision` is the labelled stopgap (`host.rs:446-520`); the sim's private `within`
  (`crates/sim/src/interpreter/cond.rs:395`); `serve::Config::parse` checks `human_seat < seats` but sets no upper
  bound on `seats` (`serve.rs:220-223`, `:251-256`); `Host::check_settings` checks none either (`host.rs:220-241`).
  **A conflict to put to the owner, not to settle here:** T14 gave the sim a second live sight term, a living
  scout's own vision (`crates/sim/src/survey.rs:1-45`, `programs::UNIT_VISION_RADIUS_VOXELS`), and item 107(6)
  says Survey-lite's sightings "join" the host's `Vision`. But the owner's own later answer, item 108(1), says enemy
  units, beacons, structures and edits "appear only inside own beacon spheres until the match-end unlock", and
  `host.rs:474`'s PLACEHOLDER puts sightings at S3. The log's latest owner word wins: the follow-up is
  `World::in_own_sphere` as items 107(6) and 110(5) name it, and scouts or sightings in the live view are an owner
  question (section D). Recorded sightings are memories with an age (`knowledge.rs:218-237`), S3's last-known
  knowledge, not live vision.
- **H6 holds** as stated.
- **H7 holds.** No template file exists; `examples/playbooks/expand_east.jsonc` is `kind: PLAYBOOK`. Note: item 81 is the
  template decision (Hold & Build, Expand & Mine, Safe Playbook); item 80 is the JSON-RPC casing. Item 93 cites
  "item 80" for the templates, a slip worth fixing when the log is next touched.

**Four holes the list does not name, found while checking:**

- **H8 — a built-in seat cannot finish a round under the rate limit.** `CALLS_PER_TICK = 8` per token
  (`crates/gateway/src/limit.rs:53`). `serve_surface` runs `plan_built_in` synchronously on the surface thread
  right after the job that opened the Lull (`serve.rs:664-768`), and in a Lull the gateway's tick moves only on a
  `report_host_clock` job. Every call a built-in seat makes lands on one tick, so its ninth call is `RATE_LIMITED`.
  Easy needs well over eight (status, briefing, beacons, view, economy, templates, several estimates, instantiate,
  up to five verifies, submit, `set_ready`). The T16a test `a_built_in_seat_calls_through_the_same_door_as_a_socket`
  uses a scripted fake that makes fewer.
- **H9 — the operator's inputs are not on the wire.** `BeaconSummary` is `{beacon_id, at}` with 3-15 reserved
  (`gateway.proto:601-610`): no owner, core flag, Quartermaster priority or power state. `GetEconomyForecastResponse`
  reserves 1-15 (`:677-681`), and the handler answers `{}` (`crates/gateway/src/surface/knowledge.rs:356-389`).
  "If power is short and up to 2 at-risk beacons ..." (spec section 14) cannot be computed by an ordinary client.
  Item 105(2) anticipated this: the forecast fields land "with the lane that fills them (S1's economy work, or T19
  if the wizard needs them first)". T18 needs them first. The sim has all the readers: `treasuries()`,
  `supplies()`, `draws()` (`crates/sim/src/tables.rs:545-600`), `dormant()` and `priorities()` (`tables.rs:2201-2229`).
- **H10 — the wave's own milestone cannot be seen.** Row W6 promises "the treasury and kW meter moving", and plan
  1.1 promises "kW supply rises". No method a client can call returns `$` or `kW` (H9). The recap's settlement
  lines are a reserved stub too (`GetRecapResponse`, `gateway.proto:578-584`). BMI settlement reaches the event list
  only as T14's `settled` event line.
- **H11 — `Setup.built_in` cannot be sized before the config line is read.** `serve::run` reads the config line
  after `Setup` is built, and `mint_tokens` refuses more built-in seats than there are non-human seats
  (`serve.rs:593-599`). So `gamectl host` cannot know how many operators to pass.

**Five more, found by the critics and confirmed in the code:**

- **H12 — the operator's tuning inputs are not on the wire either.** No method carries a cost, a `kW` rating or a
  yield: `get_capabilities` has no pair until S4 (`gateway.proto:192-195`) and the vents stub is S1's. A proto-only
  operator scoring "economy" would have to hard-code tuning values, which AGENTS.md section 12 forbids.
- **H13 — the constant is not the spec's safe playbook.** `SAFE_PLAYBOOK` holds for 1 s and then falls back to hold
  at the safest beacon (`host.rs:85-107`): no "move to the safest beacon", no shadow, no flee rule (spec section 14).
  `scenarios/skeleton/deploy-and-visit.scenario.jsonc` files it for seat 1, so making it the spec's moves that
  scenario's chain.
- **H14 — a proto field breaks another lane's crate.** `crates/client-gdext/tests/fixtures.rs:88,96` builds
  `BeaconSummary` as exhaustive struct literals; any field added to it fails to compile there.
- **H15 — the advisor can disturb the human's view feed.** View state is per viewer, not per token
  (`viewfeed.rs:122-135`): handles are minted in the order the viewer first sees each thing. An advisor calling
  `get_view` on the human's seat therefore writes into the human's viewer state.
- **H16 — the cache cannot resume today.** `serve::run` opens the surface before the cache (`serve.rs:442` vs
  `:465`); `MatchCache::open` rewrites `match.json` (`cache.rs:223-253`); and the lobby's match id is
  `local-<pid>` (`godot/scripts/lobby.gd:49`), so a new match can reuse an old id and overwrite its save.

## A. Amendments to the T17, T18 and T19 sections

Wave 6 becomes **five lanes in two runs** (section B). One new task, **T18a — the planning wire**, is added, as T16a
was. It carries everything T18 and T19 presuppose and `main` lacks. T17, T18 and T19 are re-cut so each is buildable
on `main` plus what the run before it merges.

### A1. New: T18a — `proto/**` + `crates/plan-core` + `crates/gateway` + `library/`: the planning wire (run 1)

- **Crate(s) owned:** `proto/gp/v1/playbook.proto` (one message, one field), `proto/gp/api/v1/gateway.proto`,
  `proto/buf.yaml`'s `ignore_only` block (discharged numbers), the regenerated tree and descriptor in `crates/proto`,
  `crates/plan-core`, `crates/gateway`, `scenarios/skeleton/deploy-and-visit.scenario.jsonc`'s chain (re-blessed, H13),
  the new top-level `library/` with its `LICENSE` pointer and `REUSE.toml` annotation, and one named place,
  `crates/gamectl/src/host.rs` (the library path only). **No edit in `crates/sim`, `crates/verifier`,
  `crates/operator`, `crates/client-gdext` or `godot/`.** H14's two literals are fixed on `main` before the run
  (section B), so this lane never reaches into the Godot unit's crate.
- **Builds:**
  1. **Template parameters, declared in the template** (decision C1): `gp.v1.TemplateParameter { string pointer = 1;
     string label = 2; reserved 3 to 15; }` (3-15 for S6's typed catalogue) and `Meta.parameters = 6` (`Meta` uses
     1-4 and holds 5 for `team_id`; 6 is named here so nobody "takes the next free" over a reservation).
     The field is meaningful only when `kind = TEMPLATE`. `instantiate_template` removes it as it sets `kind:
     PLAYBOOK`, so no playbook's fingerprint or golden moves. On a hand-written PLAYBOOK it is round-tripped and never
     read, as `meta.note` already is ("Round-tripped, never interpreted", `playbook.proto`); nothing is stripped.
  2. **The suggestion on the wire** (decision C2): `InstantiateTemplateRequest.suggested` (bool, 3);
     `InstantiateTemplateResponse.parameters` (repeated `FilledParameter { pointer = 1; label = 2; value = 3;
     bool suggested = 4; }`, 2) and `why` (string, 3). The declared list is always returned, in declaration order,
     with the value that was applied: explicit, then suggested when `suggested`, else the template's own. `value` is
     the raw JSON text at the pointer (a duration is game milliseconds); the wire carries no unit display.
  3. **The advisor seam** (decision C5): `serve::Advisor`, run at every `open_lull` for **each seat that has no
     built-in operator** (the human's), through a per-seat in-process token (`observe`, `docs`, `plan`; never
     `plan.submit`, never leaving the process). The in-process door enforces a **method allow-list** host-side in
     `serve.rs`: `save_notes`, `save_draft`, `list_drafts`, `submit_plan` and `set_ready` answer `FORBIDDEN` to an
     advisor, audited. It returns plain gateway types, `Advice { safe_playbook_jsonc, suggestions: [{template_id,
     parameters, why}] }`, which `serve.rs` files with a host-side `Surface::file_advice(seat, advice)`. No handler may
     reach `file_advice` (`tests/confinement.rs` needle). `begin_push` files **that seat's** safe playbook. It is
     verified FULL and compiled at filing; anything that fails falls back to `SAFE_PLAYBOOK` with an audit line, never
     silently. `get_safe_plan` returns the seat's own. A built-in seat needs no advisor: its operator submits its own
     plan and, when its repair loop fails, submits its own safe playbook through `submit_plan` on its own token.
     `SAFE_PLAYBOOK` stays as the fallback for a host with no advisor (scenario runs, tests).
  4. **`Setup` built-ins become a factory** (H11): the host builds one `BuiltInSeat` per built-in seat and one
     `Advisor` per other seat after reading the config line, each a fresh instance with no shared state.
  5. **In-process token limits** (decision C6, H8), **subject to the owner-now question in section D**: tokens minted
     for built-in seats and advisors get their own `Limits`, `IN_PROCESS_LIMITS` (PLACEHOLDER), sized to the
     operator's declared budget. They are still counted, audited and fog-filtered. A per-token `Limits` replaces the
     per-surface one for those handles only. `BuiltInSeat`'s doc (`serve.rs:131-134`, "rate-limited ... exactly like
     a socket") is rewritten to say what is now true: same door, same audit, same fog, its own rate.
  6. **The operator's inputs and the meter** (decision C7, H9/H10): `GetEconomyForecastResponse` fields 1-4, named as
     **present-state** values (`treasury_now`, `supply_kw_now`, `draw_kw_now`, `headroom_kw_now`), so S1's projection,
     income and what-ifs land in new fields and never redefine these. They are the seat's **own** economy as the world
     stands, in every phase: the frozen snapshot in a Lull or a recap, the live world in a Push. `BeaconSummary`
     gains `owner` (spelt `seat.0`, as `ViewEntity`), and for the seat's **own** beacons only `core`, `priority` and
     `powered`. Another seat's beacon carries `owner` and nothing more.
  7. **The three templates** (decision C13) as `library/hold_and_build.jsonc`, `library/expand_and_mine.jsonc` and
     `library/safe_playbook.jsonc`, `kind: TEMPLATE`, each declaring its parameters, "why" comments in the file. The
     safe template is the spec's (decision C16): move to the safest beacon, the priority raise as parameters (empty by
     default), then shadow the safest beacon, with a flee handler. **`SAFE_PLAYBOOK` becomes that template's
     nothing-raised instantiation**, and `deploy-and-visit`'s chain is re-blessed with that reason stated.
     `gamectl host` passes `root.join(LIBRARY_PATH)`.
  8. `RenderPlanResponse.prose`'s comment gains its line layout: one rule, step or block per line, in file order.
     The existing `render_plan` prose goldens pin it, so the wizard's rule list reads lines, never parses sentences.
  9. Nothing else: no chips, no structured render (decision C3).
- **Implements:** spec sections 12 (Planning group, `get_economy_forecast`, `get_safe_plan`), 13 (templates, wizard
  pre-fill), 14 (the safe playbook is the operator's and per seat); item 81; item 105(2), departed from in the open
  (decision C7); AGENTS.md section 3 rules 3 and 4, section 7 (limits and audit are part of the feature).
- **Needs:** nothing unmerged, once section B's two-line pre-run commit is on `main`.
- **Acceptance:** `each_template_instantiates_and_qualifies_for_both_seats_of_the_golden_seed`;
  `the_gateways_fallback_is_the_safe_template_with_nothing_raised` (canonical-equal, comments aside: "written once,
  tested twice", item 81); `a_suggested_instantiation_reports_every_declared_parameter_in_order`;
  `an_explicit_parameter_beats_a_suggestion`; `advice_for_one_seat_never_reaches_another` (seat 1 calls
  `get_safe_plan` and `instantiate_template{suggested}` and sees only its own);
  `a_seats_advice_is_the_same_whatever_its_own_notebook_and_drafts_hold` (the operator ignores the notebook, spec
  section 14); `the_advice_for_a_seat_is_a_function_of_that_seats_own_calls` (byte-identical whether or not the other
  seat wrote drafts and a notebook); `a_built_in_seats_sealed_plan_is_the_same_with_and_without_an_advisor_running`;
  `an_advisor_is_refused_every_method_off_its_allow_list` (each of the five, audited);
  `the_advisor_leaves_the_humans_view_feed_as_it_found_it` (H15: the human's next `get_view` deltas and handles are
  byte-identical with and without an advisor run; if they cannot be, the advisor path makes no `get_view` call and
  the lane says what the operator loses);
  `an_advised_safe_playbook_that_does_not_qualify_is_replaced_by_the_fallback_and_audited`;
  `no_handler_can_file_advice`; `an_in_process_seat_finishes_a_round_under_its_limits_and_a_socket_does_not_get_them`;
  `a_seat_is_told_its_own_economy_and_never_another_seats`; `another_seats_beacon_carries_its_owner_and_nothing_else`,
  under both fog policies; `the_host_builds_one_operator_per_built_in_seat_and_one_advisor_per_other_seat`;
  `a_token_appears_on_the_announce_line_and_nowhere_else_the_host_writes`, extended: an in-process token appears on
  no announce line and in no file the host writes. Goldens: an `instantiate_suggested/expected.response.json`
  produced with a scripted advisor, **the fixture T19's run-2 PR copies**. Re-blessed with the reason stated: the
  descriptor, `tests/golden/proto/expected.reserved.txt`, `tests/golden/schema/get_schema/expected.json` (it serves
  `gp.v1`), `tests/golden/docs/reference/**`, `tests/golden/gateway/walkthrough/expected.walkthrough.txt`
  (`get_beacon` gains fields, `get_safe_plan` changes text), and `deploy-and-visit`'s chain (the fallback safe
  playbook became the spec's; the chain moves from the first tick seat 1's commander acts differently).
  `buf breaking` green in `WIRE_JSON`, with an `ignore_only` block for `RESERVED_MESSAGE_NO_DELETE` on
  `gp/api/v1/gateway.proto` (buf scopes by rule and file, not by field number; the harness-docs PR deletes it,
  section B). No determinism or `report_hash` golden moves; if one does, the lane stops and says why.
- **Contract PR:** **yes.** `proto/**` and `buf.yaml` are the first commit, alone in it; the lane also touches
  `REUSE.toml`, adds `library/LICENSE`, and moves a scenario chain. The agent opens the PR and stops; the main
  session merges under items 85-86 once CI is green and both reviews are applied.
- **Agent-days:** 8.75: proto, goldens and ignore block 1.25; plan-core instantiate and declarations 0.75; advisor
  seam, allow-list, per-seat store, filing, limits and factory 2.5; economy and beacon fields 1.0; templates, the
  constant and the scenario re-bless 1.25; remaining tests and fixtures 1.25; review cycle 0.5; `gamectl` place 0.25.
  **One PR by default** (item 107(9)). **Split seam if it runs long:** PR 1 = 1, 7, 8, 9 and the plain instantiate
  handler's parameter list (4.25 d); PR 2 = 2 (`suggested` and `why`, so nothing ships with nothing behind it) and
  3-6 (4.5 d).
- **PLACEHOLDERs:** `IN_PROCESS_LIMITS` (owner, now, then the numbers at hardening); the typed parameter catalogue
  and a parameter's `type` (owner, S6; numbers 3-15 held); whether the verifier should refuse `meta.parameters` on a
  PLAYBOOK with a new code rather than round-trip it (owner, S6); forecast what-ifs, projection and income (S1);
  enemy beacon detail (S2); the live Push readout of own `$`/`kW` in a method named "forecast" (owner, S1);
  `LIBRARY_PATH` beside a shipped binary (T21).

### A2. T17 — `crates/sim` + `crates/gateway`: save, resume, the private replay, and the vision follow-up (run 2)

Changes to "### T17" (`skeleton-plan.md` section 3):

1. **Crate owned:** `crates/sim` **and** `crates/gateway`. The crate map puts saves in the gateway, and the sim is
   otherwise unowned in run 2. `crates/gamectl` is untouched: resume rides the config line (decision C9). The
   `godot/` Resume entry and the lobby's match id are T19's run-2 work. `library/` is frozen in run 2 (section B).
2. **Builds** (replaces the line):
   - **Save**, written by `serve.rs` through `cache.rs` alone at the two Lull boundaries that item 84's "save at Lull
     boundaries" names (decision C8): **at `begin_push`, after every seal is final and the safe playbooks are filed**,
     before the first tick; and **when the control pipe reaches end of file during a Lull**. One file per match,
     `save.json`, overwritten atomically (write, then rename). The surface builds the save into a pending slot and
     `serve.rs` flushes it, the pattern the audit log already uses. No save is written mid-Push.
   - **Save contents** (decision C10): the stamp (save format version, `rules_hash`, verifier version,
     `SNAPSHOT_VERSION`); the config line's values; `lull_offset` (so the gateway's tick, and the audit log's stamps,
     stay monotonic across a resume); the frozen snapshot's bytes; the kind of boundary (`sealed` or `lull`); and per
     seat the notebook, the drafts, and the seal (canonical JSONC, round, `filed_by_the_gateway`, `report_hash`, plan
     fingerprint). `SNAPSHOT_VERSION` does **not** move: the fingerprint lives in the save beside the snapshot, never
     inside it, so no `report_hash` golden moves (item 109(7)).
   - **Resume:** a config line whose seventh field is `resume`. Fields 1-5 must equal the save's values, and a line
     that differs is refused as `INVALID_ARGUMENT` (the host's own rule: nothing silently defaulted). `serve::run`
     reads the save through `cache.rs` **before** it opens the surface (H16), through a new `MatchCache::reopen`
     that does not rewrite `match.json` and appends a `resumed` line to `audit.log`. A new `Host::resume` sits in
     `host.rs`, the one module that drives a runner: it regenerates the pristine world from the saved config,
     `Runner::restore`s the snapshot, and re-seals every saved seal from the saved text. That closes
     `plans_for_round`'s PLACEHOLDER. The plan is recompiled and its fingerprint compared with the saved one; said
     plainly, both come from `save.json`, so the check catches a damaged file and a canonical form that changed
     between builds, and nothing more (the cache is plain local state, AGENTS.md section 7). The rules hash, verifier
     version and format version are refused on a mismatch as `INVALID_ARGUMENT`, exit code 2, with the message on
     stderr for the lobby.
   - **Where a resume lands** (decision C8, owner question in section D): a `sealed` save resumes **straight into
     that Push** with those seals and no Lull, so a killed client replays the same Push to the same chain and cannot
     re-plan what it watched; a `lull` save resumes into an ordinary Lull with the notebooks, drafts and any verified
     submission, which the spec lets a seat replace until the timer ends.
   - **What `Surface` rebuilds** (every field of `surface.rs:258-308` classified): *saved* — the seats' notebooks,
     drafts and seals, `lull_offset`; *derived* — `time` (round and phase from the runner), `host`, `feed_anchor`, the
     fog policy's eliminations (from the restored `SeatTable::is_alive`), `views` (the generated-map cache from the
     pristine world, `view_seq` restarting, every cursor and handle from the dead process stale); *reset* — `tokens`
     (new process, new announce line, as spec section 3 requires), `limits` and `limiters`, `lull_remaining_ms`,
     `reported_elapsed_ms` and `phase_elapsed` (the client re-reports), the built-ins' `planned` round (they re-plan;
     the operator is deterministic, so the same plan). `feed` (the `SegmentFeed`) is regenerated by a replayed Push
     and empty in a resumed Lull: the last recap's event list is not in the save (PLACEHOLDER, section D). The advisor
     re-runs at a resumed Lull.
   - **A new match never overwrites a save:** `MatchCache::open` refuses a match id whose folder holds `save.json`
     unless the line says `resume` (H16).
   - **The private replay** (decision C11), inputs only and no new sim format. At every `begin_push`, each seat's
     seal goes to `seats/<seat>/sealed/<round>.jsonc`, the layout T13 reserved. At every segment end, the segment's
     per-tick chain goes to `replay/<round>.hashes.txt`, in the same text format as the golden hash files, which makes
     that format a contract this PR names (AGENTS.md section 5, golden-file formats). The seed and settings are
     already in `match.json`, which gains the rules hash. Spec section 15's third input, the **log**, is not written
     (PLACEHOLDER, section D). Nothing reads the replay in v1 except the test below.
   - **H5** (decision C12): a public `World::in_own_sphere(seat, voxel)` in the sim, as items 107(6) and 110(5) name
     it, reusing `within` and moving no hashed state; `SphereVision` becomes a call to it and loses its stopgap label.
     Scouts' live vision and recorded sightings stay out of `Vision` pending the owner (section D; item 108(1)).
     `Config::parse` and `Host::open_from` refuse `seats` outside `1..=3` as `INVALID_ARGUMENT` (item 110(5)).
3. **Needs:** T18a merged.
4. **Acceptance:** `a_save_at_every_lull_boundary_of_a_three_round_match_resumes_to_the_same_chain`, in process
   with two seats: resume at each boundary, play on with the same submissions, and compare the chain with the
   uninterrupted run tick for tick. `a_resumed_lull_has_the_seats_notebooks_drafts_and_seals`.
   `a_sealed_save_resumes_into_its_push_and_offers_no_lull`. `a_save_from_a_different_rules_table_or_verifier_is_refused`
   (both asserted). `a_damaged_seal_in_a_save_is_refused` (the fingerprint check, named for what it catches).
   `a_resume_line_that_disagrees_with_the_save_is_refused`. `a_new_match_cannot_overwrite_a_saved_one`.
   `closing_the_pipe_in_a_lull_saves_and_closing_it_in_a_push_does_not`.
   `a_killed_client_mid_push_replays_that_push_to_the_same_chain`. `the_replay_files_reproduce_the_matchs_chain`:
   re-host from `match.json` and the sealed files, and compare with `replay/*.hashes.txt`.
   `no_handler_reads_the_save_or_the_replay` (confinement needles on the `cache.rs` readers).
   `a_token_appears_on_the_announce_line_and_nowhere_else_the_host_writes`, extended to `save.json` and `sealed/`.
   `a_fourth_seat_is_refused`. `the_view_uses_the_sims_own_sphere_rule` (the boundary voxel pinned through the sim's
   `within`). The determinism chain, the verifier goldens and the scenario goldens do **not** move; any fog or view
   golden that does is re-blessed with the reason.
5. **Contract PR:** **yes.** The save and replay formats, the hash-file format reused on disk (AGENTS.md section 5),
   and the new sim query next to determinism code. The agent opens the PR and stops.
6. **Agent-days: 6** (was 4). Sim query 0.25; vision swap and seat bound 0.5; save container and two triggers 1.25;
   resume, its two landings and the surface rebuild 1.75; cache reopen and the overwrite refusal 0.25; replay inputs
   and test 0.75; review 0.5; slack 0.75. The 4 d estimate priced only the sim half, and the save container, the
   restore path and the feed rebuild were never in it.
7. **PLACEHOLDERs:** one save per match, latest wins (owner, S6 with a save browser); where a resume lands (owner,
   now); a resumed Lull's timer starts full (owner, hardening); the replay's log, reader, scrub and recap use (S7,
   with the recordings); the last recap's events after a resume (S7); resuming a save from an older build (refused;
   owner, hardening).

### A3. T18 — `crates/operator` + `crates/gamectl/src/host.rs`: Easy, the safe playbook, the suggestions (run 2)

Changes to "### T18":

1. **Crate owned:** `crates/operator`; the named place `crates/gamectl/src/host.rs` (the adapters and the factory) and
   `crates/gamectl/Cargo.toml`'s one new edge. **Not `library/`:** the templates are frozen after T18a, because the
   gateway's tests and T18a's golden read them and the gateway is T17's in run 2. A template change T18 finds it
   needs is reported and sequenced after the run (section B).
2. **Builds** (additions to the line, which stands):
   - **An ordinary client by construction** (decision C4). `pharmakos-operator` depends on `pharmakos-proto` alone.
     A dev-dependency on `pharmakos-gateway` is allowed for tests only. It exposes `Easy` over a client closure
     `FnMut(&str, Json) -> Json` (method, params) and never names a gateway or sim type. `gamectl host` wraps it
     twice, as a `BuiltInSeat` (plan, submit, `set_ready`) for each built-in seat and as an `Advisor` (the safe
     playbook plus one suggestion per template) for each other seat. One instance per seat, no shared or static state.
   - **Inputs over the wire:** `get_status`; `get_briefing` (its notebook is never read, spec section 14);
     `list_beacons` (owner, core, priority, powered, from T18a); `get_economy_forecast`; `get_map_summary`;
     `get_view` (the commander's position, plus vent voxels decoded with `pharmakos_proto::chunk_rle` from the whole
     generated map item 108(1) serves every seat), subject to T18a's view-feed test on the advisor path;
     `estimate_route`; `list_templates`; `instantiate_template`; `verify_plan`; `patch_plan`; and, for a built-in
     seat only, `submit_plan` and `set_ready`.
   - **The one input not on the wire (decision C17, H12):** `gamectl` hands the factory the same public rules text it
     hands the host (`Setup.rules_json`, the committed `rules/rules.v1.json` that the client also pins), and the
     operator reads the named rows it scores with (costs, `kW` ratings, yields) through `pharmakos_proto::json`. No
     tuning value is a constant in the operator.
   - **Two determinism rules the wire makes necessary.** It never reads `_status.phase_remaining_ms` or anything
     else the host clock moves. It never keys a decision on an opaque entity id other than `b_NN`, because
     handles are minted per viewer in order of first sighting (item 107(5)).
   - **No random draw at Easy** (decision C15). Candidates are enumerated in a total order and the top 3 are taken by
     utility with ties to the lowest id. The seed (match, seat, round) is read, recorded in the "why", and unused
     until S5.
   - **The budget is in evaluation units** (spec section 14; decision C6): Easy's row is 30 candidates, one
     evaluation each, and 1 + 4 verifies. `EASY_CALL_BUDGET` is **derived** from those units (the fixed reads, one
     `estimate_route` per candidate, one `instantiate_template` per template, the verifies, submit and ready) as a
     `const` expression, not a number sized to fit, and the test below asserts a round never exceeds it.
   - **The safe playbook** is the `safe_playbook` template instantiated with the operator's parameters (item 81,
     "written once, tested twice"). Power is short when `headroom_kw_now` < 0. At-risk beacons are the own non-core
     beacons with `powered = false`, within 60 s by `estimate_route` from the commander, nearest first, at most two;
     each is raised to HIGH (both definitions PLACEHOLDER, section D). It never recycles, switches mandate or places.
     The flee handler is the template's; rescue is not (section D). A built-in seat whose own plan fails its four
     repairs submits this safe playbook itself.
   - Nothing of spec section 14's Survey post, Defend guard, chatter or target spread (section A6).
3. **Needs:** T18a merged; the harness-docs PR merged (the `gamectl -> operator` edge, the operator row).
4. **Acceptance** (the line stands, plus): `the_operator_is_deterministic_for_a_given_match_seat_round` as a golden
   playbook per seed, compared by the three-OS matrix; `the_operator_does_not_depend_on_the_host_clock`, with two runs
   whose clock reports differ; `the_operator_does_not_key_on_viewer_scoped_handles`: a scripted client permutes every
   `u_`/`s_` handle in the `get_view` answer, and the playbook is byte-identical; `the_safe_playbook_always_qualifies`
   on every fixture snapshot, including a power-short one built through a rules-text change;
   `the_safe_playbook_raises_at_most_two_beacons_nearest_first`, driven through a **scripted client** that answers
   `list_beacons`, `get_economy_forecast` and `estimate_route` with three unpowered non-core beacons, so no fixture
   match and no host test seam is needed (the operator's only input is the wire, which is what makes that honest);
   `a_two_seat_hosted_match_against_easy_reaches_the_recap_with_both_seats_sealed_and_ready` through `serve::run`;
   `easy_never_exceeds_its_derived_call_budget`; `the_operator_reads_no_notebook_and_writes_nothing_but_its_own_seal`;
   `the_operator_crate_depends_on_proto_alone`, a cargo-metadata test alongside the source-text one. **A baseline,
   not a gate:** the call count per round is asserted; the wall time the surface thread spends in `plan_built_in` and
   the advisors (it blocks every socket, `report_host_clock` included, `serve.rs:664-768`) is measured once outside
   the deterministic crates and reported in the PR body beside P2's 200 ms, which stays S5's.
5. **Contract PR:** no. The edge rides the main session's harness-docs PR (section B).
6. **Agent-days:** 8, unchanged. The suggestions for the human seat (+0.5) and reading the rules text (+0.25) are paid
   for by the templates and the wire moving to T18a (-0.5) and the scripted-client fixture replacing a built match
   (-0.25).
7. **PLACEHOLDERs** (add): "power short" and "at-risk" (owner, S1 with the grid); the rescue rule (S2/S5); Easy's use
   of its seed (S5); the Balanced weights (stands); `IN_PROCESS_LIMITS` against `EASY_CALL_BUDGET` (owner, now and
   hardening).

### A4. T19 — `godot/` + `crates/client-gdext`: the editor, in two PRs across both runs

Changes to "### T19". The split seam is **reversed and scheduled**: PR 1 needs only today's wire, PR 2 needs T18a's.

1. **PR 1 (run 1), 5.5 d.** The map route surface:
   - Clicking an own beacon offers Visit & change, Go here or Recycle; clicking ground offers Go here or Place
     beacon; Alt-click turns a fixed target into a selector.
   - Routes draw as polylines from `estimate_route` legs, with a travel time per leg. **No dashed legs** at the
     skeleton, because the estimator prices under `Fog::Clear` and the view agrees (t16a notes, B "T19"(2)).
   - The legality ghost is **per click**, not live on hover. It shows the QUICK verdict of the patched draft
     (decision C3's budget note).
   - Validation rows: QUICK on every edit, FULL after 600 ms idle, icons plus plain sentences, and Fix through
     `patch_plan` with the verifier's own patch.
   - Load rejects an out-of-vocabulary construct and shows the code and pointer. Also: the notes box
     (`save_notes`), draft continuity (`list_drafts`), and submit plus `set_ready`.
   - Acceptance: a byte-identical open/save round trip; `the_editor_makes_no_time_arithmetic_of_its_own`;
     `an_editing_burst_is_not_rate_limited_under_the_default_limits`, scheduled through `rig.rs` beside the vista's
     polls; and a **render-only** scene for the diagnostic rows (`scenes/rows_shot.tscn`), rendered headless by the
     lane and looked at, **not compared**: `cargo xtask screenshot` compares the vista alone (`VISTA_GOLDEN`,
     `xtask/src/main.rs:1407`), a first golden needs `ci.yml`'s bootstrap step, and item 110(4) books the render-only
     mode to T20, which then commits and compares both shots.
2. **PR 2 (run 2), 6.5 d.** The wizard over the three templates:
   - Pages are built from `instantiate_template{suggested:true}`'s `parameters`, pre-filled, with the "why".
     Editing a value re-instantiates with explicit parameters. **A value is shown and typed as its raw JSON under the
     template's label** (a duration in game milliseconds): converting "3 s" to 3000 in GDScript would be time maths
     (AGENTS.md section 3 rule 4), so unit display waits for S6's typed catalogue.
   - The rule list is `render_plan`'s prose **lines** (the layout T18a documents and the prose goldens pin), one row
     each, read-only; chips are S3 (decision C3). Accessible names come from those lines.
   - An own `$`/`kW` meter from `get_economy_forecast`, polled on the rig's schedule (H10).
   - The lobby's **Resume last match**: the match id is remembered in `user://` and the config line carries the
     saved values plus `resume`. New matches get an id that cannot collide with an old one (`local-<unix
     seconds>-<pid>`, within `MatchCache::valid_match_id`; the host also refuses to overwrite a save, A2).
   - The headless per-template run: wizard, parameters, patch, verify, submit; the editor's bytes must equal
     plan-core's canonical bytes, and `gamectl verify` must accept the file.
   - A render-only scene of the wizard's first page (compared at T20, as in PR 1), and the GraphEdit note.
   - Uses T18a's `instantiate_suggested` golden as a guarded copy under `godot/fixtures/` until T18 merges, then the
     real operator in the headless run.
3. **Needs:** PR 1: T16 (merged). PR 2: T18a merged; T18 merged before its headless run (train T18 -> T19).
4. **Agent-days: 12** (was 10): +1 meter and Resume entry, +1 two lanes' fixed costs; chips, 0; the two shots move
   their comparison to T20 at no saving here.
5. **PLACEHOLDERs** (add): chip editing and drag reordering (S3); a live placement ghost (S3/S6); unit display of
   parameter values (S6); the meter's layout and refresh cadence (Tuning, owner); the save browser (S6); `user://`'s
   remembered match id as a per-machine convenience, not state.

### A5. T20, T21, T22 notes

T20: plus `library/` in the REUSE check, and the render-only screenshot mode item 110(4) books now carries two more
shots (T19's diagnostic rows and wizard page), committed and compared there. T21: `library/` and `rules/` packaged
beside `gamectl` (`LIBRARY_PATH`), and saves in the OS data directory (item 98) survive a reinstall. T22: the run sheet
adds "quit in a Lull, resume from the lobby, find the notes and drafts" and "kill the client mid-Push, resume, and
watch the same Push replay to the same end". It also confirms BMI's `settled` line shows in the event list, because the
recap's settlement payload stays a stub (S1). The PLACEHOLDER roll-up gains this file's.

### A6. What moves out of wave 6, and where it goes

Chips, drag reordering and family/predicate pickers go to **S3**; the live-on-hover legality ghost to **S3**; dashed
fogged legs to **S3** (as T16a said). The typed parameter catalogue goes to **S6**. The replay's reader and scrub go
to **S7**, with the recordings, and so does the replay's log. Scouts' live vision and recorded sightings in the view
wait for the owner (section D; S3 by default). Operator chatter, Survey-post placement, the commander's Defend guard and target
spread go to **S5**; the safe playbook's rescue rule to **S2/S5**. The forecast what-ifs, income and the recap's
settlement lines go to **S1**. Enemy beacon detail goes to **S2**. The operator in `scenario run` goes to **S2**,
with the adversarial scenarios. A save browser goes to **S6**. The casual phase term, the AGENTS.md status
paragraphs and the `xtask` screenshot fixes stay at **T20** (items 107, 109(6), 110(4)), and T19's two shots are
compared there.

## B. The lane plan

**Before run 1 (main session).** One `test(client-gdext)` commit on `main`: `..Default::default()` in the two
`BeaconSummary` literals of `crates/client-gdext/tests/fixtures.rs` (H14). No contract path, no behaviour, CI green on
the branch before it lands. Without it T18a's branch cannot compile until T19a merges, or T18a has to reach into the
Godot unit's crate while its owner is working in it (AGENTS.md section 6). *Fallback:* T18a gets those two literals as
a named place and T19a is told not to touch them.

**Run 1 (two lanes).** **T18a** on `feat/gateway-t18a` owns `proto/**` (the first commit, alone), `crates/proto`'s
generated tree, `crates/plan-core`, `crates/gateway`, `library/`, `deploy-and-visit`'s chain, and
`crates/gamectl/src/host.rs` (library path only). **T19 PR 1** on `feat/client-gdext-t19a` owns `godot/` and
`crates/client-gdext` (the T12/T16 author). The two share no file and no type. The train is **t19a -> t18a**, with T18a
last because it re-blesses goldens. The sim and the operator are idle on purpose: T17's gateway half and T18 both build
on T18a's seams, and a third lane here would build blind.

**Between runs (main session).**
1. Merge T18a's contract PR under items 85-86.
2. Open and merge one **harness-docs PR**:
   - AGENTS.md section 3's operator row becomes "`pharmakos-proto`; a call closure over gp.api.v1 JSON-RPC, adapted to
     the gateway's `BuiltInSeat` and `Advisor` seams in `gamectl`; the public rules text; a dev-dependency on
     `pharmakos-gateway` for tests", replacing "the gp.api.v1 service traits", which do not exist.
   - The `gamectl` row gains `pharmakos-operator` (the edge "decided at T18's opening").
   - Rule 3 gains one sentence, **as the owner answers section D's rate question**: an in-process seat goes through the
     same door, audit and fog as a socket, with its own rate limit.
   - Rule 2 names `Surface::file_advice` as host-side, and section 8's licence table puts `library/` with the example
     playbooks (MIT OR Apache-2.0).
   - `proto/buf.yaml`'s `ignore_only` block is deleted, as `buf.yaml`'s own comment and item 109(1) require once the
     discharge is on `main`.
3. Log the item adopting this file, with T18a's review findings.

Nothing from items 109(6) or 110(4) is pulled forward; they stay T20's.

**Run 2 (three lanes).** **T17** on `feat/gateway-t17` owns `crates/sim` and `crates/gateway`. **T18** on
`feat/operator-t18` owns `crates/operator`, `crates/gamectl/src/host.rs` and gamectl's `Cargo.toml` edge. **T19 PR 2**
on `feat/client-gdext-t19b` owns `godot/` and `crates/client-gdext`. **`library/` is frozen** for the whole run: the
gateway's equality test and `instantiate_suggested` golden (T17's crate) and the `godot/fixtures/` copy (T19's) all read
it. The train is **t17 -> t18 -> t19b**:
- T17 first, because its vision swap can change what `get_view` shows.
- T18 rebases on it and re-blesses its own golden playbooks if that swap moved them, stating the reason.
- T19 PR 2 rebases last and swaps its fixture copy for the real operator in the headless run.

The second lane of each train is rebuilt locally against the first's merge before its CI is waited on (wave 4's
lesson). T18's tests host a `Surface` through the dev-dependency while T17 changes the gateway underneath it: no
shared file, but a shared type. If T18 needs anything in the gateway or `library/`, it reports the exact need and stops;
the main session sequences it after the train.

**Contract PRs, in order:** T18a (proto, `buf.yaml` ignore block, REUSE, a scenario chain) at the end of run 1; the
harness-docs PR (AGENTS.md, `buf.yaml`) between runs; T17 (save and replay formats, the hash-file format on disk, the
sim query) in run 2. No `xtask` or workflow PR is expected: T19's shots are render-only until T20. If `library/` needs
the REUSE step taught anything, that goes to T20.

**Generated files and who re-blesses what.**
- Only **T18a** regenerates `crates/proto/src/generated/descriptor.binpb` and the generated tree. It re-blesses the
  schema-derived goldens (`tests/golden/proto/expected.reserved.txt`, `tests/golden/schema/get_schema/expected.json`,
  `tests/golden/docs/reference/**`), `tests/golden/gateway/walkthrough/`, and `deploy-and-visit`'s chain.
- No run-2 lane touches `proto/**` unless the owner chooses `save_match` (decision C8, option b). In that case the
  method goes into T18a instead, as `94`, so run 2 still regenerates nothing.
- No lane moves `SNAPSHOT_VERSION`, so no verifier `report_hash` golden moves. A lane that finds it must stops, and
  re-blesses every report golden with item 109(7)'s reason.
- The determinism chain does not move in wave 6. One scenario chain moves, in T18a, for the reason stated there;
  after it, the safe playbook `scenario run` files is the constant, and the constant equals the template's
  nothing-raised form.
- `godot/fixtures/instantiate_suggested.json` is a guarded byte-identical copy of T18a's golden, with its REUSE line,
  owned by the T19 unit.

## C. The decisions, with every option and its downside

### C1. Where a template declares its parameters (H1)

**Recommendation: in the template, `gp.v1.Meta.parameters = 6: repeated TemplateParameter {pointer, label}`, untyped
until S6** (5 stays reserved for `team_id`). `gateway.proto:761-764` already says "the template declares what it accepts". Spec section 13's library is data
("Nothing in the library executes"), and S6's Save as template makes templates no operator has code for. Numbers
3-15 are held for the typed catalogue. `instantiate_template` removes the field, so no playbook golden or fingerprint
moves. On a hand-written PLAYBOOK the field round-trips unread, as `meta.note` does; nothing is stripped, and whether
the verifier should refuse it there is S6's question (a new diagnostic code moves the catalogue golden and needs the
verifier crate, which no wave-6 lane owns). *Downside:* a `gp.v1` change (the player's file format, forever) ahead of
S6. `get_schema`'s and the docs' goldens move. A PLAYBOOK can carry a field that means nothing there. About 0.75 d more
than option B.
*Other options.*
- **(B) The operator defines each template's parameter set, gp.api.v1 only.** Cheapest. The wizard's structure
  lives in Rust and not in the data. A host with no advisor (tests, scenario runs, v1.1 agents) gets no pages. S6
  rebuilds it.
- **(C) A comment convention in the JSONC.** Comments would become structure, and neither buf nor the verifier can
  see them.
- **(D) A sidecar `.params` file.** A format outside the proto, invented here.

### C2. How the operator's suggestion reaches the wizard (H1)

**Recommendation: the advisor seam (C5) computes it at `open_lull` for each seat with no built-in operator (the
human's) and the gateway stores it per seat. On the wire it is `InstantiateTemplateRequest.suggested` plus `InstantiateTemplateResponse.{parameters, why}`.**
One call gives the wizard its pages, pre-filled values and the "why". The planning snapshot is frozen through the
Lull, so a suggestion made at its start is the one made at any moment of it. *Downside:* three additive fields and
one message forever; `instantiate_template` gains a mode; the suggestion is computed for templates a player may
never open (three instantiations and verifies per round, inside the in-process budget). It never shows on the golden
seed's round 1 as anything but the templates' own values, because no seat has a non-core beacon yet.
*Other options.*
- **(F) No advisor for the human at all (the cut the scope critic offers).** The wizard pre-fills with the template's
  own values and "why" comments; Easy files only its own seat's safe playbook; the human's timeout files the constant.
  About -2 d (T18a's advisor seam, allow-list and view-feed test; T18's advisor adapter), and section D's in-process
  token question disappears. *Downside:* spec section 13's "pre-filled with the built-in operator's suggestion" and
  T19's plan line are not met; `get_safe_plan` for the human keeps showing a constant that is not spec section 14's
  safe playbook, so "the cost of a timeout" the demo promises is the wrong cost once power runs short.
- **(B) A new method, `suggest_template` = 57.** A forever method name for what one flag does, and a second call
  per page.
- **(C) The suggestion on `TemplateSummary`.** `list_templates` is `docs`-scoped and seat-agnostic by nature, and a
  per-seat suggestion there muddles that.
- **(D) The advisor saves each suggestion as a draft (no proto at all).** The gateway would be putting words in the
  seat's drafts, exactly what `carry_draft_forward` refuses to do for the safe playbook. It spends the 32-draft cap,
  and the wizard still has no parameter list.
- **(E) The gateway links the operator and computes suggestions on demand.** See C4 (B).

### C3. Chips and sentences (H1)

**Recommendation: defer chips to S3.** At the skeleton the rule list is `render_plan`'s prose lines, read-only, and
the accessible names are those lines. Plan section 1.1's demo names no chip: "fills a parameter or two" is the
wizard's job. S3, the authoring stage, designs the structured render once, with drag reordering and the pickers.
*Downside:* T19 ships a read-only rule list; "sentences with clickable parameter chips" (spec section 13) is not met
until S3; editing is by wizard, map and Fix only.
*Other options.*
- **(B) Structured render now,** `RenderPlanResponse.lines: [{pointer, spans: [{text, pointer, value}]}]` with
  `prose` equal to their join. About +2.5 agent-days (plan-core 1.5, client 1), and a forever wire shape chosen
  before the editor that uses it. Chip values would be raw JSON (a duration in ms), because the editor may not
  convert "3 s" itself.
- **(C) The client parses the prose.** A parser of English in GDScript, broken by every wording change. Rejected.

### C4. Linking the operator (H2)

**Recommendation: the operator depends on `pharmakos-proto` only and drives a `FnMut(&str, Json) -> Json` client.
`gamectl host` adapts it to `BuiltInSeat` and `Advisor`, and gamectl gains the edge to `pharmakos-operator`. Tests
use a dev-dependency on the gateway.** No sim type is reachable in the shipped build, so rule 3 is a dependency fact
rather than only a source-text test. Verify, patch and render go through the gateway, which is "same verifier, same
submit path" literally. *Downside:* every verify and patch is a wire call, so the in-process budget (C6) must cover
it. There are about forty lines of adapters in `host.rs`. The operator row's "gp.api.v1 service traits" wording
must change in the harness PR. A dev-dependency on the gateway compiles the sim into the operator's tests, which
the research ban already covers.
*Other options.*
- **(B) The operator depends on the gateway and implements `BuiltInSeat` itself.** No adapter, but the operator can
  name `Host`, `Surface` and every sim type the gateway re-exports. Only a source-text test would stand between it
  and a privileged read.
- **(C) The gateway depends on the operator** (possible once the operator needs only proto). No seams and no
  `gamectl` edge, and scenario runs would get the real safe playbook for free. But the host would depend on one of
  its own clients, the operator would compile into the crate v1.1 publishes, and nothing structural would stop the
  gateway handing it a `World`.
- **(D) A `SeatClient` trait in `crates/proto`**, the literal "gp.api.v1 service traits". One more hand-written
  module in the MIT/Apache schema crate, whose row would widen. The closure already is that trait.

### C5. The safe playbook on a miss, and what `get_safe_plan` returns (H3)

**Recommendation: the `Advisor` seam, for each seat with no built-in operator, computed at `open_lull` through a
per-seat in-process token without `plan.submit` and behind a host-side method allow-list (no `save_notes`,
`save_draft`, `list_drafts`, `submit_plan` or `set_ready`), filed by `begin_push` after its own FULL verify, with
`SAFE_PLAYBOOK` as the audited fallback. `get_safe_plan` returns that seat's advised playbook, or the fallback when no
advisor is installed. A built-in seat's operator submits its own safe playbook when its own plan fails.** This keeps the
spec's split: the operator files it (section 14) and the gateway enforces the door. Computing at the start of the
Lull equals computing at timer end, because the frozen snapshot does not change in a Lull. *Downside:* the host
process holds a `plan`-scoped token for the human's seat. The allow-list takes away every write and the drafts, but
`get_briefing` still hands it the notebook; the operator ignores it (spec section 14) and a test pins that, but the
read exists. An advisor's `get_view` writes into the human's per-viewer feed state (H15), so a test must show it
changes nothing the human sees, or the advisor path goes without `get_view`. `scenario run` and in-process tests keep
the constant until S2.
*Other options.*
- **(B) The operator submits it for every seat through `submit_plan`.** It needs a submit-scoped second token for the
  human's seat. The playbook would reach draft continuity as "yours". `plan_sealed` would misattribute it. "A
  timeout after a verified submission changes nothing" then depends on submission order. Rejected.
- **(C) The gateway links the operator** (C4 (C)). Its downsides stand.
- **(D) Generate it in plan-core, called by the gateway with the snapshot.** No seam, but it contradicts spec
  section 14 ("the built-in operator files the safe playbook") and rule 3, and puts situational choice in the
  crate that may only estimate.
- **(E) Keep the constant.** Not the spec's safe playbook: it cannot raise priority when power is short.

### C6. The built-in seat under the rate limit (H8): an owner question now, not only a number

**Recommendation: per-token `Limits` for in-process tokens (`IN_PROCESS_LIMITS`, PLACEHOLDER), with Easy's call budget
derived in the operator from its evaluation units (30 candidates, 1 + 4 verifies) and tested against it.** The limiter
exists to stop a runaway client; an operator whose calls are bounded by its own evaluation budget and asserted in a
test is not one. Every call is still counted, audited and fog-filtered. *Downside:* this changes the seam's own
contract: `serve.rs:131-134` says a built-in seat is "rate-limited ... exactly like a socket", and it would not be.
It is a rate privilege and not a read privilege, but it is a privilege, so the owner decides it (section D), T18a
rewrites the seam's doc, and the harness PR adds the sentence to rule 3.
*Other options.*
- **(B) A resumable operator** that spends at most eight calls per gateway tick and resumes on the next. No privilege
  and no seam change, and the operator stays literally "exactly like a socket". But every stage of the operator
  becomes a state machine; in a Lull the gateway's tick moves only on `report_host_clock`, so a round needs one clock
  report per eight calls (Easy's round is about fifty calls, so six or seven reports, under 2 s at the rig's 250 ms cadence, and the
  advisor as much again), and a headless
  host that reports no clock (`scenario run`, in-process tests) never finishes without a test driver that reports
  one. About +1.5 d in T18.
- **(C) Raise `CALLS_PER_TICK` for everyone.** It weakens the one guard sockets have.
- **(D) Batch reads.** A new method shape for one client.

### C7. The operator's inputs and the meter (H9, H10)

**Recommendation: in T18a, discharge `GetEconomyForecastResponse` 1-4 as present-state values (`treasury_now`,
`supply_kw_now`, `draw_kw_now`, `headroom_kw_now`) and `BeaconSummary`'s owner plus own-only core, priority and
powered.** It is the only way an ordinary client can know "power is short", and it gives the demo its meter. This
**departs from item 105(2)**, said plainly: that item has the forecast fields "filled by plan-core's `$`/`kW`
projection", landing "with the lane that fills them (S1's economy work, or T19 if the wizard needs them first)", and
warns against "a shape guessed ahead of its only producer". The departure is kept small on purpose: the four fields
are the present state the sim already reads out (`tables.rs:545-600`), not a projection, and their names say so, so
S1's projection, income and what-ifs arrive as new fields and never redefine these. *Downside:* four fields frozen
before S1's economy review; discharged reserved numbers need `buf.yaml`'s `ignore_only` for one PR; the method named
"forecast" answers the live world during a Push.
*Other options.*
- **(B) A structured briefing payload** (`GetBriefingResponse` 5-15). One call for more, but the "salience budget"
  design is S3's.
- **(C) Own economy on `get_view`.** It breaks T16a's never-on-the-wire allow-list.
- **(D) The operator reads the world in-process.** A privileged read, and rule 3 forbids it.
- **(E) Wait for S1, as item 105(2) says.** No departure; but the safe playbook cannot tell that power is short, so
  it stays the constant (C5 (E)), and the demo has no meter.

### C8. The save trigger, and where a resume lands (H4)

**Recommendation: automatic, at the two Lull boundaries item 84 names ("save at Lull boundaries"): at `begin_push`
once every seal is final and the safe playbooks are filed, and on control end-of-file during a Lull. One `save.json`
per match. A `sealed` save resumes straight into its Push with its seals; a `lull` save resumes into an ordinary
Lull.** The automatic trigger rests on item 84, not on stretching the spec's "can save". Resuming a sealed save into
its Push means a killed client replays the same Push to the same chain and cannot re-plan what it watched ("orders you
cannot take back", AGENTS.md section 1); a quit in a Lull keeps the notes, drafts and any verified submission, which
the spec already lets a seat replace until its timer ends. A host crash mid-Lull loses that Lull's drafts and resumes
from the previous `sealed` save, which replays the previous Push deterministically. A save at `open_lull` is not
needed: it would equal the previous `sealed` save plus a Push the replay reproduces. *Downside:* the spec says the
host "can save", and this saves without asking; a resumed Lull restarts its timer in full, because the timer is the
client's (item 99), so quitting buys planning time (PLACEHOLDER); the private cache now holds playbooks, as spec
section 3 says saves do; one slot means no save history.
*Other options.*
- **(B) `save_match` = 94 (admin), a lobby button.** Explicit, but a forever method plus a pending-save flag through
  `control.rs`, whose needle list bans `snapshot`. A killed client loses everything since the last press.
- **(C) Three triggers, adding `open_lull`** (the first draft). Nothing the two do not already cover, and one more
  write per round.
- **(D) Every resume lands in a Lull** (the spec read literally, the first draft). Simpler by one path, but a player
  can watch a Push, kill the client, resume and re-plan it.
- **(E) (A) now, and (B) additive later** if the owner wants named slots. This is the recommendation's own escape.

### C9. The resume entry (H4)

**Recommendation: a seventh config-line field, `resume`, with fields 1-5 required to equal the save's (refused
otherwise). `serve.rs` reads the save through `cache.rs` before it opens the surface (fs stays in one module), and the
lobby offers "Resume last match".** `gamectl host` needs no change. *Downside:* the config line grows (it was built
to); the lobby remembers a match id client-side and must repeat the saved settings; `serve::run` reorders its opening.
*Other options.*
- **(B) The binary reads the bytes** (the t16a notes' guess). `gamectl` would learn the cache layout, which is
  `cache.rs`'s.
- **(C) `gamectl resume <id>`.** A second entry to keep equal to `host`.
- **(D) A save browser listing the cache.** It needs a listing method or client file reads. S6.

### C10. What the save carries, and where the fingerprint lives (H4)

**Recommendation: a gateway-owned JSON container.** It holds the stamp, the config, the snapshot bytes (base64), and
per seat the notebook, drafts and seal with its fingerprint (items 103(5), 104(4)). The snapshot format is untouched.
Said plainly, the fingerprint check compares two values that both come from `save.json`: it catches a damaged file
and a canonical form that changed between builds, not tampering, which the private cache does not claim to stop.
*Downside:* two formats in one file (the sim's postcard inside the gateway's JSON), and a contract PR for the
container.
*Other options.*
- **(B) Fingerprints in the snapshot.** Bumps `SNAPSHOT_VERSION`, and every verifier `report_hash` golden moves (item
  109(7)) for a field no tick reads.
- **(C) postcard for the container.** The gateway would gain `serde`/`postcard`; the approved list scopes `serde` to
  postcard's derive, and the gateway has neither today.

### C11. The private replay, and "no other seat can read it" (H4)

**Recommendation: inputs only, in T13's reserved layout.** Seals go to `sealed/<round>.jsonc`, each segment's chain
to `replay/<round>.hashes.txt`, and the seed, settings and rules hash to `match.json`. One test re-hosts from them
and compares chains. "No other seat's token can read it" becomes a confinement test that no handler reaches a
`cache.rs` reader, beside T13's secrecy tests: no wire method exposes a replay in v1. *Downside:* the replay format is
"a folder", and nothing but a test reads it until S7. It is seed plus playbooks plus hashes, **not** spec section 15's
"seed + playbooks + log": the log is a PLACEHOLDER (owner, S7). `hashes.txt` reuses the golden hash-file format, which
makes that format a contract T17's PR names. The chain sits on local disk (not the wire), which T16a's no-hash rule
does not cover, and the adopting item should say so.
*Other options.*
- **(B) A sim `replay` module with its own encoded format and runner.** About +1.5 d, a determinism contract to
  freeze now, and S7 redesigns replays with recordings ("two formats", spec section 15).
- **(C) Defer the replay to S7 altogether.** -0.75 d, and item 84 never decided it, but the sealed files are nearly
  free and the equality test is the stage's best determinism check.
- **(D) Write a `pharmakos.scenario.v1` file.** That format has one playbook per seat for the whole match, so it
  cannot hold a multi-round match. A v2 is a harness-format change.

### C12. H5's placement and shape

**Recommendation: T17 in run 2, `World::in_own_sphere(seat, voxel)` as items 107(6) and 110(5) name it, reusing
`within`; the seat bound goes in `Config::parse` and `Host::open_from`. Scouts' live vision and recorded sightings stay
out of the view until the owner answers section D's question.** The owner's own answer, item 108(1), is that enemy
units, beacons, structures and edits "appear only inside own beacon spheres until the match-end unlock"; the log
outranks the spec and this design step. *Downside:* a scout fielded by a Survey mandate finds things the seat's
knowledge records but the view does not draw, and item 107(6)'s "adds Survey-lite's sightings" is left for the owner
rather than done.
*Other options.*
- **(B) `World::sees` = spheres plus scouts** (the first draft), extracted from `record_sightings` so view and memory
  share one rule. It matches spec section 6's "live vision of your own units", but overrules item 108(1) without the
  owner, and the scout radius, a code constant, becomes a view rule.
- **(C) Sightings (memories) join `Vision`,** as item 107(6) reads literally. Last-known positions drawn as live
  ones; that is S3's knowledge store, and `host.rs:474`'s PLACEHOLDER already puts it there.
- **(D) T18a does it in run 1.** It needs a sim edit in a run where the sim has no owner. Adding one would be a third
  lane for 0.5 d.

### C13. The template folder (H7)

**Recommendation: `library/` at the repository root, flat, MIT OR Apache-2.0 like the example playbooks.** Plan-core's
reader lists one folder and tells templates from samples by `kind`. `gamectl host` passes `root/library` through a
named `LIBRARY_PATH`, beside `RULES_PATH`, and T21 moves it for packaging. *Downside:* a new top-level directory with
a REUSE annotation and a `LICENSE` pointer.
*Other options.*
- **(B) `examples/templates/`.** Already MIT/Apache, but a shipped library named "examples", and plan-core would
  read a subfolder.
- **(C) `godot/library/`.** GPL by the `godot/**` annotation; the gateway would read the client's folder; one
  ownership unit would hold T18's data.
- **(D) `include_str!` in the operator.** Spec section 13 makes the library a folder the gateway reads, and a player
  could not copy one.

### C14. The run structure (H6)

**Recommendation: two runs as in section B, 2 + 3 lanes, after a two-line pre-run commit on `main` (H14).**
*Downside:* run 1 leaves one slot idle, and the main session carries a pre-run commit and a between-runs PR.
*Other options.*
- **(B) One run, with the main session's proto PR first and T18/T19 against fixtures.** Faster by a run, but the
  operator and the wizard are built blind to the real seams, with rework likely.
- **(C) Three runs** (T18a; T17 + T18; T19 whole). T19 stays one lane, but a run longer, and the Godot lane idles
  through two runs.
- **(D) Fold T18a into T17** (one gateway lane). About 14.5 d, over the 10-day seam rule, and T18/T19 wait for all of
  it.

### C15. Easy's randomness

**Recommendation: none.** Easy uses a total order with ties to the lowest id and records the seed without using it.
The spec's "seed comes from the match, seat and round" says what the operator may depend on, and a function with no
draw satisfies it. *Downside:* two Easy seats in rotated positions play alike, and "top-k" reads as "best of k".
*Other options.*
- **(B) A `Stream::Operator` in the sim's stream enum.** A determinism-contract change, and the operator would name a
  sim type.
- **(C) The operator's own SplitMix64.** A second RNG outside the one enum (AGENTS.md section 4.7).

### C16. The safe template against the gateway's constant (H13)

**Recommendation: T18a writes `library/safe_playbook.jsonc` to spec section 14 (move to the safest beacon; the
priority raise as parameters, empty by default; then shadow the safest beacon, with a flee handler), makes
`SAFE_PLAYBOOK` its nothing-raised instantiation, and re-blesses `deploy-and-visit`'s chain with that reason.
`library/` is then frozen through run 2.** One definition, the spec's, and the scenario that exercises the `safe`
seat kind pins it. *Downside:* a scenario chain moves in a wave that otherwise moves none; the rescue rule is still
absent (section D); T18 cannot retune a template in run 2.
*Other options.*
- **(B) The template's base form equals today's constant**, and the operator adds everything situational. No chain
  moves, but the file called "Safe Playbook" is not the spec's safe playbook, and the flee handler exists only in what
  the operator writes.
- **(C) Move the equality check into the operator's tests** and leave `library/` open to T18. T18 could tune the
  templates, but a template change still moves the gateway's `instantiate_suggested` golden (T17's crate) and T19's
  fixture copy mid-run.

### C17. The operator's tuning inputs (H12)

**Recommendation: `gamectl` hands the operator factory the public rules text it already hands the host
(`Setup.rules_json`), and the operator reads the named rows through `pharmakos_proto::json`.** The rules table is
public data, the same committed file every client pins (item 110(6): the rig already reads `match.lull_ms` from its
copy), so this is no privileged read of the match. *Downside:* the operator has one input that is not a gateway call,
so the operator row in AGENTS.md section 3 names it; a v1.1 agent would read the same file.
*Other options.*
- **(B) A cost-free utility at Easy** (PLACEHOLDER): score only distance and route fill. Nothing new, but "economy"
  and "capability" terms of the spec's utility are zero at the skeleton.
- **(C) A rules method on the wire** (`get_capabilities`' pair early, or a new method). A forever shape S4 designs.
- **(D) Constants in the operator.** Tuning values in code; AGENTS.md section 12 forbids it.

## D. What the spec and the log do not answer (owner questions, each with a proposed PLACEHOLDER)

**For the owner now, before run 1 launches** (each is taken on the recommendation under the standing instruction if
the owner does not answer, and each changes a lane's brief if overruled):

1. **May an in-process seat have its own rate limit?** (C6.) The seam says built-in seats are limited "exactly like a
   socket". PLACEHOLDER: yes, `IN_PROCESS_LIMITS` sized to an evaluation-unit budget, counted and audited, never a
   read privilege. The alternative is a resumable operator (+1.5 d; a round spread over six or seven clock reports;
   a headless host needs a clock driver).
2. **Where does a resume land?** (C8.) PLACEHOLDER: a save made at the Push's start resumes into that same Push with
   the same seals (the watched Push cannot be re-planned); a save made on quitting in a Lull resumes into that Lull.
   The alternative, every resume lands in a Lull, lets a player take back sealed orders by killing the client. This
   also answers the t16a notes' "client death mid-Push" PLACEHOLDER ("owner, with T17").
3. **A `plan`-scoped in-process token for the human's seat** (the advisor, C5). PLACEHOLDER: allowed, never
   `plan.submit`, behind an allow-list with no writes and no drafts, notebook ignored, tested. The alternative is
   C2 (F): no advisor for the human, -2 d, and the wizard pre-fills with the templates' own values.
4. **Do scouts, or Survey-lite's sightings, show in the live view?** (C12.) Item 108(1), the owner's answer, says
   only inside own spheres; item 107(6) said sightings join the view; spec section 6 names "the live vision of your
   own units". PLACEHOLDER: spheres only (108(1) stands), both terms S3's. If the owner says yes to scouts, T17 builds
   `World::sees` = spheres plus scouts (C12 (B), same cost) and `programs::UNIT_VISION_RADIUS_VOXELS` becomes a view
   rule that needs a rules row (owner, S1, item 105(1)'s mechanism).
5. **Automatic saving** where the spec says the host "can save". PLACEHOLDER: automatic, at item 84's Lull
   boundaries, one slot per match. Owner, S6 with a save browser, where `save_match` = 94 stays additive.

**Later, with the stage named:**

- **"Power is short" and "at-risk beacon"** (spec section 14). PLACEHOLDER: short = own `headroom_kw_now` < 0 at the
  frozen snapshot; at-risk = own non-core beacon `powered = false`. Owner, S1 with the grid.
- **The rescue rule in the safe playbook.** Section 14 says "with the flee and rescue rules"; plan T18 says the flee
  rule; nothing can rescue before combat. PLACEHOLDER: flee only. Owner, S2/S5.
- **A resumed Lull's timer** starts full, because the Lull timer is the client's (item 99) and the save does not
  carry what was left, so quitting buys planning time. PLACEHOLDER: full. Owner, hardening.
- **The replay's log** (spec section 15: "seed + playbooks + log"). T17 writes seed, playbooks and hashes, no event
  log; and the last recap's event list does not survive a resume. PLACEHOLDER: none. Owner, S7 with the recordings.
- **`meta.parameters` on a PLAYBOOK** round-trips unread, as `meta.note` does. PLACEHOLDER: no diagnostic. Owner, S6
  (a refusal is a new catalogue code).
- **What "top-k" means, and whether Easy draws at all.** PLACEHOLDER: best-of-k, no draw. Owner, S5.
- **The live own-economy readout during a Push** through `get_economy_forecast`. Spec section 3 allows "own score"
  live; `$`/`kW` are not named. PLACEHOLDER: own seat only, every phase. Owner, S1.
- **Section 14's Survey post, Defend guard, chatter and target spread** at the skeleton. PLACEHOLDER: none at Easy.
  Owner, S5.
- **Which templates the operator suggests for a human, and whether a suggestion may name another seat's beacon.**
  PLACEHOLDER: all three, own beacons and fixed targets only (Easy's row). Owner, S5.
- **The "why" note's wording** is operator-generated English from the operator's string table. PLACEHOLDER: one
  sentence per suggestion. Owner, S6 (the one string table).
- **Unit display of wizard values** (a duration shown as ms). PLACEHOLDER: raw values under the template's label.
  Owner, S6 with the typed catalogue.
- **`IN_PROCESS_LIMITS`' numbers against `EASY_CALL_BUDGET`.** Owner, hardening, with the rate limits.
- **A save from an older build** is refused. PLACEHOLDER: no converter. Owner, hardening.
- **A live placement-legality ghost** within the rate budget: per click at the skeleton. Owner, S3/S6.

## E. Cost

| Run | Lane | Agent-days |
|---|---|---|
| 1 | T18a planning wire | 8.75 |
| 1 | T19 PR 1 | 5.5 |
| 2 | T17 | 6 |
| 2 | T18 | 8 |
| 2 | T19 PR 2 | 6.5 |
| | **Wave** | **34.75** (plan: 22) |

At wave 5's measured **195 k tokens per agent-day** under item 101's process (a rate that already includes a Godot
lane, T16, item 110(7)), that is about **6.8 M** sub-agent tokens: run 1 about 2.8 M (14.25 d), run 2 about 4.0 M
(20.5 d), against the plan's 22 d ≈ 4.3 M. If item 101's ×1.3-1.6 Godot penalty is applied on top to the two T19 PRs
(12 d, 2.3 M at the flat rate), the wave is **7.5-8.2 M**. Add this design step, the pre-run commit, and the main
session's harness-docs PR. The plan's total moves from 182 to about 194.75 agent-days.

**Where the +12.75 d comes from:**
- T18a, 8.75 d: the wire the plan assumed existed, now with the allow-list, the view-feed test and the safe template
  replacing the constant.
- T17 +2: the gateway half was never priced.
- T19 +2: the meter, Resume, and a second lane's fixed cost.
- Chips cost nothing now, because they are deferred.

**Dearer:**
- If the Godot penalty applies, the top of the range above.
- Two contract PRs (T18a, T17) and one harness PR queue on the main session.
- If the owner answers D1 with the resumable operator, T18 +1.5 d.
- Lanes start cold if caches were cleared; an outage re-runs work (item 109(2)'s rule applies).

**Cheaper:**
- T18 is pure Rust against a finished wire, and its hardest test runs on a scripted client.
- No run-2 lane regenerates the descriptor.
- No determinism or `report_hash` golden is expected to move; one scenario chain moves, in T18a, on purpose.
- If the owner wants less, the cuts are, in order: C2 (F), no advisor for the human (-2 d; the wizard pre-fills with
  the templates' own values and the human's timeout files the constant); C11 (C), defer the replay (-0.75 d); C1 (B),
  operator-defined parameters (-0.75 d); the meter to S1 (-0.5 d; the demo loses "kW supply rises"); Resume in the
  lobby to T22 (-0.5 d). All five give about 30.25 d ≈ 5.9 M at the flat rate.

## F. Critique items not taken, or taken in another form

Verified against the code or the log; everything else in both critiques was taken as written.

- **Rules 5 (meta.parameters on a PLAYBOOK is a "silent strip").** Taken in part. The field number is now named (6).
  But round-tripping an unread field is not stripping: the bytes stay in the file, exactly as `meta.note` does
  ("Round-tripped, never interpreted"), and AGENTS.md section 11's rule is about out-of-vocabulary constructs, which
  this is not. A refusal needs a new diagnostic code in `crates/verifier`, which no wave-6 lane owns; it is an S6
  PLACEHOLDER instead of an owner-now one.
- **Rules 6 (resume re-opens planning).** Taken, and answered rather than only asked: C8 now recommends that a save
  made at the Push's start resumes into that Push. The owner question stays (D2).
- **Build 1 (the `BeaconSummary` literals).** Taken in another form: neither suggested fix fits one-crate-one-agent
  in a run where both lanes are live (T19a's first commit leaves T18a red until the train; a named place puts two
  agents in `crates/client-gdext`). The main session lands the two lines on `main` before run 1; the named place is
  the fallback.
- **Build 2 and Rules 2 / Build 5 (the constant, the template and `library/`).** Taken together as C16: the template
  is the spec's, the constant becomes its nothing-raised form, the scenario chain is re-blessed in T18a, and
  `library/` is frozen in run 2. Build 2's claim that the constant "does neither" is half right: it does fall back to
  the safest beacon (`host.rs:103`), after a 1 s hold; it has no flee handler and no shadow.
- **Build 6 (cut the human advisor).** Not taken as the recommendation, because spec section 13 and T19's plan line
  both require the wizard "pre-filled with the built-in operator's suggestion", and `get_safe_plan` should show the
  spec's safe playbook. Taken in part: the advisor runs only for seats with no built-in operator (built-in seats
  file their own), the cut is listed as C2 (F) and as the first cost cut, and its handle-order concern is H15 with a
  test.
- **Build 8 (the surface-thread stall).** Taken, with one change: the wall time is measured outside the deterministic
  crates (AGENTS.md section 4.5) and reported beside P2 as a baseline, while the call count is the asserted figure.
- **Build 11 (cut the replay now; two triggers).** Two triggers taken (C8). Deferring the replay not taken: it is in
  T17's plan line and acceptance, the sealed files are nearly free, and the chain-equality test is the stage's best
  determinism check. It stays the second cost cut.
- **Build 12 (the at-most-two test needs three beacons).** Taken in another form: neither a scripted Push that places
  three beacons (priced, slow, and it runs combat-free construction the operator's crate should not drive) nor a host
  test seam in the gateway (T17's crate in run 2). The operator's only input is the wire, so the test drives it
  through a scripted client closure answering with three unpowered non-core beacons.
- **Build 15 (the handle test proves nothing).** Taken in another form: another client polling the view first
  changes a different viewer's state, not the operator's. The test permutes every `u_`/`s_` handle in the scripted
  `get_view` answer instead and asserts the same playbook.

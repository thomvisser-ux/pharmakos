<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0

CONTRACT FILE: changes need owner approval.
-->

# Example playbooks

Licence: **MIT OR Apache-2.0**, like the rest of the schema surface — `.proto`
files, generated JSON Schema, docs, `llms.txt` and example playbooks. The game
itself (sim, client, operator, gateway, `gamectl`) is GPL-3.0-or-later; art and
audio are CC BY-SA 4.0. Per-directory `LICENSE` files and a REUSE-style
manifest carry this; the SPDX header at the top of each file is the
machine-readable half.

## What is in here

| File | Purpose |
| --- | --- |
| `playbooks/expand_east.jsonc` | The worked example from spec section 10, copied verbatim but for one line. The golden file for the `gp.v1` envelope. |

## What a playbook is

A playbook is typed declarative data, not a script: a route of steps,
prioritised handlers (rules), a required `on_death` block and a required
`fallback`. It drives one seat's commander for one segment, and it is sealed
during the planning Lull — nobody has live control once the Push starts.

On disk it is a **JSONC** file: canonical proto JSON for the `gp.v1` package
plus comments. It is hand-editable in any text editor, and the in-game editor
round-trips comments and formatting, so a hand-written file survives being
opened and saved. Templates and the sample library are the same format.

There is no script language, no SDK and no script runtime in v1. External
scripting arrives in v1.1 and emits exactly this format through exactly this
verifier.

Two rules worth repeating before anyone edits one of these:

- **Load never strips.** An out-of-vocabulary construct is rejected with a
  verifier code and a JSON Pointer, on Load as well as on submit. A file either
  loads as written or says why it did not.
- **Times are game milliseconds, never ticks.** The sim runs at 20 Hz and takes
  decisions every 250 ms of game time, but no number in a playbook is a tick
  count.

## Why `expand_east.jsonc` is copied verbatim

It is the reference the schema is written against, not an illustration. The
spec's example fixes the JSON spelling of every name in the envelope
(`author_kind`, `place_beacon`, `cmdr_hp_pct`, `beacon_anchor`), and canonical
proto JSON writes an enum as its value name verbatim — so `"author_kind":
"HUMAN"` in the spec means the proto enum value is literally `HUMAN`, not
`AUTHOR_KIND_HUMAN`.

That is why `proto/buf.yaml` excepts `ENUM_VALUE_PREFIX` and
`ENUM_ZERO_VALUE_SUFFIX` from buf lint STANDARD, and why every enum in `gp.v1`
is nested inside the message that owns it: proto enum values live in the parent
scope, so two top-level enums could not both spell `CONTINUE`
(`Handler.Resume.CONTINUE` and `OnDeath.OnRespawn.CONTINUE` both exist in this
example). The lint exceptions and the nesting are one decision, and it is
reversible only by changing the player-facing file format.

`buf breaking` runs in **WIRE_JSON** mode for the same reason: renaming a field
or an enum value breaks files on disk even when the wire tags are untouched.

## Open points for the owner

1. **int64 in JSON — SETTLED.** `docs/design/decisions-log.md` item 46 ratifies
   what the schema had applied provisionally: every duration field in `gp.v1`
   is `int32`, so canonical proto JSON emits a bare number and the spec's
   example loads as written; a negative value is rejected by the verifier with
   a code and a JSON Pointer, not at decode. `crates/proto`'s
   `no_duration_field_in_gp_v1_is_int64` test walks the descriptor set and
   fails if anyone ever widens one.

2. **The one line that is not verbatim: `"kind": "PLAYBOOK"`.** Spec section
   10's example predates the envelope's kind/version tag. Item 47 held
   `Playbook` field 7 and the name `kind` as a reservation, to be defined with
   the walking skeleton; item 76 defines it as
   `KIND_UNSPECIFIED / PLAYBOOK / TEMPLATE / SAMPLE`, with the verifier
   rejecting `KIND_UNSPECIFIED`. The example therefore has to carry the tag, or
   the spec's own worked example would not qualify. The deviation is recorded
   in the file's header as well as here, because "verbatim" is only worth
   anything if a departure from it is written down.

3. **Placeholder stubs.** The walking skeleton filled several of them: the
   section-5 interface-row catalogue, the four selector forms, `BuildSettings`,
   `SurveySettings`, the Common mandate settings, and the condition families
   the v1 interpreter and the three skeleton templates need. What is still a
   stub, each naming the stage that fills it: `DefendSettings` and
   `AttackSettings` (S2, with combat), the broadcast message bodies (S4, with
   radio), the rest of the predicate catalogue (S3), the blueprint and
   capability catalogues (spec section 8), and `MineSettings`' `seam_choice`
   and `pillar_spacing`, which belong to the spec section 6 mandate contract.
   Whether the mandate messages eventually move to `proto/gp/v1/mandate.proto`
   is still that contract's call.

## Checking an example

All three run from the workspace root:

```
buf lint proto
buf breaking proto --against '.git#branch=main,subdir=proto'
gamectl verify examples/playbooks/expand_east.jsonc
```

Both buf commands are run **from the workspace root**, not from `proto/`: buf
resolves the path in a `.git#…` reference relative to the directory it is
invoked from, and `.git` is at the root. `cargo xtask ci`'s `buf` step does
exactly this.

`gamectl verify` runs the same verifier the gateway runs, at the same depths
(`quick`, `full`). A verifier report is deterministic: it is a pure function of
playbook bytes, snapshot, rules hash, verifier version and depth, so a
pre-check and the check at submit are byte-identical.

## What these examples are not

Golden example playbooks, `llms.txt`, `llms-full.txt` and an agent guide ship
with the **v1.1** published API. This directory is the v1 seed: one file, kept
honest against the spec.

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
| `playbooks/expand_east.jsonc` | The worked example from spec section 10, copied verbatim. The golden file for the `gp.v1` envelope. |

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

Two things in this directory are not yet settled, and both are visible in the
example file:

1. **int64 in JSON — answered in the schema, awaiting ratification.** The
   proto3 JSON mapping *emits* 64-bit integers as quoted strings
   (`"timeout_ms": "120000"`) while *accepting* both forms. The spec's example
   writes them as bare numbers, and this file copies it verbatim. Rather than
   have the canonical writer deviate from the mapping, the five duration fields
   in `proto/gp/v1/playbook.proto` are typed **`int32`**: an int32 emits a bare
   number, so schema, mapping and golden file agree, and 2^31 ms (about 24.8
   days) is ample against an 8-minute maximum segment. The reasoning is written
   out in that file's header. **Owner:** ratify it with an entry in
   `docs/design/decisions-log.md` §2.7 before the schema is frozen; the
   alternative — a canonical writer that deviates from the proto3 mapping for
   int64 — would have to be recorded in the same place.
2. **Placeholder stubs.** `MandateSettings` and its five per-writ messages sit
   in `proto/gp/v1/playbook.proto` only so this example type-checks. They
   belong to the spec section 6 contract and should move to
   `proto/gp/v1/mandate.proto` when that contract is written. The same goes
   for `InterfaceRow` (spec section 5) and the condition predicate catalogue
   (spec section 10). Each is marked `PLACEHOLDER STUB` in the schema.

## Checking an example

Nothing here has been executed: there is no `cargo`, no `protoc` and no `buf`
on the authoring machine yet. Once the toolchain exists:

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

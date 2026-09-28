<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# `plan-core/` — canonical form, the JSONC round trip, and prose

**Filled by T8** (`crates/plan-core`).

Three kinds of golden live here, and they fail for different reasons:

* `expected.jsonc` — a hand-written playbook reproduced **byte for byte**
  through load, canonicalise and save, with comments and formatting intact. This
  is the property the whole editor model rests on (spec §13, "Exact
  round-trip").
* `expected.canonical.json` — the canonical form the writer emits.
* `expected.prose.txt` — `render_plan`'s English rendering.

## What a diff means

* **The JSONC round trip moved.** A comment moved, was dropped, or whitespace
  changed. This is never cosmetic: the editor promises the player that saving a
  file they wrote by hand gives the file back. A dropped comment is a bug even
  when the JSON is identical.
* **The canonical form moved.** Key order, number formatting or default-value
  handling changed. `report_hash` is a pure function of the playbook bytes, so a
  canonical-form change moves every verifier golden with it — if this moved and
  `verifier/` did not, one of the two is wrong.
* **The prose moved.** Either a template string changed (say so; it is the one
  the player reads) or the plan itself renders differently, which is a semantic
  change wearing a typographic disguise. The prose says **nothing** about the
  coming segment's length. The frozen snapshot carries it since T10
  (`Snapshot::coming_segment_ms`, which `crates/plan-core/src/context.rs` reads,
  saying its PLACEHOLDER against T10 is discharged), but `render_plan`
  (`crates/plan-core/src/render.rs`) reads nothing from the context it is
  handed, and its test `nothing_is_claimed_about_the_segments_length` keeps the
  word out of the prose. So a segment ladder change alone moves no prose here;
  a prose diff that follows one means the renderer started reading the segment's
  length, which is a rule change to state, not a golden to re-bless.

## The cases

| Case | File | Produced by |
| --- | --- | --- |
| `expand_east/` | `expected.jsonc` | `crates/plan-core/tests/plan_core.rs`, from `examples/playbooks/expand_east.jsonc` through load → canonicalise → save |
| `expand_east/` | `expected.prose.txt` | the same file's `render_plan` |
| `awkward/` | `expected.jsonc` | the same, from `crates/plan-core/tests/cases/awkward.jsonc` |
| `awkward/` | `expected.canonical.json` | that fixture's canonical JSON, comments stripped |

`expand_east/` has no `expected.canonical.json` of its own on purpose: its
canonical JSON is byte-identical to `tests/golden/proto/expected.expand_east.json`,
and `the_examples_canonical_json_is_the_one_the_proto_lane_pins` asserts that
rather than committing the same bytes twice. If that test goes red, `plan-core`
and `crates/proto` disagree about the canonical form and one of them is wrong —
which is the failure this arrangement exists to catch.

`awkward/` is the case with a comment in every slot the JSONC layer has: before
a member, after one on the same line as its comma, between a key and its colon,
between a colon and its value, at the end of an object, at the end of an array,
inside an empty object, in the file header and after the document. Its fixture's
own header enumerates them. One of its comments sits on a member written at its
proto3 default, which the canonical form drops: that comment is **relocated** to
the nearest surviving ancestor and reported in `Canonical::relocated`, never
dropped, and `a_comment_on_a_written_out_default_is_relocated_and_reported`
pins both halves.

## Licensing

The whole area is `MIT OR Apache-2.0` but one file,
`awkward/expected.jsonc`, which is `GPL-3.0-or-later`: it is the round trip of
plan-core's own `awkward` fixture and carries that fixture's GPL header through
byte for byte, so a permissive record would contradict the file's own header
(`REUSE.toml`'s override block, decisions-log item 116 (6)(i));
`awkward/expected.canonical.json` has no header and stays permissive. `REUSE.toml`'s
`tests/golden/plan-core/**` block puts it in the permissive tier, as
`tests/golden/proto/` is, because these files are the canonical form an
alternative tool has to reproduce (decisions-log item 100 (11)); this README
carries the same header. `expand_east/expected.jsonc` also carries the example's
own permissive header through the round trip, byte for byte, because that is
what the round trip promises. That carried-through header says "CONTRACT FILE:
changes need owner approval" — true of the example it came from, not of this
golden.

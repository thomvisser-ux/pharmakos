<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
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
  coming segment's length today: `Snapshot` does not carry one yet, and
  `crates/plan-core/src/context.rs` says so in a PLACEHOLDER against T10. From
  T10 onwards, when that field exists and `render_plan` reads it, a prose diff
  after a segment ladder change becomes expected and should be stated.

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

`expand_east/expected.jsonc` is **not** a byte reproduction of
`examples/playbooks/expand_east.jsonc` — it is the canonical re-encoding of it,
and only the 25-line header comment block is byte-identical, carried through
verbatim by the round trip because that is what the round trip promises. That
header's own SPDX line reads `MIT OR Apache-2.0`, so the golden asserts the
permissive licence **in its own bytes**, and `REUSE.toml`'s `tests/**` block
carries `precedence = "aggregate"`, under which a file's own header wins.

So one of this area's four files is already in the permissive tier and three are
`GPL-3.0-or-later`. That split is a side effect of the round-trip property, not
a decision this area took: `REUSE.toml`'s PLACEHOLDER against T8 — whether this
whole area joins the permissive tier as `tests/golden/proto/` did — is the
**owner's**, `REUSE.toml` is a contract file (AGENTS.md §5) and T8 did not touch
it. The owner ratifies the split, moves the whole area across, or annotates the
single file; `reuse` is green either way. Note also that the carried-through
header says "CONTRACT FILE: changes need owner approval" — that line is true of
the example it came from and not of this golden.

<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `xtask/tests/data/` — fixtures for the checkers

These are **the checkers' own test data**, not goldens. Goldens live under
`tests/golden/` at the repository root and pin what the *game* produces; these
pin what `xtask` does when handed a good input and a bad one. T3's acceptance is
that the checkers are tested, not just the things they check.

| File | What it is |
| --- | --- |
| `vista-fixture.png` | A 160 x 90 8-bit RGB PNG standing in for a rendered vista until T16 produces a real one. Deliberately not flat: an ash-sky gradient over blocky terrain, so the red-channel variance floor has something to clear. Every one of PNG's five row filters appears in it, so the decoder's unfiltering is exercised rather than assumed. |
| `vista-fixture-doctored.png` | The same image with a 40 x 30 patch replaced. That is 8.3 % of the pixels differing by far more than 32 — well past both thresholds — and it is what a missing chunk, a back-face-culled hole or an inverted winding looks like to a comparator. |
| `vista-fixture-blank.png` | A single-colour frame: red-channel variance 0. A failed render, which the variance floor must catch. |

`xtask/src/png.rs`'s tests read all three. They cannot skip, which is the point:
if the comparator were a script shelled out to, its own test would skip wherever
the interpreter was missing, and a check that silently checks nothing is the
failure spike G1 §10.12 wrote down.

**Do not regenerate these to make a test pass.** They are inputs, not outputs.
If the comparator's verdict on them changes, the comparator changed.

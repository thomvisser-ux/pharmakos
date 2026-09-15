<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `determinism/` — the per-tick state-hash chain

**Filled by T2** (`crates/sim`), extended by every later sim task.

`expected.hashes.txt` is one line per simulated tick, in tick order from 0:

```text
<tick in decimal>    <xxh3-64 state hash as 16 lowercase hex digits>
```

The separator is a TAB. LF endings, a trailing newline, no carriage return
anywhere — `.github/workflows/ci.yml` uploads this file per operating system and
a later job byte-compares the three. That comparison is gate G4 in the flesh:
identical hashes on three operating systems. The format is enforced line by line
by `validate_hash_file` in `xtask/src/main.rs`.

## What a diff means

**The sim's behaviour changed.** That is all it can mean: the chain is a pure
function of (map seed, playbooks, rules hash), so nothing else moves it.

* **You changed behaviour on purpose.** Say so in the pull request — which rule,
  which tick the chain first diverges at, and why. Then `cargo xtask golden
  --bless`. AGENTS.md §5: never re-bless to get a red build to green without
  that explanation.
* **You added a field to sim state.** AGENTS.md §4.8: a new field goes into the
  state hash, the snapshot/restore round trip **and** the goldens in the same
  pull request. A field that affects behaviour and is not hashed is a latent
  desync that only surfaces when two operating systems disagree.
* **It moved on one platform only.** Not a behaviour change — a determinism
  hole. Bisect by tick from the first differing line. The usual suspects are
  unordered iteration, an unchecked cast, a non-total sort comparator, a float
  that crossed the wall, and `usize`/`isize` reaching hashed state.
* **It moved and you changed nothing but a performance knob.** Then the knob is
  inside hashed state and should not be (G3' §9.17's calibration-constant
  lesson). Fix that, not the golden.

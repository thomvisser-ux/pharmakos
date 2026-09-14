<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Pharmakos contributors -->

# G4 — Determinism (spike, wk 1–3)

## 1. Goal and go/no-go criterion

**Goal.** Prove that an integer-only Rust sim, written to the section 15 rules, produces byte-identical per-tick state hashes on Windows, Linux and macOS; that snapshot/restore is hash-transparent; and that `fork`, behind the compile-time `research` feature, is equivalent to stepping the original. If this holds, the per-tick xxh3 hash becomes a cross-OS contract that guards replays, saves and CI for the rest of the project. If it does not, replays are a same-binary artefact and the harness changes shape.

**Go/no-go criterion, copied verbatim from the section 16 gate table:**

> **G4 Determinism** — Spike, wk 1–3 — Go if: *"Identical hashes on 3 operating systems over 10 full matches (toy sim); save/restore round-trips hash-identically; fork equivalence in the research build"* — Fallback: *"Replays guaranteed on the same binary only"*

Three separate assertions, all of which must pass:

| # | Assertion | Pass condition |
|---|---|---|
| G4-a | Cross-OS | The per-tick hash trace of each of 10 toy matches is identical on `ubuntu-latest`, `windows-latest` and `macos-latest`. Identical means every tick's `u64`, in order, and the trace digest over all of them. |
| G4-b | Save/restore | For every match, snapshotting at a set of tick indices, restoring into a fresh process, and running to the end yields a hash trace identical to the uninterrupted run from that tick onward. |
| G4-c | Fork equivalence | In the `research` build, `fork()` at tick *t* followed by *n* steps of the child yields the same hash trace as *n* steps of the parent from *t*, and the parent's own subsequent trace is unaffected by the fork. |

Supporting requirements that come from section 15 and are cheap to assert while we are here: the default build must not contain `fork` at all, and a dependency check must show plan-core/verifier/operator/gateway cannot reach the `research` feature. The spike stands these up in toy form so harness part 1 (wks 3–5) inherits a working shape.

## 2. The toy build

`spikes/g4-determinism/` — a standalone Rust workspace (its own `[workspace]` table so the product workspace never picks it up). Roughly 800–1200 lines total; it is thrown away.

```
spikes/g4-determinism/
  Cargo.toml            # [workspace] (empty table) + [package]; profile.release overflow-checks = true
  src/
    lib.rs              # the toy sim
    fixed.rs            # Q16.16 position, Q32.32 squared distance, u16 angle + 4096-entry sin table
    rng.rs              # split seeded streams
    hash.rs             # canonical encoding -> xxh3
    snapshot.rs         # save/restore, both candidate formats behind features
    fork.rs             # #[cfg(feature = "research")] only
  src/bin/
    run.rs              # run N matches, emit hash trace to stdout/file
    roundtrip.rs        # save/restore round-trip check
    forkcheck.rs        # research-only fork equivalence check
  tests/
    vectors.rs          # xxh3 reference vectors; fixed-point arithmetic edge cases
```

**The toy sim.** Small enough to read in one sitting, large enough that a platform difference has somewhere to hide:

- **State (SoA tables, ordered):** ~200 units × `{ id: u32, seat: u8, pos: Q16.16 ×3, heading: u16, hp: i32, target: Option<u32>, cooldown: u16 }`; ~12 beacons × `{ id, seat, pos, hp, treasury: i64, kw_draw: i32 }`; a per-seat treasury; a kill-credit table of at most 3 `(seat, damage)` counters per asset, exactly as section 15 describes, because the largest-remainder apportionment with ties to the lowest seat id is precisely the kind of thing that goes non-deterministic quietly.
- **Tick (20 Hz, fixed):** acquire target by nearest-enemy using squared distance in Q32.32 (no square roots); turn heading toward the target through the u16 angle table; integrate position in Q16.16; apply integer damage on cooldown; settle $ and kW with integer arithmetic; on death, apportion kill credit by largest remainder with ties to the lowest seat id; draw from the RNG for the small number of places the toy randomises (spawn jitter, a damage roll).
- **Determinism discipline, enforced by hand in the spike and by lints later:** no `f32`/`f64` anywhere; no `as` casts (`TryFrom`/`try_into` with explicit error handling); no `HashMap`/`HashSet` (`BTreeMap`, sorted `Vec`, and `imbl::OrdMap` for the copy-on-write table, so the spike also exercises the crate the product will use); no wall-clock time inside the sim — the benchmark harness that does use `Instant` lives in `src/bin/`, outside the sim module.
- **RNG:** split seeded streams, one stream per concern (`combat`, `spawn`, `map`), each a separate counter-based generator seeded from `(match_seed, stream_id)`. Counter-based (e.g. a small ChaCha or PCG-family construction written out in the spike) rather than shared-state, so that drawing from one stream can never reorder another. Record the construction; harness part 1 makes it a contract.
- **Overflow checks stay on in every profile.** `[profile.release] overflow-checks = true`, `debug-assertions = false`. A wrapping arithmetic difference between platforms is exactly what we are hunting; a panic on overflow is a *result*, not a bug in the test.
- **`fork` is `#[cfg(feature = "research")]`** at the module level, not just the function, and `Cargo.toml` declares `research = []` as a non-default feature.

**A "full match"** in the toy is 8 game-minutes at 20 Hz = 9,600 ticks. Ten matches = 96,000 ticks, ten distinct seeds. That is the same shape as the spec's 8-minute maximum segment, so the trace files stay small (96,000 × 8 bytes ≈ 768 KB of raw hashes, plus the text encoding).

## 3. Measurement procedure

**Step 0 — Prerequisites.** Install the Rust toolchain. On Windows use the **MSVC** ABI host (`stable-x86_64-pc-windows-msvc`), which is what CI's `windows-latest` uses; the GNU host would measure a different toolchain from the one we ship. Pin the toolchain in `spikes/g4-determinism/rust-toolchain.toml` to one exact version (`PLACEHOLDER` — the stable version current at day 0), because "identical across OSes" is only meaningful at a pinned compiler version. Record the version.

**Step 1 — Write the toy and the hash.** The hash input is a *canonical byte encoding* of the state, never raw struct memory: iterate each SoA table in id order, feed each field as fixed-width little-endian bytes into the hasher. Padding, field order in memory and `#[repr]` must be irrelevant by construction. Seed the hasher with one compiled-in constant (`PLACEHOLDER` — fix at day 1 and never change it; changing the seed invalidates every golden file).

**Step 2 — Verify the hash function itself before trusting it.** Run `tests/vectors.rs` against the published XXH3 reference vectors for the empty input, a short input and a >240-byte input, at seed 0 and at our seed. Record pass/fail and the exact crate + version. An XXH3 implementation that disagrees with the reference vectors is a project-ending trap discovered cheaply here.

**Step 3 — Local 10-match run.** `cargo run --release --bin run -- --matches 10 --ticks 9600 --out trace.txt`. Trace format: one line per tick, `match_index<TAB>tick_index<TAB>hash_hex`, written with explicit `\n` (never `println!` through a mode that could translate line endings — open the file in binary mode and write bytes). At the end, write a single **trace digest**: xxh3 over the whole trace file's bytes. Record:

- per-match final-tick hash (10 values);
- the trace digest (1 value);
- ticks/s and total wall time (context for G3′, not a gate here);
- peak RSS.

**Step 4 — Determinism within one machine.** Run the same command three times and confirm the trace digest is identical each time. This catches accidental nondeterminism (address-dependent iteration, uninitialised memory, time) before we blame a platform. Then run once under a different thread count / with the process pinned to a different core, and once with `--release` replaced by a debug build, and record whether the digest holds. *Debug and release traces must match*: if they diverge, something depends on overflow-check behaviour or on `debug_assert`-guarded code, and that must be fixed, not tolerated.

**Step 5 — Cross-OS.** Run the same binary build on all three OSes via CI (section 5 below) and compare the ten final hashes and the trace digest. On a mismatch, bisect by tick: the CI compare job reports the first differing `(match, tick)` and dumps both sides' state at `tick-1` and `tick`, so the culprit field is named rather than guessed.

**Step 6 — Save/restore round trip.** `cargo run --release --bin roundtrip`. For each of the 10 matches, snapshot at ticks {0, 1, 137, 4800, 9599} (a first tick, an odd non-aligned tick, the midpoint, the last tick). For each snapshot: write to disk, exit the process, start a fresh process, restore, run to tick 9600, and compare the tail of the trace against the uninterrupted run. Record: round trips attempted / passed, snapshot bytes at each tick, save ms, restore ms. Also record the **snapshot file's own hash** on each OS — a restore that works but writes platform-different bytes is still a failure for shared saves, and the spec's saves are stamped with a rules hash, so byte stability matters.

**Step 7 — Both snapshot formats.** Build the round trip with `--features snap-rkyv` and again with `--features snap-postcard`. Record for each: snapshot bytes, save ms, restore ms, cross-OS byte identity of the snapshot file, and any configuration needed to make it endian- and width-neutral (see risks). This produces the spike's snapshot-format decision.

**Step 8 — Fork equivalence.** `cargo run --release --features research --bin forkcheck`. For each match, at ticks {100, 5000}, fork, step the child 200 ticks, and compare the child's trace to the parent's trace over those same ticks; then continue the parent to 9600 and confirm the parent's full trace digest equals the non-forked run's. Record: forks attempted / equivalent, fork cost in ms and bytes, and whether the parent's trace was perturbed (it must not be).

**Step 9 — Feature isolation.** Confirm `cargo build --release` (no features) produces a binary with no `fork` symbol (`nm`/`dumpbin` grep, recorded) and that `cargo tree -e features` shows nothing reaching `research`. Write the toy equivalent of the CI dependency check: a script that fails if any crate named in a deny-list (here, stand-ins for plan-core, verifier, operator, gateway) resolves the `research` feature. Record the script; it graduates into `cargo xtask ci` in harness part 1.

**Step 10 — Write the results** into section 9 below and the decision into `docs/design/decisions-log.md`.

### Numbers to record (the results table)

| Number | Unit | Why |
|---|---|---|
| Per-match final hash × 10, per OS | hex u64 | G4-a |
| Trace digest, per OS | hex u64 | G4-a |
| First differing (match, tick), if any | — | diagnosis |
| Round trips passed / attempted | count | G4-b |
| Snapshot size at tick 4800, per format | bytes | format decision |
| Save / restore time, per format | ms | format decision |
| Snapshot byte identity across OSes, per format | yes/no | saves are shared artefacts |
| Fork equivalences passed / attempted | count | G4-c |
| Fork cost | ms, bytes | is `fork` affordable in research runs |
| Debug vs release trace identity | yes/no | hidden dependence on checks |
| Ticks/s, peak RSS | — | context for G3′ |
| Toolchain, crate versions, machine spec | — | a number without these is not a result |

## 4. CI hook

A workflow `.github/workflows/spike-g4.yml` in the spike worktree (it graduates into the determinism CI of harness part 1). Sketch — paths and action versions are `PLACEHOLDER` until the toolchain exists:

```yaml
name: spike-g4-determinism
on: [push, workflow_dispatch]
jobs:
  trace:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    defaults: { run: { working-directory: spikes/g4-determinism } }
    steps:
      - uses: actions/checkout@PLACEHOLDER
        with: { fetch-depth: 1 }
      # .gitattributes must mark trace/golden files binary so Windows checkout
      # never rewrites LF to CRLF.
      - run: rustup show                       # honours rust-toolchain.toml
      - run: cargo test --release              # includes the xxh3 reference vectors
      - run: cargo run --release --bin run -- --matches 10 --ticks 9600 --out trace-${{ matrix.os }}.txt
      - run: cargo run --release --bin roundtrip --features snap-postcard
      - run: cargo run --release --bin roundtrip --features snap-rkyv
      - run: cargo build --release             # default build: must not contain fork
      - run: cargo run --release --features research --bin forkcheck
      - uses: actions/upload-artifact@PLACEHOLDER
        with: { name: "trace-${{ matrix.os }}", path: "spikes/g4-determinism/trace-${{ matrix.os }}.txt" }
  compare:
    needs: trace
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@PLACEHOLDER
      # Byte-compare the three traces; on mismatch print the first differing line
      # with 3 lines of context from each side, then exit 1.
      - run: |
          cmp trace-ubuntu-latest/*.txt trace-windows-latest/*.txt
          cmp trace-ubuntu-latest/*.txt trace-macos-latest/*.txt
```

Two syntax notes on that sketch, because a workflow GitHub rejects runs nothing at all:

- Inside a **flow mapping** (`with: { … }`) the braces of `${{ … }}` are YAML indicators and
  terminate the mapping, so any scalar containing an expression must be quoted — hence
  `name: "trace-${{ matrix.os }}"` above. On its own line (`runs-on: ${{ matrix.os }}`) it is fine.
  `.github/workflows/ci.yml` sidesteps the question by using block style for `with:`; either is
  acceptable, unquoted flow style is not.
- The spike's trace format is **throwaway and does not graduate**. What graduates into harness
  part 1 is the two-column chain that `xtask/src/main.rs` (`HASH_FILE`, `validate_hash_file`) and
  `.github/workflows/ci.yml` already agree on:

  | | spike G4 (here) | harness part 1 (the contract) |
  |---|---|---|
  | path | `spikes/g4-determinism/trace-<os>.txt` | `target/determinism/hashes.txt` |
  | line | `match_index<TAB>tick_index<TAB>hash_hex` | `<tick in decimal><TAB><16 lowercase hex digits>` |
  | ticks | per match, 10 matches × 9,600 | one run, from 0 upward, `DETERMINISM_TICKS` long |
  | artefact | `trace-<os>` | `determinism-hashes-<os>` |
  | golden | the trace digest, recorded below | `tests/golden/determinism/expected.hashes.txt` |

  `validate_hash_file` rejects the three-column form outright, and that is correct: the spike needs
  a match index because it runs ten matches, and the harness's per-pull-request check runs one.
  Do not "fix" either side to match the other — record the ten per-match final hashes and the trace
  digest here, and let the harness keep its own format. Two numbers also differ on purpose:
  `DETERMINISM_TICKS` is 1,200 (one minute at 20 Hz, a smoke run on every pull request, and itself a
  PLACEHOLDER in that file), against G4's bar of 10 matches × 9,600 ticks. Raising the pull-request
  run to G4's bar would put a 96,000-tick run in front of every push; the long run belongs in a
  nightly workflow, and the decision of where it lands is the owner's at harness part 1.

What the hook asserts, mapped to the criterion:

- **identical per-tick xxh3 hashes across ubuntu/windows/macos over 10 toy matches** — the `trace` matrix plus the `compare` job's byte comparison of the three trace files;
- **save/restore round trip hash-identical** — the two `roundtrip` invocations, one per candidate format, each of which exits non-zero on any mismatch;
- **fork equivalence in the research build** — `forkcheck` under `--features research`, alongside a default `cargo build --release` that must produce a binary with no fork symbol.

Two CI details that matter more than they look. First, `.gitattributes` must mark trace and golden files `-text` so Git on Windows never rewrites line endings under us — not `*.txt` wholesale, which would also catch ordinary text, but the four patterns the file now carries: `tests/golden/**`, `*.hashes.txt`, `spikes/**/trace*.txt` and `*.trace.txt`. A trace force-added under `spikes/.gitignore`'s "minimal reproducing excerpt" rule lands under the third of those. Second, disable any incremental/sccache reuse across OSes so a cached artefact cannot mask a real difference.

## 5. Platform risks: Windows/MSVC vs Linux (and macOS)

Floats are the classic cross-platform hazard and they are *absent by construction* here — the lint set forbids float arithmetic outside a walled presentation/solve module, and the toy has no such module. That removes x87/SSE, FMA contraction and `libm` differences from the table. What remains is the list below; each one is checked in the procedure above rather than assumed away.

| Risk | Why it differs | How this spike handles it |
|---|---|---|
| **Integer overflow semantics** | Rust's integer ops are defined identically everywhere, but *whether a build checks them* is a profile setting, and a release build with checks off silently wraps where a debug build panics. A wrap that happens on one machine's timing-dependent path and not another's is the nightmare case. | `overflow-checks = true` in every profile, including release, as the spec requires. Step 4 asserts the debug and release traces are identical. Any overflow panic is recorded as a finding, and the arithmetic is widened (i64/i128 intermediates) rather than the check disabled. |
| **Shift and division edge cases** | `i32::MIN / -1` and shifts ≥ bit width panic (checked) or are UB-adjacent (unchecked). Truncation toward zero on division and the sign of `%` are Rust-defined and do *not* vary by platform — but Q16.16 right-shifts on negative values are arithmetic shifts and must be written as such. | Fixed-point helpers in `fixed.rs` are the only place shifts appear; unit tests cover negative operands, `MIN`, and the rounding direction of every helper. |
| **Sort stability and comparator totality** | `sort_unstable_by` gives no guarantee about equal elements, and the pattern-defeating quicksort's behaviour depends on the standard library version, not the OS — so a toolchain skew across runners produces a real difference. | Every sort key includes a unique id as its final tiebreak, making the order total; then stable and unstable sorts agree and the std version cannot matter. The toolchain is pinned identically on all three runners. |
| **Iteration order** | `HashMap`/`HashSet` are randomly seeded per process and are already forbidden by the lint set — but the lint does not exist yet in wk 1, so the spike must not reach for them by habit. Also: `fs::read_dir` order differs (ext4 vs NTFS vs APFS), and `glob` results differ. | `BTreeMap` / sorted `Vec` / `imbl::OrdMap` only. Any directory listing in the harness is sorted before use. |
| **rkyv endianness and pointer width** | rkyv's default archive uses native endianness and native pointer width, so an archive written on one target may be unreadable — or silently mis-read — on another. Relative pointers embed widths. | Build rkyv with the explicit little-endian archive and a fixed pointer width, and never archive `usize`/`isize`. Step 7 compares snapshot bytes across the three OSes; a format that is not byte-identical loses the decision. |
| **postcard varint width** | postcard is little-endian and platform-neutral by design, but `usize`/`isize` are encoded as varints of the host's width, so a 32- vs 64-bit target diverges. | Never serialise `usize`/`isize`; every serialised field is a fixed-width type (`u32`, `i64`, …). All three runners are 64-bit, but the rule holds anyway because a 32-bit web build is on the roadmap. |
| **xxh3 seed and variant** | XXH3's *output* is endian-neutral by specification, so the risk is not the CPU — it is the implementation: crates disagree on default seed/secret, some expose both the pre-final and final XXH3 streams, and a crate upgrade can change the digest. | Pin the crate version exactly; assert the published reference vectors in `tests/vectors.rs` (step 2); compile the seed in as one named constant; never hash raw memory, only the canonical encoding. |
| **`usize` in hashed state** | 64-bit on all three runners today, but lengths and indices leaking into the hash make the trace target-dependent for no benefit. | Hashed state uses fixed-width types only; lengths are hashed as `u32` after a checked conversion. |
| **Line endings and path handling** | Git on Windows rewrites LF→CRLF on checkout by default, which corrupts a golden trace file into a different digest. Paths differ in separator and case sensitivity (NTFS/APFS case-insensitive, ext4 not). | `.gitattributes` marks traces and goldens `binary`; the harness writes files in binary mode with explicit `\n`; golden filenames are lowercase ASCII with no characters that need quoting. |
| **Thread count and scheduling** | If any part of the sim were parallel, work-stealing order would leak into results. | The toy sim is single-threaded, full stop. Parallelism, if it ever arrives, has to come with its own determinism proof. |
| **Stack depth** | MSVC threads default to ~1 MB of stack versus ~8 MB on Linux, so deep recursion can overflow on Windows only. | No recursion in the sim; explicit work stacks. (This bites harder in G2 — see that plan.) |
| **macOS as a third OS** | macOS is a signed CI artefact only (section 15), on Apple silicon runners — a different architecture, not just a different OS. It is the most likely single source of an ARM-vs-x86 difference, and also the most valuable: an ARM agreement is strong evidence the state is genuinely portable. | Included in the matrix and in the criterion, as the gate says "3 operating systems". If macOS alone disagrees, record it as an architecture finding and treat the fallback decision separately for that platform. |

## 6. Fallback if it fails

The spec's fallback is **"Replays guaranteed on the same binary only"**. Expanded into the edits it implies:

- The deterministic replay (seed + playbooks + log) remains exactly what section 15 says it is — private, in the host's match cache, serving scrub, recap and bug reports — but its guarantee is scoped to the binary that produced it. Every replay and save gains a **build id** alongside the existing rules hash and verifier version; a mismatch refuses to load, the same way a rules-hash mismatch already does.
- The cross-OS CI job becomes **advisory**: it still runs and still reports, because a newly introduced difference is worth knowing about, but it does not block a merge. The blocking determinism job becomes single-OS (Linux) plus the save/restore and fork checks.
- Bug reports from testers must carry the build id, and "reproduce the tester's replay on the developer's machine" stops being reliable across OSes — so the playtest bundle gains the build id and the diagnostics log grows a platform stanza.
- Shared recordings (keyframes plus state deltas and events) are **unaffected**: they carry no playbooks and re-run no sim, so the watch and share stage (S7) does not change.
- Partial failure is likelier than total failure, and the fallback is applied at the granularity of what broke. If the only divergence is macOS/ARM, keep the cross-OS contract for Windows and Linux (the two first-class platforms) and drop macOS to advisory. If the only divergence is a snapshot's *bytes* while its restore is hash-transparent, keep the hash contract and make saves same-platform.
- If G4-c (fork) alone fails, nothing player-facing changes: fork exists only in the research build, so the fallback is to fix or drop research forking, and the "no dry runs" guarantee is untouched.

## 7. Estimate

**4 days**, in worktree A during week 1 of the 3-week budget.

| Day | Work |
|---|---|
| 0 (shared) | Toolchain install, pinned `rust-toolchain.toml`, repo skeleton |
| 1 | Fixed-point + angle table + RNG streams + canonical hash encoding; xxh3 reference vectors |
| 2 | Toy sim tick, SoA tables, kill-credit apportionment; local 10-match run; within-machine repeatability |
| 3 | Snapshot/restore in both formats; round-trip harness; feature isolation and the dependency-check script |
| 4 | `fork` behind `research`; CI workflow and the three-runner comparison; results write-up |

Owner review is needed once, at the end of day 2, on the hash input encoding — it is a contract and everything downstream is stamped with it.

## 8. The decision this spike must produce

> **Is the per-tick xxh3 hash a cross-OS contract or a same-binary one?** — and, with it, the four frozen choices that make the answer true: (1) the canonical byte encoding fed to the hasher and its compiled-in seed; (2) the snapshot format, rkyv or postcard, with the exact configuration that makes its bytes endian- and width-neutral; (3) the split-RNG stream construction and the stream ids; (4) the determinism rule set that harness part 1 turns into lints — no floats, no `as` casts, no `HashMap`/`HashSet`, no wall-clock time in sim crates, overflow checks on in every profile, total sort comparators, fixed-width types in hashed state.

Recorded in `docs/design/decisions-log.md` §2.7. Items (1)–(4) are contract files and need owner approval before harness part 1 encodes them.

## 9. Results

*(Empty until the spike runs. Fill in before the `spike-end` tag; a number without its machine and toolchain does not count.)*

- Machine: `PLACEHOLDER` (CPU, cores, RAM, OS build)
- Runner images: `PLACEHOLDER`
- Rust: `PLACEHOLDER` · xxh3 crate: `PLACEHOLDER` · rkyv: `PLACEHOLDER` · postcard: `PLACEHOLDER` · imbl: `PLACEHOLDER`
- G4-a cross-OS: `PLACEHOLDER`
- G4-b save/restore: `PLACEHOLDER`
- G4-c fork equivalence: `PLACEHOLDER`
- Verdict: `PLACEHOLDER` (go / fallback, and which part of the fallback was taken)

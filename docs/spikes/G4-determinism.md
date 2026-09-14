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
  Cargo.lock            # MUST be committed — see section 4
  rust-toolchain.toml   # the pinned compiler
  clippy.toml           # spike-local; shadows the product's root clippy.toml — see section 4
  check-features.sh     # step 9, feature isolation, with positive controls
  ci/spike-g4.yml       # the CI workflow, staged here; copy to .github/workflows/
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
- **Four seats, not three** — a correction made while writing the toy, and the reason is the line above. An asset belongs to one seat and there is no friendly fire, so with three seats an asset has only *two* possible damagers, a credit list can never hold more than two entries, and the `MAX_CREDITS == 3` cap is unreachable by construction: the three-way largest-remainder apportionment the plan singles out would never once run in the trace. With four seats it does — 25 of the 14,185 settlements over the ten matches apportion a bounty over three seats, and `tests/vectors.rs` asserts that the widest credit list observed equals `MAX_CREDITS` so the coverage cannot silently regress if the sim is retuned. A *fourth* distinct damager still cannot occur, so `Credits::record`'s eviction branch remains unreachable from the sim and is pinned by a direct unit test instead (including its damage-tie rule, which originally evicted the lowest seat id — the opposite of "ties to the lowest seat id" as the phrase is used everywhere else).
- **Tick (20 Hz, fixed):** acquire target by nearest-enemy using squared distance in Q32.32 (no square roots); turn heading toward the target through the u16 angle table; integrate position in Q16.16; apply integer damage on cooldown; settle $ and kW with integer arithmetic; on death, apportion kill credit by largest remainder with ties to the lowest seat id; draw from the RNG for the small number of places the toy randomises (spawn jitter, a damage roll).
- **Determinism discipline, enforced by hand in the spike and by lints later:** no `f32`/`f64` anywhere; no `as` casts (`TryFrom`/`try_into` with explicit error handling); no `HashMap`/`HashSet` (`BTreeMap`, sorted `Vec`, and `imbl::OrdMap` for the copy-on-write table, so the spike also exercises the crate the product will use); no wall-clock time inside the sim — the benchmark harness that does use `Instant` lives in `src/bin/`, outside the sim module.
- **RNG:** split seeded streams, one stream per concern (`combat`, `spawn`, `map`), each a separate counter-based generator seeded from `(match_seed, stream_id)`. Counter-based (e.g. a small ChaCha or PCG-family construction written out in the spike) rather than shared-state, so that drawing from one stream can never reorder another. Record the construction; harness part 1 makes it a contract.
- **Overflow checks stay on in every profile.** `[profile.release] overflow-checks = true`, `debug-assertions = false`. A wrapping arithmetic difference between platforms is exactly what we are hunting; a panic on overflow is a *result*, not a bug in the test.
- **`fork` is `#[cfg(feature = "research")]`** at the module level, not just the function, and `Cargo.toml` declares `research = []` as a non-default feature.

**A "full match"** in the toy is 8 game-minutes at 20 Hz = 9,600 ticks. Ten matches = 96,000 ticks, ten distinct seeds. That is the same shape as the spec's 8-minute maximum segment, so the trace files stay small (96,000 × 8 bytes ≈ 768 KB of raw hashes, plus the text encoding).

## 3. Measurement procedure

**Step 0 — Prerequisites.** Install the Rust toolchain. On Windows use the **MSVC** ABI host (`stable-x86_64-pc-windows-msvc`), which is what CI's `windows-latest` uses; the GNU host would measure a different toolchain from the one we ship. Pin the toolchain in `spikes/g4-determinism/rust-toolchain.toml` to one exact version (`1.98.1`, the stable version current at day 0), because "identical across OSes" is only meaningful at a pinned compiler version. Record the version.

**Step 1 — Write the toy and the hash.** The hash input is a *canonical byte encoding* of the state, never raw struct memory: iterate each SoA table in id order, feed each field as fixed-width little-endian bytes into the hasher. Padding, field order in memory and `#[repr]` must be irrelevant by construction. Seed the hasher with one compiled-in constant (`STATE_HASH_SEED = 0x5048_4152_4D4B_4F53`, ASCII "PHARMKOS", fixed at day 1 and never changed; changing the seed invalidates every golden file).

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

A workflow `.github/workflows/spike-g4.yml` in the spike worktree (it graduates into the determinism CI of harness part 1).

**Status: installed as `.github/workflows/spike-g4.yml` on 2026-09-13; the first three-runner result is recorded in section 9 once it lands.** The workflow was drafted at `spikes/g4-determinism/ci/spike-g4.yml` (kept there as the spike's own copy) and installed with one copy:

```sh
cp spikes/g4-determinism/ci/spike-g4.yml .github/workflows/spike-g4.yml
```

**Two preconditions must be satisfied before that copy is worth making**, or the job produces an uncontrolled answer the moment it lands:

1. **`spikes/g4-determinism/Cargo.lock` must be committed.** `spikes/.gitignore` line 10 is a bare `Cargo.lock`, which ignores it (`git check-ignore -v` confirms). Only the five direct dependencies carry `=` pins; rkyv 0.8.18 itself depends on `rend "0.5"`, `bytecheck "0.8"`, `munge "0.4"`, `ptr_meta "0.3"` and `rancor "0.1"` as caret ranges, and `rend`'s endian-aware scalar types are literally what the archive bytes are made of. Without a committed lock the three runners each resolve their own graph and step 7's cross-OS snapshot-byte comparison is not a controlled experiment — a patch-level difference on one runner would be misdiagnosed as a platform difference, which is exactly the failure step 0 pins the compiler against, one layer down. Fix: add a negation `!*/Cargo.lock` under the `Cargo.lock` line in `spikes/.gitignore` and `git add -f spikes/g4-determinism/Cargo.lock`. Every cargo invocation in the staged workflow already passes `--locked`, so a drifted graph fails the build instead of silently changing the bytes. *(Outside the spike directory; not done.)*
2. **`.gitattributes` must keep trace files out of Git's line-ending rewriter.** Already satisfied — the file carries all four patterns (`tests/golden/**`, `*.hashes.txt`, `spikes/**/trace*.txt`, `*.trace.txt`). If a snapshot hash list is ever force-added as a finding, add `spikes/**/snapshots-*.txt -text` alongside them.

What the staged workflow does, beyond the original sketch:

- `rustup show` plus `rustc -vV` and `cargo -vV`, so the pinned version is in every job's log rather than assumed;
- **three** test invocations, not one. `cargo test --release --locked` runs 27 tests; `snapshot_bytes_round_trip` and `fork_is_equivalent_to_stepping` are `#[cfg]`-gated and do **not exist** in that build. They are the only tests asserting that `encode` is a pure function of the value (`assert_eq!(bytes, encode(&snap))` — the byte-stability property the whole cross-OS snapshot comparison rests on) and that a fork does not perturb the parent in-process, so the workflow adds `--features snap-postcard,research` and `--features snap-rkyv,research` (29 tests each). A test that passes by not being compiled is not a test. The two snapshot features are **mutually exclusive by `compile_error!`** — they used to be additive with rkyv winning the `cfg` race, so `--all-features` measured rkyv twice while reporting postcard as covered;
- each `roundtrip` writes `snapshots-<format>-<os>.txt`, one `snapshot<TAB>match<TAB>tick<TAB>bytes<TAB>file_hash` line for **every** checkpoint of every match (50 lines), not five values from seed 1. Without the whole list a format whose bytes are stable for one seed and unstable for a seed with more credit entries would pass the cross-OS comparison unnoticed;
- `sh ./check-features.sh` (step 9) runs in CI rather than by hand;
- the `compare` job `cmp`s **six pairs** — trace, `snapshots-postcard`, `snapshots-rkyv`, each ubuntu-vs-windows and ubuntu-vs-macos — and on a mismatch prints the first differing line with three lines of context from each side before failing, so the culprit `(match, tick)` is named rather than guessed. It does not `fail-fast`, so a macOS-only divergence is visible as exactly that: the ARM leg disagreeing while Windows and Linux agree, which section 6 treats as an architecture finding with its own fallback;
- `CARGO_INCREMENTAL: 0` and an empty `RUSTC_WRAPPER`, with no cache action anywhere, so no cached artefact can mask a real difference.

One more CI detail found while running the spike locally, worth carrying into harness part 1. **Clippy walks up from the current directory to find `clippy.toml`,** so running clippy from `spikes/g4-determinism` picked up the *product's* root contract file even though this directory is a standalone workspace the product build never sees. The result was that the stated command

```sh
cargo clippy --release --all-targets --features research,snap-rkyv -- -D warnings
```

failed with 9 `disallowed_types` and 6 `disallowed_methods` errors on `std::time::Instant` in the three benchmark binaries — which section 2 of this plan explicitly sanctions and `spikes/README.md` rule 1 explicitly exempts — plus an MSRV-mismatch warning. The spike now carries its own `clippy.toml` (nearest ancestor wins, and clippy does not merge) with empty `disallowed-types`/`disallowed-methods` and `msrv = "1.98.1"`; the discipline the sim actually needs is `#![deny(clippy::as_conversions, clippy::float_arithmetic, clippy::disallowed_types)]` in `src/lib.rs`, where it travels with the code instead of with the working directory. **Harness part 1's `cargo xtask clippy` should pass an explicit `--config-path`**, because the same ancestor walk hits any out-of-workspace directory a developer happens to run clippy from.

Two syntax notes on the workflow, because a workflow GitHub rejects runs nothing at all:

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
| **postcard varint width** | postcard is little-endian and platform-neutral by design, but `usize`/`isize` are encoded as varints of the host's width, so a 32- vs 64-bit target diverges. | Never serialise `usize`/`isize`; every serialised **field** is a fixed-width type (`u32`, `i64`, …). **One caveat, stated exactly rather than waved away:** every `Vec<T>` in the snapshot still hands the format a sequence length, which is a `usize` supplied by the serialiser rather than a field of ours. rkyv writes it at the pinned 32-bit pointer width, so it is the same bytes on a 32- and a 64-bit host. postcard writes it as a canonical LEB128 varint with no width padding, so the bytes agree between 32- and 64-bit hosts for every length below 2^32 — a *value* identity, not a type-level one. Every snapshot this toy produces is orders of magnitude inside that, and the 32-bit web build on the roadmap stays inside it too, but the distinction belongs in the snapshot-format decision (§2.7 of the decisions log) rather than in a module comment claiming "no `usize`". All three runners are 64-bit today. |
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

> **Verdict: PROVISIONAL.** Measured end to end on **one host, one OS, one
> architecture** (Windows 10 Pro 10.0.19045 / MSVC / x86-64) on 2026-09-13.
> **G4-b passes 50/50 in both candidate formats and G4-c passes 20/20 with
> 10/10 parents unperturbed.** **G4-a is pending CI**: the Windows leg is
> measured and pinned (trace digest `340a30048a380595` over 10 matches ×
> 9,600 ticks; three consecutive release runs, a core-pinned run and a debug
> build all byte-identical), but the Linux and macOS legs have never run,
> because `.github/workflows/spike-g4.yml` does not exist. A single-OS pass is
> consistent with *both* the "go" and the fallback ("replays guaranteed on the
> same binary only"), so neither may be recorded yet. Step 5 of section 3 is
> the only step of the procedure not run here, and it cannot be: it needs the
> three-runner matrix, which needs the repository on GitHub with the workflow
> installed and `Cargo.lock` committed (section 4).

### Machine and toolchain

| | |
|---|---|
| Machine | Intel Core i7-9800X CPU @ 3.80 GHz, 8 cores / 16 threads; 34,045,517,824 B RAM (31.7 GiB) |
| OS | Windows 10 Pro, version 10.0.19045, x86-64 |
| Rust | `rustc 1.98.1 (48a229cea 2026-09-01)`, host `x86_64-pc-windows-msvc`, LLVM 22.1.8; `cargo 1.98.1 (797e8a9bc 2026-08-05)` — pinned in `rust-toolchain.toml` (`channel = "1.98.1"`, profile minimal, components rustfmt + clippy) |
| Runner images | `pending CI (step 5)` — the matrix has not run; see section 4 |
| Direct crates (`=` pinned) | xxhash-rust 0.8.18 (`xxh3`, no default features) · imbl 7.0.2 · rkyv 0.8.18 (`std`, `bytecheck`, `little_endian`, `pointer_width_32`, `aligned`) · postcard 1.1.3 (`use-std`) · serde 1.0.229 (`std`, `derive`) |
| Transitive graph (from `Cargo.lock`, 56 packages including the root) | rancor 0.1.3, munge 0.4.7 / munge_macro 0.4.7, rend 0.5.4, bytecheck 0.8.3 / bytecheck_derive 0.8.3, ptr_meta 0.3.2 / ptr_meta_derive 0.3.2, simdutf8 0.1.5, hashbrown 0.17.1, indexmap 2.14.2, tinyvec 1.13.3, bytes 1.12.1, uuid 1.26.1, archery 1.2.3, imbl-sized-chunks 0.2.0, wide 0.7.33, safe_arch 0.7.4, bytemuck 1.25.2, rand_core 0.9.5, rand_xoshiro 0.7.0, equivalent 1.0.2, version_check 0.9.5, cobs 0.3.0, embedded-io 0.4.0 + 0.6.1, thiserror 2.0.20, serde_core / serde_derive 1.0.229, syn 2.0.119 + 3.0.5, … — **caret ranges, not pinned.** This is why the lock must be committed before the matrix runs (section 4). |
| Profiles | `overflow-checks = true` in dev/release/test/bench; `debug-assertions = false` in release |
| Build | every cargo invocation passed `--locked`; `CARGO_TARGET_DIR=D:\build\cargo-target` on this host |

### Step 2 — the hash function itself

**PASS.** 26 upstream XXH3-64 reference vectors from `xsum_sanity_check.c`
(lengths 0, 1, 6, 12, 24, 48, 80, 195, 403, 512, 2048, 2240, 2367, each at seed
0 and at `PRIME64`) all pass against **xxhash-rust 0.8.18**.
`XXH3_64bits("") == 0x2D06800538D394C2` is asserted separately as the anchor,
and `xxh3_unseeded_matches_seed_zero` checks that the unseeded entry point
agrees with seed 0 at every one of those lengths.

**Frozen choice (1) — the encoding and its seed.**
`STATE_HASH_SEED = 0x5048_4152_4D4B_4F53` (`b"PHARMKOS"` big-endian).
`ENCODING_VERSION = 1`, pushed as the first byte by every `Enc` construction
path including a hand-written `Default`. Canonical encoding length at tick 0:
**6,497 bytes** = 1 version + 8 seed + 4 tick + 4 deaths + 4 len + 200×30 units
+ 4 len + 12×33 beacons + 4 len + 4×17 seats + 4 len (credits, empty at tick 0).
Pinned in `canonical_encoding_is_stable_and_fixed_stride`. Regression pins:
`digest(b"") == 0xFE1AF732B02810AD`,
`digest(b"pharmakos/g4") == 0xE17026C8A5EA4D30`.

**Frozen choice (3) — the split-RNG streams.** Literal golden pins
(`rng_construction_is_pinned`), not self-comparisons: `mix64(1) ==
0x5692161D100B05E5`, `mix64(GOLDEN) == 0xE220A8397B1DCDAF`,
`StreamRng::new(0x0102030405060708, Combat, 10, 1, 7)` draws
`0xCDC65EE918292C78` then `0x100552F8977C9F98`, the `Spawn` stream at the same
position draws `0x0A849AA1ED23D46C`, the all-zero position draws
`0x1957A7604E215178`, and `range_i32(-3, 3)` yields
`[2, -3, 3, -2, 0, -2, -1, 2]`. Stream ids: `Combat = 1`, `Spawn = 2`,
`Map = 3`.

### Step 3 — the ten-match run (Windows only)

`cargo run --release --locked --bin run -- --matches 10 --ticks 9600 --out trace.txt`

| Match | Seed | Final-tick hash |
|---|---|---|
| 0 | `0000000000000001` | `9a2f54f752039672` |
| 1 | `123456789abcdef0` | `c245e6e357f93fa3` |
| 2 | `deadbeefcafef00d` | `448e783fb5ac58a5` |
| 3 | `0f1e2d3c4b5a6978` | `792098c7679c6ea5` |
| 4 | `a5a5a5a55a5a5a5a` | `151da55f83db915a` |
| 5 | `00000000dead0001` | `59fa18b3c27ba44a` |
| 6 | `ffffffffffffffff` | `b5f223cdb4da6f5a` |
| 7 | `7fffffffffffffff` | `a6c1d8e9ddbfb628` |
| 8 | `8000000000000000` | `0672719d2cd06af9` |
| 9 | `0102030405060708` | `33edd3145f604e4a` |

- **Trace digest (Windows): `340a30048a380595`** — xxh3-64 at the project seed
  over the 2,292,900 bytes of the 96,000 tick lines. The file on disk is
  **2,292,924 bytes** because `run` appends its own 24-byte `digest<TAB>…<LF>`
  line *after* computing the digest.
- Trace: 96,000 tick lines plus one digest line = 96,001 lines, **0 CR bytes** —
  the binary-mode write holds on Windows, so Git's line-ending rewriter is the
  only remaining way a CRLF could enter (handled by `.gitattributes`, section 4).
- Asserted in-tree, not only recorded here: `ten_match_trace_digest_is_pinned`
  (`cargo test --release --locked -- --ignored`) rebuilds the same byte string
  and pins both its length and its digest. **Passes.**
- **Throughput: 60,955–66,377 ticks/s** release over four 96,000-tick runs
  (1.446–1.574 s wall); **3,132 ticks/s** debug (30.65 s).
- **Peak RSS: 6,094,848–6,119,424 B (~5.8 MiB)** release, 6,328,320 B debug,
  printed by `run` itself (`peak_rss_bytes`) from `K32GetProcessMemoryInfo` on
  Windows, `/proc/self/status` `VmHWM` on Linux and `getrusage` on macOS, so CI
  can reproduce and regress on it.
- Workload sanity (seed 0, re-measured): 1,439 deaths, first at tick 265, kills
  `[319, 412, 365, 451]`, cash `[322179, 798846, 695685, 877162]`, 6 of 12
  beacons destroyed, 135 of 200 units alive at the end, 61 live credit entries;
  widest credit list observed = 3 = `MAX_CREDITS`, in 2 observations, so the cap
  the plan singles out is genuinely exercised.

### Step 4 — determinism within one machine

| Check | Result |
|---|---|
| Three consecutive release runs | **identical** — digest `340a30048a380595` each time, all three files byte-identical under `cmp` |
| Release run pinned to a single core (affinity mask `0x8`) | **identical** — same digest, `cmp` clean against the baseline |
| Debug vs release trace | **identical** — `cmp` clean, same digest. No dependence on `debug_assert` or on overflow-check behaviour |

The sim is single-threaded, so "a different thread count" has no dial to turn;
the core-pinned run is the meaningful form of that check on this machine.

### Step 5 — cross-OS

**`pending CI — deferred until the repository is on GitHub`.** This step cannot
be run on this machine: it compares three runners against each other and there
is only one here. No comparison exists, so **no first differing `(match, tick)`
can be reported** — the diagnosis row of the numbers table is
`n/a (no cross-OS comparison yet)`. Section 4 lists the two preconditions
(install `.github/workflows/spike-g4.yml`; commit
`spikes/g4-determinism/Cargo.lock`), both of which are outside the spike
directory.

### Steps 6–7 — save/restore, both formats

`roundtrip` snapshots at ticks {0, 1, 137, 4800, 9599} of each of the 10
matches, writes each to disk, restores it in a **fresh process** (the binary
re-invokes itself with `--restore`), runs to tick 9,600 and byte-compares the
tail against the uninterrupted trace.

| | postcard 1.1.3 | rkyv 0.8.18 |
|---|---|---|
| Round trips passed | **50 / 50** | **50 / 50** |
| Snapshot bytes at tick 4800 (min / mean / max over the 10 seeds) | 4,438 / **4,563** / 4,639 | 6,976 / **7,160** / 7,344 |
| Snapshot bytes at tick 0 | 4,930–4,954 | 6,464 (constant) |
| Save, mean over 50 captures (capture + encode) | **41 µs = 0.041 ms** (41 and 41 over two runs) | **27–31 µs = 0.027–0.031 ms** |
| Restore, mean over 50 (child-reported: read + decode + rebuild) | **240–321 µs = 0.24–0.32 ms** | **275–295 µs = 0.28–0.30 ms** |
| Snapshot hash list reproducible run-to-run on this host | **yes** — `snapshots-postcard.txt` byte-identical across two runs | **yes** — `snapshots-rkyv.txt` byte-identical across two runs |
| Cross-OS byte identity | **`pending CI (step 5)`** | **`pending CI (step 5)`** |

Per-checkpoint snapshot file hashes (50 lines per format,
`snapshot<TAB>match<TAB>tick<TAB>bytes<TAB>file_hash`) are written to
`snapshots-<format>.txt` and printed on stdout; that whole list, not a sample
from one seed, is what the CI `compare` job diffs. Save and restore timings are
the one place the numbers move between runs — they are means over 50 samples on
a loaded desktop, so the ranges above are the honest form. The *sizes* and the
*hashes* did not move at all.

**Frozen choice (2) — the snapshot format.** *Provisionally **postcard***, on
size (**36 % smaller** at tick 4800: 4,563 B against 7,160 B) and on having
nothing target-dependent left in the format. rkyv saves ~10–14 µs faster and
restores ~10–50 µs slower; at this size neither difference matters. **The
decision cannot be closed until the cross-OS byte-identity row is filled**,
which is the whole reason that row exists: rkyv's configuration
(`little_endian`, `pointer_width_32`, `aligned`) is precisely the thing that has
not been tested on a second architecture. The caveat from section 5's postcard
row stands: sequence lengths are LEB128 of the host `usize`, value-identical
below 2^32 rather than width-independent by type.

### Step 8 — fork equivalence (research build)

`cargo run --release --locked --features research --bin forkcheck`

| Number | Value |
|---|---|
| Fork equivalences | **20 / 20** (2 fork points × 10 matches, child stepped 200 ticks) |
| Parents unperturbed | **10 / 10** — the forked run's full trace is byte-identical to the never-forked run |
| Fork cost, time | 3,360 ns mean (**0.00336 ms**) |
| Fork cost, bytes | min 7,064 / mean 7,384 / max 7,884, one line per fork naming its `(match, tick)` |
| Child, 200 ticks | 3,547 µs mean (~17.7 µs/tick) |

Per-match parent digests over the full forked run — the values the "unperturbed"
claim is made against: `d54317b9237b4ef4`, `1c1cb55d13098c00`,
`040ca26a5f27b75a`, `ca2f8a351cdecddb`, `a6a53b4e95083998`, `f4b76feb0fe491d7`,
`63126341c27c16c9`, `ece5457ae5c41324`, `4ba54fb97384e4f7`, `85598d316398460c`.
Fork size varies with how many credit entries are live at the fork point
(7,064 B at tick 100, when the credit table is still empty, against
7,569–7,884 B at tick 5,000), which is why it is reported as a range with every
sample labelled rather than as whichever fork happened last.

### Step 9 — feature isolation

`sh ./check-features.sh` — **PASS**, three checks, each with a positive control:

1. default release rlib (v0 symbol mangling): **0** symbols matching
   `14g4_determinism4fork`; no `forkcheck` binary produced;
2. `--features research` rlib: **3** such symbols — so check 1 cannot pass on a
   typo;
3. feature resolution via `cargo tree -f '{p} [{f}]'` (not `-e features`, which
   prints dependency feature *edges* and never the root package's own set):
   `research` is absent from the root package's resolved features in the default
   graph and from every deny-list crate (imbl, rkyv, postcard, serde,
   xxhash-rust), with a positive control asserting that the check *does* fire
   when `--features research` is passed.

`cargo build --release --locked --all-features` **fails on purpose** (exit 101,
`compile_error!`: "pick exactly one snapshot format"), because the two snapshot
features are alternatives and an additive build would measure rkyv twice.

### Lint discipline

`cargo clippy --release --locked --all-targets -- -D warnings` is **clean** under
the default features, under `--features research,snap-rkyv` and under
`--features research,snap-postcard`, with the spike-local `clippy.toml` in
place. See section 4 for why that file is needed and what it implies for
`cargo xtask clippy`.

### Test suite

**27 tests pass** under default features (28 defined; the 28th,
`ten_match_trace_digest_is_pinned`, is `#[ignore]`d as a full 96,000-tick run and
**passes** when run with `-- --ignored`). **29 pass** under
`--features snap-postcard,research` and 29 under `--features snap-rkyv,research`
— the two extra are `snapshot_bytes_round_trip` (which asserts `encode` is a
pure function of the value, the byte-stability property the whole cross-OS
snapshot comparison rests on) and `fork_is_equivalent_to_stepping`. Neither
exists in the default build, which is why CI runs three test invocations rather
than one.

### Contract candidates — section 8's four frozen choices, as implemented

Section 8 requires four choices to be frozen alongside the answer. All four are
implemented and measured; they need **owner approval** before harness part 1
encodes them, and choice (2) is explicitly conditional on step 5.

| # | Choice | As implemented | Status |
|---|---|---|---|
| 1 | **Hash encoding + seed** | Canonical byte encoding, never raw struct memory: each SoA table walked in id order, every field appended as fixed-width **little-endian** bytes; lengths hashed as `u32` after a checked conversion, so `usize` never enters the hash; `Option<u32>` as a one-byte tag **plus four value bytes in both arms**, making the encoding fixed-stride; `ENCODING_VERSION = 1` as the first byte from every `Enc` path, `Default` included. Digest = **xxh3-64 under one compiled-in seed, `STATE_HASH_SEED = 0x5048_4152_4D4B_4F53`** (`b"PHARMKOS"`). Tick-0 encoding 6,497 B; pins `digest(b"") == 0xFE1AF732B02810AD` and `digest(b"pharmakos/g4") == 0xE17026C8A5EA4D30`. | **Ready to approve.** Changing the seed or the version byte invalidates every golden file ever stamped with it. |
| 2 | **Snapshot format + configuration** | **Recommend postcard 1.1.3**, `default-features = false, features = ["use-std"]`, with serde 1.0.229 (`std`, `derive`). Every `Snapshot` field fixed-width (`u8`/`u16`/`u32`/`i32`/`i64`); no `usize`, no pointer, no `Option`, no enum — `Option<u32>` becomes the `NO_TARGET = u32::MAX` sentinel; `SNAPSHOT_VERSION = 1` is in the bytes and checked on restore. **Measured at tick 4800: 4,563 B mean, 0.041 ms save, 0.24–0.32 ms restore, 50/50 round trips.** The alternative, rkyv 0.8.18 with `little_endian` + `pointer_width_32` + `aligned` + `bytecheck` + `std`, measures **7,160 B mean, 0.027–0.031 ms save, 0.28–0.30 ms restore, 50/50 round trips**. | **Provisional — do not freeze yet.** Both formats round-trip perfectly here; the tie-breaker, cross-OS byte identity, is unmeasured. The LEB128-length caveat belongs with the entry. |
| 3 | **RNG construction + stream ids** | Counter-based, no shared mutable state; every draw a pure function of `(match_seed, stream_id, tick, seat, sub, counter)`. `GOLDEN = 0x9E3779B97F4A7C15`; `mix64` is the SplitMix64 finaliser (`^= >>30`, `× 0xBF58476D1CE4E5B9`, `^= >>27`, `× 0x94D049BB133111EB`, `^= >>31`), every multiply spelled `wrapping_mul` because overflow checks are on in every profile. `k0 = mix64(match_seed ^ stream_id*GOLDEN)`; `k1 = mix64(k0 ^ tick*GOLDEN)`; `key = mix64(k1 ^ (seat<<32) ^ sub)`; `draw(n) = mix64(key ^ n*GOLDEN)`. Ranges by branch-free multiply-shift, whose residual modulo bias is identical on every target. **Stream ids: `Combat = 1`, `Spawn = 2`, `Map = 3`** — explicit, never a discriminant cast, additive only, never renumbered. | **Ready to approve.** Pinned by literal golden values in `rng_construction_is_pinned`. |
| 4 | **The determinism rule set** (what harness part 1 turns into lints) | No `f32`/`f64` anywhere in sim code · no `as` casts (`From`/`TryFrom` with an explicit failure mode) · no `HashMap`/`HashSet` (`BTreeMap`, sorted `Vec`, `imbl::OrdMap`) · no wall-clock time inside sim crates (`Instant` appears only in the `src/bin/` harnesses) · `overflow-checks = true` in **every** profile, release included, with `debug-assertions = false` in release · every sort key ends in a unique id, so every comparator is total and stable-vs-unstable cannot matter · fixed-width types only in hashed state, lengths hashed as `u32` · no recursion in the sim (MSVC's ~1 MB default thread stack). Enforced today by `#![deny(clippy::as_conversions, clippy::float_arithmetic, clippy::disallowed_types)]` in `src/lib.rs`, where the discipline travels with the code rather than with the working directory. | **Ready to approve.** Clean under all three feature configurations. |

### Still outstanding

| Item | Where | Why it is not done here |
|---|---|---|
| Step 5 — the three-runner matrix (G4-a) | CI | **Running**: workflow `spike-g4-determinism` on commit `6e7a156` (run 34802164421, 2026-09-13). Result to be recorded in section 9. |
| Install `.github/workflows/spike-g4.yml` | repo root | **Done 2026-09-13** (commit `6e7a156`). |
| Commit `spikes/g4-determinism/Cargo.lock` | `spikes/.gitignore` | **Done 2026-09-13**: `!*/Cargo.lock` added, lock committed, every CI invocation passes `--locked`. |
| Section 8's four frozen choices → `docs/design/decisions-log.md` §2.7 | decisions log | Outside the spike directory, and the log is the owner's to edit. The candidates are tabulated above with their literal values: (1), (3) and (4) are ready to approve; **(2), the snapshot format, must not be written down as final until the cross-OS byte-identity row is filled.** The postcard varint caveat from section 5 belongs in that entry. |
| Section 3's two former `PLACEHOLDER`s (toolchain, hash seed) | this file, section 3 | **Done 2026-09-13**: `1.98.1` and `0x5048_4152_4D4B_4F53` written into steps 0 and 1. |

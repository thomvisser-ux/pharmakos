<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `package/` — what the unsigned zips hold

**Filled by T21** (decisions-log item 117 (10)). Two files, one per platform the game
ships on:

| File | Compared by | Re-baselined with |
| --- | --- | --- |
| `expected.windows.txt` | `cargo xtask package` on Windows: CI's `package (windows-latest)` job | `cargo xtask package --bless`, on Windows |
| `expected.linux.txt` | `cargo xtask package` on Linux: CI's `package (ubuntu-24.04)` job | `cargo xtask package --bless`, on Linux |

The `golden` step does not compare this area (`xtask/src/golden.rs` lists it in
`SELF_COMPARED_AREAS`): a zip is built only where Godot and its export templates are
installed, which the `cargo xtask ci` legs are not, and each package job builds one
platform's zip.

## What it pins

Every entry of the zip, read back from the zip's own central directory by `xtask`'s reader
(`xtask/src/zip.rs`), one per line, sorted in byte order, directories as their own entries
ending in `/`; LF endings and a trailing newline. `package` writes the fresh list to
`<target>/golden/package/actual.<platform>.txt`, compares it with the file here, and on a
missing or different file prints the list and fails, naming the first differing line.

**Paths only, and why.** The builds are not reproducible (Rust and Godot both), and
`library/`'s and `rules/`'s bytes move for reasons of their own (the templates'
own-values PLACEHOLDERs, before T22's demo), so sizes or hashes here would move with every
such change and say nothing about the zip. What the list does pin is what a tester gets:
the executable and its `.pck`, the client library beside it, `gamectl`, `rules/` and
`library/` as the repository has them (the three templates Easy instantiates by id among
them), `LICENSES/` with exactly the licences the zip's `REUSE.toml` names, the note, the
changelog and the third-party notices. A file that goes missing or appears is a red job.

**What it cannot see.** The inside of the `.pck`: which scenes and scripts shipped.
`package` byte-scans the written `.pck` for that instead, and fails if it names
`fixtures/`, `_shot.` or any check but the smoke check, or does not name the smoke check.
Nor does it see a truncated or wrong-build binary; the smoke check, run on the extracted
zip by CI's `clean launch (<os>)` jobs, is what shows the binaries work.

## When it moves

A different list is a change to what ships. Read the diff `package` prints; if the change
is meant (a new file in `rules/` or `library/`, a licence added because a crate's licence
changed, a file Godot's export now writes), re-bless on that platform with
`cargo xtask package --bless`, and say in the pull request what moved and why. Never
re-bless to make a red job green without that reason. The Linux list was first derived on
Windows from a cross-export of the Linux preset (a placeholder library in a scratch copy,
never shipped) and is checked by CI's first Linux package.

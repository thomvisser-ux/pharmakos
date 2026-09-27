<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# `packaging/` — what the unsigned zips carry besides the build

`cargo xtask package` (skeleton plan T21, decisions-log item 117) builds one zip per
platform, Windows x86_64 and Linux x86_64, holding the one folder `Pharmakos/`. This
directory holds what it writes into that folder from templates, and the two licence texts
the zip needs that this repository's own code does not.

| file | becomes |
| --- | --- |
| `README.windows.txt.in`, `README.linux.txt.in` | the zip's `README.txt`, the note (item 117 (12)): the version, the build line, how to open an unsigned build first, then the rest. `@VERSION@`, `@BUILD@`, `@COMMIT@` and (Linux) `@GLIBC@` are filled in; a marker left unfilled fails the command |
| `REUSE.toml.in` | the zip's `REUSE.toml` (item 117 (11)), with the build's file names and the two Rust binaries' licence expressions filled in. Named `.in` so this repository's `reuse` does not read it as a nested manifest; Godot's quoted copyright lines sit between `REUSE-IgnoreStart` and `REUSE-IgnoreEnd`, which the command drops |
| `LICENSE-MPL-2.0.txt`, `LICENSE-BSL-1.0.txt` | `LICENSES/MPL-2.0.txt` and `LICENSES/BSL-1.0.txt` in the zip: the godot-rust crates in the client library are MPL-2.0, and xxhash-rust in `gamectl` is BSL-1.0. Fetched with `reuse download -o`; `reuse` ignores `LICENSE*` names, so they are not licences this repository uses |
| `LICENSE` | this directory's licence pointer |

The zip's `LICENSES/` holds exactly the licences its `REUSE.toml` names — `reuse` fails an
unused one — taken from `../LICENSES/` or from here. `THIRD-PARTY-NOTICES.txt` is generated
from `cargo tree` and the crates' own licence files; `CHANGELOG.md` is the repository's.

## PLACEHOLDERs

- **The repository's public address** in the note's source line (GPL-3.0 section 6,
  MPL-2.0 section 3.2): the note names the commit and says the address is set before a
  release is published. OWNER, with the org, before the owner publishes a release (item
  117 (12)).
- **The export preset's product name, icon, company and copyright** are in
  `../godot/README.md`'s export section.

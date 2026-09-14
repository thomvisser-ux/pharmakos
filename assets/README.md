<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->
<!-- SPDX-FileCopyrightText: 2026 Pharmakos contributors -->

# `assets/` — art and audio

Everything the project authors here (`.vox` models, palettes, generated sound)
is **CC BY-SA 4.0**, which is one-way compatible into the game's GPL-3.0-or-later
build (see [docs/LICENSING.md](../docs/LICENSING.md)).

Third-party audio may come **only from CC0 or CC-BY sources** (Kenney, Freesound
under those filters, sfxr/ChipTone output). Because a `.vox` or `.ogg` cannot
carry a comment header, every asset file gets a REUSE sidecar next to it:
`shot.ogg` → `shot.ogg.license` naming the author, the licence and the source
URL. CC-BY assets are also listed on the in-game credits screen.

Layout, filled in from the walking skeleton onward:

| Directory | Contents |
|---|---|
| `models/` | MagicaVoxel / Goxel `.vox` sources, loaded with `dot_vox` and meshed by the terrain mesher |
| `palettes/` | The shared voxel palette(s) |
| `audio/sfx/`, `audio/music/` | Sound effects and the two or three music tracks, each with its `.license` sidecar |

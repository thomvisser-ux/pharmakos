<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `mesher/` — geometry digests

**Filled by T4** (`crates/mesher`).

Per-chunk vertex and index digests over a fixed chunk set, produced by the
headless CPU proxy that links **without gdext**, so CI checks geometry with no
GPU. Compared across Windows and Linux.

## The file

`expected.digests.txt`, one record per line, tab-separated:

```text
<case>\t<vertex-digest>\t<index-digest>\t<vertices>\t<indices>
```

Both digests are **xxh3-64** as sixteen lowercase hex digits — the project's one
hash function, the same one the sim's state hash uses (AGENTS.md §3, §5: never
add a second). The vertex digest runs over the emitted vertices in order, **28
bytes each**: position x, y, z then normal x, y, z as
`f32::to_bits().to_le_bytes()`, then the four colour bytes. The index digest runs
over the `u16` indices in order, two bytes each. The counts are decimal.

The float **bit patterns** are hashed rather than printed values on purpose: two
operating systems have to agree on the bits, and a digest over formatted decimals
would hide a one-ulp difference behind the rounding.

The producer is `crates/mesher/tests/geometry.rs`, which also builds every case —
procedurally, from a documented `splitmix64` mixer, because a 32 KiB material
array per case is not human-diffable and the generator is. Every case goes
through the real path: bake the light with `LightField`, gather the six borders
with `ChunkBorders`, mesh with `Mesher`.

## The cases

| Case | What it pins |
| --- | --- |
| `empty` | A chunk that meshes to **no surface at all** — not a surface with zero vertices |
| `full` | Solid stone at the map's rim: six whole-face quads, and the synthesised sky borders |
| `single_voxel` | The smallest non-empty mesh, six quads |
| `flat_floor` | The greedy merge's easiest win, and the case whose colours are checked by hand |
| `stair` | The mask key splitting on **material** |
| `overhang_tunnel` | G1 §10.12: a tunnel and an overhang lit from beside them |
| `crater` | The shape the per-chunk cost is measured on |
| `checkerboard` | The 16-bit headroom, 49 152 vertices against the 65 536 cap |
| `six_borders` | The middle chunk of a 3×3×3 map: all six borders are real neighbours |
| `light_split` | The mask key splitting on **light** across one otherwise-uniform quad |

## What a diff means

* **The mesh changed.** Quad merging, the `(material, light)` mask key, the
  light bake, vertex order or winding. Godot's front face is **clockwise** under
  `CULL_BACK` and the quad walk is emitted reversed to match: a digest that
  moved after a tidy-up of the walk is usually winding, and inverted winding is
  invisible in a digest but obvious in the vista screenshot.
* **Only lit chunks moved.** The flood fill seeds every sky cell that has a
  taller horizontal neighbour, not the lowest cell per column (G1 §10.12).
  Seeding the lowest renders every overhang black and degenerates the mask key.
  `overhang_tunnel` is the case that catches it, and it carries an assertion of
  its own beside the digest so the failure names the cause.
* **Every case moved at once.** Look at the parameters before the algorithm.
  `LIGHT_MAX` / `LIGHT_ATTEN` are stated at the top of `geometry.rs` because this
  crate may not read `rules/rules.v1.json`; the palette and the six face-shading
  factors are art constants in `crates/mesher/src/sweep.rs`. Any of the eight
  moves every colour in the file and nothing else.
* **The dirty pad changed.** It is derived from `LIGHT_MAX` and `LIGHT_ATTEN` as
  *light reach + 1*, not asserted. A digest that moved after an attenuation
  change is correct; one that did not is the pad being one short.
* **Windows and Linux disagree.** The mesher is walled — floats are legal inside
  it — so this is where a float difference becomes visible. Every vertex
  coordinate is an exact small integer reached through `f32::from(u8)` and every
  colour is quantised in integers before the final byte, so there should be
  nothing to disagree about; if there is, that boundary is where to look.
* **16-bit index headroom.** A chunk that no longer fits 16-bit indices is a
  correctness failure, not a golden move: the mesher returns
  `MeshError::TooManyVertices` and the case's line never appears.

<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `mesher/` — geometry digests

**Filled by T4** (`crates/mesher`).

Per-chunk vertex and index digests over a fixed chunk set, produced by the
headless CPU proxy that links **without gdext**, so CI checks geometry with no
GPU. Compared across Windows and Linux.

## What a diff means

* **The mesh changed.** Quad merging, the `(material, light)` mask key, the
  light bake, vertex order or winding. Godot's front face is **clockwise** under
  `CULL_BACK` and the quad walk is emitted reversed to match: a digest that
  moved after a tidy-up of the walk is usually winding, and inverted winding is
  invisible in a digest but obvious in the vista screenshot.
* **Only lit chunks moved.** The flood fill seeds every sky cell that has a
  taller horizontal neighbour, not the lowest cell per column (G1 §10.12).
  Seeding the lowest renders every overhang black and degenerates the mask key.
* **The dirty pad changed.** It is derived from `LIGHT_MAX` and `LIGHT_ATTEN` as
  *light reach + 1*, not asserted. A digest that moved after an attenuation
  change is correct; one that did not is the pad being one short.
* **Windows and Linux disagree.** The mesher is walled — floats are legal inside
  it — so this is where a float difference becomes visible. Whatever crosses
  back into the sim crosses as an integer newtype at a named boundary function;
  check that boundary first.
* **16-bit index headroom.** A chunk that no longer fits 16-bit indices is a
  correctness failure, not a golden move.

<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `vista/` — the rendered screenshot

**Filled by T16** (`godot/` and `crates/client-gdext`), promoted from skipping to
required by T20.

`expected.vista.png` is a 1280 x 720 render, taken **windowed under xvfb and
lavapipe on Linux**. It is the one golden in the tree that is *not* compared byte
for byte: `xtask/src/png.rs` compares it with a tolerance — the mean absolute
per-channel difference, and the share of pixels differing by more than 32.

Three things this file is not, each learned the expensive way in spike G1
(§10.12):

* It is not taken with `godot --headless`. That selects the dummy rendering
  driver, `frame_post_draw` never fires, and a screenshot coroutine awaiting it
  parks for ever — silently, producing no PNG and no error.
* It is not compared for equality. G1 measured 1.14 % of pixels differing *at
  all* between a Quadro and lavapipe on identical geometry, so exact equality
  would be permanently red.
* It is not a substitute for `mesher/`'s digests. The geometry golden is the
  stronger of the two and needs no GPU; the screenshot is an alarm that says
  "look", not a certification.

The first render of every fresh checkout is preceded by
`godot --headless --path godot --import`, which writes
`res://.godot/extension_list.cfg`. Without it the extension's classes
instantiate as placeholders and the scene fails on its first call. G1 lost four
CI runs to this.

## What a diff means

The comparator prints both statistics and the worst single-channel delta, and
publishes them as a `::notice::` annotation because GitHub hides job logs and
step summaries from logged-out viewers.

* **The mean moved and the worst delta did not.** A rasteriser tie-break — a
  driver update, a Mesa version. Look at `mesher/`: if its digests are
  unchanged, the geometry is unchanged and this is the alarm saying "look", not
  "this is broken".
* **The worst delta is large and localised.** Cracks, missing faces or inverted
  winding — the bug class this golden exists to catch. Check `mesher/` and the
  quad walk's winding before regenerating anything.
* **The image is blank or near-uniform.** The variance floor catches this. It
  usually means the run fell back to a different renderer: G1 found that
  pointing `VK_ICD_FILENAMES` at a missing lavapipe ICD makes the Vulkan loader
  find *no* driver, after which Godot silently switches to OpenGL 3. Let the
  loader enumerate the installed ICDs instead.
* **The sizes differ.** A windowed run is clamped by the xvfb screen size (G1
  asked for 1920 x 1080 on a 1920 x 1080 screen and got 1920 x 1061). That is a
  configuration error, and the comparator reports it as one rather than as a
  difference.

PLACEHOLDER: the thresholds shipped today are G1's generous gate — mean at most
6.0/255 and at most 2 % of pixels over 32. Decisions-log item 22 recommends
tightening them to G1's *measured* shape (mean 0.0039/255, zero pixels over 32)
once a real vista replaces the fixture. Owner re-ratifies at T16; the numbers are
two named constants in `xtask/src/png.rs`.

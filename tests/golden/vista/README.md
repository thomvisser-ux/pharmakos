<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `vista/` — the rendered screenshot, and the client's two text goldens

**Filled by T16** (`godot/` and `crates/client-gdext`), promoted from skipping to
required by T20.

Three files, and none of them is compared by the `golden` step
(`xtask/src/golden.rs` lists `vista/` in `SELF_COMPARED_AREAS`):

| File | Compared by | Re-baselined with |
| --- | --- | --- |
| `expected.vista.png` | `cargo xtask screenshot`, with a tolerance, on the Linux leg | a deliberate re-render (below) |
| `expected.geometry.txt` | `crates/client-gdext/tests/vista_fixture.rs`, byte for byte, on every leg | `PHARMAKOS_BLESS_VISTA=1 cargo test -p pharmakos-client-gdext` |
| `expected.events.txt` | `crates/client-gdext/tests/event_list.rs`, byte for byte, on every leg | the same |

The two text goldens write their fresh output to
`<target>/golden/vista/actual.*.txt` whether they pass or not, so a red run
leaves the new file on disk to be diffed.

## `expected.geometry.txt` — the client's geometry for the keyframe fixture

One line per chunk of the map: the chunk's index and corner in the **mesher's**
axes (y up), then the vertex and index digests in the mesher golden's own format
(xxh3-64 over 28 bytes a vertex and 2 bytes an index, `mesher/README.md`), then
the counts. The input is `godot/fixtures/view_keyframe.jsonl` — a byte-identical
copy of `gateway/view_keyframe/expected.keyframe.jsonl`, the gateway's own
golden — pushed through exactly what the live client does with a `get_view`
answer: the `_status` footer split off, the lower-case enum spelling translated
back, every chunk's run list decoded by `pharmakos_proto::chunk_rle`, each wire
material looked up in the palette table (`view::palette_of`; an unknown value
draws nothing), the chunk transposed into mesher order, the whole map lit by the
mesher's flood fill, and each chunk meshed with its six real neighbours.

The wire carries no chunk digest (T16a cut it), so this is the client's half of
the view pinned end to end. A diff means one of:

* **the gateway's keyframe golden moved** — the vista fixture test fails first,
  because the copy under `godot/` no longer matches. Copy it again; the reason
  belongs to the gateway's pull request (map generator, run-length codec or the
  view's shape; `gateway/README.md`).
* **the mesher moved** — `mesher/` will have moved too, and it explains why.
* **the client moved** — the palette lookup, the transposition, the light
  wiring or the border gathering. That is this crate's behaviour change, and the
  vista golden will usually move with it.

## `expected.events.txt` — the live event list

The rows the watch rig renders from `crates/client-gdext/tests/fixtures/
get_segment_feed.json` (a `get_segment_feed` result generated through the
codec): one row per event, in the feed's order, as `m:ss  kind  text`. The event
list is a view of the segment feed and of nothing else, so a row added, dropped,
merged, reordered or reworded by the client is the diff this file exists to
show. A new *event kind* is not a diff here: the fixture holds the kinds it holds
until somebody adds one to it.

## `expected.vista.png` — the rendered vista

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

### How it is rendered, and how it is re-rendered

The scene is `res://scenes/vista_shot.tscn`, reached through the project's main
scene with `--scene=` and `--shot=` after `--` (`xtask/src/main.rs`,
`step_screenshot`). It starts **no host**: it feeds every line of
`godot/fixtures/view_keyframe.jsonl` through `view_apply`, the same decode entry
the live watch rig uses, drains the upload queue to empty under item 54's
budget, draws a few more frames with nothing left to upload, and only then reads
the frame back — so the shot is taken on a frame outside any measured series
(nothing in the scene is timed at all). The camera is the rig's whole-map
framing, looking from the north-east (the corner at the map's largest x and
north coordinates is nearest the camera).

Decisions-log item 105 (5): the golden is rendered by CI's Linux leg, not on a
developer's machine. With no `expected.vista.png` committed, the `screenshot`
job's bootstrap step renders with the same command line and uploads the PNG as
the `vista-screenshot` artefact; somebody looks at it, commits it here and
writes below what it shows. A deliberate re-render starts the same way: delete
the golden, push, download, look.

### What the golden shows

Committed from the `screenshot` job's bootstrap render on CI's Linux leg
(workflow run 35820083263, xvfb + lavapipe, Godot 4.7.2), looked at before it
was committed and again by the main session before the merge.

It shows **the whole generated map of the golden seed** (`0x00000000ca5caded`,
384 x 384 x 64 voxels, 12 x 2 x 12 chunks) from above its north-east corner, as
a diamond on the ash-grey sky of the Pall: open dirt terrain with the generator's
contour steps visible as fine lines, the scrap seams as pale-yellow blobs, the
heat vents as small blue patches, the stone of the map's rim along the two near
edges, and seat 0's own beacon and units as small markers near the right-hand
corner of the frame (the beacon a glowing yellow pillar). Nothing else stands on it, because the
fixture is seat 0's view of the opening Lull. Checked by eye for the bug classes
this golden exists for: no cracks between chunks, no missing chunk, no
back-face-culled holes, and the tops facing the camera (winding correct).

The terrain reads as one flat colour because every open voxel is lit to
`light_max` under the flood fill and the top face is unshaded; that is the
placeholder palette and shading (mesher `PALETTE`, `FACE_SHADE_256`, and
`client-gdext`'s `view::palette_of`), all art PLACEHOLDERs for S6.

### What a diff in the PNG means

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

### Who compares the PNG

Not the `golden` step. `xtask/src/golden.rs` names `vista/` in
`SELF_COMPARED_AREAS` and leaves it alone: the fresh render exists only on the
Linux leg that produced it, the `golden` step runs before `screenshot`, and a
byte comparison would replace the tolerance below with exactly the equality test
G1 measured as permanently red. `cargo xtask screenshot` is what compares it, and
what re-renders it.

PLACEHOLDER: the thresholds shipped today are G1's *measured* shape — mean at
most 0.004/255 (G1 measured 0.0039, rounded up to the next thousandth) and
**zero** pixels over 32 — which is what skeleton-plan §7
decision 22 (recommended, not yet logged) names. The spike's own
`compare_vista.py` shipped with a gate roughly 1 500× looser (6.0/255, 2 % over
32); at 2 % of a 1280 x 720 frame a missing chunk covering ~18 000 pixels passes,
which is a green tick that means nothing. Owner re-ratifies both numbers at T16
against the first real vista; they are two named constants in
`xtask/src/png.rs`.

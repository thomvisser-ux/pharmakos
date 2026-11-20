<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# P6b task card B

**Open this file only when card B's watch has started.** The run sheet is
`docs/demo/p6-run-sheet.md`.

One mandate row, one guard and one chip edit. The words in quotes are the controls' own.

1. Under "Start from a template", press "Safe playbook", then "Use this playbook".
2. **The mandate row.** Click your core on the map. In its menu, open "Visit & change", then "Add
   a build target (then click the ground)", and take "Autocannon". The status line says "Click the
   ground where the target goes; Escape stops." Click flat ground a few voxels from your core,
   inside its sphere, clear of the core and the drones. Step 3 appears under "What the playbook
   says": "3. visit_1: interface with beacon "b_00" and add a build target: autocannon at voxel
   ...".
3. **The guard.** Directly under step 3's line, press "Condition: skip if". In "Pick a condition",
   under "Economy", select "Grid headroom", then press "Use this condition". A second line appears
   under step 3: "Skip this step if the grid's headroom is below 5 kW."
4. **The chip edit.** In that line, click "5". In "Change 5:", select the old number first
   (double-click it, or Ctrl+A), type 3 and press "Set this value". The line should now read
   "... below 3 kW." If it reads "53" or "35", the digits went in beside the old one: say so (it is
   a finding, not a miss) and set it to 3.
5. Press "Submit".

If "Visit & change" shows "Reading this beacon..." instead of its rows, close the menu with Escape
and click your core again. If a step is refused, read the refusal, put it right, and say so: the
time runs on.

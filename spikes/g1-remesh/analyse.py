# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""Reduce a run's frame-time CSV to the rows of the plan's results table.

Usage: python analyse.py results/frames_*.csv

Percentiles are nearest-rank on the sorted sample, the same definition
`stats.rs` uses, so the in-engine numbers and meshbench's are comparable.
Frames 1..WARMUP are reported separately: frame 1 carries the whole scene load
(worldgen + the initial unbudgeted upload of every non-empty chunk) and is not
part of the steady state the gate is about.

Three definitions in here are worth stating out loud, because each of them was
reported under a misleading name in the first pass of this spike:

* **Frame time comes from `ext_dt_us`**, the extension's own raw monotonic
  delta, never from `frame_time_ms`. The latter is `_process(delta)`, and Godot
  4.7 defaults `application/run/delta_smoothing` to true, which rewrites that
  delta into a smoothed estimate quantised to refresh-rate steps: with it on,
  572 of 870 steady frames read exactly 1.3889 ms (= 1/720 s) while the raw
  delta in the same rows had a p50 of 0.71 ms. The smoothed column is still
  summarised, as `gd_delta_*`, and `delta_smoothing_suspected` flags a file whose
  smoothed column looks quantised — but no gate number is taken from it.

* **`max_queue_depth` is the queue's peak depth**, read from the summary JSON,
  which is the "does the queue ever run away" row the plan asks for. The CSV's
  `queued_after` column is the depth *after* each frame's drain and is reported
  separately as `post_drain_max`; it reads about 2x lower and is not the same
  quantity.

* **`lat_*` covers uploaded chunks only.** A chunk still sitting in the queue
  when the run ends contributes no latency sample, so a configuration whose
  queue runs away hides exactly its worst chunks. `lat_pending_n` and
  `lat_incl_pending_*` come from the summary and count them at their final age.
"""

import csv
import glob
import io
import json
import os
import sys

WARMUP = 30
BUDGET_MS = 16.67


def pct(sorted_vals, p_permille):
    if not sorted_vals:
        return 0
    n = len(sorted_vals)
    rank = min(max((p_permille * n + 999) // 1000, 1), n)
    return sorted_vals[rank - 1]


def looks_smoothed(values):
    """True when a delta series is dominated by a single repeated quantum.

    A real frame-time series on a free-running renderer has almost no exactly
    repeated values; a smoothed one snaps to 1/refresh steps.
    """
    if len(values) < 100:
        return False
    dup_runs = sum(1 for a, b in zip(values, values[1:]) if a == b)
    return dup_runs > len(values) // 4


def analyse(path):
    header = {}
    with open(path, encoding="utf-8") as fh:
        text = [ln for ln in fh if not ln.startswith("#")]
        fh.seek(0)
        for ln in fh:
            if ln.startswith("# godot="):
                for part in ln[2:].strip().split(" "):
                    if "=" in part:
                        k, v = part.split("=", 1)
                        header[k] = v
    rows = list(csv.DictReader(io.StringIO("".join(text))))
    steady = [r for r in rows if int(r["frame"]) > WARMUP]

    # The honest series: a raw monotonic delta measured in the extension.
    ft = sorted(float(r["ext_dt_us"]) / 1000.0 for r in steady)
    all_ft = sorted(float(r["ext_dt_us"]) / 1000.0 for r in rows)
    total_ms = sum(float(r["ext_dt_us"]) for r in rows) / 1000.0

    # Godot's own smoothed delta, kept only so the difference stays visible.
    gd = sorted(float(r["frame_time_ms"]) for r in steady)
    gd_raw_order = [float(r["frame_time_ms"]) for r in steady]

    expl = sum(int(r["explosions_this_frame"]) for r in rows)
    up_chunks = sum(int(r["uploaded_chunks"]) for r in rows)
    up_bytes = sum(int(r["uploaded_bytes"]) for r in rows)
    remesh = sorted(float(r["remesh_ms_this_frame"]) for r in steady)
    upload = sorted(float(r["upload_ms_this_frame"]) for r in steady)
    post_drain = sorted(int(r["queued_after"]) for r in rows)
    pre_drain = sorted(int(r["queued_chunks"]) for r in rows)

    summary_path = path.replace("frames_", "summary_").replace(".csv", ".json")
    sm = {}
    if os.path.exists(summary_path):
        with open(summary_path, encoding="utf-8") as fh:
            sm = json.load(fh)

    out = {
        "file": os.path.basename(path),
        "adapter": header.get("adapter", "?"),
        "timings": header.get("timings", "?"),
        "path": header.get("path", "?"),
        "K": header.get("K", "?"),
        "B": header.get("B", "?"),
        "frames": len(rows),
        "wall_s": round(total_ms / 1000.0, 3),
        "explosions": expl,
        "expl_per_s_wall": round(expl / (total_ms / 1000.0), 1) if total_ms else 0,
        # G1-c, from the raw clock.
        "ft_p50": round(pct(ft, 500), 3),
        "ft_p90": round(pct(ft, 900), 3),
        "ft_p99": round(pct(ft, 990), 3),
        "ft_max": round(ft[-1], 3) if ft else 0,
        "fps_p50": round(1000.0 / pct(ft, 500), 1) if pct(ft, 500) else 0,
        "over_budget_steady": sum(1 for v in ft if v > BUDGET_MS),
        "over_budget_all": sum(1 for v in all_ft if v > BUDGET_MS),
        # Godot's smoothed delta, for comparison only.
        "gd_delta_p50": round(pct(gd, 500), 4),
        "gd_delta_p99": round(pct(gd, 990), 4),
        "delta_smoothing_suspected": looks_smoothed(gd_raw_order),
        "remesh_ms_p50": round(pct(remesh, 500), 3),
        "remesh_ms_p99": round(pct(remesh, 990), 3),
        "remesh_ms_max": round(remesh[-1], 3) if remesh else 0,
        "upload_ms_p99": round(pct(upload, 990), 3),
        "upload_ms_max": round(upload[-1], 3) if upload else 0,
        # The plan's "max queue depth" row is the instrumented peak.
        "max_queue_depth": sm.get("max_queue_depth"),
        "pre_drain_max": pre_drain[-1] if pre_drain else 0,
        "post_drain_max": post_drain[-1] if post_drain else 0,
        "chunks_uploaded": up_chunks,
        "bytes_uploaded": up_bytes,
        "bytes_per_chunk": round(up_bytes / up_chunks) if up_chunks else 0,
        "upload_us_per_chunk": (
            round(sum(float(r["upload_ms_this_frame"]) for r in rows) * 1000.0 / up_chunks, 1)
            if up_chunks
            else 0
        ),
        "remesh_us_per_chunk": (
            round(sum(float(r["remesh_ms_this_frame"]) for r in rows) * 1000.0 / up_chunks, 1)
            if up_chunks
            else 0
        ),
        "draw_calls": int(rows[-1]["draw_calls"]) if rows else 0,
        "primitives": int(rows[-1]["primitives"]) if rows else 0,
        "video_mem_mb": float(rows[-1]["video_mem_mb"]) if rows else 0,
        "lat_p50": sm.get("lat_p50"),
        "lat_p90": sm.get("lat_p90"),
        "lat_p99": sm.get("lat_p99"),
        "lat_max": sm.get("lat_max"),
        "lat_pending_n": sm.get("lat_pending_n"),
        "lat_incl_pending_p99": sm.get("lat_incl_pending_p99"),
        "lat_incl_pending_max": sm.get("lat_incl_pending_max"),
        "mesh_us_p50": sm.get("mesh_us_p50"),
        "mesh_us_p99": sm.get("mesh_us_p99"),
        "mesh_us_max": sm.get("mesh_us_max"),
        "inplace_updates": sm.get("inplace_updates"),
        "rebuilds": sm.get("rebuilds"),
        "rebuilds_index_changed": sm.get("rebuilds_index_changed"),
        "inplace_ratio_ppm": sm.get("inplace_ratio_ppm"),
        "colour_conv": sm.get("colour_conv"),
        "resources_created": sm.get("resources_created"),
        "coalesced": sm.get("coalesced"),
        "panics": sm.get("panics"),
    }
    return out


def main():
    args = sys.argv[1:]
    files = []
    for a in args:
        files.extend(sorted(glob.glob(a)))
    rs = [analyse(f) for f in files]
    print(json.dumps(rs, indent=1))
    print()
    cols = [
        "file", "path", "K", "B", "frames", "wall_s", "expl_per_s_wall",
        "ft_p50", "ft_p99", "ft_max", "over_budget_steady", "over_budget_all",
        "lat_p50", "lat_p99", "lat_max", "lat_pending_n", "lat_incl_pending_max",
        "mesh_us_p50", "mesh_us_p99", "remesh_ms_p99", "upload_ms_p99",
        "max_queue_depth", "post_drain_max", "bytes_per_chunk",
        "upload_us_per_chunk", "remesh_us_per_chunk", "draw_calls",
        "primitives", "video_mem_mb", "inplace_ratio_ppm",
        "rebuilds_index_changed", "delta_smoothing_suspected",
    ]
    print(" | ".join(cols))
    for r in rs:
        print(" | ".join(str(r.get(c, "")) for c in cols))

    # A panic caught at the FFI boundary would otherwise vanish into a default
    # return value and a healthy-looking CSV (plan §6, "Panics across FFI").
    bad = [r["file"] for r in rs if r.get("panics")]
    if bad:
        print(f"\nFAIL: panics were caught during {bad}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()

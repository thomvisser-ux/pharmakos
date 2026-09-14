# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""Publish the G3' job's diagnostics where a logged-out viewer can read them.

GitHub hides job logs *and* step summaries from viewers who are not signed in,
but shows annotations. So the tick summary, the per-phase table, the cost-model
fit and the hash digest go out as `::notice::` workflow commands, in chunks
under the per-annotation size cap, with newlines encoded as `%0A`. The same text
is appended to the step summary for anyone who is signed in.

Encoding rules copied verbatim from the G2 spike's `ci/annotate.py`
(`spikes/README.md` rule 2 allows copying between spikes); what differs is only
which files are read and how they are summarised.

Run from the spike directory after the tickbench and costmodel steps:

    python ci/annotate.py

`python`, not `python3`: actions/setup-python guarantees `python` on all three
runners, while `python3` on windows-latest depends on the runner image shipping
a `python3.exe` alias. This step is diagnostics, and diagnostics must not be the
thing that fails the job.
"""

import json
import os

CAP = 3800  # per-annotation message cap, with margin
LINES_PER_CHUNK = 40


def enc(text):
    return text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


def emit(title, text):
    lines = text.splitlines() or ["(empty)"]
    chunks = [lines[i : i + LINES_PER_CHUNK] for i in range(0, len(lines), LINES_PER_CHUNK)]
    for n, chunk in enumerate(chunks, 1):
        body = enc("\n".join(chunk))[:CAP]
        print(f"::notice title={title} {n}/{len(chunks)}::{body}")


def load(path):
    try:
        with open(path) as fh:
            return json.load(fh)
    except (OSError, ValueError) as exc:
        return {"error": f"{path}: {exc}"}


def get(d, *keys, default="(missing)"):
    cur = d
    for k in keys:
        if not isinstance(cur, dict) or k not in cur:
            return default
        cur = cur[k]
    return cur


def tick_summary(path):
    d = load(path)
    if "error" in d:
        return d["error"]
    cfg = d.get("config", {})
    out = [
        f"os={d.get('os')}/{d.get('arch')} overflow_checks={d.get('overflow_checks')} "
        f"pin={d.get('pin')}",
        f"seats={cfg.get('seats')} units={cfg.get('units')} beacons={cfg.get('beacons')} "
        f"structures={cfg.get('structures')} ticks={cfg.get('ticks')} "
        f"warmup={cfg.get('warmup')} edits/s={cfg.get('edits_per_s')} "
        f"repath_cap={cfg.get('repath_cap')} hash={cfg.get('hash_mode')} "
        f"repeat={cfg.get('repeat')}",
    ]
    t = d.get("tick", {})
    # The pathing phase is a SUBSTITUTE, not a measurement, and at the gate
    # configuration it is ~98% of the tick. Printing the gate verdict without
    # saying so would let a reader take a calibrated busy loop for measured sim
    # work, so the substituted share goes on the same two lines as the verdict.
    pathing = {}
    for p in d.get("phases", []):
        if p.get("phase") == "pathing":
            pathing = p
    sub = pathing.get("substituted_permille_of_tick")
    sub_note = (
        f"   [{sub}/1000 of this is CHARGED from G2, not measured: "
        f"charged={pathing.get('charged_ms')} ms real={pathing.get('real_ms')} ms]"
        if sub is not None
        else ""
    )
    out.append(
        f"G3'-a tick p50={t.get('p50')} p90={t.get('p90')} p99={t.get('p99')} "
        f"max={t.get('max')} ms   (gate: p99 <= 25){sub_note}"
    )
    out.append(
        f"G3'-b mean={t.get('mean')} ms ticks/s={t.get('ticks_per_s')} "
        f"speed={t.get('speed_multiple')}x   (gate: >= 4x, i.e. mean <= 12.5 ms){sub_note}"
    )
    out.append(
        f"tick minus the substituted pathing phase = "
        f"{d.get('tick_minus_pathing_mean_ms')} ms   "
        f"(the charged half reproduces G2's milliseconds on every machine by "
        f"construction, so this is the only figure two platforms can be compared on)"
    )
    cal = d.get("calibration", {})
    out.append(
        f"calibration {cal.get('ps_per_iter')} ps/iter -> repath={cal.get('iters_per_repath')} "
        f"tail={cal.get('iters_per_repath_tail')} repair={cal.get('iters_per_crater_repair')} "
        f"csr={cal.get('iters_csr_per_tick')} iters; "
        f"burst=1-in-{cal.get('burst_one_in_PLACEHOLDER')} x{cal.get('burst_factor_from_G2')} "
        f"(PLACEHOLDER: the rate is not measured and the p99 is a readout of it); "
        f"timing overhead {d.get('timing_overhead_ns_per_tick')} ns/tick"
    )
    rep = d.get("repeat", {})
    out.append(
        f"repeat runs={rep.get('runs')} p99s={rep.get('p99_ms_sorted')} "
        f"median_run={rep.get('median_run')} "
        f"hash_streams_identical={rep.get('hash_streams_identical')}"
    )
    h = d.get("hash", {})
    out.append(
        f"hash mode={h.get('mode')} digest={h.get('digest')} "
        f"bytes/tick={h.get('bytes_per_tick')} "
        f"tables_rehashed/tick={h.get('tables_rehashed_per_tick_milli')}/1000 "
        f"whole-voxel-store rehash={h.get('whole_voxel_store_rehash_ms')} ms "
        f"over {h.get('store_bytes')} B"
    )
    m = d.get("memory", {})
    out.append(
        f"memory allocs/tick={m.get('allocs_per_tick_milli')}/1000 "
        f"peak heap={m.get('peak_heap_bytes')} B peak RSS={m.get('peak_rss_bytes')} B"
    )
    out.append(f"drift: tick p50 per game-minute   = {d.get('per_minute_p50_ms', [])}")
    out.append(f"drift: tick mean per game-minute  = {d.get('per_minute_mean_ms', [])}")
    out.append(
        f"drift: tick p99 per game-minute   = {d.get('per_minute_p99_ms', [])}  "
        f"(BIMODAL — this series reports each minute's repath-burst count, not its cost)"
    )
    out.append(
        f"drift first->last minute = {d.get('drift_permille_first_to_last_minute')} permille "
        f"from the MEAN series ({d.get('drift_permille_first_to_last_minute_p50')} from p50; "
        f"{d.get('drift_permille_first_to_last_minute_p99_unreliable')} from p99, which is luck)"
    )
    kc = d.get("kill_credit_settlement_ticks", {})
    out.append(
        f"kill_credit on the ticks that settled a death: {kc.get('dist')} "
        f"(deaths={kc.get('deaths')}; the phase's headline distribution above is the empty loop)"
    )
    w = d.get("work_totals", {})
    out.append(
        f"work units_seen={w.get('units_seen')} programs={w.get('programs_run')} "
        f"queries={w.get('queries')} candidates={w.get('candidates')} "
        f"attacks={w.get('attacks')} damage={w.get('damage_events')} "
        f"deaths={w.get('deaths')} repaths={w.get('repaths_served')} "
        f"backlog_mean={w.get('repath_backlog_mean_milli')}/1000 craters={w.get('craters')} "
        f"voxels_removed={w.get('voxels_removed')} rules={w.get('rules')} "
        f"brownouts={w.get('brownouts')} checksum={w.get('checksum')}"
    )
    return "\n".join(out)


def phase_table(path):
    d = load(path)
    if "error" in d:
        return d["error"]
    rows = ["phase            share%   p50 ms     p99 ms     mean ms     n"]
    for p in d.get("phases", []):
        dist = p.get("dist", {})
        share = p.get("share_permille", 0)
        name = p.get("phase", "?")
        if p.get("substituted"):
            name = f"{name}*"
        rows.append(
            f"{name:<15} {share / 10:>6.1f} "
            f"{str(dist.get('p50')):>10} {str(dist.get('p99')):>10} "
            f"{str(dist.get('mean')):>11} {str(dist.get('n')):>6}"
        )
    rows.append(
        "* SUBSTITUTED: a calibrated busy loop reproducing G2's measured repath, "
        "cluster-repair and CSR-rebuild costs. Not work this spike performed."
    )
    return "\n".join(rows)


def cost_summary(path):
    d = load(path)
    if "error" in d:
        return d["error"]
    f = d.get("fit", {})
    out = [
        f"os={d.get('os')} ticks_per_point={d.get('ticks_per_point')} "
        f"warmup={d.get('warmup')} repeat={d.get('repeat')} seats={d.get('seats')} "
        f"overflow_checks={d.get('overflow_checks')} pin={d.get('pin')} "
        f"burst=1-in-{d.get('burst_one_in_PLACEHOLDER')} "
        f"calibration={d.get('calibration_ps_per_iter')} ps/iter",
        f"model: {f.get('model')}",
        f"ns per unit per tick: {f.get('ns_per_unit_per_tick_20edits')} at 20 edits/s, "
        f"a0={f.get('ns_per_unit_per_tick_0edits_a0')} with destruction off, "
        f"d={f.get('ns_per_unit_per_edit_per_s_d')} per unit per edit/s "
        f"(r2 {f.get('r2_units')} / {f.get('r2_units_quiet')})",
        f"ns per beacon per tick: resolved={f.get('ns_per_beacon_resolved')} "
        f"OLS {f.get('ns_per_beacon_per_tick_beta')} at r2 {f.get('r2_beacons_quiet')}, "
        f"bracket {f.get('ns_per_beacon_bracket')} from the consecutive-pair slopes. "
        f"It cancels out of the power budget and is not added there.",
        f"ns per CRATER, G2's pathing consequence (SUBSTITUTED): "
        f"{f.get('ns_per_crater_pathing_consequence_at_300_units_SUBSTITUTED')} at 300 units "
        f"= {f.get('ns_per_crater_repair_unit_free_SUBSTITUTED')} repair/unit-free + "
        f"{f.get('ns_per_crater_repath_per_unit_SUBSTITUTED')} repath per unit "
        f"(r2 {f.get('r2_edits')})",
        f"ns per voxel edit, chunk store (MEASURED, the `voxels` phase): "
        f"{f.get('ns_per_voxel_edit_chunk_store_MEASURED')} — this is the cost of "
        f"applying a crater; the line above is its pathing consequence",
        f"ns per decision tick per seat: {f.get('ns_per_decision_tick_per_seat')}",
        f"fixed per-tick term: {f.get('fixed_overhead_ms')} ms = "
        f"{f.get('fixed_overhead_ms_sim_own')} the sim's own (incl. 40 beacons) + "
        f"{f.get('charged_csr_rebuild_ms_from_G2')} G2's CSR rebuild, charged every tick",
        f"unit cost shape (0 edits/s, the verdict): {f.get('unit_cost_shape')} "
        f"(quadratic share at 600 units {f.get('superlinear_share_at_600_units_0edits')}); "
        f"broadphase: {f.get('broadphase_cost_shape')} "
        f"({f.get('superlinear_share_at_600_units_broadphase')}); "
        f"at 20 edits/s, where ~98% is the linear-by-construction G2 charge: "
        f"{f.get('unit_cost_shape_20edits')} "
        f"({f.get('superlinear_share_at_600_units_20edits_substitute_check')})",
        f"quiet linear-fit residuals (ns): {f.get('quiet_linear_residuals_ns')}; "
        f"marginal ns/unit {f.get('quiet_marginal_ns_per_unit')}; "
        f"broadphase marginal ns/unit {f.get('broadphase_marginal_ns_per_unit')}",
    ]
    b = d.get("power_budget", {})
    out.append("--- step 5, every input named ---")
    out.append(
        f"budget p99={b.get('budget_p99_ms')} ms mean={b.get('budget_mean_ms')} ms "
        f"reserve={b.get('reserve_percent_PLACEHOLDER')}% (PLACEHOLDER) -> "
        f"effective p99={b.get('effective_p99_ms')} mean={b.get('effective_mean_ms')} ms"
    )
    out.append(
        f"fixed={b.get('fixed_overhead_ms')} ms "
        f"(= {b.get('fixed_overhead_ms_sim_own')} sim's own incl. 40 beacons "
        f"+ {b.get('charged_csr_rebuild_ms_from_G2')} G2's CSR rebuild, SUBSTITUTED) "
        f"unit-free craters@20/s={b.get('unit_free_crater_cost_ms_at_20_per_s')} ms "
        f"per unit {b.get('per_unit_ms_at_20_edits')} ms at 20 edits/s vs "
        f"{b.get('per_unit_ms_at_0_edits')} ms with destruction off"
    )
    out.append(
        f"MEAN constraint -> affordable units = {b.get('affordable_units')} "
        f"(0/s: {b.get('affordable_units_at_0_edits')}, "
        f"10/s: {b.get('affordable_units_at_10_edits')}, "
        f"40/s: {b.get('affordable_units_at_40_edits')})"
    )
    out.append(
        f"P99 constraint -> max per-tick repath cap = "
        f"{b.get('max_repath_cap_under_p99')} (floor {b.get('p99_floor_ms')} ms, "
        f"{b.get('repath_ms_each_from_G2')} ms per repath from G2); "
        f"STABILITY floor on the cap = {b.get('sustainable_repath_cap_floor')} "
        f"(mean demand — a cap below this never drains the queue)"
    )
    out.append(
        f"kW per unit={b.get('kw_per_unit_PLACEHOLDER')} (PLACEHOLDER, Tuning) -> "
        f"PROVISIONAL PER-MAP POWER BUDGET = {b.get('per_map_kw_budget')} kW"
    )
    out.append("--- repath cap sweep (the lever) ---")
    for p in d.get("sweep_repath_cap", []):
        # `str(...)` before the format spec, not `{x:>9}`: a missing or null
        # field would otherwise raise TypeError, and this step is diagnostics —
        # diagnostics must not be the thing that fails the job.
        flag = "" if p.get("sustainable", True) else "  UNSUSTAINABLE (backlog never drains)"
        out.append(
            f"cap={str(p.get('repath_cap')):>9}  mean={p.get('mean_ms')} ms  "
            f"p99={p.get('p99_ms')} ms  pathing={get(p, 'phases_ms', 'pathing')} ms  "
            f"backlog_mean={p.get('repath_backlog_mean')}  "
            f"served={p.get('repath_served_fraction')}{flag}"
        )
    out.append("--- per-seat reading (plan §1's open question) ---")
    for p in d.get("per_seat_reading", []):
        out.append(
            f"units={p.get('units')} beacons={p.get('beacons')}  "
            f"mean={p.get('mean_ms')} ms  p99={p.get('p99_ms')} ms"
        )
    return "\n".join(out)


def hashes_summary(path):
    try:
        with open(path, errors="replace") as fh:
            lines = fh.read().splitlines()
    except OSError as exc:
        return f"(missing: {exc})"
    if not lines:
        return "(empty)"
    return (
        f"{len(lines)} per-tick hashes\nfirst: {lines[0]}\nlast:  {lines[-1]}\n"
        + "\n".join(lines[:5])
    )


def listing(path):
    """A directory listing, in pure Python (the G2 note: never shell out here)."""
    try:
        entries = sorted(os.scandir(path), key=lambda e: e.name)
        return "\n".join(f"{e.name} {e.stat().st_size}" for e in entries)
    except OSError as exc:
        return f"(missing: {exc})"


def main():
    sections = [
        ("tick", tick_summary("out/tick.json")),
        ("phases", phase_table("out/tick.json")),
        ("tick-incremental", tick_summary("out/tick-incremental.json")),
        ("cost-model", cost_summary("out/cost.json")),
        ("hashes", hashes_summary("out/hashes.txt")),
        ("out-dir", listing("out")),
    ]
    for title, text in sections:
        emit(title, text)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a") as fh:
            for title, text in sections:
                fh.write(f"### {title}\n```\n{text}\n```\n")


if __name__ == "__main__":
    # Belt and braces for the contract in this module's docstring: a formatting
    # slip over a truncated or older JSON must not be the thing that fails the
    # job. The workflow gives this step no `continue-on-error`, so the guard
    # lives here.
    try:
        main()
    except Exception as exc:  # noqa: BLE001 - diagnostics must not fail the job
        print(f"::warning title=annotate::diagnostics failed: {exc!r}")

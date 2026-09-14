# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""Publish the G2 job's diagnostics where a logged-out viewer can read them.

GitHub hides job logs *and* step summaries from viewers who are not signed in,
but shows annotations. So the bench and accuracy JSON summaries and the path
hash digest go out as `::notice::` workflow commands, in chunks under the
per-annotation size cap, with newlines encoded as `%0A`. The same text is
appended to the step summary for anyone who is signed in.

Encoding rules copied verbatim from the G1 spike's `ci/annotate.py`
(`spikes/README.md` rule 2 allows copying between spikes); what differs is only
which files are read and how they are summarised.

Run from the spike directory after the bench and accuracy steps:

    python ci/annotate.py

`python`, not `python3`: actions/setup-python guarantees `python` on both
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


def read(path, tail=None):
    try:
        with open(path, errors="replace") as fh:
            lines = fh.read().splitlines()
    except OSError as exc:
        return f"(missing: {exc})"
    if tail:
        lines = lines[-tail:]
    return "\n".join(lines)


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


def dist(d, key):
    v = d.get(key)
    if not isinstance(v, dict):
        return f"{key}: (missing)"
    return (
        f"{key}: n={v.get('n')} p50={v.get('p50')} p90={v.get('p90')} "
        f"p99={v.get('p99')} max={v.get('max')}"
    )


def bench_summary(path):
    d = load(path)
    if "error" in d:
        return d["error"]
    out = [
        f"cluster={d.get('cluster_size')} units={d.get('units')} "
        f"ticks={d.get('ticks')} craters={d.get('craters')}",
    ]
    m = d.get("map", {})
    out.append(
        f"largest component {m.get('largest_component_before')} -> "
        f"{m.get('largest_component_after')}; components "
        f"{m.get('components_before')} -> {m.get('components_after')}; "
        f"directed steps {m.get('directed_steps_before')} -> "
        f"{m.get('directed_steps_after')}"
    )
    out.append(
        f"walkable {m.get('walkable_before')} -> {m.get('walkable_after')} "
        f"(total by construction={m.get('walkable_is_total_by_construction')}); "
        f"overhangs={m.get('overhangs_possible')}"
    )
    g = d.get("graph", {})
    out.append(
        f"abstract nodes={g.get('abstract_nodes')} -> "
        f"{g.get('abstract_nodes_after_destruction')} directed edges="
        f"{g.get('abstract_edges_directed')} clusters={g.get('clusters')} "
        f"build={g.get('build_ms')} ms"
    )
    mem = d.get("memory", {})
    out.append(
        f"memory structures={mem.get('structures_kib')} KiB "
        f"peak heap={mem.get('peak_heap_kib')} KiB graph={mem.get('graph_bytes')} B"
    )
    rep = d.get("repeat", {})
    out.append(
        f"repeat runs={rep.get('runs')} median_run={rep.get('median_run')} "
        f"cold p99 {rep.get('repath_cold_p99_min_ms')}..{rep.get('repath_cold_p99_max_ms')} ms "
        f"est p99 {rep.get('estimate_p99_min_ms')}..{rep.get('estimate_p99_max_ms')} ms "
        f"same routes={rep.get('route_digest_identical_across_runs')} "
        f"digest={rep.get('route_digest')}"
    )
    r = d.get("repath", {})
    out.append(
        f"repaths={r.get('repaths')} no_path={r.get('no_path_results')} "
        f"(unreachable={r.get('no_path_genuinely_unreachable')}) "
        f"idempotence checks={r.get('idempotence_checks')}"
    )
    out.append("G2-a cold (first repath after a rebuild) " + dist(r, "cold_ms"))
    out.append("warm (same graph) " + dist(r, "warm_ms"))
    out.append(dist(r, "all_searches_ms"))
    out.append("oracle short-circuit " + dist(r, "no_path_ms"))
    out.append(dist(r, "cold_plus_repair_share_ms"))
    quartiles = r.get("cold_by_run_quartile_ms")
    if isinstance(quartiles, list):
        out.append(
            "cold by run quartile p50/p99: "
            + " | ".join(f"{q.get('p50')}/{q.get('p99')}" for q in quartiles)
        )
    out.append(
        f"peak repaths/game-second={r.get('peak_repaths_per_game_second')} "
        f"wall repaths/s={r.get('wall_repaths_per_second')}"
    )
    rp = d.get("repair", {})
    out.append(dist(rp, "per_dirtied_cluster_ms"))
    out.append(dist(rp, "graph_rebuild_ms"))
    e = d.get("estimate", {})
    out.append("P3-a " + dist(e, "static_ms"))
    out.append(dist(e, "cold_after_repair_ms"))
    out.append("levels evidence " + dist(e, "abstract_expansions"))
    out.append("levels evidence " + dist(e, "low_expansions"))
    dy = d.get("dynamic_terrain_error", {})
    out.append("dynamic " + dist(dy, "abs_pct"))
    a = d.get("allocations", {})
    out.append(
        f"allocs/query (milli): estimate={a.get('estimate_allocs_per_query_milli')} "
        f"path={a.get('path_allocs_per_query_milli')} intra={a.get('intra_mode')}"
    )
    return "\n".join(out)


def accuracy_summary(path):
    d = load(path)
    if "error" in d:
        return d["error"]
    out = [f"cluster={d.get('cluster_size')} pairs={d.get('pairs_measured')}"]
    s2 = d.get("step2_astar_vs_dijkstra", {})
    out.append(
        f"step2 A* vs Dijkstra: compared={s2.get('pairs_compared')} "
        f"mismatches={s2.get('mismatches')}"
    )
    s3 = d.get("step3_path_quality", {})
    out.append("step3 " + dist(s3, "hpa_excess_pct"))
    out.append("step3 " + dist(s3, "abstract_excess_pct"))
    s6 = d.get("step6_estimate_accuracy", {})
    out.append("P3-b " + dist(s6, "vs_walked_abs_pct"))
    out.append("P3-b signed " + dist(s6, "vs_walked_signed_pct"))
    out.append("step6 " + dist(s6, "vs_optimal_signed_pct"))
    for band in ("le32", "32_96", "96_256", "gt256"):
        out.append("step6 " + dist(s6, f"vs_walked_{band}_pct"))
    s7 = d.get("step7_fog", {})
    out.append("step7 fog " + dist(s7, "vs_walked_abs_pct"))
    return "\n".join(out)


def paths_summary(path):
    text = read(path)
    if text.startswith("(missing"):
        return text
    lines = text.splitlines()
    digest = [ln for ln in lines if ln.startswith("digest ")]
    nopath = sum(1 for ln in lines if ln == "nopath")
    fingerprints = sum(1 for ln in lines if ln.startswith("fp "))
    markers = sum(1 for ln in lines if ln in ("static", "repaired"))
    hashed = len(lines) - len(digest) - fingerprints - markers
    return (
        f"{hashed} path hashes ({nopath} unreachable) and {fingerprints} "
        f"repaired-graph fingerprints\n"
        + ("\n".join(digest) if digest else "(no digest line)")
        + "\nfirst 5:\n"
        + "\n".join(lines[:5])
    )


def listing(path):
    """A directory listing, in pure Python.

    This used to shell out to `ls -la`, which only ever worked because the step
    runs under Git bash on the Windows leg; an unresolvable `ls` would have
    raised FileNotFoundError and taken the whole job down for the sake of a
    directory listing.
    """
    try:
        entries = sorted(os.scandir(path), key=lambda e: e.name)
        return "\n".join(f"{e.name} {e.stat().st_size}" for e in entries)
    except OSError as exc:
        return f"(missing: {exc})"


def main():
    sections = [
        ("bench", bench_summary("out/bench.json")),
        ("accuracy", accuracy_summary("out/accuracy.json")),
        ("paths", paths_summary("out/paths.txt")),
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
    main()

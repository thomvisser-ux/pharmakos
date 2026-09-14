# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""Publish the geometry job's diagnostics where a logged-out viewer can read them.

GitHub hides job logs *and* step summaries from viewers who are not signed in,
but shows annotations. So the compare script's verdict, the `out/` listing,
`vulkaninfo --summary` and the tail of the Godot log go out as `::notice::`
workflow commands, in chunks under the per-annotation size cap, with newlines
encoded as `%0A`. The same text is appended to the step summary for anyone who
is signed in. Run from the spike directory after the render and compare steps:

    python3 ci/annotate.py
"""

import os
import subprocess

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


def main():
    listing = subprocess.run(["ls", "-la", "out"], capture_output=True, text=True).stdout
    log = read("out/godot.log")
    renderer = "\n".join(
        ln for ln in log.splitlines()
        if any(k in ln for k in ("Vulkan", "OpenGL", "Using Device", "Device:", "GDExtension", "godot-rust", "G1World"))
    )
    sections = [
        ("compare_vista", read("out/compare.txt")),
        ("renderer-and-extension", renderer),
        ("out-dir", listing),
        ("vulkaninfo", read("out/vulkaninfo.txt", 40)),
        ("godot.log", read("out/godot.log", 160)),
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

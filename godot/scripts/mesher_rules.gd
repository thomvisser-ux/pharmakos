# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The two rules-table rows the client reads, as canonical `gp.v1.RulesTable` JSON: the
# mesher row - item 54's drain budget (K surfaces and B bytes a frame, and the ageing
# term) and the light bake's maximum and attenuation - and `match.lull_ms`, the Lull's
# planning timer, which the rules table says is "read by the host and the editor, never by
# the sim" and which the watch rig counts down (the gateway shows what the client reports).
#
# Inline rather than read from `rules/rules.v1.json`, because a `res://` path cannot
# escape the project folder and the rules table lives at the repository root; no gateway
# method serves it. The bridge still parses it through the schema.
#
# Being a COPY of a tuning row, it is pinned, and it is the only copy in godot/:
# `crates/client-gdext/tests/godot_project.rs::the_inline_rules_table_is_the_committed_one`
# parses this very literal and compares both rows with the committed table, so the
# copy cannot drift in silence (AGENTS.md section 12). `revision` is deliberately not
# compared: it is the whole table's version and moves whenever any lane adds a row
# anywhere in it.
#
# PLACEHOLDER: where a shipped client reads the rules table from - packaging's question
# with the host's own rules file; OWNER, at T21.

extends RefCounted

const RULES_JSON := '{"revision":1,"mesher":{"surfacesPerFrame":4,"bytesPerFrame":524288,"ageFrames":2,"lightMax":15,"lightAtten":1},"match":{"lullMs":180000}}'

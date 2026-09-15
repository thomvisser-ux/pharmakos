<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `interpreter/` — decision and step transcripts

**Filled by T11** (`crates/sim`: the playbook interpreter at the v1 vocabulary).

One committed playbook per vocabulary construct, each with a pinned transcript
of decisions (every 250 ms of game time), step transitions, and the points at
which an interface row commits.

## What a diff means

* **Execution order changed.** One rule body at a time, no pre-emption, and the
  fixed 20 % reflex as the only interrupt. A transcript that reorders is either
  a real rule change or a scheduling accident; there is no third option.
* **A commit point moved.** Interface rows commit at the end of their own
  durations, at spec §5's rates, and a resumed visit pays the handshake again. A
  moved commit point usually means a rate moved in the rules table — say which
  row.
* **The reflex fires more or less often.** It is not disableable, it aborts a
  visit, it walks to the safest own beacon, and it re-fires only after *new*
  damage. A transcript with two consecutive reflexes and no damage between them
  is a bug, not a golden to bless.
* **A step that used to time out now does not.** Every `wait` has a timeout and
  every jump goes forward only. A transcript change here is a termination
  property changing, which is the one thing `every_playbook_halts` exists to
  hold.

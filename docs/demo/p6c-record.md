<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# P6, section 5.3's closing rule: the record

This is the record of the P6 editor timing gate as `docs/design/s3-plan.md` section 5.3's closing rule
reads it (decisions-log item 163). **It is blank until the main session runs the rule** on the zip below,
by computer control, and fills every cell. Every cell is **AI-driven**: the evidence shows that the
controls work and that the steps fit the limits when driven without hesitation. **It does not show a
person's time.** The owner rules at S3's stage demo review whether S3 closes on it (section 1.2 row 8);
AGENTS.md section 10 item 6's P6 part is met only on that ruling.

## The build

- The zip: `D:/build/demo3/pharmakos-s3-a8aa864a-windows-x86_64.zip`, 120 357 327 bytes, 27 entries,
  SHA-256 `12b47f3f1c220873ebcca2212f023927e6a8689a5e98ba15c4139321d74d2a08`, built by `cargo xtask
  package` from `main` at `a8aa864a88f2a7ef7129d36d381c994b58502c8e` (the demo's tree), rules hash `641055eb0d38815a` (rules revision 8, the
  final rules). It holds `fixp` (#211), on which the typing clause is read.
- The match: the lobby's **Probation** preset ("New match: Probation"), seat 0 against Easy on seed 20,
  untimed Lulls. Started as `Pharmakos.exe` (the rule needs round 1 only).
- A fresh `user://` before each timing; never "Resume last match".

## The clock

The audit log's seat-0 `submit_plan` tick, at 20 Hz, from the tick round 1's Lull opened, in the match's
cache (`%LOCALAPPDATA%\Pharmakos\matches\<match-id>\`). The status line must read "Submitted. This is
now your sealed order." with a verdict that qualifies. A verifier refusal counts inside the time.

## The template timing: at most 2 minutes

From round 1's Lull opening to "Submitted. This is now your sealed order." with a qualifying verdict,
from any template but the Safe playbook.

| Cell | Value |
|---|---|
| Match id | |
| Template taken | |
| Values changed in the wizard (each sent with "Send this value") | |
| Lull opened (audit tick) | |
| Seat 0's `submit_plan` (audit tick) | |
| Time (ticks / 20, seconds) | |
| Verdict at the submit (qualifies; codes shown) | |
| Each refusal seen, with its words | |
| Each control that did not work as the run sheet says | |
| Within 2 minutes? | |

## The scratch timing: at most 5 minutes

Ten route steps added to the empty playbook that "New" puts on screen, from `docs/demo/p6-task-card-a3.md`,
which the driver opens for the first time when the watch starts (at P6b, card A2 was read about 15 s early;
the rule forbids that).

| Cell | Value |
|---|---|
| Match id | |
| When the card was first opened (against the Lull's opening) | |
| Lull opened (audit tick) | |
| Seat 0's `submit_plan` (audit tick) | |
| Time (ticks / 20, seconds) | |
| Verdict at the submit (qualifies; codes shown) | |
| Each refusal seen, with its words | |
| Each control that did not work as the card says | |
| The sealed route against the card: ten steps, each of the card's kind in its order (read from `seats\0\sealed\1.jsonc`) | |
| Step 8: the description, or a vent's name (say which) | |
| Within 5 minutes? | |

## The typing clause (on the `fixp` build)

Untimed. The driver changes three number chips by typing as a person would: clicking the field, typing
and pressing "Set this value", with **no Enter and no select-all**. Each chip must read the new number;
anything else is a miss.

| Chip (rule or step, and the field) | Number before | Number typed | Chip reads after | Read the new number? |
|---|---|---|---|---|
| 1 | | | | |
| 2 | | | | |
| 3 | | | | |

## The result

| Cell | Value |
|---|---|
| Template timing | |
| Scratch timing | |
| Typing clause | |
| A miss? (a time over its limit, or any control the card needs that did not work as it says) | |
| Driven by | the main AI session, by computer control (AI-driven; not a person's time) |
| The owner's ruling at S3's stage demo review: does S3 close on this evidence? | |

A miss opens a short added lane before S3 closes; the clause is read again on that lane's build, and
AGENTS.md section 10 item 6 is not met until then (section 5.3). The AI review is not independent review.

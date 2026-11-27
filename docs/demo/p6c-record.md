<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# P6, section 5.3's closing rule: the record

This is the record of the P6 editor timing gate as `docs/design/s3-plan.md` section 5.3's closing rule
reads it (decisions-log item 163). The main session ran the rule on 2026-11-27 on the zip below, by
computer control, and filled every cell. Every cell is **AI-driven**: the evidence shows that the
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

**How it was driven.** Driven by the main AI session by computer control on the zip the record names
(`pharmakos-s3-a8aa864a-windows-x86_64.zip`, extracted to `D:/build/p6b/play/Pharmakos/`), the
lobby's Probation preset, a fresh `user://` before each of the three reads, never "Resume last
match". The gate was read once: no timing was repeated. Every audit tick below is from
`%LOCALAPPDATA%\Pharmakos\matches\<match-id>\audit.log`; round 1's Lull opens at audit tick 0 in
each match. The template timing was read on the primary monitor, under the driving application's
window (below); the scratch timing and the typing clause with the game's window moved to the second
monitor (same size, maximised), by a script that moves a window and touches nothing in the game.

## The clock

The audit log's seat-0 `submit_plan` tick, at 20 Hz, from the tick round 1's Lull opened, in the match's
cache (`%LOCALAPPDATA%\Pharmakos\matches\<match-id>\`). The status line must read "Submitted. This is
now your sealed order." with a verdict that qualifies. A verifier refusal counts inside the time.

## The template timing: at most 2 minutes

From round 1's Lull opening to "Submitted. This is now your sealed order." with a qualifying verdict,
from any template but the Safe playbook.

| Cell | Value |
|---|---|
| Match id | `local-1795806673-12292` |
| Template taken | Hold & Build (the one the wizard marks "Start here") |
| Values changed in the wizard (each sent with "Send this value") | none; "Use this playbook" took the wizard's suggested values as shown (the vent `vent_175_66`, hold 82900 ms) |
| Lull opened (audit tick) | 0 |
| Seat 0's `submit_plan` (audit tick) | 1700 (the first; the same match's log holds three later `submit_plan` lines, at ticks 2914, 4028 and 4550, which are the driver's resubmits of the same playbook while it diagnosed the lost clicks, after the timing was over) |
| Time (ticks / 20, seconds) | 1700 / 20 = **85.0 s** |
| Verdict at the submit (qualifies; codes shown) | "Submitted. This is now your sealed order." and "Checked at submission - Ready to seal." (qualifies; before the submit the line read "Full check - Ready to seal."; the size meter read "6 of 128 size units"; the only row seen under Checks was a note, I0003) |
| Each refusal seen, with its words | none from the game or the gateway: every seat-0 call on the editor's handle answered `ok` |
| Each control that did not work as the run sheet says | **None of the game's.** Three clicks on "Submit" (and, in practice runs, other clicks in the right panel) did nothing; a fourth, sent after an Escape key press, submitted. The cause was found and is NOT the game: the clicks never reached the game's window. They landed on the driving application's own always-on-top window (process `claude`, 536 x 728 pixels at the top right of the primary monitor, screen rectangle (1372,20)-(1908,748), which the screenshots the driver sees do not show), which lay over the game's right panel. Evidence: (a) a later diagnostic match started with the game's own `--log-input` shows every delivered mouse button at the lobby's `_input`, and the lost clicks have no line at all, while a click on the left strip in the same state has its line; (b) `WindowFromPoint` at the Submit button's screen point named that window, not the game's; (c) with the game's window moved to the second monitor, away from it, every click of the scratch timing and the typing clause registered first time. The 85.0 s includes the three lost clicks and the round trips spent on them (about 45 s of it). |
| Within 2 minutes? | **yes** (85.0 s) |

## The scratch timing: at most 5 minutes

Ten route steps added to the empty playbook that "New" puts on screen, from `docs/demo/p6-task-card-a3.md`,
which the driver opens for the first time when the watch starts (at P6b, card A2 was read about 15 s early;
the rule forbids that).

| Cell | Value |
|---|---|
| Match id | `local-1795807303-12896` |
| When the card was first opened (against the Lull's opening) | after the watch started: about 10 s after the Lull opened (the driver clicked "New match: Probation", then "New", took one screenshot, then opened `docs/demo/p6-task-card-a3.md` for the first time). It had not been opened, read or described to the driver before (the builder's report named only the file). |
| Lull opened (audit tick) | 0 |
| Seat 0's `submit_plan` (audit tick) | 1923 |
| Time (ticks / 20, seconds) | 1923 / 20 = **96.15 s** |
| Verdict at the submit (qualifies; codes shown) | "Submitted. This is now your sealed order." and "Checked at submission - Ready to seal." (qualifies; before the submit "Full check - Ready to seal."; "10 of 128 size units"; the rows under Checks were notes: I0003 rows with their "Fix: Write the default out" buttons, and the walking-time note) |
| Each refusal seen, with its words | none: all thirteen `patch_plan` calls and the one `submit_plan` answered `ok`; no step was refused |
| Each control that did not work as the card says | none. Every menu opened on its first click and every row took. One difference from the card's wording, not a failure: a plain click on the vent opened the two-row menu with "Place beacon covering this vent" FIRST and "Place beacon covering the nearest vent you can cover" SECOND (the card names them in the other order); the driver took the second row, as the card asks, and the control worked. The main session's reading: the card's wording, not the control, differed, so no miss; the owner can veto. |
| The sealed route against the card: ten steps, each of the card's kind in its order (read from `seats\0\sealed\1.jsonc`) | ten steps, each of the card's kind in its order, read from `seats\0\sealed\1.jsonc`: (1) `place_1` place_beacon at voxel (201, 25, 28); (2) `go_1` move to voxel (200, 15, 28); (3) `visit_1` interface `b_00`, `set_priority` HIGH; (4) `go_2` move to voxel (186, 13, 28); (5) `go_3` move to `b_00`'s anchor; (6) `place_2` place_beacon at voxel (172, 25, 28); (7) `go_4` move to voxel (199, 32, 28); (8) `place_3` place_beacon at `covering` `vent` `{"rank":"NEAREST","coverage":"UNCOVERED"}`; (9) `go_5` move to `b_00`'s anchor; (10) `visit_2` interface `b_00`, `set_priority` NORMAL. After the submit the driver pressed "Ready - end the Lull" so that the sealed file was written; the strip then read "The playbook of seat 0 was sealed: 10 route steps." |
| Step 8: the description, or a vent's name (say which) | **the description** (`covering` the NEAREST UNCOVERED vent), not a vent's name |
| Within 5 minutes? | **yes** (96.15 s) |

**The card was corrected after the read.** Its step 8 named the vent menu's two rows in the other order
from the one the menu shows; after the read the card's step 8 was changed to name them in the menu's
order ("Place beacon covering this vent" first). The row the card asks for is unchanged, and the read
above was made on the card as it then stood.

## The typing clause (on the `fixp` build)

Untimed. The driver changes three number chips by typing as a person would: clicking the field, typing
and pressing "Set this value", with **no Enter and no select-all**. Each chip must read the new number;
anything else is a miss.

Match `local-1795807450-12956`, the Defend the Core template's playbook ("Use this playbook"),
its rule list under "What the playbook says". Each chip: one click on the chip, one click in the
popup's field, the digits typed, one click on "Set this value". No Enter, no select-all, no
Backspace. Three `set_value_text` calls, each `ok`.

| Chip (rule or step, and the field) | Number before | Number typed | Chip reads after | Read the new number? |
|---|---|---|---|---|
| 1. Step 2 `place_guard`, the Defend mandate's "engaging within N % of the sphere" (the popup: "The gateway's range here: 1 to 100 %.") | 100 | 80 | "engaging within 80% of the sphere" | yes |
| 2. Step 2 `place_guard`, "with a picket of N scouts" (the popup: "The gateway's range here: 0 to 255. 0 here: no picket.") | 1 | 3 | "with a picket of 3 scouts" | yes |
| 3. Step 3 `hold`, "hold for N s" (the popup: "The gateway's range here: 0.001 to - s.") | 97.6 | 45 | "hold for 45 s" | yes |

## The result

| Cell | Value |
|---|---|
| Template timing | 85.0 s of 120 s: within |
| Scratch timing | 96.15 s of 300 s: within |
| Typing clause | three of three chips read the new number |
| A miss? (a time over its limit, or any control the card needs that did not work as it says) | **no**, on the main session's reading: both times are within their limits, and every control the card needs worked; the card's wording of the vent menu's row order, not the control, differed, so no miss (the owner can veto) |
| Driven by | the main AI session, by computer control (AI-driven; not a person's time) |
| The owner's ruling at S3's stage demo review: does S3 close on this evidence? | |

A miss opens a short added lane before S3 closes; the clause is read again on that lane's build, and
AGENTS.md section 10 item 6 is not met until then (section 5.3). The AI review is not independent review.

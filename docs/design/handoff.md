# Handoff: PHARMAKOS (codename AirGap) design spec — state as of 2026-09-13, end of pass 6

## Where things stand
- **Title:** PHARMAKOS (working lockup "PHARMAKOS: THE SEALED ORDER"). AirGap = codename and the in-world term for the trust model. Clearance sweeps done (see decisions-log §2.7 item 9). To do off-spec: register pharmakosgame.com, pharmakos.io/.dev, @pharmakosgame handles, a GitHub org; professional clearance search during hardening, before v1 (item 42).
- **Spec draft 6 (v0.6) is PUBLISHED** at https://claude.ai/code/artifact/601b8895-7737-4196-bafe-3c58d7c6e67a (label "Spec v0.6 draft", favicon 🗝️). Source: `pharmakos-spec.html`. To update from a new session: republish `pharmakos-spec.html` passing that `url`. `airgap-spec.html` is the draft-5 archive.
- **Draft-6 review (post-publish):** a six-lens workflow reviewed v0.6, verified findings, and applied the confirmed editorial fixes; see the "Draft 6 review result" line at the bottom of this file. Any *decision* items it raised are listed there for the owner.
- **Pass 6 interview is COMPLETE (39 items)**, logged in `decisions-log.md` **§2.7** (overrides 2.6 and everything earlier; precedence 2.7 > 2.6 > 2.5 > 2.4 > 2.3 > 2.2 > 2.0 > 2.1). Two wordings inside 2.7 are superseded by later items in the same section: item 7's "v0.1 at about wk 19" → item 31 (v0.1 = the gated build at wk 35.5); item 15's "nightly league" → item 34 (three nightly adversarial scenarios).

## The design in one paragraph (v0.6)
v1 is simplified: everything that executes is Rust; everything the player touches is data. Three levels of built-in behaviour: the commander's built-in **operator** (generates a playbook with the "rudimentary" templates + scoring tool, files the safe playbook on a miss, executes any submitted playbook), built-in **mandates** on beacons (Build, Defend, Attack, Mine, Survey), built-in **programs** for units and buildings. The human edits the **playbook** in the Godot editor; manual control is deferred. Playbooks and templates are **JSONC** files (canonical proto JSON `gp.v1` + comments). **No SDK, script language, script runtime, published API, MCP or LLM seats in v1**: v1.1 = scripting API (TypeScript-family scripts run externally; ~3 wk), v1.2 = live conduit (MCP, interactive Claude Code/Codex sessions; ~5 wk); reserved proto seams now (`operator|mandate|program {builtin|script}`, `author_kind SCRIPT`, plan fingerprint). The Seat Gateway (JSON-RPC over localhost WebSocket, per-seat tokens) stays internal and agent-shaped. Sim: Rust, deterministic, 20 Hz integer maths; Godot 4.7 via gdext; protobuf single source. Security constraints unchanged (localhost only, per-seat tokens, no bypass-permissions mode, the game never holds keys). The theme: pharmakon/pharmakos = remedy and poison; trust is engineered, not assumed; the commander is the physical courier of the trust chain ("touch to change"); radio is untrusted data; over-the-air interception/spoofing/jamming is a roadmap assumption.

## Build plan (63.5 wk, ~18 months with buffer, ~March 2028; ship-v1 confidence ~78% → ~85% after the gate)
spikes 3 (wk 3) → walking skeleton 15.5 (18.5) → S1 economy 5 (23.5) → S2 destruction & combat 6 (29.5) → S3 authoring 5 (34.5) → **gate + v0.1 public release 1 (35.5)** → S4 radio, licences, symmetric map 6 (41.5) → S5 operators 4 (45.5) → S6 editor & library 7.5 (53) → S7 watch & share 4.5 (57.5) → hardening 6 (**v1 at 63.5**). Packaging ~4 wk in total (1 before v0.1, 3 in hardening; macOS on GitHub-hosted runners + Apple Developer ID; Windows via SignPath or an OV cert). Next step per the spec: start the stack spikes (G4, G1), write harness part 1, register the name.

## Key pass-6 rules (details in §2.7)
- Power: the beacon is the unit of power (dormancy powers down everything homed to it; seal/sphere/interfacing stay); core autocannon runs off the deep bore; priority is kW-only; one Generator per vent; revive with a margin.
- Sphere = the ball of voxels within a beacon's radius (Tuning); it is the only range for physical work. Reach = knowledge only. Enemy-overlap ban deleted. One Resonance Spire per beacon.
- Money: paid means yours (charge at commit, full value from then); value follows condition (build cost × HP) at the audit and the recycle refund; settlement (BMI + award fund) at each recap; one catch-up dial (BMI by rank on held value, living seats); kill credit split with the five edge cases and largest-remainder apportionment; tie-break order unsmoothed net worth → enemy value destroyed → fewer beacons lost → shared win.
- Match end checked every tick; survivor wins; none → the final audit at that tick; "Abandon match (no winner)". Segment ladder 3/4/5/6/7/8 min by round (host flat override), length in the snapshot and briefing. Latest verified submission wins during the Lull.
- Sightings: freshness at plan time only, target pinned at seal, age on game time. Licence triggers evaluated at settlement; blindsided scoped to beacons/licensed structures with reach memory N.
- Beacon death = local elimination (structures → neutral ruins; no unclaimed state). Commander: healed by repair drones, per-Push death counter, not an asset, speed a world constant. Finite seams; fabricators build only for work in hand. Everything walks; friendly fire on; breach = dig/demolish through.
- Plan execution: one rule body at a time; only the fixed 20% reflex interrupts; interface rows atomic. v1 vocabulary has no flags/branch/repeat (field numbers reserved); Load rejects with a diagnostic, never strips; last round's playbook pre-loads as a draft.
- Dispatches cut (one-way engine chatter from S5; player Dispatches v1.2). Fog is a per-match policy applied by the gateway; own score + rank live, full detail at recap. Notebook 4,000 characters, in saves and the secrecy list.
- Watch rig in the skeleton (one camera rig, event list, 2–4× speed, free skip, full map on elimination). One gate with written criteria and a fallback ladder; three nightly adversarial scenarios (Rusher/Turtle/Hunter) replace the league; `verify_plan{depth}`.
- Art: MagicaVoxel/Goxel → .vox → dot_vox → greedy mesher; emissive + baked flood-fill light. Audio: CC0/CC-BY only, REUSE manifest. English-only strings in one table. Placeholder look in v1.

## Files in the workspace (scratch; deleted with the session; offer to move them to a user folder)
| File | Contents |
|---|---|
| `pharmakos-spec.html` | **Draft 6 (v0.6), published** |
| `airgap-spec.html` | Draft 5 archive |
| `decisions-log.md` | All decisions; **§2.7 is current** |
| `review-agenda.md` | The 30-item draft-5 review agenda, 22 editorial fixes, moot and rejected lists |
| `6c-recommendation.md` | The 36-agent format/language investigation (TypeScript external, no embedded runtime; rebind; packaging findings) — now roadmap input for v1.1 |
| `design-gameplan-api.md` | 113 KB co-design doc; superseded wherever §2.7 conflicts (notably: preempt flag and handler stack, reflex_hp_pct, branch/repeat in v1, Dispatch field, stop_at_pool, 27 MCP tools) |
| `switchyard-spec.html` | Obsolete straw man; deletion offered, no answer |

## Conventions
- Interview one item at a time; recommendation first, marked "(Recommended)", with the downsides of every option; research technical questions before recommending; simpler is better; accounting is not fun; no artificial caps; custom/scripted depth goes to the roadmap. Never raise provider ToS concerns.
- Log each answer in decisions-log.md immediately (continue §2.7 or open §2.8 for the next pass).
- Spec design: Chakra Petch / Source Sans 3 / JetBrains Mono; accent #B5400F; light and dark tokens; sticky TOC; tags Decided / Draft N / Tuning / Roadmap / Risk (t-open and t-trial classes exist but are unused).
- Subagents: the user asked for Opus and/or Sonnet subagents when calling subagents (Opus for judgement, Sonnet for mechanical steps).

## Draft 6 review result (2026-09-13, 60-agent workflow) — RESOLVED
- 65 raw findings → 52 unique → 46 editorial (61 exact edits applied) and 6 decision items, answered in this session as decisions-log §2.7 **items 40–45** and applied to `pharmakos-spec.html` (14 text edits plus every cross-reference rewritten as a linked "section N"):
  40 per-round segment length list replaces the flat override (gate = [3,5,8]) · 41 chatter is ungated open-air talk, no Mast · 42 professional name search stays pre-v1 (hardening); the spec states the v0.1 rebrand exposure · 43 numbered previews v0.2–v0.5 at S4–S7 as unsigned zips from green CI · 44 both senses of "seal" kept, physical sense glossed in §5 and in the world-terms table, §2 reads "When a Stake falls…" · 45 "section N" links everywhere, no "§".
- A verification workflow (7 Opus checkers, 3 refuters per finding) ran after the edits; confirmed residue was applied before republishing.

## Build state (2026-09-13, end of session)
- **Repo:** `C:\Users\PC\pharmakos`, git on `main`, pushed to https://github.com/thomvisser-ux/pharmakos (public, personal account for now; an org `pharmakosgame` can take it later). Commits are DCO-signed (`git commit -s`). Cargo builds to `D:\build\cargo-target` through the user-level `~/.cargo/config.toml`; CI uses the default layout.
- **Toolchain installed:** rustup/cargo 1.98.1 (stable MSVC, pinned by rust-toolchain.toml), MSVC 14.43 + Windows 11 SDK 22621 (Build Tools 2022 with the C++ workload added via `vs_installer modify`), protoc 36.0, buf 1.73, Godot 4.7.2 (winget; `godot` on PATH through WinGet Links), cargo-deny. Not installed: the `reuse` tool (pipx) and cargo-nextest (unverified). New shells have cargo, protoc and WinGet Links on PATH; the Claude Code Bash tool needs `export PATH="$USERPROFILE/.cargo/bin:$LOCALAPPDATA/Microsoft/WinGet/Links:$PATH"`.
- **Disk:** about 7.7 GB free on C: after the installs; the owner is freeing space (RogueLauncherCache 39 GB and BATTLETECH 18.6 GB are the big folders).
- **Harness part 1 done and green:** `cargo xtask ci` passes (fmt, clippy with the determinism lints, profiles, test, test-research, research-guard, wall-guard, deny, buf lint; golden, determinism and reuse are skipped until they exist). All 9 workspace members compile; `buf build` and `protoc` accept the proto tree. Written by a 10-agent workflow (6 drafters, 3 reviewers, 1 fixer applying 48 findings), then 7 compile and clippy fixes by hand.
- **Contract decisions raised by the fixer, pending the owner:** (a) playbook durations are `int32` milliseconds rather than `int64`, so canonical proto JSON emits bare numbers as in the spec's section 10 example; (b) the envelope's `kind` tag is reserved (field 7) rather than defined, to be added with the skeleton's envelope work.
- A stray file named `nul` (a Windows reserved name, created by a redirect during the bootstrap) was deleted; git cannot index it.

## NEXT
1. Initial commit (owner's call), then spike G4 (docs/spikes/G4-determinism.md) and G1 (docs/spikes/G1-destruction-remesh.md) in the throwaway `spikes/` directory. Harness part 1 is in place, so CI is real from the first spike.
2. Register pharmakosgame.com, pharmakos.io/.dev, @pharmakosgame and a GitHub org; push the repo there.

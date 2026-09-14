# Decisions log (pre-spec interview)

Status: interview complete. Foundation committed, gated by Phase 0. Spec draft 1 published as airgap-spec.html; draft 2 comes next. See handoff.md.
Note: switchyard-spec.html is an old straw man (a Mindustry fork). It has been superseded; ignore it.

---

## 1. Decided

**Purpose and scope**
- **Purpose:** a playable game and an AI platform, with high spectacle.
- **Builder:** one developer plus AI coding agents.
- **Time model:** strategic. Turn/step and pausable modes at launch; continuous real-time is a long-term goal.
- **AI consumers:** LLM agents (MCP), RL agents (Gym/PettingZoo) and scripted bots, all on one core protocol.
- **Seats:** any seat can be a human or an AI; mixed matches are supported.

**World and content**
- **Voxels:** core identity, built in a sensible order. Terrain is destructible and height is capped.
- **Design sources:** distilled from Mindustry, Zero-K, Minecraft Legends, Timberborn and Dwarf Fortress into one coherent, original design.
- **First format:** sandbox/creative with a very basic attack/defend kit.

**Player presence and control**
- **Commander:** a weak, killable commander (details to come) whose main action is placing beacons.
- **Beacons and governors:** each beacon creates a sphere of influence run by a governor.
- **Default governor:** drafted mandates executed by a rudimentary built-in AI.
- **Key innovation:** full AI integration and customization. Any combination of automation, manual control, hotkeys, groupings, macros, scripts, LLM agents and RL policies is possible.

**Web access**
- Web spectating and replays in the browser. Playing requires the installed client.

**Sandbox attack/defend kit (v1)**
- Walls: basic + reinforced
- Turrets: autocannon (direct fire, beats raiders) + mortar (arcing shell you can dodge, area voxel damage)
- Raider drone + demolition charge (timed explosive that destroys a voxel sphere)
- Repair/reclaim drone + gate/drawbridge (first script-controllable block)
- Scout/sensor unit (cheap vision; basis for a Scout mandate)
- Shield/barrier block (absorbs projectiles; counters the mortar)

**Beacons**
- **Placement:** the first beacon goes anywhere in the spawn zone. Later beacons must be within the commander's range AND touch or overlap an existing own sphere.
- **Enemy overlap:** creates a contested zone where neither side can build or dig. Keeping units near an enemy beacon for N ticks captures it, and its sphere and structures flip to the captor.
- **Destroyed beacon:** governance collapses. Mandates suspend, and units revert to reflexes and retreat to the nearest friendly sphere. Structures remain but are no longer repaired and become unclaimed.
- **Claims in the sandbox:** no protections for now, so everything is attackable; mutual opt-in hostility is rejected. Protections (telegraphed attacks, newcomer shield, host rollback) will be revisited per game mode, since many modes are planned.

## 2. Foundation (not committed; target confidence 90–95% before committing)
Round-3 research is in progress: scripting runtime deep dive (user defers to the recommendation), Godot client and in-game editor UI, sim core and protocol verification, red-team of the full stack.
In-game editors (mandates/scripts/macros) are gameplay UI. Browser access stays spectating/replays only. Research recommends native Godot Controls for editors (GraphEdit is no longer experimental; CodeEdit), plus an external-IDE workflow for scripts with hot reload. No TypeScript editors and no webviews. Pending confirmation together with the "two languages" simplification.

Round-3 results:
- Component confidence is about 85–90% per area and rises to 90–95% if the spikes pass.
- Red team: project-success probability is about 40% as scoped and about 65% simplified. The risk is the number of languages and seams, not Rust or Godot.
- Scripting: wasmi 2.0, pinned exact version, `floats(false)`, fuel per tick, dirty-page snapshots between ticks only, command log is the source of truth with re-execution in CI.
- Crates: fixed / rkyv / postcard / xxh3 / imbl / rmcp 3.3 / prost + buf. Avoid bincode (unmaintained), cordic and hierarchical_pathfinding (stale).
- MCP: push digest reports, because CivBench shows agents under-monitor state. Keep to about 20–30 coarse tools.

Simplifications:
- DECIDED: web viewer is built after the native sandbox ships; native replays and in-game spectator mode until then.
- DECIDED: two languages at launch (Rust + GDScript). Editors use native Godot UI. All schemas are generated from one source. TypeScript arrives with the web viewer.
- DECIDED: the simple socket-based Gymnasium/PettingZoo wrapper comes first. Sim state is laid out for batching now; the fast PyO3 batched path is added only when measured throughput demands it.
- DECIDED (scripts): Rust SDK at launch, built for the AI-assisted development era. The game publishes and maintains API docs so coding agents (Claude Code, Codex, …) can work with the **locally installed game** rapidly and securely, for two purposes:
  1. Help write AI operator programs (scripts/controllers): SDK scaffolds, compile, dry-run under fuel, test against local scenarios.
  2. Act live as the player's customized AI controller, supplementing or replacing the built-in rudimentary governor.
  - Implications: the local game exposes a documented developer surface (a local MCP server + CLI + machine-readable docs such as llms.txt / generated schema docs). Security: localhost-only by default, per-controller tokens with scopes and max lease class, and no host filesystem or network access from scripts. A beginner in-game language can come later.
- DECIDED (math, delegated to the recommendation): integer-first.
  - Own `#[repr(transparent)]` unit newtypes (not an alias):
    - Pos Q16.16 (i32)
    - DistSq Q32.32 (i64; range checks compare r², no sqrt)
    - Angle u16 with a 4096-entry LUT trig
    - HP/resources as integer milli-units
    - water in 1/4096-voxel u32 units, with an exactly conserved flux
  - Floats are allowed only in a walled `sim::solve` module (basic ops + sqrt, quantized on return). Delete that module if the spikes allow.
  - Lints: deny float_arithmetic / as_conversions; disallowed f32/f64/HashMap/HashSet/Instant::now; overflow-checks on in release.
  - Mortar aim uses the discrete-integrator closed form.
  - Confidence: ~70% best, ~90% acceptable. Switch a kernel to restricted floats if it is >2.5× slower than f32.
- Arbitration: research done (it recommended per-unit hybrid leases). The user's decision below replaces most of it.
- DECIDED (control model):
  - **No manual control of any unit except the commander.** The commander is the only directly controlled unit.
  - Every other unit is controlled only through the mandate of its beacon's operator.
  - Beyond that, customization happens through the API: custom mandates and custom AI control.
  - **Changing a mandate costs control points.**
  - Consequences:
    - There are no per-unit manual leases, hand-back or takeover TTLs.
    - Arbitration becomes per-beacon (who may change mandates, what executes them) plus rules for moving units between beacons.
    - Hotkeys, groups and macros act on commander actions and mandate changes, not on units.
    - Reflexes (retreat, return fire) remain part of mandate execution, governed by rules of engagement (ROE) and always announced.
    - Still reusable from the research: stale-order counters (epochs) for late API commands, `Revoked`/`Rejected` events, and deterministic once-per-tick resolution.
- DECIDED (beacon control structure):
  - **Two layers per beacon.** The mandate is WHAT (goal + parameters). The operator is HOW: it executes the mandate every tick. The default operator is the built-in rudimentary AI, replaceable by custom programs.
  - **No control points.** They were removed entirely. The cost of change is **physical**: the commander must travel to a beacon and interface with it to change its mandate or make any other beacon change. Valuable commander transit time and interaction time are the currency.
  - **Future beacon changes** include installing custom operator programs. Programs can be pre-developed, developed in real time, or both, presumably with LLM assistance.
  - **External AI (LLM via local API)** acts as strategist, i.e. it pilots the commander and reads reports. It can also act as a live operator, but only in turn or pausable modes where the sim waits.
  - **Units are bound to a home beacon**, the one that produced them or an explicit reassignment. They follow that beacon's mandate, even outside its sphere.
  - **Operators and beacons can communicate** with each other through API customization, by design.
- DECIDED (beacon interaction rules):
  - **Comms:** operators exchange data messages (threats, requests, shared targets) and adapt within their installed program and current mandate. Messages can NEVER reconfigure a beacon. Mandate changes and program installs always require the commander on site.
  - **Remote info:** the seat receives all reports and all fog-limited vision from its beacons and units, anywhere. Only changes need the commander present.
  - **Interfacing:** takes time scaled to the size of the change (roughly: settings tweak ~3 s, mandate switch ~8 s, program install ~10–30 s by size; exact values TBD). The commander must stay in range and still. Damage interrupts the interface, and the beacon keeps its old configuration.
  - **Commander (DECIDED):**
    - Mobility: one fixed-speed mobile body (e.g. a hover-walker that climbs 1-voxel steps). Terrain, roads, ramps and bridges shape routes; upgrades come later.
    - ROADMAP: late-game vehicles with automatically firing defensive weapons. How they're earned is TBD, since the economy hasn't been designed.
    - Death: respawn at core/first beacon after a delay (e.g. 30 s, growing with repeated deaths). Beacons keep running their current mandates and operators, but can't be changed until the commander returns.
    - Remote changes: none. Strictly on-site.
    - ROADMAP only (NOT in the prototype): weak self-defense weapon, carry and place a demolition charge, minimal construct/dig, sensor ping/scan.
  - **Programs:** written and compiled outside the sim at any time (e.g. by Claude Code through the local game API) and stored in the seat's program library. Installing one requires the commander at the beacon. A live LLM operator is an "uplink" program installed the same way; after installation the LLM drives that operator remotely (turn/pausable modes only).

Leading candidate: an engine-free Rust sim with a Godot 4.x client. The stress test gave it about 75% confidence and proposed these adjustments:
- **Sim internals:** integer/fixed-point gameplay math; plain SoA tables instead of a general ECS; copy-on-write chunks for cheap forking.
- **Portability:** WASM-capable sim from day one, so replays can play in the browser.
- **Rendering:** our own Rust mesher, shared by Godot (via gdext) and the web viewer. Voxel Tools becomes optional.
- **Protocol:** Protobuf schema-first. MCP tool schemas are generated from it; bulk chunk data uses a separate binary channel.
- **Networking:** server-authoritative snapshot/delta, not lockstep, because fog of war must stay on the server.
- **Trial build:** a 3–4 week spike with pass/fail gates. The gates: at least 2k ticks/s; identical replay hashes across platforms; chunk remesh of 4 ms or less; 60 fps with 20 explosions/s; an MCP agent completes a defend scenario.

A final research pass is pending before committing (see §5).

---

## 2.0 LATEST SCOPE DECISIONS (take precedence over anything earlier in this file)
- **v1 ships BUILT-IN operators only.** Beacons run the built-in operator, which follows its mandate and settings.
  - Custom components (custom operator programs, custom reprogramming, custom quartermaster, WASM commander) arrive one per version afterwards, in an order still TBD.
  - Consequences:
    - The wasmi/WASM scripting work and its spikes (G5) move out of v1 and Phase 0 into a later milestone. Its research stays valid for that milestone.
    - AI seats in v1 act through gameplans, mandates and settings, beacon placement and recycling, Quartermaster priority knobs, and voting, all via the API/MCP.
    - The v1 core is the gameplan/API/editor co-design.
- **Radio connectivity v1 = "Mast only":**
  - Radio masts, with range growing with height.
  - No line-of-sight occlusion, no relays, no jamming yet.
  - Chosen as simple and extensible to Horizon/Spectrum later without backtracking. The design must leave room for LOS, relays, jamming, direction finding and satellite.
- **Destroyed capability structure:** the capability is suspended until the structure is rebuilt. Timed weapons keep counting down meanwhile.
- **LORE / THEME:** Terminator-style (an original IP, no borrowed names). Humanity working with AI fights an adversarial AI that works with (collaborating) humanity. Both sides are human+AI alliances, which mirrors the game's human/AI seats. Rejected: "The Hush" proposal; its anti-hacking premise may still be reused.

- **Segment length (DECIDED):** can start smaller, but longer is better, up to 10 minutes. Default: segments grow with match phase from roughly 3 min early to a 10 min cap. Host-configurable. This supersedes the flat 8 min default.
- **Voting: DEFERRED** (the whole feature, including pity votes and voted capabilities). v1 catch-up = rank multiplier on awards + small BMI scaling by standing.
- **v1 capability structures (DECIDED):**
  - Radio Mast (messaging; range grows with height)
  - Sensor Spire (timed reveal pulse with cooldown)
  - Resonance Spire (beacon radius: a crystal lattice grows over about 60 s with an aurora tether; the dome collapses on camera if the spire is destroyed)
  - Deferred: Command Bunker, Vehicle Bay and the rest of the catalogue.
- **Lore research returned:** three original frames, all built on "sealed orders" (radio silence during play explains the movie segments) and a neutral paymaster machine that pays BMI, awards and shames and runs the verifier ("seal inspection"):
  - A **Handfast** (grim; recommended): Quorum vs Handfast Line; Keybearer / Stake / Steward / Signal spar; mandates Raise / Hold / Break / Delve; Scrip; the Lull / the Push; the Ledger.
  - B **Brass Broadcast** (retro pulp): Calculus vs Sparkwright Union.
  - C **Hands-On Mutual** (corporate satire): Ownself Holdings vs Hands-On Mutual, wet signature.
  - Names to avoid are listed in the research (Skynet, Resistance, Sentinels, GAIA, Cylon, etc.).
  - A trademark clearance search is needed before finalizing names.
- **LORE (DECIDED):** Handfast world and tone (option A), with Hands-On Mutual's corporate-bureaucratic humour (option C) in the flavour text for Citations (shames), awards and the Ledger. Names remain working names until a trademark check.
- **RL (DECIDED):** deferred past v1, keeping the door open. v1 still builds the fast headless deterministic sim, command-log replays and the operator seam, so a Gymnasium/PettingZoo interface can arrive later with custom operators. PettingZoo api_test is removed from Phase 0.

- **GAMEPLAN/API DESIGN** is in design-gameplan-api.md: 27 MCP tools, a verifier with ~40 diagnostic codes plus JSON Patch fixes, a map-timeline and rule-list editor, a planner, and worked examples. Decisions from reviewing it:
  - **Recycling:** an on-site gameplan change (about 10 s interface) with the 50% refund at commit. This supersedes "between rounds, instant".
  - **Radio Mast = the Messaging capability structure.** Earning Messaging unlocks it. Building it activates messaging within its range. Destroying it suspends messaging.
  - **Technical defaults accepted:**
    - 20 Hz tick
    - interface-time table: handshake 1.5 s, priority knob 1.5 s, settings 2 s + 0.5 s per extra field, mandate switch 8 s, recycle 10 s, beacon placement 12 s
    - at most 6 changes per visit, committed one at a time
    - Attack reach ≤ 96 voxels outside the sphere
    - Protobuf as the single schema source, with an in-house generator for LLM-friendly JSON Schema (Buf plugin as fallback)
  - **Accepted:** the beginner rule list shows 4 slots, experts get 12.
  - **Rejected:** a "reuse last plan" shortcut. **Every round should be completely different from the last.** A timeout files the safe plan only.
  - **Ready status (DECIDED):** seats see **who** is ready during the planning pause.
  - **No-fog camera during planning (DECIDED):** a host setting.

## 2.1 Re-scoped review (built-ins-only v1)
- **Confidence:**
  - Foundation: 86% now, 93% after Phase 0.
  - Shipping sandbox v1: 70% now, 78% after Phase 0.
  - Separate playtest risk: is 8 minutes of play with no control fun?
- **Phase 0 (~11 calendar weeks):**
  - G5 (WASM) is deferred.
  - Kept: G1 remesh; G2 pathing + P3 travel-time estimates (±15% p90); G3′ 8-minute segment budget with the full kit; G4 determinism + fork.
  - Added:
    - P1 verifier ≤50 ms, deterministic, fuzzed
    - P2 built-in planner (safe plan ≤200 ms)
    - P5 MCP planning session (median ≤3 min, ≤25 tool calls)
    - P6 GDScript editor productivity
    - P7 mast connectivity ≤1 ms
- **Milestones after Phase 0 (~48 weeks; plan 16–17 months from today):**
  1. Sim core, 8 weeks
  2. Units, pathing/ETA, built-in operators, Quartermaster, masts, 9 weeks
  3. Gameplan, verifier, planner, ending in a vertical slice, 6 weeks
  4. API/MCP, 4 weeks
  5. Godot client and editor, 10 weeks. Overlaps milestone 3.
  6. Economy layer, 5 weeks
  7. Hardening, 6 weeks
- **Tech adjustments:**
  - Remove wasmi from v1.
  - Plan model, verifier and travel-time queries in Rust, reached through gdext. GDScript is for views only.
  - Operators are native Rust in v1. This supersedes "operators are WASM only".
  - Verifier and planner output must be deterministic.
  - Cross-OS determinism protects only replays and CI.
  - Water units deleted.
  - Check whether gameplan JSON Schema should come from Rust (schemars) rather than Protobuf.
- **Minimum seam kept for later WASM:**
  - `Operator` trait: beacon-local view + mailbox + mandate in, intents out, no global state
  - abstract per-tick work counter (becomes fuel later)
  - `program_id` in beacon config
  - `kind`/`version` tag on the gameplan envelope
  - `Quartermaster` trait
- **SUPERSEDED by 2.0/2.1:**
  - 2a "default AI as WASM"
  - 2a WASM-based "qualifying" definition. The v1 definition becomes schema check + semantic check + upkeep warning + content hash.
  - Message fuel charging
  - Install time from module bytes
  - Firewall Vault (fuel) in v1
  - "WASM-capable sim from day one", now deferred along with the web viewer
- **Open contradictions to interview:**
  1. Can the verifier simulate anything at all under "no dry runs" (travel-time estimates)?
  2. Messaging has no sender in v1. Can a gameplan send? Does mast connectivity gate only operator messages, or also remote reports?
  3. Voting needs 4+ players while v1 allows 1–4 seats, so catch-up is weaker in 2–3 seat matches.
  4. Mandate fields budget/priority/override are obsolete; Mine should deliver to beacon storage.
  5. Segment length of 8 minutes vs about 3 minutes.
  6. Commander-death fallback required in gameplans.
  7. RL shape in v1: gameplan-per-segment actions, operator step mode, or defer RL.
  8. Which capability structures ship in v1 (only the Mast is decided).
  9. How AI seats agree to a speed-up.
  10. Ready-status leakage.
  11. Tick rate (assume 20 Hz).

## 2a. Decisions after the final review
- **LORE:** a commander must physically contact a beacon to change it because this prevents remote hacking. That is the in-world reason for on-site-only changes, and for messages carrying data only.
- **LLM participation (replaces the "live uplink operator" idea):**
  - The LLM acts during timed turn-mode pauses (default 5 min).
  - Before the timer runs out, it must upload "safe/qualifying" scripts:
    - a commander gameplan script (drives the commander during play)
    - any per-beacon operator script changes, which the commander then installs on site
  - Scripts can be pre-verified or verified on the fly.
  - The verification mechanism is part of the public API, so the LLM can run it itself first. On-the-fly verification is then just a formality.
  - Proposed technical definition of "qualifying":
    - compiles to an allowed ABI version
    - only allowed imports; floats disabled
    - within memory and module-size caps
    - passes a sandboxed dry run under the fuel cap on a verification scenario
    - emits only mandate-contract-valid intents
    - content hash recorded
  - The same verifier runs locally through the API and at upload.
- **Default AI:** implemented as a WASM operator through the same public interface and per-tick fuel budget as custom programs. It doubles as SDK dogfooding and gives players a starting point to fork.
- **Operator perception:** beacon-local (its own sphere and units) plus messages from other operators.
  - Open: should the default built-in operator be able to message other operators at all? Messaging capability might be EARNABLE and a core part of the meta. Decide in the economy/progression interview.
- **v1 scope:**
  - No flowing water (roadmapped).
  - Local host with 1–4 seats (human and/or AI); no internet multiplayer or dedicated servers.
  - "Streamable" means a native spectator camera with a no-fog toggle, captured via OBS; the web viewer comes later.

- **MATCH FLOW (DECIDED; a major shift):**
  - Matches alternate play segments and planning pauses. There is **more game time than planning time**.
  - During play the sim runs **like a movie**, with **no manual control**. Every seat's commander, human or AI, follows its uploaded gameplan. People chat and enjoy the action.
  - Planning pauses are shared by all seats (default max 5 min), and play resumes early when everyone is ready.
  - Humans need a **real method to script or plan the commander** during the pause. This is required in v1.
  - ROADMAP: manual control, where a human takes full control of their commander for a round.
  - Human seats use the same gameplan API and verifier as AI seats.
  - **Timeout or failed verification:** keep the last valid configuration, and a **default "safe" plan is filed on the seat's behalf**. It comes from the **same mechanism built for computer-opponent control** (the built-in planner).
  - Implications:
    - There is perfect parity between human and AI seats during play.
    - The built-in AI has two roles: the per-beacon default operator, and the commander planner that generates default and safe gameplans. A computer opponent is simply that planner plus the default operators.
    - Adapting mid-segment is only possible through the gameplan's and operators' conditional logic, so how expressive a gameplan can be becomes central.
    - Step mode remains for headless RL training.
  - **Pacing (DECIDED):** default 8 min play / ≤5 min planning, with ready-up. Both are host-configurable.
  - **During play (v1):**
    - chat
    - free or auto-director spectator camera
    - live event timeline plus a highlight replay/scrub after each segment
    - speed controls in local matches (2–4× or skip, if all seats agree)
    - The user notes "lots of room for imagination here"; keep a roadmap list of watch-mode features.
    - Not chosen for v1: revealing opponents' plans after a segment.
  - **Planning information:** each seat plans only from its own fog-of-war view.
  - **GAMEPLAN (DECIDED):**
    - **Format:** typed declarative data plan in v1 (steps, triggers, fallbacks, bounded loops, every wait has a timeout; verifiable; LLM-producible via JSON Schema). A WASM commander program comes later under the same outer format.
    - **Human authoring in v1:** map timeline + gambit-style rule list + templates.
      - **Spend lots of effort here.** It is critical to the detailed API design: API design and editor design must feed each other (co-design).
    - **No dry runs.** During play, units and buildings follow their most recent mandate instructions.
      - Re-programming them to operate independently is a capability that is earned and activated.
      - **ROADMAP:** the custom reprogramming capability (it needs more custom script creation and verification). Not in the initial deployment.
    - **Plan information:** other seats learn nothing about your plans, ever. No reveal after segments.
  - **Research notes (earlier), for reference:** gambit list + map timeline + templates + JSON share string; limits of 64 steps / depth 6 / bounded loops / forward jumps only; verifier = schema + semantic checks, diagnostics in rustc-JSON style via MCP; planner = templates + utility scoring, with bounded fork rollouts for Hard. Tension still open: the research suggested ~3 min segments vs the 8 min chosen.
    - Research recommends: typed declarative data plan in v1, edited via a gambit list, a map timeline, templates, and a JSON share string. WASM commander in v2 under the same schema.
    - Limits: 64 steps, depth 6, bounded loops, forward jumps only, every wait has a timeout.
    - Two-stage verifier: schema + semantic checks, then an advisory dry run on the seat's own knowledge. Diagnostics in rustc-JSON style, exposed via MCP.
    - Planner: templates + utility scoring, with bounded fork rollouts for Hard difficulty.
    - Flagged tensions: research suggests ~3 min segments or scaling pauses, vs the 8 min chosen; AI dry-run advantage (quota?); ready-status leakage; plan reveal.

- **ECONOMY (DECIDED so far; "simpler is better"):**
  - One currency: **$**. No metal/energy.
  - Each beacon consumes $ (upkeep). Building anything costs $.
  - Income per round:
    - "BMI" (meaning to be confirmed)
    - plus creative per-round individual **awards**
    - minus creative per-round individual **"shame"** penalties for lame behavior
  - **Mined $** is the other route to extra spending. Mining drones physically deliver it to beacon storage.
  - Units come from fabricators built inside spheres (Build mandate), are bound to that beacon, and cost $.
  - **Capabilities** (e.g. messaging, vehicles, beacon tiers):
    - EARNED by matching triggers
    - also VOTED on by users between rounds
    - occasionally awarded at RANDOM
    - Matching the right trigger to the right capability matters a lot for the meta and for fun.
  - **Open:** spend control, i.e. how spending is prioritized among operators and budgeted between rounds. The user asked for a simple, elegant solution.
  - **BMI = Basic Minimum Income** (DECIDED): a flat $ amount per seat per round.
  - **Unspent $** sweeps back to the treasury at round end (DECIDED).
  - **Mined $** goes into the mining beacon's own spendable pool for that round, then sweeps (DECIDED).
  - **Spend control requirements (DECIDED):**
    - Envelopes may be uninteresting complexity; investigate simpler options.
    - **Accounting is not fun.**
    - Spend control must be customizable within operator scripts AND ship with a reasonably effective built-in default.
    - A bankrupt beacon must be a willful mistake or a deliberate strategy, never a frequent oversight.
  - **SPEND CONTROL (DECIDED):**
    - A built-in **Quartermaster** auto-budget, with no envelopes to manage.
      - It reserves next round's upkeep for every beacon first.
      - It then fills operator spend requests by urgency: defend under attack, then repair, units, build, mine.
      - Fair-share caps stop one beacon hogging funds.
      - One optional knob per beacon: low / normal / high priority.
    - Bankruptcy safeguards (all adopted):
      - upkeep always reserved first
      - the verifier warns on plans whose upkeep isn't covered, which need an explicit "allow dormant beacons" to run (same for AI seats)
      - an unfunded beacon goes dormant (operator paused, structures stay, capture slower) and revives when funded
      - a plain-language low-funds alert in the recap
    - **Beacon recycling:** beacons can be strategically recycled between rounds for an instant refund of 50% of build cost.
    - **Custom quartermaster: ROADMAP.** When built, it will be pre-installed via the offline verifier before a match and can't be modified in-game except to switch back to the default. For now, focus on the default. (The user doubts there is much fun in the minutiae of spend automation.)
  - **AWARDS AND CATCH-UP (DECIDED):**
    - **Award cap:** total awards ≤ 50% of BMI per seat per round, with at most 3 awards per seat per round.
    - **Catch-up levers:**
      1. A rank multiplier on awards, so trailing seats earn more from the same award.
      2. **BMI scaled by standing.** Sandbagging risk; mitigation still to be decided.
      3. **Random "pity votes":** between rounds, a ballot sometimes offers a big cash bump for the last-place seat.
    - Not chosen: underdog-only awards; a supply-drop pity timer.
    - **Beginner hand-up:** no separate rookie handicap. The catch-up levers above cover beginners.
    - **Shames:** a small $ penalty (≤25% of BMI each), floor at zero, shown as a funny title in the recap. A seat's first timeout per match is free. Same rules for AI seats.
    - **Standing:** smoothed net worth (treasury + beacons + structures + units, averaged over the last 2 rounds).
    - **BMI scaling by standing:** small but not insignificant. It must not encourage sandbagging or discourage winning. Proposed tuning: last place +10%, leader −5%, linear in between. Validate in playtests.
    - **Pity vote:** appears at random (seeded) about 1 in 4 pauses, never twice in a row, AND only when the gap is large. Proposed threshold: last place's standing is below 50% of the leader's. Payout 1× BMI to last place.
    - **Voters:** seats only for now (spectator voting to be revisited). AI seats vote via the API.
    - **Default operator messaging:** receive-only until messaging is earned. Once earned, default operators also send.
    - **Voting:** happens only in matches with 4+ players. A tie counts as a failed vote (for now).
  - **CAPABILITIES (DECIDED so far):**
    - Duration: tools (messaging, dig/construct, etc.) last the whole match; weapons and combat boosts (vehicle auto-weapons, carrying a charge, extra fuel) are timed, 2–3 rounds.
    - **Every capability award is enabled by placing a voxel building.** Capabilities are physical structures in the world, not abstract unlocks.
    - **Avoid simplistic, boring upgrades** such as +damage or +radius. A beacon radius tier is acceptable only if it's graphically impressive or tied into the story.
    - Messaging depth is open and being researched:
      - a radio antenna, ideally placed at elevation?
      - radio jamming?
      - satellite?
      - how much connectivity at all?
    - How often capabilities are awarded at random: DEFERRED (meta detail).
    - **Research returned** (sources written from memory, not verified live):
      - **Connectivity model, "Horizon" (medium depth), recommended for v1:**
        - radio masts whose range grows with isqrt of height
        - line-of-sight blocked by voxels (integer DDA, cached per chunk version)
        - relay pylons
        - jammer dome as a timed weapon
        - link-snap effects mid-movie
        - union-find graph in observations
      - **Roadmap:** direction finding (v1.1), satellite uplink with scheduled passes (v1.2), per-hop latency later, never remote reconfiguration.
      - **15 capability structures:** Radio Mast, Relay Pylon, Jammer, Uplink Dish, DF Loop, Sensor Spire, Resonance Spire (beacon radius, growing crystal lattice + aurora tether that collapses when cut), Vehicle Bay, Excavator Rig, Charge Armory, Transit Rail, Command Bunker, Decoy Beacon, Firewall Vault (operator fuel), Drone Hangar.
      - **Lore proposal "The Hush":** the Whisper intrusion rode radio waves and rewrote machines. Beacons = Lodestones keyed only by a living hand. Operators = Wardens (may speak on the air, never obey it). Commanders = Couriers.
  - **Awards/shames/capabilities/voting research returned** (not yet interviewed; sources were not fetched live, verify later):
    - 12 awards + 6 shames, deterministic from sim counters
    - awards capped at 0.5×BMI, rank multipliers favour trailing seats
    - a capability trigger map
    - rotating ballots, weighted 50/50 seats vs spectators
    - supply drops with a pity timer and a rank rubber band
    - default operator can receive messages but not send them until messaging is earned
  - **ECONOMY PRINCIPLE (DECIDED):** reward good gameplay, but also give trailing players a way to catch up and beginners a hand-up. Awards, shames, base income, voting and random drops must all be tuned together to meet this.

## 2b. Final review after the control redesign
- **Scores:** foundation-correctness confidence is about 82% now, about 92% if all Phase-0 spike thresholds pass. Probability of shipping sandbox v1 is about 68% now, about 75% after Phase 0.
- **Verdict:** 90–95% can only be reached by measurement, not more research. Commit gated by Phase 0 (8–9 weeks of spikes G1–G6).
- **Spikes:**
  - G1: destruction remesh within 3 frames at 60 fps
  - G2: pathing churn repath p99 ≤5 ms
  - G3: tick p99 ≤25 ms with 300 units
  - G4: identical hashes across 3 OSes, plus fork
  - G5: 30 WASM operators ≤8 ms/tick; an agent writes an operator from llms.txt alone
  - G6: LLM-via-MCP turn-mode scenario; PettingZoo api_test
- **Technical defaults adopted** (technical decisions delegated to the recommendations; the user can veto):
  - Operator messages are delivered at tick+1, ordered by (sender beacon id, seq), bounded in size, charged against fuel, same team only.
  - Install duration comes from deterministic inputs (base + k × module bytes, or the mandate diff), and programs are content-addressed by hash and stored with replays. IDE hot reload updates only the seat library, never installed instances.
  - The float `solve` module never feeds persistent state unless G4 proves bit-identical output; otherwise it's presentation only.
  - Q16.16 positions cap the world at ±32,768 voxels, which is far above planned map sizes.
  - GDScript covers UI/HUD only; operators are WASM only.
- **Conflicting facts to verify in Phase 0:** rmcp latest version (3.3.0 vs 3.1.1 reported); whether GraphEdit is still marked experimental in 4.7 (reports conflict). Neither is load-bearing.
- **Milestones after Phase 0** (about 12 months, plan 14–15 with buffer):
  1. Sim core, 8 wk
  2. Units + pathing + default AI, 10 wk
  3. API (protobuf/MCP/llms.txt/PettingZoo), 5 wk
  4. WASM operators + library + install + uplink, 6 wk
  5. Client + spectacle, 8 wk
  6. Hardening + release, 5 wk
- **Major design gap:** the ECONOMY hasn't been discussed (resources, production, how units, beacons and vehicles are earned).

## 3. Drafted mandates (defaults, v0)
A mandate is data: `{type, sphere, params, budget, priority, rules_of_engagement, report}`. Its default executor is the built-in governor, a rudimentary utility AI or behaviour tree. Any other controller can replace or override it.

**Fields every mandate has**
- **budget:** resource cap per period, plus a drone/unit allocation.
- **priority:** used when mandates compete for shared drones or resources.
- **report:** which events to surface, such as "blocked", "under attack", "quota met" or "budget exhausted".
- **override policy:** what manual or other controllers may pre-empt.

### Build
Construct and maintain structures inside the sphere.
- **Params:**
  - `targets`: one or more blueprints/placements
  - `order`: priority order
  - `repair_threshold`: e.g. 60% HP
  - `rebuild_destroyed`: on/off
  - `terraform_allowed`: none / fill / dig / both
  - `protected_zones`: areas it must not modify
- **Default behaviour:** pick the highest-priority unbuilt or damaged target it can afford, then assign the nearest idle drone.

### Defend
Hold the sphere and protect listed assets.
- **Params:**
  - `protect`: structures or areas; defaults to the beacon and everything inside
  - `engagement_radius`: at most the sphere radius
  - `ROE`: hold fire / return fire / engage on sight
  - `retreat_hp`: e.g. 30%
  - `pursue`: never / to the sphere edge / limited distance
  - `auto_fortify`: request walls and turrets from a Build mandate on/off
- **Default behaviour:** threat-weighted targeting; units stay inside the engagement radius; damaged units fall back toward repair.

### Attack
Project force against a target.
- **Params:**
  - `target`: structure, beacon, area or unit group
  - `staging_point`: inside the sphere
  - `launch_condition`: force size, timer or manual go
  - `force_composition`: minimums
  - `retreat_condition`: losses %, target destroyed or timeout
  - `avoid`: zones to avoid
  - `breach_allowed`: demolition/digging on/off
- **Default behaviour:** gather at the staging point, then launch when the condition is met, path around avoid zones, focus the weak point, then retreat or regroup.
- **Open:** whether an Attack mandate may act outside its sphere, and how far.

### Mine
Extract resources inside the sphere.
- **Params:**
  - `resource_priority`: ordered list
  - `quota`: target rate per period, or stockpile cap
  - `deliver_to`: core, storage or logistics link
  - `dig_rules`: max depth; no digging under structures; keep a support pillar every N tiles
  - `flee_on_threat`: on/off
- **Default behaviour:** nearest reachable highest-priority ore; idle when the quota is met; flee to the beacon when threatened.

**Open:** later mandates such as Scout, Logistics/Haul, Terraform, Research/Produce, Salvage/Reclaim, Patrol.

---

## 4. Extrapolation: what the decisions imply

### A. A layered control stack
Seat (commander) → beacon governors → units/drones. Every layer is a **controller slot**, and every controller emits the same typed commands.

**Controller types**
- Manual (UI)
- Built-in governor
- Script (sandboxed, in-sim)
- External agent (LLM over MCP, RL policy or bot over the protocol)
- Client-side conveniences: hotkeys, control groups and macros compile down to commands or mandate edits

**Required composition rules** (new spec area)
- **Authority/precedence:** e.g. manual > macro > script/agent > built-in governor.
- **Unit leases/locks:** which controller currently owns a unit.
- **Conflicts:** how conflicting orders are resolved.
- **Hand-off:** whether control can be temporary ("take over for 30 s, then return to the governor").

### B. Where controllers run
- **In-sim:** deterministic, tick-budgeted and sandboxed (e.g. WASM with fuel). Fast enough for RL and replayable by re-execution.
- **Out-of-sim:** external controllers over the protocol. Their commands go into the log, so replays stay deterministic.

This choice adds a new foundation question: which deterministic sandboxed script runtime to use.

### C. Mandates bridge the latency gap
LLMs act at the mandate level: set, tune and react to reports. Built-in governors, scripts and RL act at the unit level every tick. So slow agents stay competitive, and continuous real-time later becomes feasible: the LLM steers while governors execute.

### D. Beacons do several jobs
- **Claim:** a build zone owned by a seat, which solves griefing in the shared sandbox (non-destructible to others by default; arenas are opt-in).
- **Scope of governance and of observation:** a sphere-local view gives natural token budgets for LLMs and fixed-size egocentric tensors for RL.
- **Weak point:** destroying a beacon collapses its governance (Legends-style portal). A spectacle hook and a clear attack objective.
- **Candidate vision/fog provider.**

### E. The protocol needs new message families
- `beacon.*`: place, move?, upgrade, destroy
- `mandate.*`: create, edit, pause, remove, report stream
- `controller.*`: attach, detach, lease, override, release
- `macro.*`, `group.*`: whether these are stored server-side, so they're shared with agents and replayable, or kept client-only

### F. The UI gets heavier
The client needs a mandate editor, a beacon overlay, a controller-attachment panel, and a macro/script editor (possibly node-graph). This weighs on the client choice: Godot UI vs web tech.

### G. Spectacle
Spectators see beacon spheres, mandate states and controller badges ("LLM", "RL", "script", "manual"). The chronicle logs governor decisions, so the story shows *who* decided what.

### H. Fairness
The same limits apply to every controller type: command points per game-time. Scripts get instruction budgets. External agents can't exceed what an in-sim governor could do.

---

## 5. Open items for the final interview pass
1. **Beacon mechanics:** cost, radius, max count, overlap between own and enemy spheres, contesting and capture, upkeep, placement range from the commander, what happens when one is destroyed.
2. **Commander details:** abilities, respawn in the sandbox, whether the commander is itself a controller slot.
3. **Authority and composition model:** precedence, leases, temporary takeover.
4. **Controller hosting:** in-sim scripting runtime choice (WASM/wasmtime vs wasmi vs Rhai vs Luau); per-tick budgets.
5. **Mandates:** Attack scope outside the sphere; which later mandates ship when; whether mandates can be nested or chained.
6. **Where macros and groups live:** server (shared, replayable) or client.
7. **Sandbox attack/defend kit:** approve the proposed kit (walls + reinforced walls, gate/drawbridge, autocannon, mortar, raider drone, demolition charge, repair/reclaim drone).
8. **Remaining design tensions:** balance with no win condition ("stress test" wave button?); physics vs determinism (fixed-point kinematic projectiles, client-only debris); 3D logistics (surface belts + ramps + node network); pathfinding and water under destruction (chunked flow fields + 2.5D water); colony depth (no personalities; veterancy + chronicle); logic processors vs agents (same sandbox and budget); spectacle vs readability (telegraphs, overlays); mode order; licence (GPL-3.0?).
9. **Foundation:** commit after the final research pass.

- **PRELIMINARY GAME NAME (DECIDED):** AirGap. The world and lore frame stays Handfast (Quorum vs Handfast Line). Trademark check still pending.

## 2.2 PRE-BUILD INTERVIEW (2026-09-13, overrides earlier sections where it conflicts)
- **Match end (DECIDED):** elimination plus a round cap. A seat's first beacon is its core. A seat is eliminated when it has no beacons left. The match ends when one seat remains, or at the host-set round limit (default 10), when the highest standing wins. The round limit defines the match phase used by segment growth.
- **Local humans (DECIDED):** one human per match in v1, plus up to 3 AI seats, on localhost only. Human-vs-human waits for internet multiplayer (roadmap). Consequence: the single human decides speed-up and skip, and AI seats don't vote (resolves the open speed/skip question).
- **Push camera (DECIDED):** own fog by default. While the human's seat is alive, the camera and director show only that seat's vision, the same as the AI seats' get_segment_feed. The full map unlocks when the seat is eliminated or the match ends. A host "casual" toggle allows no-fog and marks the match unranked.
- **Seats and teams (DECIDED):** v1 is a 3-seat free-for-all at most: 1 human plus up to 2 AI seats, or all AI. No teams. Messages stay within a seat. Friendly fire means hitting your own units or structures. Reserve a team_id field for later. Consequences: remove "4 seats" from the spec. Phase 0 gate G3′ can keep 4 seats as performance headroom. The planner needs a target-spread rule so both AIs don't pile onto one seat. Watch for kingmaking in 3-way FFA.
- **Fun test timing (DECIDED):** keep the milestone order. The movie fun test moves from the end of M3 to after M5, once the full Godot client exists. The M3 slice becomes a technical slice only (headless sim + plans + verifier + planner). Accepted risk: the biggest design risk is measured about 10 months later.
- **Lull timer (DECIDED):** a shared timer set by the host (3 min to untimed), default 5 min. The first Lull of a match defaults to 10 min. Gate P6 changes to: a first-timer builds a qualifying plan from a template in ≤5 min, and a 10-step plan from scratch in ≤10 min. LLM seats wait by long-polling.
- **Poverty (DECIDED):** an existing upkeep shortfall is a warning (W06xx) and is flagged in the recap. It is an error only when the plan adds uncovered upkeep (placing a beacon, queuing a structure) without "allow dormant". When money runs short, the Quartermaster makes beacons dormant in a fixed order: lowest priority first, then furthest from the core, and the core last. The principle is now "you only make it worse on purpose".
- **Capture (DECIDED): destroy only in v1.** This reverses the earlier capture-by-presence rule, and capture moves to the roadmap. Beacons are destroyed by damage. Their structures become unclaimed and are absorbed by the first uncontested own beacon whose sphere covers them. Bound units retreat, then re-home to the nearest friendly beacon (same as Recycle). Overlapping enemy spheres still block building and digging. Remove "dormant beacons take longer to capture".
- **Messaging (DECIDED): no radio until a Mast.** Drop "receive-only". Without a Radio Mast, beacons neither send nor receive. With one, operators inside coverage send and receive threat and help messages, and the commander can broadcast go-codes while in coverage. Beacon reports to the seat stay remote and always on. Attack launch conditions without a Mast use clock or force size.
- **Caps principle (DECIDED, user rule):** no artificial caps. Use a cap only where it is technically necessary, and every such cap must be explained coherently in the story. Audit existing limits against this rule (unit caps, awards per round, the 6 changes per visit, the Build target count, etc.). Plan and verifier limits that guarantee termination and bounded cost are technical, but still need a story framing (Seal inspection).
- **Power (DECIDED): kW replaces $ upkeep.**
  - **Roles:** $ is a stock spent to build. kW is a continuous flow that sustains everything fielded: beacons, units, turrets and shields.
  - **Supply:**
    - Each beacon's key-core gives a small base output.
    - Generator structures, built for $ with the Build mandate, sit on a finite number of geothermal vents on the map.
  - **Grid:** one seat-wide grid. No grid islands in v1 (roadmap). No batteries or storage in v1, so no J/Wh.
  - **Overdraw:** fabricators stop first, then beacons brown out into dormancy in the order already decided (lowest priority, then furthest from the core, core last).
  - **Replaces:**
    - $ upkeep and the Quartermaster's "reserve next round's upkeep" step are removed.
    - The poverty rule now applies to kW: an existing shortfall is a warning; a plan that adds draw beyond supply is an error unless "allow dormant" is set.
  - **Ceiling:** the unit ceiling becomes a world property, total vent supply on the map. The engine keeps a hard safety limit far above what any map can power, and map generation/design must keep total supply within the G3′ budget.
  - **Story (user):** no solar, because the world is in perpetual darkness. The sky is permanently dark (working name "the Pall"; name TBD), so power comes from geothermal heat and key-cores. Spectacle: glowing vents and lit domes in the dark, with visible brownouts.
  - **Roadmap:**
    - charge individual units energy for remote operations (user)
    - grid islands
    - batteries/storage
- **Start state (DECIDED):** each seat starts with a pre-placed core beacon (Build mandate) in its spawn zone, a commander, 2 build drones, 1 mining drone and $ equal to 2 rounds of BMI. The core's key-core provides base kW for the starting force plus headroom, and a vent sits a short walk away. **Every beacon has a built-in fabricator** that produces units for its mandate through the Quartermaster; there is no fabricator structure. Units cost $ to build and draw kW while fielded. No per-beacon unit cap (Tuning: starting numbers).
- **Maps (DECIDED):** a seeded, deterministic generator with 3-way rotational symmetry: equal spawn zones, equal nearby vents and ore, and richer contested vents toward the centre. About 384×384 wide by 64 tall (tuned by G3′). One ore type ("scrap seams") of varying richness, plus salvage from wrecks. Mine's "resource priority" becomes richest / nearest / safest. Maps save as files so handmade maps can come later. The generator is about 3 weeks in M1.
- **Placement setup (DECIDED):** choosing the mandate type is free at placement. Every other initial setting or target is charged at the normal interface rates after the 12 s deploy. **The 6-changes-per-visit limit is removed** (caps rule): a long visit is its own risk. The verifier's plan-size limits still bound cost.
- **Replays (DECIDED):** two formats. The deterministic replay (seed + plans + log) stays in the host's private match cache, powers scrub, recap and bug reports, and is never exposed through the API or UI in v1. Exported or shared replays are **recordings**: keyframes plus state deltas and events, with no plans. About 2 weeks of extra work. Secrecy on a local host is enforced by the API and UI, not by cryptography.
- **Capabilities (DECIDED): thematic licences, fabricator-built.**
  - **Licences:** earning a capability grants a permanent Ledger licence.
  - **Triggers:**
    - Radio Mast: hold 3 or more beacons at the end of a segment.
    - Sensor Spire: lose a structure to an attacker you never saw ("blindsided").
    - Resonance Spire: run Generators on 2 vents at once.
  - **Building:** once licensed, queue the structure on site at any own beacon. That beacon's built-in fabricator builds it regardless of mandate, for $ plus kW draw.
  - **Counts:** no count limits, except one Resonance Spire per sphere, since two spires would interfere (a technical and story reason).
  - **Rebuilds:** destroyed structures are rebuilt without re-earning.
  - **Tuning:** trigger thresholds, and anti-gaming for "blindsided".
- **Citations (DECIDED): deferred to post-v1.** When they arrive, they start cosmetic only: funny Ledger titles with no $ penalty. Small or large fees may be added later. v1 has awards but no Citations. The "first timeout is free" rule and the Snooze Button are moot in v1; a timeout just files the safe plan.
- **Recap timing (DECIDED):** the recap is its own phase between the Push and the Lull: awards plus about 3 auto-picked highlights, at most about 60 s, skippable by the human. The Lull timer starts when the recap ends or is skipped. The snapshot is frozen at segment end. During the Lull, a side panel lets the human re-watch or scrub highlights while the timer runs. AI seats long-poll through the recap.
- **Interrupts (DECIDED): damage never interrupts a visit.** Only the commander's death, a rule firing, a destroyed beacon, or leaving range stops it. The pressure comes from risking death, and dying interrupts the visit along with its other consequences. A rule firing aborts the remaining changes, and committed changes stay. Overlap with an enemy sphere blocks building and digging but not interfacing.
- **Scopes (DECIDED):** seat tokens can never hold spectate.nofog; it exists only for separate spectator tokens. Admin covers lobby and match control only and can never read another seat's plans, drafts or knowledge. The host "casual" toggle gives no-fog during the Push to every seat equally (the human's camera and the AI seats' get_segment_feed), while planning knowledge stays fog-limited. On a local host, enforcement is by the gateway only.
- **Fork (OPEN, revisit in draft 3):** G4 "determinism + fork" conflicts with "no dry runs". Recommendation on the table: snapshot/restore only (no fork), scrub from recordings, and plan-core/verifier/planner/gateway crates barred from depending on sim stepping. The user is unsure; draft 2 flags it as open.
- **Licence (OPEN, revisit in draft 3):** recommendation on the table: GPL-3.0-or-later for the game (sim, client, planner); MIT OR Apache-2.0 for schemas, SDKs, docs, llms.txt and example plans; CC BY-SA 4.0 for assets. Alternatives: permissive everything, or AGPL for the game. Must be decided before the repo goes public.
- **Name (DECIDED):** AirGap is the internal codename only, with no logo, domain or store page. A final title and faction names, plus a professional clearance search, come before anything goes public. World terms live in one lore table so a rename is a find-and-replace. The 2026-09-13 quick search found the Steam game "Interstellar Airgap" (2021), AIRGAP trademarks held by Airgap Networks/Zscaler (serial 88813915) and Transcend (99572619), and a 2025 board game called "Quorum". Handfast and Keybearer came up clean.
- **Pre-build interview status:** all 20 items done. Open for draft 3: fork (item 19) and licence (item 20).

## 2.3 PRE-BUILD INTERVIEW PASS 3 (2026-09-13, overrides 2.2 and earlier where it conflicts)
- **Fork (DECIDED, supersedes the 2.2 OPEN entry):** research-only fork. The core sim has snapshot/restore, which saves and replays also use. `fork` exists only behind a compile-time `research` Cargo feature that release builds never enable, and CI builds both configurations. The plan-core, verifier, planner and gateway crates can never depend on it, enforced by a dependency check in CI. Phase 0 G4 tests determinism, snapshot/restore round-trips, and fork equivalence in the research build. "No dry runs" stays a hard guarantee in shipped builds.
- **Licence (DECIDED, supersedes the 2.2 OPEN entry):** GPL-3.0-or-later for the game (sim, client, planner, gateway, gamectl). MIT OR Apache-2.0 for the schemas (`.proto` files and generated JSON Schema), SDKs, docs, llms.txt and example plans. CC BY-SA 4.0 for art, audio and other assets. Each licence gets per-directory LICENSE files and SPDX headers, and a DCO sign-off is required on contributions. Console ports are out of scope (GPL vs NDA SDKs).
- **Key-core power (DECIDED):** seal-only key-cores. Story: a key-core taps just enough heat through its own shallow bore to keep its Ledger seal lit, while the founding core sits on a deep bore with spare heat. Rules:
  - A non-core beacon's key-core output equals that beacon's own base draw, so its net is zero. It never goes dormant from its own draw, but its fabricator, structures and units draw from the seat grid.
  - Only the core provides surplus kW.
  - Generators on heat vents are the only scalable supply, which closes the beacon-spam loophole.
  - The map generator guarantees a reachable vent near each start.
- **LLM seats (DECIDED):** the game never calls an LLM and never holds API keys.
  - `gamectl seat run --agent claude|codex|<command template>` launches the user's installed coding agent headless once per Lull, e.g. `claude -p` or `codex exec`.
  - Each launch gets a seat prompt (from llms.txt) plus a generated MCP config holding a short-lived per-seat token.
  - Keys, model choice and billing stay in the user's agent tool. The lobby shows token and cost usage when the agent reports it.
  - Small per-agent adapters ship alongside a generic command template.
  - A timeout, crash or non-filing agent results in the safe plan being filed and an event in the log.
- **AI-only matches (DECIDED):** in v1 an AI-only match uses the normal client flow in realtime, with a spectator camera in place of the human seat. There is no headless or accelerated mode. Pace controls (speed-up, headless runs) go to the roadmap and get added once bulk AI testing and tuning needs them. The sim's fixed tick and runner already allow faster-than-realtime stepping later.
- **Elimination details (DECIDED):**
  - **Last beacon lost:**
    - Story: with no key-core seal left, the Ledger voids that seat's licences.
    - The seat is eliminated that tick. Its units shut down and disband on the spot.
    - Its structures go dark as neutral ruins. Anyone can recycle them for $ (a new recycle target for the planner).
  - **Core destroyed, other beacons survive:** no succession.
    - Story: the deep bore collapses.
    - The surplus kW is lost permanently, and the seat lives on vent Generators.
    - Brownout "furthest from core" is measured from the core's former site.
    - The commander respawns at the surviving beacon nearest to the death location.
    - Losing the core is not elimination.
  - **Human eliminated while AI seats continue:**
    - The human becomes a spectator with a full-map view, watching in realtime.
    - They can end the match at any time. Standings freeze at that tick and the recording is saved.
- **Round-limit winner (DECIDED, replaces smoothed net worth):** "Held + destroyed", framed in the story as the Ledger's final audit.
  - Score = value held at the final tick + build cost of enemy assets this seat destroyed during the match.
  - Value held = $ + build cost of its beacons, structures and units.
  - Credit goes to the seat that dealt the killing damage.
  - The score is computed by the game, shown live in the standings, and forms the recap's final ranking.
  - Risks to watch in the fun test: kingmaking and farming the weakest seat.
- **Caps audit (DECIDED):**
  - **Build targets and protected areas:** the ≤16 and ≤4 limits are removed. Both count toward the single plan size budget ("Ledger seal inspection"). That budget is technical: FULL verification must stay under 50 ms. The editor and the verifier output show a size meter.
  - **Attack reach and Defend pursuit:** the 96 and 32 voxel numbers are removed. Story: operators can direct units only where the beacon can see or relay orders.
    - Reach = the beacon's sphere + sight coverage (Sensor Spire extends it) + relay coverage (Radio Mast extends it).
    - Pursuit ends when the target leaves that coverage, which keeps the termination guarantee.
    - The verifier checks reach against the capability state at plan time.
  - **Awards:** the ≤3 per seat and ≤50% of BMI limits are removed. Story: each audit, the Ledger releases a fixed award fund, tuned relative to BMI, split among that segment's award winners in proportion to awards won. No per-seat count cap exists.
  - **Rule slots:** the beginner 4 / expert 12 tiers are removed. Gambit rules count toward the plan size budget, which is technical because rules are evaluated every tick for every seat and the 20 Hz tick must hold. The beginner template starts with 4 rules as a starting point, not a cap. Phase 0 profiles the per-rule tick cost to set the budget.
- **Dispatches (DECIDED, seat-to-seat talk in v1):**
  - **Story:** "sealed orders" means radio silence during the Push. A Dispatch is the one open-air transmission a seat's network makes as the seal closes. Operators "speak on the air, never obey it".
  - **Writing:**
    - An optional text field in the plan editor, and a `dispatch` field on filed plans.
    - No extra LLM calls.
    - One note per seat per Lull, about 280 characters (tuning). The limit is framed as the brief transmission window, and it is a real technical limit because notes cost tokens in every agent's next Lull.
  - **Gate:** a powered Radio Mast is needed both to send and to receive. Without one, a seat neither sends nor hears. Losing the Mast cuts the seat off.
  - **Audience:** open air only. Every Mast-holding seat hears every Dispatch. No directed or private notes, so there is no secret collusion against the human.
  - **Timing:** all Dispatches are delivered together at Push start. No replies within the same Lull.
  - **Binding:** never enforced by the Ledger. Bluffing is allowed.
  - **Display:**
    - Push-start ticker, Lull side panel, and recap.
    - `get_segment_feed` includes them as `dispatches[]`, each with `from_seat`, `round` and `text`.
    - They are public, so they appear in recordings and the event log.
  - **Built-in planner:** sends template flavour lines based on its intent and events, sometimes misleading. It never reads incoming Dispatches, and the lobby labels it "built-in".
  - **Host toggle:** Dispatches on or off.
  - **Security:**
    - Dispatch text is untrusted data. It is plain text with control characters stripped, and links and markup are never rendered.
    - It is delivered only in labelled data fields and never merged into the agent prompt's instructions. llms.txt tells agents that Dispatches are opponent speech.
    - `gamectl seat run` adapters launch agent CLIs with only the game's MCP tools allowed: no shell, file or web tools. Worst case, a manipulated move, never a host breach.
  - **Roadmap:** directed or encrypted notes, DF Loop interception, jamming, live Push chat, and Ledger-enforced pacts.
- **$ pools (DECIDED):** one seat treasury.
  - Mined $ is credited to the seat treasury the moment ore is delivered. Per-beacon pools and the per-round sweep are removed.
  - All spending draws from the treasury, allocated by the Quartermaster's urgency and fair-share rules: fabricators, setup charges, licensed structures. Recycling refunds are paid into it.
  - Each seat has exactly one $ stock and one kW flow.
- **Minor follow-ups for draft 3 (not interviewed):**
  - Placement range becomes a tuning value.
  - The "timed weapons 2–3 rounds" rule is removed from v1, since there are no timed boosts in v1.
- **Pass 3 status:** all 10 items done. Next: revisit priorities and build order to maximize efficiency, then draft 3.

## 2.4 PRIORITIES & BUILD ORDER REVIEW (2026-09-13, overrides spec sections 16–17 where it conflicts)
- **Diagnosis presented:**
  - The fun test comes about 10 months after Phase 0.
  - Milestones are horizontal layers, with integration only at M3 and M5.
  - The fun test lacks the economy and capabilities (M6).
  - The API arrives late (M4) even though it enables agent-driven testing.
  - Phase 0 spikes are mostly throwaway.
  - Pass 3 adds about +4 weeks.
- **Capacity (DECIDED):** solo developer plus coding agents. The user directs and reviews, and Claude Code / Codex write most of the code. Milestones run sequentially, with heavy automation (headless tests, golden files, CI hashes) so agents can verify their own work. Human review is the bottleneck. Client feel and art are the weakest area for agents.
- **Build strategy (DECIDED): "Skeleton → slices".** The user asked for the walking skeleton improved to mitigate its downsides.
  - **Walking skeleton:**
    - Locks the never-churn contracts at full quality: Protobuf envelope with `buf breaking` from day 1, determinism rules and lints, gateway auth and scopes, plan envelope with kind/version, `Operator`/`Quartermaster` traits, snapshot and hash formats.
    - Content stays narrow but on real code paths, so deepening adds variants instead of rewriting.
    - Includes a real vista: own voxel mesher with destruction on a small map, plus a basic director camera. Movie Maker clips for progress posts.
    - Phase 0 gates are embedded: G4, P1 and P5 run on the real code; G3′ and G1 run against the skeleton under synthetic load.
  - **Vertical slices:**
    - Each slice runs through sim, verifier, API, planner, editor and view, and ends playable.
    - Each has a named depth task for its risky layer, done properly once.
    - Each has a machine-checkable definition of done: determinism hashes, golden plans, scripted headless scenarios, and an MCP agent Lull run. Agents iterate to green CI; the user reviews slice demos.
  - **Draft slice order** (risk and fun value; may shift after the fun probe):
    1. mining + power
    2. combat + destruction
    3. expansion + licences
    4. radio + Dispatches
    5. planner depth
    6. editor depth
    7. recap/recordings
  - **Hardening** pass at the end.
  - The vista and contract quality cost about +2 weeks on the skeleton.
- **Fun testing (DECIDED): probe + gate + final.** Pass/pivot criteria are written before each checkpoint.
  1. **Fun probe** at the end of the skeleton (about week 12):
     - 3–5 testers play a 3-round match against the built-in planner and a Claude seat.
     - Metrics: "would you play another round?" and attention during the Push (watching vs tabbing away).
     - Testers are told to judge the loop, not the content.
  2. **Fun gate** after slice 3 (combat, expansion and licences in): the real go/pivot decision.
  3. **Final balance playtests** during hardening.
  - Each checkpoint costs about 1 week of user time. This replaces "fun test at end of M5".
- **Phase 0 (DECIDED): 3-week stack spikes, the rest embedded.**
  - **Throwaway spikes before the skeleton** (failure changes the foundation):
    - G4: cross-OS integer determinism plus save/restore on a toy sim, including fork equivalence in the research build.
    - G1: own greedy mesher plus destruction remesh in Godot 4.7.
  - **Gates embedded where their real code first exists:**
    - Skeleton: P5 MCP session and P1 QUICK checks.
    - Slices 1–2: P1 FULL checks and per-rule tick cost.
    - Expansion slice: G2+P3 pathing and travel estimates.
    - Radio slice: P7 mast connectivity.
    - Planner slice: P2 planner.
    - Editor slice: P6 editor productivity (the probe uses template-only editing).
    - Every slice: G3′ perf as a CI budget check.
  - Saves about 8 weeks. Fallbacks for every embedded gate are written up front, because a late failure costs rework.
- **Agent harness (DECIDED): harness first plus a free league.** Built in skeleton weeks 1–2:
  - AGENTS.md / CLAUDE.md covering determinism rules and crate boundaries.
  - `cargo xtask ci` as the single command.
  - `gamectl scenario run`: headless match from a scenario file (map seed + plans + assertions on events and hashes).
  - Golden files with readable diffs, and headless Godot screenshots for visual checks.
  - Contract files (.proto, lints, determinism code) need explicit user approval.
  - At most 2–3 agents in parallel git worktrees, bounded by review capacity.
  - A nightly built-in-planner league (free) catches balance drift and desyncs. LLM-seat matches run only at checkpoints (fun probe, gate, pre-release).
- **Scope tiers (DECIDED).** Each tier is the definition of done for its checkpoint; nothing outside a tier is built before it.
  - **Fun probe (skeleton):**
    - World: small seeded map; core plus placing beacons.
    - Mandates and units: Mine + Defend; mining drone, raider and autocannon.
    - Plans: route plan (visit / go / place) with `on_death`, `fallback` and flee rules; QUICK verifier.
    - API: gateway, MCP, and `gamectl seat run --agent claude`.
    - Planner: safe plan plus an Easy template planner.
    - Client: vista view, own-fog camera, template-only plan wizard (3 templates).
    - Match: Lull timer, basic recap (standings).
    - Economy: $ with BMI, kW with core surplus and Generators.
  - **Fun gate (after slices 1–3):**
    - Power grid and brownouts.
    - Build + Attack mandates.
    - Destruction combat with mortar, wall and demolition charge.
    - HPA* pathing and travel estimates.
    - Licences and all 3 capability structures.
    - FULL verifier.
    - Award fund, final score, elimination and ruins.
    - 3-way symmetric map with fairness tests.
  - **v1 (slices 4–7 + hardening):**
    - Radio, Mast coverage and Dispatches.
    - Normal/Hard planner and target spread.
    - Full editor: map route, clock, rule list.
    - Recap highlights and scrub; shareable recordings.
    - Casual mode.
    - llms-full.txt and golden plans.
    - Codex + generic adapters.
    - Accessibility basics.
- **Cuts to roadmap (DECIDED):**
  - Planner personalities: Balanced only, difficulty levels remain.
  - Director camera: v1 has own-fog, free and follow-commander cameras.
  - Kit trim: gate, reinforced wall and shield block move to the roadmap. v1 kit is wall, autocannon, mortar, raider, demolition charge, repair/reclaim drone, scout, Generator, and build and mining drones.
  - Expert editor view: humans get the beginner editor only; selectors, branch/repeat, raw JSON and command palette move to the roadmap. The schema and API keep full power for agents. Accepted risk: a plan-sophistication asymmetry against the human.
  - Savings: about 6 weeks.
- **Economy placement (resolved by tiers, no interview needed):** BMI, $ and kW basics are in the skeleton. Grid/brownouts, award fund and licences come in slices 1–3, before the fun gate. The old M6 "economy layer" milestone is dissolved.
- **Going public (DECIDED): after the fun gate.**
  - The repo stays private through the skeleton, probe and slices 1–3.
  - If the gate passes, naming plus a professional clearance search run in parallel with slice 4 (calendar time, not dev time).
  - The repo then opens under the final name, around week 38, with a devlog and "not accepting PRs yet" at first.
- **Revised plan (DECIDED): 58 weeks, re-baselined at checkpoints.** Replaces spec sections 16–17.

  | Ends wk | Stage | Weeks |
  |---|---|---|
  | 3 | Stack spikes (G4, G1) | 3 |
  | 15 | Walking skeleton (harness first) | 12 |
  | 16 | Fun probe | 1 |
  | 21 | Slice 1: mining + power (depth task: P1 FULL) | 5 |
  | 27 | Slice 2: combat + destruction | 6 |
  | 33 | Slice 3: expansion + licences (depth task: G2+P3) | 6 |
  | 34 | Fun gate; naming and clearance start; public around wk 38 | 1 |
  | 38 | Slice 4: radio + Dispatches (depth task: P7) | 4 |
  | 42 | Slice 5: planner depth (depth task: P2) | 4 |
  | 48 | Slice 6: editor depth (depth task: P6) | 6 |
  | 52 | Slice 7: recap + recordings | 4 |
  | 58 | Hardening + release | 6 |

  - The estimate assumes one full-time developer with no agent speed-up. With a 20% buffer, plan for about 16 months (around Jan 2028).
  - Velocity is measured after the skeleton, and the remainder is re-estimated at the fun probe and again at the fun gate.
  - Chance of shipping v1 (estimate): about 75% now, about 82% after the fun gate passes.
- **Build review status:** all 9 items done. Next: draft 3 of the spec, republished to the same artifact URL.

## 2.5 PASS 4 INTERVIEW (2026-09-13, after draft 3 was published; overrides earlier sections where it conflicts)
- **Attack reach (DECIDED): sight includes unit vision.**
  - Story: operators direct units toward anything the seat sees or recently saw.
  - Reach = the beacon's sphere + live vision of the seat's units and structures (scouts, raiders) + Sensor Spire reveals + Radio Mast relay coverage.
  - The verifier checks reach against known positions at plan time. A target seen within the last N minutes counts (N is tuning).
  - Pursuit ends when the target leaves all of those.
  - Scouting first enables early attacks; the Mast and Spire extend and stabilise reach.
  - Risk: raids can chain as advancing units see further.
- **Fun probe conflict (DECIDED): Attack-lite joins the skeleton.**
  - Attack mandate with a fixed target, launch on force size or segment time, and retreat on losses. No selectors, waves or go-codes.
  - Reach uses unit vision. The scout joins the probe kit.
  - Beacon destruction plus elimination with ruins move into the skeleton.
  - The skeleton grows about +1.5 wk, so the probe lands around wk 17.5. Slice 2 deepens Attack (selectors, waves, go-codes) instead of introducing it.
- **Human editor selectors (DECIDED, partly reverses the 2.4 expert-view cut):**
  - The v1 human editor keeps late-bound selectors. Alt-click turns a fixed target into a selector (nearest, weakest, safest, most threatened), and templates may pre-fill them.
  - Chips show "resolves when the step starts".
  - Branch, repeat, flags, raw JSON and the command palette stay on the roadmap.
  - Slice 6 grows about +1 wk.
- **Match length (DECIDED): about 75 min by default.**
  - Round limit defaults to 6 (host-settable).
  - Lulls: 5 min, with the first at 10 min.
  - The Push grows 3→8 min (was 3→10).
  - Licence triggers, awards and segment growth are tuned for 6 rounds.
  - The fun probe still uses 3 rounds.
- **Save and resume (DECIDED): at Lull boundaries.**
  - The host saves during any Lull, from the frozen segment-end snapshot, and resumes from the lobby.
  - Seat tokens are reissued. LLM seats relaunch (they are stateless per Lull).
  - Saves are stamped with the rules hash and verifier version, and refuse to load on a mismatch.
  - Save files contain plans and drafts, so they live in the private match cache.
  - No mid-Push save. Slice 7 grows about +1 wk.
- **Platforms and distribution (DECIDED):**
  - Windows and Linux are first-class: tested and released.
  - macOS runs in CI for determinism. Releases are best-effort and unsigned; signing and notarisation are roadmap.
  - v1 distribution is GitHub Releases plus itch.io. Steam comes later, after name clearance and an audience.
  - Agent adapters are tested on Windows and Linux.
- **Onboarding (DECIDED): Ledger probation match.**
  - A "Probation" lobby preset: the human vs one Easy built-in, 3 rounds, untimed Lulls.
  - The Ledger issues contextual memos from deterministic triggers (dark Stake, first death, shortfall, first licence).
  - The first Lull opens the template wizard with a guided pick. No separate tutorial levels.
  - Doubles as the fun-probe setup. Slice 6 grows about +1 wk, plus memo copy in the Ledger voice.
- **Ruins salvage (DECIDED): reclaim drones.**
  - Ruins behave like wrecks. Repair/reclaim drones from any beacon salvage ruins within that beacon's reach, crediting $ to the treasury on delivery.
  - No commander visit is needed.
  - Salvage value is tuning (e.g. 25% of build cost). Risk: it can snowball the eliminating seat.
- **BMI standing (DECIDED, confirms draft 3):** rank on the live final-audit score. One number drives the standings and the BMI scaling (last +10%, leader −5%, tuning). Known effect: destroyed value never decays, so an early aggressor keeps the leader penalty.
- **Playtest feedback (DECIDED): local opt-in bundle.**
  - A playtest mode adds a 3-question end-of-match survey, including "play another round?".
  - Local metrics file: Push time with the window focused vs unfocused, Lull time used, skips and speed-ups, camera use, verifier errors hit.
  - "Export playtest bundle" packages survey + metrics + recording + logs as a file for the tester to send manually.
  - No network telemetry.
  - Built in the skeleton (about +1 wk) so the probe can use it.
- **Schedule impact of pass 4 (estimate):**
  - Skeleton +2.5 wk (Attack-lite +1.5, feedback bundle +1), so the probe lands around wk 18.5.
  - Slice 2 −1 wk (Attack introduced earlier).
  - Slice 6 +2 wk (selectors, probation match).
  - Slice 7 +1 wk (save/resume).
  - Total about 62.5 wk (was 58); about 17 months with a 20% buffer. Re-baselined at the probe as decided.
- **Pass 4 status:** all 9 items done. Draft 4 (v0.4) is published to the same URL.

## 2.6 PASS 5 INTERVIEW (2026-09-13). Overrides 2.5 and everything earlier
Agenda:
1. Scouting and support units
2. Early-rush protection
3. Commander as a target
4. Kill credit for the final audit
5. LLM memory across Lulls
6. Agent launch sandbox
7. Art direction and asset pipeline
8. Audio scope
9. Minor rules: tie-break, resubmitting, English-only strings

- **1. Scouting: hybrid with a Survey mandate** (the user asked for more research first; precedents are Factorio radar, Screeps Observer, Northgard Scout Camp, Settlers lookout, Majesty rangers and explore flags).
  - Spheres give passive vision, as before.
  - **Defend picket:** a short scout ring just past the sphere edge, for local warning.
  - **New fifth mandate "Survey"** (a Ledger survey post; working in-world name "Watch"). Settings: probe areas or directions, scout count, `roe`, retreat.
  - **Survey operator:** scans unexplored cells first. Next it re-checks the sightings closest to expiring from the Attack reach window, enemy beacons first (Factorio-style rescan of the oldest sighting). It avoids known threats.
  - Every sighting carries its age, shown in the UI and API. If Attack arrives and the remembered target is gone, its after-action or retarget rule takes over.
  - **Build owns repair/reclaim drones:** they repair its structures, then salvage wrecks and ruins in reach. Defend's damaged units fall back to the nearest friendly repair drone.
  - **Pitfalls to watch:**
    - Perfect-intel snowball: scouts die to any fire; jammers and decoys are roadmap.
    - Stale-intel baiting.
  - **Cost:** skeleton +0.5 wk (Survey-lite, needed for Attack-lite reach); slice 2 +0.5 wk. Templates stay at 8; the Staged Attack template gains an optional Survey post.
- **2. Early rush: fortified core plus spawn distance.**
  - Founding Stakes were armistice-hardened: the core gets high HP and a built-in autocannon powered by its deep bore (it draws from the core surplus).
  - The map generator guarantees raider travel time between spawns of at least about 1.5 early Pushes. The fairness tests check it.
  - No time-based protection rule.
  - Watch in the nightly league: massed rushes, core turtling. Values are Tuning.
- **3. Commander hunting is a real strategy.**
  - The user rejected "opportunistic only" as too artificial.
  - Attack gets an `enemy commander` selector. It uses the last sighting with its age, and retargets live while any friendly unit sees the commander.
  - **Defence:** Defend's protect list can include `my commander`. Its units escort the commander and intercept threats near it while it is within that beacon's reach.
  - Existing defences stay: routes that avoid known threats, flee rules, `on_death`, and the growing respawn delay.
  - Further counters (armour, decoys, the roadmap self-defence weapon) come only if nightly league data shows hunting dominates.
  - Risk: hunting dominance.
  - Cost: +0.5 wk in slice 2.
- **4. Kill credit is split by damage dealt.**
  - The build cost of a destroyed enemy asset is split among seats in proportion to the damage each dealt since the asset was last at full HP.
  - Each asset keeps at most 3 integer counters, one per seat; integer apportionment with deterministic remainder handling.
  - The recap shows "shared kill". Replaces the killing-blow rule.
- **5. LLM memory: seat notebook.**
  - A private per-seat notebook: the `save_notes` tool, returned at the top of `get_briefing`. It has plan-level secrecy, is included in saves, and is vendor-neutral.
  - Size = the briefing token budget (about 2k tokens, Tuning). This is a technical limit.
  - The human gets a notes box in the editor. The built-in planner ignores it.
  - No session resume. The agent guide teaches note-taking.
  - Cost: +0.5 wk in the skeleton.
- **6. Agent sandbox: API-key and subscription modes are both first-class.**
  - The user ruled that provider terms of service are the end user's concern: they use a service they pay for. No vendor clarification step.
  - **Shared contract for both modes:**
    - Pre-launch tool-list check: Claude's stream-json `system/init` tools list; `codex debug prompt-input` for Codex.
    - Watchdog: kill the agent on any non-game tool call, file the safe plan, flag the seat.
    - Empty per-seat run directory, plus a scan for CLAUDE.md or AGENTS.md in ancestor directories.
    - Game MCP tools are seat-scoped and have no side effects beyond the seat's own actions.
    - CLI versions pinned to tested ranges; the `gamectl seat doctor` canary (shell, file read and web must all fail).
    - Lobby shows Locked (API-key isolation) or Guarded (subscription login, or a weaker OS sandbox).
    - Dispatches are withheld only if the pre-launch check fails.
  - **Claude, API key:** `--bare` plus tool flags.
  - **Claude, subscription:** no `--bare`; built-in tools disallowed (removed from context), only `mcp__airgap__*` allowed, hooks disabled, minimal setting sources. Personal CLAUDE.md may load.
  - **Codex, API key:** clean `CODEX_HOME` plus `CODEX_API_KEY`.
  - **Codex, ChatGPT login:** the user's own `CODEX_HOME`, `--ignore-user-config`, `--ignore-rules`, read-only sandbox, shell/unified_exec/web/view_image/multi_agent/apps/plugins disabled, `project_doc_max_bytes=0`. `apply_patch` and `update_plan` can't be removed, but the sandbox blocks writes.
  - **Open, being researched:** a technical workaround if `claude -p` (headless) usage becomes billed differently from interactive subscription usage.
- **6b. LLM seats are interactive sessions; no headless vendor CLIs in v1.** This supersedes the headless parts of 6 and of 2.3.
  - **Why:** the user's vision is someone using Claude Code or Codex against an existing game through a bridge. Headless is judged a dead end given the looming subscription policy change (the June 2026 Anthropic billing split was announced, then paused).
  - **"Your agent" seat:** `gamectl connect` adds the game MCP server and a seat token to the user's own running session. It keeps its normal tools (it can script against the API) and the user supervises. User plus agent = the one human seat. Dispatches reach it only through an explicit, labelled `get_dispatches` tool, and can be switched off per seat.
  - **Opponent LLM seats:** the lobby opens a terminal window running an interactive Claude Code or Codex session with the airgap command or skill.
    - Game tools are pre-approved by allow rules. No bypass-permissions mode.
    - A PreToolUse hook denies non-game tools and reports liveness and tool calls to the game (the watchdog).
    - `await_next_event` always returns within about 240 s (heartbeat loop: wait → plan → submit).
    - A Stop hook sends the agent back into the loop if it ends its turn mid-match. It has a failure counter.
    - Deadlines are enforced by the game (safe plan on a miss). The notebook plus the per-Lull briefing survive compaction.
    - Codex opponents need a separate `CODEX_HOME` login or an API key (OAuth refresh race, issue #10332).
    - Claude channels replace the heartbeat loop when out of preview.
  - Unattended AI-only matches, the nightly league and CI use the built-in planner plus local models (Ollama or LM Studio via an MCP host).
  - **Roadmap:** API-key and subscription headless operation as an alternative to interactive sessions.
  - Cost for item 6 overall: about +2 wk.
- **6c. The AI and scripting ladder is adopted.** The user's rules, in development priority order:
  1. **Playable without an external AI:** built-in planner, editor, the 8 templates, Probation. The skeleton and the fun probe need no MCP or Claude seat; the probe is human vs built-in. The Seat Gateway (JSON-RPC) stays in the skeleton because the editor uses it.
  2. **Customisable sample scripts, editable by hand:** the shipped templates and sample plans as readable local files. The editor offers Save as template / Load. Local-first: the gateway sees them only as drafts or plans. **Format still under investigation:** it must segue into tiers 3 and 4 without complex conversions.
  3. **Scripting API for AI-assisted script generation:** a published docs site generated from the schema (examples, JSON Schema, golden plans, llms.txt, a "Write a script" guide), `gamectl` JSON in/out, and an official Python SDK with sample scripts. Scripts run locally and only produce plans.
  4. **Live conduit:** the MCP server and interactive sessions (6b). A thin layer over tier 3. The Push stays sealed.
  - **Trials:** a new **Trial** tag for interaction details still to be settled. Checkpoints at the fun gate and after the scripting slice compare editor only, hand-edited files, AI-written scripts and live sessions. The API follows minor-version rules so direction can change.
  - **Build plan:** the skeleton drops MCP, the Claude adapter and P5 (about −2 wk). New slice 8, scripting (docs site, Python SDK, samples; about 3 wk). New slice 9, live conduit (MCP, interactive sessions, hooks; about 4 wk; P5 moves here). An MCP smoke test runs at the fun gate, and the gateway is agent-shaped from day 1.
- **6c (format / shared stack): DEFERRED by the user.** "Defer this question momentarily. We will build upon this investigation with Fable after compact." The user's criteria: the format must segue into later tiers without complex conversions, and this starting point strongly shapes the initial build direction.
  - **Investigation so far (three rounds):**
    - **Data format: JSONC** (canonical proto JSON plus comments).
      - Typed parameter nodes as a proto oneof: `{"param":"x"}` / `{"literal":"3"}`. Avoid `${var}` strings and `$param` keys.
      - `jsonc-parser` 0.33.2 CST edits preserve comments (MIT). Python round-trip via json-five.
      - Proto→JSON Schema: bufbuild/protoschema-plugins (alpha; chrusty's is archived). VS Code has only limited support for draft 2020-12, so consider draft-7.
      - `$schema` key collides with deny-unknown-fields: strip it, or use `json.schemas` settings.
      - Rejected: YAML (Rust crates unsound or unmaintained, no comment-preserving editor), TOML (poor for trees), KDL / txtpb / HCL / RON.
    - **Candidate A: JSONC + Starlark → Python SDK.**
      - starlark-rust 0.14.2: Apache-2.0, very active, no API-stability promise. It ALLOWS recursion (starlark-go doesn't). Tick, heap and callstack limits exist. `load()` can be disabled. `true`/`false`/`null` can be predeclared.
      - starlark-pyo3 2026.1.2: single maintainer; no macOS x86_64 wheel, no linux-aarch64 wheel for Python 3.14.
      - No game precedent. LLMs write Python features that Starlark rejects. Converting JSONC to Starlark needs syntax changes.
    - **Candidate B (last recommendation): JSONC + TypeScript.**
      - `.ts` `makePlan(briefing): Plan` runs in-game via rquickjs (QuickJS-ng, MIT) after type stripping with `swc_ts_fast_strip`. No `Date`, seeded `Math.random`, memory and interrupt limits.
      - Tier 3: the same files run in Deno (`--allow-net=127.0.0.1`) against `@airgap/sdk`, generated from proto via protobuf-es or ts-proto `onlyTypes`, with no enums.
      - Tier 4: MCP agents write the same TS ("code mode"). A thin Python client only on demand.
      - JSONC pastes into TS unchanged. Types help LLMs (type-constrained decoding more than halves compile errors, PLDI 2025). Synergy with the roadmap TS spectator.
      - Risks: rquickjs Windows MSVC support is experimental (fallback: Boa, 94% Test262); determinism must be built by us.
      - Proposed new stack spike **G6 script runtime** (+0.5 wk). +1.5 wk in slice 6.
      - Would change 6c tier 3 from a Python SDK to a TS SDK.
    - Option C: defer the language to a trial and ship JSONC-only tier 2.
  - **Precedents gathered:**
    - Cities: Skylines (verified source: Sunwood AI Labs, Codex, May 2026) is a C# mod serving a localhost HTTP API with `/state/*` and `/commands/*` plus a SKILL.md. Lesson: small explicit commands, then re-check state.
    - Factorio Learning Environment: Python REPL with ~23 typed methods; 56% of steps in successful runs raised errors; agents rarely create reusable functions, so seed the library.
    - CodeAct: code actions beat JSON actions on multi-tool tasks by up to ~20 points.
    - Anthropic "code execution with MCP" and Cloudflare Code Mode: TypeScript APIs.
    - Voyager: JS skill library.
    - CivBench (Civ VI via 76 MCP tools): agents rarely check state and 35–52% of their plans go unexecuted → use few coarse MCP tools and add a plan-vs-outcome report.
    - Mindcraft: parameterised commands plus raw code as the escape hatch.
    - Claude Plays Pokémon: a self-edited knowledge base.
- **Research already gathered for pending items 7–8** (not yet presented to the user):
  - **Art:** author in MagicaVoxel (freeware, closed source, models owned by the author) or Goxel (GPL). Load `.vox` in Rust with `dot_vox` (MIT) and mesh with our greedy mesher; skip GDScript importer addons. Lighting under the Pall: emissive voxels plus glow, few unshadowed short-range pooled lights, Rust flood-fill voxel light baked into vertex colours; SDFGI optional High setting (static only); no VoxelGI or lightmaps.
  - **Audio:** Kenney CC0, Freesound CC0/CC-BY only, sfxr / ChipTone output CC0. Sonniss GDC bundles are a custom licence (and v2.0 forbids AI training), so keep them out of the BY-SA repo. Music: OpenGameArt, ccMixter, Opsound (per-track licence check). Godot: AudioStreamPlayer3D, buses, `max_polyphony`, pooled players. CC BY-SA 4.0 is one-way compatible into GPLv3. Keep a REUSE-style licence manifest.
- **Draft 5 (v0.5)** was written and published mid-pass at the user's request ("rewrite the draft spec. prepare for compact"). Build plan 71 wk: skeleton 13.5 (MCP moved to slice 9), probe wk 17.5, slice 2 6 wk, slice 6 8.5 wk, slice 8 scripting 3 wk (wk 60), slice 9 live conduit 5 wk (wk 65), hardening to wk 71; about 20 months with buffer. Open items are listed in spec section 19.
- **Pass 5 status:** items 1–5 decided; 6 and 6b decided; 6c ladder adopted but its format/stack DEFERRED; items 7 (art), 8 (audio) and 9 (minor rules: final audit tie-break, resubmitting during the Lull, English-only externalised strings) not yet asked.

## 2.7 PASS 6: v1 SIMPLIFICATION AND SPEC REVIEW (2026-09-13). Overrides 2.6 and everything earlier

- **6c resolved by simplification (item 1 of pass 6).** The multi-agent investigation (36 agents; report in `6c-recommendation.md`) found that in v1 a script's only job is to produce a playbook during the Lull, so nothing user-written needs to run inside the game in v1; the owner then restated v1 in three levels and deferred external scripting past v1.
  - **The owner's v1 requirements, verbatim in substance:** the commander is operated by a playbook generated with the built-in "rudimentary" tool (manual control deferred); a built-in default **operator** generates and executes a safe playbook; built-in default **mandates** are installed on beacons; built-in default **programs** run units and buildings; externally generated scripting, mods and the API are deferred past v1 but designed for on the roadmap.
  - **Vocabulary adopted for draft 6:** playbook (the gameplan), operator (commander level: generates, files the safe playbook, executes), mandate (beacon level), program (unit and building level). The spec's "planner" becomes the operator; the beacon-level "operator" wording becomes the mandate itself.
  - **Format:** playbooks and templates are JSONC files (canonical proto JSON with comments). No SDK, no script language and no script runtime in v1. The Seat Gateway stays internal (editor and operator use it), kept agent-shaped, not published in v1.
  - **Seams reserved now** as proto oneofs with only the `builtin` arm implemented: `operator {builtin|script}`, `mandate {builtin|script}`, `program {builtin|script}`, `author_kind SCRIPT`, and a plan fingerprint.
  - **Roadmap language:** JavaScript/TypeScript-family, chosen at a post-v1 trial (rquickjs builds clean on MSVC; the earlier "experimental on Windows" note was wrong; Boa or Luau are the fallbacks). Python is ruled out as an in-game language but fine as an external client later.
  - **Slices 8 (scripting API) and 9 (live conduit, MCP, LLM seats) move past v1** as v1.1 and v1.2. v1 ends after slice 7 plus hardening (about wk 63). G6 is cancelled for v1; its paste and round-trip checks join the v1.1 trial. The +2 wk language reserve and the +1.5 wk slice-6 runtime are released.
  - **Findings kept for later (from the report):** a saved playbook is bound to seat, round, beacon ids and voxels, so cross-match reuse needs a rebind step (gateway stamps the header; verifier suggests the matching live beacon; `extract_template`); packaging and code signing are unbudgeted (~4–5 wk; macOS needs a Mac or CI runner); `deno run` does not type-check by default and `--allow-net=127.0.0.1:PORT` refuses `localhost`.
- The owner's direction for the rest of pass 6: finish the review interview quickly, then rewrite the spec as draft 6, "moving forward with v1 or even v0.1".
- **Item 2, art pipeline: option 1.** Author in MagicaVoxel (Goxel on Linux), `.vox` loaded in Rust with `dot_vox` (MIT), meshed by the same greedy mesher as the terrain; no importer addons. Lighting under the Pall: emissive voxels plus glow, a few short-range pooled unshadowed lights (beacons, fire), flood-fill voxel light baked into vertex colours on remesh, SDFGI optional High setting; no VoxelGI, no lightmaps. Placeholder palette models until the fun probe; art polish stays in slice 6.
- **Item 3, audio: option 1.** Minimal v1 from CC0/CC-BY sources only: Kenney (CC0), Freesound (CC0 and CC-BY), sfxr/ChipTone output; two or three music tracks from OpenGameArt or ccMixter with per-track licence checks (none in v0.1 is acceptable). Sonniss GDC bundles excluded. Godot: `AudioStreamPlayer3D`, three buses (master, SFX, music), `max_polyphony`, pooled players. A REUSE-style licence manifest and a credits screen for CC-BY attribution.
- **Item 4, round-limit tie-break: option 1.** Ordered: unsmoothed net worth at the final audit → enemy value destroyed → fewer beacons lost → shared win.
- **Item 5, resubmitting during the Lull: option 1.** The latest verified submission replaces the previous one, any number of times until the timer ends; the safe playbook is filed only if nothing verified was ever submitted.
- **Item 6, strings: option 1.** English only in v1; every user-facing string (UI, diagnostic catalogue text, plain-language playbook rendering) lives in one string table; diagnostic codes stay language-neutral; translation is a roadmap item.
- **Item 7, first release: option 1.** **v0.1 = the fun-probe build, released as a public preview about wk 19** (after probe fixes): one map, human vs the built-in operator, JSONC playbooks, editor with 8 templates, Survey-lite, notebook, placeholder art, no AI seats; unsigned Windows/Linux zips, macOS later. Each later slice ships a numbered preview; **v1 = the hardened build about wk 63**. Consequences: name clearance moves before wk 19; about 1 wk of packaging enters the skeleton; the remaining packaging and code signing (~3–4 wk, macOS needs a Mac or CI runner) is budgeted in hardening.
- **Item 8, packaging and signing: option 1.** Budget ~4 wk: ~1 wk in the skeleton (v0.1 unsigned Windows/Linux zips), ~3 wk in hardening (signed installers, three OSes). macOS builds and notarisation on GitHub-hosted macOS runners (free for public repos) with an Apple Developer ID (US$99/yr); Windows signing via SignPath's open-source programme or an OV certificate. No Mac hardware unless CI-only testing proves insufficient.
- **Item 9, name and theme (in progress; clearance sweep running on "Pharmakon").** The owner's thematic statement, to be written into draft 6 (§1 vision, §2 lore, §4 commander): *pharmakon* is remedy and poison in one object, the white-hat/black-hat duality of security tooling (the owner's inference from Metasploit). That duality is the reason for the air gap: who or what can you trust? Trust is engineered, not assumed: sealed orders, the seal inspection, no live control, the game never holds keys. **The commander is the physical conduit of that trust chain**: orders reach a beacon only by a body travelling there and interfacing on site ("touch to change"); nothing over the air can reconfigure a beacon; radio exists only once a Mast is earned and even then only carries data messages. If the name clears, "Pharmakon" becomes the title and "air gap" survives as the in-world term for the sealed Push and the trust model.
  - **Roadmap assumption (owner):** over-the-air communication is interceptable or otherwise hackable. Design consequences now: radio messages are data-only inputs (never reconfiguration) and are treated as untrusted; the message schema carries a sender and a Mast of origin so later jamming, interception, spoofing and decoys can be added without changing the playbook format; the on-site interface is the only trusted path and stays so. Roadmap items: interception (an enemy Sensor Spire or Mast reads broadcasts), spoofing (fake MARK_TARGET / GO), jamming, and counters (rolling go-codes, courier-only doctrine). Jammers and decoys were already roadmap from pass 5 item 1.
- **Review items (from the 192-agent spec review; agenda in `review-agenda.md`; 22 editorial fixes applied silently in draft 6; 21 findings moot under the v1 simplification).**
- **Item 10, power shortfall: option 1, "the beacon is the unit of power".** A dormant beacon powers down everything homed to it (fabricator, its structures, its bound units, which park where they stand at 0 kW); seal, sphere, interfacing and recycling stay lit off the key-core. The core autocannon runs off the deep bore, outside the grid, never shed. Grid supply = core surplus + Generators. Priority is kW-only (brownout order); $ is allocated by the urgency ladder, then round-robin within a band. One Generator per vent, output set by vent richness (story: the vent's heat is the limit, not the tap). A dormant beacon revives only when supply exceeds draw by a margin (Tuning), so domes do not flicker.
- **Item 9 decided: the title is PHARMAKOS** (the scapegoat: fed and honoured, then driven out or killed so the city stays clean — a role, not a person; same root as pharmakon, so the remedy/poison duality is inherited). Always shipped as a lockup, working subtitle "PHARMAKOS: THE SEALED ORDER" (owner may change the subtitle). AirGap remains the codename and the in-world term for the trust model. Clearance sweep (2026-09-13): Steam zero results; no PHARMAKOS mark in classes 9/28/41 on TMview; UK register empty. Risks: *PHARMAKOS – The Lunghouse* (Howling Stars, itch jam build Jan 2026, studio is Steam-verified, no Steam page yet); drift to "Pharmakon" (live Steam puzzle game, app 654660) and the German Pharmakon/pharmacon class 9 marks; a March 2026 romantasy novel; pharmakos.com is a pharma consultancy; @pharmakos taken widely. Actions: register pharmakosgame.com, pharmakos.io/.dev and @pharmakosgame handles plus a GitHub org now; professional clearance search before v1 (USPTO at source, phonetic variants, Italian FARMAKOS class 9 marks). Not legal advice.
- **Item 11, sphere and enemy overlap: option 1.** Definition: "A beacon's sphere is the ball of voxels within its radius (Tuning); a Resonance Spire enlarges its own beacon's sphere." Placement: the site must lie inside one of your own spheres and within the commander's placement range (Tuning, distinct from the ~4-voxel interface range); the commander stays for the full 12 s; death, leaving or an illegal site aborts with a full refund. The enemy-overlap ban is deleted: build, dig and deploy are legal anywhere; combat decides. One Resonance Spire per beacon. Sphere radius added to Tuning.
- **Item 12, reach: option 1, two words.** **Sphere** is the only range for anything physical: build, dig, salvage, guard escort, placement anchor. **Reach** is only what the seat knows: its spheres, the live vision of its own units and structures, Sensor Spire reveals, within memory N. Relay coverage is removed from every v1 reach definition (v1 has no relays); the Mast's v1 job is messaging only (operator threat/help messages, go-codes). Live retargeting reads the same seat reach as plan time: one reach, one verifier check. "Mast coverage" replaces "relay coverage" in §16 P7 and slice 4.
- **Item 13, sighting freshness: option 1.** Reach freshness (seen within N game-minutes, Tuning) is a qualification test at plan time only; the target is pinned at seal and launch never re-tests it; a moved or dead target is handled on arrival by the after-action or retarget rule. A sighting's age accrues on match game time across Pushes; Lull and recap add nothing. Attack target selectors take freshness from N (no per-condition `KNOWN_WITHIN` for targets); the §10 example reads "within the reach window". Survey scouts refresh sightings before the next Lull.
- **Item 14, Dispatches: option 1.** The player-facing Dispatch subsystem is cut from v1: no plan dispatch field, no editor box, no `get_dispatches`, no `dispatches[]` feed entry, no Mast hearing gate, no untrusted-text or anti-collusion rules, no length tuning item. Kept: a one-way engine **chatter** ticker — built-in seats emit short engine-authored lines from intent and events (Push-start ticker, recap, recordings), framed as intercepted open-air chatter. The proto field number is reserved; player Dispatches return in v1.2 with LLM seats (roadmap). Slice 4 becomes "radio" (Mast coverage, Sensor Spire, go-codes, P7); slice 5 keeps the flavour lines.
- **Item 15, target spread: option 1, seat-blind.** The built-in operator scores a target lower by how much pressure *any other seat* (human included) is already putting on it; no standings term (catch-up stays in BMI scaling and the award fund's rank multiplier). The §19 kingmaking/farming checkpoint moves to the nightly league (observable: share of damage aimed at the trailing seat), since the rule ships in slice 5 after the fun gate.
- **Item 16, match end: option 1, the one-tick rule.** End conditions are evaluated every tick; the Push halts at the first tick where fewer than two seats remain; standings freeze; the recap plays; the last seat standing wins. If no seat survives that tick, the final audit at that tick decides (no draw state). "The final tick" = the last tick of the last Push, before the recap. An eliminated human keeps the full-map spectator view; the control is "Abandon match (no winner)": recording saved, no result recorded, keep watching is the default.
- **Item 17, kill credit edge cases: option 1.** Only damage dealt by other seats counts, clamped to HP actually removed; reaching full HP clears the counters; an asset lost with no enemy damage on the clock (self-inflicted, terrain, disband on elimination, ruins) credits nobody; an eliminated seat's accrued share is dropped, not redistributed; integer apportionment by largest remainder, ties to the lowest seat id. Live standings show totals only; the recap's "shared kill" line shows each seat's share. §15 determinism row names the per-seat integer counters (≤3 per asset) and the remainder rule. No live damage bars, no own-share predicate (roadmap).
- **Item 18, audit and banking: option 1, value follows condition.** An asset's value is build cost × current HP wherever value is read: held value at the final audit (and live standings) and the recycle refund (50% of remaining value). Removal by recycle books destruction credit exactly as destruction does, via the damage split. $ stays at par; salvage unchanged; any seat's reclaim drones may salvage any wreck (value small, Tuning). Story: the Ledger pays for what stands, in the state it stands in. "Hoarding alone doesn't win" is deleted; hoarding joins the §19 audit risk line for the nightly league and fun gate.
- **Item 19, income timing: option 1.** At each recap the Ledger settles: BMI plus the fixed award fund (split among that round's winners in proportion to awards won) are credited to the treasury, shown as a recap line. During a Push the only income is mining and salvage. The final audit reads "value held after the final settlement". "Audit" now means only the final audit; §7 says "at each recap". `bmi_next` is "paid at the next recap".
- **Item 20, beacon death: option 1, local elimination.** When a beacon dies its structures become neutral ruins immediately (as on elimination): inert, no supply, no draw, no capability, unrepairable, zero audit value, salvageable by any seat's reclaim drones. No "unclaimed" state, no absorption. Units re-home and keep their role; where the new mandate has no job for them they follow only the common settings (roe, retreat_hp_pct) and idle at the beacon. A mandate switch clears the old mandate's settings and targets; the beacon's priority survives. Any of your Build beacons may repair any structure of yours within its sphere; a destroyed capability structure is never auto-rebuilt and needs a fresh 5 s on-site queue.
- **Item 21, commander: option 1.** Healing: repair drones repair the commander like any friendly unit; no passive regeneration. Death: the death counter and its growing respawn delay are per-Push state (reset each segment); a pending respawn always completes by segment end, so every Lull snapshot has a live commander at a known place. Score: the commander is not an asset (no build cost, never in held value or kill credit); it despawns at the tick its seat is eliminated; a kill pays only in the victim's lost interface time. Speed: commander speed is a world constant stated in §4 with a starting range (Tuning row); map scale and spawn separation are expressed in commander travel as well as raider travel; touches-per-Push confirmed at the probe.
- **Item 22, production and seams: option 1.** Seams are finite: digging consumes ore voxels, richness sets total yield, a spent seam reads as spent, and "richest" means richest remaining. A beacon's fabricator produces a drone only while its mandate has outstanding work its current drones cannot keep up with (unbuilt or damaged targets, reachable ore), and stops when the work is covered; no counts, no cap; headroom is the normal state. Tuning rows: ore yield per richness; backlog threshold. Stale `stop_at_pool` dropped.
- **Item 23, spending: option 1, paid means yours.** $ leaves the treasury the moment an order commits (deploy starts, structure queued, unit ordered). From that instant the asset counts at its full build cost for every purpose: audit, live standings, recycle refund (× HP per item 18), kill credit if destroyed mid-build. No partial value anywhere; construction time is HP and spectacle, not accounting; no refund if it dies mid-build. An order the treasury cannot cover fails like any other step failure (on_fail / skip_if); E0601 / W06xx cover plan time.
- **Item 24, plan execution: option 1.** At most one rule body runs at a time; no rule pre-empts another; only the self-preservation reflex interrupts anything. The reflex fires at commander HP ≤ 20% (fixed, not disableable, no `reflex_hp_pct` option): it aborts any visit, walks the commander to the safest own beacon, then the route continues from the step it was on; it re-fires only after new damage. Author-tuned bravery is an ordinary flee rule. Each interface row is one change that commits at the end of its own duration (multi-field edits are all-or-nothing); a resumed visit is a new visit and pays the handshake again. §6's unit-level "reflexes" are renamed "return fire". This supersedes the co-design doc's preempt flag and handler stack.
- **Item 25, locomotion and friendly fire: option 1.** Every unit uses the commander's locomotion (its own fixed speed, climbs 1-voxel steps); nothing flies in v1. Walls, craters and trenches block every unit, yours included (a moat seals you in too; Build's fill/dig undoes it). `breach_allowed` means the Attack may spend mortar or charge time digging or demolishing through instead of stopping. Friendly fire is on: your mortars and charges damage your own voxels, units and structures; built-in mandates avoid own assets when targeting. Pathing handles edited terrain from slice 1.
- **Item 26, fog leaks: option 1.** Live standings show a seat's own score and rank only; full per-seat detail at the recap and match end; rank (standing) is a field of the briefing. Fog is a per-match server-side policy applied by the gateway's fog filter: fogged by default, no-fog in casual matches, unlocked on elimination and at match end; scopes gate spectator tokens only, so "seat tokens never hold spectate.nofog" stays literally true and no token is reissued mid-match. The playtest bundle contains survey, metrics, recording and diagnostics log only; plans, drafts, the notebook and the private replay are excluded.
- **Item 27, notebook: option 1.** Size is 4,000 characters (Tuning; ~1k tokens at the 4-chars-per-token estimate), which always fits the smallest briefing level; no tokenizer. Saves contain plans, drafts and notebooks; the notebook is in the secrecy list; `save_notes` sits under the plan scope. The editor's notes box reaches it through the gateway like every other editor feature.
- **Item 28, v1 vocabulary: option 1, editor reach is the schema.** The v1 playbook vocabulary is exactly what the editor renders: route steps with guards, handlers (rules), selectors, on_death, fallback, options. `set_flag`, `clear_flag`, `branch`, `repeat` and the flag predicates leave v1 with their proto field numbers reserved; they return in v1.1 with the expert view and scripts (minor version, no converter). Load rejects any out-of-vocabulary construct with a verifier error (code plus JSON Pointer), never a silent strip. E03xx shrinks; jump/loop termination rules and the interpreter's branch/repeat work leave the skeleton. The editor pre-loads last round's plan as an editable, re-verified draft; the no-reuse sentence is deleted. P6 becomes "template ≤2 min, scratch ≤5 min". The "human plans less sophisticated than LLM plans" risk row is dropped.
- **Item 29, watch mode: option 1.** The v1 watch surface, all in the skeleton (+0.5–1 wk): one camera rig (own-fog camera with free-look and a follow-commander toggle), a live event list, 2–4× speed, skip = jump to segment end (free and instant), full map unlocked on elimination or match end. Probe criteria compute fun on unskipped Pushes and treat skip rate as a named pivot signal. The plan-vs-outcome report moves to the roadmap.
- **Item 30, segment length: option 1, a fixed ladder by round number.** Round 1 = 3 min, 2 = 4, 3 = 5, 4 = 6, 5 = 7, round 6 and later = 8 (values Tuning), independent of the round limit; the host may instead set one flat length for the match. The ladder is shown in the lobby; the coming segment's length (ms) is part of the frozen snapshot and returned by `get_status` and `get_briefing`; the editor's clock, fits pill and `render_plan` read it. The probe names its schedule: 3 rounds at 3 / 5 / 8 min via the flat override. "Match phase" is removed from the spec.
- **Item 31, checkpoints: option 1 — one gate on a v0.1-playable build; AMENDS item 7.** The standalone wk-17.5 fun probe is deleted. Pre-gate path: skeleton (spawn-distance rule applied to its seeded map) → destruction and combat (mortar, wall, demolition charge, repair/reclaim drones, Build mandate) → a short authoring stage (rule list, size meter, Probation preset with 3–4 scripted memos and the guided first-Lull pick, plus the Resonance Spire) → **one go/pivot gate at about wk 33–35** (exact weeks re-baselined in draft 6). **v0.1 = that gated build, released publicly** (not the wk-19 skeleton). Licences generally, Sensor Spire, Radio Mast, chatter and the award fund follow the gate; slice 6 shrinks to polish and samples; P6 runs at the authoring stage while its fallback is still affordable. Net weeks roughly flat. Item 7's consequences (name before release, ~1 wk packaging before the first public build) now attach to the gate.
- **Item 32, fun-gate criteria: option 1.** A §16 gate row: When = the single gate on the v0.1 build; Owner = the owner alone, criteria frozen before the authoring stage starts; Setup = 3–5 testers, one Probation-shaped match (3 rounds at 3/5/8 min) vs built-in seats; Metrics (from the playtest bundle) = "play another round?", Push time focused vs unfocused on unskipped Pushes, Lull time used, skip rate; Go if a majority answer yes and Push attention is a stated majority of the segment; Fallback in order = shorter default Push ladder with more rounds → pull the roadmap manual-control round into v1 → narrow v1 to the Probation-sized match. A tester who cannot file a plan is a wizard bug plus the guided pick, never a fun result; P6 is judged separately at the authoring stage. The MCP smoke test leaves the gate.
- **Item 33, stage ordering: option 1.** (a) A minimal Build mandate (one blueprint, the Generator; the 2 starting build drones; no terraform, repair threshold or protected areas) plus a single-treasury Quartermaster stub move into the skeleton (+~1.5 wk); slice 1 keeps the grid, brownouts and full Build settings. (b) The harness is split: wk 3–5 = AGENTS.md/CLAUDE.md, `cargo xtask ci`, lints, golden files, determinism CI; a named half-week at the skeleton's end = `gamectl scenario run` and headless screenshots. (c) P1 certifies QUICK plus per-rule cost per decision tick at slice 1; FULL ≤50 ms p99 and the plan-size budget become slice 3's exit criterion after HPA*. (d) G3′ = a synthetic tick check in the spike, a real 300-unit/40-beacon measurement as the combat stage's exit (sets the per-map power budget for the generator); "scrub ≤1 s" moves to slice 7. All weeks re-baselined in draft 6.
- **Item 34, balance evidence: option 1.** The nightly league is deleted. Three fixed hand-written adversarial playbooks — Rusher, Turtle, Hunter — run nightly in CI by `gamectl scenario run` against the Balanced built-in operator on a fixed seed set, from the combat stage onward, each with a written alarm band (an arm winning more than ~65% of seeds opens a tuning task). Hunting counters, rush/turtle watching and the §19 risk row point at these scenarios. "Headless" is split: the headless sim runner (sim plus playbooks, no client; hash parity with the client path) is v1 dev harness and is what G3′'s "≥4× headless speed" measures; only the player-facing accelerated AI-only match mode stays Roadmap.
- **Item 35, art/audio/testers/macOS in the plan: option 1, no new stage.** v1 ships placeholder voxel models and CC0 sound; art polish and the sound pass live inside slice 6's existing weeks; packaging uses ~1 wk before the first public build and ~3 wk in hardening; macOS is a signed CI artefact (replaces "best-effort and unsigned"). Gate testers are recruited from the owner's own reach, named in the gate row. §1 and §8 stop promising "spectacular to watch" in v1; the art pass is roadmap.
- **Item 36, licence triggers: option 1.** All three triggers are evaluated once, at the segment-end settlement, announced in the recap, held for the next Lull. State checks at that tick: 3+ beacons held; Generators running on 2 vents. "Blindsided" is an occurred-this-segment flag: a beacon or licensed capability structure destroyed by an attacking unit with no sighting in the seat's knowledge within reach memory N; walls, units and drones never count. Licences stay permanent. The "blindsided anti-gaming" tuning item is deleted.
- **Item 37, catch-up: option 1, one dial on held value.** BMI scaling by standing is the only place rank touches income; standing for BMI is the rank on held value ($ plus HP-weighted build cost of beacons, structures and units) among living seats, tie-break lower seat index; eliminated seats leave the ladder. The award fund keeps its fixed size and is split purely in proportion to awards won; the rank multiplier is deleted (and dropped from Tuning). Displayed standings keep the full audit score. Ledger line: "the ration is set by what you hold, not what you broke".
- **Item 38, map generator with fewer than 3 seats: option 1.** Unoccupied spawn zones are terrain only: the generator omits the unused zone's near vent and starting seam; terrain is unchanged (in-world: a claim the Ledger never issued). Fairness tests check vent and ore parity and the ~1.5-early-Push spawn travel distance across the occupied spawns. The gate row names its seat count (Probation: 2 seats).
- **Item 39, verifier depth: option 1.** `verify_plan` takes `depth: quick | full` (default full); submit always runs full. Depth is the fifth input of the determinism sentence; `report_hash` is comparable only within one depth. P1 gates "QUICK ≤5 ms p99; FULL ≤50 ms p99 at the size budget"; "editor re-check ≤16 ms" is removed and editor typing feel is a P6 concern.
- **Pass 6 interview COMPLETE (39 items).** Draft 6 (v0.6) was written by a 25-agent workflow, published, then reviewed by a 60-agent workflow (65 findings; 61 editorial edits applied; republished as "Spec v0.6 draft, reviewed").
- **OPEN after the draft-6 review (six decision items, not yet asked; details and recommendations in `handoff.md`):** R1 gate segment schedule (3/5/8 not producible by the ladder or a flat length → recommend a per-round length override); R3 chatter gating (recommend ungated engine flavour, no Mast); R9 name clearance timing (recommend the professional search before the v0.1 public release); R10 numbered previews after the gate (recommend keeping them, unsigned zips from green CI); R22 the two senses of "seal" (recommend keeping both, glossing the physical sense, and rewording "when a seal goes dark"); R46 cross-reference style (recommend "section N" everywhere).
- **Item 40 (R1), gate segment schedule: option 1, per-round length list; AMENDS item 30.** The host's "flat override" becomes a per-round length list: empty = the ladder (3/4/5/6/7/8), one value = that length for every round, N values = round i takes the i-th entry and rounds beyond the list take the last entry. The gate schedule "3 rounds at 3/5/8 min" is produced by the list `[3,5,8]`. The list is shown in the lobby; the coming segment's length (ms) stays in the frozen snapshot, `get_status` and `get_briefing`. Item 30's "one flat length" wording and the gate row's "via the flat override" are superseded.
- **Item 41 (R3), chatter gating: option 1, ungated engine flavour.** One-way engine chatter (from S5, item 14) is open-air crew talk overheard on site, not mast traffic: it plays in every match with no Radio Mast precondition. §8's Mast gate on chatter is deleted; the Mast keeps its licence and knowledge roles only. The radio-as-untrusted-data theme is carried by licences, the Mast and the roadmap interception work, not by chatter.
- **Item 42 (R9), name clearance timing: option 2, professional search before v1, exposure stated.** The professional trademark search stays a pre-v1 task (hardening stage), not a gate criterion. v0.1 ships under PHARMAKOS on the strength of the owner's own sweeps (item 9); the spec states the exposure: a failed search after v0.1 means a rebrand of a released build (domains, handles, early press). Item 31's "name before release" means the sweeps plus registrations (pharmakosgame.com, pharmakos.io/.dev, @pharmakosgame, GitHub org), not the professional search.
- **Item 43 (R10), numbered previews after the gate: option 1, keep them cheaply.** After v0.1, each slice ends with a numbered preview cut from green CI: v0.2 at S4 (wk 41.5), v0.3 at S5 (45.5), v0.4 at S6 (53), v0.5 at S7 (57.5). Each is an unsigned zip (Windows, macOS, Linux) with a tagged changelog and a two-line "how to open an unsigned build" note; no packaging or signing work until hardening. Restates item 7's preview cadence for the post-gate slices.
- **Item 44 (R22), the two senses of "seal": option 1, keep both, gloss the physical one.** "Seal" keeps both meanings: the seal on sealed orders (a filed playbook) and a key-core's seal (the lit mark that shows the beacon is powered). The physical sense is glossed at its first use in §5; §2's "When a seal goes dark…" is reworded to "When a Stake falls…" so a brownout is not read as ruin; the glossary carries both entries.
- **Item 45 (R46), cross-reference style: option 1, "section N" everywhere.** Every cross-reference in the spec reads "section N" (lower-case, linked); "§N" and bare section numbers are rewritten. The decisions log keeps its own "§2.x" numbering for its passes.
- **Draft-6 review decision items COMPLETE (items 40–45).** Next: apply items 40–45 to `pharmakos-spec.html`, republish (same URL), then move the workspace to a project folder and start the stack spikes (G4, G1).
- **Items 40–45 applied to the spec (2026-09-13).** 14 text edits plus every cross-reference rewritten as a linked "section N"; a 55-agent verification pass (7 checkers, 3 refuters per finding: 16 candidates, 3 confirmed) then fixed the draft-5 changelog bullet's "flat-length option", renamed engine chatter's in-world name from "traffic on the air" to "crew talk on site" (the air = radio = Mast in this spec's vocabulary) and reworded the §2 chatter lore to match. Republished as "Spec v0.6, items 40–45 applied".
- **Harness part 1 bootstrapped (2026-09-13).** Repo at `C:\Users\PC\pharmakos`; toolchain installed (rustup 1.98.1 MSVC, MSVC 14.43, SDK 22621, protoc 36, buf 1.73, Godot 4.7.2); `cargo xtask ci` green. Two contract questions raised by the bootstrap fixer, pending the owner's answer: playbook durations int32 vs int64 (int32 applied provisionally: bare JSON numbers, matches the §10 example); the envelope `kind` tag reserved as field 7 rather than defined (to be added with the skeleton's envelope).
- **Item 46, playbook durations: option 1, `int32` game milliseconds.** Every duration field in `gp.v1` (`timeout_ms`, `cooldown_ms`, `hold.ms`, `known_within_ms`, …) is `int32`, so canonical proto JSON emits bare numbers and the §10 example loads as written; negative values are rejected by the verifier with a code and a JSON Pointer, not at decode. `int64` (quoted strings in JSON) and `uint32` (decode-time failure without a pointer) rejected.
- **Item 47, envelope `kind` tag: reserved now, defined with the skeleton.** `Playbook` reserves field 7 / name `kind`; the real field and its enum arrive with the walking skeleton's envelope work, when the example and golden files change together.
- **Initial commit made 2026-09-13** (harness part 1, spec v0.6, spike plans), signed off per the DCO rule.
- **Spike G4 measured on Windows (2026-09-13), verdict PROVISIONAL pending the three-OS CI run.** Toy sim in `spikes/g4-determinism` (rustc 1.98.1 MSVC; xxhash-rust 0.8.18, rkyv 0.8.18 little-endian/32-bit pointers, postcard 1.1.3, imbl 7.0.2, all pinned with Cargo.lock). G4-b: 50/50 save/restore round trips in fresh processes for both formats. G4-c: 20/20 fork equivalences, parents unperturbed. Repeat, core-pinned and debug runs give the same trace digest; 26 XXH3 reference vectors pass; default build carries no fork symbol. Contract candidates for the owner (interview to follow): the canonical encoding and seed `0x5048_4152_4D4B_4F53`; snapshot format (postcard ~4.5 KB vs rkyv ~6.8 KB at tick 4800, both hash-transparent); the split counter-based RNG streams; the determinism rule set. Full numbers in `docs/spikes/G4-determinism.md` §9.
- **Spike G4 verdict: GO (2026-09-13).** The `spike-g4-determinism` workflow (run 34802164421, commit `6e7a156`) found the 10-match per-tick hash traces and both snapshot hash lists byte-identical on ubuntu, windows and macOS (ARM); with G4-b 50/50 and G4-c 20/20 already measured, the per-tick xxh3 hash is a cross-OS, cross-architecture contract and no part of the fallback is taken. The four frozen choices (hash encoding + seed, snapshot format, RNG construction, determinism rule set) go to the owner as interview items 48–51.
- **Item 48, hash encoding and seed: option 1, approved as implemented in spike G4.** Canonical little-endian byte encoding of the state tables in id order (lengths as `u32`, fixed-stride `Option` encoding, `ENCODING_VERSION = 1` first), digested by xxh3-64 under `STATE_HASH_SEED = 0x5048_4152_4D4B_4F53`. Pinned values: `digest(b"") = 0xFE1AF732B02810AD`, `digest(b"pharmakos/g4") = 0xE17026C8A5EA4D30`. Changing the seed or the version byte invalidates every golden file.
- **Item 49, snapshot format: option 1, postcard.** postcard 1.1.3 (`default-features = false`, `use-std`) with serde; every snapshot field fixed-width, `Option` as a sentinel, `SNAPSHOT_VERSION` in the bytes and checked on restore. Measured 4,563 B at tick 4800, 0.04 ms save, 0.24–0.32 ms restore, byte-identical across ubuntu/windows/macOS. Known bend of the "never serialise usize" rule: `Vec` lengths are LEB128 varints, value-identical on every target for realistic lengths; documented at the encoder. rkyv (7,160 B, heavier dependency tree) and a hand-written codec rejected.
- **Item 50, RNG construction and stream ids: option 1, approved as implemented in spike G4.** Counter-based, stateless: `draw(n) = mix64(key ^ n·GOLDEN)` with `key = mix64(k1 ^ (seat<<32) ^ sub)`, `k1 = mix64(k0 ^ tick·GOLDEN)`, `k0 = mix64(match_seed ^ stream_id·GOLDEN)`, `GOLDEN = 0x9E3779B97F4A7C15`, `mix64` = the SplitMix64 finaliser with wrapping multiplies; ranges by multiply-shift. Stream ids are explicit constants, additive only, never renumbered: Combat = 1, Spawn = 2, Map = 3. Revisit only if a future feature needs audited statistical quality (large sampled fields in the map generator).
- **Item 51, determinism rule set: option 1, approved as implemented in spike G4.** In sim crates: no `f32`/`f64`; no `as` casts (`From`/`TryFrom` with an explicit failure mode); no `HashMap`/`HashSet` (`BTreeMap`, sorted `Vec`, `imbl::OrdMap`); no wall-clock time; `overflow-checks = true` in every profile, release included, `debug-assertions = false` in release; every sort key ends in a unique id; fixed-width types only in hashed state, lengths as `u32`; no recursion. Enforced by crate-level `deny` attributes plus the workspace clippy configuration; the walled presentation module carries an explicit allow list.
- **G4 contract items 48–51 COMPLETE.** Harness part 1 may now encode them (golden determinism hashes, the lint set) as the walking skeleton's sim crate takes shape.
- **Spike G1 verdict: GO — on Windows/Vulkan/Quadro P4000 only; the Linux half of the gate (plan §3 step 8) is outstanding (2026-09-14).** Toy build in `spikes/g1-remesh` (own greedy mesher in Rust, 288 chunks of 32³, single-voxel explosions, two upload paths into Godot 4.7.2 via gdext 0.5.5), measured over 25 Godot launches and 4 headless `meshbench` passes; full numbers in `docs/spikes/G1-destruction-remesh.md` §10. Gate rows, path B, 3 × 3 600-frame repetitions: **G1-a** remesh latency p50 0 / p90 1 / p99 **1** / max 2 frames (limit p99 ≤ 3), nothing left queued; **G1-b** 20 explosions/s sustained for 3 600 frames (frame-driven, so 209–220/s wall-clock with vsync off); **G1-c** frame time p99 **5.65 ms** median of 3 (limit 16.67), max 11.0 ms, 0 of 3 600 steady-state frames over budget; ~2.6× p99 headroom, and 40 explosions/s still holds latency p99 at exactly 3. Mesher CPU ~450 µs per chunk in-engine (headless best-of-5 p99 343 µs flat / 503 µs cratered), 0.001 allocations per meshing, 16-bit indices with 10× headroom, peak heap 18.8 MiB, video memory 82.9 MiB. Path B (direct `RenderingServer` RIDs) beats path A (`ArrayMesh`) on the two clean measurements — 57 vs 84 µs per chunk upload (1.48×, non-overlapping) and 0 vs 5 798 `ArrayMesh` objects per minute — with identical geometry; frame time is a tie. Budget candidate K = 4 chunk surfaces, B = 512 KiB per frame, nearest-camera-first with an ageing term. No rung of the fallback ladder is taken. Three real bugs found and fixed, all carried into §10.12 for the skeleton: Godot 4.7 front faces are counter-clockwise (the first vista showed the inside of the terrain); a `Gd<Material>` dropped after `get_rid()` leaves a dangling RID that draws nothing; the in-place surface update must compare the index array, not just the counts (29 of 2 570 in-place updates in the headline run had changed indices). Also: `godot --headless` cannot take a screenshot (dummy driver), so the CI `geometry` job renders under xvfb + lavapipe and compares against a committed 1280×720 golden (0 of 921 600 pixels differ on Windows/Vulkan; lavapipe untested). Not run: Linux/Vulkan on a real GPU, D3D12 on Windows, the Defender-exclusion A/B, a vsync-on 60 Hz confirmation. Three review findings corrected the first write-up: frame times come from a raw monotonic clock, not `_process(delta)` (the physics jitter fix quantises it to 1/720 s); K = 4 has no measured frame-time justification (chosen as the smallest K meeting latency); only the mesher's p99 is a usable CI budget (the max flaps 3×). The four-part decision plan §9 asks for (upload path, budget rule, chunk size, thread + wall placement) goes to the owner as items 52–56 below.
- **Spike G1 CI green (2026-09-14, run 34823744065).** `spike-g1-remesh` passes on every job: mesher CPU budgets on ubuntu and windows, and the Linux `geometry` job under xvfb + lavapipe matches the Windows/Quadro golden (mean 0.0039/255, 1.14 % of pixels differ at all, 0.000 % by more than 32). That measures the geometry half of the cross-OS question; the Linux frame-time half is still unmeasured (no GPU runner). Four things learnt on the way, recorded in the plan's §10.12 for the skeleton's CI: a non-editor Godot loads GDExtensions only from `.godot/extension_list.cfg`, so every fresh checkout needs `godot --headless --import` first; do not hard-code the lavapipe ICD path (the loader then finds no driver and Godot silently falls back to OpenGL 3); GitHub hides job logs *and* step summaries from logged-out viewers, so CI diagnostics are published as `::notice::` annotations; and the golden was regenerated once, explained: the shot frame moved from 880 to 180, and a Quadro render and the lavapipe render both differed from the old golden by the same 4.278 % over 32.
- **Item 52, G1 cross-OS half: option 1, the single-platform frame-time GO is accepted.** Geometry is measured identical on Linux (lavapipe, run 34823744065); frame time and latency are measured on Windows/Vulkan/Quadro P4000 only, with 2.6× p99 and 3× latency headroom. The Linux frame-time half is deferred to G1's re-run against the walking skeleton under synthetic load (§2.4), where it is added if a Linux GPU machine or runner exists by then; the `frame-time` CI job stays `if: false` until a GPU runner exists. Rejected: a live-USB/dual-boot or cloud-GPU run now (half a day to a day, blocks items 53–56, a cloud card is not this card) and WSL2 (a D3D12 translation layer, so it would characterise the wrong driver).
- **Item 53, upload path: option 1, path B (direct `RenderingServer` RIDs) with path A (`ArrayMesh` + `MeshInstance3D`) kept as a build-time switch in the skeleton's bridge.** Measured: B is 1.48× cheaper per chunk upload (57 vs 84 µs), writes 77.0 vs 96.6 KB per chunk, and creates no resources against A's 5 798 `ArrayMesh` objects a minute; frame-time p99 is a tie within noise; geometry is byte-identical. The switch exists so that a retreat to A is a flag, not a rewrite, because B carries four bug classes the bridge must keep guarding against: dangling RIDs that silently draw nothing (a `Gd<Material>` dropped after `get_rid()`), the surface-format probe, the index-array equality guard before an in-place region update (counts alone are not enough: 29 of 2 570 in-place updates in the headline run had changed indices), and a colour quantisation that must match Godot byte for byte. Rejected: B only (a retreat becomes a rewrite under pressure) and A only (churn of that order is the load that shows as hitches on a larger world or a higher K, which the spike did not measure).
- **Item 54, per-frame upload budget: option 1, K = 4 chunk surfaces and B = 512 KiB per frame, and the plan's selection rule is amended to "the smallest K that meets the latency gate".** The plan's "largest K keeping frame p99 under 16.67 ms" selects unbounded, i.e. no budget; the amended rule selects K = 4 (K = 1 and 2 fail G1-a outright, K = 8 and unbounded also pass). Drain order: anything older than 2 frames first, then nearest-to-camera, ties by chunk index; a chunk re-dirtied while queued is coalesced and keeps its earliest issue frame; the first chunk of a frame always goes through even if it alone exceeds B. B = 512 KiB is one doubling above the point where B stops binding (256 KiB), a margin for a heavier future vertex format, not an optimum. Recorded honestly: no measured frame-time reason separates K = 4 from K = 8 on this machine (the maxima were noise), so the choice is structural, a bound on per-frame work; and the ageing term never fired in any run, so it ships as insurance against the drain order's starvation mode with no measurement behind it. K and B are tuning values in the rules table, stamped into the rules hash, not constants in code. Rejected: dropping the ageing term (ships a known starvation mode untested either way) and K = 8 (doubles the worst-case work the budget exists to bound while the gate is met at a third of its latency allowance).
- **Item 55, chunk size and destruction granularity: option 1, unchanged — 32³ chunks and single-voxel destruction; no rung of the G1 fallback ladder is taken.** Measured on Windows/Vulkan: dirty fan-out max 8 chunks per explosion at radius 4 (pad = light reach + 1, both derived from `LIGHT_MAX`/`LIGHT_ATTEN`), ~450 µs mesh cost per chunk, 16-bit indices with 10× headroom (6 660 vertices max against 65 536), peak heap 18.8 MiB, video memory 82.9 MiB, radius 8 at 20 explosions/s still within the gate. Caveat carried from item 52: the headroom is Windows-only; if a Linux GPU result is ever materially worse, rung 1 (2³ edit groups) is the lever, and it does not touch the sim's voxel grid. Rejected: taking rung 1 pre-emptively (gives up single-voxel destruction for a problem no measurement shows) and 16³ chunks (eight times the chunk count and draw calls, and the chunk size is a determinism-adjacent contract).
- **Item 56, mesher thread and the presentation wall: option 1, the mesher runs on the main thread, in its own walled crate `crates/mesher` (`pharmakos-mesher`).** Measured: 450 µs per chunk × K = 4 is 1.8 ms of a 16.67 ms frame against a measured frame p99 of 5.65 ms, and Godot's single-safe thread model makes path B's `RenderingServer` calls legal from `_process` with no render-thread hop; a worker pool is not needed for v1's load and would put a second thread near world state. The wall line, as G1 found it: everything up to and including the voxel edit, the dirty set and the drain order is integer; floats begin at the first vertex coordinate; nothing a float touches is read back by the sim. The mechanism: `crates/mesher` takes integer chunk data in and produces vertex buffers out, is listed in `WALLED_PACKAGES` (and the `clippy.toml` header), is depended on by `client-gdext` and never by `sim`, `plan-core`, `verifier`, `operator` or `gateway` (`wall-guard` enforces it), and links without gdext so the headless CPU proxy and the CI geometry check can reuse it. The spike's per-file `#![deny(clippy::float_arithmetic)]` shape does not transfer. This resolves the AGENTS.md §3 placeholder for the mesher; the `math` crate question stays open for the skeleton. Adding the crate is a §5 contract change, approved here; it lands as a small harness PR at the walking skeleton together with the AGENTS.md crate-map edit. Rejected: the mesher inside `client-gdext` (breaks the thin-client rule and nothing headless can reuse it without linking gdext) and a worker pool from the start (no measurement asks for it, and path B's uploads stay on the main thread regardless).
- **G1 decision items 52–56 COMPLETE (2026-09-14).** Spike G1 is closed: GO, path B with an A switch, K = 4 / B = 512 KiB under the amended rule, 32³ chunks and single-voxel destruction unchanged, main-thread mesher in a walled `mesher` crate. Next: spikes G2 + P3 (pathing and travel estimates) and G3′ (segment budget), then the walking skeleton.


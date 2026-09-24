// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//
// One wave of the walking skeleton as a Claude Code workflow: every lane is built by one
// agent in its own worktree, reviewed by two adversarial agents with different lenses, and
// fixed by a fourth, as a pipeline (lane B's review does not wait for lane A's build).
//
// Run it from the owner's session with
// `Workflow({scriptPath: 'C:/Users/PC/pharmakos/.claude/workflows/wave-lanes.js', args: {...}})`.
// `args` is the whole wave description; nothing wave-specific lives in this file:
//
//   {
//     wave: 'wave3',                          // label only
//     scratch: 'C:/.../scratchpad',           // REQUIRED: where PR bodies and CI logs land
//     repo: 'C:/Users/PC/pharmakos',          // default shown
//     worktree_root: 'C:/Users/PC',           // default shown; worktrees are <root>/pharmakos-<id>
//     build_root: 'D:/build',                 // default shown; CARGO_TARGET_DIR is <root>/<id>
//     lanes: [{
//       id: 't7',                             // short id: worktree, target dir, PR-body file
//       task: 'T7',                           // the `### T7 —` section of skeleton-plan.md §3
//       branch: 'feat/sim-hpa-estimator',
//       owns: 'crates/sim (all of it), tests/golden/pathing, plus Cargo.lock',
//       items: '57, 58, 59, 60, 61, 62, 69',  // decisions-log §2.7 items to read
//       extras: '...',                        // wave-specific emphasis for the builder (may be '')
//       lens_a: '...',                        // extra checks for lens A (may be '')
//       lens_b: '...',                        // extra checks for lens B (may be '')
//       build_resume: '...',                  // optional: only when resuming a run whose builder
//                                             // died mid-way (what the worktree already holds)
//       fix_resume: '...',                    // optional: the same for a fix pass that died
//     }],
//   }
//
// What the lanes return is deliberately small (lane, branch, PR number, CI state, counts);
// every agent's full report is in the run's journal.jsonl. The main session merges the PRs
// in order with scripts/merge-train.sh — sub-agents open a PR and stop (AGENTS.md §5, §6;
// decisions-log item 85).

export const meta = {
  name: 'wave-lanes',
  description: 'One wave of the walking skeleton: each lane built in its own worktree, reviewed by two adversarial lenses, fixed, and left as an open pull request for the main session to merge in order',
  phases: [{ title: 'Build' }, { title: 'Review' }, { title: 'Fix' }],
}

const REPO = (args && args.repo) || 'C:/Users/PC/pharmakos'
const WT_ROOT = (args && args.worktree_root) || 'C:/Users/PC'
const BUILD_ROOT = (args && args.build_root) || 'D:/build'
const SCRATCH = args && args.scratch
const WAVE = (args && args.wave) || 'wave'
const LANES = (args && args.lanes) || []
if (!SCRATCH) throw new Error('args.scratch is required (the session scratchpad path)')
if (!LANES.length) throw new Error('args.lanes is empty')
for (const l of LANES) {
  for (const k of ['id', 'task', 'branch', 'owns', 'items']) {
    if (!l[k]) throw new Error(`lane ${JSON.stringify(l)} is missing "${k}"`)
  }
}

const wt = lane => `${WT_ROOT}/pharmakos-${lane.id}`
const target = lane => `${BUILD_ROOT}/${lane.id}`
const bodyPath = lane => `${SCRATCH}/${lane.id}-pr-body.md`
const ciLog = (lane, who) => `${SCRATCH}/${lane.id}-${who}-ci.log`

function env(lane, who) {
  return `
ENVIRONMENT (read first)
- Windows host. Use the Bash tool for everything. Before ANY cargo / buf / git / gh command in a shell call, run:
    export PATH="$USERPROFILE/.cargo/bin:$LOCALAPPDATA/Microsoft/WinGet/Links:$PATH"; export CARGO_TARGET_DIR=${target(lane)}
  (shell state does not persist between calls: repeat both exports in every call that needs them).
- The repository is ${REPO} (branch main). Work ONLY in the worktree ${wt(lane)} on branch ${lane.branch}.
  If the worktree does not exist yet, create it from main:  cd ${REPO} && git worktree add ${wt(lane)} -b ${lane.branch} main
  Never edit files under ${REPO} itself. The spike code is readable at ${WT_ROOT}/pharmakos-spikes/spikes/ (tag spike-end): a reference, never copied, never edited.
- Target directory: ${target(lane)} and NO other. The builder warms it; reviewers and the fix pass reuse it (a cold workspace build costs ten minutes and hundreds of lines of context; a warm full suite costs two minutes). A probe crate you write under ${SCRATCH} may use ${target(lane)}-probe.
- CI etiquette: "cargo xtask ci --quick" is the inner loop. Run the FULL suite by writing it to a file and reading only the summary, with the exit code checked separately, because a pipe hides it:
    cargo xtask ci > ${ciLog(lane, who)} 2>&1; echo "exit=$?"; grep -A 20 '== summary' ${ciLog(lane, who)}
  On a red step, grep that step's section of the log; do not paste the whole log into your context.
- You own exactly these crates/paths: ${lane.owns}. Do not edit any other crate; another agent owns it right now (AGENTS.md section 6). A dependency line in your own crate's Cargo.toml comes only from the approved list (AGENTS.md section 3 rule 5) and only via an existing [workspace.dependencies] entry (uncommenting one in the root Cargo.toml is allowed; adding a new crate to the workspace list is not). Cargo.lock changes are expected and fine.
- Toolchain present: rustc/cargo 1.98.1 (MSVC), protoc 36, buf 1.73.0, protoc-gen-prost 0.5.0, cargo-deny, reuse, gh (signed in). "cargo xtask ci" is the single check suite; "cargo xtask golden --bless" accepts fresh outputs under tests/golden for the areas the golden step compares; "cargo xtask determinism --bless" re-baselines the determinism chain and is ONLY for the sim lane, with the movement explained in the PR body.
- Goldens: read tests/golden/README.md first. A producing test writes actual.<ext> under $CARGO_TARGET_DIR/golden/<area>/... and the golden step compares it with tests/golden/<area>/...; LF endings, trailing newline, no carriage return. Every area's README.md says what a diff means; keep it accurate. A golden that moves is a behaviour change: say which in the PR body, never re-bless to make a red test green without the reason.
- Determinism rules are not advisory (AGENTS.md section 4): no floats outside walled crates, no as-casts outside the sim's audited math module, no HashMap/HashSet, no wall clock, ordered iteration, integer newtypes, overflow checks on, clippy pedantic plus the deny set with -D warnings; never add #[allow] on a determinism lint. Sim state added goes into the state hash, the snapshot round-trip and the goldens in the same PR - four places for a voxel write, which must mark its chunk. Every guessed value is a "// PLACEHOLDER: <what, who decides, when>" comment listed in the PR body. Tuning values come from the rules table (rules/rules.v1.json via pharmakos_sim::rules::RulesTable; RulesTable::message() reaches the whole decoded table), never from constants; a tuning value with no row yet is a named constant carrying a PLACEHOLDER (AGENTS.md section 12).
- Commits: conventional-commit subject with your crate as scope, DCO sign-off via "git commit -s", and the line "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" ABOVE the Signed-off-by line. Write each message to a file and commit with -F. Several commits are fine. Never amend or rewrite a pushed commit; never merge anything.
- The Bash tool's heredocs are not UTF-8 safe for non-ASCII characters (the docs use section signs, primes and em dashes). Write or edit files containing them with the Write/Edit tools. Keep new Rust source ASCII where you can.
- SPDX header on every new file (AGENTS.md section 8): game code is GPL-3.0-or-later. Doc comments on every public item (missing_docs warns; warnings are denied).
`
}

function brief(lane) {
  return `
WHAT TO BUILD - task ${lane.task} of the walking skeleton (${WAVE}).
The brief is the plan itself, not a restatement of it. Read, in this order, before editing:
1. docs/design/skeleton-plan.md section 3, the "### ${lane.task} " section in full: its Builds, Implements, Needs, Acceptance, Contract PR and PLACEHOLDERs lines are the deliverables and the tests, line by line; then section 2 (what the spikes fixed) and every section 6/7 decision that section names, whose answers are in the decisions log.
2. docs/design/decisions-log.md section 2.7 items ${lane.items}, and the last entries of the file (the wave's opening entry and the decisions taken for it). The decisions log outranks the spec, which outranks the co-design doc (docs/design/README.md).
3. The spec sections the plan section cites, in docs/spec/pharmakos-spec-v0.6.html (search the HTML for the section title).
4. Every source file of the crate you own, in full, and its tests; the goldens' READMEs under tests/golden; rules/rules.v1.json and proto/gp/v1/rules.proto for the rows you read; the spike modules the plan section names, as references.
${lane.extras ? '\nWAVE-SPECIFIC EMPHASIS:\n' + lane.extras + '\n' : ''}
Then build every deliverable and every acceptance test the plan section names, with the goldens it names, until "cargo xtask ci" is fully green in the worktree (no --skip).

WHEN GREEN - the PR, then stop:
1. Write the PR body to ${bodyPath(lane)}: first line is the PR title (the conventional-commit subject of the work, ending with "(${lane.task})"), a blank line, then a body that (a) says which crates you touched, (b) names every CONTRACT path touched and what the contract change is, (c) explains every golden that moved and why (for the sim: the first tick the determinism chain diverges at and which rule moved it), (d) lists every PLACEHOLDER you left and who resolves it when, (e) lists anything in AGENTS.md you found wrong or missing (do not edit AGENTS.md), and ends with the line "🤖 Generated with [Claude Code](https://claude.com/claude-code)".
2. Push and open the pull request, then stop - never merge, never enable auto-merge:
    cd ${wt(lane)} && git push -u origin ${lane.branch}
    tail -n +3 ${bodyPath(lane)} > ${SCRATCH}/${lane.id}-pr-body.notitle.md
    gh pr create --title "$(head -n 1 ${bodyPath(lane)})" --body-file ${SCRATCH}/${lane.id}-pr-body.notitle.md --base main --head ${lane.branch}
   Record the PR number the command prints. The three-OS matrix now runs on it while the reviews happen; you do not wait for it.
`
}

function lensA(lane) {
  return `LENS A: determinism, the contract paths and the harness rules. Adversarially check, with a file and line for every claim: every contract path the PR body names is really the whole set the diff touches (git diff main...HEAD --stat, then AGENTS.md section 5's list); every new piece of sim state is in the state hash's declared order, in the snapshot and restored, and in the goldens (a voxel write also marks its chunk); the snapshot stays fixed-width; RNG streams are drawn only where declared and no stream is reused; no float, as-cast, HashMap/HashSet, wall clock or unordered iteration outside the allowances (a walled crate may use floats and clocks; nothing else may); no #[allow] on a determinism lint anywhere in the diff; a research-guarded crate declares no [features] and takes the sim with default-features = false and never names the stepping API; goldens are LF-terminated, land under $CARGO_TARGET_DIR/golden/<area>/ at the committed relative paths, and every golden that moved is explained in the PR body by a behaviour change you can confirm in the code. Try to make the code panic or overflow with an adversarial input and report the input with the panic.${lane.lens_a ? '\nALSO FOR THIS LANE: ' + lane.lens_a : ''}`
}

function lensB(lane) {
  return `LENS B: the plan's acceptance lines and the decisions. Adversarially compare the branch, line by line, with skeleton-plan.md section 3's "### ${lane.task} " section (Builds, Acceptance, PLACEHOLDERs) and decisions-log items ${lane.items}: every deliverable present, every acceptance test present and testing what its name claims (read the test bodies, not the names), every number read from the rules table rather than a constant (grep numeric literals in the crate's src and challenge each), every guessed value carrying a PLACEHOLDER that is also in the PR body, every README under tests/golden this lane touched accurate, module docs describing what now exists, no other crate edited (git diff main...HEAD --stat), DCO and co-author trailers on every commit, SPDX headers on new files, doc comments on public items, and the PR body's claims true. Report each gap with file and line.${lane.lens_b ? '\nALSO FOR THIS LANE: ' + lane.lens_b : ''}`
}

const BUILD_SCHEMA = {
  type: 'object',
  properties: {
    lane: { type: 'string' },
    branch: { type: 'string' },
    pr: { type: 'integer', description: 'the pull request number gh pr create printed' },
    commits: { type: 'array', items: { type: 'string' } },
    ci_green: { type: 'boolean' },
    ci_summary: { type: 'string' },
    pr_body_path: { type: 'string' },
    placeholders: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string' },
  },
  required: ['lane', 'branch', 'pr', 'commits', 'ci_green', 'ci_summary', 'pr_body_path', 'placeholders', 'notes'],
}

const REVIEW_SCHEMA = {
  type: 'object',
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          severity: { type: 'string', enum: ['blocker', 'major', 'minor', 'nit'] },
          file: { type: 'string' },
          line: { type: 'integer' },
          claim: { type: 'string' },
          evidence: { type: 'string' },
          fix: { type: 'string' },
        },
        required: ['severity', 'file', 'claim', 'evidence', 'fix'],
      },
    },
    verdict: { type: 'string' },
  },
  required: ['findings', 'verdict'],
}

function reviewPrompt(lane, lens, who, build) {
  return env(lane, who) + `
You are REVIEWING pull request #${build.pr} (branch ${lane.branch}) in the worktree ${wt(lane)}, which another agent created and committed. Do NOT create, edit or commit anything in it; you only read and run checks, and you may write scratch files under ${SCRATCH}.
The diff: git -C ${wt(lane)} diff main...HEAD
The builder's report: ${JSON.stringify(build)}
The brief the builder worked from is the plan: read skeleton-plan.md section 3's "### ${lane.task} " section and decisions-log items ${lane.items} before you start.
The three-OS matrix is already running on the PR: read its state with "gh pr checks ${build.pr}" (and "gh run view <run-id> --log-failed" if a leg is red) instead of running the full suite yourself; run the targeted tests you need with "cargo test -p <crate> <name>" in the lane's target directory. Everything the builder claims about CI is checked that way, not by repeating it.
` + lens + `
Report nothing you cannot evidence with a file and line or a command's output. Return findings (each with severity, file, line if known, the claim, the evidence, the fix) and a one-paragraph verdict. If you find nothing, say so with what you checked.`
}

function fixPrompt(lane, build, findings) {
  return env(lane, 'fix') + brief(lane) + `

YOU ARE THE FIX PASS for lane ${lane.id}: pull request #${build.pr} is open on branch ${lane.branch} in the worktree ${wt(lane)} (builder's report: ${JSON.stringify(build)}). Two adversarial reviews returned these findings:
${JSON.stringify(findings, null, 2)}

Verify each finding against the actual files before acting: apply the ones that are real (every real blocker and major; minors and nits where cheap), skip the ones that are wrong and say why. Make NEW commits (never amend, never rewrite, never force-push). Run "cargo xtask ci --quick" as you go and the full suite once at the end (to the log file, summary only, exit code checked); re-bless a golden only with the behaviour change explained. Update the PR body file at ${bodyPath(lane)} so it is accurate, adding a "Review" section that lists each finding and whether it was applied or rejected and why, then push the commits (plain "git push") and update the PR with:
    tail -n +3 ${bodyPath(lane)} > ${SCRATCH}/${lane.id}-pr-body.notitle.md && gh pr edit ${build.pr} --body-file ${SCRATCH}/${lane.id}-pr-body.notitle.md
Then wait for the matrix ("gh pr checks ${build.pr} --watch --interval 30") and fix anything red the same way. Stop when every check is green: never merge. Return the same structured output as the builder, with notes describing what you changed.` + resume(lane.fix_resume)
}

// Only set when a run is resumed after an agent died mid-way (an API outage): the prompts of
// the agents that completed stay byte-identical, so they replay from the run's cache.
function resume(note) {
  return note ? `\n\nRESUMING A PREVIOUS ATTEMPT (read before anything else): ${note}\nStart with "git status" and "git diff" (and "git log main..HEAD") in the worktree to see exactly what that attempt left; continue from it, never discard it and never start over.` : ''
}

const results = await pipeline(
  LANES,
  lane => agent(env(lane, 'build') + brief(lane) + resume(lane.build_resume), { label: `build:${lane.id}`, phase: 'Build', model: 'opus', effort: 'high', schema: BUILD_SCHEMA }),
  async (build, lane) => {
    if (!build) { log(`${lane.id}: build returned nothing`); return null }
    log(`${lane.id}: build done, PR #${build.pr}, ci_green=${build.ci_green}, ${build.commits.length} commits`)
    const reviews = await parallel([['A', lensA(lane)], ['B', lensB(lane)]].map(([tag, lens]) => () =>
      agent(reviewPrompt(lane, lens, `review${tag}`, build), { label: `review:${lane.id}:${tag}`, phase: 'Review', model: 'opus', effort: 'high', schema: REVIEW_SCHEMA })))
    const findings = reviews.filter(Boolean).flatMap((r, i) => r.findings.map(f => ({ ...f, lens: i === 0 ? 'A' : 'B' })))
    const count = s => findings.filter(f => f.severity === s).length
    log(`${lane.id}: ${findings.length} findings (${count('blocker')} blockers, ${count('major')} majors)`)
    return { build, findings }
  },
  async (prev, lane) => {
    if (!prev) return null
    const fix = await agent(fixPrompt(lane, prev.build, prev.findings), { label: `fix:${lane.id}`, phase: 'Fix', model: 'opus', effort: 'high', schema: BUILD_SCHEMA })
    const count = s => prev.findings.filter(f => f.severity === s).length
    log(`${lane.id}: fix done, ci_green=${fix ? fix.ci_green : 'n/a'}`)
    // Small on purpose: the full reports are in the run's journal.jsonl.
    return {
      lane: lane.id,
      task: lane.task,
      branch: lane.branch,
      pr: prev.build.pr,
      commits: fix ? fix.commits.length : prev.build.commits.length,
      ci_green: fix ? fix.ci_green : false,
      findings: { total: prev.findings.length, blockers: count('blocker'), majors: count('major') },
      placeholders: fix ? fix.placeholders.length : prev.build.placeholders.length,
      pr_body: bodyPath(lane),
    }
  },
)
return results

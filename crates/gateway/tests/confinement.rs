// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rules this crate is confined by, asserted against its own **source
//! text** and against the schema's vocabulary.
//!
//! The lints deny most of it and `cargo xtask ci`'s `research-guard` proves the
//! dependency edge. This file exists because of the lesson the G2 and G3-prime
//! spikes both wrote down and `crates/sim` and `crates/verifier` both copied:
//! **copy the test, do not merely rely on the lint.** A lint can be switched off
//! by an `#[allow]` somebody adds in a hurry; a test that greps the source
//! cannot be, and it fails in the pull request that tries.
//!
//! Six rules, and the gateway's list differs from the verifier's in ways that
//! are worth stating rather than quietly leaving out:
//!
//! 1. **No clock, no hash container, no float.** The same list as every
//!    deterministic crate (AGENTS.md sections 4.2, 4.4, 4.5). The gateway is not
//!    a walled crate, so it gets no allowance at all.
//! 2. **No wildcard binding.** `0.0.0.0`, `UNSPECIFIED` and name resolution --
//!    the half of "localhost only" that a type cannot enforce (AGENTS.md
//!    section 7). The needles are exactly the three in
//!    [`NO_WILDCARD_BINDING`]; an unspecified IPv6 address is written
//!    `Ipv6Addr::UNSPECIFIED` in Rust, which the second needle catches, and its
//!    textual form cannot be a needle at all.
//! 3. **No outbound connection.** The gateway accepts; it never dials. "No
//!    network telemetry" (AGENTS.md section 7) is a rule about the shipped
//!    binary, and the shipped binary has no client in it.
//! 4. **The filesystem lives in one module.** `std::fs` appears in
//!    `src/cache.rs` and nowhere else, so "no filesystem access through
//!    playbooks" is checkable rather than asserted.
//! 5. **No research build, anywhere.** [`NO_DRY_RUNS`]: no file of this crate
//!    names `fork` or the sim's `research` feature, so no seat can reach either.
//! 6. **The match is stepped in one module, and driven from one more.**
//!    [`STEPPING`] and [`the_match_is_stepped_in_one_module`].
//!
//!    T9's version said "never". T13 changed the fact rather than the rule:
//!    the gateway *hosts* the match, so something here has to build a world
//!    and drive a runner, and `src/host.rs` became the one module that names a
//!    stepping call.
//!
//!    **T16a changes the rule itself** (decisions-log item 107 (4)), because
//!    before this lane nothing outside a test called `Host::step` at all and
//!    the Push therefore had no owner. The rule is now:
//!
//!    > No handler may step or seal, **except** the admin-scoped control
//!    > handlers in `surface/control.rs`, which may drive only the live match
//!    > through the surface's three driving methods, and may name none of
//!    > `Runner`, `Host`, `World`, `host_mut`, `seal_plans`, `seal_playbook`,
//!    > `snapshot`, `.clone()`, `file_voxel_edit` or `file_damage`.
//!
//!    So: `src/host.rs` names the stepping calls, `src/surface.rs` reaches
//!    them only through `host_mut()`, `src/surface/control.rs` may name the
//!    three driving methods and nothing else on the list, and the read
//!    handlers may name none of it. `every_control_handler_needs_the_admin_scope_in_the_schema`
//!    in `tests/control.rs` is the other half: the exemption is for
//!    admin-scoped methods, and the scope is read off the descriptor set
//!    rather than off this file.
//!
//! 7. **No wire method files a voxel edit or a damage order.**
//!    [`no_wire_method_files_a_voxel_edit_or_a_damage_order`]. `Host` gained
//!    two public mutators at T16a as a **test seam** -- nothing on `main`
//!    edits a voxel or kills a seat in a hosted match, and three acceptance
//!    lines about fog need one to. They are reached by no method, and the
//!    scenario runner must not grow a dependency on them without the owner's
//!    word.
//! 8. **No handler files advice or gives a token a rate.**
//!    [`no_handler_can_file_advice`]: `Surface::file_advice` and
//!    `Surface::register_in_process` are host-side (decisions-log item 111,
//!    decisions C5 and C6), defined in `src/surface.rs`, called from
//!    `src/serve.rs`, and named nowhere else.
//!
//! And then the rule that is about the *schema* rather than the source: a
//! playbook is data with a closed vocabulary, and no field of that vocabulary
//! names a path, a URL or a socket.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_proto::descriptor::schema;
use std::path::PathBuf;

/// Determinism hazards: the list `crates/sim` and `crates/verifier` are held to.
const BANNED: &[(&str, &str)] = &[
    (
        "HashMap",
        "iteration order is randomly seeded per process (AGENTS.md section 4.4)",
    ),
    (
        "HashSet",
        "iteration order is randomly seeded per process (AGENTS.md section 4.4)",
    ),
    (
        "RandomState",
        "the per-process random seed behind HashMap's order",
    ),
    (
        "DefaultHasher",
        "SipHash with unstable output across toolchain versions",
    ),
    (
        "Instant",
        "wall-clock time; the gateway is not a walled crate (AGENTS.md section 4.5)",
    ),
    ("SystemTime", "wall-clock time, and it can move backwards"),
    ("f32", "the gateway is integer-only (AGENTS.md section 4.2)"),
    ("f64", "the gateway is integer-only (AGENTS.md section 4.2)"),
    (
        "rayon",
        "nothing here is parallelised (AGENTS.md section 4.6)",
    ),
    ("par_iter", "nothing here is parallelised"),
    (
        "thread_rng",
        "a token comes from the OS entropy source, and nothing else here is random",
    ),
    (
        "xxhash",
        "there is exactly one hash function and it is the sim's, reached through \
         pharmakos_sim::digest (AGENTS.md section 5)",
    ),
    (
        ".unwrap()",
        "a panic in the gateway takes the whole match down, and the seat that caused it may \
         have been the one probing",
    ),
];

/// The sim's fixture API (`World`'s block comment: "not game actions, and
/// nothing outside a test, a scenario fixture or the host's lobby layout may
/// call them"), by method name: shipped code reaches a match's state through
/// the game's own paths, never these (S3's `lic`, its fix pass).
const SIM_FIXTURES: &[(&str, &str)] = &[
    (
        "award_licence_directly",
        "a licence is earned at a settlement, never handed over (S3's `lic`)",
    ),
    (
        "place_beacon_directly",
        "a beacon is placed by a deploy, never by a fixture",
    ),
    (
        "raise_structure",
        "a structure is bought through the Quartermaster and raised by drones or a fabricator",
    ),
    (
        "field_unit",
        "a unit is fabricated by its beacon, never fielded by hand",
    ),
    (
        "field_movers_directly",
        "the movers are a test's load, never a match's",
    ),
    (
        "switch_mandate_directly",
        "a writ changes on site, by a visit",
    ),
    (
        "place_unit_directly",
        "a unit walks; it is never put somewhere",
    ),
    (
        "mark_explored_directly",
        "a Survey scout explores; nothing marks a cell by hand",
    ),
    (
        "set_treasury",
        "the treasury moves by spending and income alone",
    ),
    ("set_dormant", "dormancy is the power phase's"),
    ("send_unit_directly", "a unit is sent by its program"),
    (
        "set_build_settings_directly",
        "Build settings change on site, by a visit",
    ),
    (
        "set_site_funds_directly",
        "site funds are the Quartermaster's",
    ),
    (
        "plant_charge_directly",
        "a charge is planted by its carrier",
    ),
];

/// Wildcard addresses: the half of "localhost only" a type cannot enforce.
const NO_WILDCARD_BINDING: &[(&str, &str)] = &[
    (
        "0.0.0.0",
        "the gateway binds loopback only (AGENTS.md section 7)",
    ),
    ("UNSPECIFIED", "a wildcard address by another name"),
    (
        "to_socket_addrs",
        "a name resolved to an address is not a loopback literal",
    ),
];

/// The gateway accepts connections. It never makes one.
const NO_OUTBOUND: &[(&str, &str)] = &[
    (
        "TcpStream::connect",
        "the gateway never dials out: no telemetry, no update check, nothing (AGENTS.md \
         section 7)",
    ),
    (
        "UdpSocket",
        "there is one transport and it is a loopback TCP listener",
    ),
    ("reqwest", "no HTTP client reaches a shipped crate"),
];

/// "No dry runs": no module of this crate may reach the research build, in any
/// file, ever (AGENTS.md section 3 rules 1 and 2).
///
/// `phase_` is deliberately **not** on this list, unlike `crates/verifier`'s:
/// `phase_remaining_ms` is spec section 12's own field name for the `_status`
/// footer's timer, and a needle that fired on it would be a needle somebody
/// deletes rather than a rule somebody keeps.
const NO_DRY_RUNS: &[(&str, &str)] = &[
    (
        "fork",
        "fork exists only in the research build and no seat may reach it",
    ),
    (
        "research",
        "the gateway must never name the sim's research feature",
    ),
];

/// Building and stepping a match.
///
/// **T9's version of this list said "never", and T13 changed the fact it was
/// describing rather than the rule.** The gateway now *hosts* the match
/// (AGENTS.md section 3's crate map, spec section 15), so something in this
/// crate has to build a world and drive a runner. What has not changed, and is
/// what "no dry runs" is actually about, is that **no method handler may**: a
/// handler that stepped, or cloned and stepped, a runner would be answering
/// "what would happen" by making it happen, which is the thing AGENTS.md
/// section 3 rule 2 is for. Read exactly, that rule names `plan-core` and the
/// verifier and never mentions the gateway; the pull request asks for a
/// sentence that names the match host the way section 4.9 names
/// `WALLED_PACKAGES`, and until there is one this test is the only thing
/// holding the line.
///
/// So the rule is a *place* rather than a prohibition, and
/// [`the_match_is_stepped_in_one_module`] is where it is enforced.
const STEPPING: &[(&str, &str)] = &[
    (
        "World::new",
        "a world is built by the match host and by nothing else",
    ),
    (
        "world_mut",
        "the world is written by the match host and by nothing else: a handler holding it could \
         put a voxel edit or a damage order into a match from inside a read",
    ),
    (
        "file_voxel_edit",
        "the host-side test seam: no wire method may reach it (decisions-log item 107 (7))",
    ),
    (
        "file_damage",
        "the host-side test seam: no wire method may reach it (decisions-log item 107 (7))",
    ),
    (
        "Runner::new",
        "a runner is made by the match host and by nothing else",
    ),
    (
        ".step(",
        "stepping the sim belongs to the match host (AGENTS.md section 3 rule 2)",
    ),
    ("begin_push", "starting a Push belongs to the match host"),
    ("end_recap", "ending a recap belongs to the match host"),
    (
        "seal_playbook",
        "filing a seat's orders into the match belongs to the match host: a handler that could \
         seal could rewrite a seat's orders from inside a read (decisions-log item 103 (1))",
    ),
    (
        "seal_plans",
        "the same rule, spelled the way a handler would actually reach it. `seal_playbook` is \
         the *runner's* name for it and `Host::seal_plans` is this crate's, and a needle that \
         only knew the first would have let `host_mut()?.seal_plans(..)` through a handler \
         untouched -- the review found exactly that hole",
    ),
    (
        ".restore(",
        "restoring a snapshot into the match belongs to the match host (T17's `Host::resume`): \
         a handler that could restore could put the whole world back to a moment of its choosing",
    ),
    (
        "phased_tick",
        "the walled harness's phase stepper (S2's plan, E30) belongs to the match host",
    ),
    (
        "seed_layout",
        "the walled harness's layout seam (S2's plan, E30) belongs to the match host",
    ),
    (
        "seed_layout_with_settings",
        "the walled harness's layout seam with its mandate settings (S3's plan, E13) belongs to \
         the match host",
    ),
];

/// The two modules every method handler lives in.
///
/// Neither may name any of [`STEPPING`] — but both may name `Plan::compile`,
/// and `surface/planning.rs` does. That pairing is the whole of T13b's addition
/// to this file, so it is written down rather than inferred: compiling a
/// playbook is a pure function of the playbook and the rules table, and
/// **sealing it** is what belongs to the host. A rule that banned both would
/// have put the second door at `begin_push`, where nobody is listening for its
/// refusal; a rule that allowed both would have let a handler rewrite the
/// match.
const HANDLER_MODULES: &[&str] = &["knowledge.rs", "planning.rs", "watch.rs"];

/// The module that may name [`STEPPING`]'s needles outright.
const HOST_MODULE: &str = "host.rs";

/// The module that may *reach* them, and only through the host.
const HOST_DRIVER_MODULE: &str = "surface.rs";

/// The one handler module that may drive the live match (item 107 (4)).
///
/// Matched by its **bare file name**, like every other module on these lists,
/// which is why the view handler is `surface/watch.rs` and not
/// `surface/view.rs`: `src/view.rs` already exists and a scan that keyed on a
/// bare name could not tell the two apart.
const CONTROL_MODULE: &str = "control.rs";

/// What [`CONTROL_MODULE`] may name of [`STEPPING`]: the surface's three
/// driving methods, and those alone.
const CONTROL_MAY_DRIVE: &[&str] = &[".step(", "begin_push", "end_recap"];

/// What [`CONTROL_MODULE`] may never name.
///
/// The exemption above is "drive the live match", not "reach into it". A
/// control handler that could name a `Runner`, a `World` or `host_mut` could
/// do anything the host can; one that could `.clone()` could fork a match in a
/// crate whose whole rule is that it does not
/// (AGENTS.md section 3 rule 2).
const CONTROL_BANNED: &[(&str, &str)] = &[
    ("Runner", "the runner is the host's, not a handler's"),
    ("Host", "the host type is the host module's"),
    ("World", "the world is the host's"),
    (
        "host_mut",
        "a control handler drives the surface, which drives the host: two doors would be two \
         places the phase check could be forgotten",
    ),
    ("seal_plans", "sealing is the host's (item 103 (1))"),
    ("seal_playbook", "the runner's spelling of the same thing"),
    (
        "snapshot",
        "the frozen planning snapshot is a read the knowledge handlers already have",
    ),
    (
        ".clone()",
        "a cloned match is a forked match, in a crate that must not fork one",
    ),
    ("file_voxel_edit", "the host-side test seam"),
    ("file_damage", "the host-side test seam"),
];

/// The `#[allow]`s this crate is permitted, each with the reason it is not a
/// determinism allowance.
///
/// A determinism lint is never on this list and never will be: AGENTS.md section
/// 5 says an `#[allow]` that defeats one is a contract change wearing a
/// disguise.
///
/// `clippy::too_many_lines` is on it for `strings::event_text`, whose match has
/// one arm per sim event kind on purpose, so a kind added to the sim's bus fails
/// to compile there (S2's plan, E17; S2's `fire` took it past the threshold).
const PERMITTED_ALLOWS: &[&str] = &[
    "clippy::struct_field_names",
    "clippy::unused_self",
    "clippy::too_many_lines",
];

/// Every `.rs` file under `crates/gateway/src`, in path order.
///
/// `std::fs::read_dir` is on `clippy.toml`'s disallowed list because directory
/// order differs between ext4, NTFS and APFS. The sanctioned use is exactly this
/// one, and `clippy.toml`'s own reason string says so: collect, sort, then use.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml sanctions read_dir when the listing is collected and sorted before use, which is what happens here"
)]
fn sources() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack: Vec<PathBuf> = vec![root];
    let mut found: Vec<PathBuf> = Vec::new();
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        let mut here: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        here.sort();
        for path in here {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// What ends a line read from a checkout made with CRLF line ends, and is not
/// part of it (what `str::lines` drops).
const CARRIAGE_RETURN: char = '\u{d}';

/// The file's shipped code: comment-only lines removed, so prose *about* a
/// banned name is not mistaken for a use of it, and every item a real
/// `#[cfg(test)]` attribute stands on blanked ([`test_item_spans`]), because a
/// unit test is allowed to panic -- that is what a failing assertion is -- and
/// is not compiled into the game. An attribute written in a comment or a
/// string attributes nothing, and the rest of the file is read past it (the
/// review of #219's T13 and T14; lane `opsave` ported `gamectl`'s reading).
/// Only the item's own span is blanked, so code after its closing brace on
/// that brace's line is read (`code_after_a_test_items_closing_brace_is_read`).
/// A line the blanking leaves empty is dropped. The lines keep their
/// literals, which the needles of some scans are in.
fn code_lines(text: &str) -> Vec<(usize, String)> {
    let spans = test_item_spans(text);
    let covered = |index: usize| {
        spans
            .iter()
            .any(|(from, to)| (*from..=*to).contains(&index))
    };
    let mut lines = Vec::new();
    let mut start = 0_usize;
    let pieces = text.split('\n').count();
    for (index, line) in text.split('\n').enumerate() {
        let length = line.chars().count();
        // A line a test item touches is dropped only when nothing unblanked is
        // left on it (an empty line inside an item, whose line feed the item
        // covers, included): shipped code between two test items on one line
        // is read (fix pass 3, item 8).
        let touched = (0..length.max(1)).any(|offset| covered(start.saturating_add(offset)));
        let shipped: String = line
            .chars()
            .enumerate()
            .map(|(offset, c)| {
                if covered(start.saturating_add(offset)) {
                    ' '
                } else {
                    c
                }
            })
            .collect();
        start = start.saturating_add(length).saturating_add(1);
        let inside = touched && shipped.trim().is_empty();
        if inside || shipped.trim_start().starts_with("//") {
            continue;
        }
        if index == pieces.saturating_sub(1) && line.is_empty() {
            // The empty piece after a final line feed is not a line.
            continue;
        }
        lines.push((
            index.saturating_add(1),
            shipped.trim_end_matches(CARRIAGE_RETURN).to_owned(),
        ));
    }
    lines
}

/// `text` with every character of every item a real `#[cfg(test)]` attribute
/// stands on blanked to a space, line feeds kept, so a finding names the
/// file's own line and column, code after a test item's end on its last line
/// is read, and a literal or a comment of the shipped code is read as
/// written.
fn shipped_text(text: &str) -> String {
    let spans = test_item_spans(text);
    text.chars()
        .enumerate()
        .map(|(index, c)| {
            if c != '\n'
                && spans
                    .iter()
                    .any(|(from, to)| (*from..=*to).contains(&index))
            {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// `text`'s shipped code as [`code_only`] reads it (every literal emptied,
/// every comment removed), with every item a `#[cfg(test)]` in that code
/// stands on blanked: `gamectl`'s `shipped_code`, the same reading.
fn shipped_code(text: &str) -> String {
    code_only(&shipped_text(text))
}

/// The characters, counting from 0 and both ends included, of each item that
/// a real `#[cfg(test)]` in `text` attributes: from the attribute through any
/// further attributes to the item's end, which is its first `;` outside
/// brackets, or the brace that closes its first `{`. Read over [`masked`],
/// which blanks every comment and literal without moving a character, so an
/// attribute or a bracket in a comment or a literal counts for nothing and a
/// span's ends are `text`'s own positions. `gamectl`'s `test_item_lines`,
/// read by character rather than by line: the gateway's scan blanks the item
/// and not the rest of its last line.
fn test_item_spans(text: &str) -> Vec<(usize, usize)> {
    const ATTRIBUTE: &[char] = &['#', '[', 'c', 'f', 'g', '(', 't', 'e', 's', 't', ')', ']'];
    let code = masked(text);
    let mut spans = Vec::new();
    let mut from = 0_usize;
    while let Some(start) = (from..code.len())
        .find(|at| code.get(*at..at.saturating_add(ATTRIBUTE.len())) == Some(ATTRIBUTE))
    {
        let mut at = start.saturating_add(ATTRIBUTE.len());
        let mut depth = 0_usize;
        let mut braced = false;
        let end = loop {
            let Some(c) = code.get(at).copied() else {
                break code.len().saturating_sub(1);
            };
            match c {
                '(' | '[' => depth = depth.saturating_add(1),
                ')' | ']' => depth = depth.saturating_sub(1),
                '{' => {
                    depth = depth.saturating_add(1);
                    braced = true;
                }
                '}' => {
                    depth = depth.saturating_sub(1);
                    if braced && depth == 0 {
                        break at;
                    }
                }
                ';' if depth == 0 => break at,
                _ => {}
            }
            at = at.saturating_add(1);
        };
        spans.push((start, end));
        from = end.saturating_add(1);
    }
    spans
}

/// `text`'s characters with every comment and literal blanked to spaces, line
/// feeds kept: [`code_only`]'s reading, but no character moves, so a position
/// in it is a position in `text`.
fn masked(text: &str) -> Vec<char> {
    let chars: Vec<char> = text.chars().collect();
    let at_char = |index: usize| chars.get(index).copied();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut at = 0_usize;
    while let Some(c) = at_char(at) {
        let next = at_char(at.saturating_add(1));
        let word_before = at
            .checked_sub(1)
            .and_then(at_char)
            .is_some_and(|before| before.is_alphanumeric() || before == '_');
        let skipped = if c == '/' && next == Some('/') {
            Some(line_comment_end(&chars, at))
        } else if c == '/' && next == Some('*') {
            Some(block_comment_end(&chars, at))
        } else if word_before {
            None
        } else {
            literal_end(&chars, at).map(|(end, _)| end)
        };
        if let Some(end) = skipped {
            for index in at..end {
                out.push(if at_char(index) == Some('\n') {
                    '\n'
                } else {
                    ' '
                });
            }
            at = end;
        } else {
            out.push(c);
            at = at.saturating_add(1);
        }
    }
    out
}

/// True when `line` uses `needle` as a name rather than merely spelling its
/// letters inside a longer one.
///
/// The hole this closes was real and `InstantiateTemplate` found it: the word
/// contains `Instant`, so a plain substring scan reported a wall clock in the
/// method table. A scan that fires on the wrong thing gets deleted, and a scan
/// that fires on nothing at all is worse. So a needle that begins and ends with
/// an identifier character has to sit on identifier boundaries, and a needle
/// with punctuation in it (`.step(`, `World::new`) is matched as written,
/// because its punctuation is the boundary.
fn uses(line: &str, needle: &str) -> bool {
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let bounded = needle.bytes().next().is_some_and(is_word)
        && needle.bytes().next_back().is_some_and(is_word);
    if !bounded {
        return line.contains(needle);
    }
    let haystack = line.as_bytes();
    let mut from = 0_usize;
    while let Some(offset) = line.get(from..).and_then(|rest| rest.find(needle)) {
        let start = from.saturating_add(offset);
        let end = start.saturating_add(needle.len());
        let before_is_word = start
            .checked_sub(1)
            .and_then(|index| haystack.get(index))
            .copied()
            .is_some_and(is_word);
        let after_is_word = haystack.get(end).copied().is_some_and(is_word);
        if !before_is_word && !after_is_word {
            return true;
        }
        from = start.saturating_add(1);
    }
    false
}

/// The name of the method `line` defines, when it is an `impl` block's method
/// definition (four spaces in, `fn` with or without a visibility).
fn method_defined_on(line: &str) -> Option<String> {
    let rest = line.strip_prefix("    ")?;
    if rest.starts_with(' ') {
        return None;
    }
    let rest = rest
        .strip_prefix("pub(crate) ")
        .or_else(|| rest.strip_prefix("pub(super) "))
        .or_else(|| rest.strip_prefix("pub "))
        .unwrap_or(rest);
    let rest = rest.strip_prefix("fn ")?;
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    rest.get(..end).map(str::to_owned)
}

fn scan(needles: &[(&str, &str)]) -> Vec<String> {
    scan_except(needles, &[])
}

/// [`scan`], skipping files whose path ends with an entry on `exempt`: a bare
/// file name, or a path under the crate such as `src/serve.rs`, compared
/// component by component.
fn scan_except(needles: &[(&str, &str)], exempt: &[&str]) -> Vec<String> {
    let mut findings: Vec<String> = Vec::new();
    for path in sources() {
        if exempt.iter().any(|name| path.ends_with(name)) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in code_lines(&text) {
            for (needle, why) in needles {
                if uses(&line, needle) {
                    findings.push(format!(
                        "{}:{number}: `{needle}` -- {why}\n    {}",
                        path.display(),
                        line.trim()
                    ));
                }
            }
        }
    }
    findings
}

#[test]
fn the_crate_has_sources_to_check() {
    let files = sources();
    assert!(
        files.len() >= 15,
        "the source walk found only {} files; it is looking in the wrong place and would pass \
         vacuously",
        files.len()
    );
    for expected in ["lib.rs", "token.rs", "fog.rs", "handshake.rs", "cache.rs"] {
        assert!(
            files.iter().any(|path| path.ends_with(expected)),
            "{expected} is outside the checked set"
        );
    }
}

#[test]
fn the_source_scan_is_not_looking_at_an_empty_set() {
    // The hole this closes is a scan that reads nothing and passes. Assert that
    // the filter keeps the code and drops the prose, on a spelling that is in
    // the source right now.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("fog.rs");
    let text = std::fs::read_to_string(&path).expect("fog.rs is where it always was");
    let lines = code_lines(&text);
    assert!(lines.len() > 50, "the comment filter ate the file");
    assert!(
        text.contains("HashSet"),
        "fog.rs's own prose explains why it is a BTreeSet; if that sentence went, so did the \
         thing this test is proving it can see past"
    );
    assert!(
        !lines.iter().any(|(_, line)| line.contains("HashSet")),
        "the prose survives the comment filter, so the filter is not filtering"
    );
}

#[test]
fn no_clock_no_hash_container_and_no_float_reach_the_crate() {
    let findings = scan(BANNED);
    assert!(
        findings.is_empty(),
        "the determinism rule set is broken in the source text:\n{}",
        findings.join("\n")
    );
}

#[test]
fn no_wildcard_binding_is_written_anywhere() {
    let findings = scan(NO_WILDCARD_BINDING);
    assert!(
        findings.is_empty(),
        "the gateway binds 127.0.0.1 and ::1 and nothing else:\n{}",
        findings.join("\n")
    );
}

/// Every thread the gateway makes is made on a stated stack (S3's `vfyq2`,
/// fix pass 2, item E): through `pharmakos_plan_core::stack::player_text_thread`,
/// never `std::thread::spawn` (the platform's default, which `RUST_MIN_STACK`
/// moves) nor a builder of its own (a second size). The surface thread answers
/// every method and the connection threads parse every frame, so the depth
/// bound is a stack bound only on the stack this names.
///
/// An allow-list, not a list of banned spellings (S3's `probw`, carried from
/// `vfyq2`'s record; decisions-log item 168 (5)), and a scan of the whole
/// source text, not of its lines one at a time (the reviews of #212): any
/// whole-word `thread` in the shipped code, outside every string, byte-string,
/// raw-string (`r#"..."#`, `br#"..."#`) and character literal and every line
/// and block comment, each of which may span lines, is refused unless it is
/// written `thread::JoinHandle`, the handle `player_text_thread`'s builder
/// answers. So `std::thread::scope`, an imported `Builder`, a braced import
/// over several lines (`thread,`, `thread as t,`), a renamed module, a spaced
/// path (`thread ::spawn`), a local binding named `thread`, and a spawn after
/// a literal or a comment that ends on its line are all refused, and a spawn
/// written inside a string or a comment is not code.
fn thread_offences(text: &str) -> Vec<(usize, &'static str)> {
    let code = code_only(text);
    let mut out = Vec::new();
    for (number, line) in code.lines().enumerate() {
        let mut from = 0_usize;
        while let Some(offset) = line.get(from..).and_then(|rest| find_word(rest, "thread")) {
            let start = from.saturating_add(offset);
            let end = start.saturating_add("thread".len());
            if !line.get(end..).unwrap_or("").starts_with("::JoinHandle") {
                out.push((
                    number.saturating_add(1),
                    "names `thread` other than as `thread::JoinHandle`: make every thread with \
                     player_text_thread",
                ));
                break;
            }
            from = end;
        }
    }
    out
}

/// [`thread_offences`] of one piece of text, its first finding.
fn thread_offence(text: &str) -> Option<&'static str> {
    thread_offences(text).first().map(|(_, why)| *why)
}

/// The byte offset of the first whole-word `word` in `text`, as [`uses`]
/// bounds a word.
fn find_word(text: &str, word: &str) -> Option<usize> {
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let bytes = text.as_bytes();
    let mut from = 0_usize;
    while let Some(offset) = text.get(from..).and_then(|rest| rest.find(word)) {
        let start = from.saturating_add(offset);
        let end = start.saturating_add(word.len());
        let before = start
            .checked_sub(1)
            .and_then(|index| bytes.get(index))
            .copied()
            .is_some_and(is_word);
        let after = bytes.get(end).copied().is_some_and(is_word);
        if !before && !after {
            return Some(start);
        }
        from = start.saturating_add(1);
    }
    None
}

/// `text`'s code, line for line: every literal (string, byte string, raw and
/// byte raw string, character and byte character) reduced to an empty pair
/// of quotes and every comment (line, and block, nested) removed, each
/// keeping the newlines it spans so a finding names its line. A lifetime
/// (`'a`) is not a literal and is kept. An unterminated literal or comment
/// runs to the end of the text, as the compiler would read it.
fn code_only(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let at_char = |index: usize| chars.get(index).copied();
    let mut out = String::with_capacity(text.len());
    let mut at = 0_usize;
    while let Some(c) = at_char(at) {
        let next = at_char(at.saturating_add(1));
        let word_before = at
            .checked_sub(1)
            .and_then(at_char)
            .is_some_and(|before| before.is_alphanumeric() || before == '_');
        let skipped = if c == '/' && next == Some('/') {
            Some((line_comment_end(&chars, at), ""))
        } else if c == '/' && next == Some('*') {
            Some((block_comment_end(&chars, at), ""))
        } else if word_before {
            None
        } else {
            literal_end(&chars, at)
        };
        if let Some((end, kept)) = skipped {
            out.push_str(kept);
            for index in at..end {
                if at_char(index) == Some('\n') {
                    out.push('\n');
                }
            }
            at = end;
        } else {
            out.push(c);
            at = at.saturating_add(1);
        }
    }
    out
}

/// Where the line comment starting at `at` ends: at its newline, kept.
fn line_comment_end(chars: &[char], at: usize) -> usize {
    let mut end = at;
    while chars.get(end).is_some_and(|c| *c != '\n') {
        end = end.saturating_add(1);
    }
    end
}

/// One past the end of the block comment starting at `at`, nested comments
/// included; the text's end for one never closed.
fn block_comment_end(chars: &[char], at: usize) -> usize {
    let mut end = at;
    let mut depth = 0_usize;
    loop {
        match (chars.get(end), chars.get(end.saturating_add(1))) {
            (None, _) => return end,
            (Some('/'), Some('*')) => {
                depth = depth.saturating_add(1);
                end = end.saturating_add(2);
            }
            (Some('*'), Some('/')) => {
                depth = depth.saturating_sub(1);
                end = end.saturating_add(2);
                if depth == 0 {
                    return end;
                }
            }
            _ => end = end.saturating_add(1),
        }
    }
}

/// The literal starting at `at`, if one does: one past its end, and what
/// stands in for it (an empty pair of its quotes). A string, byte string,
/// raw or byte raw string, character or byte character; `None` for anything
/// else, a lifetime included.
fn literal_end(chars: &[char], at: usize) -> Option<(usize, &'static str)> {
    let at_char = |index: usize| chars.get(index).copied();
    let c = at_char(at)?;
    let next = at_char(at.saturating_add(1));
    let (prefix, raw) = match (c, next) {
        ('b', Some('r')) => (2, true),
        ('r', _) => (1, true),
        ('b', Some('"' | '\'')) => (1, false),
        _ => (0, false),
    };
    if raw {
        let mut hashes = 0_usize;
        while at_char(at.saturating_add(prefix).saturating_add(hashes)) == Some('#') {
            hashes = hashes.saturating_add(1);
        }
        let open = at.saturating_add(prefix).saturating_add(hashes);
        if at_char(open) != Some('"') {
            return None;
        }
        let mut close = open.saturating_add(1);
        while let Some(c) = at_char(close) {
            if c == '"' && (1..=hashes).all(|k| at_char(close.saturating_add(k)) == Some('#')) {
                return Some((close.saturating_add(1).saturating_add(hashes), "\"\""));
            }
            close = close.saturating_add(1);
        }
        return Some((close, "\"\""));
    }
    let quote = at.saturating_add(prefix);
    match at_char(quote) {
        Some('"') => {
            let mut close = quote.saturating_add(1);
            while let Some(c) = at_char(close) {
                match c {
                    '\\' => close = close.saturating_add(2),
                    '"' => return Some((close.saturating_add(1), "\"\"")),
                    _ => close = close.saturating_add(1),
                }
            }
            Some((close, "\"\""))
        }
        Some('\'') => {
            // A character literal: one character or one escape, then a quote.
            let close = if at_char(quote.saturating_add(1)) == Some('\\') {
                (quote.saturating_add(3)..quote.saturating_add(12))
                    .find(|i| at_char(*i) == Some('\''))
            } else {
                (at_char(quote.saturating_add(2)) == Some('\'')).then_some(quote.saturating_add(2))
            };
            close.map(|close| (close.saturating_add(1), "''"))
        }
        _ => None,
    }
}

#[test]
fn every_thread_the_gateway_makes_is_on_the_stated_stack() {
    let mut findings: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        // The shipped code, read whole, so a literal or a comment that spans
        // lines is read as one, less only the items `#[cfg(test)]` attributes
        // ([`shipped_code`]); its lines are the file's.
        for (number, why) in thread_offences(&shipped_code(&text)) {
            let line = text.lines().nth(number.saturating_sub(1)).unwrap_or("");
            findings.push(format!(
                "{}:{number}: {why}\n    {}",
                path.display(),
                line.trim()
            ));
        }
    }
    assert!(
        findings.is_empty(),
        "every gateway thread is made by player_text_thread:\n{}",
        findings.join("\n")
    );
}

/// The review of #219's T13 and T14, re-cast for the gateway (lane
/// `opsave`): the scans once stopped at the first `#[cfg(test)]` text, even
/// one in a comment (T13) or a string (T14), so a spawn after it was never
/// read; now only an item a real attribute stands on is left out, and the
/// rest of the file is read past it. `gamectl`'s cases, held to the same.
#[test]
fn the_scans_read_past_an_attribute_that_attributes_nothing() {
    for text in [
        "fn p() {\n    // #[cfg(test)] is where the tests start\n    let _ = std::thread::spawn(|| ());\n}\n",
        "fn p() {\n    let _s = \"#[cfg(test)]\";\n    let _ = std::thread::spawn(|| ());\n}\n",
        "/* #[cfg(test)] */\nfn p() { let _ = std::thread::spawn(|| ()); }\n",
        "#[cfg(test)]\nmod tests { fn t() {} }\nfn p() { let _ = std::thread::spawn(|| ()); }\n",
        "#[cfg(test)]\nfn t() {}\nfn p() { let _ = std::thread::spawn(|| ()); }\n",
        "#[cfg(test)]\nuse std::thread;\nfn p() { let _ = std::thread::spawn(|| ()); }\n",
    ] {
        assert!(
            !thread_offences(&shipped_code(text)).is_empty(),
            "the thread guard let through: {text}"
        );
        // The line scans read past it too: the fallback scan's needle and the
        // string scan's run, written after the same attribute, are found.
        let planted = text.replace(
            "let _ = std::thread::spawn(|| ());",
            "let x = y.unwrap_or(0);",
        );
        assert!(
            fallback_lines(&planted)
                .iter()
                .any(|line| line.contains("unwrap_or")),
            "the line scans stopped at: {planted}"
        );
        let quoted = text.replace("let _ = std::thread::spawn(|| ());", "let s = \"a  b\";");
        assert!(
            string_literals(&quoted)
                .iter()
                .any(|literal| literal == "a  b"),
            "the string scan stopped at: {quoted}"
        );
    }
    for text in [
        "#[cfg(test)]\nmod tests {\n    fn t() { let _ = std::thread::spawn(|| ()); }\n}\n",
        "#[cfg(test)]\n#[allow(dead_code)]\nfn t() { let _ = std::thread::spawn(|| { \"}\" }); }\n",
    ] {
        assert!(
            thread_offences(&shipped_code(text)).is_empty(),
            "a test item's own thread is not shipped code: {text}"
        );
        assert!(
            code_lines(text)
                .iter()
                .all(|(_, line)| !line.contains("spawn")),
            "a test item's lines are not shipped code: {text}"
        );
    }
    // The finding names the file's own line.
    let text = "// one\n#[cfg(test)]\nmod t {\n}\nfn p() { std::thread::spawn(f); }\n";
    assert_eq!(
        thread_offences(&shipped_code(text))
            .first()
            .map(|(line, _)| *line),
        Some(5)
    );
}

/// Code after a test item's closing brace, on that brace's own line, is
/// shipped code and is read: only the item's own span is blanked
/// ([`test_item_spans`]), never the rest of its last line. A layout `cargo fmt
/// --check` refuses is still a layout the compiler builds, so the scan does
/// not lean on the formatter.
#[test]
fn code_after_a_test_items_closing_brace_is_read() {
    for text in [
        "#[cfg(test)]\nmod t {\n} fn p() { std::thread::spawn(f); }\n",
        "#[cfg(test)]\nfn t() { let _ = \"}\"; } fn p() { std::thread::spawn(f); }\n",
        "#[cfg(test)]\nuse std::fs; fn p() { std::thread::spawn(f); }\n",
    ] {
        assert!(
            !thread_offences(&shipped_code(text)).is_empty(),
            "the code after the test item was not read: {text}"
        );
        assert!(
            code_lines(text)
                .iter()
                .any(|(_, line)| line.contains("std::thread::spawn(f)")),
            "the line scans did not read the code after the test item: {text}"
        );
        assert!(
            string_literals(&text.replace("spawn(f)", "spawn(\"a  b\")"))
                .iter()
                .any(|literal| literal == "a  b"),
            "the string scan did not read the code after the test item: {text}"
        );
    }
    // Shipped code between two test items on one line is read too (fix pass
    // 3, item 8; review 3's evasion probe): a line is dropped only when
    // nothing unblanked is left on it.
    let between = "#[cfg(test)] fn a() {} fn p() { std::thread::spawn(f); } #[cfg(test)] fn b() {}
";
    assert!(
        code_lines(between)
            .iter()
            .any(|(_, line)| line.contains("std::thread::spawn(f)")),
        "the line scans dropped the code between two test items"
    );
    assert!(!thread_offences(&shipped_code(between)).is_empty());
    // The test item itself is still blanked, up to and including its end.
    let text = "#[cfg(test)]\nmod t { fn t() { std::thread::spawn(g); }\n} fn p() {}\n";
    assert!(
        thread_offences(&shipped_code(text)).is_empty(),
        "a test item's own thread is not shipped code"
    );
    assert!(
        code_lines(text)
            .iter()
            .all(|(_, line)| !line.contains("spawn")),
        "a test item's lines are not shipped code"
    );
}

#[test]
fn the_thread_guard_refuses_every_way_round_the_door() {
    for text in [
        "    let handle = std::thread::spawn(move || run());",
        "    thread::spawn(move || run());",
        "    let built = std::thread::Builder::new().stack_size(1);",
        "    std::thread::scope(|s| { s.spawn(|| run()); });",
        "    thread::scope(|s| { s.spawn(|| run()); });",
        "use std::thread::Builder;",
        "use std::thread::{Builder, JoinHandle};",
        "use std::thread;",
        "use std::{fs, thread};",
        "use std::thread as t;",
        "    let _ = ::std::thread::available_parallelism();",
        // The first review of #212: lines of a braced `use` written over
        // several lines, a character literal that holds a quote, a spaced
        // path.
        "use std::{\n    fs,\n    thread as t,\n};",
        "use std::{\n    fs,\n    thread,\n};",
        "    let q = '\"'; std::thread::spawn(f);",
        "    std::thread\t::spawn(f);",
        "    let q = '\\''; thread::spawn(f);",
        "    let thread = player_text_thread(\"x\");",
        "    let s = r#\"a \" quote\"#; std::thread::spawn(f);",
        // The second review: a block comment holding a quote, the end of a
        // string that began on an earlier line, byte and byte raw strings.
        "    /* \" */ std::thread::spawn(f);",
        "    let s = \"a\nline\"; std::thread::spawn(f);",
        "    let s = br#\"a \" b\"#; std::thread::spawn(f);",
        "    let s = b\"x\"; std::thread::spawn(f);",
        "    let c = b'\"'; std::thread::spawn(f);",
        "    /* one /* nested */ still */ std::thread::spawn(f);",
    ] {
        assert!(
            thread_offence(text).is_some(),
            "the guard let through: {text}"
        );
    }
    for text in [
        "use std::thread::JoinHandle;",
        "    let mut threads: Vec<JoinHandle<()>> = Vec::new();",
        "    let accepter = pharmakos_plan_core::stack::player_text_thread(\"gateway accept\")",
        "    threads.push(accepter);",
        ") -> Result<std::thread::JoinHandle<()>, Error> {",
        "                \"this host could not start a connection thread: {}\",",
        "    \"a \\\"quoted\\\" thread\" ; let threads = 1;",
        "    let s = r#\"a thread\"#; // the surface thread",
        "fn f<'a>(x: &'a str) -> &'a str { x }",
        // A spawn faked inside a comment or a literal is not code.
        "    /* std::thread::spawn(f); */",
        "    /*\n    std::thread::spawn(f);\n    */",
        "    let s = \"\n    std::thread::spawn(f);\n\";",
        "    let s = br#\"\n    std::thread::spawn(f);\n\"#;",
        "    let s = b\"std::thread::spawn(f)\";",
    ] {
        assert_eq!(thread_offence(text), None, "the guard refused: {text}");
    }
}

/// The one file of the gateway that names the sim's `PlanError` (S3's
/// `vfyq2`, fix pass 5, item L3).
const PLAN_WORDS: &str = "src/surface/plan_words.rs";

/// What [`PLAN_WORDS`] may not write: every way to say a value through its
/// `Display` or `Debug` rather than through `said!`.
const PLAN_WORDS_BANNED: &[&str] = &[
    "format!(",
    "format_args!(",
    "write!(",
    "writeln!(",
    "print!(",
    "println!(",
    "eprint!(",
    "eprintln!(",
    "to_string(",
    "{:?}",
    "{error}",
    "{self}",
    "Debug",
    "Display",
    "fmt::",
];

/// Every sim function that refuses with a `PlanError`, as the identifier a
/// caller must write to reach it (S3's `vfyq2`, fix pass 6, item 5): its
/// own name, or, for a name the sim gives many functions (`from_rules`,
/// `new`), its owner's. Read from the sim's sources, so a new one is held
/// without this test changing. An alias (`use … as X`) still names it once.
fn plan_error_producers() -> Vec<String> {
    let files = sources_of("sim");
    let mut found = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let mut owner: Option<String> = None;
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("impl ")
                && !line.starts_with(' ')
            {
                let name = rest.split(" for ").last().unwrap_or(rest);
                owner = name
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .find(|word| !word.is_empty())
                    .map(str::to_owned);
            } else if !line.starts_with(' ')
                && !trimmed.is_empty()
                && !trimmed.starts_with('}')
                && !trimmed.starts_with("//")
                && !trimmed.starts_with('#')
            {
                owner = None;
            }
            let Some(after) = trimmed.strip_prefix("pub fn ") else {
                continue;
            };
            let name: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let signature: String = lines
                .iter()
                .skip(index)
                .take(8)
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            let signature = signature.split('{').next().unwrap_or_default();
            if !signature.contains("PlanError>") {
                continue;
            }
            let shared = matches!(name.as_str(), "from_rules" | "new" | "default");
            found.push(match (&owner, shared) {
                (Some(owner), true) => owner.clone(),
                _ => name,
            });
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The one line outside [`PLAN_WORDS`] that may name a producer: the sim's
/// own door, `compile_decoded`'s compile in `src/surface/planning.rs`, whose
/// refusal goes straight to `plan_refusal`.
const COMPILE_SITE: (&str, &str) = (
    "src/surface/planning.rs",
    "Plan::compile(playbook, rules).map_err(|error| super::plan_words::plan_refusal(&error))",
);

/// A file's code, line by line: its string literals' contents (a string
/// that runs over several lines included) and its `//` comments dropped, so
/// a word in a message or a comment is not taken for code.
fn code_of(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for line in text.lines() {
        let mut out = String::with_capacity(line.len());
        let mut previous = ' ';
        for character in line.chars() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == '"' {
                    in_string = false;
                    out.push('"');
                }
            } else if character == '"' && previous != '\'' {
                in_string = true;
                out.push('"');
            } else if character == '/' && previous == '/' {
                out.pop();
                break;
            } else {
                out.push(character);
            }
            previous = character;
        }
        // A `\` at a line's end continues the string onto the next.
        escaped = false;
        lines.push(out);
    }
    lines
}

/// The identifiers on a line of code.
fn identifiers_of(code: &str) -> Vec<&str> {
    code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .collect()
}

/// A `PlanError` reaches a seat only through the gateway's own sentence for
/// it (S3's `vfyq2`, fix pass 3, item M2 (a)), held file by file (fix pass 5,
/// item L3) and token by token (fix pass 6, item 5). The sim's `Display` for
/// it writes ids and labels raw. So the type, and the identifier of every
/// sim function that refuses with one, is named in [`PLAN_WORDS`] alone, and
/// at [`COMPILE_SITE`]. A call, a function value (`let f = Plan::compile`),
/// an alias, a function of any form and a statement that also says `.ok()`
/// or `plan_refusal(` are all found, because the rule is the token in the
/// file, not the statement. [`PLAN_WORDS`] says the type with `said!` and
/// nothing that formats a value. Every line is read, a unit test's included.
#[test]
fn a_plan_error_reaches_a_seat_only_through_the_gateways_own_sentence() {
    let mut tokens = plan_error_producers();
    assert!(
        tokens.iter().any(|token| token == "compile"),
        "the producers are read from the sim: {tokens:?}"
    );
    tokens.push(String::from("PlanError"));
    let mut stray: Vec<String> = Vec::new();
    let mut named_in_words = false;
    let mut compile_sites = 0_usize;
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let words = path.ends_with(PLAN_WORDS);
        let site = path.ends_with(COMPILE_SITE.0);
        let codes = code_of(&text);
        for (index, (line, code)) in text.lines().zip(&codes).enumerate() {
            let number = index.saturating_add(1);
            let identifiers = identifiers_of(code);
            let named: Vec<&String> = tokens
                .iter()
                .filter(|token| identifiers.contains(&token.as_str()))
                .collect();
            if words {
                named_in_words |= identifiers.contains(&"PlanError");
                for banned in PLAN_WORDS_BANNED {
                    if code.contains(banned) {
                        stray.push(format!(
                            "{}:{number}: `{banned}` in the PlanError words: {}",
                            path.display(),
                            line.trim()
                        ));
                    }
                }
                continue;
            }
            if named.is_empty() {
                continue;
            }
            if site && line.trim() == COMPILE_SITE.1 {
                compile_sites = compile_sites.saturating_add(1);
                continue;
            }
            stray.push(format!(
                "{}:{number}: names {named:?} outside {PLAN_WORDS}: {}",
                path.display(),
                line.trim()
            ));
        }
    }
    assert!(
        named_in_words,
        "{PLAN_WORDS} names PlanError and says every one"
    );
    assert_eq!(compile_sites, 1, "the one compile site, as written");
    assert!(
        stray.is_empty(),
        "a PlanError is said only by {PLAN_WORDS}:\n{}",
        stray.join("\n")
    );
}

/// S3's `p3est`: what makes a search scratch. A fresh one is megabytes of
/// zeroes on the shipped map, and making one per planning read put
/// `estimate_route` several times over P3's bar (`src/routes.rs`, "One
/// scratch"), so shipped code makes exactly one, the hosted match's, in
/// `PlanningScratch::lend`.
const OWN_SCRATCH: &[(&str, &str)] = &[
    (
        "Scratch::for_map",
        "a planning read searches in the match's one scratch (`Host::planning_scratch`)",
    ),
    (
        "Scratch::new",
        "a planning read searches in the match's one scratch (`Host::planning_scratch`)",
    ),
    (
        "scratch_for",
        "`targeting::scratch_for` is for a test or a tool that holds a world and no host",
    ),
];

/// How many code lines of the source file `name` use `needle`.
fn uses_in(name: &str, needle: &str) -> usize {
    sources()
        .iter()
        .filter(|path| path.ends_with(name))
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .map(|text| {
            code_lines(&text)
                .iter()
                .filter(|(_, line)| uses(line, needle))
                .count()
        })
        .sum()
}

#[test]
fn every_planning_read_searches_in_the_matchs_one_scratch() {
    assert!(
        uses(
            "let scratch = Scratch::for_map(surface, edge);",
            "Scratch::for_map"
        ) && uses("scratch: scratch_for(world)?,", "scratch_for"),
        "the scan finds what it looks for"
    );
    let findings = scan_except(OWN_SCRATCH, &["src/routes.rs", "src/targeting.rs"]);
    assert!(
        findings.is_empty(),
        "a search scratch made outside the match's one:
{}",
        findings.join(
            "
"
        )
    );
    assert_eq!(
        uses_in("src/routes.rs", "Scratch::for_map"),
        1,
        "`PlanningScratch::lend` makes the match's scratch, and nothing else in the file makes one"
    );
    assert_eq!(uses_in("src/routes.rs", "Scratch::new"), 0);
    assert_eq!(uses_in("src/routes.rs", "scratch_for"), 0);
    assert_eq!(
        (
            uses_in("src/targeting.rs", "Scratch::for_map"),
            uses_in("src/targeting.rs", "scratch_for"),
            uses_in("src/targeting.rs", "Scratch::new"),
        ),
        (1, 1, 0),
        "`scratch_for` is defined in `targeting.rs` and called by nothing shipped"
    );
}

#[test]
fn the_gateway_never_dials_out() {
    let findings = scan(NO_OUTBOUND);
    assert!(
        findings.is_empty(),
        "the gateway accepts connections and makes none:\n{}",
        findings.join("\n")
    );
}

#[test]
fn no_research_build_is_named_anywhere_in_the_crate() {
    let findings = scan(NO_DRY_RUNS);
    assert!(
        findings.is_empty(),
        "the no-dry-runs rule is broken in the source text:\n{}",
        findings.join("\n")
    );
}

/// The match is built and stepped in `host.rs`, driven from `surface.rs` and
/// from `surface/control.rs`, and named nowhere else.
///
/// Four assertions, and the rule they hold is the one T16a changed -- see the
/// module docs, rule 6.
///
/// * **Nothing outside those three names a stepping call at all**, so a read
///   handler cannot answer "what would happen" by making it happen.
/// * **`surface.rs` reaches the runner only through `host_mut()`**, so there
///   is no second path to it in the file that drives it.
/// * **`surface/control.rs` names the three driving methods and nothing else
///   on the list**, which is the whole of the exemption: it may move the live
///   match on, and it may not reach inside it.
/// * **Each exemption guards something**: the host really does build and step
///   a match, and the control module really does drive one. An exemption over
///   an empty room is a rule that has quietly stopped applying.
#[test]
fn the_match_is_stepped_in_one_module() {
    let findings = scan_except(STEPPING, &[HOST_MODULE, HOST_DRIVER_MODULE, CONTROL_MODULE]);
    assert!(
        findings.is_empty(),
        "the match is stepped outside {HOST_MODULE}, {HOST_DRIVER_MODULE} and {CONTROL_MODULE}:\n{}",
        findings.join("\n")
    );

    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let driver = source.join(HOST_DRIVER_MODULE);
    let text = std::fs::read_to_string(&driver).expect("the surface is where it always was");
    let mut direct: Vec<String> = Vec::new();
    for (number, line) in code_lines(&text) {
        // A `fn begin_push(...)` of the surface's own is a name, not a call:
        // the surface has host-driving methods and they are allowed to be
        // called what they drive. What the rule is about is the body.
        let trimmed = line.trim_start();
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            continue;
        }
        for (needle, _) in STEPPING {
            if uses(&line, needle) && !line.contains("host_mut()") {
                direct.push(format!("{HOST_DRIVER_MODULE}:{number}: {}", line.trim()));
            }
        }
    }
    assert!(
        direct.is_empty(),
        "{HOST_DRIVER_MODULE} reaches the runner other than through the host:\n{}",
        direct.join("\n")
    );

    // The control module: the three driving methods, and nothing else.
    let control = source.join("surface").join(CONTROL_MODULE);
    let control_text =
        std::fs::read_to_string(&control).expect("the control handlers are where they live");
    let control_lines = code_lines(&control_text);
    assert!(
        control_lines.len() > 40,
        "{CONTROL_MODULE} read as {} lines of code; the scan is looking at the wrong file",
        control_lines.len()
    );
    let mut overreach: Vec<String> = Vec::new();
    for (number, line) in &control_lines {
        for (needle, _) in STEPPING {
            if uses(line, needle) && !CONTROL_MAY_DRIVE.contains(needle) {
                overreach.push(format!(
                    "{CONTROL_MODULE}:{number}: `{needle}`\n    {}",
                    line.trim()
                ));
            }
        }
        for (needle, why) in CONTROL_BANNED {
            if uses(line, needle) {
                overreach.push(format!(
                    "{CONTROL_MODULE}:{number}: `{needle}` -- {why}\n    {}",
                    line.trim()
                ));
            }
        }
    }
    assert!(
        overreach.is_empty(),
        "a control handler reaches past the three driving methods:\n{}",
        overreach.join("\n")
    );

    // And neither exemption is guarding an empty room.
    let host = source.join(HOST_MODULE);
    let host_text = std::fs::read_to_string(&host).expect("the host is where it was written");
    let host_lines = code_lines(&host_text);
    for (needle, _) in STEPPING {
        assert!(
            host_lines.iter().any(|(_, line)| uses(line, needle)),
            "`{needle}` is on the stepping list and nothing in {HOST_MODULE} does it, so the \
             exemption is guarding nothing"
        );
    }
    for needle in CONTROL_MAY_DRIVE {
        assert!(
            control_lines.iter().any(|(_, line)| uses(line, needle)),
            "`{needle}` is what {CONTROL_MODULE} is exempted for and it does not do it, so the \
             exemption is guarding nothing"
        );
    }
}

/// The host-side test seam is reached by no wire method (item 107 (7)).
///
/// The scan above exempts `surface.rs` for anything reached through
/// `host_mut()`, which is right for a stepping call and wrong for these two:
/// `self.host_mut()?.file_voxel_edit(..)` inside a dispatch arm would pass it
/// and would be a wire method that edits the world. So the two names are
/// checked separately, over the whole crate, with **no** exemption but the
/// module that defines them.
///
/// `Host::file_voxel_edit` and `Host::file_damage` exist because nothing on
/// `main` edits a voxel or kills a seat in a hosted match -- combat's craters
/// are S2's and construction's sets are T14's -- and three of this lane's
/// acceptance lines are about what a seat may see of an edit and of an
/// elimination.
#[test]
fn no_wire_method_files_a_voxel_edit_or_a_damage_order() {
    const SEAM: &[(&str, &str)] = &[
        (
            "file_voxel_edit",
            "a host-side test seam, reached by no wire method",
        ),
        (
            "file_damage",
            "a host-side test seam, reached by no wire method",
        ),
    ];
    let findings = scan_except(SEAM, &[HOST_MODULE]);
    assert!(
        findings.is_empty(),
        "the host-side test seam is reachable from a method:\n{}",
        findings.join("\n")
    );

    let host = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(HOST_MODULE);
    let text = std::fs::read_to_string(&host).expect("the host is where it was written");
    let lines = code_lines(&text);
    for (needle, _) in SEAM {
        assert!(
            lines.iter().any(|(_, line)| uses(line, needle)),
            "`{needle}` is guarded everywhere and defined nowhere, so this test guards nothing"
        );
    }
}

/// No handler and no dispatch arm reaches the walled harness's two seams, the
/// phase stepper and the layout seam (S2's plan, E30).
///
/// [`STEPPING`] keeps both names out of every handler module and lets
/// `surface.rs` reach the host's `phased_tick` through `host_mut()`, which a
/// dispatch arm in `surface.rs` could do too. So this scan is stricter: the
/// host's two seams and the surface's entry to the stepper, `phased_step`
/// and `SurfaceTick`, may be named only in `host.rs`, which defines the
/// seams, and in `surface.rs` on the lines of the entry's own definition --
/// never in a dispatch arm. Only `crates/bench` and tests call them.
#[test]
fn no_wire_method_reaches_the_phase_stepper_or_the_layout_seam() {
    const SEAMS: &[(&str, &str)] = &[
        ("phased_tick", "the host's phase stepper"),
        ("seed_layout", "the host's layout seam"),
        (
            "seed_layout_with_settings",
            "the host's layout seam with its mandate settings",
        ),
        ("phased_step", "the surface's entry to the phase stepper"),
        ("SurfaceTick", "the surface's phased tick"),
    ];
    let findings = scan_except(SEAMS, &[HOST_MODULE, HOST_DRIVER_MODULE]);
    assert!(
        findings.is_empty(),
        "a harness seam is reachable from a method:\n{}",
        findings.join("\n")
    );
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let text = std::fs::read_to_string(source.join(HOST_DRIVER_MODULE))
        .expect("the surface is where it always was");
    let stray = stray_seams(&text, SEAMS);
    assert!(
        stray.is_empty(),
        "the surface names a harness seam outside the stepper's entry:\n{}",
        stray.join("\n")
    );

    // S3's plan, E13: a seat cannot reach the layout's mandate settings. The
    // needles match whole words, so the settings' door needs one of its own.
    let call = "self.host_mut()?.seed_layout_with_settings(&layout, &rows);";
    assert!(uses(call, "seed_layout_with_settings"));
    assert!(!uses(call, "seed_layout"));

    // A positive control: the same scan over a planted copy reports a seam
    // named outside the two windows, and nothing inside them.
    let planted = "impl Surface {\n    pub fn phased_step(&mut self) {\n        self.host_mut()?.phased_tick();\n    }\n    fn handler(&mut self) {\n        self.phased_step();\n    }\n}\n#[derive(Debug)]\npub struct SurfaceTick<'a> {\n    surface: &'a mut Surface,\n}\nimpl SurfaceTick<'_> {\n    fn run(&mut self) {\n        self.surface.host_mut()?.phased_tick();\n    }\n}\nfn after() {\n    let _ = SurfaceTick::new();\n}\n";
    let found = stray_seams(planted, SEAMS);
    assert_eq!(
        found.len(),
        2,
        "the planted handler's and the line after the impl's seams are found, and only those:\n{}",
        found.join("\n")
    );
}

/// Every line of `text` (a copy of `surface.rs`) that names one of `seams`
/// outside the stepper's entry -- `pub fn phased_step` to the close of its
/// body -- and outside the phased tick -- the `#[derive(Debug)]` above
/// `pub struct SurfaceTick` to the close of `impl SurfaceTick`.
fn stray_seams(text: &str, seams: &[(&str, &str)]) -> Vec<String> {
    let lines = code_lines(text);
    let start = lines
        .iter()
        .position(|(_, line)| line.trim_start().starts_with("pub fn phased_step"))
        .expect("the surface defines its entry to the phase stepper");
    let entry_end = lines
        .iter()
        .skip(start)
        .position(|(_, line)| line.starts_with("    }"))
        .map(|offset| start + offset)
        .expect("the entry ends");
    let tick_start = lines
        .iter()
        .position(|(_, line)| line.starts_with("pub struct SurfaceTick"))
        .and_then(|at| at.checked_sub(1))
        .filter(|derive| {
            lines
                .get(*derive)
                .is_some_and(|(_, line)| line.trim() == "#[derive(Debug)]")
        })
        .expect("the surface defines its phased tick, derived Debug on the line above");
    let impl_start = lines
        .iter()
        .position(|(_, line)| line.starts_with("impl SurfaceTick"))
        .expect("the phased tick has its impl");
    let tick_end = lines
        .iter()
        .skip(impl_start)
        .position(|(_, line)| line == "}")
        .map(|offset| impl_start + offset)
        .expect("the impl closes");
    let mut stray: Vec<String> = Vec::new();
    for (index, (number, line)) in lines.iter().enumerate() {
        let inside =
            (start..=entry_end).contains(&index) || (tick_start..=tick_end).contains(&index);
        for (needle, why) in seams {
            if uses(line, needle) && !inside {
                stray.push(format!(
                    "{HOST_DRIVER_MODULE}:{number}: `{needle}` ({why})\n    {}",
                    line.trim()
                ));
            }
        }
    }
    stray
}

/// No method handler can file advice, or give a token a rate of its own
/// (decisions-log item 111, decisions C5 and C6).
///
/// `Surface::file_advice` writes a seat's safe playbook and its wizard
/// suggestions into that seat's private store, and
/// `Surface::register_in_process` gives a token its own rate limit and a
/// scratch view feed. Both are **host-side**: the host loop in `serve.rs`
/// calls them for the tokens it minted for itself, and nothing a client sends
/// may reach either. So each name may appear in `surface.rs` only on the line
/// that defines it, in `serve.rs` (the one caller), and nowhere else in the
/// crate -- not in a handler module, not in `surface/control.rs`, not in a
/// dispatch arm. The same holds for `InProcessSeats`, the public type in
/// `serve.rs` that drives both, which may be named nowhere else in the crate.
#[test]
fn no_handler_can_file_advice() {
    const HOST_SIDE: &[(&str, &str)] = &[
        (
            "file_advice",
            "the host files a seat's advice; a handler that could would let a client write a \
             seat's safe playbook",
        ),
        (
            "register_in_process",
            "the host gives its own tokens their own rate; a handler that could would let a \
             client give itself one",
        ),
    ];
    const CALLER: &str = "serve.rs";
    const DEFINER: &str = "surface.rs";
    // Exempted by their path under the crate, not by their bare name, so a
    // `serve.rs` or `surface.rs` added in a handler directory is scanned.
    const CALLER_PATH: &str = "src/serve.rs";
    const DEFINER_PATH: &str = "src/surface.rs";
    // The public driver of both seams: `InProcessSeats::open` registers the
    // in-process tokens and `InProcessSeats::plan` files the advice. A handler
    // that named it would reach both without naming either, so it is as
    // host-side as they are and may be named only where it is defined.
    const DRIVER: &[(&str, &str)] = &[(
        "InProcessSeats",
        "the host loop's driver of both seams; a handler that could name it would reach \
         file_advice and register_in_process through it",
    )];
    let findings = scan_except(HOST_SIDE, &[CALLER_PATH, DEFINER_PATH]);
    assert!(
        findings.is_empty(),
        "a host-side seam is named outside the host loop:\n{}",
        findings.join("\n")
    );

    let findings = scan_except(DRIVER, &[CALLER_PATH]);
    assert!(
        findings.is_empty(),
        "the host loop's in-process driver is named outside the host loop:\n{}",
        findings.join("\n")
    );

    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let surface_text = std::fs::read_to_string(source.join(DEFINER)).expect("the surface");
    let mut definitions = 0_usize;
    let mut reached: Vec<String> = Vec::new();
    for (number, line) in code_lines(&surface_text) {
        for (needle, _) in HOST_SIDE {
            if !uses(&line, needle) {
                continue;
            }
            if line.trim_start().starts_with("pub fn ") {
                definitions = definitions.saturating_add(1);
            } else {
                reached.push(format!("{DEFINER}:{number}: {}", line.trim()));
            }
        }
    }
    assert!(
        reached.is_empty(),
        "{DEFINER} reaches a host-side seam from inside itself, which is where the dispatch \
         arms are:\n{}",
        reached.join("\n")
    );
    assert_eq!(
        definitions,
        HOST_SIDE.len(),
        "each seam is defined once in {DEFINER}; a guard over a name nobody defines guards \
         nothing"
    );

    let caller = std::fs::read_to_string(source.join(CALLER)).expect("the host loop");
    let caller_lines = code_lines(&caller);
    for (needle, _) in HOST_SIDE {
        assert!(
            caller_lines.iter().any(|(_, line)| uses(line, needle)),
            "`{needle}` is never called by {CALLER}, so the exemption is guarding nothing"
        );
    }
}

/// A handler may **compile** a playbook and may not **seal** or step one.
///
/// The two halves of T13b's door. `Plan::compile` is a pure function of the
/// playbook and the rules table — it resolves labels, checks that every wait
/// has a timeout and every jump goes forward, and prices nothing against the
/// world — so `submit_plan` may run it while the caller is still on the line
/// and report its refusal as a method error. Sealing the result into the match
/// is the host's, in `host.rs`, and this test is what says so about the files
/// where the handlers actually live.
///
/// Both halves are asserted, because each without the other passes vacuously: a
/// handler module that named neither would satisfy the ban while the door it is
/// about sat somewhere else entirely.
#[test]
fn a_handler_may_compile_a_playbook_and_may_never_seal_or_step_one() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut findings: Vec<String> = Vec::new();
    let mut compiles = 0_usize;
    for module in HANDLER_MODULES {
        let path = source.join("surface").join(module);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{module} is where the handlers live"));
        let lines = code_lines(&text);
        assert!(
            lines.len() > 50,
            "{module} read as {} lines of code; the scan is looking at the wrong file",
            lines.len()
        );
        for (number, line) in lines {
            if line.contains("Plan::compile") {
                compiles = compiles.saturating_add(1);
            }
            for (needle, why) in STEPPING {
                if uses(&line, needle) {
                    findings.push(format!(
                        "{module}:{number}: `{needle}` -- {why}\n    {}",
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(
        findings.is_empty(),
        "a method handler reaches the match itself:\n{}",
        findings.join("\n")
    );
    assert_eq!(
        compiles, 1,
        "`Plan::compile` is called once, in `surface/planning.rs`'s `compile_decoded`, which \
         `compile_canonical` and `resolve_refs` share: the one door a playbook goes through into \
         the sim. If this is 0 the ban above is guarding an empty room; if it is more than 1 \
         there are two doors and they can drift"
    );
}

/// AGENTS.md section 7: "No filesystem or network access through playbooks."
///
/// The source half. `std::fs` lives in `src/cache.rs`, which is the private
/// match cache, and in no other module -- so every path this crate opens is
/// built from a validated match id and a fixed file name, and there is no second
/// place for a caller-supplied string to become one.
#[test]
fn the_filesystem_lives_in_one_module() {
    let mut elsewhere: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let is_cache = path.ends_with("cache.rs");
        for (number, line) in code_lines(&text) {
            let touches_filesystem = line.contains("std::fs")
                || line.contains("fs::")
                || line.contains("File::")
                || line.contains("OpenOptions");
            if touches_filesystem && !is_cache {
                elsewhere.push(format!("{}:{number}: {}", path.display(), line.trim()));
            }
        }
    }
    assert!(
        elsewhere.is_empty(),
        "the gateway touches the filesystem outside src/cache.rs:\n{}",
        elsewhere.join("\n")
    );

    let cache = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("cache.rs"),
    )
    .expect("cache.rs");
    assert!(
        cache.contains("use std::fs;"),
        "cache.rs is the module this test is about; if it stopped using the filesystem the \
         test above would pass vacuously"
    );
}

/// AGENTS.md section 7, the schema half: "A playbook is data with a closed
/// vocabulary. There is no verb that reads a path, opens a socket, shells out or
/// loads code, and none may be added."
///
/// Asserted over the checked-in descriptor set rather than over this crate,
/// because that is where the claim actually lives: if `gp.v1` ever grew a
/// `file`, a `url` or a `command` field, the gateway would be handing it to
/// something, and no amount of care in this crate would help.
#[test]
fn nothing_a_playbook_can_contain_names_a_path_a_url_or_a_socket() {
    // `Diagnostic.path` and `Diagnostic.related_paths` are RFC 6901 JSON
    // Pointers into a playbook -- `/declarative/route/0/move` -- and not
    // filesystem paths. The name is the verifier's and the whole project's, and
    // the field carries a pointer in every one of the 48 committed verifier
    // report goldens. Named here so the rule can be checked without the one
    // exception quietly widening it.
    const POINTERS_NOT_PATHS: &[&str] = &[
        "gp.api.v1.Diagnostic.path",
        "gp.api.v1.Diagnostic.related_paths",
    ];
    const FORBIDDEN: &[&str] = &[
        "file",
        "filename",
        "filepath",
        "dir",
        "directory",
        "url",
        "uri",
        "host",
        "hostname",
        "port",
        "socket",
        "address",
        "endpoint",
        "command",
        "cmd",
        "exec",
        "shell",
        "script_path",
        "load",
        "import",
        "include",
        "require",
        "module",
    ];

    let mut findings: Vec<String> = Vec::new();
    let mut fields = 0_usize;
    for message in schema().messages() {
        if !message.full_name.starts_with("gp.") {
            continue;
        }
        for field in &message.fields {
            fields = fields.saturating_add(1);
            let full = format!("{}.{}", message.full_name, field.name);
            if POINTERS_NOT_PATHS.contains(&full.as_str()) {
                continue;
            }
            let name = field.name.to_ascii_lowercase();
            for forbidden in FORBIDDEN {
                let matches_word = name == *forbidden
                    || name.starts_with(&format!("{forbidden}_"))
                    || name.ends_with(&format!("_{forbidden}"))
                    || name.contains(&format!("_{forbidden}_"));
                if matches_word {
                    findings.push(format!(
                        "{full} names `{forbidden}`: a playbook is data with a closed \
                         vocabulary and nothing in it reaches the filesystem or a socket \
                         (AGENTS.md section 7)"
                    ));
                }
            }
        }
    }
    assert!(
        fields > 100,
        "the schema walk found only {fields} fields and would pass vacuously"
    );
    assert!(findings.is_empty(), "{}", findings.join("\n"));
}

/// AGENTS.md section 5: an `#[allow]` that defeats a determinism lint is a
/// contract change wearing a disguise.
#[test]
fn no_allow_defeats_a_determinism_lint() {
    let mut findings: Vec<String> = Vec::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for named in allows(&text) {
            if !PERMITTED_ALLOWS.contains(&named.as_str()) {
                findings.push(format!("{}: #[allow({named})]", path.display()));
            }
        }
    }
    assert!(
        findings.is_empty(),
        "an #[allow] this crate has not accounted for. If it is a determinism lint the answer          is not an allow (AGENTS.md section 5); if it is not, add it to PERMITTED_ALLOWS with          the reason:
{}",
        findings.join("
")
    );
    // And the scan can see one: the permitted allow is in the source right now.
    let surface = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("surface.rs"),
    )
    .expect("surface.rs");
    assert_eq!(
        allows(&surface),
        vec![
            String::from("clippy::struct_field_names"),
            String::from("clippy::unused_self"),
        ],
        "if this list is empty the test above passes vacuously"
    );
}

/// Every lint named by an `#[allow]` or `#![allow]` in `text`, attribute
/// bodies spanning several lines included -- rustfmt writes one that way as soon
/// as it carries a `reason`, and a line-at-a-time scan would miss exactly those.
///
/// Two things this has to get right, because both are how the hole AGENTS.md
/// section 4.9 names would actually be dug:
///
/// * **every** lint of a multi-lint allow, not just the first. A determinism
///   lint sitting second in `#[allow(clippy::struct_field_names,
///   clippy::float_arithmetic)]` would otherwise pass a scan that only read the
///   first name, while silencing the lint for clippy all the same;
/// * both spellings in one pass. `#![allow(` does not contain `#[allow(`, so
///   looking for the second and only falling back to the first would skip an
///   inner attribute that sits earlier in the file -- and an inner attribute is
///   precisely the crate- or module-local allowance section 4.9 forbids.
fn allows(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut rest = text;
    loop {
        let outer = rest.find("#[allow(");
        let inner_attribute = rest.find("#![allow(");
        let index = match (outer, inner_attribute) {
            (Some(left), Some(right)) => left.min(right),
            (Some(only), None) | (None, Some(only)) => only,
            (None, None) => break,
        };
        let after = rest.get(index..).unwrap_or_default();
        let body_at = after.find('(').map_or(0, |at| at.saturating_add(1));
        let body = after.get(body_at..).unwrap_or_default();
        let end = body.find(")]").unwrap_or(body.len());
        let inner = body.get(..end).unwrap_or_default();
        for piece in inner.split(',') {
            let name = piece.split_whitespace().collect::<String>();
            if name.is_empty() {
                continue;
            }
            // `reason = "..."` ends the lint list; its own text may hold commas.
            if name.starts_with("reason=") {
                break;
            }
            found.push(name);
        }
        rest = body.get(end..).unwrap_or_default();
    }
    found
}

/// The scan above is the test's eyes, so it gets a test of its own: a fixture
/// holding both of the shapes that would otherwise slip past it.
#[test]
fn the_allow_scan_sees_every_lint_and_both_spellings() {
    let fixture = "#![allow(clippy::float_arithmetic)]\n\
                   fn f() {}\n\
                   #[allow(\n    clippy::struct_field_names,\n    clippy::as_conversions,\n    \
                   reason = \"a reason, with a comma in it\"\n)]\n\
                   struct S;\n";
    assert_eq!(
        allows(fixture),
        vec![
            String::from("clippy::float_arithmetic"),
            String::from("clippy::struct_field_names"),
            String::from("clippy::as_conversions"),
        ],
        "an inner attribute before the first outer one, and every lint of a multi-lint allow"
    );
}

/// The manifest half of the research ban, which `cargo xtask ci`'s
/// `research-guard` also proves from the dependency graph.
#[test]
fn the_manifest_declares_no_feature_and_takes_the_sim_without_default_features() {
    let manifest =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("the crate's own manifest");
    assert!(
        !manifest.lines().any(|line| line.trim() == "[features]"),
        "the gateway declares no features: only crates/sim defines `research`"
    );
    assert!(
        manifest.contains("pharmakos-sim = { workspace = true, default-features = false }"),
        "the sim is taken for its type surface only"
    );
    assert!(
        !manifest.contains("pharmakos-mesher"),
        "the mesher is a walled crate and `cargo xtask wall-guard` fails over this edge"
    );
    // Item 99's one off-list dependency, and the only one.
    assert!(
        manifest.contains("getrandom = { workspace = true }"),
        "{manifest}"
    );
    for banned in [
        "tungstenite",
        "tokio",
        "serde_json",
        "rand",
        "sha1",
        "base64",
        "subtle",
    ] {
        assert!(
            !manifest.contains(&format!("\n{banned} ")),
            "`{banned}` is not on the approved list (AGENTS.md section 3 rule 5)"
        );
    }
}

/// No method handler reads the save or the private replay, or reaches the
/// host-side calls that write them (T17; the wave-6 notes, decision C11).
///
/// Spec section 12: the private replay cache never leaves the gateway for
/// another seat. On a local host that is enforcement, not cryptography, and
/// the enforcement is here: no wire method returns a save or a replay in v1,
/// and this test holds that no handler module -- nor `surface/control.rs` --
/// names a single one of the cache's readers or writers, the surface's
/// host-side save calls, the save format's reader, or a resume. The names are
/// allowed where they are defined and in `serve.rs`, the host loop that calls
/// them.
/// The surface's host-side save seams (`no_handler_reads_the_save_or_the_replay`).
const SURFACE_SIDE: &[(&str, &str)] = &[
    (
        "take_persistence",
        "what a Push's beginning and a segment's end leave for the disk is the host loop's",
    ),
    (
        "lull_save",
        "a Lull is saved by the host loop, on end of file",
    ),
    // Since S3's `gwu` the surface's resume is a method, reached as
    // `.resume(` (the entry below, which also covers `Host::resume`); this
    // one still blocks the path form, `Surface::resume(..)` or a
    // `Surface::resume` handed around as a function value.
    (
        "Surface::resume",
        "a surface is built over a save by the host loop's resume, and by nothing else",
    ),
    (".resume(", "a restore belongs to the match host"),
    // Lane `opsave`: the last Push's feed and recap are put back by the
    // one restore, `Surface::resume`, and by nothing a seat can reach.
    (
        "restore_last_push",
        "the last Push is put back by the surface's resume, and by nothing else",
    ),
    // Its capture reads the whole bus, every seat's lines: what a save is
    // built from (`saved_match`), never what a handler answers.
    (
        ".last_push(",
        "the last Push is captured for a save, and by nothing a seat can reach",
    ),
];

#[test]
fn no_handler_reads_the_save_or_the_replay() {
    const CACHE_SIDE: &[(&str, &str)] = &[
        (
            "read_save",
            "the save is read by a resume, in the host loop, and by nothing else",
        ),
        (
            "reopen",
            "a saved match is reopened by a resume, in the host loop",
        ),
        ("write_save", "a save is written by the host loop"),
        (
            "write_sealed",
            "the replay's sealed playbooks are written by the host loop",
        ),
        (
            "write_replay",
            "the replay's hash chains are written by the host loop",
        ),
        (
            "audit_text",
            "the audit log is read back by a host, never by a seat",
        ),
        (
            "Save::parse",
            "the save format is read by a resume, in the host loop",
        ),
    ];
    // Exempted by their path under the crate, as `no_handler_can_file_advice`
    // does, so a file of the same name in a handler directory is scanned.
    const DEFINERS_AND_CALLER: &[&str] = &[
        "src/cache.rs",
        "src/save.rs",
        "src/serve.rs",
        "src/surface.rs",
        "src/surface/last_push.rs",
        "src/host.rs",
    ];
    let mut needles: Vec<(&str, &str)> = CACHE_SIDE.to_vec();
    needles.extend_from_slice(SURFACE_SIDE);
    let findings = scan_except(&needles, DEFINERS_AND_CALLER);
    assert!(
        findings.is_empty(),
        "a save or a replay is reachable from outside the host loop:\n{}",
        findings.join("\n")
    );

    // The surface, where every dispatch arm lives, reaches the cache not at
    // all: it builds what is to be written and the host loop writes it.
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let surface_text = std::fs::read_to_string(source.join("surface.rs")).expect("the surface");
    let mut reached: Vec<String> = Vec::new();
    for (number, line) in code_lines(&surface_text) {
        for (needle, why) in CACHE_SIDE {
            if uses(&line, needle) {
                reached.push(format!("surface.rs:{number}: `{needle}` -- {why}"));
            }
        }
    }
    assert!(
        reached.is_empty(),
        "the surface reaches the cache itself:\n{}",
        reached.join("\n")
    );

    surface_calls_no_save_seam(&surface_text, SURFACE_SIDE);

    // And the guard is over something: the host loop does call them.
    let serve = std::fs::read_to_string(source.join("serve.rs")).expect("the host loop");
    let serve_lines = code_lines(&serve);
    for needle in [
        "read_save",
        "reopen",
        "write_save",
        "write_sealed",
        "write_replay",
        "take_persistence",
        "lull_save",
        "Save::parse",
        ".resume(",
    ] {
        assert!(
            serve_lines.iter().any(|(_, line)| uses(line, needle)),
            "`{needle}` is never called by serve.rs, so the exemption is guarding nothing"
        );
    }
    serve_resumes_with_its_preset_first(&serve_lines);
}

/// The `.resume(` the host loop calls is the surface's, built with its preset
/// first (S3's `gwu`): one statement takes a surface through `.with_preset(`
/// and then `.resume(host`, so a host loop that restored the host alone, or
/// set the preset after the save went in, fails here.
fn serve_resumes_with_its_preset_first(serve_lines: &[(usize, String)]) {
    let statements: Vec<String> = serve_lines
        .iter()
        .map(|(_, line)| line.trim().to_owned())
        .collect::<Vec<String>>()
        .join(" ")
        .split(';')
        .map(str::to_owned)
        .collect();
    assert!(
        statements.iter().any(|statement| {
            statement
                .find(".with_preset(")
                .and_then(|preset| statement.get(preset..))
                .is_some_and(|rest| rest.contains(".resume(host"))
        }),
        "serve.rs no longer resumes a save into a surface whose preset is set first"
    );
}

/// Inside surface.rs, where every dispatch arm lives, the surface's host-side
/// save seams are named only where they are defined, and the host's restore
/// only inside `Surface::resume` itself (`no_handler_reads_the_save_or_the_replay`).
fn surface_calls_no_save_seam(surface_text: &str, needles: &[(&str, &str)]) {
    // The surface's own host-side save calls are exempt from the crate scan
    // only because they are *defined* there, and `dispatch` -- every wire
    // method's arm -- lives in the same file. So inside surface.rs a
    // SURFACE_SIDE name may appear on its own definition line and, for the
    // host's restore, inside `Surface::resume` itself, and nowhere else: not
    // in a dispatch arm, not in a handler method the arm calls.
    let mut method = String::new();
    let mut defined = 0_usize;
    let mut called: Vec<String> = Vec::new();
    for (number, line) in code_lines(surface_text) {
        let definition = method_defined_on(&line);
        if let Some(name) = definition.clone() {
            if ["resume", "take_persistence", "lull_save"].contains(&name.as_str()) {
                defined = defined.saturating_add(1);
            }
            method = name;
        }
        for (needle, why) in needles {
            if !uses(&line, needle) || definition.is_some() {
                continue;
            }
            // The save's own builder captures the last Push for it.
            let builds_the_save = *needle == ".last_push(" && method == "saved_match";
            if method != "resume" && !builds_the_save {
                called.push(format!(
                    "surface.rs:{number} (in `{method}`): `{needle}` -- {why}\n    {}",
                    line.trim()
                ));
            }
        }
    }
    assert!(
        called.is_empty(),
        "the surface calls a host-side save seam from inside itself, which is where the \
         dispatch arms are:\n{}",
        called.join("\n")
    );
    assert_eq!(
        defined, 3,
        "`resume`, `take_persistence` and `lull_save` are each defined once in surface.rs; a \
         guard over a name nobody defines guards nothing"
    );
}

// ---------------------------------------------------------------------------
// No silent fallback (S2's plan, E7: each crate's next S2 owner sweeps its own)
// ---------------------------------------------------------------------------

/// Every fallback call left in the crate's shipped code, each with the rule
/// that makes its default the exact answer rather than a guess: `(file name,
/// the code, why)`.
///
/// S2's plan counted 66 `try_from(..).unwrap_or(..)` and `.get(..)…unwrap_or(..)`
/// sites here at `7a351e7`; S2's `wgw` swept them with the wider net below
/// (every `unwrap_or`, `map_or` and `map_or_else`, 144 lines at `cb2da2e`),
/// widened on #128's reviews to [`FALLBACK_NEEDLES`] and `let Ok(..) else`.
/// Each one that hid a failure now returns a typed error, or is rewritten so
/// the failure cannot be written; what is left is a default that **is** the
/// answer -- proto3's default for an absent parameter, the log formats' `-`
/// for "none", nothing to see where no match is hosted -- and says so here.
const KEPT_FALLBACKS: &[(&str, &str, &str)] = &[
    (
        "surface.rs",
        ".map_or_else(LiveVision::default, |host| LiveVision::of(host.world()));",
        "with no match hosted there is no sensor at all, so nobody witnesses an event: the vision is blind, as `serve::vision_of`'s is",
    ),
    (
        "viewfeed.rs",
        ".unwrap_or_default()",
        "a viewer the view has never named a beacon for holds no handles, and a save keeps an empty table for it",
    ),
    (
        "audit.rs",
        ".map_or_else(|| String::from(\"-\"), Subject::render);",
        "the audit log's format spells a line with no subject `-` (its reader, `MatchCache::continuation`, reads it so)",
    ),
    (
        "audit.rs",
        ".map_or_else(|| String::from(\"-\"), |handle| handle.to_string());",
        "the audit log's format spells a line with no token handle `-`",
    ),
    (
        "save.rs",
        "self.config.human_seat.map_or(Json::Null, number),",
        "the save container writes a match with no human seat as `null`, which its reader reads back as none",
    ),
    (
        "save.rs",
        "|seat: Option<u8>| seat.map_or_else(|| String::from(\"-\"), |raw| raw.to_string());",
        "a refusal's message names an absent human seat `-`, as the config line spells it",
    ),
    (
        "save.rs",
        "let sealed = seat.seal.as_ref().map_or(Json::Null, |held| {",
        "the save container writes a seat that sealed nothing as `null`",
    ),
    (
        "serve.rs",
        ".map_or_else(|| String::from(\"-\"), |seat| seat.to_string());",
        "the config line spells no human seat `-` (`ConfigLine`'s format)",
    ),
    (
        "serve.rs",
        "let eighth = trimmed.split('\\t').nth(7).unwrap_or(\"\");",
        "a config line of seven fields is a new match: the eighth is absent, read as the empty field the format allows",
    ),
    (
        "serve.rs",
        "let seat = self.seat_token.clone().unwrap_or_else(|| String::from(\"-\"));",
        "the announce line spells a match with no human seat's token `-`, which `Announce::parse` reads back as none",
    ),
    (
        "serve.rs",
        "method.map_or_else(",
        "a refusal names a method the wire does not know `<unknown>`, rather than echoing an unbounded name",
    ),
    (
        "serve.rs",
        "surface.host().map_or_else(",
        "with no match hosted there is no sensor, so the vision is blind: nothing is seen where nothing stands",
    ),
    (
        "sha1.rs",
        "schedule.get(index).copied().unwrap_or(0)",
        "every index the compression reads is under 80 by its loop bounds; the hash is checked against RFC 3174's vectors",
    ),
    (
        "strings.rs",
        "let seat = event.seat.map_or_else(",
        "an event with no seat is about an asset nobody owns, and the sentence says so",
    ),
    (
        "strings.rs",
        "EventKind::MatchEnded => event.seat.map_or_else(",
        "a match that ended with no single winner names none (a shared win, or no seat left)",
    ),
    (
        "surface.rs",
        ".map_or(self.time.tick, |host| host.runner().tick());",
        "with no match hosted the feed's anchor is the gateway's own tick, as nothing has been stamped",
    ),
    (
        "surface.rs",
        "self.host.as_ref().map_or(Ms::ZERO, |host| {",
        "with no match hosted no segment has opened, so none of it has elapsed",
    ),
    (
        "surface.rs",
        "let last = self.reported_elapsed_ms.unwrap_or(Ms::ZERO);",
        "a phase whose clock nobody has reported yet is at its start",
    ),
    (
        "surface.rs",
        "let ticks = self.host.as_ref().map_or(0, |host| {",
        "with no match hosted a segment has no ticks to reserve room for",
    ),
    (
        "surface.rs",
        ".reserve(usize::try_from(ticks).unwrap_or(0).saturating_add(1));",
        "a capacity hint: reserving less only means the chain grows by allocating, and its contents are the same",
    ),
    (
        "surface.rs",
        ".map_or(self.time.round, |host| host.runner().round());",
        "with no match hosted the round is the gateway's own",
    ),
    (
        "surface.rs",
        "reported.unwrap_or(Ms::ZERO)",
        "a Lull no host has put a countdown on is untimed, and the footer's `untimed` says that this 0 is no countdown (S1-11)",
    ),
    (
        "surface.rs",
        ".map_or(self.limits, |treatment| treatment.limits);",
        "a socket seat is rate limited by the match's limits; only an in-process seat has its own",
    ),
    (
        "surface.rs",
        "let cursor = request.string_param(\"cursor\")?.unwrap_or_default();",
        "proto3's default: an absent cursor is the empty one, which waits from now",
    ),
    (
        "surface.rs",
        "let ready = request.bool_param(\"ready\")?.unwrap_or(true);",
        "`set_ready`'s documented default: a seat that says nothing says it is ready",
    ),
    (
        "surface.rs",
        "scopes::method_from_wire(method).map_or_else(",
        "the audit log names a method the wire does not know `call <unknown>`, never an unbounded name",
    ),
    (
        "planning.rs",
        "let part = request.string_param(\"part\")?.unwrap_or_default();",
        "proto3's default: an absent part is the empty one, the whole playbook schema",
    ),
    (
        "planning.rs",
        "let tag = request.string_param(\"tag\")?.unwrap_or_default();",
        "proto3's default: an absent tag is the empty one, every template",
    ),
    (
        "planning.rs",
        "let suggested = request.bool_param(\"suggested\")?.unwrap_or(false);",
        "proto3's default: an absent `suggested` is false, the template's own defaults",
    ),
    (
        "planning.rs",
        "suggestion.map_or_else(Vec::new, |suggestion| {",
        "with no suggestion there are no suggested parameters, and the template's own defaults stand",
    ),
    (
        "planning.rs",
        "let why = suggestion.map_or_else(String::new, |suggestion| suggestion.why.clone());",
        "with no suggestion there is no reason for one, which proto3 writes empty",
    ),
    (
        "feed.rs",
        "at.raw().max(0).div_euclid(DIGEST_PERIOD.raw())",
        "the feed stamps events from the segment's own epoch, so a time before it is the first window's",
    ),
    (
        "feed.rs",
        "let seconds = at.raw().max(0).div_euclid(1000);",
        "a digest's span starts at the segment's epoch, and no window starts before it",
    ),
    (
        "save.rs",
        "out.push(u8::from_str_radix(pair, 16).ok()?);",
        "`bytes_of_hex` answers `Option` and its caller refuses `None` as a damaged save with a typed error",
    ),
    (
        "scopes.rs",
        "let number = u32::try_from(i32::from(scope)).ok()?;",
        "a generated scope's number is never negative, and `None` is `bit`'s documented \"not a scope\"",
    ),
    (
        "session.rs",
        "let Ok(grant) = self.surface.tokens().authenticate(token, &match_id, tick) else {",
        "a token that does not authenticate opens no session: `false` is the refusal the upgrade answers",
    ),
    (
        "strings.rs",
        "match u8::try_from(value).ok().and_then(posture_words) {",
        "a posture this build does not name is written as its code, not read as one it does",
    ),
    (
        "strings.rs",
        "let seconds = ms.raw().max(0).div_euclid(1000);",
        "a clock renders a duration, and the one negative game time is a segment's remaining time past its end, which is none",
    ),
    (
        "surface.rs",
        "Ms::new(lull_ms.saturating_sub(left.raw()).max(0)).to_ticks_floor()",
        "a client reporting more left than the Lull's length has spent none of it",
    ),
    (
        "targeting.rs",
        "let index = world.units().row_of(commander).ok()?.index();",
        "a commander whose row has gone stands nowhere, which `commander_point` answers as `None`, its documented dead",
    ),
    (
        "targeting.rs",
        "crate::surface::plan_words::beacon_spec(reference, reader.world.rules()).ok()?",
        "a beacon reference the sim's compile refuses places nothing, and `compile_canonical` says why at the door",
    ),
    (
        "targeting.rs",
        "let spec = crate::surface::plan_words::beacon_spec(reference?, reader.world.rules()).ok()?;",
        "as the placement above: a refused reference visits nothing, and the compile door names it",
    ),
    (
        "targeting.rs",
        "let row = world.beacons().row_of(id).ok()?.index();",
        "an id whose row was freed or reused names no place (S2's plan, E1)",
    ),
    (
        "planning.rs",
        ".map_or(\"feature\", pharmakos_sim::features::FeatureKind::name);",
        "a `covered {}` reference asks for no kind, so its sentence names a feature",
    ),
];

/// The needles a fallback is written with: the calls that answer a default
/// for a failure (`unwrap_or`, and so `unwrap_or_default` and
/// `unwrap_or_else`; `map_or`; `map_or_else`), and the forms that turn a
/// failure into one quietly -- an error read as nothing (`.ok()?`,
/// `.ok().and_then(`), a value pushed into range (`.clamp(`, `.max(0)`), and
/// `let Ok(..) else` answering with a value rather than an error.
const FALLBACK_NEEDLES: &[&str] = &[
    "unwrap_or",
    "map_or(",
    "map_or_else(",
    ".ok()?",
    ".ok().and_then(",
    ".clamp(",
    ".max(0)",
];

/// The fallback lines of one file's shipped code, trimmed: every line holding
/// a needle, and every `let Ok(..) .. else {` whose body returns anything but
/// an `Err` (that line is the one reported).
fn fallback_lines(text: &str) -> Vec<String> {
    let lines = code_lines(text);
    let mut found: Vec<String> = Vec::new();
    for (index, (_, line)) in lines.iter().enumerate() {
        if FALLBACK_NEEDLES.iter().any(|needle| line.contains(needle)) {
            found.push(line.trim().to_owned());
            continue;
        }
        if line.contains("let Ok(") && line.trim_end().ends_with("else {") {
            let body = lines
                .get(index.saturating_add(1))
                .map(|(_, next)| next.trim())
                .unwrap_or_default();
            if body.starts_with("return") && !body.starts_with("return Err(") {
                found.push(line.trim().to_owned());
            }
        }
    }
    found
}

/// Every fallback in the crate's shipped code, as `(file name, the line)`.
fn fallbacks() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for path in sources() {
        let text = std::fs::read_to_string(&path).expect("a source file reads");
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .expect("a source file has a name");
        for line in fallback_lines(&text) {
            found.push((name.clone(), line));
        }
    }
    found
}

/// The scan's positive control: planted text holding one of each form is
/// found, a typed `let Ok(..) else { return Err(..) }` and comment prose are
/// not, and nothing after `#[cfg(test)]` is read.
#[test]
fn the_fallback_scan_sees_every_form_it_names() {
    let planted = "fn a() {\n    let x = y.unwrap_or(0);\n    let x = y.map_or(0, f);\n    \
                   let x = y.map_or_else(g, f);\n    let x = y.ok()?;\n    let x = \
                   y.ok().and_then(f);\n    let x = y.clamp(1, 9);\n    let x = y.max(0);\n    \
                   let Ok(x) = y else {\n        return 0;\n    };\n    let Ok(x) = y else {\n        \
                   return Err(e);\n    };\n    // y.unwrap_or(0) in prose\n}\n#[cfg(test)]\n\
                   fn b() { y.unwrap_or(0); }\n";
    let found = fallback_lines(planted);
    assert_eq!(
        found,
        [
            "let x = y.unwrap_or(0);",
            "let x = y.map_or(0, f);",
            "let x = y.map_or_else(g, f);",
            "let x = y.ok()?;",
            "let x = y.ok().and_then(f);",
            "let x = y.clamp(1, 9);",
            "let x = y.max(0);",
            "let Ok(x) = y else {",
        ],
        "every form the scan names, once, and nothing else"
    );
}

/// No value in the crate's shipped code falls back to a default instead of
/// saying what went wrong (S2's plan, E7, and the global rule: never a silent
/// fallback), except the defaults [`KEPT_FALLBACKS`] lists with the rule that
/// makes each one the exact answer. A new fallback fails here until it is a
/// typed error or takes its line and its reason in the list.
#[test]
fn no_silent_fallback_is_left() {
    let found = fallbacks();
    let unexplained: Vec<String> = found
        .iter()
        .filter(|(file, line)| {
            !KEPT_FALLBACKS
                .iter()
                .any(|(kept_file, code, _)| kept_file == file && line.contains(code))
        })
        .map(|(file, line)| format!("{file}: {line}"))
        .collect();
    assert!(
        unexplained.is_empty(),
        "a value falls back to a default instead of saying what went wrong. Return a typed \
         error, make the failure unrepresentable, or, where the default is a rule that leaves \
         the answer exact, add the line to KEPT_FALLBACKS with that rule:\n{}",
        unexplained.join("\n")
    );
    for (file, code, why) in KEPT_FALLBACKS {
        let count = found
            .iter()
            .filter(|(found_file, line)| found_file == file && line.contains(code))
            .count();
        assert_eq!(
            count, 1,
            "KEPT_FALLBACKS' entry in {file} (`{code}`, kept because {why}) matches {count} \
             lines; an entry that matches nothing is stale, and one that matches two hides one"
        );
        assert!(!why.is_empty(), "{file}: a kept fallback names its rule");
    }
}

/// The scan must see the kept fallbacks, or every assertion above passes over
/// nothing.
#[test]
fn the_fallback_scan_finds_what_it_looks_for() {
    let found = fallbacks();
    assert_eq!(
        found.len(),
        KEPT_FALLBACKS.len(),
        "the fallback scan sees {} lines and the list keeps {}",
        found.len(),
        KEPT_FALLBACKS.len()
    );
    assert!(
        found.iter().any(|(file, _)| file == "sha1.rs"),
        "the fallback scan no longer sees sha1.rs' kept read"
    );
}

#[test]
fn no_shipped_code_calls_a_sim_fixture() {
    let findings = scan(SIM_FIXTURES);
    assert!(
        findings.is_empty(),
        "{}",
        findings.join(
            "
"
        )
    );
}

// ---------------------------------------------------------------------------
// One space between words (S3's `gwr` review: a lost `\` continuation)
// ---------------------------------------------------------------------------

/// Every `.rs` file under a sibling crate's `src`, in path order, as
/// [`sources`] lists this crate's.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml sanctions read_dir when the listing is collected and sorted before use, which is what happens here"
)]
fn sources_of(krate: &str) -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(krate)
        .join("src");
    let mut stack: Vec<PathBuf> = vec![root];
    let mut found: Vec<PathBuf> = Vec::new();
    while let Some(directory) = stack.pop() {
        let entries = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("{}: {error}", directory.display()));
        let mut here: Vec<PathBuf> = entries
            .map(|entry| {
                entry
                    .unwrap_or_else(|error| panic!("an entry: {error}"))
                    .path()
            })
            .collect();
        here.sort();
        for path in here {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// A reader over a file's characters, for [`string_literals`].
struct Chars {
    chars: Vec<char>,
}

impl Chars {
    fn at(&self, index: usize) -> Option<char> {
        self.chars.get(index).copied()
    }

    /// The index past a `//` comment starting at `index`.
    fn past_line_comment(&self, mut index: usize) -> usize {
        while self.at(index).is_some_and(|c| c != '\n') {
            index += 1;
        }
        index
    }

    /// The index past a (nested) block comment starting at `index`.
    fn past_block_comment(&self, mut index: usize) -> usize {
        let mut depth = 0_usize;
        while let Some(c) = self.at(index) {
            if c == '/' && self.at(index + 1) == Some('*') {
                depth += 1;
                index += 2;
            } else if c == '*' && self.at(index + 1) == Some('/') {
                depth -= 1;
                index += 2;
                if depth == 0 {
                    break;
                }
            } else {
                index += 1;
            }
        }
        index
    }

    /// The index past a character literal (`'"'`, `'\''`) or a lifetime
    /// (`'a`) starting at `index`.
    fn past_quote(&self, mut index: usize) -> usize {
        if self.at(index + 1) == Some('\\') {
            index += 2;
            while self.at(index).is_some_and(|c| c != '\'') {
                index += 1;
            }
            index + 1
        } else if self.at(index + 2) == Some('\'') {
            index + 3
        } else {
            index + 1
        }
    }

    /// A raw string starting at the `r` at `index`, read as written, and the
    /// index past it; `None` when the `r` starts no raw string.
    fn raw_string(&self, index: usize) -> Option<(String, usize)> {
        let mut hashes = 0;
        let mut cursor = index + 1;
        while self.at(cursor) == Some('#') {
            hashes += 1;
            cursor += 1;
        }
        if self.at(cursor) != Some('"') {
            return None;
        }
        cursor += 1;
        let mut literal = String::new();
        while let Some(c) = self.at(cursor) {
            if c == '"' && (1..=hashes).all(|offset| self.at(cursor + offset) == Some('#')) {
                return Some((literal, cursor + 1 + hashes));
            }
            literal.push(c);
            cursor += 1;
        }
        Some((literal, cursor))
    }

    /// A string starting at the `"` at `index`, as the compiler reads it, and
    /// the index past it.
    fn string(&self, index: usize) -> (String, usize) {
        let mut cursor = index + 1;
        let mut literal = String::new();
        while let Some(c) = self.at(cursor) {
            match c {
                '"' => return (literal, cursor + 1),
                '\\' if self.at(cursor + 1) == Some('\n')
                    || (self.at(cursor + 1) == Some('\r') && self.at(cursor + 2) == Some('\n')) =>
                {
                    cursor += 1;
                    while self.at(cursor).is_some_and(char::is_whitespace) {
                        cursor += 1;
                    }
                }
                '\\' => {
                    let kind = self.at(cursor + 1);
                    literal.push(match kind {
                        Some('n') => '\n',
                        Some('r') => '\r',
                        Some('t') => '\t',
                        _ => '\u{fffd}',
                    });
                    cursor += 2;
                    if kind == Some('u') {
                        while self.at(cursor).is_some_and(|c| c != '}') {
                            cursor += 1;
                        }
                        cursor += 1;
                    } else if kind == Some('x') {
                        cursor += 2;
                    }
                }
                other => {
                    literal.push(other);
                    cursor += 1;
                }
            }
        }
        (literal, cursor)
    }
}

/// The string literals of `text`'s shipped code (less every item a real
/// `#[cfg(test)]` stands on, [`shipped_text`]), each as the compiler reads
/// it: a `\` at a line's end
/// swallows the newline and the next line's leading white space, `\n`, `\r`
/// and `\t` read as themselves, and every other escape as one character.
/// Comments, character literals and lifetimes are skipped; a raw string is
/// read as written.
fn string_literals(text: &str) -> Vec<String> {
    let shipped = shipped_text(text);
    let reader = Chars {
        chars: shipped.chars().collect(),
    };
    let mut found = Vec::new();
    let mut index = 0;
    while let Some(here) = reader.at(index) {
        let after_word = index > 0
            && reader
                .at(index - 1)
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        index = match (here, reader.at(index + 1)) {
            ('/', Some('/')) => reader.past_line_comment(index),
            ('/', Some('*')) => reader.past_block_comment(index),
            ('\'', _) => reader.past_quote(index),
            ('r', Some('"' | '#')) if !after_word => match reader.raw_string(index) {
                Some((literal, next)) => {
                    found.push(literal);
                    next
                }
                None => index + 1,
            },
            ('"', _) => {
                let (literal, next) = reader.string(index);
                found.push(literal);
                next
            }
            _ => index + 1,
        };
    }
    found
}

/// Whether `literal` holds two spaces in a row after a word: indentation
/// (a run at the start of the text or of one of its lines, as the canonical
/// JSON writer and the safe playbook's text indent) is not one.
fn inner_run(literal: &str) -> bool {
    literal.lines().any(|line| line.trim_start().contains("  "))
}

/// S3's `gwr` review: no string the gateway or plan-core ships -- a string
/// table entry, an error's message, a sentence's piece -- holds two spaces in
/// a row after a word, the mark of a `\` continuation lost in an edit (the
/// first-licence memo and a licence error once read "at      settlement").
#[test]
fn no_shipped_string_holds_a_run_of_spaces() {
    let mut runs = Vec::new();
    let mut read = 0;
    for krate in ["gateway", "plan-core"] {
        for path in sources_of(krate) {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            for literal in string_literals(&text) {
                read += 1;
                if inner_run(&literal) {
                    runs.push(format!("{}: {literal:?}", path.display()));
                }
            }
        }
    }
    assert!(read > 500, "the scan read only {read} string literals");
    assert!(runs.is_empty(), "{}", runs.join("\n"));
}

/// The literal reader reads a kept continuation as one space, sees a lost
/// one, and is not fooled by a character literal, a comment or a raw string;
/// and indentation is no run.
#[test]
fn the_string_scan_reads_literals_as_the_compiler_does() {
    let text = concat!(
        "const A: &str = \"one \\\n     two\";\n",
        "const B: char = '\"';\n",
        "// \"a  b\"\n",
        "const C: &str = r#\"c  d\"#;\n",
        "const D: &str = \"e      f\";\n",
    );
    assert_eq!(
        string_literals(text),
        vec![
            String::from("one two"),
            String::from("c  d"),
            String::from("e      f")
        ]
    );
    assert!(inner_run("does not                      define"));
    assert!(!inner_run("{\n  \"indented\": 1\n}"));
}

/// S3's `fixq`: `compose_wall_box` reads the frozen world in a Lull and steps
/// nothing. Its body, comments aside, names no stepping, sealing or mutating
/// call: no runner, no `host_mut`, no world accessor that writes, no voxel
/// write path; only the sim's two read-only wall readers.
#[test]
fn compose_wall_box_reads_the_frozen_world_and_steps_nothing() {
    let text = std::fs::read_to_string("src/surface/planning.rs").expect("planning.rs");
    // The handler and the two helpers it alone calls.
    let mut body: Vec<String> = Vec::new();
    for (opening, closing) in [
        (
            "pub(super) fn compose_wall_box<",
            "
    }
",
        ),
        (
            "
fn sized_wall(",
            "
}
",
        ),
        (
            "
fn wall_target_json(",
            "
}
",
        ),
    ] {
        let start = text.find(opening).expect("the function is in planning.rs");
        let rest = &text[start..];
        let end = rest.find(closing).expect("its body ends");
        body.extend(code_lines(&rest[..end]).into_iter().map(|(_, line)| line));
    }
    assert!(body.len() > 20, "the scan reads the handler: {body:?}");
    for banned in [
        "host_mut",
        "runner(",
        "Runner",
        "_mut(",
        "seal",
        "phased",
        ".tick(",
        "crater",
        ".set(",
        "restore",
        "snapshot(",
    ] {
        assert!(
            !body.iter().any(|line| line.contains(banned)),
            "compose_wall_box or its helpers name `{banned}`"
        );
    }
    for read in ["site::ground_under_walls", "site::check_wall"] {
        assert!(
            body.iter().any(|line| line.contains(read)),
            "compose_wall_box reads through `{read}`"
        );
    }
}

// ---------------------------------------------------------------------------
// A seal carries the fingerprint it was made with (S3's `fpstream`, part 2)
// ---------------------------------------------------------------------------

/// The seal types, in `surface/sealed.rs`, and the functions there that write
/// a literal of each: every seal is made by one of these, from one decode of
/// its text.
const SEAL_MAKERS: &[(&str, &str)] = &[
    ("Sealed", "file"),
    ("Sealed", "restored_seal"),
    ("Unfiled", "fallback_for"),
    ("Unfiled", "harness_seal"),
    ("Unfiled", "qualify"),
];

/// The file the seal types live in, relative to this crate.
const SEAL_FILE: &str = "src/surface/sealed.rs";

/// The function a line sits in: the name after the last `fn ` at or above it.
fn enclosing_fn(lines: &[(usize, String)], index: usize) -> String {
    lines
        .get(..=index)
        .unwrap_or_default()
        .iter()
        .rev()
        .find_map(|(_, line)| {
            let (_, after) = line.split_once("fn ")?;
            let name: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let rest = after.get(name.len()..).unwrap_or_default();
            (!name.is_empty() && rest.starts_with(['(', '<'])).then_some(name)
        })
        .unwrap_or_default()
}

/// The lines of the `pub struct <name>` block in `lines`.
fn struct_block(lines: &[(usize, String)], name: &str) -> Vec<String> {
    let opener = format!("pub struct {name} {{");
    lines
        .iter()
        .skip_while(|(_, line)| line.trim() != opener)
        .skip(1)
        .take_while(|(_, line)| line.trim() != "}")
        .map(|(_, line)| line.clone())
        .collect()
}

/// A seal holds the pair "this fingerprint, and this price, are of this text"
/// by its type (S3's `fpstream`, part 2, fix pass 2), and these needles hold
/// the type to it, since a `compile_fail` doc test can pass for the wrong
/// reason on stable:
///
/// * a seal is written as a literal only in `surface/sealed.rs`, and there
///   only by the functions [`SEAL_MAKERS`] names;
/// * no field of `Sealed` or `Unfiled` carries any visibility (`pub`,
///   `pub(crate)`, `pub(super)`, `pub(in ..)`): the seal's fields are the seal
///   module's alone;
/// * no field of `SeatState` or `Advised` carries any visibility either, and no
///   plain `pub` function hands out `&mut SeatState` (`seat_state_mut` is
///   `pub(crate)`), so code outside the crate cannot write a seat's store,
///   assign a whole one or move an advice into another seat's (S3's
///   `fpstream`, part 2, fix pass 3; review 3's N1 to N3 are compile errors);
/// * nothing in `surface/sealed.rs` names `&mut self`, `&mut Self`, `&mut
///   Sealed` or `&mut Unfiled`, so no seal is changed after it is made;
/// * no plain `pub` function there returns a seal by value or by `&mut`, in
///   any shape (`Self`, `Option<Sealed>`, `Result<Unfiled, _>`): its makers
///   are `pub(super)`, and a shared reference hands out nothing to change.
///
/// The review's alias blind spot (`type Seal = Sealed; Seal::new(..)`) is
/// moot outside the module: there is no public constructor to alias, and a
/// literal outside the module does not compile. Inside it, a literal spelt
/// `Self { .. }` or through a `type` alias is refused by name, so every
/// literal there is one this scan counts.
#[test]
fn a_seal_is_made_only_by_the_named_makers_and_never_changed() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut findings: Vec<String> = Vec::new();
    let mut makers: Vec<(String, String)> = Vec::new();
    let mut files = sources();
    files.extend(sources_of("gamectl"));
    for path in &files {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let shown = path.display().to_string().replace('\\', "/");
        let in_seal_file = shown.ends_with(SEAL_FILE);
        let lines = code_lines(&text);
        for (index, (number, line)) in lines.iter().enumerate() {
            let trimmed = line.trim();
            for name in ["Sealed", "Unfiled"] {
                let declares = trimmed == format!("pub struct {name} {{")
                    || trimmed == format!("impl {name} {{");
                // `Boundary::Sealed { .. }` is the save's boundary, a variant
                // and not this type, so a path-qualified name is skipped, and
                // so are signatures (`-> Sealed {`).
                let written = line.contains(&format!("{name} {{"))
                    && !declares
                    && !line.contains(&format!("::{name} {{"))
                    && !line.contains(&format!("-> {name} {{"))
                    && !trimmed.starts_with("pub ")
                    && !trimmed.starts_with("pub(");
                if !written {
                    continue;
                }
                let function = enclosing_fn(&lines, index);
                if in_seal_file {
                    makers.push((name.to_owned(), function));
                } else {
                    findings.push(format!("{shown}:{number}: a `{name}` literal: {trimmed}"));
                }
            }
        }
    }
    makers.sort();
    let expected: Vec<(String, String)> = SEAL_MAKERS
        .iter()
        .map(|(name, function)| ((*name).to_owned(), (*function).to_owned()))
        .collect();
    assert_eq!(
        makers, expected,
        "the seal makers moved; a new one is a maker whose carried fingerprint and price must \
         be held to its text (`tests/seal_work.rs`), and its name joins SEAL_MAKERS with that test"
    );

    findings.extend(seal_file_findings(&root));
    findings.extend(store_findings(&root));
    assert!(
        findings.is_empty(),
        "a seal is made by its named makers, holds its fields privately and is never changed:\n{}",
        findings.join("\n")
    );
}

/// The scenario harness's door, `Surface::seal_by_harness`, is host-side like
/// `Surface::file_advice` (`no_handler_can_file_advice`): no module of this
/// crate names it but the surface that defines it, and in `gamectl` only the
/// scenario runner does. A handler that could reach it would let a client seal
/// a playbook the verifier refused.
#[test]
fn no_handler_can_seal_by_the_harness() {
    const NEEDLE: &str = "seal_by_harness";
    const DEFINER_PATH: &str = "src/surface.rs";
    const RUNNER_PATH: &str = "gamectl/src/scenario/run.rs";
    let mut findings: Vec<String> = Vec::new();
    let mut definitions = 0_usize;
    let mut called = 0_usize;
    let mut files = sources();
    files.extend(sources_of("gamectl"));
    for path in &files {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let shown = path.display().to_string().replace('\\', "/");
        for (number, line) in code_lines(&text) {
            if !uses(&line, NEEDLE) {
                continue;
            }
            let trimmed = line.trim();
            if shown.ends_with(DEFINER_PATH) && trimmed.starts_with("pub fn ") {
                definitions = definitions.saturating_add(1);
            } else if shown.ends_with(RUNNER_PATH) {
                called = called.saturating_add(1);
            } else {
                findings.push(format!("{shown}:{number}: {trimmed}"));
            }
        }
    }
    assert!(
        findings.is_empty(),
        "the harness's door is named outside the surface and the scenario runner:\n{}",
        findings.join("\n")
    );
    assert_eq!(definitions, 1, "the surface defines the door once");
    assert_eq!(called, 1, "the scenario runner calls it once");
}

/// Every function signature in `lines`, joined onto one line from its `fn`
/// to the brace or semicolon that ends it, with the line it starts on.
fn signatures(lines: &[(usize, String)]) -> Vec<(usize, String)> {
    let mut found: Vec<(usize, String)> = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (number, line) in lines {
        let trimmed = line.trim();
        if open.is_none() && (trimmed.starts_with("fn ") || trimmed.contains(" fn ")) {
            open = Some((*number, String::new()));
        }
        if let Some((_, text)) = open.as_mut() {
            text.push_str(trimmed);
            text.push(' ');
            if (trimmed.ends_with('{') || trimmed.ends_with(';'))
                && let Some(done) = open.take()
            {
                found.push(done);
            }
        }
    }
    found
}

/// Whether a signature's visibility is plain `pub` (reachable outside the
/// crate), as opposed to `pub(crate)`, `pub(super)` or private.
fn is_plain_pub(signature: &str) -> bool {
    signature.starts_with("pub fn ") || signature.starts_with("pub const fn ")
}

/// The part of a signature after its `->`, or empty.
fn returned(signature: &str) -> &str {
    signature.split_once("->").map_or("", |(_, rest)| rest)
}

/// Whether a return type names a seal by value or by `&mut`: a shape that
/// could hand out a seal made or changed outside its makers. A shared
/// reference (`&Sealed`, `Option<&Unfiled>`) hands out nothing to change.
fn hands_out_a_seal(returned: &str) -> bool {
    ["Self", "Sealed", "Unfiled"].iter().any(|name| {
        returned.match_indices(name).any(|(at, _)| {
            let before = returned.get(..at).unwrap_or_default().trim_end();
            let after = returned
                .get(at.saturating_add(name.len())..)
                .unwrap_or_default();
            let whole = !after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
            whole && !before.ends_with('&')
        })
    })
}

/// What `surface/sealed.rs` must not hold: a public field of a seal type, of
/// any visibility spelling; a literal under another name; a `&mut` to a seal,
/// as a receiver or anywhere else; or a public function that hands a seal out
/// by value or by `&mut`.
fn seal_file_findings(root: &std::path::Path) -> Vec<String> {
    let mut findings: Vec<String> = Vec::new();
    let sealed_text = std::fs::read_to_string(root.join(SEAL_FILE)).expect("the seal types' file");
    let sealed_lines = code_lines(&sealed_text);
    for name in ["Sealed", "Unfiled"] {
        let fields = struct_block(&sealed_lines, name);
        assert!(
            fields.len() > 2,
            "{name} read as {} lines; the scan is looking at the wrong text",
            fields.len()
        );
        for field in fields {
            if field.trim_start().starts_with("pub") {
                findings.push(format!(
                    "{SEAL_FILE}: `{name}` has a public field: {}",
                    field.trim()
                ));
            }
        }
    }
    for (number, line) in &sealed_lines {
        let trimmed = line.trim();
        // A literal spelt `Self { .. }` or through an alias would make a seal
        // out of this scan's sight, so neither is written here.
        if line.contains("Self {") || trimmed.starts_with("type ") || line.contains(" as Sealed") {
            findings.push(format!(
                "{SEAL_FILE}:{number}: a seal written under another name: {trimmed}"
            ));
        }
        let changes = ["&mut self", "&mut Self", "&mut Sealed", "&mut Unfiled"]
            .iter()
            .any(|shape| line.contains(shape));
        if changes {
            findings.push(format!(
                "{SEAL_FILE}:{number}: a `&mut` to a seal: {trimmed}"
            ));
        }
    }
    for (number, signature) in signatures(&sealed_lines) {
        if is_plain_pub(&signature) && hands_out_a_seal(returned(&signature)) {
            findings.push(format!(
                "{SEAL_FILE}:{number}: a public function hands out a seal: {signature}"
            ));
        }
    }
    findings
}

/// What `src/surface.rs` must not hold: a field of `SeatState` or `Advised`
/// with any visibility (`pub`, `pub(crate)`, `pub(super)` or `pub(in ..)`),
/// since every write to a seat's store is the surface's; or a public function
/// that hands out `&mut SeatState`, through which a whole store could be
/// assigned (S3's `fpstream`, part 2, fix pass 3).
fn store_findings(root: &std::path::Path) -> Vec<String> {
    let mut findings: Vec<String> = Vec::new();
    let surface_text = std::fs::read_to_string(root.join("src/surface.rs")).expect("the surface");
    let lines = code_lines(&surface_text);
    for name in ["SeatState", "Advised"] {
        let fields = struct_block(&lines, name);
        assert!(
            fields.len() > 2,
            "{name} read as {} lines; the scan is looking at the wrong text",
            fields.len()
        );
        for field in fields {
            if field.trim_start().starts_with("pub") {
                findings.push(format!(
                    "src/surface.rs: `{name}` has a field reachable outside the surface: {}",
                    field.trim()
                ));
            }
        }
    }
    for (number, signature) in signatures(&lines) {
        if is_plain_pub(&signature) && returned(&signature).contains("&mut SeatState") {
            findings.push(format!(
                "src/surface.rs:{number}: a public function hands out a seat's store to write: \
                 {signature}"
            ));
        }
    }
    findings
}

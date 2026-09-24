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
const PERMITTED_ALLOWS: &[&str] = &["clippy::struct_field_names", "clippy::unused_self"];

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

/// The file's shipped code: comment-only lines removed, so prose *about* a
/// banned name is not mistaken for a use of it, and everything from the first
/// `#[cfg(test)]` onward removed, because a unit test is allowed to panic --
/// that is what a failing assertion is -- and is not compiled into the game.
fn code_lines(text: &str) -> Vec<(usize, String)> {
    let shipped = match text.find("#[cfg(test)]") {
        Some(index) => text.get(..index).unwrap_or(text),
        None => text,
    };
    shipped
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .map(|(number, line)| (number.saturating_add(1), (*line).to_owned()))
        .collect()
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

fn scan(needles: &[(&str, &str)]) -> Vec<String> {
    scan_except(needles, &[])
}

/// [`scan`], skipping files whose name is on `exempt`.
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
/// dispatch arm.
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
    let findings = scan_except(HOST_SIDE, &[CALLER, DEFINER]);
    assert!(
        findings.is_empty(),
        "a host-side seam is named outside the host loop:\n{}",
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
        "`Plan::compile` is called once, in `surface/planning.rs`'s `compile_playbook`, which is \
         the one door a submitted playbook goes through into the sim. If this is 0 the ban above \
         is guarding an empty room; if it is more than 1 there are two doors and they can drift"
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

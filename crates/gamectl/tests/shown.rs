// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every text `gamectl` prints is shown escaped (S3's `vfyq2`, fix pass 5,
//! item L9): a path or an operand the user typed on the command line, a
//! scenario file's names and pointers, and another error's words. A
//! character that would reorder a terminal's line (U+202E) or drive it (ESC)
//! reaches it as `\uXXXX`; text without one is printed as it is.

use pharmakos_gamectl::strings;

const HOSTILE: &str = "a\u{202E}b\u{1b}[2Jc";

fn escaped(line: &str) -> bool {
    !line.contains('\u{202E}') && !line.contains('\u{1b}') && line.contains("\\u202e")
}

#[test]
fn a_path_an_operand_or_a_name_the_user_typed_is_printed_escaped() {
    let lines = [
        strings::unknown_command(HOSTILE),
        strings::unknown_subcommand("scenario", HOSTILE),
        strings::extra_operand("verify", HOSTILE),
        strings::bad_depth(HOSTILE),
        strings::bad_value("--depth", HOSTILE, &["quick", "full"]),
        strings::unreadable("scenario", HOSTILE, "no such file"),
        strings::path_escapes(HOSTILE),
        strings::rules_gap(HOSTILE, "a gap"),
        strings::verify_heading(HOSTILE, "quick", &strings::verify_view_reference()),
        strings::verify_diagnostic("error", "E0001", HOSTILE, "a message"),
        // A problem's message is a sentence the reader composed, its leaves
        // escaped where it wrote them (fix pass 6, item 1); the path and the
        // pointer are leaves.
        strings::scenario_invalid(
            HOSTILE,
            &[(
                format!("/{HOSTILE}"),
                format!(
                    "`{}` must be one of playbook, safe, builtin",
                    pharmakos_proto::json::grammar::escaped(HOSTILE)
                ),
            )],
        ),
        strings::scenario_heading(HOSTILE, 2, 10, "0"),
        strings::scenario_playbook_refused(0, HOSTILE, "why"),
        strings::scenario_event_missing(HOSTILE, None, &[HOSTILE.to_owned()]),
        strings::scenario_passed(HOSTILE, 1),
        strings::seeds_refused(HOSTILE, "why"),
        strings::result_written(HOSTILE, 1, 0),
        // `verify --preset`'s refusals (fix pass 1 of #219, item 12): the
        // gateway's sentence is a leaf, escaped once more where it is said,
        // which leaves text the gateway already escaped as it was.
        strings::lobby_line_refused(HOSTILE),
        strings::lobby_match_refused(HOSTILE),
        strings::lobby_text_refused(HOSTILE),
        strings::lobby_answer_unread(HOSTILE),
        strings::verify_preset_too_long(HOSTILE, Some(1), 2, 3),
    ];
    for line in &lines {
        assert!(escaped(line), "printed raw: {line:?}");
    }
}

/// Text with no hidden character is printed as it is.
#[test]
fn a_plain_path_is_printed_as_it_is() {
    assert_eq!(
        strings::unreadable("scenario", "scenarios/a b.jsonc", "no such file"),
        "gamectl: the scenario at `scenarios/a b.jsonc` could not be read: no such file"
    );
}

const GOLDEN: &str = "tests/golden/scenarios/x/expected.hashes.txt";

/// A sentence gamectl composes keeps its own lines and columns (S3's
/// `vfyq2`, fix pass 6, item 1): only the leaves are escaped, never the
/// sentence. A moved chain prints exactly as it did before fix pass 5.
#[test]
fn a_moved_chain_prints_on_its_own_lines() {
    let why = strings::scenario_chain_moved(GOLDEN, 12, "0011", "0012", 40, 40);
    let printed =
        strings::scenario_assertion_failed(1, &strings::scenario_hash_chain(GOLDEN), &why);
    let expected = format!(
        "  FAIL  /assertions/1  the per-tick chain equals `{GOLDEN}`\n        the chain moved. \
         `{GOLDEN}` line 12 is\n          0011\n        and this run's is\n          0012\n        \
         (40 committed lines against 40 played ticks.) tests/golden/scenarios/README.md says \
         what a diff there means; the pull request has to say which rule moved it, and \
         re-blessing it to turn a red build green is the one thing that is never the answer."
    );
    assert_eq!(printed, expected);
}

/// A leaf inside a composed sentence is still escaped: a hostile line read
/// from the golden, and a hostile golden path.
#[test]
fn a_hostile_leaf_inside_a_moved_chain_is_escaped() {
    let why = strings::scenario_chain_moved(HOSTILE, 12, HOSTILE, "0012", 40, 40);
    let printed =
        strings::scenario_assertion_failed(1, &strings::scenario_hash_chain(HOSTILE), &why);
    assert!(escaped(&printed), "printed raw: {printed:?}");
    assert_eq!(printed.lines().count(), 6, "its own lines kept: {printed}");
}

/// The doctor's columns and a multi-line detail, as before fix pass 5.
#[test]
fn a_doctor_line_keeps_its_columns_and_lines() {
    assert_eq!(
        strings::doctor_line("FAIL", "host a match", "line one\nline two"),
        "FAIL  host a match           line one\nline two"
    );
    assert_eq!(
        strings::verify_diagnostic("error", "E0001", "/meta", "a message"),
        "  error   E0001 /meta\n          a message"
    );
}

/// A scenario file's own words are escaped where the reader composes them
/// into a problem (fix pass 6, item 1): a seat kind, an assertion kind, an
/// event name, a format string and a path the file wrote.
#[test]
fn a_scenario_file_s_words_are_escaped_where_the_reader_composes_them() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace root")
        .to_path_buf();
    let scratch = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shown-scenario");
    std::fs::create_dir_all(&scratch).expect("a scratch folder");
    let file = scratch.join("hostile.scenario.jsonc");
    // The hostile text as JSON writes it, `` and all.
    let h = pharmakos_proto::json::write(&pharmakos_proto::json::Json::String(HOSTILE.to_owned()));
    let h = h.trim_end().trim_matches('"');
    let text = format!(
        r#"{{"format":"{h}","name":"n","seed":"0000000000000001","seats":[{{"seat":0,"kind":"{h}"}}],"segments":[{{"index":0,"length_ms":3000}}],"assertions":[{{"assert":"{h}"}},{{"assert":"event_fired","event":"{h}","by_tick":1}}]}}"#,
    );
    std::fs::write(&file, text).expect("the file");
    let Err(failure) = pharmakos_gamectl::scenario::load(&root, &file) else {
        panic!("the file is refused");
    };
    assert!(
        !failure.message.contains('\u{202E}') && !failure.message.contains('\u{1b}'),
        "printed raw: {}",
        failure.message
    );
    assert!(failure.message.lines().count() > 3, "{}", failure.message);
}

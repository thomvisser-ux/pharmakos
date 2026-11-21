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
        strings::verify_heading(HOSTILE, "quick"),
        strings::verify_diagnostic("error", "E0001", HOSTILE, "a message"),
        strings::scenario_invalid(HOSTILE, &[(format!("/{HOSTILE}"), HOSTILE.to_owned())]),
        strings::scenario_heading(HOSTILE, 2, 10, "0"),
        strings::scenario_playbook_refused(0, HOSTILE, "why"),
        strings::scenario_event_missing(HOSTILE, None, &[HOSTILE.to_owned()]),
        strings::scenario_passed(HOSTILE, 1),
        strings::seeds_refused(HOSTILE, "why"),
        strings::result_written(HOSTILE, 1, 0),
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

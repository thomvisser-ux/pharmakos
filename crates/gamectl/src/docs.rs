// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `gamectl docs` — the generated reference.
//!
//! Spec §12: "`get_schema` and the `gamectl` docs output are generated from the
//! schema so they can't drift." **Nothing in this file is written twice.** The
//! vocabulary section is walked out of
//! [`pharmakos_proto::descriptor::schema`], the same checked-in descriptor set
//! the canonical JSON codec and the gateway's `get_schema` are driven from; the
//! diagnostics section is [`pharmakos_verifier::catalogue::CATALOGUE`] and the
//! verifier's own string table; and the command and exit-code sections are
//! [`crate::cli::COMMANDS`] and [`crate::exit::Exit::ALL`], which are also what
//! `--help` prints. A field added to `gp.v1`, a diagnostic added to the
//! catalogue or a command added to the parser appears here the moment the tree
//! is regenerated, and the golden moves in the pull request that added it.
//!
//! # What the golden is for
//!
//! `tests/golden/docs/reference/expected.docs.txt` is the committed half, and
//! `tests/golden/docs/README.md` says what each kind of diff there means. The
//! interesting failure is the one that shape catches: documentation that moved
//! while the schema did not, which means this file started *building* an answer
//! instead of reading one.
//!
//! # Deliberately not here
//!
//! `gp.api.v1` — the methods, the scopes, the error codes. It is the
//! transport, not the vocabulary, and v1 publishes nothing (AGENTS.md §11:
//! `llms.txt`, the agent guide and the published schema docs ship with v1.1).
//! The gateway's own `get_schema` draws the same line and says why.

use std::fmt::Write as _;

use pharmakos_proto::descriptor::{Field, Message, ScalarKind, schema};
use pharmakos_verifier::catalogue::{CATALOGUE, severity_name};

use crate::cli::COMMANDS;
use crate::exit::{Exit, Failure};
use crate::strings;

/// The package the vocabulary section documents.
const PACKAGE: &str = "gp.v1.";

/// Render the reference.
///
/// # Errors
///
/// [`crate::exit::Exit::Internal`] when the checked-in descriptor set carries
/// no `gp.v1` at all, which would mean the generated tree in `crates/proto` and
/// this build disagree.
pub fn run() -> Result<String, Failure> {
    let schema = schema();
    let mut messages: Vec<&Message> = schema
        .messages()
        .filter(|message| message.full_name.starts_with(PACKAGE))
        .collect();
    messages.sort_by(|left, right| left.full_name.cmp(&right.full_name));
    if messages.is_empty() {
        return Err(Failure::internal(
            "the checked-in descriptor set carries no gp.v1 messages: the generated tree in \
             crates/proto and this build disagree",
        ));
    }

    let mut text = String::with_capacity(64 * 1024);
    heading(&mut text);
    commands(&mut text);
    vocabulary(&mut text, &messages);
    enums(&mut text, schema);
    diagnostics(&mut text);
    Ok(text)
}

fn heading(text: &mut String) {
    text.push_str("# Pharmakos reference\n\n");
    text.push_str(strings::TAGLINE);
    text.push_str(
        "\n\nEVERY LINE BELOW IS GENERATED. The vocabulary comes from the checked-in gp.v1\n\
         descriptor set, the diagnostics from the verifier's catalogue and string table, and\n\
         the commands and exit codes from this binary's own tables. Nothing here is written\n\
         by hand, which is what makes it unable to drift from what the code does (spec §12,\n\
         \"Generated docs\"). Regenerate it with `gamectl docs`; the committed copy is\n\
         tests/golden/docs/reference/expected.docs.txt.\n\n",
    );
}

fn commands(text: &mut String) {
    text.push_str("## Commands\n\n");
    for command in COMMANDS {
        let _ = writeln!(text, "{:<26}{}", command.usage, command.about);
    }
    text.push_str("\n## Exit codes\n\n");
    for (code, meaning) in Exit::ALL {
        let _ = writeln!(text, "{}  {meaning}", code.code());
    }
    text.push('\n');
}

fn vocabulary(text: &mut String, messages: &[&Message]) {
    text.push_str("## The playbook vocabulary (gp.v1)\n\n");
    let _ = writeln!(
        text,
        "{} messages. A field's name is the one the canonical form writes, which is",
        messages.len()
    );
    text.push_str(
        "the .proto spelling and not lowerCamelCase. A reserved number is one held on\n\
         purpose: field numbers are added, never renumbered or reused.\n",
    );
    for message in messages {
        let _ = writeln!(text, "\n{}", message.full_name);
        if !message.oneofs.is_empty() {
            let _ = writeln!(text, "  oneof: {}", message.oneofs.join(", "));
        }
        for field in &message.fields {
            let _ = writeln!(
                text,
                "  {:<28} {:<3} {}{}",
                field.name,
                field.number,
                if field.repeated { "repeated " } else { "" },
                type_of(field)
            );
        }
        for (low, high) in &message.reserved_ranges {
            // Reserved numbers are a contract (AGENTS.md §5, "reserved field
            // numbers stay reserved"), so the reference prints them: a reader
            // asking "why can I not use 12" gets the answer here.
            let _ = if low == high {
                writeln!(text, "  reserved {low}")
            } else {
                writeln!(text, "  reserved {low} to {high}")
            };
        }
        if !message.reserved_names.is_empty() {
            let _ = writeln!(text, "  reserved {}", message.reserved_names.join(", "));
        }
    }
    text.push('\n');
}

fn enums(text: &mut String, schema: &pharmakos_proto::descriptor::Schema) {
    let mut listed: Vec<&pharmakos_proto::descriptor::Enum> = schema
        .enums()
        .filter(|item| item.full_name.starts_with(PACKAGE))
        .collect();
    listed.sort_by(|left, right| left.full_name.cmp(&right.full_name));
    let _ = writeln!(
        text,
        "\n## The enumerations (gp.v1)\n\n{} of them. JSON spells an enum by value name.\n",
        listed.len()
    );
    for item in listed {
        let _ = writeln!(text, "\n{}", item.full_name);
        for value in &item.values {
            let _ = writeln!(text, "  {:<32} {}", value.name, value.number);
        }
    }
    text.push('\n');
}

fn diagnostics(text: &mut String) {
    let _ = writeln!(
        text,
        "\n## Diagnostics\n\n{} codes. A code is language-neutral, append-only and never\n\
         reused; only an ERROR stops a playbook qualifying. `none at the skeleton` names the\n\
         task that owes the emitter, which is honesty about coverage rather than a gap.\n",
        CATALOGUE.len()
    );
    for entry in CATALOGUE {
        let _ = writeln!(
            text,
            "\n{}  {}  {}  raised at: {}",
            entry.code,
            severity_name(entry.severity),
            entry.family.name(),
            entry.emitter.label()
        );
        let _ = writeln!(text, "  {}", entry.message());
        let _ = writeln!(text, "  {}", entry.beginner());
    }
}

/// How the reference spells one field's type.
fn type_of(field: &Field) -> String {
    match field.kind {
        ScalarKind::Message | ScalarKind::Enum => field.type_name.clone(),
        ScalarKind::Double => String::from("double"),
        ScalarKind::Float => String::from("float"),
        ScalarKind::Int64 => String::from("int64"),
        ScalarKind::Uint64 => String::from("uint64"),
        ScalarKind::Int32 => String::from("int32"),
        ScalarKind::Fixed64 => String::from("fixed64"),
        ScalarKind::Fixed32 => String::from("fixed32"),
        ScalarKind::Bool => String::from("bool"),
        ScalarKind::String => String::from("string"),
        ScalarKind::Bytes => String::from("bytes"),
        ScalarKind::Uint32 => String::from("uint32"),
        ScalarKind::Sfixed32 => String::from("sfixed32"),
        ScalarKind::Sfixed64 => String::from("sfixed64"),
        ScalarKind::Sint32 => String::from("sint32"),
        ScalarKind::Sint64 => String::from("sint64"),
    }
}

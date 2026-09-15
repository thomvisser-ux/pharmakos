// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The local template and sample library: a folder of JSONC files the gateway
//! **reads** and stores nothing from.
//!
//! Spec section 13, "Local-first library":
//!
//! > templates and a sample library are JSONC files in a local folder … The
//! > gateway reads that folder to list and instantiate templates, but stores
//! > none of it: your files stay local, and anything you write reaches the
//! > gateway only once it becomes a draft or is submitted. … Nothing in the
//! > library executes: these are data files, not scripts.
//!
//! So this module reads and parses; it never writes, never caches, and never
//! runs anything. Every call re-reads the folder, which is what "stores none
//! of it" means in code.
//!
//! # The path rule
//!
//! A `template_id` arrives from a client (`gp.api.v1.InstantiateTemplateRequest`)
//! and is a **plain file stem**, never a path. [`read`] refuses anything with a
//! separator, a drive letter, a `..` or a Win32 device name in it, so a client
//! cannot walk out of the folder it was pointed at or reach a device instead of
//! a file. The refusal is decided on characters rather than on
//! `std::path::Component`, because the host's path rules differ: a backslash is
//! a separator on Windows and an ordinary filename byte on Linux and macOS, and
//! the same client bytes must get the same answer on all three.
//! AGENTS.md section 7 puts this the other way
//! round — "no filesystem or network access through playbooks" — and this is
//! the one place in the crate that touches a filesystem at all, so it is the
//! one place that has to say no.
//!
//! # Listing order
//!
//! Collected and sorted by id before use. Directory order differs between
//! ext4, NTFS and APFS, and a template list that came back in a different
//! order on three operating systems would make the editor's wizard
//! platform-dependent (`clippy.toml`'s own reason string names this case).

use std::path::{Component, Path, PathBuf};

use pharmakos_proto::gp::v1::{Playbook, playbook};

use crate::canonical::canonicalise_text;
use crate::error::Error;
use crate::jsonc::Document;
use crate::patch::{Op, Operation, Patch, apply};

/// The extension every library file carries.
pub const EXTENSION: &str = "jsonc";

/// One entry of the library, as `gp.api.v1.TemplateSummary` spells it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Summary {
    /// The file's stem, which is how a client asks for it.
    pub template_id: String,
    /// `meta.title`.
    pub title: String,
    /// The author's labels, taken from the tags the template's own
    /// `place_beacon` steps carry. There is no separate tag field in `gp.v1`
    /// and inventing one would be a proto change (AGENTS.md section 5).
    pub tags: Vec<String>,
    /// One sentence, from `meta.note`'s first line.
    pub blurb: String,
    /// What the file says it is. A `PLAYBOOK` in the library is a worked file
    /// rather than a template, and the wizard does not offer it.
    pub kind: playbook::Kind,
}

/// Lists the library, in `template_id` order.
///
/// A file that does not parse or does not verify structurally is **skipped
/// rather than fatal**: one broken file a player dropped in their own folder
/// must not stop the wizard from opening. The editor surfaces it when the
/// player tries to open it, with the verifier's code and pointer.
///
/// # Errors
///
/// When the folder cannot be read at all.
#[allow(
    clippy::disallowed_methods,
    reason = "clippy.toml's own reason string sanctions read_dir for a template list when the listing is collected and sorted before use, which is exactly what happens here"
)]
pub fn list(folder: &Path) -> Result<Vec<Summary>, Error> {
    let entries = std::fs::read_dir(folder)
        .map_err(|error| Error::at("", format!("reading {}: {error}", folder.display())))?;
    let mut ids: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == EXTENSION)
        })
        .filter_map(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .collect();
    ids.sort();
    ids.dedup();

    let mut found = Vec::new();
    for template_id in ids {
        let Ok(text) = read(folder, &template_id) else {
            continue;
        };
        let Ok(canonical) = canonicalise_text(&text) else {
            continue;
        };
        found.push(summarise(&template_id, &canonical.playbook));
    }
    Ok(found)
}

/// Reads one library file.
///
/// # Errors
///
/// When `template_id` is not a plain name, or the file cannot be read.
pub fn read(folder: &Path, template_id: &str) -> Result<String, Error> {
    let path = resolve(folder, template_id)?;
    std::fs::read_to_string(&path)
        .map_err(|error| Error::at("", format!("reading {}: {error}", path.display())))
}

/// The Win32 device names. `<dir>\CON.jsonc` resolves to the console device on
/// Windows whatever the directory and whatever the extension, so a client-supplied
/// id that spells one is refused **on every platform**: the three operating
/// systems have to answer a hostile id identically or the gateway's behaviour is
/// host-dependent, which is the same argument the listing order makes above.
const DEVICE_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Turns a `template_id` into a path inside the folder, refusing anything that
/// is not a plain name.
///
/// The decision is made on **characters, never on the host's path rules**.
/// `std::path::Component` is platform-dependent — on Unix a backslash is an
/// ordinary filename byte, so `Path::new("a\\b")` is one `Normal` component and
/// a separator on Windows is a filename on Linux and macOS. A client's bytes
/// must get the same answer on all three, so the separators are named here and
/// the `Component` walk is kept only as belt and braces.
fn resolve(folder: &Path, template_id: &str) -> Result<PathBuf, Error> {
    let refuse = || {
        Error::at(
            "",
            format!("`{template_id}` is not a template name; a template id is a plain file stem"),
        )
    };
    if template_id.is_empty() || template_id == "." || template_id == ".." {
        return Err(refuse());
    }
    if template_id.contains(['/', '\\', ':']) {
        return Err(refuse());
    }
    if DEVICE_NAMES
        .iter()
        .any(|device| template_id.eq_ignore_ascii_case(device))
    {
        return Err(refuse());
    }
    let candidate = Path::new(template_id);
    let mut components = candidate.components();
    let Some(Component::Normal(only)) = components.next() else {
        return Err(refuse());
    };
    if components.next().is_some() || only != template_id {
        return Err(refuse());
    }
    Ok(folder.join(format!("{template_id}.{EXTENSION}")))
}

fn summarise(template_id: &str, book: &Playbook) -> Summary {
    let meta = book.meta.as_ref();
    let mut tags: Vec<String> = Vec::new();
    if let Some(body) = book.declarative.as_ref() {
        for entry in &body.route {
            if let Some(pharmakos_proto::gp::v1::step::Kind::PlaceBeacon(place)) =
                entry.kind.as_ref()
            {
                tags.extend(place.tags.iter().cloned());
            }
        }
    }
    tags.sort();
    tags.dedup();
    Summary {
        template_id: template_id.to_owned(),
        title: meta.map(|meta| meta.title.clone()).unwrap_or_default(),
        tags,
        blurb: meta
            .and_then(|meta| meta.note.lines().next())
            .unwrap_or_default()
            .trim()
            .to_owned(),
        kind: book.kind(),
    }
}

/// One parameter the wizard fills in, as `gp.api.v1.Parameter` carries it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Parameter {
    /// Where in the template it goes.
    pub name: String,
    /// What to put there.
    pub value: String,
}

/// Instantiates a template into a playbook.
///
/// The template's own comments and formatting survive: a template's "why"
/// notes are the point of it (spec section 13, "a parameter wizard pre-filled
/// with the built-in operator's suggestion and a 'why' note"), and losing them
/// on instantiation would throw away the teaching half of the library.
///
/// Three things happen, in order: each parameter is applied as an RFC 6902
/// `replace`, `kind` becomes `PLAYBOOK`, and the result is handed back as
/// JSONC text. It is **not** canonicalised — the caller verifies it, and the
/// editor opens it — so a template that was hand-formatted opens looking as
/// its author wrote it.
///
/// **PLACEHOLDER — how a template declares its parameters.**
/// `gp.api.v1.Parameter` is a `PLACEHOLDER STUB` naming S6 for the typed
/// parameter catalogue, and `gp.v1` has no parameter message at all. Until it
/// does, a parameter's `name` is read as an **RFC 6901 JSON Pointer** into the
/// template and its `value` as the JSON text to put there — which is the same
/// mechanism the editor already uses for every other edit (spec section 13:
/// "Edits are JSON Patches"), so nothing new is invented and nothing has to be
/// unpicked when the catalogue lands. The **owner** settles the catalogue at
/// **S6**, with the editor and the library.
///
/// # Errors
///
/// When the template does not parse, when a parameter's pointer does not
/// resolve, or when its value is not JSON.
pub fn instantiate_template(
    template_jsonc: &str,
    parameters: &[Parameter],
) -> Result<String, Error> {
    let document = Document::parse(template_jsonc)?;
    let mut operations = Vec::with_capacity(parameters.len().saturating_add(1));
    for parameter in parameters {
        let value = pharmakos_proto::json::read(&parameter.value).map_err(|error| {
            Error::at(
                &parameter.name,
                format!(
                    "`{}` is not a JSON value: {}",
                    parameter.value, error.message
                ),
            )
        })?;
        operations.push(Operation::with_value(
            Op::Replace,
            parameter.name.clone(),
            crate::jsonc::Fragment::from_json(&value),
        ));
    }
    operations.push(Operation::with_value(
        Op::Add,
        "/kind",
        crate::jsonc::Fragment::from_json(&pharmakos_proto::json::Json::String(
            "PLAYBOOK".to_owned(),
        )),
    ));
    let (instantiated, _undo) = apply(&document, &Patch::new(operations))?;
    Ok(instantiated.to_text())
}

#[cfg(test)]
mod tests {
    use super::{Parameter, instantiate_template, list, read, resolve};
    use std::path::Path;

    const TEMPLATE: &str = concat!(
        "// Expand east, then mine.\n",
        "{\"schema_version\":{\"major\":1},\n",
        " \"meta\":{\"title\":\"Expand & Mine\",\"author_kind\":\"HUMAN\",",
        "\"note\":\"Walks east and puts a Mine beacon down.\"},\n",
        " \"declarative\":{\"route\":[\n",
        "   // the site the wizard fills in\n",
        "   {\"label\":\"go\",\"place_beacon\":{\"at\":{\"voxel\":{\"x\":1,\"y\":2,\"z\":3}},",
        "\"tags\":[\"east\"]}}]},\n",
        " \"on_death\":{\"on_respawn\":\"CONTINUE\"},\n",
        " \"fallback\":{\"hold\":{\"at\":{\"beacon_anchor\":{\"safest\":{}}}}},\n",
        " \"kind\":\"TEMPLATE\"}\n"
    );

    #[test]
    fn a_template_id_is_a_plain_name() {
        let folder = Path::new("templates");
        assert!(resolve(folder, "hold_and_build").is_ok());
        // The answer has to be the same on Windows, Linux and macOS: a
        // backslash is a separator on one and an ordinary filename byte on the
        // other two, and `con` is a device on one and a name on the other two.
        for hostile in [
            "",
            ".",
            "..",
            "../secrets",
            "a/b",
            "a\\b",
            "C:/x",
            "con",
            "CON",
            "LPT9",
        ] {
            assert!(
                resolve(folder, hostile).is_err(),
                "`{hostile}` walked out of the folder"
            );
        }
    }

    #[test]
    fn instantiating_keeps_the_comments_and_flips_the_kind() {
        let playbook = instantiate_template(
            TEMPLATE,
            &[Parameter {
                name: "/declarative/route/0/place_beacon/at/voxel/x".to_owned(),
                value: "96".to_owned(),
            }],
        )
        .expect("the template instantiates");
        assert!(
            playbook.contains("// the site the wizard fills in"),
            "{playbook}"
        );
        assert!(
            playbook.contains("// Expand east, then mine."),
            "{playbook}"
        );
        assert!(playbook.contains("\"x\":96"), "{playbook}");
        assert!(playbook.contains("\"kind\":\"PLAYBOOK\""), "{playbook}");
        assert!(!playbook.contains("TEMPLATE"), "{playbook}");
    }

    #[test]
    fn a_parameter_that_does_not_resolve_is_refused() {
        let error = instantiate_template(
            TEMPLATE,
            &[Parameter {
                name: "/declarative/route/9/label".to_owned(),
                value: "\"nope\"".to_owned(),
            }],
        )
        .expect_err("there is no ninth step");
        assert!(!error.pointer.is_empty());
    }

    #[test]
    fn a_parameter_whose_value_is_not_json_is_refused() {
        assert!(
            instantiate_template(
                TEMPLATE,
                &[Parameter {
                    name: "/meta/title".to_owned(),
                    value: "not json".to_owned(),
                }],
            )
            .is_err()
        );
    }

    #[test]
    fn a_folder_that_is_not_there_is_an_error_and_not_an_empty_library() {
        assert!(list(Path::new("no/such/folder")).is_err());
        assert!(read(Path::new("no/such/folder"), "nothing").is_err());
    }
}

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
use pharmakos_proto::json::Json;

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
    /// Where in the template it goes: the RFC 6901 pointer a
    /// `gp.v1.TemplateParameter` declares.
    pub name: String,
    /// What to put there, as JSON text.
    pub value: String,
}

/// One parameter a template declares (`gp.v1.Meta.parameters`), in the file's
/// own order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Declared {
    /// Where the value goes: an RFC 6901 pointer into the template.
    pub pointer: String,
    /// What the wizard calls it.
    pub label: String,
}

/// One declared parameter as it was filled, as `gp.api.v1.FilledParameter`
/// carries it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Filled {
    /// The declaration's pointer.
    pub pointer: String,
    /// The declaration's label.
    pub label: String,
    /// The JSON that was applied, as compact text: the explicit value, else the
    /// suggestion, else the template's own.
    pub value: String,
    /// True when `value` is the suggestion.
    pub suggested: bool,
}

/// A template made into a playbook, with the list of what was filled in.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Instance {
    /// The playbook, JSONC, the template's comments and formatting kept.
    pub playbook_jsonc: String,
    /// Every parameter the template declares, in declaration order.
    pub parameters: Vec<Filled>,
}

/// The parameters a template declares, in the file's order.
///
/// Read from `meta.parameters` in the file as written (decisions-log item 111,
/// decision C1): the declaration is data in the template, which is what lets a
/// template nobody wrote code for — S6's "Save as template" — have a wizard.
/// A file that declares none has none, which is what every playbook has.
///
/// # Errors
///
/// When the text does not parse, or when a declaration is not an object with
/// a string `pointer` that resolves in the template and a string `label`. A
/// declaration that points at nothing would be a wizard page that fills
/// nothing, so it is refused rather than listed.
pub fn declared(template_jsonc: &str) -> Result<Vec<Declared>, Error> {
    let document = Document::parse(template_jsonc)?;
    declared_in(&document.to_json())
}

/// [`declared`], over a template already parsed.
fn declared_in(template: &Json) -> Result<Vec<Declared>, Error> {
    let Some(list) = template
        .get("meta")
        .and_then(|meta| meta.get(PARAMETERS_MEMBER))
    else {
        return Ok(Vec::new());
    };
    let Json::Array(items) = list else {
        return Err(Error::at(
            PARAMETERS_POINTER,
            "`meta.parameters` is a list of {pointer, label}",
        ));
    };
    let mut found: Vec<Declared> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = format!("{PARAMETERS_POINTER}/{index}");
        let text = |member: &str| -> Result<String, Error> {
            match item.get(member) {
                Some(Json::String(text)) => Ok(text.clone()),
                _ => Err(Error::at(
                    format!("{at}/{member}"),
                    format!("a declared parameter's `{member}` is a string"),
                )),
            }
        };
        let pointer = text("pointer")?;
        let label = text("label")?;
        if value_at(template, &pointer)?.is_none() {
            return Err(Error::at(
                format!("{at}/pointer"),
                format!("`{pointer}` names nothing in this template"),
            ));
        }
        found.push(Declared { pointer, label });
    }
    Ok(found)
}

/// The member `meta` holds the declarations under.
const PARAMETERS_MEMBER: &str = "parameters";

/// Where the declarations live, as a pointer.
const PARAMETERS_POINTER: &str = "/meta/parameters";

/// The value an RFC 6901 pointer names in `root`, if it names one.
///
/// # Errors
///
/// When the pointer is not an RFC 6901 pointer at all.
fn value_at<'a>(root: &'a Json, pointer: &str) -> Result<Option<&'a Json>, Error> {
    let mut here = root;
    for token in crate::error::split_pointer(pointer)? {
        let next = match here {
            Json::Object(_) => here.get(&token),
            Json::Array(items) => token
                .parse::<usize>()
                .ok()
                // RFC 6901: an array index has no leading zero and no sign.
                .filter(|_| token == "0" || !token.starts_with('0'))
                .and_then(|index| items.get(index)),
            _ => None,
        };
        let Some(next) = next else {
            return Ok(None);
        };
        here = next;
    }
    Ok(Some(here))
}

/// A value's compact JSON text, as `gp.api.v1.FilledParameter.value` carries
/// it. One spelling for all three sources, so a wizard never shows `3000` for
/// one page and `3000 ` for the next.
fn compact(value: &Json) -> String {
    let mut out = String::new();
    compact_into(value, &mut out);
    out
}

/// [`compact`]'s recursion. Scalars and keys are spelt by the proto crate's
/// one writer, so a string is escaped exactly as everywhere else; only the
/// layout of arrays and objects is this function's.
fn compact_into(value: &Json, out: &mut String) {
    let scalar = |value: &Json| pharmakos_proto::json::write(value).trim_end().to_owned();
    match value {
        Json::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                compact_into(item, out);
            }
            out.push(']');
        }
        Json::Object(entries) => {
            out.push('{');
            for (index, (key, item)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&scalar(&Json::String(key.clone())));
                out.push(':');
                compact_into(item, out);
            }
            out.push('}');
        }
        other => out.push_str(&scalar(other)),
    }
}

/// One parameter's text as JSON, or the refusal that names it.
fn parse_value(parameter: &Parameter) -> Result<Json, Error> {
    pharmakos_proto::json::read(&parameter.value).map_err(|error| {
        Error::at(
            &parameter.name,
            format!(
                "`{}` is not a JSON value: {}",
                parameter.value, error.message
            ),
        )
    })
}

/// Instantiates a template into a playbook.
///
/// [`instantiate`] with explicit values only, and the playbook alone.
///
/// # Errors
///
/// As [`instantiate`].
pub fn instantiate_template(
    template_jsonc: &str,
    parameters: &[Parameter],
) -> Result<String, Error> {
    instantiate(template_jsonc, parameters, &[]).map(|made| made.playbook_jsonc)
}

/// Instantiates a template into a playbook, and says what was filled in.
///
/// The template's own comments and formatting survive: a template's "why"
/// notes are the point of it (spec section 13, "a parameter wizard pre-filled
/// with the built-in operator's suggestion and a 'why' note"), and losing them
/// on instantiation would throw away the teaching half of the library.
///
/// # What is applied, in order
///
/// 1. Each **declared** parameter (`meta.parameters`, decisions-log item 111,
///    decision C1), in declaration order: the `explicit` value naming its
///    pointer if there is one, else the `suggested` one, else nothing — the
///    template's own value stays, formatting and comments included. Explicit
///    beats suggested, always: a suggestion is what the wizard pre-fills and
///    the player's word is what goes in.
/// 2. Each explicit value naming a pointer the template does **not** declare,
///    in the order given. A parameter is an RFC 6902 `replace`, the mechanism
///    every other editor edit uses, so a client that could patch the file
///    after instantiating it may equally name the pointer here; the reply's
///    list is the declared one and does not echo these. A suggestion for an
///    undeclared pointer is **not** applied: a suggestion pre-fills a page,
///    and there is no page for it.
/// 3. `meta.parameters` is **removed** and `kind` becomes `PLAYBOOK`: what
///    comes back is a playbook, so it carries no declaration and no
///    playbook's fingerprint or golden moves because a template grew one.
///
/// The text is **not** canonicalised — the caller verifies it, and the editor
/// opens it — so a template that was hand-formatted opens looking as its
/// author wrote it.
///
/// PLACEHOLDER: the typed parameter catalogue — type, unit, range, choices —
/// is the **owner**'s, at **S6**, with the editor and the library
/// (`gp.v1.TemplateParameter` holds 3 to 15 for it). Until then a value is raw
/// JSON and the verifier refuses what does not fit.
///
/// # Errors
///
/// When the template does not parse, when a declaration is malformed
/// ([`declared`]), when a pointer does not resolve, or when a value is not
/// JSON.
pub fn instantiate(
    template_jsonc: &str,
    explicit: &[Parameter],
    suggested: &[Parameter],
) -> Result<Instance, Error> {
    let document = Document::parse(template_jsonc)?;
    let template = document.to_json();
    let declarations = declared_in(&template)?;

    let mut operations = Vec::with_capacity(explicit.len().saturating_add(2));
    let mut filled: Vec<Filled> = Vec::with_capacity(declarations.len());
    for declaration in &declarations {
        let named = |list: &[Parameter]| {
            list.iter()
                .rev()
                .find(|parameter| parameter.name == declaration.pointer)
                .cloned()
        };
        let (value, from_suggestion) = if let Some(given) = named(explicit) {
            (Some(parse_value(&given)?), false)
        } else if let Some(offered) = named(suggested) {
            (Some(parse_value(&offered)?), true)
        } else {
            (None, false)
        };
        let shown = match value.as_ref() {
            Some(applied) => {
                operations.push(Operation::with_value(
                    Op::Replace,
                    declaration.pointer.clone(),
                    crate::jsonc::Fragment::from_json(applied),
                ));
                compact(applied)
            }
            // Resolved by `declared_in` already; `Null` only if it vanished,
            // which a parsed document cannot do.
            None => value_at(&template, &declaration.pointer)?.map_or_else(String::new, compact),
        };
        filled.push(Filled {
            pointer: declaration.pointer.clone(),
            label: declaration.label.clone(),
            value: shown,
            suggested: from_suggestion,
        });
    }
    for parameter in explicit {
        if declarations
            .iter()
            .any(|declaration| declaration.pointer == parameter.name)
        {
            continue;
        }
        operations.push(Operation::with_value(
            Op::Replace,
            parameter.name.clone(),
            crate::jsonc::Fragment::from_json(&parse_value(parameter)?),
        ));
    }
    if template
        .get("meta")
        .and_then(|meta| meta.get(PARAMETERS_MEMBER))
        .is_some()
    {
        operations.push(Operation::plain(Op::Remove, PARAMETERS_POINTER));
    }
    operations.push(Operation::with_value(
        Op::Add,
        "/kind",
        crate::jsonc::Fragment::from_json(&Json::String("PLAYBOOK".to_owned())),
    ));
    let (instantiated, _undo) = apply(&document, &Patch::new(operations))?;
    Ok(Instance {
        playbook_jsonc: instantiated.to_text(),
        parameters: filled,
    })
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

    /// A template that declares two parameters, the second of them twice as
    /// deep as the first, with a comment inside the declared value.
    const DECLARING: &str = concat!(
        "{\"schema_version\":{\"major\":1},\n",
        " \"meta\":{\"title\":\"Hold\",\"author_kind\":\"BUILTIN\",\n",
        "  \"parameters\":[\n",
        "   {\"pointer\":\"/declarative/route/0/hold/ms\",\"label\":\"How long\"},\n",
        "   {\"pointer\":\"/fallback/hold/at\",\"label\":\"Where\"}]},\n",
        " \"declarative\":{\"route\":[{\"label\":\"h\",\"hold\":{\"ms\": 3000}}]},\n",
        " \"on_death\":{\"on_respawn\":\"CONTINUE\"},\n",
        " \"fallback\":{\"hold\":{\"at\":{ /* why here */ \"safest\":{}}}},\n",
        " \"kind\":\"TEMPLATE\"}\n"
    );

    fn parameter(name: &str, value: &str) -> Parameter {
        Parameter {
            name: name.to_owned(),
            value: value.to_owned(),
        }
    }

    #[test]
    fn a_template_declares_its_parameters_in_order() {
        let found = super::declared(DECLARING).expect("two declarations");
        let pointers: Vec<&str> = found.iter().map(|each| each.pointer.as_str()).collect();
        assert_eq!(
            pointers,
            ["/declarative/route/0/hold/ms", "/fallback/hold/at"]
        );
        assert_eq!(
            found.first().map(|each| each.label.as_str()),
            Some("How long")
        );
        assert!(
            super::declared(TEMPLATE)
                .expect("a template with no declaration")
                .is_empty()
        );
    }

    #[test]
    fn a_declaration_that_points_at_nothing_is_refused() {
        let broken = DECLARING.replace("/fallback/hold/at", "/fallback/shadow");
        let error = super::declared(&broken).expect_err("points at nothing");
        assert_eq!(error.pointer, "/meta/parameters/1/pointer");
    }

    #[test]
    fn every_declared_parameter_comes_back_in_order_with_where_its_value_came_from() {
        let made = super::instantiate(
            DECLARING,
            &[],
            &[parameter("/declarative/route/0/hold/ms", "5000")],
        )
        .expect("instantiated");
        let seen: Vec<(&str, &str, bool)> = made
            .parameters
            .iter()
            .map(|each| (each.pointer.as_str(), each.value.as_str(), each.suggested))
            .collect();
        assert_eq!(
            seen,
            [
                ("/declarative/route/0/hold/ms", "5000", true),
                ("/fallback/hold/at", "{\"safest\":{}}", false),
            ]
        );
        assert!(
            made.playbook_jsonc.contains("/* why here */"),
            "a parameter left at the template's own value keeps its comment: {}",
            made.playbook_jsonc
        );
    }

    #[test]
    fn an_explicit_value_beats_a_suggestion() {
        let made = super::instantiate(
            DECLARING,
            &[parameter("/declarative/route/0/hold/ms", "7000")],
            &[parameter("/declarative/route/0/hold/ms", "5000")],
        )
        .expect("instantiated");
        let first = made.parameters.first().expect("the first declaration");
        assert_eq!((first.value.as_str(), first.suggested), ("7000", false));
        assert!(
            made.playbook_jsonc.contains("7000"),
            "{}",
            made.playbook_jsonc
        );
        assert!(
            !made.playbook_jsonc.contains("5000"),
            "{}",
            made.playbook_jsonc
        );
    }

    #[test]
    fn a_suggestion_for_a_pointer_the_template_does_not_declare_is_not_applied() {
        let made = super::instantiate(DECLARING, &[], &[parameter("/meta/title", "\"Mine\"")])
            .expect("instantiated");
        assert!(
            made.playbook_jsonc.contains("\"Hold\""),
            "{}",
            made.playbook_jsonc
        );
    }

    #[test]
    fn instantiating_removes_the_declaration_and_nothing_else_of_the_meta() {
        let playbook = instantiate_template(DECLARING, &[]).expect("instantiated");
        assert!(!playbook.contains("parameters"), "{playbook}");
        assert!(!playbook.contains("TEMPLATE"), "{playbook}");
        let canonical = crate::canonical::canonicalise_text(&playbook).expect("canonical");
        let meta = canonical.playbook.meta.expect("a meta block");
        assert_eq!(meta.title, "Hold");
        assert!(meta.parameters.is_empty());
    }

    #[test]
    fn a_folder_that_is_not_there_is_an_error_and_not_an_empty_library() {
        assert!(list(Path::new("no/such/folder")).is_err());
        assert!(read(Path::new("no/such/folder"), "nothing").is_err());
    }
}

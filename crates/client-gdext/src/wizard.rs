// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The template wizard's and the rule list's half of the editor: what the gateway's
//! `list_templates`, `instantiate_template` and `render_plan` answers mean, as data the
//! panel draws (skeleton plan T19, pull request 2; `docs/design/skeleton-plan-w6-notes.md`
//! section A4, as decisions-log item 112 amends it).
//!
//! # Generic over what each template declares
//!
//! Nothing here names a template, a pointer or a label. The pages are the
//! `instantiate_template{suggested: true}` answer's `parameters`, in the order the gateway
//! listed them (which is the template's declaration order), each shown as it came: the
//! label, the value as raw JSON text (a duration stays game milliseconds; unit display is
//! S6's), a mark where the value is the built-in operator's suggestion, and the answer's
//! "why" as it came. So a template added to `library/`, or a parameter added to one, needs
//! no change here.
//!
//! # What the client never does to a value
//!
//! It never parses, checks or converts one. An edited value is sent **exactly as typed**,
//! as an explicit `{name: pointer, value: text}`, with `suggested` still true, so every
//! page the player did not touch keeps the operator's value and its mark (making every page
//! explicit would lose the marks at the first edit). A value the gateway refuses is shown as
//! the gateway wrote the refusal. The playbook the wizard hands the editor is the gateway's
//! `playbook_jsonc`, byte for byte.
//!
//! # The rule list
//!
//! `render_plan`'s prose, split at line ends, one read-only row per line, drawn as it came.
//! The line layout is the gateway's contract (`gp.api.v1.RenderPlanResponse`'s comment:
//! one rule, step or block per line); no sentence is parsed here, because chips are S3's
//! (decision C3).

use pharmakos_proto::gp::api::v1::{
    InstantiateTemplateResponse, ListTemplatesResponse, RenderPlanResponse,
};
use pharmakos_proto::json::{self, Json};

use crate::enums;
use crate::error::BridgeError;
use crate::view::split_footer;

/// One template `list_templates` offers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TemplateRow {
    /// The id `instantiate_template` takes.
    pub id: String,
    /// Its title, from the file.
    pub title: String,
    /// Its one-sentence summary, from the file.
    pub summary: String,
    /// The guided first pick's mark, as sent (S3's plan, decision 5 and E25): the gateway's
    /// sentence saying why to start here, present exactly when the gateway marked this
    /// template (`guided_pick`). The gateway marks one template, for a Probation match's
    /// human seat in its first Lull, and none anywhere else; the client names none.
    pub guided: Option<String>,
}

/// Why the gateway refused some of the library's files, as `list_templates` sent it
/// (`gp.api.v1.RefusedTemplates.Reason`, S3's `leftout`). Each names the words the panel says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefusedReason {
    /// A file the gateway could not open or read.
    Unreadable,
    /// A file larger than the gateway reads.
    TooLarge,
    /// A file that is not UTF-8 text, or not a playbook.
    NotAPlaybook,
}

impl RefusedReason {
    /// The name GDScript reads, and the key of its words in `strings.gd`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unreadable => "unreadable",
            Self::TooLarge => "too_large",
            Self::NotAPlaybook => "not_a_playbook",
        }
    }
}

/// Library files the listing refused for one reason, with the count as the gateway sent it:
/// the client counts and sums nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RefusedRow {
    /// Why.
    pub reason: RefusedReason,
    /// How many files, as sent: at least 1.
    pub count: u32,
}

/// One `list_templates` answer: the templates offered, and the library files refused, each as
/// the gateway sent it, in its order.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TemplateList {
    /// The templates, in the gateway's order.
    pub rows: Vec<TemplateRow>,
    /// The refused files, one entry per reason, in the gateway's order; empty when none.
    pub refused: Vec<RefusedRow>,
}

/// One wizard page: one declared parameter, as the gateway filled it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Page {
    /// Where the value goes: the pointer the template declares. Opaque to the client; it is
    /// sent back as the explicit value's `name` and never read.
    pub pointer: String,
    /// What the template calls it.
    pub label: String,
    /// The value that was applied, as the compact JSON text the gateway wrote.
    pub value: String,
    /// Whether the value is the built-in operator's suggestion.
    pub suggested: bool,
}

/// One `instantiate_template` answer: the playbook, the pages and the why.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Instance {
    /// The playbook, JSONC, exactly as the gateway wrote it.
    pub playbook_jsonc: String,
    /// Every declared parameter, in the order the gateway listed them.
    pub pages: Vec<Page>,
    /// The operator's reason, as it came; empty when there is none.
    pub why: String,
}

/// The wizard as the editor holds it: which template, what the player typed, and the
/// gateway's last answer.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Wizard {
    /// The template the wizard is open on.
    pub template_id: String,
    /// The values the player typed, as `(pointer, text)`, in the order first typed. Only
    /// these are sent explicitly.
    pub explicit: Vec<(String, String)>,
    /// Which ask is the newest; an answer to an older one is not shown.
    pub asked: u64,
    /// Which ask the instance on screen answered.
    pub answered: u64,
    /// The last answer the gateway gave.
    pub instance: Option<Instance>,
    /// The gateway's refusal of the newest ask, as `CODE: message`, when it refused.
    pub refusal: Option<String>,
}

impl Wizard {
    /// A wizard on `template_id`, asked for the first time as ask `asked`.
    #[must_use]
    pub fn new(template_id: &str, asked: u64) -> Self {
        Self {
            template_id: template_id.to_owned(),
            explicit: Vec::new(),
            asked,
            answered: 0,
            instance: None,
            refusal: None,
        }
    }

    /// Whether the instance on screen answers the newest ask: only then can it be used.
    #[must_use]
    pub fn current(&self) -> bool {
        self.instance.is_some() && self.answered == self.asked && self.refusal.is_none()
    }

    /// Records a typed value for the page at `pointer`, replacing an earlier one. Returns
    /// whether a page at that pointer is on screen.
    pub fn set(&mut self, pointer: &str, text: &str) -> bool {
        let on_screen = self
            .instance
            .as_ref()
            .is_some_and(|instance| instance.pages.iter().any(|page| page.pointer == pointer));
        if !on_screen {
            return false;
        }
        if let Some(slot) = self.explicit.iter_mut().find(|(named, _)| named == pointer) {
            text.clone_into(&mut slot.1);
        } else {
            self.explicit.push((pointer.to_owned(), text.to_owned()));
        }
        true
    }

    /// The `instantiate_template` params for the newest ask.
    #[must_use]
    pub fn params(&self) -> Json {
        instantiate_params(&self.template_id, &self.explicit)
    }
}

/// `instantiate_template`'s params: the template, the typed values exactly as typed, and
/// `suggested: true`, so every page not named keeps the operator's value.
#[must_use]
pub fn instantiate_params(template_id: &str, explicit: &[(String, String)]) -> Json {
    let parameters = explicit
        .iter()
        .map(|(pointer, text)| {
            Json::Object(vec![
                ("name".to_owned(), Json::String(pointer.clone())),
                ("value".to_owned(), Json::String(text.clone())),
            ])
        })
        .collect();
    Json::Object(vec![
        (
            "template_id".to_owned(),
            Json::String(template_id.to_owned()),
        ),
        ("parameters".to_owned(), Json::Array(parameters)),
        ("suggested".to_owned(), Json::Bool(true)),
    ])
}

/// An `instantiate_template` result, footer and all, as an [`Instance`].
///
/// # Errors
///
/// [`BridgeError::Schema`] when the result is not a `gp.api.v1.InstantiateTemplateResponse`.
pub fn instance_of(result: &Json) -> Result<Instance, BridgeError> {
    let response: InstantiateTemplateResponse =
        json::decode_json(&body(result, "gp.api.v1.InstantiateTemplateResponse"))?;
    Ok(Instance {
        playbook_jsonc: response.playbook_jsonc,
        pages: response
            .parameters
            .into_iter()
            .map(|parameter| Page {
                pointer: parameter.pointer,
                label: parameter.label,
                value: parameter.value,
                suggested: parameter.suggested,
            })
            .collect(),
        why: response.why,
    })
}

/// An [`Instance`] from a whole JSON-RPC answer's text, or from its `result` alone: what the
/// hostless wizard scene draws from `godot/fixtures/instantiate_suggested.json` with no
/// gateway behind it.
///
/// # Errors
///
/// [`BridgeError::Rpc`] for a refusal, and the errors of [`instance_of`].
pub fn instance_of_text(text: &str) -> Result<Instance, BridgeError> {
    let frame = json::read(text)?;
    if frame.get("error").is_some() {
        return Err(BridgeError::Rpc(
            "the answer is a refusal, not an instance".to_owned(),
        ));
    }
    match frame.get("result") {
        Some(result) => instance_of(result),
        None => instance_of(&frame),
    }
}

/// A `list_templates` result as rows and refused files, in the gateway's order.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the result is not a `gp.api.v1.ListTemplatesResponse`, or a
/// refused entry names no reason this client knows or counts no file.
pub fn templates_of(result: &Json) -> Result<TemplateList, BridgeError> {
    let response: ListTemplatesResponse =
        json::decode_json(&body(result, "gp.api.v1.ListTemplatesResponse"))?;
    // The list is one page: the gateway lists the whole library on it. A cursor would mean
    // templates this client was not shown, so the list is refused rather than drawn short.
    if !response.next_cursor.is_empty() {
        return Err(unreadable_list(
            "/next_cursor",
            "the template list says more templates follow, and it is read as one page",
        ));
    }
    let refused = refused_of(&response.refused)?;
    let mut rows = Vec::with_capacity(response.templates.len());
    let mut marked = 0_usize;
    for (index, template) in response.templates.into_iter().enumerate() {
        let at = |field: &str| format!("/templates/{index}/{field}");
        // The reason is set exactly when the mark is (the schema's comment): a mark with no
        // sentence, or a sentence with no mark, is a list this client cannot draw as sent.
        let guided = match (template.guided_pick, template.guided_pick_reason.is_empty()) {
            (true, false) => Some(template.guided_pick_reason),
            (false, true) => None,
            (true, true) => {
                return Err(unreadable_list(
                    &at("guided_pick_reason"),
                    "a template is marked as the first pick with no sentence saying why",
                ));
            }
            (false, false) => {
                return Err(unreadable_list(
                    &at("guided_pick"),
                    "a template carries the first pick's sentence and no mark",
                ));
            }
        };
        if guided.is_some() {
            marked = marked.saturating_add(1);
            if marked > 1 {
                return Err(unreadable_list(
                    &at("guided_pick"),
                    "a second template is marked as the first pick, and the gateway marks one",
                ));
            }
        }
        rows.push(TemplateRow {
            id: template.template_id,
            title: template.title,
            summary: template.summary,
            guided,
        });
    }
    Ok(TemplateList { rows, refused })
}

/// The refused entries as sent, each read to a reason this client has words for. A reason it
/// does not name (a value from a newer schema, or the never-sent `REASON_UNSPECIFIED`) and a
/// count of none are refusals of the list, never drawn as another reason or dropped.
fn refused_of(
    entries: &[pharmakos_proto::gp::api::v1::RefusedTemplates],
) -> Result<Vec<RefusedRow>, BridgeError> {
    use pharmakos_proto::gp::api::v1::refused_templates::Reason;
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let reason = match Reason::try_from(entry.reason) {
                Ok(Reason::Unreadable) => RefusedReason::Unreadable,
                Ok(Reason::TooLarge) => RefusedReason::TooLarge,
                Ok(Reason::NotAPlaybook) => RefusedReason::NotAPlaybook,
                Ok(Reason::Unspecified) | Err(_) => {
                    return Err(unreadable_list(
                        &format!("/refused/{index}/reason"),
                        "a refused library file has no reason this client knows",
                    ));
                }
            };
            if entry.count == 0 {
                return Err(unreadable_list(
                    &format!("/refused/{index}/count"),
                    "a refused entry counts no file",
                ));
            }
            Ok(RefusedRow {
                reason,
                count: entry.count,
            })
        })
        .collect()
}

/// A template list this client cannot draw as sent: a schema refusal at `pointer`.
fn unreadable_list(pointer: &str, message: &str) -> BridgeError {
    BridgeError::Schema {
        pointer: pointer.to_owned(),
        message: message.to_owned(),
    }
}

/// A `list_templates` answer's text (a whole JSON-RPC answer, or its result) read with no
/// gateway behind it, as [`templates_of`] reads one: what the hostless template-list shot draws.
///
/// # Errors
///
/// As [`templates_of`], and [`BridgeError::Rpc`] for a refusal in place of an answer.
pub fn templates_of_text(text: &str) -> Result<TemplateList, BridgeError> {
    let frame = json::read(text)?;
    if frame.get("error").is_some() {
        return Err(BridgeError::Rpc(
            "the answer is a refusal, not a template list".to_owned(),
        ));
    }
    match frame.get("result") {
        Some(result) => templates_of(result),
        None => templates_of(&frame),
    }
}

/// A `render_plan` result's prose, one row per line, each line exactly as it came.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the result is not a `gp.api.v1.RenderPlanResponse`.
pub fn prose_of(result: &Json) -> Result<Vec<String>, BridgeError> {
    let response: RenderPlanResponse =
        json::decode_json(&body(result, "gp.api.v1.RenderPlanResponse"))?;
    Ok(prose_rows(&response.prose))
}

/// Prose split at its line ends: one row per line, the text of each exactly as it came
/// (indent included). Nothing is merged, dropped or reworded.
#[must_use]
pub fn prose_rows(prose: &str) -> Vec<String> {
    prose.lines().map(str::to_owned).collect()
}

/// The result with its footer set aside and its enum values spelt as the schema spells
/// them, ready for the typed decode.
fn body(result: &Json, full_name: &str) -> Json {
    let (answer, _) = split_footer(result);
    enums::canonical(full_name, &answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_value_replaces_an_earlier_one_for_its_page_and_nothing_else() {
        let mut wizard = Wizard::new("t", 1);
        assert!(!wizard.set("/a", "1"), "no page is on screen yet");
        wizard.instance = Some(Instance {
            pages: vec![Page {
                pointer: "/a".to_owned(),
                label: "A".to_owned(),
                value: "0".to_owned(),
                suggested: true,
            }],
            ..Instance::default()
        });
        assert!(wizard.set("/a", "1"));
        assert!(wizard.set("/a", " 2 "));
        assert!(!wizard.set("/b", "3"), "not a page on screen");
        assert_eq!(wizard.explicit, vec![("/a".to_owned(), " 2 ".to_owned())]);
    }

    #[test]
    fn prose_is_split_at_line_ends_and_nothing_else() {
        assert_eq!(
            prose_rows("Title\n\nRoute\n  1. go\n    guard\n"),
            vec!["Title", "", "Route", "  1. go", "    guard"]
        );
    }

    /// The gateway's `list_templates_guided` golden, read where it is committed: a Probation
    /// match's first Lull, one template marked with the gateway's sentence.
    const GUIDED: &str =
        include_str!("../../../tests/golden/gateway/list_templates_guided/expected.response.json");

    fn result_of(text: &str) -> Json {
        json::read(text)
            .expect("json")
            .get("result")
            .expect("a result")
            .clone()
    }

    #[test]
    fn the_guided_pick_is_the_one_template_the_gateway_marked_with_its_sentence() {
        let rows = templates_of(&result_of(GUIDED))
            .expect("the golden reads")
            .rows;
        let marked: Vec<(&str, &str)> = rows
            .iter()
            .filter_map(|row| row.guided.as_deref().map(|why| (row.id.as_str(), why)))
            .collect();
        assert_eq!(marked.len(), 1, "{rows:?}");
        let golden = json::read(GUIDED).expect("json");
        let Some(Json::Array(templates)) = golden.get("result").and_then(|r| r.get("templates"))
        else {
            panic!("the golden lists templates");
        };
        let sent = templates
            .iter()
            .find(|template| matches!(template.get("guided_pick"), Some(Json::Bool(true))))
            .expect("the golden marks one");
        let Some(Json::String(id)) = sent.get("template_id") else {
            panic!("an id");
        };
        let Some(Json::String(why)) = sent.get("guided_pick_reason") else {
            panic!("a reason");
        };
        assert_eq!(marked, [(id.as_str(), why.as_str())], "drawn as sent");
        assert_eq!(
            rows.len(),
            templates.len(),
            "every other template stays offered"
        );
    }

    #[test]
    fn a_list_marked_twice_or_marked_without_a_reason_is_unreadable() {
        let refused = [
            (
                r#"{"templates":[{"template_id":"a","guided_pick":true}]}"#,
                "/templates/0/guided_pick_reason",
            ),
            (
                r#"{"templates":[{"template_id":"a","guided_pick_reason":"why"}]}"#,
                "/templates/0/guided_pick",
            ),
            (
                r#"{"templates":[{"template_id":"a","guided_pick":true,"guided_pick_reason":"x"},{"template_id":"b","guided_pick":true,"guided_pick_reason":"y"}]}"#,
                "/templates/1/guided_pick",
            ),
            (
                r#"{"templates":[{"template_id":"a"}],"next_cursor":"more"}"#,
                "/next_cursor",
            ),
        ];
        for (text, at) in refused {
            match templates_of(&json::read(text).expect("json")) {
                Err(BridgeError::Schema { pointer, .. }) => assert_eq!(pointer, at, "{text}"),
                other => panic!("{text}: {other:?}"),
            }
        }
        let none =
            templates_of(&json::read(r#"{"templates":[{"template_id":"a"}]}"#).expect("json"))
                .expect("an unmarked list reads");
        assert_eq!(none.rows.first().and_then(|row| row.guided.clone()), None);
        assert!(none.refused.is_empty(), "no refused entry, none drawn");
    }

    /// The refused library files are read as sent (S3's `leftout`): each reason to its own,
    /// in the gateway's order, the count as it came, nothing summed; a reason the client does
    /// not know, the unspecified one, or a count of none refuses the list.
    #[test]
    fn the_refused_files_are_read_as_sent_and_an_unknown_reason_is_refused() {
        let list = templates_of(
            &json::read(
                r#"{"templates":[],"refused":[{"reason":"unreadable","count":3},{"reason":"too_large","count":1},{"reason":"not_a_playbook","count":2}]}"#,
            )
            .expect("json"),
        )
        .expect("reads");
        assert_eq!(
            list.refused,
            vec![
                RefusedRow {
                    reason: RefusedReason::Unreadable,
                    count: 3
                },
                RefusedRow {
                    reason: RefusedReason::TooLarge,
                    count: 1
                },
                RefusedRow {
                    reason: RefusedReason::NotAPlaybook,
                    count: 2
                },
            ]
        );
        for (text, at) in [
            (
                r#"{"refused":[{"reason":"reason_unspecified","count":1}]}"#,
                Some("/refused/0/reason"),
            ),
            (r#"{"refused":[{"count":1}]}"#, Some("/refused/0/reason")),
            (
                r#"{"refused":[{"reason":"too_large","count":0}]}"#,
                Some("/refused/0/count"),
            ),
            // A name the schema does not have does not decode at all.
            (r#"{"refused":[{"reason":"sideways","count":1}]}"#, None),
        ] {
            match templates_of(&json::read(text).expect("json")) {
                Err(BridgeError::Schema { pointer, .. }) if at.is_some() => {
                    assert_eq!(Some(pointer.as_str()), at, "{text}");
                }
                Err(_) if at.is_none() => {}
                other => panic!("{text}: {other:?}"),
            }
        }
        // A numeric value past the schema's names is refused too, never drawn as another.
        let numeric = r#"{"refused":[{"reason":9,"count":1}]}"#;
        assert!(
            templates_of(&json::read(numeric).expect("json")).is_err(),
            "{numeric}"
        );
        assert_eq!(RefusedReason::NotAPlaybook.name(), "not_a_playbook");
    }
}

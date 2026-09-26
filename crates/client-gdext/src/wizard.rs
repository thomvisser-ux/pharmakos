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
/// render-only wizard scene draws from `godot/fixtures/instantiate_suggested.json` with no
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

/// A `list_templates` result as rows, in the gateway's order.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the result is not a `gp.api.v1.ListTemplatesResponse`.
pub fn templates_of(result: &Json) -> Result<Vec<TemplateRow>, BridgeError> {
    let response: ListTemplatesResponse =
        json::decode_json(&body(result, "gp.api.v1.ListTemplatesResponse"))?;
    Ok(response
        .templates
        .into_iter()
        .map(|template| TemplateRow {
            id: template.template_id,
            title: template.title,
            summary: template.summary,
        })
        .collect())
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
}

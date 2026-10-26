// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Targeting's surfaces as the editor draws them: the features the map lists, the chip a
//! description shows, and the Lull's "this round" sentence (S1's plan, task `ui`;
//! `docs/design/targeting.md`, "Surfaces"; decisions-log items 127 (12) and 133 (3) (i)).
//!
//! **Every answer is the gateway's.** What a description reads now, how far it is, what
//! comes next, why it reads nothing, and what the playbook does this round are
//! `resolve_refs`', `get_map_summary`'s and `get_briefing`'s; "nearest" is ranked in the sim
//! and the gateway and never here (AGENTS.md section 3 rule 2, "never ranks", and rule 4).
//! What is left for this module is marshalling:
//!
//! * reading each answer into rows the panel draws ([`features_of`], [`chips_of`]);
//! * spelling a feature's name `vent_<x>_<y>` back into its kind and column, so the panel
//!   can say "Heat vent (120, 88)" through the string table ([`FeatureName`]) — the digits
//!   are kept as the text they were, never read as numbers;
//! * reading, out of the player's own text, whether a reference is a name or a description,
//!   so the chip says which (a lax walk, like the route's: it decides nothing, and the
//!   verdict on the file stays the verifier's);
//! * finding the gateway's "This round: ..." sentence at the end of the briefing's prose,
//!   where it rides until v1.1 gives it a field (item 133 (3) (i)).

use pharmakos_proto::gp::api::v1::map_feature::Kind;
use pharmakos_proto::gp::api::v1::{GetMapSummaryResponse, ResolveRefsResponse};
use pharmakos_proto::json::{self, Json};

use crate::enums;
use crate::error::BridgeError;
use crate::view::split_footer;

/// The words the gateway's "this round" sentence starts with
/// (`crates/gateway/src/strings.rs`, `this_round`). The client finds the sentence by them;
/// `tests/targeting.rs` reads the gateway's source and fails if they move there.
pub const THIS_ROUND_LEAD: &str = "This round: ";

/// What kind of feature a name or an answer names.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum FeatureKind {
    /// A heat vent, `vent_<x>_<y>`.
    Vent,
    /// A scrap seam, `seam_<x>_<y>`.
    Seam,
}

impl FeatureKind {
    /// The name GDScript looks the kind's words up by, and the prefix of a feature's id.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Vent => "vent",
            Self::Seam => "seam",
        }
    }

    /// The kind a name or a wire value spells: `vent`, `seam`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "vent" => Some(Self::Vent),
            "seam" => Some(Self::Seam),
            _ => None,
        }
    }
}

/// A feature's name spelt back into its parts: `vent_120_88` is a vent at column
/// `(120, 88)`. The column stays the text the name carried; the panel puts it into a
/// sentence and computes nothing with it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FeatureName {
    /// The name as the gateway wrote it.
    pub id: String,
    /// Vent or seam.
    pub kind: FeatureKind,
    /// The anchor column's x, as the name spells it.
    pub x: String,
    /// The anchor column's y, as the name spells it.
    pub y: String,
}

impl FeatureName {
    /// The parts of `id`, `vent_<x>_<y>` or `seam_<x>_<y>`. `None` for anything else, which
    /// the panel shows as the raw id rather than a column it cannot read.
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        let mut parts = id.split('_');
        let kind = FeatureKind::from_name(parts.next()?)?;
        let x = parts.next()?;
        let y = parts.next()?;
        let digits = |part: &str| {
            // An optional leading `-`: one byte, so the split is on a boundary.
            let (_, unsigned) = part.split_at(usize::from(part.starts_with('-')));
            !unsigned.is_empty() && unsigned.bytes().all(|byte| byte.is_ascii_digit())
        };
        if parts.next().is_some() || !digits(x) || !digits(y) {
            return None;
        }
        Some(Self {
            id: id.to_owned(),
            kind,
            x: x.to_owned(),
            y: y.to_owned(),
        })
    }
}

/// One feature as `get_map_summary` lists it: what the editor's vent click needs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Feature {
    /// Its name, `vent_<x>_<y>`, as a playbook writes it.
    pub id: String,
    /// Vent or seam.
    pub kind: FeatureKind,
    /// The generation anchor column, sim axes (x east, y north).
    pub x: i32,
    /// The generation anchor column, sim axes (x east, y north).
    pub y: i32,
    /// Whether one of the seat's own living beacons covers it, as the gateway said.
    pub covered: bool,
    /// Whether the feature is still there, as the gateway said.
    pub live: bool,
}

/// The features a `get_map_summary` answer lists, in its order. A feature of a kind this
/// build has no name for is left out: the editor offers nothing it cannot write.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the answer is not a `GetMapSummaryResponse`.
pub fn features_of(result: &Json) -> Result<Vec<Feature>, BridgeError> {
    let (body, _) = split_footer(result);
    let response: GetMapSummaryResponse =
        json::decode_json(&enums::canonical("gp.api.v1.GetMapSummaryResponse", &body))?;
    Ok(response
        .features
        .into_iter()
        .filter_map(|feature| {
            let kind = match Kind::try_from(feature.kind) {
                Ok(Kind::Vent) => FeatureKind::Vent,
                Ok(Kind::Seam) => FeatureKind::Seam,
                _ => return None,
            };
            Some(Feature {
                id: feature.feature_id,
                kind,
                x: feature.x,
                y: feature.y,
                covered: feature.covered,
                live: feature.live,
            })
        })
        .collect())
}

/// Which site arm a reference sits under, which says where "nearest" is measured from
/// (`gp.v1.FeatureRef.Rank`'s comment; `docs/design/targeting.md`, "Nearest").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChipArm {
    /// `covering`: from the commander's column when the step starts.
    Covering,
    /// `on`: from the target beacon's anchor, or the site `covering` chose.
    On,
    /// A pointer this build cannot place.
    Other,
}

impl ChipArm {
    /// The name GDScript looks the chip's closing words up by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Covering => "covering",
            Self::On => "on",
            Self::Other => "other",
        }
    }
}

/// What the player wrote at a reference: a name, a description, or "the feature this
/// beacon was placed to cover".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChipForm {
    /// `feature_id`: a name, read when the step starts.
    Name,
    /// `vent` or `seam`: a description, ranked when the step starts.
    Description,
    /// `covered {}`: binds as a name at deploy.
    Covered,
    /// The text could not be read at the pointer.
    Unknown,
}

impl ChipForm {
    /// The name GDScript looks the chip's closing words up by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Description => "description",
            Self::Covered => "covered",
            Self::Unknown => "unknown",
        }
    }
}

/// One candidate on a chip: a feature and the travel to it, as the gateway answered.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChipFeature {
    /// The feature's name.
    pub id: String,
    /// Its parts, when the name is one this build can spell.
    pub name: Option<FeatureName>,
    /// Travel from the origin, game milliseconds, exactly as the gateway answered.
    pub travel_ms: i32,
}

/// One chip: what one reference in the file reads now (targeting.md, "Surfaces": "now: Heat
/// vent (120, 88) · 14 s · next (150, 20) · 16 s — nearest by travel from the commander,
/// read when the step starts").
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Chip {
    /// The reference's JSON Pointer, as the gateway wrote it.
    pub pointer: String,
    /// The route step it is in, numbered from 1 as `render_plan` numbers steps; `None` for
    /// a reference outside the route.
    pub step: Option<usize>,
    /// The site arm it sits under.
    pub arm: ChipArm,
    /// What the player wrote there.
    pub form: ChipForm,
    /// What it reads now; `None` when it reads nothing.
    pub now: Option<ChipFeature>,
    /// The candidate after it in the gateway's rank order, when there is one.
    pub next: Option<ChipFeature>,
    /// Why it reads nothing, spelt as the feed spells a step failure; empty when it reads.
    pub failure: String,
    /// How many features matched before reachability was asked.
    pub matched: u32,
    /// Whether a route reaches what it reads from its origin, as the gateway answered
    /// (`ResolvedRef.reachable`): false when it reads nothing, or reads a named feature the
    /// origin cannot reach, and then `now`'s travel time means nothing and is not drawn.
    pub reachable: bool,
}

/// The chips of one `resolve_refs` answer, in the file's order, for the playbook `text`
/// they were asked about.
///
/// "Next" is the candidate listed after the one the reference reads: the gateway lists the
/// candidates in rank order and the pick is the first with a legal site, so the pick need
/// not be first ([`pharmakos_proto::gp::api::v1::ResolvedRef`]); nothing here ranks.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the answer is not a `ResolveRefsResponse`.
pub fn chips_of(result: &Json, text: &str) -> Result<Vec<Chip>, BridgeError> {
    let (body, _) = split_footer(result);
    let response: ResolveRefsResponse =
        json::decode_json(&enums::canonical("gp.api.v1.ResolveRefsResponse", &body))?;
    let document = crate::editor::lax_document(text);
    Ok(response
        .refs
        .into_iter()
        .map(|reference| {
            let now = (!reference.feature_id.is_empty()).then(|| ChipFeature {
                name: FeatureName::parse(&reference.feature_id),
                id: reference.feature_id.clone(),
                travel_ms: reference.travel_ms,
            });
            let pick = reference
                .candidates
                .iter()
                .position(|candidate| candidate.feature_id == reference.feature_id);
            let after = pick.map_or(0, |index| index.saturating_add(1));
            let next = reference
                .candidates
                .get(after..)
                .and_then(|rest| {
                    rest.iter()
                        .find(|candidate| candidate.feature_id != reference.feature_id)
                })
                .filter(|_| now.is_some())
                .map(|candidate| ChipFeature {
                    id: candidate.feature_id.clone(),
                    name: FeatureName::parse(&candidate.feature_id),
                    travel_ms: candidate.travel_ms,
                });
            let form = document
                .as_ref()
                .and_then(|document| at_pointer(document, &reference.pointer))
                .map_or(ChipForm::Unknown, form_of);
            Chip {
                step: step_of(&reference.pointer),
                arm: arm_of(&reference.pointer),
                form,
                now,
                next,
                failure: reference.failure,
                matched: reference.matched,
                reachable: reference.reachable,
                pointer: reference.pointer,
            }
        })
        .collect())
}

/// Whether the playbook `text` holds any feature reference (`covering` or `on`) anywhere,
/// so `resolve_refs` is worth asking. Read laxly: a text this walk cannot read holds none,
/// and the verifier still has the last word on it.
#[must_use]
pub fn has_feature_refs(text: &str) -> bool {
    fn walk(value: &Json) -> bool {
        match value {
            Json::Object(entries) => entries
                .iter()
                .any(|(key, value)| key == "covering" || key == "on" || walk(value)),
            Json::Array(items) => items.iter().any(walk),
            _ => false,
        }
    }
    crate::editor::lax_document(text).is_some_and(|document| walk(&document))
}

/// What an Attack beacon's `target` names, as the player wrote it (`gp.v1.AttackTarget`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AttackAim {
    /// A fixed voxel, sim axes (x east, y north, z up).
    Voxel([i32; 3]),
    /// An enemy beacon by the name the player wrote (`e_NN`), which the map draws over the
    /// beacon the view shows under that id, if it shows one.
    Beacon(String),
    /// An enemy beacon chosen by a selector when the attack runs: the sim's ranking, so
    /// nothing is drawn on the map.
    Selector,
    /// "The enemy commander", from the seat's last sighting: the sim's reading, so nothing
    /// is drawn on the map.
    EnemyCommander,
    /// A target this lax walk cannot read; the verifier has the last word on it.
    Unknown,
}

impl AttackAim {
    /// The name GDScript looks the target's words up by.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Voxel(_) => "voxel",
            Self::Beacon(_) => "beacon",
            Self::Selector => "selector",
            Self::EnemyCommander => "enemy_commander",
            Self::Unknown => "unknown",
        }
    }
}

/// One Attack beacon's target, found in the player's text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AttackMark {
    /// The `target`'s JSON Pointer in the file.
    pub pointer: String,
    /// The route step it is written in, numbered from 1 as `render_plan` numbers steps;
    /// `None` outside the route.
    pub step: Option<usize>,
    /// What it names.
    pub aim: AttackAim,
}

/// Every Attack beacon's target in the playbook `text`, in the file's order: an `attack`
/// object's `target`, wherever the text writes one (a placement's initial mandate, a
/// visit's settings row). So the editor can draw what an Attack beacon will go for (S2's
/// plan, task `view`).
///
/// Read laxly, like the route's targets ([`crate::editor::route_waypoints`]): it decides
/// nothing, a target this walk cannot read is [`AttackAim::Unknown`] and is not drawn, and
/// the verdict on the file is the verifier's. A selector and "the enemy commander" are
/// drawn as words only, because where they point is the sim's to read when the attack runs
/// (AGENTS.md section 3 rule 2, "never ranks"). A text that is not JSON once its comments
/// are gone names none.
#[must_use]
pub fn attack_targets(text: &str) -> Vec<AttackMark> {
    fn walk(value: &Json, pointer: &mut String, out: &mut Vec<AttackMark>) {
        match value {
            Json::Object(entries) => {
                for (key, child) in entries {
                    let mark = pointer.len();
                    pointer.push('/');
                    pointer.push_str(&key.replace('~', "~0").replace('/', "~1"));
                    if key == "attack"
                        && let Some(target) = child.get("target")
                    {
                        let at = format!("{pointer}/target");
                        out.push(AttackMark {
                            step: step_of(&at),
                            aim: aim_of(target),
                            pointer: at,
                        });
                    } else {
                        walk(child, pointer, out);
                    }
                    pointer.truncate(mark);
                }
            }
            Json::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    let mark = pointer.len();
                    pointer.push('/');
                    pointer.push_str(&index.to_string());
                    walk(child, pointer, out);
                    pointer.truncate(mark);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Some(document) = crate::editor::lax_document(text) {
        walk(&document, &mut String::new(), &mut out);
    }
    out
}

/// `gp.v1.BeaconRef`'s selector arms, in both of proto JSON's spellings: a `beacon` that
/// writes one of these is a selector, and one that writes neither a name nor a selector
/// is a target this walk cannot read.
const SELECTOR_ARMS: &[&str] = &[
    "safest",
    "nearest",
    "weakest",
    "most_threatened",
    "mostThreatened",
];

/// What one `gp.v1.AttackTarget` object names, in either of proto JSON's spellings.
fn aim_of(target: &Json) -> AttackAim {
    if let Some(voxel) = target.get("voxel") {
        return voxel_aim(voxel);
    }
    if target.get("enemy_commander").is_some() || target.get("enemyCommander").is_some() {
        return AttackAim::EnemyCommander;
    }
    let Some(beacon) = target.get("beacon") else {
        return AttackAim::Unknown;
    };
    match beacon.get("beacon_id").or_else(|| beacon.get("beaconId")) {
        Some(Json::String(id)) => AttackAim::Beacon(id.clone()),
        None if SELECTOR_ARMS.iter().any(|arm| beacon.get(arm).is_some()) => AttackAim::Selector,
        Some(_) | None => AttackAim::Unknown,
    }
}

/// A `gp.v1.Voxel` object as an aim. An absent axis is zero, which is proto3 JSON's own
/// rule for an unwritten integer; an axis that is not a whole number reads as
/// [`AttackAim::Unknown`], not as zero.
fn voxel_aim(voxel: &Json) -> AttackAim {
    let axis = |name: &str| -> Option<i32> {
        match voxel.get(name) {
            None => Some(0),
            Some(Json::Number(lexeme) | Json::String(lexeme)) => lexeme.parse::<i32>().ok(),
            Some(_) => None,
        }
    };
    match (axis("x"), axis("y"), axis("z")) {
        (Some(x), Some(y), Some(z)) => AttackAim::Voxel([x, y, z]),
        _ => AttackAim::Unknown,
    }
}

/// The gateway's "This round: ..." sentence, found at the end of a Lull briefing's prose
/// (decisions-log item 133 (3) (i): it rides `get_briefing`'s prose, for the seat's own
/// token, until v1.1 publishes a field for it). `None` when the prose carries none, which
/// is what the gateway sends when the playbook reads the map nowhere on its route.
#[must_use]
pub fn this_round_of(prose: &str) -> Option<String> {
    prose
        .rfind(THIS_ROUND_LEAD)
        .and_then(|at| prose.get(at..))
        .map(str::to_owned)
}

/// The route step a pointer is in, numbered from 1.
fn step_of(pointer: &str) -> Option<usize> {
    let token = pointer
        .strip_prefix("/declarative/route/")?
        .split('/')
        .next()?;
    token.parse::<usize>().ok()?.checked_add(1)
}

/// The site arm a pointer ends in.
fn arm_of(pointer: &str) -> ChipArm {
    match pointer.rsplit('/').next() {
        Some("covering") => ChipArm::Covering,
        Some("on") => ChipArm::On,
        _ => ChipArm::Other,
    }
}

/// What a `FeatureRef` object holds.
fn form_of(reference: &Json) -> ChipForm {
    if reference.get("feature_id").is_some() || reference.get("featureId").is_some() {
        ChipForm::Name
    } else if reference.get("covered").is_some() {
        ChipForm::Covered
    } else if reference.get("vent").is_some() || reference.get("seam").is_some() {
        ChipForm::Description
    } else {
        ChipForm::Unknown
    }
}

/// The value at an RFC 6901 JSON Pointer.
fn at_pointer<'a>(document: &'a Json, pointer: &str) -> Option<&'a Json> {
    if pointer.is_empty() {
        return Some(document);
    }
    let mut here = document;
    for token in pointer.strip_prefix('/')?.split('/') {
        let token = token.replace("~1", "/").replace("~0", "~");
        here = match here {
            Json::Object(_) => here.get(&token)?,
            Json::Array(items) => items.get(token.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(here)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feature_name_is_spelt_back_into_its_kind_and_column() {
        let name = FeatureName::parse("vent_120_88").expect("a vent");
        assert_eq!(name.kind, FeatureKind::Vent);
        assert_eq!((name.x.as_str(), name.y.as_str()), ("120", "88"));
        assert_eq!(
            FeatureName::parse("seam_3_400").map(|name| name.kind),
            Some(FeatureKind::Seam)
        );
        for not_a_name in ["vent_1", "vent_1_2_3", "pit_1_2", "vent_a_2", "vent__2", ""] {
            assert_eq!(FeatureName::parse(not_a_name), None, "{not_a_name}");
        }
    }

    const COVERING: &str = r#"{
  // a comment the walk ignores
  "declarative": {"route": [
    {"label": "a", "place_beacon": {"at": {"covering": {"vent": {"rank": "NEAREST", "coverage": "UNCOVERED"}}},
      "initial": {"mandate": {"build": {"targets": [{"anchor": {"on": {"covered": {}}}}]}}}}},
    {"label": "b", "place_beacon": {"at": {"covering": {"feature_id": "vent_150_20"}}}}
  ]}
}"#;

    #[test]
    fn a_chip_is_the_gateways_pick_with_the_next_candidate_after_it() {
        let answer = json::read(
            r#"{"refs":[
              {"pointer":"/declarative/route/0/place_beacon/at/covering","feature_id":"vent_120_88","travel_ms":14000,
               "candidates":[{"feature_id":"vent_9_9","travel_ms":9000},{"feature_id":"vent_120_88","travel_ms":14000},{"feature_id":"vent_150_20","travel_ms":16000}],
               "matched":3,"failure":"","reachable":true},
              {"pointer":"/declarative/route/0/place_beacon/initial/mandate/build/targets/0/anchor/on","feature_id":"vent_120_88","travel_ms":13000,
               "candidates":[{"feature_id":"vent_120_88","travel_ms":13000}],"matched":1,"failure":"","reachable":true},
              {"pointer":"/declarative/route/1/place_beacon/at/covering","feature_id":"","travel_ms":0,
               "candidates":[],"matched":3,"failure":"no_target"}
            ],"_status":{"phase":"lull","round":1}}"#,
        )
        .expect("json");
        let chips = chips_of(&answer, COVERING).expect("a ResolveRefsResponse");
        assert_eq!(chips.len(), 3);
        let first = chips.first().expect("one");
        assert_eq!(first.step, Some(1));
        assert!(first.reachable, "the gateway said the pick is reached");
        assert_eq!(first.arm, ChipArm::Covering);
        assert_eq!(first.form, ChipForm::Description);
        assert_eq!(
            first
                .now
                .as_ref()
                .map(|now| (now.id.as_str(), now.travel_ms)),
            Some(("vent_120_88", 14_000))
        );
        assert_eq!(
            first
                .next
                .as_ref()
                .map(|next| (next.id.as_str(), next.travel_ms)),
            Some(("vent_150_20", 16_000)),
            "next is the one listed after the pick, not the first listed"
        );
        let second = chips.get(1).expect("two");
        assert_eq!((second.arm, second.form), (ChipArm::On, ChipForm::Covered));
        assert_eq!(second.next, None, "one candidate has no next");
        let third = chips.get(2).expect("three");
        assert_eq!(third.step, Some(2));
        assert_eq!(third.form, ChipForm::Name);
        assert_eq!(third.now, None);
        assert_eq!(third.next, None);
        assert_eq!((third.failure.as_str(), third.matched), ("no_target", 3));
        assert!(
            !third.reachable,
            "a reference that reads nothing reaches nothing"
        );
    }

    #[test]
    fn only_a_text_with_a_covering_or_an_on_is_worth_resolving() {
        assert!(has_feature_refs(COVERING));
        assert!(!has_feature_refs(
            r#"{"declarative":{"route":[{"label":"go_1","move":{"to":{"voxel":{"x":1,"y":2,"z":3}}}}]}}"#
        ));
        assert!(!has_feature_refs("not json at all"));
        assert!(
            !has_feature_refs(r#"{"declarative":{"route":[{"label":"on","on_death":{}}]}}"#),
            "a key that starts with `on` is not `on`"
        );
    }

    /// Every Attack beacon's target is found where the text writes it, named, fixed,
    /// described or the enemy commander, in either spelling, and nothing is ranked.
    #[test]
    fn an_attack_beacons_target_is_read_where_the_text_writes_it() {
        let text = r#"{
  // the targets the editor draws
  "declarative": {"route": [
    {"label": "a", "place_beacon": {"at": {"voxel": {"x": 1, "y": 2, "z": 3}},
      "initial": {"mandate": {"attack": {"target": {"beacon": {"beacon_id": "e_03"}}}}}}},
    {"label": "b", "place_beacon": {"at": {"voxel": {"x": 4, "y": 5, "z": 6}},
      "initial": {"mandate": {"attack": {"target": {"voxel": {"x": 200, "y": 40}}}}}}},
    {"label": "c", "place_beacon": {"at": {"voxel": {"x": 7, "y": 8, "z": 9}},
      "initial": {"mandate": {"attack": {"target": {"enemyCommander": {}}}}}}},
    {"label": "d", "place_beacon": {"at": {"voxel": {"x": 7, "y": 8, "z": 9}},
      "initial": {"mandate": {"attack": {"target": {"beacon": {"nearest": {}}}}}}}},
    {"label": "e", "place_beacon": {"at": {"voxel": {"x": 7, "y": 8, "z": 9}},
      "initial": {"mandate": {"attack": {"target": {"voxel": {"x": "east"}}}}}}},
    {"label": "f", "place_beacon": {"at": {"voxel": {"x": 7, "y": 8, "z": 9}},
      "initial": {"mandate": {"attack": {"target": {"beacon": {}}}}}}}
  ]}
}"#;
        let marks = attack_targets(text);
        let aims: Vec<&AttackAim> = marks.iter().map(|mark| &mark.aim).collect();
        assert_eq!(
            aims,
            [
                &AttackAim::Beacon("e_03".to_owned()),
                &AttackAim::Voxel([200, 40, 0]),
                &AttackAim::EnemyCommander,
                &AttackAim::Selector,
                &AttackAim::Unknown,
                &AttackAim::Unknown,
            ]
        );
        let first = marks.first().expect("one");
        assert_eq!(
            first.pointer,
            "/declarative/route/0/place_beacon/initial/mandate/attack/target"
        );
        assert_eq!(first.step, Some(1));
        assert!(attack_targets("not json").is_empty());
        assert!(
            attack_targets(COVERING).is_empty(),
            "no Attack beacon, no target"
        );
    }

    #[test]
    fn this_round_is_the_briefings_last_sentence_when_it_has_one() {
        let prose = "Round 1 of 3, the Lull. Your score is $ 320. This round: step 2 places a \
                     new beacon near (150, 20), $ 60.";
        assert_eq!(
            this_round_of(prose).as_deref(),
            Some("This round: step 2 places a new beacon near (150, 20), $ 60.")
        );
        assert_eq!(this_round_of("Round 1 of 3, the Lull."), None);
    }
}

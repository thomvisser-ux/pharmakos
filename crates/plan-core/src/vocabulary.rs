// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `get_vocabulary`'s content: every condition family, predicate, mandate,
//! route step and blueprint a playbook may name, with each one's parameters
//! and, where this seat cannot use it yet, why (S3's plan, E8; spec section
//! 13: "Vocabulary you haven't unlocked shows greyed out with a lock and the
//! reason").
//!
//! # Where each part comes from
//!
//! * **The words** are the schema's: each entry's `name` is a oneof arm of
//!   `gp.v1.Condition`, `gp.v1.MandateSettings` or `gp.v1.Step`, or a
//!   blueprint the sim builds, and `tests` hold each to the schema, so a
//!   rename there fails here rather than in a picker.
//! * **Each parameter's shape** is [`crate::shape`]'s, the one a rendered
//!   chip at the same field carries.
//! * **A lock** is one of three things, and nothing else: a block the schema
//!   still reserves, "from S4" or "from S6" as `playbook.proto`'s catalogue by
//!   stage says (S3's plan, decision 2); a construct the verifier still refuses,
//!   in the verifier's own sentence for it ([`pharmakos_verifier::REFUSED_CONSTRUCTS`]),
//!   so the lock lifts in the build whose verifier lifts the refusal; or a
//!   licence the seat does not hold.
//!
//! Nothing here reads the world: the seat's licences are the caller's to
//! pass, and the answer is otherwise a pure function of the build and the
//! rules table.

use pharmakos_proto::gp::v1::Licence;
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::StructureKind;

use crate::shape::{
    BEACON_ARMS, NEAR_ARMS, ReferenceKind, Shape, UNIT_COUNT, UNIT_DOLLARS, UNIT_KW, UNIT_MS,
    UNIT_VOXELS, UnknownEnum,
};
use crate::strings as s;

/// Why the vocabulary could not be built: this crate disagreeing with the
/// schema or with the verifier, which the module's tests pin never happens.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum VocabularyError {
    /// A parameter's enum is not in the schema.
    UnknownEnum(UnknownEnum),
    /// The verifier refuses a construct by a code its string table has no
    /// sentence for.
    NoRefusalText(&'static str),
    /// The verifier could not be asked which blueprints a Build target may
    /// name: its input would not assemble, or its answer did not hold the
    /// refusal the probe plants, so it was not read.
    BlueprintProbe(String),
}

impl From<UnknownEnum> for VocabularyError {
    fn from(error: UnknownEnum) -> Self {
        VocabularyError::UnknownEnum(error)
    }
}

impl core::fmt::Display for VocabularyError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VocabularyError::UnknownEnum(error) => write!(formatter, "{error}"),
            VocabularyError::NoRefusalText(code) => write!(
                formatter,
                "the verifier refuses a construct as `{code}` and has no sentence for it"
            ),
            VocabularyError::BlueprintProbe(why) => write!(
                formatter,
                "the verifier could not be asked which blueprints a Build target may name: {why}"
            ),
        }
    }
}

impl std::error::Error for VocabularyError {}

/// The whole vocabulary (`gp.api.v1.GetVocabularyResponse`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Vocabulary {
    /// Spec section 10's condition families, in its order.
    pub families: Vec<Family>,
    /// The mandates a beacon may carry.
    pub mandates: Vec<Entry>,
    /// The route steps.
    pub steps: Vec<Entry>,
    /// The blueprints a Build target or a queued structure may name.
    pub blueprints: Vec<Entry>,
}

/// One condition family (`gp.api.v1.VocabularyFamily`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Family {
    /// A stable id, `lower_snake_case` (`VocabularyFamily.family`).
    pub id: &'static str,
    /// Its name in the player's words.
    pub title: &'static str,
    /// Its predicates, in the schema's order.
    pub predicates: Vec<Entry>,
    /// Why the whole family cannot be used, when it cannot.
    pub locked: Option<String>,
}

/// One entry (`gp.api.v1.VocabularyEntry`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// The word a playbook writes; empty on an entry the schema does not hold
    /// yet, which always carries `locked`.
    pub name: &'static str,
    /// Its name in the player's words.
    pub title: &'static str,
    /// What may be written inside it.
    pub parameters: Vec<Parameter>,
    /// What a picker inserts, as JSON text; empty on a locked entry.
    pub skeleton_json: String,
    /// Why this seat cannot use it yet.
    pub locked: Option<String>,
}

/// One parameter of an entry (`gp.api.v1.VocabularyParameter`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Parameter {
    /// Its JSON Pointer inside the entry's own object.
    pub pointer: &'static str,
    /// Its name in the player's words.
    pub title: &'static str,
    /// What may be written there.
    pub shape: Shape,
}

/// The vocabulary as this seat sees it, holding `held`.
///
/// # Errors
///
/// [`VocabularyError`], which a test pins never happens.
pub fn vocabulary(held: &[Licence], rules: &RulesTable) -> Result<Vocabulary, VocabularyError> {
    let kinds = blueprints_by_slot(rules)?;
    Ok(Vocabulary {
        families: families(&kinds)?,
        mandates: mandates()?,
        steps: steps()?,
        blueprints: blueprints(held, &kinds)?,
    })
}

/// One kind of structure this build knows, and the slots the verifier lets a
/// playbook name it in (S3's `gwr` review, item 4).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Blueprint {
    /// The sim's kind.
    pub kind: StructureKind,
    /// The id a playbook writes.
    pub id: &'static str,
    /// Its name in the player's words.
    pub title: &'static str,
    /// Whether a Build target may name it (`E0010` refuses every other).
    pub build_target: bool,
    /// Whether a `queue_structure` row may name it: the sim's
    /// [`pharmakos_sim::spire::queued_kind`], which the verifier's `E0009`
    /// reads too.
    pub queued: bool,
    /// The licence a seat must hold to queue it, if any.
    pub licence: Option<Licence>,
}

impl Blueprint {
    /// Whether a structure of this kind can stand at a beacon in this build:
    /// one a Build target or a queue may name.
    #[must_use]
    pub const fn can_stand(&self) -> bool {
        self.build_target || self.queued
    }

    /// The id as JSON text, quotes included.
    #[must_use]
    pub fn json(&self) -> String {
        format!("\"{}\"", self.id)
    }
}

/// The id a playbook writes for `kind`, `gp.v1.RulesTable.Structures`'
/// spelling: the one place this crate and the gateway spell it.
#[must_use]
pub const fn blueprint_id(kind: StructureKind) -> &'static str {
    match kind {
        StructureKind::Generator => "generator",
        StructureKind::Autocannon => "autocannon",
        StructureKind::Mortar => "mortar",
        StructureKind::SurveyPost => "survey_post",
        StructureKind::ResonanceSpire => pharmakos_sim::spire::SPIRE_BLUEPRINT,
        StructureKind::Wall => "wall",
    }
}

/// `kind`'s name in the player's words.
const fn blueprint_title(kind: StructureKind) -> &'static str {
    match kind {
        StructureKind::Generator => s::BLUEPRINT_GENERATOR,
        StructureKind::Autocannon => s::BLUEPRINT_AUTOCANNON,
        StructureKind::Mortar => s::BLUEPRINT_MORTAR,
        StructureKind::SurveyPost => s::BLUEPRINT_SURVEY_POST,
        StructureKind::ResonanceSpire => s::BLUEPRINT_RESONANCE_SPIRE,
        StructureKind::Wall => s::BLUEPRINT_WALL,
    }
}

/// The licence a seat holds to queue `kind` (spec section 8): S3 has the
/// one, the Resonance Spire's.
const fn licence_for(kind: StructureKind) -> Option<Licence> {
    match kind {
        StructureKind::ResonanceSpire => Some(Licence::ResonanceSpire),
        StructureKind::Generator
        | StructureKind::Autocannon
        | StructureKind::Mortar
        | StructureKind::SurveyPost
        | StructureKind::Wall => None,
    }
}

/// Where the probe's Build targets stand.
const PROBE_ROWS: &str = "/declarative/route/0/interface/rows";

/// The probe [`blueprints_by_slot`] asks the verifier: one interface step
/// holding a Build target of each kind, in [`StructureKind::ALL`]'s order,
/// and one with an empty id last, whose `E0101` shows the refusals were read.
fn blueprint_probe() -> String {
    let target = |id: &str| {
        format!(
            r#"{{"add_build_target":{{"target":{{"blueprint_id":"{id}","anchor":{{"voxel":{{"x":0,"y":0,"z":0}}}}}}}}}}"#
        )
    };
    let mut rows: Vec<String> = StructureKind::ALL
        .into_iter()
        .map(|kind| target(blueprint_id(kind)))
        .collect();
    rows.push(target(""));
    format!(
        r#"{{"schema_version":{{"major":1}},"meta":{{"title":"Blueprint probe","author_kind":"BUILTIN"}},"kind":"PLAYBOOK","declarative":{{"route":[{{"label":"probe","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{}]}}}}]}},"on_death":{{"on_respawn":"CONTINUE"}},"fallback":{{"hold":{{"at":{{"safest":{{}}}}}}}}}}"#,
        rows.join(",")
    )
}

/// Every kind of structure the sim defines ([`StructureKind::ALL`]), each
/// with the slots the verifier lets a playbook name it in: a Build target's
/// by asking the verifier itself, at QUICK, over one probe playbook holding a
/// Build target of each kind (its `E0010` is the answer, from its own
/// `BUILDABLE` table), and a queue's by the sim's
/// [`pharmakos_sim::spire::queued_kind`], which the verifier's `E0009`
/// reads. No list here says which kind goes where.
///
/// # Errors
///
/// [`VocabularyError::BlueprintProbe`] when the verifier's input will not
/// assemble from `rules`, or its answer lacks the refusal the probe plants
/// (an empty id's `E0101`), so it was not read.
pub fn blueprints_by_slot(rules: &RulesTable) -> Result<Vec<Blueprint>, VocabularyError> {
    let scope = pharmakos_verifier::Scope::new(
        pharmakos_sim::tables::SeatId::new(0),
        pharmakos_sim::knowledge::SeatEconomy {
            treasury: pharmakos_sim::math::quantity::Money::ZERO,
            supply: pharmakos_sim::math::quantity::Kw::ZERO,
            draw: pharmakos_sim::math::quantity::Kw::ZERO,
        },
    );
    let report = crate::verify::verify_jsonc(
        &blueprint_probe(),
        &[],
        &scope,
        rules,
        pharmakos_proto::gp::api::v1::verify_plan::Depth::Quick,
    )
    .map_err(|error| VocabularyError::BlueprintProbe(error.to_string()))?;
    let refused = |code: &str, index: usize| {
        let path = format!("{PROBE_ROWS}/{index}/add_build_target/target/blueprint_id");
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == code && diagnostic.path == path)
    };
    if !refused("E0101", StructureKind::ALL.len()) {
        return Err(VocabularyError::BlueprintProbe(String::from(
            "its answer lacks the empty id's E0101, so its Build-target refusals were not read",
        )));
    }
    Ok(StructureKind::ALL
        .into_iter()
        .enumerate()
        .map(|(index, kind)| {
            let id = blueprint_id(kind);
            Blueprint {
                kind,
                id,
                title: blueprint_title(kind),
                build_target: !refused("E0010", index),
                queued: pharmakos_sim::spire::queued_kind(id) == Some(kind),
                licence: licence_for(kind),
            }
        })
        .collect())
}

/// A choice over the ids of the `kinds` that `keep` picks.
#[must_use]
pub fn blueprint_choice(kinds: &[Blueprint], keep: impl Fn(&Blueprint) -> bool) -> Shape {
    Shape::Choice {
        values: kinds
            .iter()
            .filter(|kind| keep(kind))
            .map(Blueprint::json)
            .collect(),
    }
}

/// An open entry.
fn entry(
    name: &'static str,
    title: &'static str,
    parameters: Vec<Parameter>,
    skeleton_json: &'static str,
) -> Entry {
    Entry {
        name,
        title,
        parameters,
        skeleton_json: String::from(skeleton_json),
        locked: None,
    }
}

/// An entry the schema does not hold yet, locked until `stage`.
fn reserved(title: &'static str, stage: &str) -> Entry {
    Entry {
        name: "",
        title,
        parameters: Vec::new(),
        skeleton_json: String::new(),
        locked: Some(format!("{} {stage}", s::VOCABULARY_FROM)),
    }
}

/// `entry` with the verifier's lock when it still refuses `construct`, in its
/// own sentence; open otherwise. A refused entry offers no skeleton, so a
/// picker inserts nothing the verifier refuses.
fn unless_refused(mut entry: Entry, construct: &str) -> Result<Entry, VocabularyError> {
    if let Some(code) = pharmakos_verifier::code_for_construct(construct) {
        let text =
            pharmakos_verifier::strings::text(code).ok_or(VocabularyError::NoRefusalText(code))?;
        entry.locked = Some(String::from(text.beginner));
        entry.skeleton_json = String::new();
    }
    Ok(entry)
}

/// A predicate, locked while the verifier refuses it.
fn predicate(
    name: &'static str,
    title: &'static str,
    parameters: Vec<Parameter>,
    skeleton_json: &'static str,
) -> Result<Entry, VocabularyError> {
    unless_refused(
        entry(name, title, parameters, skeleton_json),
        &format!("predicate `{name}`"),
    )
}

fn parameter(pointer: &'static str, title: &'static str, shape: Shape) -> Parameter {
    Parameter {
        pointer,
        title,
        shape,
    }
}

/// An `IntCompare`'s two parameters at `at`: its operator and its value.
fn comparison(
    op: &'static str,
    value: &'static str,
    title: &'static str,
    shape: Shape,
) -> Result<[Parameter; 2], UnknownEnum> {
    Ok([
        parameter(
            op,
            s::VOCABULARY_COMPARED,
            Shape::choice("gp.v1.IntCompare.Op")?,
        ),
        parameter(value, title, shape),
    ])
}

/// A beacon reference parameter.
fn beacon_parameter() -> Parameter {
    parameter(
        "/beacon",
        s::VOCABULARY_BEACON,
        Shape::reference(ReferenceKind::Beacon, &BEACON_ARMS),
    )
}

/// The enemy predicates' place, radius and freshness.
fn near_parameters() -> Result<Vec<Parameter>, UnknownEnum> {
    let mut found = vec![
        parameter(
            "/near",
            s::VOCABULARY_NEAR,
            Shape::reference(ReferenceKind::Location, &NEAR_ARMS),
        ),
        parameter(
            "/radius_voxels",
            s::VOCABULARY_RADIUS,
            Shape::count(UNIT_VOXELS),
        ),
    ];
    found.extend(freshness()?);
    Ok(found)
}

/// A `Freshness`' two parameters. The window has no least: the verifier
/// refuses an enemy predicate whole until S3's `vfy6` checks it, so nothing
/// bounds it yet (as the rendered chip says).
fn freshness() -> Result<[Parameter; 2], UnknownEnum> {
    Ok([
        parameter(
            "/freshness/kind",
            s::VOCABULARY_FRESHNESS,
            Shape::choice("gp.v1.Freshness.Kind")?,
        ),
        parameter(
            "/freshness/known_within_ms",
            s::VOCABULARY_WINDOW,
            Shape::integer(None, None, UNIT_MS),
        ),
    ])
}

fn families(kinds: &[Blueprint]) -> Result<Vec<Family>, VocabularyError> {
    Ok(vec![
        clock_family()?,
        commander_family()?,
        own_beacon_family(kinds)?,
        closed_family("aggregates", s::FAMILY_AGGREGATES, "S6"),
        enemy_knowledge_family(kinds)?,
        economy_family()?,
        closed_family("capability", s::FAMILY_CAPABILITY, "S4"),
        closed_family("mast_coverage", s::FAMILY_MAST_COVERAGE, "S4"),
        playbook_state_family()?,
    ])
}

/// A family open to use.
fn open_family(id: &'static str, title: &'static str, predicates: Vec<Entry>) -> Family {
    Family {
        id,
        title,
        predicates,
        locked: None,
    }
}

/// A family the schema still reserves whole, locked until `stage`.
fn closed_family(id: &'static str, title: &'static str, stage: &str) -> Family {
    Family {
        id,
        title,
        predicates: Vec::new(),
        locked: Some(format!("{} {stage}", s::VOCABULARY_FROM)),
    }
}

/// The `clock` family.
fn clock_family() -> Result<Family, VocabularyError> {
    Ok(open_family(
        "clock",
        s::FAMILY_CLOCK,
        vec![
            predicate(
                "segment_elapsed",
                s::PREDICATE_SEGMENT_ELAPSED,
                comparison(
                    "/ms/op",
                    "/ms/value",
                    s::VOCABULARY_TIME,
                    Shape::integer(None, None, UNIT_MS),
                )?
                .to_vec(),
                r#"{"ms":{"op":"GE","value":60000}}"#,
            )?,
            reserved(s::RESERVED_CLOCK, "S6"),
        ],
    ))
}

/// The `commander` family.
fn commander_family() -> Result<Family, VocabularyError> {
    Ok(open_family(
        "commander",
        s::FAMILY_COMMANDER,
        vec![
            predicate(
                "cmdr_hp_pct",
                s::PREDICATE_CMDR_HP_PCT,
                vec![
                    parameter(
                        "/cmp",
                        s::VOCABULARY_COMPARED,
                        Shape::choice("gp.v1.CmdrHpPct.Cmp")?,
                    ),
                    parameter("/pct", s::VOCABULARY_PERCENT, Shape::percent()),
                ],
                r#"{"cmp":"LE","pct":40}"#,
            )?,
            predicate(
                "cmdr_took_damage_within",
                s::PREDICATE_CMDR_TOOK_DAMAGE,
                vec![parameter("/ms", s::VOCABULARY_WITHIN, Shape::duration())],
                r#"{"ms":3000}"#,
            )?,
            predicate(
                "cmdr_deaths",
                s::PREDICATE_CMDR_DEATHS,
                comparison(
                    "/deaths/op",
                    "/deaths/value",
                    s::VOCABULARY_COUNT,
                    Shape::integer(None, None, UNIT_COUNT),
                )?
                .to_vec(),
                r#"{"deaths":{"op":"GE","value":1}}"#,
            )?,
            reserved(s::RESERVED_COMMANDER, "S6"),
        ],
    ))
}

/// The `own_beacon` family.
fn own_beacon_family(kinds: &[Blueprint]) -> Result<Family, VocabularyError> {
    Ok(open_family(
        "own_beacon",
        s::FAMILY_OWN_BEACON,
        vec![
            predicate(
                "beacon_hp_pct",
                s::PREDICATE_BEACON_HP_PCT,
                [
                    vec![beacon_parameter()],
                    comparison(
                        "/pct/op",
                        "/pct/value",
                        s::VOCABULARY_PERCENT,
                        Shape::percent(),
                    )?
                    .to_vec(),
                ]
                .concat(),
                r#"{"beacon":{"safest":{}},"pct":{"op":"LE","value":50}}"#,
            )?,
            predicate(
                "beacon_under_attack",
                s::PREDICATE_BEACON_UNDER_ATTACK,
                vec![
                    beacon_parameter(),
                    parameter("/within_ms", s::VOCABULARY_WITHIN, Shape::duration()),
                ],
                r#"{"beacon":{"safest":{}},"within_ms":5000}"#,
            )?,
            predicate(
                "beacon_powered",
                s::PREDICATE_BEACON_POWERED,
                vec![
                    beacon_parameter(),
                    parameter("/powered", s::VOCABULARY_POWERED, Shape::boolean()),
                ],
                r#"{"beacon":{"safest":{}},"powered":true}"#,
            )?,
            predicate(
                "beacon_units_by_role",
                s::PREDICATE_BEACON_UNITS_BY_ROLE,
                [
                    vec![
                        beacon_parameter(),
                        parameter(
                            "/role",
                            s::VOCABULARY_ROLE,
                            Shape::choice("gp.v1.BeaconUnitsByRole.Role")?,
                        ),
                    ],
                    comparison(
                        "/count/op",
                        "/count/value",
                        s::VOCABULARY_COUNT,
                        Shape::integer(None, None, UNIT_COUNT),
                    )?
                    .to_vec(),
                ]
                .concat(),
                r#"{"beacon":{"safest":{}},"role":"SCOUT","count":{"op":"GE","value":1}}"#,
            )?,
            predicate(
                "beacon_mandate",
                s::PREDICATE_BEACON_MANDATE,
                vec![
                    beacon_parameter(),
                    parameter(
                        "/mandate",
                        s::VOCABULARY_MANDATE,
                        Shape::choice("gp.v1.BeaconFilter.MandateKind")?,
                    ),
                ],
                r#"{"beacon":{"safest":{}},"mandate":"BUILD"}"#,
            )?,
            predicate(
                "beacon_has_structure",
                s::PREDICATE_BEACON_HAS_STRUCTURE,
                vec![
                    beacon_parameter(),
                    parameter(
                        "/blueprint_id",
                        s::VOCABULARY_STRUCTURE,
                        blueprint_choice(kinds, Blueprint::can_stand),
                    ),
                ],
                r#"{"beacon":{"safest":{}},"blueprint_id":"generator"}"#,
            )?,
            reserved(s::RESERVED_OWN_BEACON, "S6"),
        ],
    ))
}

/// The `enemy_knowledge` family.
fn enemy_knowledge_family(kinds: &[Blueprint]) -> Result<Family, VocabularyError> {
    Ok(open_family(
        "enemy_knowledge",
        s::FAMILY_ENEMY_KNOWLEDGE,
        vec![
            predicate(
                "enemy_units_near",
                s::PREDICATE_ENEMY_UNITS_NEAR,
                [
                    near_parameters()?,
                    comparison(
                        "/count/op",
                        "/count/value",
                        s::VOCABULARY_COUNT,
                        Shape::integer(None, None, UNIT_COUNT),
                    )?
                    .to_vec(),
                ]
                .concat(),
                r#"{"near":{"safest":{}},"radius_voxels":24,"count":{"op":"GE","value":1},"freshness":{"kind":"VISIBLE_NOW"}}"#,
            )?,
            predicate(
                "enemy_beacon_known",
                s::PREDICATE_ENEMY_BEACON_KNOWN,
                freshness()?.to_vec(),
                r#"{"freshness":{"kind":"VISIBLE_NOW"}}"#,
            )?,
            predicate(
                "enemy_structure_near",
                s::PREDICATE_ENEMY_STRUCTURE_NEAR,
                [
                    near_parameters()?,
                    // `structure`'s `blueprint_id` arm; the skeleton writes
                    // its other arm, `any_kind`.
                    vec![parameter(
                        "/blueprint_id",
                        s::VOCABULARY_STRUCTURE,
                        blueprint_choice(kinds, Blueprint::can_stand),
                    )],
                ]
                .concat(),
                r#"{"near":{"safest":{}},"radius_voxels":16,"any_kind":{},"freshness":{"kind":"VISIBLE_NOW"}}"#,
            )?,
            reserved(s::RESERVED_ENEMY_KNOWLEDGE, "S6"),
        ],
    ))
}

/// The `economy` family.
fn economy_family() -> Result<Family, VocabularyError> {
    Ok(open_family(
        "economy",
        s::FAMILY_ECONOMY,
        vec![
            predicate(
                "treasury",
                s::PREDICATE_TREASURY,
                comparison(
                    "/dollars/op",
                    "/dollars/value",
                    s::VOCABULARY_AMOUNT,
                    Shape::integer(None, None, UNIT_DOLLARS),
                )?
                .to_vec(),
                r#"{"dollars":{"op":"GE","value":100}}"#,
            )?,
            predicate(
                "kw_headroom",
                s::PREDICATE_KW_HEADROOM,
                comparison(
                    "/kw/op",
                    "/kw/value",
                    s::VOCABULARY_POWER,
                    Shape::integer(None, None, UNIT_KW),
                )?
                .to_vec(),
                r#"{"kw":{"op":"LT","value":5}}"#,
            )?,
            predicate(
                "kw_supply",
                s::PREDICATE_KW_SUPPLY,
                comparison(
                    "/kw/op",
                    "/kw/value",
                    s::VOCABULARY_POWER,
                    Shape::integer(None, None, UNIT_KW),
                )?
                .to_vec(),
                r#"{"kw":{"op":"GE","value":20}}"#,
            )?,
            predicate(
                "kw_draw",
                s::PREDICATE_KW_DRAW,
                comparison(
                    "/kw/op",
                    "/kw/value",
                    s::VOCABULARY_POWER,
                    Shape::integer(None, None, UNIT_KW),
                )?
                .to_vec(),
                r#"{"kw":{"op":"GE","value":20}}"#,
            )?,
            reserved(s::RESERVED_ECONOMY, "S6"),
        ],
    ))
}

/// The `playbook_state` family.
fn playbook_state_family() -> Result<Family, VocabularyError> {
    Ok(open_family(
        "playbook_state",
        s::FAMILY_PLAYBOOK_STATE,
        vec![
            predicate(
                "step_reached",
                s::PREDICATE_STEP_REACHED,
                vec![parameter(
                    "/label",
                    s::VOCABULARY_STEP,
                    Shape::reference(ReferenceKind::StepLabel, &[]),
                )],
                // A label the player then picks from the route; until then the
                // verifier names it as a step the playbook may not hold, never
                // as a missing field (`E0101`).
                r#"{"label":"step_1"}"#,
            )?,
            predicate(
                "rule_fired",
                s::PREDICATE_RULE_FIRED,
                vec![parameter(
                    "/handler_id",
                    s::VOCABULARY_RULE,
                    Shape::reference(ReferenceKind::HandlerId, &[]),
                )],
                // As `step_reached`'s: a rule id the player then picks.
                r#"{"handler_id":"rule_1"}"#,
            )?,
            reserved(s::RESERVED_PLAYBOOK_STATE, "S6"),
        ],
    ))
}

/// The two settings every writ shares (`gp.v1.MandateSettings` 1 and 2): the
/// rules of engagement and the retreat threshold, each a parameter of the
/// `MandateSettings` object itself.
///
/// # Errors
///
/// As [`vocabulary`].
pub fn common_parameters() -> Result<Vec<Parameter>, VocabularyError> {
    Ok(vec![
        parameter(
            "/roe",
            s::VOCABULARY_ROE,
            Shape::choice("gp.v1.MandateSettings.Roe")?,
        ),
        parameter(
            "/retreat_hp_pct",
            s::VOCABULARY_RETREAT_HP,
            Shape::percent(),
        ),
    ])
}

fn mandates() -> Result<Vec<Entry>, VocabularyError> {
    Ok(vec![
        build_mandate()?,
        defend_mandate()?,
        attack_mandate()?,
        survey_mandate(),
        mine_mandate()?,
    ])
}

/// The `build` writ.
fn build_mandate() -> Result<Entry, VocabularyError> {
    Ok(entry(
        "build",
        s::MANDATE_BUILD,
        vec![
            parameter(
                "/repair_threshold_pct",
                s::VOCABULARY_REPAIR,
                Shape::percent(),
            ),
            parameter(
                "/rebuild_destroyed",
                s::VOCABULARY_REBUILD,
                Shape::boolean(),
            ),
            parameter(
                "/terraform",
                s::VOCABULARY_TERRAFORM,
                Shape::choice("gp.v1.BuildSettings.Terraform")?,
            ),
        ],
        "{}",
    ))
}

/// The `defend` writ.
fn defend_mandate() -> Result<Entry, VocabularyError> {
    Ok(entry(
        "defend",
        s::MANDATE_DEFEND,
        vec![
            // 1 to 100 in an arm that writes anything: a 0 there is the
            // verifier's `E0101` (only a wholly empty arm, legal at a deploy
            // and a switch, leaves it unwritten).
            parameter(
                "/engagement_radius_pct",
                s::VOCABULARY_ENGAGEMENT,
                Shape::integer(Some(1), Some(100), crate::shape::UNIT_PERCENT),
            ),
            parameter(
                "/pursue",
                s::VOCABULARY_PURSUE,
                Shape::choice("gp.v1.DefendSettings.Pursue")?,
            ),
            parameter(
                "/picket_scouts",
                s::VOCABULARY_PICKET,
                Shape::integer(Some(0), Some(i32::from(u8::MAX)), UNIT_COUNT),
            ),
        ],
        "{}",
    ))
}

/// The `attack` writ.
fn attack_mandate() -> Result<Entry, VocabularyError> {
    let percent_one = Shape::integer(Some(1), Some(100), crate::shape::UNIT_PERCENT);
    Ok(entry(
        "attack",
        s::MANDATE_ATTACK,
        vec![
            parameter(
                "/target/beacon",
                s::VOCABULARY_TARGET,
                // A handle, or `nearest` with `side: ENEMY_KNOWN`: the
                // verifier refuses every other selector in an Attack target
                // (`E0015`, `E0415`).
                Shape::reference(ReferenceKind::Beacon, &crate::shape::ATTACK_TARGET_ARMS),
            ),
            parameter(
                "/retarget",
                s::VOCABULARY_RETARGET,
                Shape::choice("gp.v1.AttackSettings.Retarget")?,
            ),
            parameter(
                "/launch/force_size",
                s::VOCABULARY_FORCE,
                Shape::count(UNIT_COUNT),
            ),
            parameter(
                "/launch/segment_ms",
                s::VOCABULARY_LAUNCH,
                Shape::duration(),
            ),
            parameter(
                "/retreat/force_below_pct",
                s::VOCABULARY_RETREAT_FORCE,
                percent_one,
            ),
            parameter(
                "/retreat/after_launch_ms",
                s::VOCABULARY_RETREAT_TIME,
                Shape::duration(),
            ),
            parameter(
                "/after_action",
                s::VOCABULARY_AFTER,
                Shape::choice("gp.v1.AttackSettings.AfterAction")?,
            ),
            parameter("/breach_allowed", s::VOCABULARY_BREACH, Shape::boolean()),
            // 1 or more in an arm that writes anything (`E0510`).
            parameter(
                "/waves",
                s::VOCABULARY_WAVES,
                Shape::integer(Some(1), None, UNIT_COUNT),
            ),
        ],
        "{}",
    ))
}

/// The `survey` writ.
fn survey_mandate() -> Entry {
    entry(
        "survey",
        s::MANDATE_SURVEY,
        // The sim holds a beacon's scout count as a byte.
        vec![parameter(
            "/scout_count",
            s::VOCABULARY_SCOUTS,
            Shape::integer(Some(0), Some(i32::from(u8::MAX)), UNIT_COUNT),
        )],
        "{}",
    )
}

/// The `mine` writ.
fn mine_mandate() -> Result<Entry, VocabularyError> {
    Ok(entry(
        "mine",
        s::MANDATE_MINE,
        vec![
            parameter(
                "/dig_max_depth",
                s::VOCABULARY_DIG,
                Shape::count(UNIT_VOXELS),
            ),
            parameter("/flee_on_threat", s::VOCABULARY_FLEE, Shape::boolean()),
            parameter(
                "/seam_choice",
                s::VOCABULARY_SEAM,
                Shape::choice("gp.v1.MineSettings.SeamChoice")?,
            ),
            parameter(
                "/pillar_spacing",
                s::VOCABULARY_PILLARS,
                Shape::count(UNIT_VOXELS),
            ),
        ],
        "{}",
    ))
}

fn steps() -> Result<Vec<Entry>, VocabularyError> {
    let place_arms = crate::shape::place_arms();
    Ok(vec![
        entry(
            "move",
            s::STEP_TITLE_MOVE,
            vec![
                parameter(
                    "/to",
                    s::VOCABULARY_TO,
                    Shape::reference(ReferenceKind::Location, &NEAR_ARMS),
                ),
                parameter(
                    "/pace",
                    s::VOCABULARY_PACE,
                    Shape::choice("gp.v1.MoveStep.Pace")?,
                ),
            ],
            r#"{"to":{"safest":{}},"pace":"DIRECT"}"#,
        ),
        entry(
            "interface",
            s::STEP_TITLE_INTERFACE,
            vec![beacon_parameter()],
            r#"{"beacon":{"beacon_id":"b_00"}}"#,
        ),
        entry(
            "place_beacon",
            s::STEP_TITLE_PLACE_BEACON,
            vec![parameter(
                "/at",
                s::VOCABULARY_AT,
                Shape::reference(ReferenceKind::Location, &place_arms),
            )],
            r#"{"at":{"covering":{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}}}"#,
        ),
        entry(
            "wait_until",
            s::STEP_TITLE_WAIT_UNTIL,
            Vec::new(),
            r#"{"condition":{"segment_elapsed":{"ms":{"op":"GE","value":60000}}}}"#,
        ),
        entry(
            "hold",
            s::STEP_TITLE_HOLD,
            vec![parameter(
                "/ms",
                s::VOCABULARY_TIME,
                Shape::integer(Some(1), None, UNIT_MS),
            )],
            r#"{"ms":5000}"#,
        ),
        Entry {
            locked: Some(format!("{} S4", s::VOCABULARY_FROM)),
            ..entry("broadcast", s::STEP_TITLE_BROADCAST, Vec::new(), "")
        },
    ])
}

/// The blueprints a Build target or a queued structure may name, from
/// [`blueprints_by_slot`]: a kind no slot takes is locked in the verifier's
/// own sentence for a Build target's (`E0010`), and a kind only a queue takes
/// is locked while the seat lacks its licence. The list cannot say which slot
/// takes which kind (`gp.api.v1.VocabularyEntry` has no field for it), so a
/// kind one slot takes is offered open and nothing is locked wrongly; the
/// chips say it per slot.
///
/// # Errors
///
/// [`VocabularyError::NoRefusalText`] when the verifier's string table has
/// no sentence for `E0010`.
fn blueprints(held: &[Licence], kinds: &[Blueprint]) -> Result<Vec<Entry>, VocabularyError> {
    kinds
        .iter()
        .map(|kind| {
            let mut found = entry(kind.id, kind.title, Vec::new(), "");
            if !kind.can_stand() {
                let text = pharmakos_verifier::strings::text("E0010")
                    .ok_or(VocabularyError::NoRefusalText("E0010"))?;
                found.locked = Some(String::from(text.beginner));
            } else if !kind.build_target
                && kind.licence.is_some_and(|licence| !held.contains(&licence))
            {
                found.locked = Some(String::from(s::VOCABULARY_NEEDS_SPIRE_LICENCE));
            } else {
                found.skeleton_json = kind.json();
            }
            Ok(found)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Entry, vocabulary};

    fn rules() -> pharmakos_sim::rules::RulesTable {
        pharmakos_sim::rules::RulesTable::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules/rules.v1.json"),
        )
        .expect("the shipped rules table")
    }
    use pharmakos_proto::descriptor::schema;
    use pharmakos_proto::gp::v1::Licence;

    /// Each word is the schema's: a predicate is an arm of `Condition`, a
    /// mandate of `MandateSettings`, a step of `Step`, and each parameter's
    /// pointer names fields its message declares.
    #[test]
    fn every_word_and_parameter_is_the_schemas() {
        let words = vocabulary(&[], &rules()).expect("the vocabulary");
        let arm = |message: &str, name: &str| {
            let declared = schema().message(message).expect("a declared message");
            let field = declared
                .field_by_name(name)
                .unwrap_or_else(|| panic!("{message} declares no `{name}`"));
            assert!(field.oneof_index.is_some(), "{message}.{name} is an arm");
            field.type_name.trim_start_matches('.').to_owned()
        };
        let walk = |message: String, entry: &Entry| {
            for parameter in &entry.parameters {
                let mut here = message.clone();
                for token in parameter.pointer.trim_start_matches('/').split('/') {
                    let declared = schema().message(&here).expect("a declared message");
                    let field = declared
                        .field_by_name(token)
                        .unwrap_or_else(|| panic!("{here} declares no `{token}` ({})", entry.name));
                    here = field.type_name.trim_start_matches('.').to_owned();
                }
            }
        };
        for family in &words.families {
            for predicate in family
                .predicates
                .iter()
                .filter(|entry| !entry.name.is_empty())
            {
                walk(arm("gp.v1.Condition", predicate.name), predicate);
            }
        }
        for mandate in &words.mandates {
            walk(arm("gp.v1.MandateSettings", mandate.name), mandate);
        }
        for step in &words.steps {
            walk(arm("gp.v1.Step", step.name), step);
        }
    }

    /// A locked entry offers nothing to insert, an open one always does, and
    /// an entry the schema does not hold yet is always locked.
    #[test]
    fn a_lock_and_a_skeleton_never_go_together() {
        let words = vocabulary(&[], &rules()).expect("the vocabulary");
        let entries = words
            .families
            .iter()
            .flat_map(|family| &family.predicates)
            .chain(&words.mandates)
            .chain(&words.steps)
            .chain(&words.blueprints);
        for entry in entries {
            assert_eq!(
                entry.locked.is_some(),
                entry.skeleton_json.is_empty(),
                "{entry:?}"
            );
            if entry.name.is_empty() {
                assert!(entry.locked.is_some(), "{entry:?}");
            } else if entry.locked.is_none() {
                pharmakos_proto::json::read(&entry.skeleton_json)
                    .unwrap_or_else(|error| panic!("{}: {error:?}", entry.name));
            }
        }
    }

    /// The Resonance Spire is locked until the seat holds its licence, and
    /// the predicates the verifier still refuses are locked in its words.
    #[test]
    fn the_spire_waits_for_its_licence_and_a_refused_predicate_is_locked() {
        let spire = |held: &[Licence]| {
            vocabulary(held, &rules())
                .expect("the vocabulary")
                .blueprints
                .into_iter()
                .find(|entry| entry.name == "resonance_spire")
                .expect("the Spire")
        };
        assert!(spire(&[]).locked.is_some());
        assert_eq!(spire(&[Licence::ResonanceSpire]).locked, None);
        let words = vocabulary(&[], &rules()).expect("the vocabulary");
        for predicate in words.families.iter().flat_map(|family| &family.predicates) {
            if predicate.name.is_empty() {
                continue;
            }
            let refused =
                pharmakos_verifier::code_for_construct(&format!("predicate `{}`", predicate.name));
            assert_eq!(
                predicate.locked.is_some(),
                refused.is_some(),
                "{}",
                predicate.name
            );
        }
    }
}

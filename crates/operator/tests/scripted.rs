// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The operator driven through a **scripted client**: a closure that answers
//! the gateway's methods from a small, hand-built world.
//!
//! The operator's only input is the wire (and the public rules text), which is
//! what makes a scripted client an honest fixture (decisions-log item 111,
//! section F, "Build 12"): it can put three browned-out beacons in front of
//! the safe playbook, permute every viewer-scoped handle, or move the host
//! clock, without a match or a host test seam. Every test that hosts a real
//! match is in `crates/gamectl/tests/operator.rs`, where the adapter is.
//!
//! The scripted gateway does not apply a patch or run a verifier: it records
//! every call, method and params, into a transcript, and the tests compare
//! transcripts -- which is a stronger claim than comparing the playbook
//! alone, because every estimate, every patch and every submission is in it.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use pharmakos_operator::easy::{
    EASY_ADVISOR_CALL_BUDGET, EASY_CALL_BUDGET, PAGES_MAX, SAFE_MAX_RAISED,
};
use pharmakos_operator::{Easy, Submitted};
use pharmakos_proto::json::{Json, read, write};

// ---------------------------------------------------------------------------
// The scripted world
// ---------------------------------------------------------------------------

/// A chunk's edge.
const EDGE: usize = 32;
/// The ground's top voxel.
const GROUND: i32 = 10;
/// The wire's material bytes (`gp.api.v1.ViewChunk`).
const DIRT: u8 = 1;
const STONE: u8 = 2;
const SEAM_STANDARD: u8 = 4;
const VENT_LEAN: u8 = 6;

/// One beacon in the scripted `list_beacons` answer.
#[derive(Clone)]
struct Beacon {
    id: String,
    at: [i32; 3],
    owner: u8,
    core: bool,
    powered: bool,
}

/// One entity in the scripted `get_view` answer.
#[derive(Clone)]
struct Entity {
    id: String,
    kind: &'static str,
    subtype: &'static str,
    owner: u8,
    at: [i32; 3],
}

/// How the scripted verifier answers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verify {
    /// Every playbook qualifies.
    Qualifies,
    /// No playbook qualifies, and every report offers a machine-applicable fix.
    NeverButFixable,
}

/// The scripted gateway.
#[derive(Clone)]
struct Script {
    seat: u8,
    round: u32,
    segment_ms: i64,
    remaining_ms: i64,
    notes: String,
    size: [i32; 3],
    /// Top material per column, `(x, y)`, beyond the plain dirt.
    features: BTreeMap<(i32, i32), u8>,
    beacons: Vec<Beacon>,
    entities: Vec<Entity>,
    treasury: i64,
    headroom: i64,
    /// Travel from anywhere to a beacon, by id, overriding the distance rule.
    to_beacon_ms: BTreeMap<String, i64>,
    /// Page every paged read to [`PAGES_MAX`].
    paged: bool,
    verify: Verify,
    accept_submit: bool,
    /// Refuse every call with this code.
    refuse_everything: Option<&'static str>,
    /// Answer `Leg.to` as the bare voxel `main` wrote before T17.
    bare_legs: bool,
    /// Answer `get_view` with a page that is never complete and names no
    /// page after it.
    view_never_complete: bool,
    transcript: Vec<String>,
}

impl Script {
    /// A seat on a 64 x 64 map with its core in the middle, its commander
    /// beside it, and nothing else.
    fn new(seat: u8) -> Script {
        Script {
            seat,
            round: 1,
            segment_ms: 180_000,
            remaining_ms: 180_000,
            notes: String::new(),
            size: [64, 64, 32],
            features: BTreeMap::new(),
            beacons: vec![Beacon {
                id: String::from("b_01"),
                at: [32, 32, GROUND + 1],
                owner: seat,
                core: true,
                powered: true,
            }],
            entities: vec![Entity {
                id: String::from("u_1"),
                kind: "unit",
                subtype: "commander",
                owner: seat,
                at: [32, 30, GROUND + 1],
            }],
            treasury: 200,
            headroom: 6,
            to_beacon_ms: BTreeMap::new(),
            paged: false,
            verify: Verify::Qualifies,
            accept_submit: true,
            refuse_everything: None,
            bare_legs: false,
            view_never_complete: false,
            transcript: Vec::new(),
        }
    }

    /// Two seams and a vent inside the core's sphere, and an enemy scout and
    /// two drones in sight, with viewer-scoped handles.
    fn busy(seat: u8) -> Script {
        let mut script = Script::new(seat);
        for column in [(40, 36), (41, 36), (40, 37), (25, 25)] {
            script.features.insert(column, SEAM_STANDARD);
        }
        script.features.insert((36, 26), VENT_LEAN);
        script.entities.push(Entity {
            id: String::from("u_2"),
            kind: "unit",
            subtype: "build_drone",
            owner: seat,
            at: [33, 33, GROUND + 1],
        });
        script.entities.push(Entity {
            id: String::from("u_3"),
            kind: "unit",
            subtype: "scout",
            owner: seat ^ 1,
            at: [26, 26, GROUND + 1],
        });
        script.entities.push(Entity {
            id: String::from("s_1"),
            kind: "structure",
            subtype: "generator",
            owner: seat ^ 1,
            at: [60, 60, GROUND + 1],
        });
        script
    }

    fn status(&self) -> Json {
        read(&format!(
            r#"{{"phase":"lull","phase_remaining_ms":{},"segment_length_ms":{},"round":{}}}"#,
            self.remaining_ms, self.segment_ms, self.round
        ))
        .unwrap()
    }

    fn ok(&self, mut result: Json) -> Json {
        if let Json::Object(entries) = &mut result {
            entries.push((String::from("_status"), self.status()));
        }
        Json::Object(vec![
            (String::from("jsonrpc"), Json::String(String::from("2.0"))),
            (String::from("id"), Json::Number(String::from("1"))),
            (String::from("result"), result),
        ])
    }

    fn refuse(code: &str) -> Json {
        read(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"error":{{"code":-32000,"message":"scripted","data":{{"code":"{code}"}}}}}}"#
        ))
        .unwrap()
    }

    /// Where a `gp.v1.Location` is in the scripted world.
    fn place(&self, location: &Json) -> Option<[i32; 3]> {
        if let Some(voxel) = location.get("voxel") {
            return Some(voxel_of(voxel));
        }
        let id = location
            .get("beacon_anchor")
            .and_then(|anchor| anchor.get("beacon_id"))
            .and_then(text)?;
        self.beacons
            .iter()
            .find(|beacon| beacon.id == id)
            .map(|beacon| beacon.at)
    }

    /// The whole call closure.
    fn answer(&mut self, method: &str, params: &Json) -> Json {
        self.transcript
            .push(format!("{method} {}", compact(params)));
        if let Some(code) = self.refuse_everything {
            return Script::refuse(code);
        }
        let cursor = params.get("cursor").and_then(text).unwrap_or("").to_owned();
        match method {
            "get_status" | "set_ready" => {
                let status = self.status();
                self.ok(obj(vec![("status", status)]))
            }
            "get_briefing" => self.ok(obj(vec![
                ("notes", Json::String(self.notes.clone())),
                ("segment_length_ms", num(self.segment_ms)),
                ("prose", Json::String(String::from("scripted"))),
            ])),
            "list_beacons" => self.list_beacons(&cursor),
            "get_economy_forecast" => self.ok(obj(vec![
                ("treasury_now", num(self.treasury)),
                ("supply_kw_now", num(10)),
                ("draw_kw_now", num(10 - self.headroom)),
                ("headroom_kw_now", num(self.headroom)),
            ])),
            "get_map_summary" => self.ok(obj(vec![
                ("size", voxel_json(self.size)),
                (
                    "match_seed",
                    Json::String(String::from("0x0000000000005eed")),
                ),
            ])),
            "get_view" => self.get_view(&cursor),
            "list_templates" => self.list_templates(&cursor),
            "estimate_route" => self.estimate_route(params),
            "instantiate_template" => self.instantiate(params),
            "patch_plan" => {
                // Not applied: the patch is in the transcript, and the text
                // gains one line saying it went through a patch.
                let text_in = params.get("playbook_jsonc").and_then(text).unwrap_or("");
                self.ok(obj(vec![
                    (
                        "playbook_jsonc",
                        Json::String(format!("{text_in}// patched\n")),
                    ),
                    ("inverse_json_patch", Json::String(String::from("[]"))),
                ]))
            }
            "verify_plan" => {
                let report = match self.verify {
                    Verify::Qualifies => obj(vec![("qualifies", Json::Bool(true))]),
                    Verify::NeverButFixable => read(
                        r#"{"qualifies":false,"diagnostics":[{"code":"E0301","severity":"error","path":"/declarative/route/0","suggestions":[{"title":"fix","json_patch":"[{\"op\":\"remove\",\"path\":\"/meta/note\"}]","applicability":"machine_applicable"}]}]}"#,
                    )
                    .unwrap(),
                };
                self.ok(obj(vec![("report", report)]))
            }
            "submit_plan" => {
                let accepted = self.accept_submit;
                self.accept_submit = true;
                self.ok(obj(vec![
                    ("report", obj(vec![("qualifies", Json::Bool(accepted))])),
                    ("accepted", Json::Bool(accepted)),
                ]))
            }
            _ => Script::refuse("FORBIDDEN_SCOPE"),
        }
    }

    /// The page after `cursor`, or the empty cursor after the last.
    fn next_page(&self, cursor: &str) -> String {
        if !self.paged {
            return String::new();
        }
        let at = cursor.parse::<u32>().unwrap_or(0).saturating_add(1);
        if at < PAGES_MAX {
            at.to_string()
        } else {
            String::new()
        }
    }

    fn list_beacons(&self, cursor: &str) -> Json {
        let rows: Vec<Json> = if cursor.is_empty() {
            self.beacons
                .iter()
                .map(|beacon| {
                    let mut row = vec![
                        ("beacon_id", Json::String(beacon.id.clone())),
                        ("at", voxel_json(beacon.at)),
                        ("owner", Json::String(format!("seat.{}", beacon.owner))),
                    ];
                    if beacon.owner == self.seat {
                        row.push(("core", Json::Bool(beacon.core)));
                        row.push(("priority", Json::String(String::from("normal"))));
                        row.push(("powered", Json::Bool(beacon.powered)));
                    }
                    obj(row)
                })
                .collect()
        } else {
            Vec::new()
        };
        self.ok(obj(vec![
            ("beacons", Json::Array(rows)),
            ("next_cursor", Json::String(self.next_page(cursor))),
        ]))
    }

    fn get_view(&self, cursor: &str) -> Json {
        let next = self.next_page(cursor);
        let last = next.is_empty() && !self.view_never_complete;
        let chunks = if cursor.is_empty() {
            self.chunks()
        } else {
            Vec::new()
        };
        let entities: Vec<Json> = if last {
            self.entities
                .iter()
                .map(|entity| {
                    obj(vec![
                        ("id", Json::String(entity.id.clone())),
                        ("kind", Json::String(entity.kind.to_owned())),
                        ("subtype", Json::String(entity.subtype.to_owned())),
                        ("owner", Json::String(format!("seat.{}", entity.owner))),
                        ("at", voxel_json(entity.at)),
                    ])
                })
                .collect()
        } else {
            Vec::new()
        };
        self.ok(obj(vec![
            ("at_ms", num(0)),
            ("chunks", Json::Array(chunks)),
            ("entities", Json::Array(entities)),
            ("next_cursor", Json::String(next)),
            ("complete", Json::Bool(last)),
        ]))
    }

    fn list_templates(&self, cursor: &str) -> Json {
        let rows = if cursor.is_empty() {
            ["expand_and_mine", "hold_and_build", "safe_playbook"]
                .iter()
                .map(|id| obj(vec![("template_id", Json::String((*id).to_owned()))]))
                .collect()
        } else {
            Vec::new()
        };
        self.ok(obj(vec![
            ("templates", Json::Array(rows)),
            ("next_cursor", Json::String(self.next_page(cursor))),
        ]))
    }

    fn estimate_route(&self, params: &Json) -> Json {
        let waypoints = match params.get("waypoints") {
            Some(Json::Array(items)) => items.clone(),
            _ => return Script::refuse("INVALID_ARGUMENT"),
        };
        let (Some(from), Some(to)) = (
            waypoints.first().and_then(|w| self.place(w)),
            waypoints.get(1).and_then(|w| self.place(w)),
        ) else {
            return Script::refuse("INVALID_ARGUMENT");
        };
        let id = waypoints
            .get(1)
            .and_then(|w| w.get("beacon_anchor"))
            .and_then(|a| a.get("beacon_id"))
            .and_then(text)
            .map(str::to_owned);
        let ms = id
            .and_then(|id| self.to_beacon_ms.get(&id).copied())
            .unwrap_or_else(|| {
                let dx = i64::from(from[0] - to[0]).abs();
                let dy = i64::from(from[1] - to[1]).abs();
                (dx + dy) * 1_000
            });
        // `Leg.to` in the DECLARED shape, a `gp.v1.Location`, unless asked
        // for the bare voxel `main` wrote before T17.
        let leg_to = if self.bare_legs {
            voxel_json(to)
        } else {
            obj(vec![("voxel", voxel_json(to))])
        };
        self.ok(obj(vec![
            ("reachable", Json::Bool(true)),
            ("ms", num(ms)),
            (
                "legs",
                Json::Array(vec![obj(vec![
                    ("to", leg_to),
                    ("ms", num(ms)),
                    ("fogged", Json::Bool(false)),
                ])]),
            ),
        ]))
    }

    fn instantiate(&self, params: &Json) -> Json {
        let id = params.get("template_id").and_then(text).unwrap_or("");
        let Some((jsonc, declared)) = template(id) else {
            return Script::refuse("NOT_FOUND");
        };
        let parameters: Vec<Json> = declared
            .iter()
            .map(|(pointer, value)| {
                obj(vec![
                    ("pointer", Json::String(pointer.clone())),
                    ("label", Json::String(String::from("scripted"))),
                    ("value", Json::String(value.clone())),
                    ("suggested", Json::Bool(false)),
                ])
            })
            .collect();
        self.ok(obj(vec![
            ("playbook_jsonc", Json::String(jsonc)),
            ("parameters", Json::Array(parameters)),
            ("why", Json::String(String::new())),
        ]))
    }

    /// The scripted map, 32^3 chunks, run-length encoded.
    fn chunks(&self) -> Vec<Json> {
        let mut out: Vec<Json> = Vec::new();
        for cy in (0..self.size[1]).step_by(EDGE) {
            for cx in (0..self.size[0]).step_by(EDGE) {
                let mut voxels = vec![0_u8; EDGE * EDGE * EDGE];
                for ly in 0..EDGE {
                    for lx in 0..EDGE {
                        let x = cx + i32::try_from(lx).unwrap();
                        let y = cy + i32::try_from(ly).unwrap();
                        let top = self.features.get(&(x, y)).copied().unwrap_or(DIRT);
                        for z in 0..=GROUND {
                            let at = lx + EDGE * ly + EDGE * EDGE * usize::try_from(z).unwrap();
                            *voxels.get_mut(at).unwrap() = if z == GROUND { top } else { STONE };
                        }
                    }
                }
                let encoded = pharmakos_proto::chunk_rle::encode(&voxels);
                out.push(obj(vec![
                    ("origin", voxel_json([cx, cy, 0])),
                    (
                        "voxels_rle",
                        Json::String(pharmakos_proto::json::base64::encode(&encoded)),
                    ),
                ]));
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// The templates, read from the library as the gateway would declare them
// ---------------------------------------------------------------------------

/// The library folder: `<root>/library`.
fn library() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/operator sits two levels below the root")
        .join("library")
}

/// The rules text every client pins.
fn rules_json() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the root")
        .join("rules")
        .join("rules.v1.json");
    std::fs::read_to_string(path).expect("the rules text")
}

/// One template's text and its declared parameters with their own values,
/// the way `instantiate_template` with no parameters answers them.
fn template(id: &str) -> Option<(String, Vec<(String, String)>)> {
    let text = std::fs::read_to_string(library().join(format!("{id}.jsonc"))).ok()?;
    let body = read(&strip_comments(&text)).ok()?;
    let declared = match body.get("meta").and_then(|meta| meta.get("parameters")) {
        Some(Json::Array(items)) => items
            .iter()
            .filter_map(|item| item.get("pointer").and_then(text_of_json))
            .map(|pointer| {
                let value = pointer_get(&body, &pointer)
                    .map(compact)
                    .unwrap_or_default();
                (pointer, value)
            })
            .collect(),
        _ => Vec::new(),
    };
    Some((
        text.replace("\"kind\": \"TEMPLATE\"", "\"kind\": \"PLAYBOOK\""),
        declared,
    ))
}

fn text_of_json(value: &Json) -> Option<String> {
    text(value).map(str::to_owned)
}

fn pointer_get<'j>(root: &'j Json, pointer: &str) -> Option<&'j Json> {
    let mut at = root;
    for token in pointer.strip_prefix('/')?.split('/') {
        at = match at {
            Json::Object(_) => at.get(token)?,
            Json::Array(items) => items.get(token.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(at)
}

fn strip_comments(text: &str) -> String {
    let mut out = String::new();
    let mut in_string = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if c == '/' && chars.peek() == Some(&'/') {
            for skipped in chars.by_ref() {
                if skipped == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

fn obj(members: Vec<(&str, Json)>) -> Json {
    Json::Object(
        members
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn num(value: i64) -> Json {
    Json::Number(value.to_string())
}

fn text(value: &Json) -> Option<&str> {
    match value {
        Json::String(found) => Some(found),
        _ => None,
    }
}

fn voxel_json(at: [i32; 3]) -> Json {
    obj(vec![
        ("x", num(i64::from(at[0]))),
        ("y", num(i64::from(at[1]))),
        ("z", num(i64::from(at[2]))),
    ])
}

fn voxel_of(value: &Json) -> [i32; 3] {
    let axis = |name: &str| match value.get(name) {
        Some(Json::Number(lexeme)) => lexeme.parse::<i32>().unwrap_or(0),
        _ => 0,
    };
    [axis("x"), axis("y"), axis("z")]
}

/// One-line JSON, for the transcript.
fn compact(value: &Json) -> String {
    write(value)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Driving it
// ---------------------------------------------------------------------------

/// Play one round against a script, and return the script (with its
/// transcript) and what the operator said it did.
fn play(mut script: Script) -> (Script, pharmakos_operator::Played) {
    let mut easy = Easy::new(&rules_json()).expect("the rules text reads");
    let seat = script.seat;
    let mut call = |method: &str, params: Json| script.answer(method, &params);
    let played = easy.play(seat, &mut call);
    (script, played)
}

/// Advise one round against a script.
fn advise(mut script: Script) -> (Script, pharmakos_operator::Advice) {
    let mut easy = Easy::new(&rules_json()).expect("the rules text reads");
    let seat = script.seat;
    let mut call = |method: &str, params: Json| script.answer(method, &params);
    let advice = easy.advise(seat, &mut call);
    (script, advice)
}

/// The `instantiate_template` calls that named the Safe Playbook with a
/// route, as the route's JSON.
fn safe_routes(transcript: &[String]) -> Vec<Json> {
    transcript
        .iter()
        .filter_map(|line| line.strip_prefix("instantiate_template "))
        .filter_map(|params| read(params).ok())
        .filter(|params| params.get("template_id").and_then(text) == Some("safe_playbook"))
        .filter_map(|params| match params.get("parameters") {
            Some(Json::Array(items)) => items.first().cloned(),
            _ => None,
        })
        .filter_map(|parameter| {
            parameter
                .get("value")
                .and_then(text)
                .and_then(|v| read(v).ok())
        })
        .collect()
}

/// A power-short seat with three browned-out non-core beacons, all within
/// 60 s by travel, the nearest by travel NOT the nearest by id.
fn three_dark(seat: u8) -> Script {
    let mut script = Script::new(seat);
    script.headroom = -3;
    for (id, at, ms) in [
        ("b_04", [40, 40, GROUND + 1], 30_000),
        ("b_05", [44, 44, GROUND + 1], 10_000),
        ("b_06", [20, 20, GROUND + 1], 20_000),
    ] {
        script.beacons.push(Beacon {
            id: id.to_owned(),
            at,
            owner: seat,
            core: false,
            powered: false,
        });
        script.to_beacon_ms.insert(id.to_owned(), ms);
    }
    script
}

// ---------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------

/// Spec section 14: "If power is short and up to 2 at-risk beacons are within
/// 60 s travel, raise their Quartermaster priority (nearest first ...)".
/// Three browned-out non-core beacons, the nearest by travel being neither the
/// lowest id nor the nearest in a straight line: the route is `to_safety`,
/// then the two nearest by travel in that order, each one interface step with
/// one `set_priority` row and nothing else.
#[test]
fn the_safe_playbook_raises_at_most_two_beacons_nearest_first() {
    let (script, advice) = advise(three_dark(1));
    let routes = safe_routes(&script.transcript);
    assert_eq!(
        routes.len(),
        1,
        "one instantiate with the route: {:#?}",
        script.transcript
    );
    let Some(Json::Array(steps)) = routes.first() else {
        panic!("the route is an array");
    };
    assert_eq!(steps.len(), 1 + SAFE_MAX_RAISED, "to_safety and two raises");
    let labels: Vec<String> = steps
        .iter()
        .map(|step| step.get("label").and_then(text).unwrap_or("").to_owned())
        .collect();
    assert_eq!(labels, ["to_safety", "raise_b_05", "raise_b_06"]);
    for step in steps.iter().skip(1) {
        let interface = step.get("interface").expect("an interface step");
        assert_eq!(
            compact(interface.get("rows").unwrap()),
            compact(&read(r#"[{"set_priority":"HIGH"}]"#).unwrap()),
            "one row, priority HIGH: never recycle, switch mandate or place"
        );
        assert!(step.get("place_beacon").is_none() && step.get("move").is_none());
    }
    assert!(!advice.safe_playbook_jsonc.is_empty());
    let safe = advice
        .suggestions
        .iter()
        .find(|suggestion| suggestion.template_id == "safe_playbook")
        .expect("a suggestion for the safe template");
    assert_eq!(safe.parameters.len(), 1, "the route, whole");
    assert_eq!(
        safe.parameters.first().unwrap().pointer,
        "/declarative/route"
    );

    // Out of reach: past 60 s nothing is raised, and a seat with power to
    // spare raises nothing whatever is dark.
    let mut far = three_dark(1);
    for ms in far.to_beacon_ms.values_mut() {
        *ms = 61_000;
    }
    let (script, _) = advise(far);
    assert!(
        safe_routes(&script.transcript).is_empty(),
        "nothing within 60 s"
    );
    let mut spare = three_dark(1);
    spare.headroom = 0;
    let (script, _) = advise(spare);
    assert!(
        safe_routes(&script.transcript).is_empty(),
        "power is not short"
    );
    assert!(
        !script
            .transcript
            .iter()
            .any(|line| line.starts_with("estimate_route") && line.contains("beacon_anchor")),
        "and not even estimated"
    );
}

/// Decision C15: the (match, seat, round) seed is read, recorded in the
/// "why", and unused -- in the why of a safe seal and of every suggestion
/// too, not only in a composed plan's note.
#[test]
fn the_seed_is_recorded_in_every_why() {
    let (_, played) = play(Script::new(0));
    assert_eq!(played.submitted, Submitted::Safe, "{played:#?}");
    assert!(
        played.why.contains("seed 0x0000000000005eed"),
        "{}",
        played.why
    );
    let (_, advice) = advise(Script::busy(0));
    assert_eq!(advice.suggestions.len(), 3);
    for suggestion in &advice.suggestions {
        assert!(
            suggestion.why.contains("seed 0x0000000000005eed"),
            "{suggestion:#?}"
        );
    }
}

/// A `get_map_summary` size the operator cannot hold is a round it cannot
/// read: it asks for no view, plans nothing, and still says ready -- rather
/// than allocating a column per `(x, y)` of a map it was told is 2^31 wide.
#[test]
fn a_map_too_large_to_hold_is_a_round_not_read_and_ready_is_still_said() {
    let mut huge = Script::busy(0);
    huge.size = [i32::MAX, i32::MAX, 32];
    let (script, played) = play(huge);
    assert_eq!(played.submitted, Submitted::Nothing);
    assert!(played.ready);
    let methods: Vec<&str> = script
        .transcript
        .iter()
        .map(|line| line.split(' ').next().unwrap_or(""))
        .collect();
    assert_eq!(
        methods,
        [
            "get_status",
            "get_briefing",
            "list_beacons",
            "get_economy_forecast",
            "get_map_summary",
            "set_ready"
        ]
    );
}

/// A view that never completes and names no page after it is read once, not
/// again from the empty cursor; with no entity list there is no commander,
/// so Easy seals its safe playbook and its why says the view was short.
#[test]
fn an_incomplete_view_is_read_once_and_said_in_the_why() {
    let mut short = Script::busy(0);
    short.view_never_complete = true;
    let (script, played) = play(short);
    assert_eq!(
        script
            .transcript
            .iter()
            .filter(|line| line.starts_with("get_view"))
            .count(),
        1
    );
    assert_eq!(played.submitted, Submitted::Safe);
    assert!(played.why.contains("did not complete"), "{}", played.why);
}

/// Decisions-log item 107 (5): a unit's or a structure's id is a handle
/// minted per viewer in the order it first saw the thing. Permute every
/// `u_`/`s_` handle in the view, and the order the view lists them in: the
/// operator's every call, and its playbook, are byte-identical.
#[test]
fn the_operator_does_not_key_on_viewer_scoped_handles() {
    let base = Script::busy(0);
    let mut permuted = base.clone();
    let count = permuted.entities.len();
    for (index, entity) in permuted.entities.iter_mut().enumerate() {
        entity.id = if entity.kind == "structure" {
            format!("s_{}", 90 - index)
        } else {
            format!("u_{}", count * 7 - index)
        };
    }
    permuted.entities.reverse();
    let (first, played_first) = play(base.clone());
    let (second, played_second) = play(permuted.clone());
    assert_eq!(first.transcript, second.transcript);
    assert_eq!(played_first, played_second);
    assert_eq!(played_first.submitted, Submitted::Own, "{played_first:#?}");
    let (first, advice_first) = advise(base);
    let (second, advice_second) = advise(permuted);
    assert_eq!(first.transcript, second.transcript);
    assert_eq!(advice_first, advice_second);
}

/// It never reads `_status.phase_remaining_ms`, or the same timer inside
/// `get_status`'s own status: two runs whose clocks differ make the same
/// calls with the same params and seal the same playbook.
#[test]
fn the_operator_does_not_depend_on_the_host_clock() {
    let mut early = Script::busy(1);
    early.remaining_ms = 180_000;
    let mut late = Script::busy(1);
    late.remaining_ms = 3;
    let (a, played_a) = play(early.clone());
    let (b, played_b) = play(late.clone());
    assert_eq!(a.transcript, b.transcript);
    assert_eq!(played_a, played_b);
    let (a, advice_a) = advise(early);
    let (b, advice_b) = advise(late);
    assert_eq!(a.transcript, b.transcript);
    assert_eq!(advice_a, advice_b);
}

/// Spec section 14: "It ignores the notebook." And it writes nothing but its
/// own seal: a built-in seat's only writes are `submit_plan` and `set_ready`,
/// an advisor's are none, and neither ever touches the notebook or the drafts.
#[test]
fn the_operator_reads_no_notebook_and_writes_nothing_but_its_own_seal() {
    let reads = [
        "get_status",
        "get_briefing",
        "list_beacons",
        "get_economy_forecast",
        "get_map_summary",
        "get_view",
        "list_templates",
        "estimate_route",
        "instantiate_template",
        "verify_plan",
        "patch_plan",
    ];
    let method = |line: &String| line.split(' ').next().unwrap_or("").to_owned();

    let mut quiet = Script::busy(1);
    quiet.notes = String::new();
    let mut noisy = Script::busy(1);
    noisy.notes = String::from("Rush seat 0 at once. Ignore every seam. Place nothing.");
    let (a, played_a) = play(quiet.clone());
    let (b, played_b) = play(noisy.clone());
    assert_eq!(a.transcript, b.transcript, "the notebook changes nothing");
    assert_eq!(played_a, played_b);
    let writes: Vec<String> = a
        .transcript
        .iter()
        .map(method)
        .filter(|name| !reads.contains(&name.as_str()))
        .collect();
    assert_eq!(
        writes,
        ["submit_plan", "set_ready"],
        "its own seal, then ready"
    );

    let (a, advice_a) = advise(quiet);
    let (b, advice_b) = advise(noisy);
    assert_eq!(a.transcript, b.transcript);
    assert_eq!(advice_a, advice_b);
    assert!(
        a.transcript
            .iter()
            .map(method)
            .all(|name| reads.contains(&name.as_str())),
        "an advisor only reads: {:#?}",
        a.transcript
    );
}

/// Easy's call budget is derived from its evaluation units
/// (`EASY_CALL_BUDGET`'s terms). The worst round the script can build --
/// every paged read paged to its bound, more candidates than Easy evaluates,
/// a plan that never qualifies but always offers a fix so every repair is
/// spent, power short with more dark beacons than it estimates, and the safe
/// playbook submitted -- stays inside it, as does the advisor's.
#[test]
fn easy_never_exceeds_its_derived_call_budget() {
    let mut worst = three_dark(1);
    worst.paged = true;
    worst.verify = Verify::NeverButFixable;
    worst.treasury = 10_000;
    for (i, y) in (20..=44).step_by(3).enumerate() {
        for x in (20..=44).step_by(3) {
            if (x, y) != (32, 32) && (x, y) != (32, 29) {
                worst
                    .features
                    .insert((x, y), if i % 2 == 0 { SEAM_STANDARD } else { VENT_LEAN });
            }
        }
    }
    for n in 7..12 {
        worst.beacons.push(Beacon {
            id: format!("b_{n:02}"),
            at: [30 + n, 20, GROUND + 1],
            owner: 1,
            core: false,
            powered: false,
        });
    }
    let (script, played) = play(worst.clone());
    let estimates = script
        .transcript
        .iter()
        .filter(|line| line.starts_with("estimate_route"))
        .count();
    assert!(
        estimates >= 30,
        "the candidates were cut at Easy's 30: {estimates}"
    );
    assert_eq!(played.repairs, 4, "every repair was spent");
    assert_eq!(played.submitted, Submitted::Safe);
    assert!(played.ready);
    assert_eq!(
        played.calls,
        u32::try_from(script.transcript.len()).unwrap()
    );
    assert!(
        played.calls <= EASY_CALL_BUDGET,
        "{} calls against a budget of {EASY_CALL_BUDGET}",
        played.calls
    );
    // And the budget is the derivation, not a number sized to fit: this round
    // spends every term but one `submit_plan` (its own plan never qualified,
    // so only the safe one was submitted).
    assert_eq!(played.calls, EASY_CALL_BUDGET - 1);

    // The other worst branch: its own plan qualifies and is refused at submit.
    let mut refused = worst.clone();
    refused.verify = Verify::Qualifies;
    refused.accept_submit = false;
    let (script, played) = play(refused);
    assert_eq!(played.submitted, Submitted::Safe);
    assert_eq!(
        script
            .transcript
            .iter()
            .filter(|line| line.starts_with("submit_plan"))
            .count(),
        2
    );
    assert!(played.calls <= EASY_CALL_BUDGET);

    let (script, advice) = advise(worst);
    assert_eq!(
        advice.calls,
        u32::try_from(script.transcript.len()).unwrap()
    );
    assert!(
        advice.calls <= EASY_ADVISOR_CALL_BUDGET,
        "{} calls against an advisor budget of {EASY_ADVISOR_CALL_BUDGET}",
        advice.calls
    );
    assert_eq!(advice.calls, EASY_ADVISOR_CALL_BUDGET, "every term, spent");
}

/// A built-in seat ends by calling `set_ready` whatever else happened, so the
/// lobby's Ready can end a Lull (T19 PR 1's finding 6): even when every call
/// before it is refused.
#[test]
fn a_built_in_seat_says_ready_even_when_it_could_plan_nothing() {
    let mut refusing = Script::busy(0);
    refusing.refuse_everything = Some("INTERNAL");
    let (script, played) = play(refusing);
    assert_eq!(played.submitted, Submitted::Nothing);
    assert_eq!(
        script
            .transcript
            .last()
            .map(|line| line.split(' ').next().unwrap_or("")),
        Some("set_ready")
    );
}

/// The busy seat's round, told plainly: Easy fills Expand & Mine with a site
/// beside a seam inside its core's sphere, and its note records the seed it
/// did not use.
#[test]
fn easy_fills_a_template_by_the_pointers_it_declares() {
    let (script, played) = play(Script::busy(0));
    assert_eq!(played.submitted, Submitted::Own);
    let patch = script
        .transcript
        .iter()
        .find_map(|line| line.strip_prefix("patch_plan "))
        .and_then(|params| read(params).ok())
        .and_then(|params| params.get("json_patch").and_then(text).map(str::to_owned))
        .expect("one composing patch");
    let ops = read(&patch).expect("a JSON Patch");
    let Json::Array(ops) = ops else {
        panic!("an array of operations");
    };
    let paths: Vec<&str> = ops
        .iter()
        .filter_map(|op| op.get("path").and_then(text))
        .collect();
    assert!(paths.contains(&"/meta/note"), "{paths:?}");
    let note = ops
        .iter()
        .find(|op| op.get("path").and_then(text) == Some("/meta/note"))
        .and_then(|op| op.get("value").and_then(text))
        .unwrap();
    assert!(note.contains("seed 0x0000000000005eed"), "{note}");
    assert!(
        played.why.contains("seed 0x0000000000005eed"),
        "{}",
        played.why
    );
    assert!(
        paths.iter().any(|path| path.ends_with("/place_beacon/at"))
            || paths.iter().any(|path| path.ends_with("/anchor/voxel")),
        "{paths:?}"
    );
}

/// The JSON Patch operations the one composing `patch_plan` carried.
fn composing_ops(transcript: &[String]) -> Vec<Json> {
    let patch = transcript
        .iter()
        .find_map(|line| line.strip_prefix("patch_plan "))
        .and_then(|params| read(params).ok())
        .and_then(|params| params.get("json_patch").and_then(text).map(str::to_owned))
        .expect("one composing patch");
    match read(&patch).expect("a JSON Patch") {
        Json::Array(ops) => ops,
        other => panic!("an array of operations, not {other:?}"),
    }
}

/// Spec section 14: "compose visit goals by greedy insertion by utility per
/// second until the route fills its target share of the segment". Two seams
/// inside the core's sphere and money for two beacons: the second site is
/// inserted as one more walk-and-place before the walk home. Money for one:
/// it is not.
#[test]
fn easy_inserts_goals_greedily_while_the_share_and_the_treasury_allow() {
    let mut rich = Script::new(0);
    rich.features.insert((38, 30), SEAM_STANDARD);
    rich.features.insert((26, 30), SEAM_STANDARD);
    rich.treasury = 500;
    let (script, played) = play(rich.clone());
    assert_eq!(played.submitted, Submitted::Own);
    let ops = composing_ops(&script.transcript);
    let added: Vec<String> = ops
        .iter()
        .filter(|op| op.get("op").and_then(text) == Some("add"))
        .filter_map(|op| op.get("path").and_then(text).map(str::to_owned))
        .collect();
    assert_eq!(
        added,
        ["/declarative/route/2", "/declarative/route/3"],
        "{ops:#?}"
    );

    let mut poor = rich;
    poor.treasury = 100;
    let (script, _) = play(poor);
    assert!(
        composing_ops(&script.transcript)
            .iter()
            .all(|op| op.get("op").and_then(text) == Some("replace")),
        "one beacon's worth of money, one site"
    );
}

/// A heat vent inside the core's sphere and no seam: Easy fills Hold & Build,
/// its Generator anchor and a hold that fills what is left of the share.
#[test]
fn easy_fills_hold_and_build_when_a_vent_is_inside_the_core_sphere() {
    let mut vent = Script::new(1);
    vent.features.insert((36, 36), VENT_LEAN);
    let (script, played) = play(vent);
    assert_eq!(played.submitted, Submitted::Own, "{:#?}", script.transcript);
    let ops = composing_ops(&script.transcript);
    let anchor = ops
        .iter()
        .find(|op| {
            op.get("path")
                .and_then(text)
                .is_some_and(|p| p.ends_with("/anchor/voxel"))
        })
        .and_then(|op| op.get("value").cloned())
        .expect("the anchor is filled");
    assert_eq!(voxel_of(&anchor), [36, 36, GROUND + 1]);
    assert!(
        ops.iter().any(|op| op
            .get("path")
            .and_then(text)
            .is_some_and(|p| p.ends_with("/hold/ms"))),
        "the hold fills the share: {ops:#?}"
    );
}

/// `estimate_route`'s `Leg.to` is a `gp.v1.Location` as declared; `main`
/// wrote a bare voxel until T17 fixed it. Easy reads both, and plays the same
/// round from either.
#[test]
fn a_leg_is_read_in_the_declared_shape_and_the_bare_one() {
    let declared = Script::busy(1);
    let mut bare = declared.clone();
    bare.bare_legs = true;
    let (a, played_a) = play(declared);
    let (b, played_b) = play(bare);
    assert_eq!(a.transcript, b.transcript);
    assert_eq!(played_a, played_b);
    assert_eq!(played_a.submitted, Submitted::Own);
}

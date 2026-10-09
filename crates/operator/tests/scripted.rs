// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The operator driven through a **scripted client**: a closure that answers
//! the gateway's methods from a small, hand-built world.
//!
//! The operator's only input is the wire (and the public rules text), which is
//! what makes a scripted client an honest fixture (decisions-log item 111,
//! section F, "Build 12"): it can put three lit beacons in front of the safe
//! playbook, permute every viewer-scoped handle, or move the host clock,
//! without a match or a host test seam. Every test that hosts a real match is
//! in `crates/gamectl/tests/operator.rs`, where the adapter is.
//!
//! The scripted world is the wire's since S1's targeting: a list of named
//! vents and seams (`get_map_summary.features`), and an `estimate_route` that
//! answers a `covering` waypoint with the site the world says the sim's
//! covering rule would choose, or refuses it when none would. The scripted
//! gateway does not apply a patch or run a verifier: it records every call,
//! method and params, into a transcript, and the tests compare transcripts --
//! which is a stronger claim than comparing the playbook alone, because every
//! estimate, every patch and every submission is in it.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use pharmakos_operator::easy::{
    CORE_DIG_MAX_DEPTH, EASY_ADVISOR_CALL_BUDGET, EASY_CALL_BUDGET, PAGES_MAX, SAFE_MAX_RAISED,
};
use pharmakos_operator::{Easy, Submitted};
use pharmakos_proto::json::{Json, read, write};

// ---------------------------------------------------------------------------
// The scripted world
// ---------------------------------------------------------------------------

/// The ground's standing height.
const GROUND: i32 = 11;

/// One beacon in the scripted `list_beacons` answer.
#[derive(Clone)]
struct Beacon {
    id: String,
    at: [i32; 3],
    owner: u8,
    core: bool,
    powered: bool,
    /// The lower-case wire value `list_beacons` answers for an own beacon.
    priority: &'static str,
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

/// One vent or seam in the scripted `get_map_summary.features`.
#[derive(Clone)]
struct Feature {
    /// `vent_<x>_<y>` or `seam_<x>_<y>`.
    id: String,
    /// The wire's lower-case kind.
    kind: &'static str,
    /// The wire's lower-case grade.
    grade: &'static str,
    anchor: [i32; 2],
    live: bool,
    covered: bool,
    /// Travel from the commander; `None` when unreachable.
    travel_ms: Option<i64>,
    /// The site a `covering` waypoint naming it answers; `None` when the
    /// gateway refuses it (`NOT_FOUND`, no site covers it).
    site: Option<[i32; 3]>,
}

impl Feature {
    fn new(kind: &'static str, grade: &'static str, anchor: [i32; 2]) -> Feature {
        let [x, y] = anchor;
        Feature {
            id: format!("{kind}_{x}_{y}"),
            kind,
            grade,
            anchor,
            live: true,
            covered: false,
            travel_ms: Some(10_000),
            site: Some([x - 4, y, GROUND]),
        }
    }
}

/// How the scripted verifier answers a FULL verify.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verify {
    /// Every playbook qualifies.
    Qualifies,
    /// No playbook qualifies, and every report offers a machine-applicable fix.
    NeverButFixable,
}

/// How the scripted paged reads page.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Paging {
    /// One page each, the last and complete.
    One,
    /// Every paged read paged to [`PAGES_MAX`].
    ToTheBound,
    /// One page each, but `get_view`'s is never complete (and names no page
    /// after it).
    ViewNeverComplete,
}

/// The scripted gateway.
#[derive(Clone)]
struct Script {
    seat: u8,
    round: u32,
    segment_ms: i64,
    remaining_ms: i64,
    notes: String,
    features: Vec<Feature>,
    beacons: Vec<Beacon>,
    entities: Vec<Entity>,
    treasury: i64,
    supply: i64,
    draw: i64,
    /// Travel from anywhere to a beacon, by id, overriding the distance rule.
    to_beacon_ms: BTreeMap<String, i64>,
    /// The `(beacon, vent)` pairs `resolve_refs` reads an `on` for.
    on_reads: BTreeSet<(String, String)>,
    /// The beacons on the Build mandate; a tap at any other answers `E0503`.
    build_beacons: BTreeSet<String>,
    /// How the paged reads page.
    paging: Paging,
    verify: Verify,
    accept_submit: bool,
    /// Refuse every call with this code.
    refuse_everything: Option<&'static str>,
    /// Answer `Leg.to` as the bare voxel `main` wrote before T17.
    bare_legs: bool,
    /// Answer one feature row with no grade.
    malformed_feature: bool,
    transcript: Vec<String>,
}

impl Script {
    /// A seat with its core `b_00` in the middle of a 64 x 64 map, its
    /// commander beside it, and nothing else.
    fn new(seat: u8) -> Script {
        Script {
            seat,
            round: 1,
            segment_ms: 180_000,
            remaining_ms: 180_000,
            notes: String::new(),
            features: Vec::new(),
            beacons: vec![Beacon {
                id: String::from("b_00"),
                at: [32, 32, GROUND],
                owner: seat,
                core: true,
                powered: true,
                priority: "normal",
            }],
            entities: vec![Entity {
                id: String::from("u_1"),
                kind: "unit",
                subtype: "commander",
                owner: seat,
                at: [32, 30, GROUND],
            }],
            treasury: 200,
            supply: 10,
            draw: 4,
            to_beacon_ms: BTreeMap::new(),
            on_reads: BTreeSet::new(),
            build_beacons: BTreeSet::from([String::from("b_00")]),
            paging: Paging::One,
            verify: Verify::Qualifies,
            accept_submit: true,
            refuse_everything: None,
            bare_legs: false,
            malformed_feature: false,
            transcript: Vec::new(),
        }
    }

    /// A seat with its starting seam covered by the core, a lean vent and a
    /// standard seam it does not cover yet, and an enemy scout and a
    /// Generator in sight, with viewer-scoped handles.
    fn busy(seat: u8) -> Script {
        let mut script = Script::new(seat);
        let mut start = Feature::new("seam", "standard", [36, 34]);
        start.covered = true;
        start.travel_ms = Some(3_000);
        script.features.push(start);
        let mut vent = Feature::new("vent", "lean", [56, 32]);
        vent.travel_ms = Some(14_000);
        vent.site = Some([46, 32, GROUND]);
        script.features.push(vent);
        let mut far_seam = Feature::new("seam", "standard", [20, 60]);
        far_seam.travel_ms = Some(20_000);
        far_seam.site = Some([24, 52, GROUND]);
        script.features.push(far_seam);
        script.entities.push(Entity {
            id: String::from("u_2"),
            kind: "unit",
            subtype: "build_drone",
            owner: seat,
            at: [33, 33, GROUND],
        });
        script.entities.push(Entity {
            id: String::from("u_3"),
            kind: "unit",
            subtype: "scout",
            owner: seat ^ 1,
            at: [26, 26, GROUND],
        });
        script.entities.push(Entity {
            id: String::from("s_1"),
            kind: "structure",
            subtype: "generator",
            owner: seat ^ 1,
            at: [60, 60, GROUND],
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

    /// Where a `gp.v1.Location` waypoint is in the scripted world, or the
    /// refusal a `covering` one that no site covers gets.
    fn place(&self, location: &Json) -> Result<[i32; 3], &'static str> {
        if let Some(voxel) = location.get("voxel") {
            return Ok(voxel_of(voxel));
        }
        if let Some(name) = location
            .get("covering")
            .and_then(|covering| covering.get("feature_id"))
            .and_then(text)
        {
            return self
                .features
                .iter()
                .find(|feature| feature.id == name)
                .and_then(|feature| feature.site)
                .ok_or("NOT_FOUND");
        }
        let id = location
            .get("beacon_anchor")
            .and_then(|anchor| anchor.get("beacon_id"))
            .and_then(text)
            .ok_or("INVALID_ARGUMENT")?;
        self.beacons
            .iter()
            .find(|beacon| beacon.id == id)
            .map(|beacon| beacon.at)
            .ok_or("NOT_FOUND")
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
                ("supply_kw_now", num(self.supply)),
                ("draw_kw_now", num(self.draw)),
                ("headroom_kw_now", num(self.supply - self.draw)),
            ])),
            "get_map_summary" => self.map_summary(),
            "get_view" => self.get_view(&cursor),
            "list_templates" => self.list_templates(&cursor),
            "estimate_route" => self.estimate_route(params),
            "instantiate_template" => self.instantiate(params),
            "resolve_refs" => self.resolve_refs(params),
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
            "verify_plan" => self.verify_plan(params),
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
        if self.paging != Paging::ToTheBound {
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
                        row.push(("priority", Json::String(beacon.priority.to_owned())));
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

    fn map_summary(&self) -> Json {
        let rows: Vec<Json> = self
            .features
            .iter()
            .enumerate()
            .map(|(index, feature)| {
                let mut row = vec![
                    ("feature_id", Json::String(feature.id.clone())),
                    ("kind", Json::String(feature.kind.to_owned())),
                ];
                if !(self.malformed_feature && index == 0) {
                    row.push(("grade", Json::String(feature.grade.to_owned())));
                }
                row.push(("x", num(i64::from(feature.anchor[0]))));
                row.push(("y", num(i64::from(feature.anchor[1]))));
                row.push(("live", Json::Bool(feature.live)));
                row.push(("covered", Json::Bool(feature.covered)));
                row.push(("travel_ms", num(feature.travel_ms.unwrap_or(0))));
                row.push(("reachable", Json::Bool(feature.travel_ms.is_some())));
                obj(row)
            })
            .collect();
        self.ok(obj(vec![
            ("size", voxel_json([64, 64, 32])),
            (
                "match_seed",
                Json::String(String::from("0x0000000000005eed")),
            ),
            ("features", Json::Array(rows)),
        ]))
    }

    fn get_view(&self, cursor: &str) -> Json {
        let next = self.next_page(cursor);
        let last = next.is_empty() && self.paging != Paging::ViewNeverComplete;
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
            ("chunks", Json::Array(Vec::new())),
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
        let (Some(from), Some(to)) = (waypoints.first(), waypoints.get(1)) else {
            return Script::refuse("INVALID_ARGUMENT");
        };
        let (from, to) = match (self.place(from), self.place(to)) {
            (Ok(from), Ok(to)) => (from, to),
            (Err(code), _) | (_, Err(code)) => return Script::refuse(code),
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
                (dx + dy) * 500
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

    /// The interface steps that add a Generator `on` a named vent, as
    /// `(route index, beacon, vent)`.
    fn taps_in(params: &Json) -> Vec<(usize, String, String)> {
        let text_in = params.get("playbook_jsonc").and_then(text).unwrap_or("");
        let Ok(playbook) = read(text_in) else {
            return Vec::new();
        };
        let Some(Json::Array(route)) = playbook
            .get("declarative")
            .and_then(|declarative| declarative.get("route"))
        else {
            return Vec::new();
        };
        route
            .iter()
            .enumerate()
            .filter_map(|(index, step)| {
                let interface = step.get("interface")?;
                let beacon = interface.get("beacon")?.get("beacon_id").and_then(text)?;
                let Json::Array(rows) = interface.get("rows")? else {
                    return None;
                };
                let vent = rows
                    .first()?
                    .get("add_build_target")?
                    .get("target")?
                    .get("anchor")?
                    .get("on")?
                    .get("feature_id")
                    .and_then(text)?;
                Some((index, beacon.to_owned(), vent.to_owned()))
            })
            .collect()
    }

    fn resolve_refs(&self, params: &Json) -> Json {
        let refs: Vec<Json> = Script::taps_in(params)
            .into_iter()
            .map(|(index, beacon, vent)| {
                let reads = self.on_reads.contains(&(beacon, vent.clone()));
                obj(vec![
                    (
                        "pointer",
                        Json::String(format!(
                            "/declarative/route/{index}/interface/rows/0/add_build_target/target/anchor/on"
                        )),
                    ),
                    (
                        "feature_id",
                        Json::String(if reads { vent } else { String::new() }),
                    ),
                    ("matched", num(i64::from(reads))),
                    (
                        "failure",
                        Json::String(if reads {
                            String::new()
                        } else {
                            String::from("illegal_site")
                        }),
                    ),
                ])
            })
            .collect();
        self.ok(obj(vec![("refs", Json::Array(refs))]))
    }

    fn verify_plan(&self, params: &Json) -> Json {
        let mut diagnostics: Vec<Json> = Script::taps_in(params)
            .into_iter()
            .filter(|(_, beacon, _)| !self.build_beacons.contains(beacon))
            .map(|(index, _, _)| {
                obj(vec![
                    ("code", Json::String(String::from("E0503"))),
                    ("severity", Json::String(String::from("error"))),
                    (
                        "path",
                        Json::String(format!(
                            "/declarative/route/{index}/interface/rows/0/add_build_target"
                        )),
                    ),
                ])
            })
            .collect();
        let quick = params.get("depth").and_then(text) == Some("quick");
        let qualifies = diagnostics.is_empty() && (quick || self.verify == Verify::Qualifies);
        if !quick && self.verify == Verify::NeverButFixable {
            diagnostics.push(read(
                r#"{"code":"E0301","severity":"error","path":"/declarative/route/0","suggestions":[{"title":"fix","json_patch":"[{\"op\":\"remove\",\"path\":\"/meta/note\"}]","applicability":"machine_applicable"}]}"#,
            ).unwrap());
        }
        self.ok(obj(vec![(
            "report",
            obj(vec![
                ("qualifies", Json::Bool(qualifies)),
                ("diagnostics", Json::Array(diagnostics)),
            ]),
        )]))
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

/// The methods of a transcript, in order.
fn methods(transcript: &[String]) -> Vec<&str> {
    transcript
        .iter()
        .map(|line| line.split(' ').next().unwrap_or(""))
        .collect()
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

/// The labels of the one safe route the round instantiated, or none.
fn safe_labels(transcript: &[String]) -> Vec<String> {
    let routes = safe_routes(transcript);
    let Some(Json::Array(steps)) = routes.first() else {
        return Vec::new();
    };
    steps
        .iter()
        .map(|step| step.get("label").and_then(text).unwrap_or("").to_owned())
        .collect()
}

/// True when the round estimated a route to a beacon: the safe playbook's
/// estimates, and a tap's, are the only ones that end on a `beacon_anchor`.
fn beacon_estimated(transcript: &[String]) -> bool {
    transcript
        .iter()
        .any(|line| line.starts_with("estimate_route") && line.contains("beacon_anchor"))
}

/// The why of an advice's suggestion for one template.
fn why_of(advice: &pharmakos_operator::Advice, template: &str) -> String {
    advice
        .suggestions
        .iter()
        .find(|suggestion| suggestion.template_id == template)
        .map_or_else(
            || panic!("a suggestion for {template}"),
            |suggestion| suggestion.why.clone(),
        )
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

/// The value of the first operation of `kind` at `path`.
fn op_value(ops: &[Json], kind: &str, path: &str) -> Option<Json> {
    ops.iter()
        .find(|op| {
            op.get("op").and_then(text) == Some(kind) && op.get("path").and_then(text) == Some(path)
        })
        .and_then(|op| op.get("value").cloned())
}

/// The seed line every "why" opens with (decision C15).
const SEED_LINE: &str =
    "Easy, seat 1, round 1, seed 0x0000000000005eed (recorded; Easy draws nothing at random).";

/// The sentence every safe why ends with when the route writes the core's
/// depth.
fn deepen_sentence() -> String {
    format!(
        " Then set the core b_00 to dig {CORE_DIG_MAX_DEPTH} voxels deep, so its starting drone \
         keeps working its seam below the top layer."
    )
}

/// A power-short seat -- its draw 1 kW above its supply -- with four lit
/// non-core beacons: `b_04` NORMAL, far from the core; `b_05` LOW, nearest the
/// core; `b_06` NORMAL, near the core; `b_07` HIGH. The brownout's order over
/// them sheds `b_05` first (lowest priority), then `b_04` (furthest), then
/// `b_06`; `b_07` is HIGH and is not at risk. By travel `b_04` is nearer than
/// `b_05`.
fn short_with_four_lit(seat: u8) -> Script {
    let mut script = Script::new(seat);
    script.supply = 10;
    script.draw = 11;
    for (id, at, priority, ms) in [
        ("b_04", [56, 56, GROUND], "normal", 10_000),
        ("b_05", [34, 32, GROUND], "low", 30_000),
        ("b_06", [36, 36, GROUND], "normal", 5_000),
        ("b_07", [60, 60, GROUND], "high", 1_000),
    ] {
        script.beacons.push(Beacon {
            id: id.to_owned(),
            at,
            owner: seat,
            core: false,
            powered: true,
            priority,
        });
        script.to_beacon_ms.insert(id.to_owned(), ms);
    }
    script
}

// ---------------------------------------------------------------------------
// The safe playbook
// ---------------------------------------------------------------------------

/// Spec section 14 as S1's plan, decision 14, reads it (the register's
/// S1-21): power is short, and the beacons at risk are the lit ones next in
/// the shed order. The two first in that order -- `b_05`, the lowest
/// priority, then `b_04`, the furthest from the core -- are raised, walked to
/// nearest by travel first, each one interface step with one `set_priority`
/// row and nothing else; `b_06` comes after them in the order and `b_07` is
/// HIGH already. The route ends at the core with its dig depth.
#[test]
fn the_safe_playbook_raises_the_next_two_in_the_shed_order_nearest_first() {
    let (script, advice) = advise(short_with_four_lit(1));
    let routes = safe_routes(&script.transcript);
    assert_eq!(
        routes.len(),
        1,
        "one instantiate with the route: {:#?}",
        script.transcript
    );
    assert_eq!(
        safe_labels(&script.transcript),
        ["to_safety", "raise_b_04", "raise_b_05", "deepen_core"]
    );
    let Some(Json::Array(steps)) = routes.first() else {
        panic!("the route is an array");
    };
    for step in steps.iter().skip(1).take(SAFE_MAX_RAISED) {
        let interface = step.get("interface").expect("an interface step");
        assert_eq!(
            compact(interface.get("rows").unwrap()),
            compact(&read(r#"[{"set_priority":"HIGH"}]"#).unwrap()),
            "one row, priority HIGH: never recycle, switch mandate or place"
        );
        assert!(step.get("place_beacon").is_none() && step.get("move").is_none());
    }
    // Only the beacons at risk were estimated: never the HIGH one.
    assert!(
        !script
            .transcript
            .iter()
            .any(|line| line.starts_with("estimate_route") && line.contains("b_07")),
        "{:#?}",
        script.transcript
    );
    let why = why_of(&advice, "safe_playbook");
    assert_eq!(
        why,
        format!(
            "{SEED_LINE} Power is short because the draw of 11 kW is above the supply of 10 kW: \
             raise b_04 and b_05 to HIGH, nearest first, so they are the last of your lit \
             beacons to brown out.{}",
            deepen_sentence()
        )
    );
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
    assert!(!advice.safe_playbook_jsonc.is_empty());
}

/// Out of reach: past 60 s nothing is raised, the core still gets its depth,
/// and the why says why without saying power is not short.
#[test]
fn a_beacon_at_risk_out_of_reach_is_not_raised_and_the_why_says_so() {
    let mut far = short_with_four_lit(1);
    for ms in far.to_beacon_ms.values_mut() {
        *ms = 61_000;
    }
    let (script, advice) = advise(far);
    assert_eq!(
        safe_labels(&script.transcript),
        ["to_safety", "deepen_core"]
    );
    let why = why_of(&advice, "safe_playbook");
    assert!(!why.contains("Power is not short"), "{why}");
    assert!(
        why.contains("no lit beacon next in the brownout order is within 60 s"),
        "{why}"
    );
}

/// Power is not short -- nothing dark, the draw within the supply -- so
/// nothing is raised and nothing is even estimated; the route is the walk to
/// safety and the core's depth.
#[test]
fn power_not_short_raises_nothing_and_estimates_nothing() {
    let mut spare = short_with_four_lit(1);
    spare.draw = 10;
    let (script, advice) = advise(spare);
    assert_eq!(
        safe_labels(&script.transcript),
        ["to_safety", "deepen_core"]
    );
    assert!(
        !beacon_estimated(&script.transcript),
        "and not even estimated"
    );
    assert_eq!(
        why_of(&advice, "safe_playbook"),
        format!(
            "{SEED_LINE} Power is not short, so nothing is raised: move to the safest beacon and \
             stay with it.{}",
            deepen_sentence()
        )
    );
}

/// A dark beacon is draw the supply did not carry: power is short though the
/// forecast's draw is within its supply (a settle sheds until it is), and
/// the lit beacons next in the shed order are raised. The dark one is not:
/// raising it would only swap it in for a lit one.
#[test]
fn a_dark_beacon_makes_power_short_and_the_lit_ones_next_to_shed_are_raised() {
    let mut settled = short_with_four_lit(1);
    settled.draw = 10;
    settled.beacons.push(Beacon {
        id: String::from("b_08"),
        at: [10, 10, GROUND],
        owner: 1,
        core: false,
        powered: false,
        priority: "normal",
    });
    let (script, advice) = advise(settled);
    assert_eq!(
        safe_labels(&script.transcript),
        ["to_safety", "raise_b_04", "raise_b_05", "deepen_core"]
    );
    let why = why_of(&advice, "safe_playbook");
    assert!(
        why.contains("Power is short because b_08 is browned out:"),
        "{why}"
    );
    assert!(!why.contains("kW"), "no draw is given as the reason: {why}");
}

/// Every lit non-core beacon HIGH already: nothing to raise, nothing
/// estimated, and the why names them.
#[test]
fn beacons_already_high_are_not_raised_again() {
    let mut all_high = short_with_four_lit(1);
    for beacon in &mut all_high.beacons {
        if !beacon.core {
            beacon.priority = "high";
        }
    }
    let (script, advice) = advise(all_high);
    assert_eq!(
        safe_labels(&script.transcript),
        ["to_safety", "deepen_core"]
    );
    assert!(!beacon_estimated(&script.transcript), "nothing estimated");
    let why = why_of(&advice, "safe_playbook");
    assert!(
        why.contains("b_04, b_05, b_06 and b_07 are HIGH already"),
        "{why}"
    );
}

/// Only the core, and it dark: the state the hosted power-short rows show
/// from round 2 on when a seat placed nothing. The core is never raised, so
/// nothing is raised and nothing is estimated, and the why names the core as
/// the shortage.
#[test]
fn only_the_core_dark_raises_nothing_and_says_the_core_is_never_raised() {
    let mut blackout = Script::new(1);
    blackout.supply = 0;
    blackout.draw = 0;
    if let Some(core) = blackout.beacons.first_mut() {
        core.powered = false;
    }
    let (script, advice) = advise(blackout);
    assert_eq!(
        safe_labels(&script.transcript),
        ["to_safety", "deepen_core"]
    );
    assert!(!beacon_estimated(&script.transcript), "nothing estimated");
    assert_eq!(
        why_of(&advice, "safe_playbook"),
        format!(
            "{SEED_LINE} Power is short because the core b_00 is browned out, and no beacon of \
             yours but the core is lit, and the safe playbook never raises the core, so nothing \
             is raised: move to the safest beacon and stay with it.{}",
            deepen_sentence()
        )
    );
}

/// The core's dig depth (decisions-log item 133 (3) (a)): the route ends with
/// one interface step at the core, one `set_mandate_settings` row giving its
/// Mine settings Easy's depth, skipped if it fails -- it switches no mandate,
/// recycles nothing and places nothing. A seat whose core is not shown gets
/// the template as written, at no call.
#[test]
fn the_safe_playbook_ends_by_setting_the_cores_dig_depth() {
    let (script, _) = advise(Script::new(1));
    let routes = safe_routes(&script.transcript);
    let Some(Json::Array(steps)) = routes.first() else {
        panic!("a route: {:#?}", script.transcript);
    };
    let deepen = steps.last().expect("a last step");
    assert_eq!(
        compact(deepen),
        compact(
            &read(&format!(
                r#"{{"label":"deepen_core","interface":{{"beacon":{{"beacon_id":"b_00"}},"rows":[{{"set_mandate_settings":{{"mine":{{"dig_max_depth":{CORE_DIG_MAX_DEPTH}}}}}}}]}},"on_fail":{{"action":"SKIP"}}}}"#
            ))
            .unwrap()
        )
    );

    let mut coreless = Script::new(1);
    coreless.beacons.clear();
    let (script, advice) = advise(coreless);
    assert!(
        safe_routes(&script.transcript).is_empty(),
        "the template as written: {:#?}",
        script.transcript
    );
    let safe = advice
        .suggestions
        .iter()
        .find(|suggestion| suggestion.template_id == "safe_playbook")
        .expect("a suggestion for the safe template");
    assert!(
        safe.parameters.is_empty(),
        "the template's own route stands"
    );
}

// ---------------------------------------------------------------------------
// Reading the round
// ---------------------------------------------------------------------------

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

/// A feature row the operator cannot read -- here one with no grade -- is a
/// round it cannot read: it asks for no view, plans nothing, and still says
/// ready, rather than guessing what the feature is.
#[test]
fn a_feature_that_does_not_read_is_a_round_not_read_and_ready_is_still_said() {
    let mut odd = Script::busy(0);
    odd.malformed_feature = true;
    let (script, played) = play(odd);
    assert_eq!(played.submitted, Submitted::Nothing);
    assert!(played.ready);
    assert_eq!(
        methods(&script.transcript),
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
    short.paging = Paging::ViewNeverComplete;
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
        "resolve_refs",
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
/// every paged read paged to its bound, a tap to probe, more candidates than
/// Easy evaluates, a plan that never qualifies but always offers a fix so
/// every repair is spent, power short with more beacons at risk than it
/// estimates, and the safe playbook submitted -- stays inside it, as does the
/// advisor's.
#[test]
fn easy_never_exceeds_its_derived_call_budget() {
    let mut worst = short_with_four_lit(1);
    worst.paging = Paging::ToTheBound;
    worst.verify = Verify::NeverButFixable;
    worst.treasury = 10_000;
    for n in 0..40 {
        let kind = if n % 2 == 0 { "seam" } else { "vent" };
        let mut feature = Feature::new(kind, "standard", [2 + n, 2]);
        feature.travel_ms = Some(1_000 + i64::from(n));
        worst.features.push(feature);
    }
    let mut covered = Feature::new("vent", "rich", [40, 40]);
    covered.covered = true;
    worst.features.push(covered);
    worst
        .on_reads
        .insert((String::from("b_00"), String::from("vent_40_40")));
    for n in 8..12 {
        worst.beacons.push(Beacon {
            id: format!("b_{n:02}"),
            at: [30 + n, 20, GROUND],
            owner: 1,
            core: false,
            powered: true,
            priority: "low",
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
    assert!(
        script
            .transcript
            .iter()
            .any(|line| line.starts_with("resolve_refs")),
        "the tap was probed"
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

// ---------------------------------------------------------------------------
// Names, not places
// ---------------------------------------------------------------------------

/// A seat's round, told plainly: Easy fills Hold & Build with the vent
/// it does not cover yet, **by name** in place of the template's description
/// (S1's targeting: Easy emits names), estimated as a `covering` waypoint
/// whose last leg is the site; it writes no walk (a `place_beacon` walks to
/// its site itself), fills the hold, and ends the route at the core with its
/// dig depth. Its note records the seed it did not use. The covered seam
/// offers no goal and is never estimated.
#[test]
fn easy_names_the_vent_it_covers_and_writes_no_site_of_its_own() {
    // The busy seat without its uncovered seam, whose Mine beacon would
    // otherwise be worth more per second than the vent.
    let mut world = Script::busy(0);
    world.features.retain(|feature| feature.id != "seam_20_60");
    let (script, played) = play(world);
    assert_eq!(played.submitted, Submitted::Own, "{:#?}", script.transcript);
    assert!(
        script
            .transcript
            .iter()
            .any(|line| line.starts_with("estimate_route")
                && line.contains(r#""covering": { "feature_id": "vent_56_32" }"#)),
        "{:#?}",
        script.transcript
    );
    assert!(
        !script
            .transcript
            .iter()
            .any(|line| line.starts_with("estimate_route") && line.contains("seam_36_34")),
        "the covered seam is the core's: no goal, no estimate"
    );
    let ops = composing_ops(&script.transcript);
    assert_eq!(
        op_value(
            &ops,
            "replace",
            "/declarative/route/0/place_beacon/at/covering"
        )
        .map(|value| compact(&value)),
        Some(compact(&read(r#"{"feature_id":"vent_56_32"}"#).unwrap())),
        "{ops:#?}"
    );
    assert!(
        op_value(&ops, "replace", "/declarative/route/1/hold/ms").is_some(),
        "the hold fills the share: {ops:#?}"
    );
    assert!(
        !ops.iter().any(|op| op
            .get("path")
            .and_then(text)
            .is_some_and(|path| path.contains("/move/") || path.ends_with("/voxel"))),
        "no walk and no voxel: {ops:#?}"
    );
    let last = ops.last().expect("an operation");
    assert_eq!(
        last.get("path").and_then(text),
        Some("/declarative/route/-")
    );
    assert_eq!(
        last.get("value")
            .and_then(|step| step.get("label"))
            .and_then(text),
        Some("deepen_core")
    );
    let note = op_value(&ops, "replace", "/meta/note")
        .and_then(|value| text(&value).map(str::to_owned))
        .expect("the note");
    assert!(note.contains("seed 0x0000000000005eed"), "{note}");
    assert!(
        played
            .why
            .contains("A Build beacon covering the lean heat vent vent_56_32, at (46, 32, 11)"),
        "{}",
        played.why
    );
}

/// The advisor suggests the same name for Hold & Build's covering page, and
/// the hold; and Expand & Mine's covering page names the seam it does not
/// cover yet.
#[test]
fn the_advisor_suggests_names_for_the_covering_pages() {
    let (_, advice) = advise(Script::busy(1));
    let page = |template: &str| {
        advice
            .suggestions
            .iter()
            .find(|suggestion| suggestion.template_id == template)
            .unwrap_or_else(|| panic!("a {template} suggestion"))
            .parameters
            .iter()
            .map(|value| (value.pointer.clone(), value.value.clone()))
            .collect::<Vec<(String, String)>>()
    };
    let hold = page("hold_and_build");
    assert_eq!(
        hold.first(),
        Some(&(
            String::from("/declarative/route/0/place_beacon/at/covering"),
            String::from(r#"{"feature_id":"vent_56_32"}"#)
        ))
    );
    assert_eq!(
        hold.get(1).map(|(pointer, _)| pointer.as_str()),
        Some("/declarative/route/1/hold/ms")
    );
    assert_eq!(
        page("expand_and_mine"),
        [(
            String::from("/declarative/route/0/place_beacon/at/covering"),
            String::from(r#"{"feature_id":"seam_20_60"}"#)
        )]
    );
}

/// Spec section 14: "compose visit goals by greedy insertion by utility per
/// second until the route fills its target share of the segment". Two seams
/// the seat does not cover and money for two beacons: the second is inserted
/// as one more copy of the place step, named and numbered, before the walk
/// home. Money for one: it is not.
#[test]
fn easy_inserts_goals_greedily_while_the_share_and_the_treasury_allow() {
    let mut rich = Script::new(0);
    for (anchor, site) in [([40, 30], [36, 30, GROUND]), ([24, 30], [28, 30, GROUND])] {
        let mut seam = Feature::new("seam", "rich", anchor);
        seam.site = Some(site);
        rich.features.push(seam);
    }
    rich.treasury = 500;
    let (script, played) = play(rich.clone());
    assert_eq!(played.submitted, Submitted::Own);
    let ops = composing_ops(&script.transcript);
    let inserted = op_value(&ops, "add", "/declarative/route/1").expect("one more step");
    assert_eq!(
        inserted.get("label").and_then(text),
        Some("place_mine_2"),
        "{ops:#?}"
    );
    assert!(
        compact(&inserted).contains(r#""covering": { "feature_id": "seam_"#),
        "{ops:#?}"
    );

    let mut poor = rich;
    poor.treasury = 100;
    let (script, _) = play(poor);
    assert!(
        op_value(
            &composing_ops(&script.transcript),
            "add",
            "/declarative/route/1"
        )
        .is_none(),
        "one beacon's worth of money, one seam"
    );
}

/// A feature no site covers -- `estimate_route` refuses its `covering`
/// waypoint -- offers no goal; a feature the seat covers already offers no
/// placement. With every vent within reach covered, Hold & Build's page says
/// so and suggests nothing.
#[test]
fn a_covered_or_uncoverable_vent_offers_no_placement_and_the_page_says_why() {
    let mut uncoverable = Script::new(1);
    let mut vent = Feature::new("vent", "lean", [56, 32]);
    vent.site = None;
    uncoverable.features.push(vent);
    let (script, played) = play(uncoverable);
    assert_eq!(played.submitted, Submitted::Safe, "{played:#?}");
    assert!(
        script
            .transcript
            .iter()
            .any(|line| line.starts_with("estimate_route") && line.contains("vent_56_32")),
        "it was asked"
    );

    let mut covered = Script::new(1);
    let mut vent = Feature::new("vent", "lean", [40, 32]);
    vent.covered = true;
    covered.features.push(vent);
    let (_, advice) = advise(covered);
    let page = advice
        .suggestions
        .iter()
        .find(|suggestion| suggestion.template_id == "hold_and_build")
        .expect("a Hold & Build suggestion");
    assert!(page.parameters.is_empty(), "{page:#?}");
    assert!(
        page.why
            .contains("Every heat vent within reach is inside a sphere of yours already"),
        "{}",
        page.why
    );
}

/// The register's S1-15 (decision 14): a vent the seat covers, inside the
/// sphere of an existing own beacon on the Build mandate, is a tap -- one
/// interface step adding a Generator `on` the vent by name to that beacon's
/// list. The gateway is asked twice before anything is estimated: whether
/// the `on` reads from that beacon (`resolve_refs`) and whether the beacon
/// is on the Build mandate (a QUICK verify: no `E0503`).
#[test]
fn a_covered_vent_inside_a_build_beacons_sphere_is_tapped_by_name() {
    let mut tap = Script::new(0);
    let mut vent = Feature::new("vent", "rich", [40, 40]);
    vent.covered = true;
    tap.features.push(vent);
    tap.on_reads
        .insert((String::from("b_00"), String::from("vent_40_40")));
    let (script, played) = play(tap.clone());
    assert_eq!(played.submitted, Submitted::Own, "{:#?}", script.transcript);
    let all = methods(&script.transcript);
    let resolve = all.iter().position(|method| *method == "resolve_refs");
    let estimate = all.iter().position(|method| *method == "estimate_route");
    assert!(
        resolve.is_some() && resolve < estimate,
        "probed before it was estimated: {all:?}"
    );
    let ops = composing_ops(&script.transcript);
    let step = op_value(&ops, "replace", "/declarative/route/0").expect("the place step replaced");
    assert_eq!(step.get("label").and_then(text), Some("add_generator"));
    assert_eq!(
        compact(step.get("interface").unwrap()),
        compact(
            &read(
                r#"{"beacon":{"beacon_id":"b_00"},"rows":[{"add_build_target":{"target":{"blueprint_id":"generator","anchor":{"on":{"feature_id":"vent_40_40"}}}}}]}"#
            )
            .unwrap()
        )
    );
    assert!(
        played
            .why
            .contains("added on site to the Build list of b_00"),
        "{}",
        played.why
    );

    // Not on the Build mandate: the verifier's E0503, so no tap.
    let mut mine = tap.clone();
    mine.build_beacons.clear();
    let (script, played) = play(mine);
    assert_eq!(
        played.submitted,
        Submitted::Safe,
        "{:#?}",
        script.transcript
    );
    assert!(!beacon_estimated(&script.transcript));

    // The `on` does not read from that beacon: no tap.
    let mut unread = tap;
    unread.on_reads.clear();
    let (script, played) = play(unread);
    assert_eq!(played.submitted, Submitted::Safe);
    assert!(!beacon_estimated(&script.transcript));
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

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
    CARRIED_DRAFT_ID, CORE_DIG_MAX_DEPTH, EASY_ADVISOR_CALL_BUDGET, EASY_CALL_BUDGET,
    EASY_CANDIDATES, LAST_SEAL_CALLS, PAGES_MAX, RAID_FORCE_SIZE, RAID_TARGETS_MAX,
    SAFE_MAX_RAISED, STONE_CALLS, STONE_TARGETS_MAX, SURVEY_PROBE_CALLS,
};
use pharmakos_operator::{Easy, FailedStones, PushUnread, StonesUnread, Submitted};
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
    /// The stone a `toward` waypoint naming it answers; `None` when the
    /// gateway refuses it (`NOT_FOUND`, no stepping stone).
    stone: Option<[i32; 3]>,
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
            stone: None,
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
    /// Every playbook qualifies FULL, and a QUICK verify answers no `report`.
    QuickUnreported,
    /// No playbook qualifies FULL, and no report offers a fix, so every repair
    /// takes out a goal.
    NeverUnfixed,
}

/// How the scripted map summary writes a feature's travel.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Travel {
    /// As the gateway writes it in a Lull.
    Written,
    /// Left out, `reachable` and `travel_ms` both, as outside a Lull.
    Withheld,
    /// `reachable: true` with no `travel_ms`.
    NoFigure,
    /// `reachable: false` beside a `travel_ms`.
    FigureUnreached,
}

/// Whether the scripted feed shows the last Push.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Push {
    /// Its digest counts the Push's `segment_ended`, as every Lull's after
    /// the first does.
    Shown,
    /// It counts nothing: a Lull resumed from a save made before the save
    /// carried the last Push (format version 3), whose feed shows none, or a
    /// first round, which had no Push.
    NotShown,
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
    /// How the first feature row carries its travel: as the gateway writes
    /// it in a Lull, withheld (no `reachable`, no `travel_ms`, as outside a
    /// Lull), or `reachable: true` with no `travel_ms`.
    travel: Travel,
    /// The seats whose commander the last Push's feed says died, one
    /// `commander_died` line each.
    feed_deaths: Vec<u8>,
    /// The seat's own step lines in the last Push's feed, `(kind, text)`, in
    /// order, after the deaths; the digest counts their `step_failed`s.
    feed_steps: Vec<(&'static str, String)>,
    /// Whether the feed shows the last Push: its digest counts the Push's
    /// `segment_ended`.
    push: Push,
    /// Failures of the seat's own steps the digest counts beyond
    /// `feed_steps`' lines, which no page serves: lines past what a round
    /// reads.
    unserved_failures: i64,
    /// The seat's own drafts, by id: the round each is stamped with, its
    /// text.
    drafts: BTreeMap<String, (u32, String)>,
    /// Refuse `get_draft` with this code.
    refuse_draft: Option<&'static str>,
    /// Where the seat's nearest Defend beacon stands, if one does: what an
    /// `estimate_route` to `nearest {filter: {mandate: DEFEND}}` reaches.
    defend_post: Option<[i32; 3]>,
    /// The sighting age `list_beacons` answers for another seat's beacon, by
    /// id; 0, in live vision, for one not named here.
    sighting_ages: BTreeMap<String, i64>,
    /// Another seat's beacons answered with no sighting age, by id.
    ageless: BTreeSet<String>,
    /// Where the seat's nearest Attack beacon stands, if one does.
    attack_post: Option<[i32; 3]>,
    /// Where the seat's nearest Survey beacon stands, if one does.
    survey_post: Option<[i32; 3]>,
    /// Beacons a route ending on them does not reach, by id.
    unreachable: BTreeSet<String>,
    /// A `covering` or `toward` waypoint naming a feature refused with this
    /// code instead of answered, by feature name.
    refuse_feature: BTreeMap<String, &'static str>,
    /// An own beacon's sphere radius by id, where it is not the rules
    /// table's base (24 voxels): a finished Spire grows it.
    radii: BTreeMap<String, i64>,
    /// The briefing's `licences_held`, as the wire spells each; left out of
    /// the answer while empty, as the gateway leaves it.
    licences: Vec<&'static str>,
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
            travel: Travel::Written,
            feed_deaths: Vec::new(),
            feed_steps: Vec::new(),
            push: Push::Shown,
            unserved_failures: 0,
            drafts: BTreeMap::new(),
            refuse_draft: None,
            defend_post: None,
            sighting_ages: BTreeMap::new(),
            ageless: BTreeSet::new(),
            attack_post: None,
            survey_post: None,
            unreachable: BTreeSet::new(),
            refuse_feature: BTreeMap::new(),
            radii: BTreeMap::new(),
            licences: Vec::new(),
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
        if let Some(code) = ["covering", "toward"]
            .iter()
            .filter_map(|arm| location.get(arm))
            .filter_map(|arm| {
                arm.get("feature_id")
                    .or_else(|| arm.get("feature")?.get("feature_id"))
            })
            .filter_map(text)
            .find_map(|name| self.refuse_feature.get(name))
        {
            return Err(code);
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
        if let Some(name) = location
            .get("toward")
            .and_then(|toward| toward.get("feature"))
            .and_then(|feature| feature.get("feature_id"))
            .and_then(text)
        {
            return self
                .features
                .iter()
                .find(|feature| feature.id == name)
                .and_then(|feature| feature.stone)
                .ok_or("NOT_FOUND");
        }
        if let Some(nearest) = location
            .get("beacon_anchor")
            .and_then(|anchor| anchor.get("nearest"))
        {
            let mandate = nearest
                .get("filter")
                .and_then(|filter| filter.get("mandate"))
                .and_then(text);
            let post = match mandate {
                Some("ATTACK") => self.attack_post,
                Some("SURVEY") => self.survey_post,
                _ => self.defend_post,
            };
            return post.ok_or("NOT_FOUND");
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
            "get_briefing" => {
                let mut briefing = vec![
                    ("notes", Json::String(self.notes.clone())),
                    ("segment_length_ms", num(self.segment_ms)),
                    ("prose", Json::String(String::from("scripted"))),
                ];
                if !self.licences.is_empty() {
                    briefing.push((
                        "licences_held",
                        Json::Array(
                            self.licences
                                .iter()
                                .map(|held| Json::String((*held).to_owned()))
                                .collect(),
                        ),
                    ));
                }
                self.ok(obj(briefing))
            }
            "list_beacons" => self.list_beacons(&cursor),
            "get_economy_forecast" => self.ok(obj(vec![
                ("treasury_now", num(self.treasury)),
                ("supply_kw_now", num(self.supply)),
                ("draw_kw_now", num(self.draw)),
                ("headroom_kw_now", num(self.supply - self.draw)),
            ])),
            "get_map_summary" => self.map_summary(),
            "get_view" => self.get_view(&cursor),
            "get_segment_feed" => self.segment_feed(&cursor),
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
            "save_draft" => {
                let id = params
                    .get("draft_id")
                    .and_then(text)
                    .unwrap_or("")
                    .to_owned();
                let body = params
                    .get("playbook_jsonc")
                    .and_then(text)
                    .unwrap_or("")
                    .to_owned();
                self.drafts.insert(id.clone(), (self.round, body));
                self.ok(obj(vec![("draft_id", Json::String(id))]))
            }
            "get_draft" => {
                if let Some(code) = self.refuse_draft {
                    return Script::refuse(code);
                }
                let id = params.get("draft_id").and_then(text).unwrap_or("");
                match self.drafts.get(id) {
                    Some((round, body)) => self.ok(obj(vec![
                        ("playbook_jsonc", Json::String(body.clone())),
                        ("label", Json::String(String::from("carried from round 1"))),
                        ("round", num(i64::from(*round))),
                    ])),
                    None => Script::refuse("NOT_FOUND"),
                }
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

    /// The last Push's feed: one `commander_died` line per seat in
    /// `feed_deaths`, and the one digest counting them.
    fn segment_feed(&self, cursor: &str) -> Json {
        // Paged to the bound, the lines are never reached: every page but
        // the digest's count is empty.
        let lines: &[u8] = if self.paging == Paging::ToTheBound {
            &[]
        } else {
            &self.feed_deaths
        };
        let steps: &[(&'static str, String)] = if self.paging == Paging::ToTheBound {
            &[]
        } else {
            &self.feed_steps
        };
        let mut events: Vec<Json> = lines
            .iter()
            .map(|seat| {
                obj(vec![
                    ("at_ms", num(1_000)),
                    ("kind", Json::String(String::from("commander_died"))),
                    (
                        "text",
                        Json::String(format!(
                            "The commander of seat {seat} died. That is death 1 in this Push."
                        )),
                    ),
                ])
            })
            .collect();
        events.extend(steps.iter().map(|(kind, line)| {
            obj(vec![
                ("at_ms", num(2_000)),
                ("kind", Json::String((*kind).to_owned())),
                ("text", Json::String(line.clone())),
            ])
        }));
        let mut counts: Vec<Json> = Vec::new();
        if self.push == Push::Shown {
            counts.push(obj(vec![
                ("kind", Json::String(String::from("segment_ended"))),
                ("count", num(1)),
            ]));
        }
        let count = i64::try_from(self.feed_deaths.len()).unwrap();
        if count > 0 {
            counts.push(obj(vec![
                ("kind", Json::String(String::from("commander_died"))),
                ("count", num(count)),
            ]));
        }
        let failed = i64::try_from(
            self.feed_steps
                .iter()
                .filter(|(kind, _)| *kind == "step_failed")
                .count(),
        )
        .unwrap()
            + self.unserved_failures;
        if failed > 0 {
            counts.push(obj(vec![
                ("kind", Json::String(String::from("step_failed"))),
                ("count", num(failed)),
            ]));
        }
        self.ok(obj(vec![
            ("events", Json::Array(events)),
            (
                "digests",
                Json::Array(vec![obj(vec![
                    ("from_ms", num(0)),
                    ("to_ms", num(60_000)),
                    ("text", Json::String(String::from("scripted"))),
                    ("counts", Json::Array(counts)),
                ])]),
            ),
            ("next_cursor", Json::String(self.next_page(cursor))),
        ]))
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
                        let radius = self.radii.get(&beacon.id).copied().unwrap_or(24);
                        row.push(("sphere_radius_voxels", num(radius)));
                    } else if !self.ageless.contains(&beacon.id) {
                        let age = self.sighting_ages.get(&beacon.id).copied().unwrap_or(0);
                        row.push(("sighting_age_ms", num(age)));
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
                match (index, self.travel) {
                    (0, Travel::Withheld) => {}
                    (0, Travel::NoFigure) => row.push(("reachable", Json::Bool(true))),
                    (0, Travel::FigureUnreached) => {
                        row.push(("travel_ms", num(1_000)));
                        row.push(("reachable", Json::Bool(false)));
                    }
                    _ => {
                        // `travel_ms` only beside `reachable: true`, as the
                        // gateway writes it.
                        if let Some(travel) = feature.travel_ms {
                            row.push(("travel_ms", num(travel)));
                        }
                        row.push(("reachable", Json::Bool(feature.travel_ms.is_some())));
                    }
                }
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
            [
                "defend_the_core",
                "expand_and_mine",
                "hold_and_build",
                "raid_an_enemy_beacon",
                "safe_playbook",
            ]
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
            Some(Json::Array(items)) if items.len() >= 2 => items.clone(),
            _ => return Script::refuse("INVALID_ARGUMENT"),
        };
        let mut places: Vec<[i32; 3]> = Vec::new();
        for waypoint in &waypoints {
            match self.place(waypoint) {
                Ok(at) => places.push(at),
                Err(code) => return Script::refuse(code),
            }
        }
        let mut legs: Vec<Json> = Vec::new();
        let mut total = 0_i64;
        for (index, pair) in places.windows(2).enumerate() {
            let &[from, to] = pair else {
                return Script::refuse("INTERNAL");
            };
            let id = waypoints
                .get(index + 1)
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
            total += ms;
            // `Leg.to` in the DECLARED shape, a `gp.v1.Location`, unless
            // asked for the bare voxel `main` wrote before T17.
            let leg_to = if self.bare_legs {
                voxel_json(to)
            } else {
                obj(vec![("voxel", voxel_json(to))])
            };
            legs.push(obj(vec![
                ("to", leg_to),
                ("ms", num(ms)),
                ("fogged", Json::Bool(false)),
            ]));
        }
        let unreached = waypoints
            .last()
            .and_then(|w| w.get("beacon_anchor"))
            .and_then(|a| a.get("beacon_id"))
            .and_then(text)
            .is_some_and(|id| self.unreachable.contains(id));
        if unreached {
            return self.ok(obj(vec![
                ("reachable", Json::Bool(false)),
                ("ms", num(0)),
                ("legs", Json::Array(Vec::new())),
            ]));
        }
        self.ok(obj(vec![
            ("reachable", Json::Bool(true)),
            ("ms", num(total)),
            ("legs", Json::Array(legs)),
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
        if quick && self.verify == Verify::QuickUnreported {
            return self.ok(obj(vec![]));
        }
        let qualifies = diagnostics.is_empty()
            && (quick
                || (self.verify != Verify::NeverButFixable && self.verify != Verify::NeverUnfixed));
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
/// estimates, and a tap's, are the only ones whose last waypoint is a
/// `beacon_anchor` (a placement's goes by way of the core's anchor and ends
/// on a `covering`).
/// The guard probe's estimate to the nearest own Defend beacon is not one.
fn beacon_estimated(transcript: &[String]) -> bool {
    transcript.iter().any(|line| {
        line.strip_prefix("estimate_route ")
            .and_then(|params| read(params).ok())
            .and_then(|params| match params.get("waypoints") {
                Some(Json::Array(items)) => items.last().cloned(),
                _ => None,
            })
            .is_some_and(|last| {
                last.get("beacon_anchor")
                    .is_some_and(|anchor| anchor.get("nearest").is_none())
            })
    })
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
    assert_eq!(advice.suggestions.len(), 5);
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
// ---------------------------------------------------------------------------
// A feature's travel, withheld (lane `mapw`)
// ---------------------------------------------------------------------------

/// Outside a Lull the gateway leaves a feature's `reachable` and `travel_ms`
/// out (lane `mapw`), and Easy reads the summary only in a Lull: a row with
/// no `reachable`, or `reachable: true` with no `travel_ms`, is an answer
/// the operator cannot plan from, refused as `MALFORMED` rather than read as
/// unreachable or as a walk of 0 ms; and `set_ready` is still said.
#[test]
fn a_feature_whose_travel_is_withheld_is_a_round_not_read() {
    for travel in [Travel::Withheld, Travel::NoFigure, Travel::FigureUnreached] {
        let mut world = Script::busy(0);
        world.travel = travel;
        let (script, played) = play(world);
        assert_eq!(
            played.refused.map(|refused| (refused.method, refused.code)),
            Some((String::from("get_map_summary"), String::from("MALFORMED")))
        );
        assert_eq!(
            script
                .transcript
                .last()
                .map(|line| line.split(' ').next().unwrap_or("")),
            Some("set_ready")
        );
    }
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
/// own seal: a built-in seat's only writes are `submit_plan`, the draft it
/// keeps of its own seal (`save_draft`, fix pass 3, item 6) and `set_ready`;
/// an advisor's are none; and neither ever touches the notebook.
#[test]
fn the_operator_reads_no_notebook_and_writes_nothing_but_its_own_seal() {
    let reads = [
        "get_status",
        "get_briefing",
        "list_beacons",
        "get_economy_forecast",
        "get_map_summary",
        "get_view",
        "get_segment_feed",
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
/// The worst round the script can build, for the budget's test.
fn worst_round() -> Script {
    let mut worst = short_with_four_lit(1);
    worst.paging = Paging::ToTheBound;
    // Another seat's commander died where this one could see, and the line
    // is never reached: the feed is paged to its bound.
    worst.feed_deaths = vec![0];
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
    // The starting seam beside the core, so the raid post has its seam and the
    // fresh enemy beacons below are weighed as raids.
    let mut start = Feature::new("seam", "standard", [36, 34]);
    start.covered = true;
    worst.features.push(start);
    for n in 1..=4 {
        worst.beacons.push(Beacon {
            id: format!("e_0{n}"),
            at: [32, 60 + 10 * n, GROUND],
            owner: 0,
            core: false,
            powered: false,
            priority: "",
        });
    }
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
    worst
}

#[test]
fn easy_never_exceeds_its_derived_call_budget() {
    let worst = worst_round();
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
            .any(|line| line.starts_with("estimate_route")
                && line.contains("covering")
                && line.contains("\"e_01\"")),
        "a raid was weighed among them"
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
    // so only the safe one was submitted) and the stepping stones, since every
    // placement here has a site (`the_worst_stone_round_spends_every_term`
    // spends those), the Survey probe, since a fresh sighting is held, and
    // the last seal's one: the feed paged to its bound reaches no failed
    // step to read the carried seal back for.
    assert_eq!(
        played.calls,
        EASY_CALL_BUDGET - 1 - STONE_CALLS - SURVEY_PROBE_CALLS - LAST_SEAL_CALLS
    );

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
/// offers no Mine goal; it is estimated once, as the guard post's site (a
/// Survey post stands already), and
/// the post is not placed, since nothing threatens the core and the commander
/// did not die.
#[test]
fn easy_names_the_vent_it_covers_and_writes_no_site_of_its_own() {
    // The busy seat without its uncovered seam, whose Mine beacon would
    // otherwise be worth more per second than the vent.
    let mut world = Script::busy(0);
    world.features.retain(|feature| feature.id != "seam_20_60");
    // A Survey post stands already, so none takes the share.
    world.survey_post = Some([34, 34, GROUND]);
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
    assert_eq!(
        script
            .transcript
            .iter()
            .filter(|line| line.starts_with("estimate_route") && line.contains("seam_36_34"))
            .count(),
        1,
        "the covered seam is the core's: no Mine goal, and one estimate, as the guard post's site"
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
    // The core's step opens the route: inserted at route 0 by the patch's
    // last operation, so no pointer before it moves.
    let last = ops.last().expect("an operation");
    assert_eq!(last.get("op").and_then(text), Some("add"));
    assert_eq!(
        last.get("path").and_then(text),
        Some("/declarative/route/0")
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

/// The composed route visits the core first, so every goal is estimated by
/// way of the core's anchor, and the walk from the commander to the core is
/// paid, with the step's interface time, out of Hold & Build's hold: a
/// commander 5 s further from its core leaves a hold 5 s shorter, and the
/// goal's own cost, which runs from the core, is the same.
#[test]
fn the_walk_to_the_core_is_estimated_and_paid_out_of_the_hold() {
    let near = || {
        let mut world = Script::busy(0);
        world.features.retain(|feature| feature.id != "seam_20_60");
        // A Survey post stands already, so none takes the share.
        world.survey_post = Some([34, 34, GROUND]);
        world
    };
    let hold_of = |world: Script| {
        let (script, played) = play(world);
        assert_eq!(played.submitted, Submitted::Own, "{:#?}", script.transcript);
        let estimate = script
            .transcript
            .iter()
            .find(|line| line.starts_with("estimate_route") && line.contains("vent_56_32"))
            .cloned()
            .expect("the vent estimated");
        let core_at = estimate.find(r#""beacon_id": "b_00""#);
        let vent_at = estimate.find("vent_56_32");
        assert!(
            core_at.is_some() && core_at < vent_at,
            "by way of the core: {estimate}"
        );
        let ops = composing_ops(&script.transcript);
        let hold = op_value(&ops, "replace", "/declarative/route/1/hold/ms")
            .and_then(|value| match value {
                Json::Number(n) => n.parse::<i64>().ok(),
                _ => None,
            })
            .expect("the hold, a number");
        (hold, played.why)
    };
    let (near_hold, near_why) = hold_of(near());
    let mut far = near();
    // Ten voxels further from the core (at 500 ms a voxel in the script).
    far.entities
        .iter_mut()
        .filter(|entity| entity.subtype == "commander")
        .for_each(|commander| commander.at = [32, 20, GROUND]);
    let (far_hold, far_why) = hold_of(far);
    assert_eq!(near_hold - far_hold, 5_000, "{near_hold} and {far_hold}");
    let cost = |why: &str| {
        why.split(" points for ")
            .nth(1)
            .map(|rest| rest.split(" of route").next().unwrap_or("").to_owned())
    };
    assert_eq!(cost(&near_why), cost(&far_why), "{near_why} / {far_why}");
}

/// A Hold & Build route whose goal fills the share leaves nothing for the
/// hold once the core's step is paid for: the hold is taken out, not left at
/// the template's own 30 s past the share.
#[test]
fn a_hold_with_nothing_left_for_it_is_taken_out() {
    let world = || {
        let mut world = Script::busy(0);
        world.features.retain(|feature| feature.id != "seam_20_60");
        // A Survey post stands already, so none takes the share.
        world.survey_post = Some([34, 34, GROUND]);
        world
    };
    let (script, _) = play(world());
    let ops = composing_ops(&script.transcript);
    let hold = op_value(&ops, "replace", "/declarative/route/1/hold/ms")
        .and_then(|value| match value {
            Json::Number(n) => n.parse::<i64>().ok(),
            _ => None,
        })
        .expect("the hold, a number");
    // The busy seat's segment fills 126 s; take the share down to 1 s less
    // than the goal, the core's walk and its interface time, so the goal
    // still fits and the hold has nothing.
    let fill = 126_000 - hold - 1_000;
    let mut tight = world();
    tight.segment_ms = (fill * 100).div_euclid(70) + 1;
    tight.remaining_ms = tight.segment_ms;
    let (script, played) = play(tight);
    assert_eq!(played.submitted, Submitted::Own, "{:#?}", script.transcript);
    let ops = composing_ops(&script.transcript);
    assert!(
        op_value(&ops, "replace", "/declarative/route/1/hold/ms").is_none(),
        "{ops:#?}"
    );
    assert!(
        ops.iter()
            .any(|op| op.get("op").and_then(text) == Some("remove")
                && op.get("path").and_then(text) == Some("/declarative/route/1")),
        "the hold step taken out: {ops:#?}"
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
    let mut unread = tap.clone();
    unread.on_reads.clear();
    let (script, played) = play(unread);
    assert_eq!(played.submitted, Submitted::Safe);
    assert!(!beacon_estimated(&script.transcript));

    // A QUICK verify that answers no report cannot say no E0503 was raised:
    // read as a refusal, so no tap (fail closed, as a refused call is).
    let mut unreported = tap;
    unreported.verify = Verify::QuickUnreported;
    let (script, played) = play(unreported);
    assert_eq!(
        played.submitted,
        Submitted::Safe,
        "{:#?}",
        script.transcript
    );
    assert!(!beacon_estimated(&script.transcript));
}

/// The shed order's last tie is the beacon's number, as the sim's row id
/// orders it, not its name's text: of three lit NORMAL beacons equally far
/// from the core, `b_99` and `b_100` are next to shed and raised, and
/// `b_101` is not (by text, `b_100` and `b_101` would sort before `b_99`).
#[test]
fn the_shed_orders_last_tie_is_the_beacons_number() {
    let mut script = Script::new(1);
    script.supply = 10;
    script.draw = 11;
    for (id, at, ms) in [
        ("b_99", [40, 32, GROUND], 3_000),
        ("b_100", [24, 32, GROUND], 2_000),
        ("b_101", [32, 40, GROUND], 1_000),
    ] {
        script.beacons.push(Beacon {
            id: id.to_owned(),
            at,
            owner: 1,
            core: false,
            powered: true,
            priority: "normal",
        });
        script.to_beacon_ms.insert(id.to_owned(), ms);
    }
    let (script, _) = advise(script);
    assert_eq!(
        safe_labels(&script.transcript),
        ["to_safety", "raise_b_100", "raise_b_99", "deepen_core"]
    );
}

/// The register's S1-14 as decision 14 reads it: a placed beacon adds no
/// draw (net zero through its own key-core), so no goal is charged a kW
/// shortfall. A seat whose supply only just carries its draw scores the same
/// vent and the same seam for the same points as a seat with supply to
/// spare.
#[test]
fn a_placed_beacon_is_charged_no_draw_however_tight_the_grid() {
    let points = |supply: i64, draw: i64| {
        let mut world = Script::busy(1);
        world.supply = supply;
        world.draw = draw;
        let (_, advice) = advise(world);
        [
            why_of(&advice, "hold_and_build"),
            why_of(&advice, "expand_and_mine"),
        ]
        .map(|why| {
            why.split(": ")
                .find(|part| part.contains(" points for "))
                .map_or_else(|| panic!("a scored why: {why}"), str::to_owned)
        })
    };
    let spare = points(10, 4);
    let tight = points(10, 10);
    assert_eq!(spare, tight);
}

/// The core's dig depth is Expand & Mine's own value for a new Mine beacon
/// ([`CORE_DIG_MAX_DEPTH`]'s doc says the two agree): the template's declared
/// `dig_max_depth`, read from the library as the gateway declares it.
#[test]
fn the_cores_dig_depth_is_expand_and_mines_own() {
    let (_, declared) = template("expand_and_mine").expect("the template");
    let own = declared
        .iter()
        .find(|(pointer, _)| pointer.ends_with("/mine/dig_max_depth"))
        .map(|(_, value)| value.clone())
        .expect("a declared dig depth");
    assert_eq!(own, CORE_DIG_MAX_DEPTH.to_string());
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

// ---------------------------------------------------------------------------
// The guard post: Defend and the threat model under Easy (S2's `oprd`)
// ---------------------------------------------------------------------------

/// The patched text's template: the `template_id` of the instantiate the
/// composing patch was applied to, read as the last instantiate before it.
fn composed_template(transcript: &[String]) -> String {
    let at = transcript
        .iter()
        .position(|line| line.starts_with("patch_plan "))
        .expect("a composing patch");
    let earlier = transcript.get(..at).expect("the lines before it");
    // Every template is instantiated once with its own values before the
    // composition; the composed one is named by the patch's own note.
    let ops = composing_ops(transcript);
    let note = op_value(&ops, "replace", "/meta/note")
        .or_else(|| op_value(&ops, "add", "/meta/note"))
        .and_then(|note| text(&note).map(str::to_owned))
        .expect("the patch writes the note");
    assert!(
        earlier
            .iter()
            .any(|line| line.starts_with("instantiate_template")),
        "the templates were declared first"
    );
    note
}

/// Spec section 14 and decision 35: in the round after a Push in which its
/// commander died, Easy adds a Defend guard on it -- the Defend the Core
/// template's place step, naming the seam its core covers, inserted right
/// after the core's own step and ahead of the round's goal.
#[test]
fn easy_adds_a_guard_post_in_the_round_after_its_commander_died() {
    let mut world = Script::busy(0);
    world.round = 2;
    world.feed_deaths = vec![0];
    let (script, played) = play(world);
    assert_eq!(played.submitted, Submitted::Own, "{:#?}", script.transcript);
    let ops = composing_ops(&script.transcript);
    let adds: Vec<&Json> = ops
        .iter()
        .filter(|op| {
            op.get("op").and_then(text) == Some("add")
                && op.get("path").and_then(text) == Some("/declarative/route/0")
        })
        .collect();
    assert_eq!(adds.len(), 2, "the guard's step, then the core's: {ops:#?}");
    let guard = adds
        .first()
        .and_then(|op| op.get("value"))
        .expect("the guard's step");
    assert_eq!(guard.get("label").and_then(text), Some("place_guard"));
    let covering = pointer_get(guard, "/place_beacon/at/covering/feature_id").and_then(text);
    assert_eq!(
        covering,
        Some("seam_36_34"),
        "the seam the core covers, by name"
    );
    let protect = pointer_get(guard, "/place_beacon/initial/mandate/defend/protect")
        .map(compact)
        .expect("a protect list");
    assert!(protect.contains("my_commander"), "{protect}");
    let deepen = adds
        .get(1)
        .and_then(|op| op.get("value"))
        .and_then(|step| step.get("label"))
        .and_then(text);
    assert_eq!(deepen, Some("deepen_core"));
    let note = composed_template(&script.transcript);
    assert!(
        note.contains("your commander died in the last Push"),
        "{note}"
    );
}

/// Only this seat's death counts: another seat's commander dying where this
/// seat could see adds no guard.
#[test]
fn another_seats_death_adds_no_guard_post() {
    let mut world = Script::busy(0);
    world.round = 2;
    world.feed_deaths = vec![1];
    let (script, played) = play(world);
    assert_eq!(played.submitted, Submitted::Own);
    let ops = composing_ops(&script.transcript);
    assert!(
        !compact(&Json::Array(ops)).contains("place_guard"),
        "no guard post"
    );
}

/// With nothing else worth a beacon, the guard post is the round: the Defend
/// the Core template, its description replaced by the seam's name.
#[test]
fn a_guard_post_alone_fills_the_defend_template() {
    let mut world = Script::new(0);
    let mut start = Feature::new("seam", "standard", [36, 34]);
    start.covered = true;
    world.features.push(start);
    world.round = 3;
    world.feed_deaths = vec![0];
    let (script, played) = play(world);
    assert_eq!(played.submitted, Submitted::Own, "{:#?}", script.transcript);
    let ops = composing_ops(&script.transcript);
    let covering = op_value(
        &ops,
        "replace",
        "/declarative/route/1/place_beacon/at/covering",
    )
    .map(|value| compact(&value));
    assert_eq!(
        covering.as_deref(),
        Some(r#"{ "feature_id": "seam_36_34" }"#),
        "{}",
        compact(&Json::Array(ops.clone()))
    );
    let note = composed_template(&script.transcript);
    assert!(note.contains("A Defend beacon beside the core"), "{note}");
}

/// Decision 12's threat model scores the post: armed enemies the view shows
/// inside the core's sphere, by their weapons' damage per second from the
/// rules rows, place it with no death; unarmed ones do not.
#[test]
fn armed_enemies_inside_the_cores_sphere_place_a_guard_post_and_unarmed_ones_do_not() {
    let armed = |subtype: &'static str| {
        let mut world = Script::busy(0);
        for (n, at) in [[30, 28, GROUND], [34, 28, GROUND]].into_iter().enumerate() {
            world.entities.push(Entity {
                id: format!("u_9{n}"),
                kind: "unit",
                subtype,
                owner: 1,
                at,
            });
        }
        let (script, _) = play(world);
        compact(&Json::Array(composing_ops(&script.transcript))).contains("place_guard")
    };
    assert!(armed("raider"), "two raiders by the core: a guard post");
    assert!(!armed("scout"), "two scouts are unarmed: no guard post");
}

/// The advisor offers the guard post's site to the wizard's Defend the Core
/// page by name, whether or not Easy would place it this round.
#[test]
fn the_advisor_suggests_the_guard_posts_seam_by_name() {
    let (_, advice) = advise(Script::busy(0));
    let defend = advice
        .suggestions
        .iter()
        .find(|suggestion| suggestion.template_id == "defend_the_core")
        .expect("a Defend the Core suggestion");
    assert!(
        defend.parameters.iter().any(|value| value.pointer
            == "/declarative/route/1/place_beacon/at/covering"
            && value.value == r#"{"feature_id":"seam_36_34"}"#),
        "{defend:#?}"
    );
    assert!(
        defend.why.contains("did not die in the last Push"),
        "{}",
        defend.why
    );
}

/// A feed paged past the operator's bound before the line naming whose
/// commander died is reached adds no guard, and the why says so.
#[test]
fn an_unread_death_adds_no_guard_and_the_why_says_so() {
    let mut world = Script::busy(0);
    world.paging = Paging::ToTheBound;
    world.feed_deaths = vec![0];
    let (script, played) = play(world);
    assert!(
        !script
            .transcript
            .iter()
            .any(|line| line.starts_with("patch_plan") && line.contains("place_guard")),
        "no guard post"
    );
    assert!(played.why.contains("not read within"), "{}", played.why);
}

/// Posts never stack: with a Defend beacon already standing inside the
/// core's sphere, no second post is weighed, even after a death.
#[test]
fn a_post_already_beside_the_core_is_not_placed_again() {
    let mut world = Script::busy(0);
    world.round = 2;
    world.feed_deaths = vec![0];
    world.defend_post = Some([34, 34, GROUND]);
    let (script, played) = play(world);
    assert_eq!(played.submitted, Submitted::Own);
    assert_eq!(
        script
            .transcript
            .iter()
            .filter(|line| line.starts_with("estimate_route") && line.contains("seam_36_34"))
            .count(),
        1,
        "no guard site is estimated: the seam's one estimate is the Survey post's"
    );
    assert!(
        !script
            .transcript
            .iter()
            .any(|line| line.starts_with("patch_plan") && line.contains("place_guard")),
        "no guard post is placed"
    );
}

/// A seat with its starting seam covered and nothing else to place, and the
/// enemy beacon `e_01` it saw 20 s ago, 40 voxels north of its core.
fn raid_world() -> Script {
    let mut world = Script::new(0);
    let mut start = Feature::new("seam", "standard", [36, 34]);
    start.covered = true;
    world.features.push(start);
    world.beacons.push(Beacon {
        id: String::from("e_01"),
        at: [32, 72, GROUND],
        owner: 1,
        core: false,
        powered: false,
        priority: "",
    });
    world.sighting_ages.insert(String::from("e_01"), 20_000);
    world
}

/// The raid's target, staging and post seam, as the composing patch writes
/// them into the Raid template's place step.
fn raid_ops(transcript: &[String]) -> (Option<String>, Option<String>, Option<String>) {
    let ops = composing_ops(transcript);
    let at = |pointer: &str| {
        op_value(
            &ops,
            "replace",
            &format!("/declarative/route/1/place_beacon/{pointer}"),
        )
        .map(|value| compact(&value))
    };
    (
        at("initial/mandate/attack/target"),
        at("initial/mandate/attack/staging"),
        at("at/covering"),
    )
}

/// S2's `opra`: an enemy beacon the seat saw inside the reach window is
/// raided by its `e_NN`, from an Attack beacon covering the seam beside the
/// core, its raiders staged at the site the estimate answered.
#[test]
fn easy_raids_the_nearest_enemy_beacon_it_knows_of_by_name() {
    let (script, played) = play(raid_world());
    assert_eq!(played.submitted, Submitted::Own, "{:#?}", script.transcript);
    let (target, staging, covering) = raid_ops(&script.transcript);
    assert_eq!(
        target.as_deref(),
        Some(r#"{ "beacon": { "beacon_id": "e_01" } }"#)
    );
    assert_eq!(
        covering.as_deref(),
        Some(r#"{ "feature_id": "seam_36_34" }"#)
    );
    // The scripted site of a `covering` waypoint is four voxels west of the
    // seam's anchor.
    assert_eq!(
        staging,
        Some(format!(r#"{{ "x": 32, "y": 34, "z": {GROUND} }}"#))
    );
    let note = composed_template(&script.transcript);
    assert!(note.contains("raid the enemy beacon e_01"), "{note}");
}

/// A sighting older than `verifier.reach_memory_ms` is E0418's refusal at
/// the seal, so it is never weighed: no estimate goes to it.
#[test]
fn a_stale_sighting_is_not_raided() {
    let mut world = raid_world();
    world.sighting_ages.insert(String::from("e_01"), 180_001);
    let (script, _) = play(world);
    assert!(
        !script
            .transcript
            .iter()
            .any(|line| line.starts_with("estimate_route") && line.contains("e_01")),
        "{:#?}",
        script.transcript
    );
}

/// Another seat's beacon with no sighting age is an answer the operator
/// cannot tell fresh from stale: the round is not read, and ready is said.
#[test]
fn an_enemy_beacon_with_no_sighting_age_is_a_round_not_read() {
    let mut world = raid_world();
    world.ageless.insert(String::from("e_01"));
    let (_, played) = play(world);
    assert_eq!(
        played.refused.map(|refused| refused.code).as_deref(),
        Some("MALFORMED")
    );
    assert!(played.ready);
}

/// Spec section 14: Easy "scores enemy-commander raids like any other
/// target". One rule weighs both kinds, so the threat in sight beside a
/// target costs a commander what it costs a beacon: whichever of the two is
/// defended by two raiders loses to the undefended one, the nearer or not.
#[test]
fn an_enemy_commander_is_raided_by_its_seat_and_scored_like_a_beacon() {
    let raided = |defended: [i32; 3]| {
        let mut world = raid_world();
        world.entities.push(Entity {
            id: String::from("u_7"),
            kind: "unit",
            subtype: "commander",
            owner: 1,
            at: [62, 40, GROUND],
        });
        for (n, dx) in [-2, 2].into_iter().enumerate() {
            let [x, y, z] = defended;
            world.entities.push(Entity {
                id: format!("u_8{n}"),
                kind: "unit",
                subtype: "raider",
                owner: 1,
                at: [x + dx, y, z],
            });
        }
        let (script, played) = play(world);
        assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
        raid_ops(&script.transcript).0
    };
    assert_eq!(
        raided([32, 72, GROUND]).as_deref(),
        Some(r#"{ "enemy_commander": { "seat": 1 } }"#),
        "the beacon is defended: the commander, by its seat"
    );
    assert_eq!(
        raided([62, 40, GROUND]).as_deref(),
        Some(r#"{ "beacon": { "beacon_id": "e_01" } }"#),
        "the commander is defended: the beacon, though it is further"
    );
}

/// Two enemy commanders the same distance from the core: ties go to the
/// lowest seat (the owner's answer of 2026-10-26), as the sim's hunt breaks
/// them.
#[test]
fn a_tie_between_two_commanders_goes_to_the_lowest_seat() {
    let mut world = raid_world();
    world.sighting_ages.insert(String::from("e_01"), 180_001);
    for (owner, x) in [(2, 62), (1, 2)] {
        world.entities.push(Entity {
            id: format!("u_{owner}"),
            kind: "unit",
            subtype: "commander",
            owner,
            at: [x, 32, GROUND],
        });
    }
    let (script, played) = play(world);
    assert_eq!(played.submitted, Submitted::Own, "{:#?}", script.transcript);
    assert_eq!(
        raid_ops(&script.transcript).0.as_deref(),
        Some(r#"{ "enemy_commander": { "seat": 1 } }"#)
    );
}

/// The advisor offers the nearest raid to the wizard's Raid an Enemy Beacon
/// page: its post seam, its target by name and its staging at the post's
/// site.
#[test]
fn the_advisor_suggests_the_nearest_raid_by_name() {
    let (_, advice) = advise(raid_world());
    let raid = advice
        .suggestions
        .iter()
        .find(|suggestion| suggestion.template_id == "raid_an_enemy_beacon")
        .expect("a Raid an Enemy Beacon suggestion");
    let value = |pointer: &str| {
        raid.parameters
            .iter()
            .find(|value| value.pointer == format!("/declarative/route/1/place_beacon/{pointer}"))
            .map(|value| value.value.clone())
    };
    assert_eq!(
        value("initial/mandate/attack/target").as_deref(),
        Some(r#"{"beacon":{"beacon_id":"e_01"}}"#),
        "{raid:#?}"
    );
    assert_eq!(
        value("initial/mandate/attack/staging"),
        Some(format!(r#"{{"x":32,"y":34,"z":{GROUND}}}"#))
    );
    assert_eq!(
        value("at/covering").as_deref(),
        Some(r#"{"feature_id":"seam_36_34"}"#)
    );
    // With nothing known, the template's own values stand and the why says
    // what a raid needs.
    let (_, advice) = advise(Script::busy(0));
    let raid = advice
        .suggestions
        .iter()
        .find(|suggestion| suggestion.template_id == "raid_an_enemy_beacon")
        .expect("a Raid an Enemy Beacon suggestion");
    assert!(raid.parameters.is_empty(), "{raid:#?}");
    assert!(raid.why.contains("No raid was weighed"), "{}", raid.why);
}

/// A raid's price counts the raiders the Raid template's own launch waits
/// for, so [`RAID_FORCE_SIZE`] is that template's `force_size`.
#[test]
fn the_raid_force_is_the_templates_own() {
    let (text, _) = template("raid_an_enemy_beacon").expect("the Raid template");
    let body = read(&strip_comments(&text)).expect("it reads");
    let force = pointer_get(
        &body,
        "/declarative/route/1/place_beacon/initial/mandate/attack/launch/force_size",
    )
    .map(compact);
    assert_eq!(force, Some(RAID_FORCE_SIZE.to_string()));
}

/// Easy's raid is a fixed target, so it seals `retarget: NONE` where the
/// template's own value is `NEAREST_KNOWN` (spec section 14: Easy's
/// selectors are "Fixed only", and retargeting is a rule of Normal's).
#[test]
fn easy_seals_its_raid_with_no_retarget() {
    let (script, _) = play(raid_world());
    let ops = composing_ops(&script.transcript);
    assert_eq!(
        op_value(
            &ops,
            "replace",
            "/declarative/route/1/place_beacon/initial/mandate/attack/retarget"
        )
        .map(|value| compact(&value))
        .as_deref(),
        Some(r#""NONE""#)
    );
}

/// The window's edge is inside it: a sighting exactly `reach_memory_ms` old
/// is raided, as E0418 accepts it, and one a millisecond older is not.
#[test]
fn a_sighting_at_the_windows_edge_is_raided() {
    let mut world = raid_world();
    world.sighting_ages.insert(String::from("e_01"), 180_000);
    let (script, _) = play(world);
    assert_eq!(
        raid_ops(&script.transcript).0.as_deref(),
        Some(r#"{ "beacon": { "beacon_id": "e_01" } }"#)
    );
}

/// A sighting age below zero is no age: the round is not read.
#[test]
fn a_negative_sighting_age_is_a_round_not_read() {
    let mut world = raid_world();
    world.sighting_ages.insert(String::from("e_01"), -1);
    let (_, played) = play(world);
    assert_eq!(
        played.refused.map(|refused| refused.code).as_deref(),
        Some("MALFORMED")
    );
}

/// "Nearest": of two fresh enemy beacons, the nearer the core is raided, and
/// one no route reaches is dropped for the next.
#[test]
fn the_nearest_reachable_enemy_beacon_is_raided() {
    let with_far = || {
        let mut world = raid_world();
        world.beacons.push(Beacon {
            id: String::from("e_02"),
            at: [32, 100, GROUND],
            owner: 1,
            core: false,
            powered: false,
            priority: "",
        });
        world
    };
    let (script, _) = play(with_far());
    assert_eq!(
        raid_ops(&script.transcript).0.as_deref(),
        Some(r#"{ "beacon": { "beacon_id": "e_01" } }"#)
    );
    let mut cut_off = with_far();
    cut_off.unreachable.insert(String::from("e_01"));
    let (script, _) = play(cut_off);
    assert_eq!(
        raid_ops(&script.transcript).0.as_deref(),
        Some(r#"{ "beacon": { "beacon_id": "e_02" } }"#)
    );
}

/// At most [`RAID_TARGETS_MAX`] targets are weighed, nearest first: a fourth,
/// further than the other three, is never estimated.
#[test]
fn raid_targets_are_cut_nearest_first() {
    let mut world = raid_world();
    for (n, y) in [(2, 80), (3, 90), (4, 120)] {
        world.beacons.push(Beacon {
            id: format!("e_0{n}"),
            at: [32, y, GROUND],
            owner: 1,
            core: false,
            powered: false,
            priority: "",
        });
    }
    let (script, _) = play(world);
    let estimated = |id: &str| {
        script
            .transcript
            .iter()
            .any(|line| line.starts_with("estimate_route") && line.contains(&format!("\"{id}\"")))
    };
    assert!(estimated("e_01") && estimated("e_02") && estimated("e_03"));
    assert!(!estimated("e_04"), "{:#?}", script.transcript);
    assert_eq!(RAID_TARGETS_MAX, 3);
}

/// An Attack beacon of the seat's already beside the core regroups and
/// relaunches every segment, so no second raid post is placed; a guard post
/// added for a death takes the seam a raid post would cover, so that round
/// makes no raid either.
#[test]
fn no_raid_post_stacks_on_one_or_on_a_guard() {
    let mut standing = raid_world();
    standing.attack_post = Some([33, 36, GROUND]);
    let (script, _) = play(standing);
    assert!(
        !script
            .transcript
            .iter()
            .any(|line| line.starts_with("estimate_route") && line.contains("e_01")),
        "no raid weighed beside a standing post"
    );
    let mut died = raid_world();
    died.round = 2;
    died.feed_deaths = vec![0];
    let (script, played) = play(died);
    assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
    let ops = compact(&Json::Array(composing_ops(&script.transcript)));
    assert!(ops.contains("A Defend beacon beside the core"), "{ops}");
    assert!(!ops.contains("Attack beacon"), "{ops}");
    assert!(!ops.contains("\"attack\""), "{ops}");
}

// ---------------------------------------------------------------------------
// The Resonance Spire (S3's `opr3`)
// ---------------------------------------------------------------------------

/// The briefing's spelling of the Resonance Spire's licence: `gp.v1.Licence`'s
/// value in the wire's lower case.
const SPIRE_LICENCE: &str = "resonance_spire";

/// The composing patch's `queue_spire` steps.
fn spire_steps(ops: &[Json]) -> Vec<Json> {
    ops.iter()
        .filter_map(|op| op.get("value"))
        .flat_map(|value| match value {
            Json::Array(steps) => steps.clone(),
            other => vec![other.clone()],
        })
        .filter(|step| step.get("label").and_then(text) == Some("queue_spire"))
        .collect()
}

/// A busy seat that holds the Spire's licence.
fn licensed() -> Script {
    let mut world = Script::busy(0);
    world.licences = vec![SPIRE_LICENCE];
    world.treasury = 1_000;
    world
}

/// Held licence, a core at the base radius, headroom for its 3 kW and the
/// treasury for its $120: the Spire is queued at the core once, by one
/// `queue_structure` row in an `interface` step of its own, inserted right
/// after the core's dig depth (so the patch adds it at route 0 just before
/// `deepen_core`), and the round is Easy's own.
#[test]
fn easy_queues_the_spire_at_its_core_once_it_holds_the_licence() {
    let (script, played) = play(licensed());
    assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
    let ops = composing_ops(&script.transcript);
    let steps = spire_steps(&ops);
    assert_eq!(steps.len(), 1, "queued exactly once: {ops:#?}");
    let step = compact(steps.first().unwrap());
    assert!(
        step.contains(r#""beacon": { "beacon_id": "b_00" }"#)
            && step.contains(r#""queue_structure": { "blueprint_id": "resonance_spire" }"#)
            && step.contains(r#""on_fail": { "action": "SKIP" }"#),
        "{step}"
    );
    let adds: Vec<&str> = ops
        .iter()
        .filter(|op| op.get("path").and_then(text) == Some("/declarative/route/0"))
        .filter_map(|op| op.get("value")?.get("label").and_then(text))
        .collect();
    assert_eq!(
        adds.last().copied(),
        Some("deepen_core"),
        "the core's step lands at route 0 last: {adds:?}"
    );
    assert_eq!(
        adds.iter().rev().nth(1).copied(),
        Some("queue_spire"),
        "the Spire's lands right after it: {adds:?}"
    );
    assert!(played.why.contains("Resonance Spire"), "{}", played.why);
}

/// No licence, no Spire; and a licence the schema does not name is a round
/// not read rather than one read as unlicensed.
#[test]
fn no_licence_queues_no_spire_and_an_unknown_licence_is_refused() {
    let mut world = licensed();
    world.licences = Vec::new();
    let (script, played) = play(world);
    assert_eq!(played.submitted, Submitted::Own);
    assert!(
        !script
            .transcript
            .iter()
            .any(|line| line.contains("resonance_spire")),
        "{:#?}",
        script.transcript
    );
    let mut unknown = licensed();
    unknown.licences = vec!["sensor_spire"];
    let (_, played) = play(unknown);
    assert_eq!(
        played.refused.map(|refused| (refused.method, refused.code)),
        Some((String::from("get_briefing"), String::from("MALFORMED")))
    );
}

/// Never twice across rounds: a Spire of the seat's in view, finished or
/// still going up, or a core whose sphere is grown past the base, queues no
/// second one; and an incomplete view, which could not show one, queues none.
#[test]
fn a_spire_standing_or_going_up_is_never_queued_again() {
    let mut going_up = licensed();
    going_up.entities.push(Entity {
        id: String::from("s_9"),
        kind: "structure",
        subtype: "resonance_spire",
        owner: 0,
        at: [34, 32, GROUND],
    });
    let mut grown = licensed();
    grown.radii.insert(String::from("b_00"), 36);
    let mut unseen = licensed();
    unseen.paging = Paging::ViewNeverComplete;
    for (what, world) in [("going up", going_up), ("grown", grown), ("unseen", unseen)] {
        let (script, _) = play(world);
        assert!(
            !script
                .transcript
                .iter()
                .any(|line| line.contains("queue_structure")),
            "{what}: {:#?}",
            script.transcript
        );
    }
}

/// The grid and the treasury: a headroom below the Spire's draw, or a
/// treasury below its price once the round's goals are weighed, queues none.
#[test]
fn a_spire_the_grid_or_the_treasury_cannot_carry_is_not_queued() {
    let mut short = licensed();
    short.supply = 6;
    short.draw = 4;
    let mut poor = licensed();
    poor.treasury = 100;
    for (what, world) in [("kW", short), ("$", poor)] {
        let (script, _) = play(world);
        assert!(
            !script
                .transcript
                .iter()
                .any(|line| line.contains("queue_structure")),
            "{what}: {:#?}",
            script.transcript
        );
    }
}

/// A round whose only addition is the Spire is the Safe Playbook template
/// with its route filled -- its own walk to safety, the core's dig depth, the
/// Spire -- and is Easy's own seal, not its safe playbook.
#[test]
fn a_round_with_only_the_spire_fills_the_safe_template_as_its_own() {
    let mut world = Script::new(0);
    world.licences = vec![SPIRE_LICENCE];
    world.treasury = 1_000;
    let (script, played) = play(world);
    assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
    let ops = composing_ops(&script.transcript);
    let route = op_value(&ops, "replace", "/declarative/route").expect("the route filled");
    let Json::Array(steps) = route else {
        panic!("a route: {ops:#?}");
    };
    let labels: Vec<&str> = steps
        .iter()
        .filter_map(|step| step.get("label").and_then(text))
        .collect();
    assert_eq!(labels, ["to_safety", "deepen_core", "queue_spire"]);
    assert!(
        script
            .transcript
            .iter()
            .any(|line| line.starts_with("patch_plan") && line.contains("Safe playbook")),
        "the Safe Playbook template's own text is the base"
    );
}

/// The repair of last resort still takes out the right step with the Spire
/// in front: two seams, the second inserted at template route 1, which
/// stands at route 3 behind `deepen_core` and `queue_spire` (route 2 without
/// the Spire).
#[test]
fn a_repair_removes_the_right_step_behind_the_spire() {
    let mut rich = Script::new(0);
    for (anchor, site) in [([40, 30], [36, 30, GROUND]), ([24, 30], [28, 30, GROUND])] {
        let mut seam = Feature::new("seam", "rich", anchor);
        seam.site = Some(site);
        rich.features.push(seam);
    }
    rich.treasury = 500;
    rich.verify = Verify::NeverUnfixed;
    let removal = |world: Script| {
        let (script, _) = play(world);
        script
            .transcript
            .iter()
            .filter_map(|line| line.strip_prefix("patch_plan "))
            .nth(1)
            .and_then(|params| read(params).ok())
            .and_then(|params| params.get("json_patch").and_then(text).map(str::to_owned))
            .expect("a second patch: the removal")
    };
    assert_eq!(
        removal(rich.clone()),
        r#"[{"op":"remove","path":"/declarative/route/2"}]"#
    );
    rich.licences = vec![SPIRE_LICENCE];
    rich.treasury = 1_000;
    assert_eq!(
        removal(rich),
        r#"[{"op":"remove","path":"/declarative/route/3"}]"#
    );
}

/// The advisor never offers the Spire: a human seat's safe playbook leaves
/// configurations untouched and spends nothing (spec section 14), so neither
/// it nor any suggestion queues one.
#[test]
fn the_spire_is_never_in_the_safe_playbook_or_a_suggestion() {
    let (script, advice) = advise(licensed());
    assert!(
        !script
            .transcript
            .iter()
            .any(|line| line.contains("queue_structure")),
        "{:#?}",
        script.transcript
    );
    assert!(!advice.safe_playbook_jsonc.contains("queue_structure"));
    assert!(advice.suggestions.iter().all(|suggestion| {
        suggestion
            .parameters
            .iter()
            .all(|value| !value.value.contains("queue_structure"))
    }));
}

// ---------------------------------------------------------------------------
// Stepping stones (S3's `opr3`; the register's S5-04)
// ---------------------------------------------------------------------------

/// The `estimate_route` lines that end on a `toward` naming `feature`.
fn towards(transcript: &[String], feature: &str) -> usize {
    transcript
        .iter()
        .filter(|line| {
            line.starts_with("estimate_route")
                && line.contains(&format!(
                    r#""toward": {{ "feature": {{ "feature_id": "{feature}" }} }}"#
                ))
        })
        .count()
}

/// The busy seat whose vent no site of its spheres covers: the `covering`
/// estimate is refused `NOT_FOUND`, and a `toward` names the stone.
fn far_vent() -> Script {
    let mut world = Script::busy(0);
    world.features.retain(|feature| feature.id != "seam_20_60");
    for feature in &mut world.features {
        if feature.id == "vent_56_32" {
            feature.site = None;
            feature.stone = Some([50, 32, GROUND]);
        }
    }
    world
}

/// A vent no site covers is weighed once more as a stone: one `toward`
/// estimate naming it, and the round places a beacon `toward` it by name,
/// with a timeout of its own (never Hold & Build's 60 s: fix pass 2, item 1;
/// `no_composed_placement_times_out_before_its_own_price`) and `on_fail`
/// SKIP.
#[test]
fn a_far_vent_refused_for_covering_is_reached_with_a_stone() {
    let (script, played) = play(far_vent());
    assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
    assert_eq!(towards(&script.transcript, "vent_56_32"), 1);
    let ops = compact(&Json::Array(composing_ops(&script.transcript)));
    assert!(
        ops.contains(r#""place_beacon": { "at": { "toward": { "feature": { "feature_id": "vent_56_32" } } } }"#),
        "{ops}"
    );
    assert!(!ops.contains(r#""timeout_ms": 60000"#), "{ops}");
    assert!(ops.contains(r#""label": "stone""#), "{ops}");
    assert!(played.why.contains("stepping-stone"), "{}", played.why);
}

/// A `toward` the gateway refuses drops the stone, and the round seals what
/// else it has (here nothing: its safe playbook).
#[test]
fn a_refused_toward_drops_the_stone() {
    let mut world = far_vent();
    for feature in &mut world.features {
        feature.stone = None;
    }
    let (script, played) = play(world);
    assert_eq!(towards(&script.transcript, "vent_56_32"), 1);
    assert!(
        !script
            .transcript
            .iter()
            .any(|line| line.starts_with("patch_plan") && line.contains("toward")),
        "{:#?}",
        script.transcript
    );
    // What else the round has is its Survey post: no enemy sighting is held.
    assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
    assert!(played.why.contains("A Survey beacon"), "{}", played.why);
}

/// Every refused placement is weighed again once, and never twice; two
/// stones the sim would stand on one site add one beacon, not two; and the
/// bound is the candidate count, so it never binds.
#[test]
fn every_refused_placement_is_weighed_once_and_one_site_takes_one_stone() {
    let mut world = far_vent();
    let mut seam = Feature::new("seam", "rich", [56, 40]);
    seam.site = None;
    seam.stone = Some([50, 32, GROUND]);
    world.features.push(seam);
    world.treasury = 1_000;
    let (script, _) = play(world);
    assert_eq!(towards(&script.transcript, "vent_56_32"), 1);
    assert_eq!(towards(&script.transcript, "seam_56_40"), 1);
    let ops = compact(&Json::Array(composing_ops(&script.transcript)));
    assert_eq!(ops.matches(r#""toward""#).count(), 1, "{ops}");
    assert_eq!(STONE_TARGETS_MAX, EASY_CANDIDATES);
}

/// The wizard never sees a stone: the advisor weighs none, so it makes no
/// `toward` estimate, and Hold & Build's page says why it has no vent.
#[test]
fn the_advisor_weighs_no_stone() {
    let (script, advice) = advise(far_vent());
    assert_eq!(towards(&script.transcript, "vent_56_32"), 0);
    let hold = advice
        .suggestions
        .iter()
        .find(|suggestion| suggestion.template_id == "hold_and_build")
        .expect("a Hold & Build page");
    assert!(hold.parameters.is_empty(), "{hold:#?}");
}

/// The other worst round: every candidate a placement no site covers and
/// every stone answered, so the stepping-stone term is spent to its bound,
/// with every other term but one `submit_plan` -- the paged reads, the tap
/// probe (a pair the gateway says is not legal), the guard and raid probes,
/// the templates, the composition, every repair and the safe playbook.
#[test]
fn the_worst_stone_round_spends_every_term() {
    let mut worst = short_with_four_lit(1);
    worst.paging = Paging::ToTheBound;
    worst.feed_deaths = vec![0];
    worst.verify = Verify::NeverButFixable;
    worst.treasury = 10_000;
    for n in 0..40 {
        let kind = if n % 2 == 0 { "seam" } else { "vent" };
        let mut feature = Feature::new(kind, "standard", [2 + n, 2]);
        feature.travel_ms = Some(1_000 + i64::from(n));
        feature.site = None;
        feature.stone = Some([30, 30 - n, GROUND]);
        worst.features.push(feature);
    }
    let mut covered = Feature::new("vent", "rich", [40, 40]);
    covered.covered = true;
    worst.features.push(covered);
    // More beacons at risk than the safe playbook estimates.
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
    let (script, played) = play(worst);
    let stones = script
        .transcript
        .iter()
        .filter(|line| line.starts_with("estimate_route") && line.contains("toward"))
        .count();
    assert_eq!(u32::try_from(stones).unwrap(), STONE_CALLS);
    assert_eq!(played.repairs, 4);
    assert_eq!(played.submitted, Submitted::Safe);
    assert_eq!(
        played.calls,
        u32::try_from(script.transcript.len()).unwrap()
    );
    // Every term but one `submit_plan` and the last seal's one (as above).
    assert_eq!(played.calls, EASY_CALL_BUDGET - 1 - LAST_SEAL_CALLS);
}

// ---------------------------------------------------------------------------
// The Survey post (S3's `opr3`; spec section 14; S3's plan, decision 3)
// ---------------------------------------------------------------------------

/// The `estimate_route` lines that probe for the nearest own Survey beacon.
fn survey_probes(transcript: &[String]) -> usize {
    transcript
        .iter()
        .filter(|line| {
            line.starts_with("estimate_route") && line.contains(r#""mandate": "SURVEY""#)
        })
        .count()
}

/// The `survey_post` step of the composing patch, if it carries one.
fn survey_step(transcript: &[String]) -> Option<Json> {
    composing_ops(transcript)
        .into_iter()
        .filter_map(|op| op.get("value").cloned())
        .flat_map(|value| match value {
            Json::Array(steps) => steps,
            other => vec![other],
        })
        .find(|step| step.get("label").and_then(text) == Some("survey_post"))
}

/// Spec section 14's trigger as written: with no enemy beacon sighted inside
/// the reach window, Easy probes once for a Survey post beside its core, and,
/// none standing, places one covering the post seam, its mandate Survey's
/// with one probe area over the whole map (`get_map_summary.size`, here 64 x
/// 64 x 32) and two scouts, ranked with the round's goals and funded beside
/// them.
#[test]
fn with_no_fresh_sighting_easy_places_a_survey_post_beside_its_core() {
    let (script, played) = play(Script::busy(0));
    assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
    assert_eq!(survey_probes(&script.transcript), 1);
    let step = compact(&survey_step(&script.transcript).expect("a Survey post"));
    for wanted in [
        r#""covering": { "feature_id": "seam_36_34" }"#,
        r#""tags": [ "survey" ]"#,
        // The core b_00 stands at (32, 32) of a 64-voxel map: one voxel past
        // the middle (31) on both axes, so `x` decides, and the far half is
        // the low one.
        r#""probe_areas": [ { "min": { "x": 0, "y": 0, "z": 0 }, "max": { "x": 31, "y": 63, "z": 31 } } ]"#,
        r#""scout_count": 2"#,
        r#""on_fail": { "action": "SKIP" }"#,
    ] {
        assert!(step.contains(wanted), "{wanted} in {step}");
    }
    assert!(
        !step.contains("defend"),
        "the Defend settings are replaced: {step}"
    );
    assert!(played.why.contains("A Survey beacon"), "{}", played.why);
}

/// A fresh sighting of an enemy beacon is what the raid needs, so no probe is
/// made and no post placed; a sighting older than the reach window is stale,
/// and the post goes up; at the window's edge it is still fresh.
#[test]
fn a_fresh_sighting_places_no_survey_post_and_a_stale_one_does() {
    let with_age = |age: i64| {
        let mut world = Script::busy(0);
        world.beacons.push(Beacon {
            id: String::from("e_01"),
            at: [60, 60, GROUND],
            owner: 1,
            core: false,
            powered: false,
            priority: "",
        });
        world.sighting_ages.insert(String::from("e_01"), age);
        world
    };
    for (age, surveys) in [(0, false), (180_000, false), (180_001, true)] {
        let (script, _) = play(with_age(age));
        assert_eq!(
            survey_probes(&script.transcript) == 1,
            surveys,
            "age {age}: {:#?}",
            script.transcript
        );
        assert_eq!(
            survey_step(&script.transcript).is_some(),
            surveys,
            "age {age}"
        );
    }
}

/// One Survey post at a time: one standing beside the core is not placed
/// again; a guard post added for a death takes the seam it would cover, so
/// that round places none; and a treasury below its beacon and two scouts
/// places none.
#[test]
fn a_survey_post_never_stacks_contends_with_the_guard_or_overspends() {
    let mut standing = Script::busy(0);
    standing.survey_post = Some([34, 34, GROUND]);
    let mut died = Script::busy(0);
    died.round = 2;
    died.feed_deaths = vec![0];
    let mut poor = Script::busy(0);
    poor.treasury = 79;
    for (what, world) in [("standing", standing), ("died", died), ("poor", poor)] {
        let (script, _) = play(world);
        let placed = script
            .transcript
            .iter()
            .filter(|line| line.starts_with("patch_plan"))
            .any(|line| line.contains("survey_post"));
        assert!(!placed, "{what}: {:#?}", script.transcript);
    }
}

/// The advisor offers no Survey post, so it never probes for one.
#[test]
fn the_advisor_never_probes_for_a_survey_post() {
    let (script, _) = advise(Script::busy(0));
    assert_eq!(survey_probes(&script.transcript), 0);
}

// ---------------------------------------------------------------------------
// Fix pass 1: one kW budget, refusals, stones worth their goal
// ---------------------------------------------------------------------------

/// A whole-number row of the rules text, by its path of keys.
fn rules_row(path: &[&str]) -> i64 {
    let table = read(&rules_json()).expect("the rules text reads");
    let row = path
        .iter()
        .try_fold(&table, |at, key| at.get(key))
        .unwrap_or_else(|| panic!("the rules text has no `{}` row", path.join(".")));
    match row {
        Json::Number(lexeme) => lexeme.parse().expect("a whole number"),
        other => panic!("`{}` is no number: {other:?}", path.join(".")),
    }
}

/// Every `scout_count` and `picket_scouts` a JSON value sets, summed.
fn scouts_set(value: &Json) -> i64 {
    match value {
        Json::Object(members) => members
            .iter()
            .map(|(key, held)| match (key.as_str(), held) {
                ("scout_count" | "picket_scouts", Json::Number(lexeme)) => {
                    lexeme.parse::<i64>().expect("a count")
                }
                _ => scouts_set(held),
            })
            .sum(),
        Json::Array(items) => items.iter().map(scouts_set).sum(),
        _ => 0,
    }
}

/// The new draw a composed round fields, kW, priced at the rows every grid
/// charges (fix pass 2, item 4): each scout it orders at `power.kw_per_unit`
/// -- a Survey post's `scout_count` and a guard copy's `picket_scouts`, read
/// from the patch, and the Defend the Core template's own picket when the
/// guard fills that template alone (its step is the template's, so the patch
/// does not carry it) -- and a queued Spire at its own `draw_kw`.
fn new_draw(transcript: &[String]) -> i64 {
    if !transcript
        .iter()
        .any(|line| line.starts_with("patch_plan "))
    {
        return 0;
    }
    let ops = Json::Array(composing_ops(transcript));
    let written = write(&ops);
    let spire = if written.contains("queue_spire") {
        rules_row(&["structures", "resonance_spire", "draw_kw"])
    } else {
        0
    };
    let guard_alone =
        !written.contains("place_guard") && written.contains("A Defend beacon beside the core");
    let template_picket = if guard_alone {
        let (text, _) = template("defend_the_core").expect("the library's Defend the Core");
        scouts_set(&read(&strip_comments(&text)).expect("the template reads"))
    } else {
        0
    };
    spire + (scouts_set(&ops) + template_picket) * rules_row(&["power", "kw_per_unit"])
}

/// One kW budget (fix pass 1, item 1): the guard's picket, the Spire and the
/// Survey post's scouts are weighed against one headroom, in that order, so
/// the round never fields more new draw than the grid carries. Probe 5's case
/// first (supply 10, draw 6: 4 kW, where the Spire and the post together are
/// 5), then every headroom from 0 to 8 kW with the commander dead in the last
/// Push, so the guard is weighed too.
#[test]
fn one_kw_budget_never_composes_more_draw_than_the_headroom() {
    let mut probe = licensed();
    probe.supply = 10;
    probe.draw = 6;
    let (script, played) = play(probe);
    assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
    assert_eq!(
        new_draw(&script.transcript),
        3,
        "the Spire alone fits 4 kW: {}",
        played.why
    );
    for died in [false, true] {
        for headroom in 0..=8_i64 {
            let mut world = licensed();
            world.supply = 10;
            world.draw = 10 - headroom;
            if died {
                world.round = 2;
                world.feed_deaths = vec![0];
            }
            let (script, _) = play(world);
            let drawn = new_draw(&script.transcript);
            assert!(
                drawn <= headroom,
                "{drawn} kW against {headroom} (died {died})"
            );
        }
    }
}

/// A stone is weighed only after the gateway's `NOT_FOUND` on `covering`;
/// any other refusal drops the goal with no stone, and is reported with the
/// round (fix pass 1, item 6).
#[test]
fn only_not_found_makes_a_stone_and_other_refusals_are_reported() {
    let mut world = far_vent();
    world
        .refuse_feature
        .insert(String::from("vent_56_32"), "RATE_LIMITED");
    let (script, played) = play(world);
    assert_eq!(towards(&script.transcript, "vent_56_32"), 0);
    assert_eq!(
        played
            .estimates_refused
            .iter()
            .map(|refused| refused.code.as_str())
            .collect::<Vec<_>>(),
        ["RATE_LIMITED"]
    );
    // A `toward` refused for another reason than `NOT_FOUND` is reported too.
    let mut toward = far_vent();
    toward
        .refuse_feature
        .insert(String::from("vent_56_32"), "NOT_FOUND");
    let (script, played) = play(toward);
    assert_eq!(towards(&script.transcript, "vent_56_32"), 1);
    assert!(
        played.estimates_refused.is_empty(),
        "{:?}",
        played.estimates_refused
    );
}

/// A stone is worth the goal it serves, less its price, over its walk and
/// the served placement's own time (fix pass 1, item 2): of two far vents as
/// far away, the rich one's stone outranks the lean one's, and a vent
/// placeable now outranks a stone toward another vent of its grade; one
/// stone a round.
#[test]
fn a_stone_is_worth_the_goal_it_serves_and_one_goes_up_a_round() {
    let mut world = Script::new(0);
    world.treasury = 1_000;
    let mut vent = Feature::new("vent", "rich", [56, 32]);
    vent.site = None;
    vent.stone = Some([50, 32, GROUND]);
    let mut lean = Feature::new("vent", "lean", [8, 32]);
    lean.site = None;
    lean.stone = Some([14, 32, GROUND]);
    world.features.extend([lean, vent]);
    let (script, played) = play(world.clone());
    assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
    let ops = compact(&Json::Array(composing_ops(&script.transcript)));
    assert_eq!(
        ops.matches(r#""toward""#).count(),
        1,
        "one stone a round: {ops}"
    );
    assert!(ops.contains("vent_56_32"), "the rich vent's stone: {ops}");

    let mut near = world;
    let mut placeable = Feature::new("vent", "rich", [40, 40]);
    placeable.site = Some([36, 40, GROUND]);
    near.features.push(placeable);
    let (script, played) = play(near);
    let first = composing_ops(&script.transcript);
    let covering = op_value(
        &first,
        "replace",
        "/declarative/route/0/place_beacon/at/covering",
    )
    .map(|value| compact(&value));
    assert_eq!(
        covering.as_deref(),
        Some(r#"{ "feature_id": "vent_40_40" }"#),
        "the vent placeable now is first: {}",
        played.why
    );
}

/// A Spire-grown core sphere is read where a tap is looked for (fix pass 1,
/// item 5): a covered vent 30 voxels from the core, past the base radius of
/// 24 and inside the grown 36, is a tap pair the probe asks about; at the
/// base radius it is not.
#[test]
fn a_tap_inside_a_spire_grown_sphere_is_found() {
    let world = |radius: i64| {
        let mut tap = Script::new(0);
        let mut vent = Feature::new("vent", "rich", [62, 32]);
        vent.covered = true;
        tap.features.push(vent);
        tap.on_reads
            .insert((String::from("b_00"), String::from("vent_62_32")));
        tap.radii.insert(String::from("b_00"), radius);
        tap
    };
    let probed = |radius: i64| {
        let (script, _) = play(world(radius));
        script
            .transcript
            .iter()
            .any(|line| line.starts_with("resolve_refs") && line.contains("vent_62_32"))
    };
    assert!(probed(36), "the grown sphere's tap is probed");
    assert!(!probed(24), "the base sphere does not reach it");
}

/// A goal the round's share cannot hold is no goal this round (fix pass 1):
/// three far rich seams whose stones rank above a near vent by rate, each too
/// long to fit the segment's share, no longer fill the top-k, so the vent
/// placeable now is composed.
#[test]
fn goals_too_long_for_the_share_do_not_crowd_out_one_that_fits() {
    let mut world = Script::new(0);
    world.treasury = 1_000;
    world.segment_ms = 60_000;
    world.remaining_ms = 60_000;
    for x in [2, 4, 6] {
        let mut seam = Feature::new("seam", "rich", [x, 63]);
        seam.site = None;
        // Far enough that its walk (500 ms a voxel) passes the 42 s share.
        seam.stone = Some([x, 75, GROUND]);
        world.features.push(seam);
    }
    let mut vent = Feature::new("vent", "lean", [40, 32]);
    vent.site = Some([36, 32, GROUND]);
    world.features.push(vent);
    let (script, played) = play(world);
    assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
    let ops = composing_ops(&script.transcript);
    assert_eq!(
        op_value(
            &ops,
            "replace",
            "/declarative/route/0/place_beacon/at/covering"
        )
        .map(|value| compact(&value))
        .as_deref(),
        Some(r#"{ "feature_id": "vent_40_32" }"#),
        "{}",
        played.why
    );
}

// ---------------------------------------------------------------------------
// Fix pass 2: each step's timeout is its own
// ---------------------------------------------------------------------------

/// The travel estimate's allowance for the wait before a walk starts, as the
/// rules text derives it: `(4095 - 1) / repath_cap_per_tick + 1` ticks of
/// 50 ms (`crates/operator/src/tuning.rs`' `walk_allowance_ms`, which
/// `crates/gamectl/tests/ladder.rs` holds to the sim's).
fn walk_allowance_ms() -> i64 {
    let ticks = (4_095_i64 - 1)
        .checked_div(rules_row(&["locomotion", "repath_cap_per_tick"]))
        .expect("a positive repath cap");
    (ticks + 1) * 50
}

/// Every placement a round composes, as the whole step it seals: each step
/// the patch adds whole, and the template's own place step with what the
/// patch writes into it (its `covering`, its `timeout_ms`), read from the
/// text the composing `patch_plan` was given.
fn composed_placements(transcript: &[String]) -> Vec<Json> {
    let params = transcript
        .iter()
        .find_map(|line| line.strip_prefix("patch_plan "))
        .and_then(|params| read(params).ok())
        .expect("one composing patch");
    let base = params
        .get("playbook_jsonc")
        .and_then(text)
        .and_then(|jsonc| read(&strip_comments(jsonc)).ok())
        .expect("the template the patch was applied to");
    let ops = composing_ops(transcript);
    let mut steps: Vec<Json> = ops
        .iter()
        .filter(|op| matches!(op.get("op").and_then(text), Some("add" | "replace")))
        .filter_map(|op| op.get("value"))
        .filter(|step| step.get("place_beacon").is_some())
        .cloned()
        .collect();
    for op in &ops {
        let Some(path) = op.get("path").and_then(text) else {
            continue;
        };
        let Some(step_at) = path.strip_suffix("/timeout_ms") else {
            continue;
        };
        let mut step = pointer_get(&base, step_at)
            .cloned()
            .expect("the template's place step");
        let covering = format!("{step_at}/place_beacon/at/covering");
        if let (Json::Object(members), Some(value)) = (&mut step, op.get("value")) {
            members.retain(|(key, _)| key != "timeout_ms");
            members.push((String::from("timeout_ms"), value.clone()));
        }
        if let Some(named) = ops
            .iter()
            .find(|op| op.get("path").and_then(text) == Some(covering.as_str()))
            .and_then(|op| op.get("value"))
            && let Some(Json::Object(place)) = match &mut step {
                Json::Object(members) => members
                    .iter_mut()
                    .find(|(key, _)| key == "place_beacon")
                    .map(|(_, place)| place),
                _ => None,
            }
            && let Some((_, Json::Object(at))) = place.iter_mut().find(|(key, _)| key == "at")
        {
            at.clear();
            at.push((String::from("covering"), named.clone()));
        }
        steps.push(step);
    }
    steps
}

/// The worlds the timeout property is held over: a busy round, a far vent's
/// stone, a raid, a licensed core, the round after a commander's death (a
/// guard beside the goals) and the same with every goal gone (the guard
/// alone fills Defend the Core).
fn timeout_worlds() -> Vec<(&'static str, Script)> {
    let mut died = Script::busy(0);
    died.round = 2;
    died.feed_deaths = vec![0];
    let mut guard_alone = Script::new(0);
    let mut start = Feature::new("seam", "standard", [36, 34]);
    start.covered = true;
    guard_alone.features.push(start);
    guard_alone.round = 3;
    guard_alone.feed_deaths = vec![0];
    vec![
        ("busy", Script::busy(0)),
        ("far vent", far_vent()),
        ("raid", raid_world()),
        ("licensed", licensed()),
        ("died", died),
        ("guard alone", guard_alone),
    ]
}

/// No composed placement times out before its own price (fix pass 2, item
/// 1): a `place_beacon`'s timeout bounds its walk, and every one Easy composes
/// -- the template's own place step, a copy of it, a stone, a raid, a guard
/// post, a Survey post -- carries a timeout of at least its walk from the core
/// (the scripted estimate's leg), its on-site time as Easy prices it, and the
/// travel estimate's allowance.
#[test]
fn no_composed_placement_times_out_before_its_own_price() {
    let easy = Easy::new(&rules_json()).expect("the rules text reads");
    let allowance = walk_allowance_ms();
    assert_eq!(allowance, 12_800, "the committed repath cap's allowance");
    let mut checked = 0_usize;
    for (what, world) in timeout_worlds() {
        let (script, played) = play(world);
        assert_eq!(played.submitted, Submitted::Own, "{what}: {}", played.why);
        let core = script
            .beacons
            .iter()
            .find(|beacon| beacon.core && beacon.owner == script.seat)
            .map(|beacon| beacon.at)
            .expect("the seat's core");
        for step in composed_placements(&script.transcript) {
            let at = step
                .get("place_beacon")
                .and_then(|place| place.get("at"))
                .expect("a placement's location");
            let site = script.place(at).expect("the script answers the site");
            let walk =
                (i64::from(core[0] - site[0]).abs() + i64::from(core[1] - site[1]).abs()) * 500;
            let on_site = easy.on_site_ms(&step).expect("Easy prices the step");
            let timeout = match step.get("timeout_ms") {
                Some(Json::Number(lexeme)) => lexeme.parse::<i64>().expect("a timeout"),
                _ => panic!("{what}: a placement with no timeout: {}", compact(&step)),
            };
            eprintln!(
                "{what}: {} walk {walk} on site {on_site} timeout {timeout}",
                step.get("label").and_then(text).unwrap_or("?")
            );
            assert!(
                timeout >= walk + on_site + allowance,
                "{what}: {timeout} ms against a walk of {walk}, {on_site} on site and the \
                 {allowance} allowance: {}",
                compact(&step)
            );
            checked += 1;
        }
    }
    assert!(checked >= 6, "every world composed a placement: {checked}");
}

// ---------------------------------------------------------------------------
// Fix pass 2: one ranking
// ---------------------------------------------------------------------------

/// A world of the reviewer's probe (fix pass 2, item 3): a 60 s segment, so
/// a 42 s share; the starting seam covered beside the core; three stones
/// toward rich seams whose site is at `y`; and a near lean vent.
fn crowded(y: i32) -> Script {
    let mut world = Script::new(0);
    world.treasury = 1_000;
    world.segment_ms = 60_000;
    world.remaining_ms = 60_000;
    let mut start = Feature::new("seam", "standard", [36, 34]);
    start.covered = true;
    start.travel_ms = Some(3_000);
    world.features.push(start);
    for x in [2, 4, 6] {
        let mut seam = Feature::new("seam", "rich", [x, 63]);
        seam.site = None;
        seam.stone = Some([x, y, GROUND]);
        world.features.push(seam);
    }
    let mut vent = Feature::new("vent", "lean", [40, 32]);
    vent.site = Some([36, 32, GROUND]);
    world.features.push(vent);
    world
}

/// The top-k cuts against what is funded ahead of the ranking, not the whole
/// share (fix pass 2, item 3): in the round after the commander's death the
/// guard post is funded first, and stones that fit the 42 s share but not
/// what the guard leaves of it are no goals this round, so they never crowd
/// out the near vent, which fits: Easy composes the guard and the vent.
#[test]
fn goals_that_do_not_fit_what_the_guard_leaves_do_not_crowd_out_one_that_does() {
    for y in [44_i32, 48, 52, 56] {
        let mut world = crowded(y);
        world.round = 2;
        world.feed_deaths = vec![0];
        let (script, played) = play(world);
        let ops = compact(&Json::Array(composing_ops(&script.transcript)));
        assert!(
            ops.contains("place_guard"),
            "y {y}: {} -- {ops}",
            played.why
        );
        assert!(ops.contains("vent_40_32"), "y {y}: {} -- {ops}", played.why);
        assert!(!ops.contains("toward"), "y {y}: {} -- {ops}", played.why);
    }
}

/// The reviewer's probe itself (fix pass 2, items 2 and 3), a Survey post due
/// or not: the post is ranked with the goals, worth the raid it buys, so it
/// never takes the share ahead of them and the round is never the post alone.
/// Each round composes its best goal by utility per second that fits: with a
/// fresh sighting, the raid on it (and no post is due); else the stone where
/// its site is near enough to fit the share (its served Mine is worth more
/// per second than the lean vent's Generator), the vent where the stone no
/// longer fits.
#[test]
fn a_due_survey_post_never_crowds_out_the_goals() {
    for y in [40_i32, 44, 48, 52, 56, 60, 64, 68] {
        for fresh in [false, true] {
            let mut world = crowded(y);
            if fresh {
                world.beacons.push(Beacon {
                    id: String::from("e_01"),
                    at: [60, 60, GROUND],
                    owner: 1,
                    core: false,
                    powered: false,
                    priority: "",
                });
                world.sighting_ages.insert(String::from("e_01"), 0);
            }
            let (script, played) = play(world);
            let ops = compact(&Json::Array(composing_ops(&script.transcript)));
            eprintln!(
                "y {y} fresh {fresh}: vent {} stone {} raid {} survey {}",
                ops.contains("vent_40_32"),
                ops.contains("toward"),
                ops.contains("\"e_01\""),
                ops.contains("survey_post")
            );
            let goal = if fresh {
                ops.contains("\"e_01\"")
            } else {
                ops.contains("vent_40_32") || ops.contains("toward")
            };
            assert!(goal, "y {y}, fresh {fresh}: {} -- {ops}", played.why);
        }
    }
}

/// A goal placeable now outranks a stone toward a feature of the same worth
/// (fix pass 2, item 2): two rich seams, one with a site the seat's spheres
/// allow, a 20 s walk away, and one no site covers whose stone is an 8 s
/// walk. The stone's worth is its Mine's discounted by the stone and the two
/// rounds its chain takes, so the Mine placeable now is composed and the
/// stone is not, though its own walk is the shorter.
#[test]
fn a_goal_placeable_now_outranks_a_stone_toward_a_feature_of_the_same_worth() {
    let mut world = Script::new(0);
    world.treasury = 1_000;
    let mut near = Feature::new("seam", "rich", [56, 32]);
    near.site = Some([72, 32, GROUND]);
    world.features.push(near);
    let mut far = Feature::new("seam", "rich", [8, 32]);
    far.site = None;
    far.stone = Some([24, 32, GROUND]);
    world.features.push(far);
    let (script, played) = play(world);
    assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
    let ops = compact(&Json::Array(composing_ops(&script.transcript)));
    assert!(ops.contains("seam_56_32"), "{} -- {ops}", played.why);
    assert!(!ops.contains("toward"), "{} -- {ops}", played.why);
}

/// A chain once begun is continued (fix pass 2, item 2): the seat's stone
/// `b_02` stands north of its core, its sphere holding no feature, so it is
/// the stone toward the rich seam further north, whose gap to the seat's
/// spheres it closed by its whole radius. A second far rich seam east of the
/// core has a nearer stone site, and would rank first, but its gap the stone
/// closed not at all: only the begun chain's stone is weighed, and composed.
#[test]
fn a_chain_once_begun_is_continued_toward_its_feature() {
    let mut world = Script::new(0);
    world.treasury = 1_000;
    world.beacons.push(Beacon {
        id: String::from("b_02"),
        at: [32, 56, GROUND],
        owner: 0,
        core: false,
        powered: true,
        priority: "normal",
    });
    let mut north = Feature::new("seam", "rich", [32, 110]);
    north.site = None;
    north.stone = Some([32, 80, GROUND]);
    world.features.push(north);
    let mut east = Feature::new("seam", "rich", [80, 40]);
    east.site = None;
    east.stone = Some([56, 40, GROUND]);
    world.features.push(east);
    let (script, played) = play(world);
    assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
    assert_eq!(towards(&script.transcript, "seam_32_110"), 1);
    assert_eq!(towards(&script.transcript, "seam_80_40"), 0);
    let ops = compact(&Json::Array(composing_ops(&script.transcript)));
    assert!(
        ops.contains(r#""toward": { "feature": { "feature_id": "seam_32_110" } }"#),
        "{} -- {ops}",
        played.why
    );
}

/// A begun chain whose `toward` is refused is shown unreachable: the rest of
/// the far features are weighed, and the round reaches toward another.
#[test]
fn a_begun_chain_refused_its_stone_weighs_the_rest() {
    let mut world = Script::new(0);
    world.treasury = 1_000;
    world.beacons.push(Beacon {
        id: String::from("b_02"),
        at: [32, 56, GROUND],
        owner: 0,
        core: false,
        powered: true,
        priority: "normal",
    });
    let mut north = Feature::new("seam", "rich", [32, 110]);
    north.site = None;
    north.stone = None;
    world.features.push(north);
    let mut east = Feature::new("seam", "rich", [80, 40]);
    east.site = None;
    east.stone = Some([56, 40, GROUND]);
    world.features.push(east);
    let (script, played) = play(world);
    assert_eq!(towards(&script.transcript, "seam_32_110"), 1);
    assert_eq!(towards(&script.transcript, "seam_80_40"), 1);
    let ops = compact(&Json::Array(composing_ops(&script.transcript)));
    assert!(
        ops.contains(r#""toward": { "feature": { "feature_id": "seam_80_40" } }"#),
        "{} -- {ops}",
        played.why
    );
}

/// The Spire's time is its priced step's, paid out of the hold (fix pass 2,
/// item 6): the same Hold & Build round with and without the licence, its
/// goals unchanged, holds shorter by exactly the `queue_spire` step's price
/// as Easy prices it (`crates/gamectl/tests/pricing.rs` holds that price to
/// the verifier's).
#[test]
fn the_spire_takes_its_priced_time_out_of_the_hold() {
    let world = || {
        let mut world = Script::busy(0);
        world.features.retain(|feature| feature.id != "seam_20_60");
        world.survey_post = Some([34, 34, GROUND]);
        world.treasury = 10_000;
        world
    };
    let hold_of = |world: Script| {
        let (script, played) = play(world);
        assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
        let ops = composing_ops(&script.transcript);
        op_value(&ops, "replace", "/declarative/route/1/hold/ms")
            .and_then(|value| match value {
                Json::Number(n) => n.parse::<i64>().ok(),
                _ => None,
            })
            .expect("the hold, a number")
    };
    let unlicensed = hold_of(world());
    let mut licensed = world();
    licensed.licences = vec![SPIRE_LICENCE];
    let with_spire = hold_of(licensed);
    let easy = Easy::new(&rules_json()).expect("the rules text reads");
    let spire = easy
        .on_site_ms(&pharmakos_operator::steps::spire("b_00"))
        .expect("the Spire's step prices");
    assert!(spire > 0);
    assert_eq!(
        unlicensed - with_spire,
        spire,
        "{unlicensed} and {with_spire}"
    );
}

/// A begun chain whose stone no round's share carries is out of reach this
/// round, as a refused `toward` is: the rest of the far features are weighed,
/// and the round reaches toward another, rather than seal nothing of its own.
#[test]
fn a_begun_chain_whose_stone_does_not_fit_weighs_the_rest() {
    let mut world = Script::new(0);
    world.treasury = 1_000;
    world.segment_ms = 60_000;
    world.remaining_ms = 60_000;
    world.beacons.push(Beacon {
        id: String::from("b_02"),
        at: [32, 56, GROUND],
        owner: 0,
        core: false,
        powered: true,
        priority: "normal",
    });
    // The chain's stone: 62 voxels north of the core, 31 s of walk, which
    // with its deploy is past the 42 s share.
    let mut north = Feature::new("seam", "rich", [32, 110]);
    north.site = None;
    north.stone = Some([32, 94, GROUND]);
    world.features.push(north);
    let mut east = Feature::new("seam", "rich", [80, 40]);
    east.site = None;
    east.stone = Some([44, 40, GROUND]);
    world.features.push(east);
    let (script, played) = play(world);
    assert_eq!(towards(&script.transcript, "seam_32_110"), 1);
    assert_eq!(towards(&script.transcript, "seam_80_40"), 1);
    assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
    let ops = compact(&Json::Array(composing_ops(&script.transcript)));
    assert!(
        ops.contains(r#""toward": { "feature": { "feature_id": "seam_80_40" } }"#),
        "{} -- {ops}",
        played.why
    );
}

// ---------------------------------------------------------------------------
// Fix pass 3: exact rates
// ---------------------------------------------------------------------------

/// A vent at `[x, 32]` whose site is four voxels west of it, its travel the
/// scripted walk from the core to that site.
fn vent_at(grade: &'static str, x: i32) -> Feature {
    let mut vent = Feature::new("vent", grade, [x, 32]);
    vent.travel_ms = Some(i64::from((x - 4 - 32).abs()) * 500);
    vent.site = Some([x - 4, 32, GROUND]);
    vent
}

/// The review's probe (fix pass 3, item 2): a lean vent beside the core, 140
/// points in 17.5 s, and a standard vent 240 points in 29.5 s, with $160 for
/// one. The standard vent's rate is the higher (8.14 against 8.0 points a
/// second), and a rate divided down to whole points a second tied them at 8,
/// so the lean one went first on its lower id. Compared exactly, the standard
/// vent is funded.
#[test]
fn the_higher_rate_is_funded_however_close() {
    let mut world = Script::new(0);
    world.treasury = 160;
    world.features.push(vent_at("lean", 36));
    world.features.push(vent_at("standard", 60));
    let (script, played) = play(world);
    let ops = compact(&Json::Array(composing_ops(&script.transcript)));
    assert!(ops.contains("vent_60_32"), "{} -- {ops}", played.why);
    assert!(!ops.contains("vent_36_32"), "{} -- {ops}", played.why);
}

/// The funding order is the exact rate order (fix pass 3, item 2): over
/// generated worlds of vents at random grades and distances, with the
/// treasury for one, the vent composed is the one of the highest utility per
/// second by exact cross-multiplication of its points and its time, ties to
/// the lowest id (the nearest, then the name).
#[test]
fn the_funding_order_is_the_exact_rate_order() {
    let easy = Easy::new(&rules_json()).expect("the rules text reads");
    let mut seed: u64 = 0x0005_eed0;
    let mut next = |below: u64| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        i64::try_from((seed >> 33) % below).expect("small")
    };
    let mut checked = 0;
    for _ in 0..200 {
        let mut world = Script::new(0);
        world.treasury = 160;
        let mut vents: Vec<(i64, i64, String, i64)> = Vec::new();
        for _ in 0..3 {
            let x = i32::try_from(37 + next(28)).expect("a column");
            let (grade, points) = *[("lean", 140_i64), ("standard", 240), ("rich", 340)]
                .get(usize::try_from(next(3)).expect("an index"))
                .expect("a grade");
            let vent = vent_at(grade, x);
            if world.features.iter().any(|held| held.id == vent.id) {
                continue;
            }
            let walk = vent.travel_ms.expect("a travel");
            vents.push((points, walk, vent.id.clone(), walk));
            world.features.push(vent);
        }
        let (script, played) = play(world);
        if played.submitted != Submitted::Own {
            continue;
        }
        let ops = composing_ops(&script.transcript);
        let Some(on_site) = template("hold_and_build")
            .and_then(|(text, _)| read(&strip_comments(&text)).ok())
            .and_then(|body| pointer_get(&body, "/declarative/route/0").cloned())
            .and_then(|step| easy.on_site_ms(&step))
        else {
            panic!("Hold & Build's place step prices");
        };
        // The best by exact rate, ties to the nearest, then the name.
        let best = vents
            .iter()
            .max_by(|a, b| {
                let (ta, tb) = (i128::from(a.1 + on_site), i128::from(b.1 + on_site));
                (i128::from(a.0) * tb)
                    .cmp(&(i128::from(b.0) * ta))
                    .then_with(|| b.3.cmp(&a.3))
                    .then_with(|| b.2.cmp(&a.2))
            })
            .expect("a vent");
        let written = compact(&Json::Array(ops));
        assert!(
            written.contains(&best.2),
            "{vents:?}: {} -- {written}",
            played.why
        );
        checked += 1;
    }
    assert!(checked > 100, "{checked} worlds composed");
}

// ---------------------------------------------------------------------------
// Fix pass 3: the top-k cut and the segment fit, held by their own cases
// ---------------------------------------------------------------------------

/// The top-k is cut against what the guard leaves (fix pass 3, item 3): in
/// the round after the commander's death the guard is funded first and
/// leaves 23 s of the 42 s share. Three stones toward rich seams, each 27 s
/// of route, outrank the near lean vent and fit the whole share, but not
/// what the guard leaves; the vent, 17.5 s, does. Cut against the whole share
/// the stones would take the top-k and nothing past the guard would be
/// composed; cut against what the guard leaves, the vent is.
#[test]
fn stones_that_fit_the_share_but_not_what_the_guard_leaves_never_crowd_out_the_vent() {
    let mut world = Script::new(0);
    world.treasury = 1_000;
    world.segment_ms = 60_000;
    world.remaining_ms = 60_000;
    world.round = 2;
    world.feed_deaths = vec![0];
    let mut start = Feature::new("seam", "standard", [36, 34]);
    start.covered = true;
    start.travel_ms = Some(3_000);
    world.features.push(start);
    for x in [30, 32, 34] {
        let mut seam = Feature::new("seam", "rich", [x, 70]);
        seam.site = None;
        seam.stone = Some([x, 62, GROUND]);
        world.features.push(seam);
    }
    world.features.push(vent_at("lean", 36));
    let (script, played) = play(world);
    assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
    let ops = compact(&Json::Array(composing_ops(&script.transcript)));
    assert!(ops.contains("place_guard"), "{} -- {ops}", played.why);
    assert!(ops.contains("vent_36_32"), "{} -- {ops}", played.why);
    assert!(!ops.contains("toward"), "{} -- {ops}", played.why);
}

/// A step whose timeout is longer than the segment is not composed (fix pass
/// 3, item 4): in a 40 s segment, so a 28 s share, a vent 27.5 s of route
/// away fits the share, but its timeout (its walk, its on-site time and the
/// estimate's 12.8 s allowance, 40.3 s) is past the segment, so it could not
/// finish in the round and is not composed.
#[test]
fn a_step_whose_timeout_is_past_the_segment_is_not_composed() {
    let mut world = Script::new(0);
    world.treasury = 1_000;
    world.segment_ms = 40_000;
    world.remaining_ms = 40_000;
    world.features.push(vent_at("lean", 56));
    let (script, played) = play(world);
    let composed = script
        .transcript
        .iter()
        .any(|line| line.starts_with("patch_plan ") && line.contains("vent_56_32"));
    assert!(!composed, "{}", played.why);
    // The same vent in a segment long enough for its timeout is composed.
    let mut long = Script::new(0);
    long.treasury = 1_000;
    long.segment_ms = 41_000;
    long.remaining_ms = 41_000;
    long.features.push(vent_at("lean", 56));
    let (script, played) = play(long);
    let ops = compact(&Json::Array(composing_ops(&script.transcript)));
    assert!(ops.contains("vent_56_32"), "{} -- {ops}", played.why);
}

// ---------------------------------------------------------------------------
// Fix pass 3: a failed stone is noticed (fix pass 4: through the carried draft)
// ---------------------------------------------------------------------------

/// The `toward` failure line the gateway writes for a timed-out step.
fn timed_out() -> (&'static str, String) {
    (
        "step_failed",
        String::from(
            "A step failed (timeout): it ran past its timeout. Its on_fail decided what came next.",
        ),
    )
}

/// Round 2 of the far vent's world: last round's seal, which the gateway
/// carried into this Lull as the seat's draft `carried` stamped with round 2,
/// walked a stone toward `vent_56_32` as its route step 2, and the last
/// Push's feed says that step failed on `timeout`.
fn a_stone_failed_last_round() -> Script {
    let mut world = far_vent();
    world.round = 2;
    let carried = r#"{"schema_version": {"major": 1, "minor": 0}, "meta": {"title": "Hold & Build"},
        "kind": "PLAYBOOK", "declarative": {"route": [
        {"label": "deepen_core", "interface": {"beacon": {"beacon_id": "b_00"}, "rows": []}},
        // The stone, step 2.
        {"label": "stone", "place_beacon": {"at": {"toward": {"feature": {"feature_id": "vent_56_32"}}}}, "timeout_ms": 1000},
        {"label": "hold", "hold": {"ms": 1000}}]}}"#;
    world
        .drafts
        .insert(String::from(CARRIED_DRAFT_ID), (2, carried.to_owned()));
    world.feed_steps = vec![
        ("step_started", String::from("Step 1 started.")),
        ("step_completed", String::from("Step 1 completed.")),
        ("step_started", String::from("Step 2 started.")),
        timed_out(),
    ];
    world
}

/// A stone that failed last round is not weighed again unchanged (fix pass
/// 3, item 6): its chain ends this round as a refused `toward` ends it. The
/// last Push's feed counts a failed step of the seat's own, the carried
/// draft says that step was the stone toward `vent_56_32`, and this round
/// makes no `toward` estimate for it.
#[test]
fn a_stone_that_failed_last_round_is_not_weighed_again() {
    let (script, played) = play(a_stone_failed_last_round());
    assert_eq!(
        towards(&script.transcript, "vent_56_32"),
        0,
        "{}",
        played.why
    );
    assert_eq!(
        script
            .transcript
            .iter()
            .filter(|line| line.starts_with("get_draft"))
            .collect::<Vec<_>>(),
        [r#"get_draft { "draft_id": "carried" }"#],
    );
    assert_eq!(
        played.failed_stones,
        FailedStones::Known(vec![String::from("vent_56_32")])
    );
}

/// What does not say a stone failed leaves it weighed, and says which it is:
/// a failure inside a rule's body (its steps are numbered in the body) and a
/// step that completed are no failed stone; a carried draft stamped with
/// another round, no carried draft at all, a refused `get_draft` and a
/// carried seal that does not read are [`FailedStones::Unread`], each with
/// its cause, and never "nothing failed".
#[test]
fn only_a_failed_stone_of_last_rounds_seal_ends_its_chain() {
    let mut in_rule = a_stone_failed_last_round();
    in_rule.feed_steps = vec![
        ("step_started", String::from("Step 1 started.")),
        ("rule_fired", String::from("Rule 1 fired.")),
        ("step_started", String::from("Step 2 started.")),
        timed_out(),
        ("rule_ended", String::from("Rule 1 ended.")),
    ];
    let mut older = a_stone_failed_last_round();
    older.round = 3;
    let mut none_carried = a_stone_failed_last_round();
    none_carried.drafts.clear();
    let mut completed = a_stone_failed_last_round();
    completed.feed_steps = vec![
        ("step_started", String::from("Step 2 started.")),
        ("step_completed", String::from("Step 2 completed.")),
    ];
    let mut unreadable = a_stone_failed_last_round();
    unreadable.drafts.insert(
        String::from(CARRIED_DRAFT_ID),
        (2, String::from("{ not json")),
    );
    let mut past_route = a_stone_failed_last_round();
    past_route.feed_steps = vec![
        ("step_started", String::from("Step 9 started.")),
        timed_out(),
    ];
    for (what, world, expected) in [
        ("in a rule's body", in_rule, Expected::Nothing),
        ("completed", completed, Expected::Nothing),
        (
            "an older draft",
            older,
            Expected::Unread(StonesUnread::CarriedOfRound(2)),
        ),
        (
            "no carried draft",
            none_carried,
            Expected::Unread(StonesUnread::NoCarriedSeal),
        ),
        ("unreadable", unreadable, Expected::UnreadCarried),
        ("past the route", past_route, Expected::UnreadCarried),
    ] {
        let (script, played) = play(world);
        assert_eq!(
            towards(&script.transcript, "vent_56_32"),
            1,
            "{what}: {}",
            played.why
        );
        match expected {
            Expected::Nothing => assert_eq!(played.failed_stones, FailedStones::None, "{what}"),
            Expected::Unread(why) => {
                assert_eq!(
                    played.failed_stones,
                    FailedStones::Unread {
                        known: Vec::new(),
                        why
                    },
                    "{what}"
                );
                assert!(
                    played.why.contains("is not all known"),
                    "{what}: {}",
                    played.why
                );
            }
            Expected::UnreadCarried => {
                assert!(
                    matches!(
                        &played.failed_stones,
                        FailedStones::Unread {
                            why: StonesUnread::CarriedUnread(refused),
                            ..
                        } if refused.method == "get_draft" && refused.code == "MALFORMED"
                    ),
                    "{what}: {:?}",
                    played.failed_stones
                );
                assert!(
                    played.why.contains("is not all known"),
                    "{what}: {}",
                    played.why
                );
            }
        }
    }
}

/// A `get_draft` of the carried seal that the gateway refuses is
/// [`FailedStones::Unread`] with the refusal, and the stone is weighed.
#[test]
fn a_refused_carried_draft_is_unread_with_its_refusal() {
    let mut refused = a_stone_failed_last_round();
    refused.refuse_draft = Some("RATE_LIMITED");
    let (_, played) = play(refused);
    assert!(
        matches!(
            &played.failed_stones,
            FailedStones::Unread {
                why: StonesUnread::CarriedUnread(refused),
                ..
            } if refused.code == "RATE_LIMITED"
        ),
        "{:?}",
        played.failed_stones
    );
}

/// What [`only_a_failed_stone_of_last_rounds_seal_ends_its_chain`] expects of
/// a case.
enum Expected {
    /// No failed stone.
    Nothing,
    /// Not known, for this cause.
    Unread(StonesUnread),
    /// Not known: the carried seal refused as `MALFORMED`.
    UnreadCarried,
}

/// A built-in seat writes nothing but its own seal (fix pass 4, item 1): an
/// own seal and a safe one alike leave the seat's drafts as they were.
#[test]
fn a_built_in_seat_writes_no_draft() {
    let (script, played) = play(far_vent());
    assert_eq!(played.submitted, Submitted::Own, "{}", played.why);
    assert!(script.drafts.is_empty());
    let mut nothing = Script::new(0);
    nothing.treasury = 0;
    let (script, played) = play(nothing);
    assert_eq!(played.submitted, Submitted::Safe, "{}", played.why);
    assert!(script.drafts.is_empty());
    assert!(
        !script
            .transcript
            .iter()
            .any(|line| line.starts_with("save_draft")),
        "{:#?}",
        script.transcript
    );
}

/// A Lull after the first whose feed shows no Push, as a Lull resumed from a
/// save opens (fix pass 4, item 2), knows neither the last Push's deaths nor
/// its failed steps: both are the one typed state, no carried seal is read,
/// every stone is weighed, no guard is added for a death, and the "why" says
/// so. A first round, which had no Push, reads nothing as unread.
#[test]
fn a_lull_whose_feed_shows_no_push_reads_it_as_unread() {
    let mut resumed = a_stone_failed_last_round();
    resumed.push = Push::NotShown;
    resumed.feed_steps.clear();
    resumed.feed_deaths.clear();
    let (script, played) = play(resumed);
    assert_eq!(
        played.failed_stones,
        FailedStones::Unread {
            known: Vec::new(),
            why: StonesUnread::Push(PushUnread::Resumed),
        }
    );
    assert_eq!(
        towards(&script.transcript, "vent_56_32"),
        1,
        "{}",
        played.why
    );
    assert!(
        !script
            .transcript
            .iter()
            .any(|line| line.starts_with("get_draft")),
        "{:#?}",
        script.transcript
    );
    assert!(
        played.why.contains("not in this Lull's feed"),
        "{}",
        played.why
    );
    assert!(
        !composing_ops(&script.transcript)
            .iter()
            .any(|op| compact(op).contains("DEFEND")),
        "no guard for a death it cannot see"
    );
    let mut first = far_vent();
    first.push = Push::NotShown;
    let (_, played) = play(first);
    assert_eq!(played.failed_stones, FailedStones::None);
    assert!(
        !played.why.contains("not in this Lull's feed"),
        "{}",
        played.why
    );
}

/// Failures past the lines a round reads are the same typed state as a
/// resumed Lull's (fix pass 4, item 3): the digest counts two failed steps
/// of the seat's own and the pages serve one, the stone it names is left
/// out, and the round says the rest were not read, never "only one failed".
#[test]
fn failures_past_the_pages_read_are_unread_with_the_ones_seen() {
    let mut world = a_stone_failed_last_round();
    world.unserved_failures = 1;
    let (script, played) = play(world);
    assert_eq!(
        played.failed_stones,
        FailedStones::Unread {
            known: vec![String::from("vent_56_32")],
            why: StonesUnread::Push(PushUnread::PastPages),
        }
    );
    assert_eq!(
        towards(&script.transcript, "vent_56_32"),
        0,
        "{}",
        played.why
    );
    assert!(
        played
            .why
            .contains(&format!("not all read within {PAGES_MAX} pages")),
        "{}",
        played.why
    );
    // Paged to the bound, no line is reached at all: nothing is known, and
    // it is said.
    let mut bound = a_stone_failed_last_round();
    bound.paging = Paging::ToTheBound;
    let (_, played) = play(bound);
    assert_eq!(
        played.failed_stones,
        FailedStones::Unread {
            known: Vec::new(),
            why: StonesUnread::Push(PushUnread::PastPages),
        }
    );
}

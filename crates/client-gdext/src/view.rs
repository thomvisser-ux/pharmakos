// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The view feed, decoded: a `get_view` result in, chunks on the drain queue out.
//!
//! Terrain and entities reach the client from ONE gateway method, `get_view` (skeleton
//! plan T16a; decisions-log item 107 (5)). An empty cursor asks for a keyframe, which is the
//! whole generated map (item 107 (1), confirmed as item 108 (1)); any other cursor asks what
//! changed for this viewer since the view the cursor came from. This module is the client's
//! half of that contract, and it is **marshalling**:
//!
//! 1. the result text is read against the schema (`gp.api.v1.GetViewResponse`), with the
//!    `_status` footer split off first because the footer is an envelope concern and not a
//!    field of the message;
//! 2. each chunk's `voxels_rle` is decoded by `pharmakos_proto::chunk_rle` — the one codec,
//!    shared with the producer — and refused whole if its runs do not sum to a chunk;
//! 3. each material byte is looked up in the wire's table ([`palette_of`]) and a value the
//!    table does not know is drawn as **nothing**, which is what `ViewChunk`'s comment
//!    requires of every decoder;
//! 4. the chunk is transposed from the sim's voxel order into the mesher's by
//!    [`crate::chunks`], and stored under its **origin**, which is its key;
//! 5. the light is baked from the materials ([`LightField`], the mesher's own bake), and the
//!    chunks whose surfaces changed go on item 54's [`DrainQueue`].
//!
//! # What the client does not do
//!
//! **No fog logic.** A chunk the seat may not see change is simply never re-sent, and the
//! client keeps its copy; a chunk that is re-sent replaces the copy. There is no reveal
//! toggle and no "last known" state here, because the server decides what each viewer is
//! entitled to (item 107 (5)) and the full-map unlock at elimination or match end is the
//! server's policy change, arriving as ordinary deltas under the same token.
//!
//! **No entity bookkeeping.** `entities` on a completing page is the complete visible list,
//! so an id absent from it is gone; the model replaces its list rather than merging.
//!
//! # The chunk set and the dirty set
//!
//! `bridge.rs` promised (T12) that "the chunk set arrives with the watch rig". It is exactly
//! `GetViewResponse.chunks`: the keyframe establishes the grid and every chunk in it, and a
//! delta's chunks are the dirty set — rebaked where the light changed and queued for the
//! drain. The six neighbour borders T12 wrote and did not wire are wired here, through the
//! mesher's own [`ChunkBorders`], because a whole map is now resident.

use std::collections::{BTreeMap, BTreeSet};

use pharmakos_mesher::{
    AIR, CHUNK_EDGE, CHUNK_VOLUME, ChunkBorders, ChunkGrid, DrainBudget, DrainQueue, LightField,
    LightParams, MeshBuffers, Mesher, chunk_view, gpu_surface_bytes,
};
use pharmakos_proto::chunk_rle;
use pharmakos_proto::gp::api::v1::{GetViewResponse, Status, view_entity};
use pharmakos_proto::json::{self, Json};

use crate::chunks;
use crate::enums;
use crate::error::BridgeError;

/// The key of the envelope footer every gateway result carries (spec section 12,
/// "Budgets"). A leading underscore is not a legal Protobuf identifier, which is exactly why
/// the footer is split off before the result is read against its message.
pub const STATUS_KEY: &str = "_status";

/// The mesher's palette id for a wire material byte.
///
/// The wire's table is `ViewChunk`'s comment in `gateway.proto`: 0 air, 1 dirt, 2 stone,
/// 3 to 5 the scrap seam lean, standard and rich, 6 to 8 the heat vent lean, standard and
/// rich. The mesher's palette is its own art table (`pharmakos_mesher::PALETTE`): 1 stone,
/// 2 dirt, 3 grass, 4 ore A, 5 ore B, 6 concrete. So the two disagree about what 1 and 2
/// mean, and the palette has no row for a vent's richness at all — this lookup is where the
/// wire's meaning meets the art's colours, and it decides nothing about the world.
///
/// **A value this table does not know draws nothing** ([`AIR`]), which is what
/// `ViewChunk`'s comment requires of every decoder and what lets a material be added to
/// the wire without breaking this client.
///
/// PLACEHOLDER: the seam and vent colours, and whether richness is visible at all. Art,
/// like the mesher's palette itself; OWNER, at S6's art pass, when the palette grows the
/// rows these three share today.
#[must_use]
pub const fn palette_of(wire: u8) -> u8 {
    match wire {
        1 => 2,     // dirt
        2 => 1,     // stone
        3..=5 => 4, // scrap seam, lean to rich: the palette's ore A
        6..=8 => 5, // heat vent, lean to rich: the palette's ore B
        _ => AIR,   // air, and every value the wire's table does not name
    }
}

/// What kind of thing an entity is, as `ViewEntity.Kind` names it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum EntityKind {
    /// A unit; the commander is a unit whose subtype is `commander`.
    Unit,
    /// A beacon, whose id is `b_NN` and which `get_beacon` takes.
    Beacon,
    /// A structure.
    Structure,
    /// A kind this build does not know. Drawn as a generic marker rather than dropped: the
    /// server decided the viewer may see it.
    Unknown,
}

impl EntityKind {
    /// The lower-case name GDScript reads.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Beacon => "beacon",
            Self::Structure => "structure",
            Self::Unknown => "unknown",
        }
    }
}

/// One thing standing on the terrain, as the viewer may see it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entity {
    /// Opaque and viewer-scoped. Never parsed here.
    pub id: String,
    /// Unit, beacon or structure.
    pub kind: EntityKind,
    /// `commander`, `build_drone`, `generator`; empty when the build has no name for it.
    pub subtype: String,
    /// `seat.0`, or empty for nobody.
    pub owner: String,
    /// Whole voxels, in the SIM's axes: x east, y north, z up.
    pub at: [i32; 3],
}

/// One chunk of a page, decoded and ready to store.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PageChunk {
    /// The chunk's lowest voxel, in the sim's axes — the chunk's key.
    pub origin: [i32; 3],
    /// The chunk's materials as palette ids, in the MESHER's voxel order.
    pub materials: Vec<u8>,
}

/// One `get_view` page, decoded.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Page {
    /// Game milliseconds since the start of the segment; zero in a Lull.
    pub at_ms: i32,
    /// The page's chunks, in the order the gateway sent them.
    pub chunks: Vec<PageChunk>,
    /// The complete visible list, on a completing page; empty on any other.
    pub entities: Vec<Entity>,
    /// Where to read from next.
    pub next_cursor: String,
    /// Whether this is the last page of the view.
    pub complete: bool,
}

/// Splits the `_status` footer off a result object.
///
/// Returns the result without its footer, and the footer when there was one. A value that
/// is not an object comes back unchanged with no footer, and the schema read that follows
/// refuses it with a pointer.
#[must_use]
pub fn split_footer(result: &Json) -> (Json, Option<Json>) {
    match result {
        Json::Object(entries) => {
            let mut body: Vec<(String, Json)> = Vec::with_capacity(entries.len());
            let mut footer = None;
            for (key, value) in entries {
                if key == STATUS_KEY {
                    footer = Some(value.clone());
                } else {
                    body.push((key.clone(), value.clone()));
                }
            }
            (Json::Object(body), footer)
        }
        other => (other.clone(), None),
    }
}

/// The `_status` footer, read against `gp.api.v1.Status`.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the footer is not a `Status`.
pub fn read_status(footer: &Json) -> Result<Status, BridgeError> {
    Ok(json::decode_json::<Status>(&enums::canonical(
        "gp.api.v1.Status",
        footer,
    ))?)
}

/// Decodes one `get_view` result: the object the JSON-RPC `result` member holds, footer
/// and all.
///
/// # Errors
///
/// [`BridgeError::Schema`] when the result is not a `GetViewResponse`, and
/// [`BridgeError::View`] when a chunk has no origin, an origin that is not on the chunk
/// grid, or a run list `chunk_rle` refuses. A refused chunk refuses the page: a page drawn
/// with a hole in it is indistinguishable from a map with one.
pub fn decode_page(result: &Json) -> Result<Page, BridgeError> {
    let (body, _) = split_footer(result);
    let response: GetViewResponse =
        json::decode_json(&enums::canonical("gp.api.v1.GetViewResponse", &body))?;
    let mut decoded: Vec<PageChunk> = Vec::with_capacity(response.chunks.len());
    let mut sim_order: Vec<u8> = Vec::with_capacity(CHUNK_VOLUME);
    for chunk in &response.chunks {
        let origin = chunk
            .origin
            .as_ref()
            .map(|voxel| [voxel.x, voxel.y, voxel.z])
            .ok_or_else(|| BridgeError::View("a chunk arrived with no origin".to_owned()))?;
        if chunk_coords_of(origin).is_none() {
            return Err(BridgeError::View(format!(
                "a chunk's origin {origin:?} is not a corner of the 32-voxel chunk grid"
            )));
        }
        let wire = chunk_rle::decode(&chunk.voxels_rle).map_err(|error| {
            BridgeError::View(format!("the chunk at {origin:?} did not decode: {error}"))
        })?;
        sim_order.clear();
        sim_order.extend(wire.iter().map(|byte| palette_of(*byte)));
        let mut materials = Vec::with_capacity(CHUNK_VOLUME);
        chunks::sim_to_mesher_chunk("chunk materials", &sim_order, &mut materials)?;
        decoded.push(PageChunk { origin, materials });
    }
    let entities = response
        .entities
        .iter()
        .map(|entity| Entity {
            id: entity.id.clone(),
            kind: match view_entity::Kind::try_from(entity.kind) {
                Ok(view_entity::Kind::Unit) => EntityKind::Unit,
                Ok(view_entity::Kind::Beacon) => EntityKind::Beacon,
                Ok(view_entity::Kind::Structure) => EntityKind::Structure,
                _ => EntityKind::Unknown,
            },
            subtype: entity.subtype.clone(),
            owner: entity.owner.clone(),
            at: entity
                .at
                .as_ref()
                .map_or([0, 0, 0], |voxel| [voxel.x, voxel.y, voxel.z]),
        })
        .collect();
    Ok(Page {
        at_ms: response.at_ms,
        chunks: decoded,
        entities,
        next_cursor: response.next_cursor,
        complete: response.complete,
    })
}

/// Decodes one `get_view` result from its text.
///
/// # Errors
///
/// As [`decode_page`], plus [`BridgeError::Schema`] when the text is not JSON.
pub fn decode_page_text(text: &str) -> Result<Page, BridgeError> {
    decode_page(&json::read(text)?)
}

/// A sim-axes chunk origin as the mesher's chunk coordinates.
///
/// The sim's y is north and its z is up; the mesher's y is up and its z is north
/// ([`crate::chunks`]). `None` for an origin that is negative or not a multiple of the chunk
/// edge.
#[must_use]
pub fn chunk_coords_of(origin: [i32; 3]) -> Option<[i32; 3]> {
    let edge = i32::try_from(CHUNK_EDGE).ok()?;
    let mut out = [0_i32; 3];
    for (slot, value) in out.iter_mut().zip(origin) {
        if value < 0 || value.checked_rem(edge)? != 0 {
            return None;
        }
        *slot = value.checked_div(edge)?;
    }
    let [east, north, up] = out;
    Some([east, up, north])
}

/// How many chunks one completing view may change before the light is baked afresh over the
/// whole map instead of box by box.
///
/// Not a rule and not a tuning value: a cost cut with no effect on any pixel. A reconnect's
/// keyframe or the full-map unlock re-sends many chunks at once, and a box rebake per chunk
/// would redo overlapping work hundreds of times; above this count one whole bake is
/// cheaper. Both paths produce the same light field, which
/// `a_box_rebake_and_a_whole_bake_agree` in this module's tests checks.
pub const WHOLE_BAKE_ABOVE: usize = 16;

/// The whole map as this client holds it, once a keyframe has established it.
#[derive(Debug)]
struct MapState {
    grid: ChunkGrid,
    /// Chunk-major, mesher order, palette ids — the layout [`LightField`] and
    /// [`chunk_view`] borrow.
    materials: Vec<u8>,
    light: LightField,
    queue: DrainQueue,
    borders: ChunkBorders,
    /// The last surface byte size per chunk, for the drain's byte budget.
    sizes: Vec<usize>,
}

/// What one page did to the model.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Applied {
    /// Whether the view is complete; `false` while more pages are waiting.
    pub complete: bool,
    /// The cursor to read from next.
    pub next_cursor: String,
    /// The page's `at_ms`.
    pub at_ms: i32,
    /// How many chunks the page carried.
    pub chunks: usize,
    /// How many chunks went onto the drain queue because of it.
    pub queued: usize,
    /// Whether the chunk grid was established or re-established by it — the renderer is
    /// rebuilt when it is.
    pub grid_changed: bool,
}

/// The client's copy of the view: every chunk it was sent, the light baked from them, the
/// drain queue, and the latest entity list.
#[derive(Debug)]
pub struct ViewModel {
    params: LightParams,
    budget: DrainBudget,
    /// Every chunk ever received, keyed by its sim-axes origin, in mesher order.
    held: BTreeMap<[i32; 3], Vec<u8>>,
    /// Origins whose bytes changed since the last completing page.
    changed: BTreeSet<[i32; 3]>,
    map: Option<MapState>,
    entities: Vec<Entity>,
    /// Drain calls so far: the drain queue's frame counter.
    drains: u64,
}

impl ViewModel {
    /// An empty model that will bake with `params` and drain under `budget` — both read
    /// from the rules table by [`crate::rules`].
    #[must_use]
    pub fn new(params: LightParams, budget: DrainBudget) -> Self {
        Self {
            params,
            budget,
            held: BTreeMap::new(),
            changed: BTreeSet::new(),
            map: None,
            entities: Vec::new(),
            drains: 0,
        }
    }

    /// The chunk grid, once a keyframe has established it.
    #[must_use]
    pub fn grid(&self) -> Option<ChunkGrid> {
        self.map.as_ref().map(|map| map.grid)
    }

    /// How many chunks are waiting on the drain queue.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.map.as_ref().map_or(0, |map| map.queue.len())
    }

    /// The latest complete entity list.
    #[must_use]
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// How many chunks this client holds.
    #[must_use]
    pub fn held(&self) -> usize {
        self.held.len()
    }

    /// Stores one page, and on a completing page bakes and queues what changed.
    ///
    /// # Errors
    ///
    /// [`BridgeError::View`] when the grid cannot be built from the chunks held, or the
    /// light bake refuses the map.
    pub fn apply(&mut self, page: Page) -> Result<Applied, BridgeError> {
        let chunks = page.chunks.len();
        for chunk in page.chunks {
            if self.held.get(&chunk.origin) != Some(&chunk.materials) {
                self.changed.insert(chunk.origin);
                self.held.insert(chunk.origin, chunk.materials);
            }
        }
        let mut queued = 0;
        let mut grid_changed = false;
        if page.complete {
            self.entities = page.entities;
            let outside = self.map.as_ref().is_some_and(|map| {
                self.changed
                    .iter()
                    .any(|origin| index_of(map.grid, *origin).is_none())
            });
            if self.map.is_none() || outside {
                self.build()?;
                grid_changed = true;
                queued = self.pending();
            } else {
                queued = self.rebake_changed()?;
            }
            self.changed.clear();
        }
        Ok(Applied {
            complete: page.complete,
            next_cursor: page.next_cursor,
            at_ms: page.at_ms,
            chunks,
            queued,
            grid_changed,
        })
    }

    /// Builds the grid from every chunk held, bakes the whole map and queues every chunk.
    fn build(&mut self) -> Result<(), BridgeError> {
        let mut extent = [0_i32; 3];
        for origin in self.held.keys() {
            let coords = chunk_coords_of(*origin)
                .ok_or_else(|| BridgeError::View(format!("{origin:?} is not a chunk origin")))?;
            for (slot, value) in extent.iter_mut().zip(coords) {
                *slot = (*slot).max(value.saturating_add(1));
            }
        }
        let dims = extent.map(|value| u32::try_from(value).unwrap_or(0));
        let grid = ChunkGrid::new(dims[0], dims[1], dims[2])
            .map_err(|error| BridgeError::View(format!("no chunk grid: {error}")))?;
        let mut materials = vec![AIR; grid.voxel_count()];
        for (origin, bytes) in &self.held {
            if let Some(slot) = slice_of(grid, &mut materials, *origin) {
                slot.copy_from_slice(bytes);
            }
        }
        let mut light = LightField::new(grid, self.params);
        light
            .bake_all(&materials)
            .map_err(|error| BridgeError::View(format!("the light bake refused: {error}")))?;
        let mut queue = DrainQueue::new(grid, self.budget);
        for chunk in 0..grid.chunk_count() {
            queue.push(chunk, self.drains);
        }
        let count = usize::try_from(grid.chunk_count()).unwrap_or(0);
        self.map = Some(MapState {
            grid,
            materials,
            light,
            queue,
            borders: ChunkBorders::new(),
            sizes: vec![0; count],
        });
        Ok(())
    }

    /// Writes the changed chunks into the map, rebakes their light and queues what moved.
    fn rebake_changed(&mut self) -> Result<usize, BridgeError> {
        let Some(map) = self.map.as_mut() else {
            return Ok(0);
        };
        if self.changed.is_empty() {
            return Ok(0);
        }
        let before = map.queue.len();
        let mut boxes: Vec<([i32; 3], [i32; 3])> = Vec::with_capacity(self.changed.len());
        for origin in &self.changed {
            let (Some(bytes), Some(slot)) = (
                self.held.get(origin),
                slice_of(map.grid, &mut map.materials, *origin),
            ) else {
                continue;
            };
            slot.copy_from_slice(bytes);
            if let Some(coords) = chunk_coords_of(*origin) {
                let edge = i32::try_from(CHUNK_EDGE).unwrap_or(32);
                let lo = coords.map(|value| value.saturating_mul(edge));
                let hi = lo.map(|value| value.saturating_add(edge.saturating_sub(1)));
                boxes.push((lo, hi));
            }
        }
        let refused = |error: pharmakos_mesher::LightError| {
            BridgeError::View(format!("the light bake refused: {error}"))
        };
        if boxes.len() > WHOLE_BAKE_ABOVE {
            map.light.bake_all(&map.materials).map_err(refused)?;
            for chunk in 0..map.grid.chunk_count() {
                map.queue.push(chunk, self.drains);
            }
        } else {
            let mut dirty: Vec<u32> = Vec::new();
            for (lo, hi) in &boxes {
                map.light
                    .rebake_box(&map.materials, *lo, *hi)
                    .map_err(refused)?;
                map.light.dirty_chunks(*lo, *hi, &mut dirty);
            }
            for chunk in dirty {
                map.queue.push(chunk, self.drains);
            }
        }
        Ok(map.queue.len().saturating_sub(before))
    }

    /// This drain's chunks, in item 54's order, under the rules table's K and B.
    ///
    /// `camera_chunk` is the camera's chunk in the MESHER's axes. Each call is one drain
    /// "frame" to the queue, so a chunk's age counts drains, which is what the ageing term
    /// was defined over.
    pub fn next_uploads(&mut self, camera_chunk: [i32; 3]) -> Vec<u32> {
        self.drains = self.drains.saturating_add(1);
        let drains = self.drains;
        let Some(map) = self.map.as_mut() else {
            return Vec::new();
        };
        let sizes = &map.sizes;
        map.queue.drain(drains, camera_chunk, |chunk| {
            usize::try_from(chunk)
                .ok()
                .and_then(|at| sizes.get(at).copied())
                .unwrap_or(0)
        })
    }

    /// Meshes one chunk of the map with its six real neighbours and its baked light.
    ///
    /// Returns the chunk's corner in the MESHER's axes, in voxels, which is where its
    /// instance is placed.
    ///
    /// # Errors
    ///
    /// [`BridgeError::View`] before a keyframe, [`BridgeError::Chunk`] for an index outside
    /// the grid, and whatever the mesher refuses.
    pub fn mesh(
        &mut self,
        chunk: u32,
        mesher: &mut Mesher,
        buffers: &mut MeshBuffers,
    ) -> Result<[i32; 3], BridgeError> {
        let Some(map) = self.map.as_mut() else {
            return Err(BridgeError::View("no keyframe has arrived yet".to_owned()));
        };
        let coords = map
            .grid
            .chunk_coords(chunk)
            .ok_or_else(|| BridgeError::View(format!("chunk {chunk} is outside the map")))?;
        map.borders.gather(
            map.grid,
            &map.materials,
            map.light.all(),
            self.params,
            chunk,
        );
        let view = chunk_view(&map.materials, &map.light, &map.borders, chunk)?;
        mesher.mesh_chunk_into(&view, buffers)?;
        if let Some(size) = usize::try_from(chunk)
            .ok()
            .and_then(|at| map.sizes.get_mut(at))
        {
            *size = gpu_surface_bytes(buffers);
        }
        let edge = i32::try_from(CHUNK_EDGE).unwrap_or(32);
        Ok(coords.map(|value| value.saturating_mul(edge)))
    }
}

/// The mesher chunk index of a sim-axes origin, when it is inside `grid`.
fn index_of(grid: ChunkGrid, origin: [i32; 3]) -> Option<u32> {
    grid.chunk_index(chunk_coords_of(origin)?)
}

/// The slice of a chunk-major map that holds the chunk at `origin`.
fn slice_of(grid: ChunkGrid, materials: &mut [u8], origin: [i32; 3]) -> Option<&mut [u8]> {
    let index = usize::try_from(index_of(grid, origin)?).ok()?;
    let from = index.checked_mul(CHUNK_VOLUME)?;
    materials.get_mut(from..from.checked_add(CHUNK_VOLUME)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pharmakos_proto::gp::api::v1::{ViewChunk, ViewEntity};
    use pharmakos_proto::gp::v1::Voxel;

    fn params() -> LightParams {
        LightParams::new(15, 1).expect("the committed pair")
    }

    fn budget() -> DrainBudget {
        DrainBudget {
            surfaces_per_frame: 4,
            bytes_per_frame: 512 * 1024,
            age_frames: 2,
        }
    }

    /// A chunk in SIM order: a floor `depth` voxels deep of `material`.
    fn floor(depth: usize, material: u8) -> Vec<u8> {
        let mut out = vec![0_u8; CHUNK_VOLUME];
        for (index, slot) in out.iter_mut().enumerate() {
            if index.checked_div(1024).unwrap_or(0) < depth {
                *slot = material;
            }
        }
        out
    }

    fn result(chunks: &[([i32; 3], Vec<u8>)], complete: bool) -> Json {
        let response = GetViewResponse {
            at_ms: 0,
            chunks: chunks
                .iter()
                .map(|(origin, sim)| ViewChunk {
                    origin: Some(Voxel {
                        x: origin[0],
                        y: origin[1],
                        z: origin[2],
                    }),
                    voxels_rle: chunk_rle::encode(sim),
                })
                .collect(),
            entities: vec![ViewEntity {
                id: "u_1".to_owned(),
                kind: view_entity::Kind::Unit.into(),
                subtype: "commander".to_owned(),
                owner: "seat.0".to_owned(),
                at: Some(Voxel { x: 3, y: 4, z: 5 }),
            }],
            next_cursor: "c".to_owned(),
            complete,
        };
        let mut value = json::encode_json(&response).expect("encodes");
        if let Json::Object(entries) = &mut value {
            entries.push((
                STATUS_KEY.to_owned(),
                json::read(r#"{"phase":"lull","round":1}"#).expect("json"),
            ));
        }
        value
    }

    #[test]
    fn the_wire_table_maps_every_known_value_and_draws_nothing_for_the_rest() {
        assert_eq!(palette_of(0), AIR);
        for known in 1..=8_u8 {
            assert_ne!(
                palette_of(known),
                AIR,
                "wire material {known} is in the table"
            );
        }
        for unknown in 9..=255_u8 {
            assert_eq!(palette_of(unknown), AIR, "{unknown} is not in the table");
        }
    }

    #[test]
    fn the_footer_is_split_off_and_read_as_a_status() {
        let value = result(&[([0, 0, 0], floor(4, 2))], true);
        let (body, footer) = split_footer(&value);
        assert!(body.get(STATUS_KEY).is_none());
        let status = read_status(&footer.expect("a footer")).expect("a Status");
        assert_eq!(status.round, 1);
    }

    #[test]
    fn a_page_decodes_with_the_unknown_value_drawn_as_air() {
        let mut sim = floor(4, 2);
        if let Some(slot) = sim.get_mut(0) {
            *slot = 200;
        }
        let page = decode_page(&result(&[([32, 0, 0], sim)], true)).expect("decodes");
        let chunk = page.chunks.first().expect("one chunk");
        assert_eq!(chunk.origin, [32, 0, 0]);
        assert_eq!(
            chunk.materials.first().copied(),
            Some(AIR),
            "200 draws nothing"
        );
        assert_eq!(chunk.materials.get(1).copied(), Some(palette_of(2)));
        assert_eq!(page.entities.len(), 1);
        assert_eq!(
            page.entities.first().map(|entity| entity.kind),
            Some(EntityKind::Unit)
        );
    }

    #[test]
    fn a_chunk_off_the_grid_or_short_is_refused_not_drawn() {
        let error = decode_page(&result(&[([5, 0, 0], floor(1, 1))], true))
            .expect_err("5 is not a chunk corner");
        assert!(matches!(error, BridgeError::View(_)), "{error}");
        let error = decode_page(&result(&[([0, 0, 0], vec![1_u8; 100])], true))
            .expect_err("a short run list");
        assert!(matches!(error, BridgeError::View(_)), "{error}");
    }

    #[test]
    fn a_keyframe_builds_the_grid_and_queues_every_chunk() {
        let mut model = ViewModel::new(params(), budget());
        let first = decode_page(&result(&[([0, 0, 0], floor(4, 2))], false)).expect("page 1");
        let applied = model.apply(first).expect("applies");
        assert!(!applied.complete);
        assert_eq!(
            model.grid(),
            None,
            "nothing is built before the view completes"
        );
        let second = decode_page(&result(&[([32, 32, 0], floor(4, 2))], true)).expect("page 2");
        let applied = model.apply(second).expect("applies");
        assert!(applied.grid_changed);
        let grid = model.grid().expect("a grid");
        assert_eq!(
            (grid.chunks_x(), grid.chunks_y(), grid.chunks_z()),
            (2, 1, 2),
            "sim north is the mesher's z"
        );
        assert_eq!(model.pending(), 4, "every chunk of the grid is queued");
        assert_eq!(model.entities().len(), 1);
    }

    #[test]
    fn a_delta_that_changes_nothing_queues_nothing_and_one_that_does_queues_its_fan_out() {
        let mut model = ViewModel::new(params(), budget());
        let keyframe = [([0, 0, 0], floor(4, 2)), ([32, 0, 0], floor(4, 2))];
        model
            .apply(decode_page(&result(&keyframe, true)).expect("keyframe"))
            .expect("applies");
        while model.pending() > 0 {
            let _ = model.next_uploads([0, 0, 0]);
        }
        let same = decode_page(&result(&keyframe, true)).expect("the same view again");
        assert_eq!(model.apply(same).expect("applies").queued, 0);

        let edited = decode_page(&result(&[([32, 0, 0], floor(3, 2))], true)).expect("delta");
        let applied = model.apply(edited).expect("applies");
        assert!(!applied.grid_changed);
        assert!(
            applied.queued >= 1,
            "the edited chunk is queued: {applied:?}"
        );
    }

    #[test]
    fn a_box_rebake_and_a_whole_bake_agree() {
        let keyframe = [([0, 0, 0], floor(6, 2)), ([32, 0, 0], floor(6, 2))];
        let mut boxed = ViewModel::new(params(), budget());
        boxed
            .apply(decode_page(&result(&keyframe, true)).expect("keyframe"))
            .expect("applies");
        boxed
            .apply(decode_page(&result(&[([32, 0, 0], floor(2, 1))], true)).expect("delta"))
            .expect("applies");

        let mut whole = ViewModel::new(params(), budget());
        whole
            .apply(
                decode_page(&result(
                    &[([0, 0, 0], floor(6, 2)), ([32, 0, 0], floor(2, 1))],
                    true,
                ))
                .expect("keyframe"),
            )
            .expect("applies");

        let light = |model: &ViewModel| {
            model
                .map
                .as_ref()
                .map(|map| map.light.all().to_vec())
                .unwrap_or_default()
        };
        assert_eq!(light(&boxed), light(&whole));
    }
}

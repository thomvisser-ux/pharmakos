// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The view feed: derived, unhashed state behind `get_view`.
//!
//! Spec section 15 makes the Godot client a JSON-RPC client of this gateway,
//! and spec section 13 gives it a camera. Neither says how the terrain gets
//! there, and before T16a nothing did (decisions-log item 106 (1)): the sim
//! exposes a borrowable chunk view, and `client-gdext` may not depend on the
//! sim. So the terrain travels down the one wire that already exists, through
//! the one fog policy that already exists, and this module is what keeps that
//! affordable.
//!
//! # Nothing in here is hashed, and nothing in here is a tick
//!
//! Every field below is **derived** from the world and the fog policy. None of
//! it enters the state hash, none of it enters a snapshot, and a restore
//! rebuilds it from a pristine world (T17: [`ViewFeed::attach`] is what a
//! restore calls, every cursor then goes stale, and every entity handle is
//! re-minted).
//!
//! The stamp is the feed's own [`ViewFeed::seq`], bumped once per stepped
//! tick and once per attach, phase change and fog-policy change. It is
//! deliberately **not** a sim tick: a Lull and a recap consume none, and a
//! seat's view changes in both -- a beacon it may see comes into a sphere, a
//! seat is eliminated and the map opens up. A tick-stamped feed would answer
//! "nothing changed" to every one of those.
//!
//! # The delta rule is about WIRE BYTES, and that is what makes it leak-proof
//!
//! For each viewer the feed keeps, **for modified chunks only**, the bytes
//! that viewer is entitled to -- the current byte where the viewer is unfogged
//! or its [`Vision`] reaches that voxel, and the **generated** byte everywhere
//! else -- and the stamp at which those bytes last changed. A chunk is in a
//! delta *if and only if its bytes for that viewer changed*.
//!
//! Three things follow, and they are the whole design:
//!
//! * an enemy edit one voxel outside a seat's spheres, inside a chunk the seat
//!   half sees, produces **no chunk at all** -- not an empty one, not a
//!   smaller one, nothing. A delta whose mere presence said "something
//!   happened over there" would be a fog leak by arithmetic, which is the same
//!   mistake [`crate::feed`]'s cursor exists to avoid;
//! * the full-map unlock is not a feature. A seat is eliminated or the match
//!   ends, [`crate::fog::FogPolicy`] says the seat is unfogged, every modified
//!   chunk's entitled bytes change at once and page out on the next call. Only
//!   the *modified* ones: the generated map was delivered at the keyframe;
//! * a chunk nobody has ever written is the generated chunk for everybody, so
//!   it is encoded once, at attach, and served from there.
//!
//! # The generated map is served whole (decisions-log item 107 (1))
//!
//! What is fogged is what *changes* the terrain and what *stands on* it, voxel
//! by voxel. The generated map itself is not: `main` already serves the match
//! seed to every seat as "not a secret", the map is a pure function of that
//! seed and the rules table, and the travel estimator already prices every leg
//! as known ground.
//!
//! PLACEHOLDER: **OWNER**, at **S3**, with the knowledge store -- real terrain
//! fog (never seen, last known, the x1.5 cost bound) arrives as **one** mask
//! that feeds both this view and `RouteAdapter::set_unknown_columns`, so the
//! camera and the editor's dashed legs cannot disagree. The wire is ready for
//! it: `ViewChunk`'s comment says a decoder draws nothing for a material value
//! it does not know.

use pharmakos_proto::chunk_rle;
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_sim::tables::SeatId;
use pharmakos_sim::voxels::VoxelStore;

use crate::feed::ViewCursor;
use crate::fog::{Viewer, Vision};

/// How many bytes of encoded runs one page of the view carries.
///
/// The cut is on **bytes** rather than on a chunk count because a chunk's
/// encoded size is between three bytes and three times raw, and a count would
/// therefore bound nothing. The first chunk of a page always goes through, so
/// a chunk larger than the whole budget is delivered rather than being a hole
/// no page can ever fill.
///
/// PLACEHOLDER: 256 KiB is a working number with two technical reasons behind
/// it and no measurement of its own. Godot's `WebSocketPeer` defaults to an
/// inbound buffer of about 64 KiB and the client has to raise it either way;
/// and one page is answered by the single thread that owns the surface, so a
/// larger page is a longer stretch in which no other connection is answered.
/// **OWNER**, at hardening, with `MAX_MESSAGE_BYTES`, `READ_TIMEOUT` and the
/// rate limits.
pub const VIEW_PAGE_BYTES: usize = 256 * 1024;

/// What decides, for one viewer, which voxels of a modified chunk it may see.
///
/// The feed takes it as a pair of plain numbers rather than reaching for a
/// world, because the day the sim answers "what changed about this seat's
/// sight" (when units get a sight radius -- PLACEHOLDER, **OWNER**, S1/S3)
/// this is the value that stops being recomputed from scratch.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Sight {
    /// True when the fog policy has lifted for this viewer, so every voxel of
    /// every chunk is its own.
    pub unfogged: bool,
    /// A digest of whatever else decides what the viewer sees -- today, its
    /// own living beacons' centres. When this changes, every modified chunk is
    /// recomputed for the viewer rather than only the ones the world touched.
    pub key: u64,
}

/// One modified chunk's bytes as one viewer is entitled to them.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Entitled {
    chunk: u32,
    /// Run-length encoded, ready for the wire.
    bytes: Vec<u8>,
    /// The stamp at which **these** bytes last changed.
    changed_at: u64,
}

/// One viewer's derived state. Per viewer, never per token and never per
/// connection: two connections on one seat's token share this, so neither can
/// corrupt the other's position, and a dropped connection resumes with the
/// cursor it already had.
#[derive(Clone, PartialEq, Eq, Debug)]
struct ViewerState {
    viewer: Viewer,
    entitled: Vec<Entitled>,
    refreshed_at: u64,
    sight: Sight,
    /// Opaque handles, in the order this viewer first saw each thing.
    units: Vec<(u32, String)>,
    structures: Vec<(u32, String)>,
}

impl ViewerState {
    const fn new(viewer: Viewer) -> ViewerState {
        ViewerState {
            viewer,
            entitled: Vec::new(),
            refreshed_at: 0,
            sight: Sight {
                unfogged: false,
                key: 0,
            },
            units: Vec::new(),
            structures: Vec::new(),
        }
    }
}

/// One page of a view, before it becomes JSON.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ViewPage {
    /// The chunks this page carries, ascending: origin and encoded runs.
    pub chunks: Vec<(Voxel, Vec<u8>)>,
    /// False while more pages of this listing are waiting.
    pub complete: bool,
    /// Where the next call resumes.
    pub next: ViewCursor,
}

/// The derived view state for one hosted match.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ViewFeed {
    /// Zero until a match is attached: a cursor can then never be mistaken for
    /// one of a view that exists.
    view_id: u64,
    seq: u64,
    /// The generated map, encoded once, one entry per chunk in chunk order.
    generated: Vec<Vec<u8>>,
    /// The stamp at which each chunk's **world** bytes last changed.
    changed_at: Vec<u64>,
    /// Every chunk that has ever been written, ascending and unique.
    modified: Vec<u32>,
    modified_flag: Vec<bool>,
    viewers: Vec<ViewerState>,
}

impl ViewFeed {
    /// A feed with no match behind it.
    #[must_use]
    pub const fn new() -> ViewFeed {
        ViewFeed {
            view_id: 0,
            seq: 0,
            generated: Vec::new(),
            changed_at: Vec::new(),
            modified: Vec::new(),
            modified_flag: Vec::new(),
            viewers: Vec::new(),
        }
    }

    /// Take the generated map and open a view over it.
    ///
    /// Called by [`crate::surface::Surface::attach`], and by nothing else.
    /// Every cursor issued before it is
    /// [`crate::error::Code::StaleSnapshot`] afterwards, because the view id
    /// is minted here.
    ///
    /// The whole map is encoded once: 288 chunks at the skeleton's size, a few
    /// hundred kilobytes of runs, and every keyframe of the match is served
    /// from it without touching the world again.
    pub fn attach(&mut self, voxels: &VoxelStore, match_seed: u64) {
        let count = voxels.chunk_count();
        let slots = usize::try_from(count).unwrap_or(0);
        let mut generated: Vec<Vec<u8>> = Vec::with_capacity(slots);
        for chunk in 0..count {
            let bytes = voxels
                .chunk_bytes(chunk)
                .map_or_else(Vec::new, |raw| chunk_rle::encode(raw.as_slice()));
            generated.push(bytes);
        }
        let mut identity: Vec<u8> = Vec::with_capacity(16);
        identity.extend_from_slice(&match_seed.to_le_bytes());
        identity.extend_from_slice(&u64::from(count).to_le_bytes());
        // Not a counter: a cursor from another match is then refused rather
        // than accepted by coincidence, which is `SnapshotId`'s own argument.
        self.view_id = pharmakos_sim::digest(&identity) | 1;
        self.seq = 1;
        self.generated = generated;
        self.changed_at = vec![0; slots];
        self.modified = Vec::new();
        self.modified_flag = vec![false; slots];
        self.viewers = Vec::new();
    }

    /// True when a match has been attached.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.view_id != 0
    }

    /// The view's match-tied id.
    #[must_use]
    pub const fn view_id(&self) -> u64 {
        self.view_id
    }

    /// The feed's own monotonic stamp.
    #[must_use]
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    /// Move the stamp on, because something a viewer can see may have changed.
    ///
    /// Once per stepped tick, and once per attach, phase change and fog-policy
    /// change. Bumping too often costs a recomputation; bumping too seldom
    /// loses a change, so this is called from the places that cannot be missed
    /// rather than from the places that are interesting.
    pub const fn bump(&mut self) {
        if self.view_id != 0 {
            self.seq = self.seq.saturating_add(1);
        }
    }

    /// Record that these chunks' world bytes changed at the current stamp.
    ///
    /// `settled` is [`VoxelStore::settled`], which is the sim's own answer to
    /// "which chunks did this tick write" -- ascending, unique, and refreshed
    /// inside the tick that wrote them.
    pub fn stamp(&mut self, settled: &[u32]) {
        for chunk in settled {
            let Ok(index) = usize::try_from(*chunk) else {
                continue;
            };
            if let Some(slot) = self.changed_at.get_mut(index) {
                *slot = self.seq;
            }
            if matches!(self.modified_flag.get(index), Some(false)) {
                if let Some(flag) = self.modified_flag.get_mut(index) {
                    *flag = true;
                }
                self.modified.push(*chunk);
                // Ascending, because every listing this feed produces is, and
                // because the key IS the chunk index so the order is total.
                self.modified.sort_unstable();
            }
        }
    }

    /// How many chunks have ever been written.
    #[must_use]
    pub fn modified(&self) -> usize {
        self.modified.len()
    }

    /// Bring one viewer's entitled bytes up to date.
    ///
    /// **This is the only place a [`Vision`] exists in the view's life**, and
    /// that is deliberate: `Surface::step` takes none -- its signature is
    /// T15's and does not move -- so the tick can stamp *which* chunks changed
    /// and nothing else. What a particular viewer may see of that change is
    /// worked out here, inside the `get_view` call that asked.
    ///
    /// Two reasons to recompute a chunk, and the second is the one that makes
    /// the unlock free: the world wrote it since this viewer last looked, or
    /// the viewer's own sight changed -- a beacon gained or lost, the fog
    /// policy lifted.
    pub fn refresh<V: Vision>(
        &mut self,
        viewer: Viewer,
        sight: Sight,
        voxels: &VoxelStore,
        vision: &V,
    ) {
        let ViewFeed {
            seq,
            generated,
            changed_at,
            modified,
            viewers,
            ..
        } = self;
        let index = slot_of(viewers, viewer);
        let Some(state) = viewers.get_mut(index) else {
            return;
        };
        let whole = state.sight != sight || state.refreshed_at == 0;
        let seat = match viewer {
            Viewer::Seat(seat) => Some(seat),
            Viewer::Spectator { .. } | Viewer::Admin => None,
        };

        for chunk in modified.iter() {
            let slot = usize::try_from(*chunk).unwrap_or(usize::MAX);
            let written = changed_at
                .get(slot)
                .copied()
                .is_some_and(|stamp| stamp > state.refreshed_at);
            if !whole && !written {
                continue;
            }
            let base = generated.get(slot).map_or(&[][..], Vec::as_slice);
            let bytes = entitled_bytes(*chunk, voxels, base, seat, sight.unfogged, vision);
            if let Some(at) = state.entitled.iter().position(|held| held.chunk == *chunk) {
                if let Some(held) = state.entitled.get_mut(at) {
                    if held.bytes != bytes {
                        held.bytes = bytes;
                        held.changed_at = *seq;
                    }
                }
            } else {
                // A chunk entering this viewer's list for the FIRST time has
                // not necessarily changed for it. A world edit the viewer
                // cannot see leaves the entitled bytes exactly the generated
                // ones -- and stamping those with the current stamp would put
                // the chunk in the viewer's next delta, whose mere presence
                // would say "something happened over there". So the stamp is
                // zero unless the bytes really differ from the map everybody
                // already has.
                let changed_at = if bytes == base { 0 } else { *seq };
                state.entitled.push(Entitled {
                    chunk: *chunk,
                    bytes,
                    changed_at,
                });
                // Ascending, because every listing this feed produces is.
                state.entitled.sort_unstable_by_key(|held| held.chunk);
            }
        }
        state.sight = sight;
        state.refreshed_at = *seq;
    }

    /// One page of the view for a viewer that has been refreshed.
    ///
    /// `cursor` is `None` for a keyframe. The listing is every chunk of the
    /// map for a keyframe and, for a delta, exactly the modified chunks whose
    /// bytes **for this viewer** changed after the cursor's `from_seq`.
    #[must_use]
    pub fn page(
        &self,
        viewer: Viewer,
        cursor: Option<ViewCursor>,
        voxels: &VoxelStore,
    ) -> ViewPage {
        let (from_seq, to_seq, start) = match cursor {
            None => (0, self.seq, 0),
            Some(held) if held.index == 0 => (held.from_seq, self.seq, 0),
            Some(held) => (held.from_seq, held.to_seq, held.index),
        };
        let state = self.viewers.iter().find(|state| state.viewer == viewer);

        let listing: Vec<u32> = if from_seq == 0 {
            (start..voxels.chunk_count()).collect()
        } else {
            state.map_or_else(Vec::new, |state| {
                state
                    .entitled
                    .iter()
                    .filter(|held| held.changed_at > from_seq && held.chunk >= start)
                    .map(|held| held.chunk)
                    .collect()
            })
        };

        let mut chunks: Vec<(Voxel, Vec<u8>)> = Vec::new();
        let mut spent: usize = 0;
        let mut next: Option<u32> = None;
        for chunk in listing {
            let bytes = self.bytes_for(state, chunk);
            // The first chunk of a page always goes through: a chunk whose
            // encoding is larger than the whole budget would otherwise be a
            // hole no page could ever fill.
            if !chunks.is_empty() && spent.saturating_add(bytes.len()) > VIEW_PAGE_BYTES {
                next = Some(chunk);
                break;
            }
            spent = spent.saturating_add(bytes.len());
            let origin = voxels
                .chunk_origin(chunk)
                .map_or(Voxel { x: 0, y: 0, z: 0 }, |at| Voxel {
                    x: at.first().copied().unwrap_or(0),
                    y: at.get(1).copied().unwrap_or(0),
                    z: at.get(2).copied().unwrap_or(0),
                });
            chunks.push((origin, bytes));
        }

        match next {
            Some(index) => ViewPage {
                chunks,
                complete: false,
                next: ViewCursor {
                    view_id: self.view_id,
                    from_seq,
                    to_seq,
                    index,
                },
            },
            None => ViewPage {
                chunks,
                complete: true,
                // The stamp the listing STARTED at, not the one the feed is on
                // now: see [`ViewCursor`]'s own doc for why that is the
                // difference between a chunk being re-sent and a chunk being
                // lost.
                next: ViewCursor {
                    view_id: self.view_id,
                    from_seq: to_seq,
                    to_seq,
                    index: 0,
                },
            },
        }
    }

    /// The bytes one chunk goes on the wire as for this viewer: its entitled
    /// bytes when the chunk has ever been written, and the generated chunk
    /// otherwise.
    fn bytes_for(&self, state: Option<&ViewerState>, chunk: u32) -> Vec<u8> {
        if let Some(held) = state.and_then(|state| {
            state
                .entitled
                .iter()
                .find(|entitled| entitled.chunk == chunk)
        }) {
            return held.bytes.clone();
        }
        self.generated
            .get(usize::try_from(chunk).unwrap_or(usize::MAX))
            .cloned()
            .unwrap_or_default()
    }

    /// This viewer's opaque handle for a unit, minted on first sighting.
    ///
    /// **A dense table row id is a count.** One visible enemy drone carrying
    /// the id `u_812` would tell a seat that the match has made eight hundred
    /// units, which is a number no answer of this gateway reports and no seat
    /// may derive. So a handle counts what *this viewer* has seen, which is by
    /// construction something it already knows.
    pub fn unit_handle(&mut self, viewer: Viewer, id: u32) -> String {
        ViewFeed::handle(&mut self.viewers, viewer, id, false)
    }

    /// This viewer's opaque handle for a structure. As [`ViewFeed::unit_handle`].
    pub fn structure_handle(&mut self, viewer: Viewer, id: u32) -> String {
        ViewFeed::handle(&mut self.viewers, viewer, id, true)
    }

    fn handle(viewers: &mut Vec<ViewerState>, viewer: Viewer, id: u32, structure: bool) -> String {
        let index = slot_of(viewers, viewer);
        let Some(state) = viewers.get_mut(index) else {
            return String::new();
        };
        let (prefix, table) = if structure {
            ("s_", &mut state.structures)
        } else {
            ("u_", &mut state.units)
        };
        if let Some((_, handle)) = table.iter().find(|(held, _)| *held == id) {
            return handle.clone();
        }
        let handle = format!("{prefix}{}", table.len().saturating_add(1));
        table.push((id, handle.clone()));
        handle
    }
}

/// This viewer's slot, made on first sight of it.
///
/// A `Vec` walked in order rather than a map: a handful of viewers, and no
/// hash container anywhere near this crate's state (AGENTS.md section 4.4).
fn slot_of(viewers: &mut Vec<ViewerState>, viewer: Viewer) -> usize {
    if let Some(found) = viewers.iter().position(|state| state.viewer == viewer) {
        return found;
    }
    viewers.push(ViewerState::new(viewer));
    viewers.len().saturating_sub(1)
}

/// One chunk's bytes as `seat` is entitled to them, run-length encoded.
///
/// Built by **inclusion**: the answer starts from the generated map -- which
/// every viewer is entitled to whole (decisions-log item 107 (1)) -- and a
/// voxel is replaced by the world's current material only where this viewer
/// may see it. Nothing is ever redacted out of a current byte, because a
/// redaction has to know what it is hiding and a mistake in one is a leak.
///
/// A voxel whose current material equals the generated one is copied without
/// asking the vision at all: identical bytes disclose nothing, and skipping
/// the question is what keeps a chunk with one crater in it cheap.
fn entitled_bytes<V: Vision>(
    chunk: u32,
    voxels: &VoxelStore,
    generated: &[u8],
    seat: Option<SeatId>,
    unfogged: bool,
    vision: &V,
) -> Vec<u8> {
    let Some(world) = voxels.chunk_bytes(chunk) else {
        return generated.to_vec();
    };
    if unfogged {
        return chunk_rle::encode(world.as_slice());
    }
    let Some(seat) = seat else {
        // No seat, no sight, and the policy has not lifted: the generated map
        // and nothing of what happened to it.
        return generated.to_vec();
    };
    let base = chunk_rle::decode(generated).unwrap_or_else(|_| vec![0; chunk_rle::CHUNK_VOXELS]);
    let Some(origin) = voxels.chunk_origin(chunk) else {
        return generated.to_vec();
    };
    let (ox, oy, oz) = (
        origin.first().copied().unwrap_or(0),
        origin.get(1).copied().unwrap_or(0),
        origin.get(2).copied().unwrap_or(0),
    );

    let mut out: Vec<u8> = Vec::with_capacity(chunk_rle::CHUNK_VOXELS);
    for index in 0..chunk_rle::CHUNK_VOXELS {
        let current = world.get(index).copied().unwrap_or(0);
        let pristine = base.get(index).copied().unwrap_or(current);
        if current == pristine {
            out.push(current);
            continue;
        }
        // The voxel order inside a chunk is x + 32y + 1024z (item 92), so the
        // address splits by a shift and a mask and never by a division.
        let Ok(offset) = i32::try_from(index) else {
            out.push(pristine);
            continue;
        };
        let at = Voxel {
            x: ox.saturating_add(offset & 31),
            y: oy.saturating_add((offset >> 5_i32) & 31),
            z: oz.saturating_add(offset >> 10_i32),
        };
        out.push(if vision.sees(seat, &at) {
            current
        } else {
            pristine
        });
    }
    chunk_rle::encode(&out)
}

#[cfg(test)]
mod tests {
    use super::{Sight, VIEW_PAGE_BYTES, ViewFeed};
    use crate::fog::{Blind, KnownVoxels, Viewer};
    use pharmakos_proto::chunk_rle;
    use pharmakos_proto::gp::v1::Voxel;
    use pharmakos_sim::tables::SeatId;
    use pharmakos_sim::voxels::{Material, VoxelStore};

    fn store() -> VoxelStore {
        let mut store = VoxelStore::new([64, 32, 32]).expect("a two-chunk store");
        // Something to see: a solid floor across both chunks.
        for y in 0..32_i32 {
            for x in 0..64_i32 {
                store.set([x, y, 0], Material::STONE);
            }
        }
        let mut digests = pharmakos_sim::chunks::ChunkDigests::new(store.chunk_count());
        store.settle(&mut digests);
        store
    }

    fn open() -> (ViewFeed, VoxelStore) {
        let store = store();
        let mut feed = ViewFeed::new();
        feed.attach(&store, 7);
        (feed, store)
    }

    fn sight(unfogged: bool) -> Sight {
        Sight { unfogged, key: 0 }
    }

    #[test]
    fn a_keyframe_carries_every_chunk_of_the_map() {
        let (mut feed, store) = open();
        let viewer = Viewer::Seat(SeatId::new(0));
        feed.refresh(viewer, sight(false), &store, &Blind);
        let page = feed.page(viewer, None, &store);
        assert!(page.complete);
        assert_eq!(page.chunks.len(), 2, "a 64x32x32 map is two chunks");
        let origins: Vec<(i32, i32, i32)> = page
            .chunks
            .iter()
            .map(|(at, _)| (at.x, at.y, at.z))
            .collect();
        assert_eq!(origins, vec![(0, 0, 0), (32, 0, 0)]);
        for (_, bytes) in &page.chunks {
            assert_eq!(
                chunk_rle::decode(bytes).expect("a chunk").len(),
                chunk_rle::CHUNK_VOXELS
            );
        }
        assert!(feed.is_open());
        assert_eq!(feed.modified(), 0, "the generator's writes are pristine");
    }

    #[test]
    fn an_edit_a_seat_cannot_see_never_reaches_it_as_a_delta() {
        let (mut feed, mut store) = open();
        let seat = SeatId::new(0);
        let viewer = Viewer::Seat(seat);
        feed.refresh(viewer, sight(false), &store, &Blind);
        let caught_up = feed.page(viewer, None, &store).next;

        // A tick writes one voxel of chunk 0 that this blind seat cannot see.
        let mut digests = pharmakos_sim::chunks::ChunkDigests::new(store.chunk_count());
        store.set([4, 4, 0], Material::AIR);
        store.settle(&mut digests);
        feed.bump();
        feed.stamp(store.settled());
        assert_eq!(feed.modified(), 1);

        feed.refresh(viewer, sight(false), &store, &Blind);
        let page = feed.page(viewer, Some(caught_up), &store);
        assert!(page.complete);
        assert!(
            page.chunks.is_empty(),
            "a delta whose presence said `something happened over there` is a leak"
        );
    }

    #[test]
    fn an_edit_a_seat_can_see_arrives_with_its_current_bytes() {
        let (mut feed, mut store) = open();
        let seat = SeatId::new(0);
        let viewer = Viewer::Seat(seat);
        let place = Voxel { x: 4, y: 4, z: 0 };
        let mut vision = KnownVoxels::new();
        vision.see(seat, &place);

        feed.refresh(viewer, sight(false), &store, &vision);
        let caught_up = feed.page(viewer, None, &store).next;

        let mut digests = pharmakos_sim::chunks::ChunkDigests::new(store.chunk_count());
        store.set([4, 4, 0], Material::AIR);
        store.settle(&mut digests);
        feed.bump();
        feed.stamp(store.settled());

        feed.refresh(viewer, sight(false), &store, &vision);
        let page = feed.page(viewer, Some(caught_up), &store);
        assert_eq!(page.chunks.len(), 1);
        let (_, bytes) = page.chunks.first().expect("the chunk");
        let voxels = chunk_rle::decode(bytes).expect("a chunk");
        let index = 4_usize
            .saturating_add(4_usize.saturating_mul(32))
            .saturating_add(0);
        assert_eq!(
            voxels.get(index).copied(),
            Some(Material::AIR.raw()),
            "the seat saw it happen"
        );
    }

    #[test]
    fn the_full_map_unlock_is_the_viewer_becoming_unfogged() {
        let (mut feed, mut store) = open();
        let viewer = Viewer::Seat(SeatId::new(0));
        feed.refresh(viewer, sight(false), &store, &Blind);
        let caught_up = feed.page(viewer, None, &store).next;

        let mut digests = pharmakos_sim::chunks::ChunkDigests::new(store.chunk_count());
        store.set([40, 4, 0], Material::AIR);
        store.settle(&mut digests);
        feed.bump();
        feed.stamp(store.settled());
        feed.refresh(viewer, sight(false), &store, &Blind);
        assert!(
            feed.page(viewer, Some(caught_up), &store).chunks.is_empty(),
            "fogged: nothing"
        );

        // The policy lifts. Nothing about the world changed.
        feed.bump();
        feed.refresh(viewer, sight(true), &store, &Blind);
        let page = feed.page(viewer, Some(caught_up), &store);
        assert_eq!(page.chunks.len(), 1, "only the modified chunk pages out");
    }

    #[test]
    fn a_handle_counts_what_this_viewer_has_seen_and_nothing_else() {
        let (mut feed, _store) = open();
        let viewer = Viewer::Seat(SeatId::new(0));
        assert_eq!(feed.unit_handle(viewer, 812), "u_1");
        assert_eq!(feed.unit_handle(viewer, 9), "u_2");
        assert_eq!(feed.unit_handle(viewer, 812), "u_1", "stable");
        assert_eq!(feed.structure_handle(viewer, 400), "s_1");
        let other = Viewer::Seat(SeatId::new(1));
        assert_eq!(
            feed.unit_handle(other, 812),
            "u_1",
            "another viewer's numbering is its own"
        );
    }

    /// A keyframe wider than one page is paged, and the pages between them
    /// carry the stamp the listing started at.
    ///
    /// The golden seed's own map does **not** exercise this: 288 chunks of
    /// generated heightmap encode to about 200 KiB of runs, which is one page.
    /// So the case is made here, with the worst chunk the encoding has -- no
    /// two neighbours alike, three bytes a voxel, 96 KiB a chunk -- which is
    /// also the shape the page budget is counted in bytes for.
    #[test]
    fn a_keyframe_wider_than_a_page_is_paged_in_ascending_chunk_order() {
        let mut store = VoxelStore::new([128, 32, 32]).expect("a four-chunk store");
        for z in 0..32_i32 {
            for y in 0..32_i32 {
                for x in 0..128_i32 {
                    let alternate = x.saturating_add(y).saturating_add(z) & 1;
                    if alternate == 1 {
                        store.set([x, y, z], Material::STONE);
                    }
                }
            }
        }
        let mut digests = pharmakos_sim::chunks::ChunkDigests::new(store.chunk_count());
        store.settle(&mut digests);

        let mut feed = ViewFeed::new();
        feed.attach(&store, 11);
        let viewer = Viewer::Seat(SeatId::new(0));
        feed.refresh(viewer, sight(false), &store, &Blind);

        let mut pages = 0_u32;
        let mut delivered: Vec<i32> = Vec::new();
        let mut cursor = None;
        loop {
            let page = feed.page(viewer, cursor, &store);
            pages = pages.saturating_add(1);
            let spent: usize = page.chunks.iter().map(|(_, bytes)| bytes.len()).sum();
            assert!(
                spent <= VIEW_PAGE_BYTES || page.chunks.len() == 1,
                "a page is cut at {VIEW_PAGE_BYTES} bytes of runs, and only a single chunk \
                 larger than the whole budget goes over"
            );
            for (origin, _) in &page.chunks {
                delivered.push(origin.x);
            }
            cursor = Some(page.next);
            if page.complete {
                break;
            }
            assert!(pages < 16, "a keyframe that never completes");
        }
        assert!(pages > 1, "four worst-case chunks do not fit one page");
        assert_eq!(
            delivered,
            vec![0, 32, 64, 96],
            "every chunk, once, in ascending chunk order"
        );
        let last = cursor.expect("a completing cursor");
        assert_eq!(last.from_seq, last.to_seq, "and the client is caught up");
        assert!(!last.is_keyframe());
    }

    #[test]
    fn a_keyframe_pages_and_the_completing_cursor_is_the_stamp_it_started_at() {
        let (mut feed, store) = open();
        let viewer = Viewer::Seat(SeatId::new(0));
        feed.refresh(viewer, sight(false), &store, &Blind);
        let started = feed.seq();
        let page = feed.page(viewer, None, &store);
        assert!(page.complete, "two small chunks fit one page");
        assert_eq!(page.next.from_seq, started);
        assert_eq!(page.next.to_seq, started);
        assert_eq!(page.next.index, 0);
        assert!(!page.next.is_keyframe());
    }
}

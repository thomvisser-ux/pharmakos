// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Generational ids: a row and a generation in the low 28 bits of every id
//! (S2's plan, E1).
//!
//! # The layout, and why it is additive
//!
//! Every per-kind id ([`crate::tables::UnitId`], [`crate::tables::BeaconId`],
//! [`crate::tables::StructureId`], [`crate::tables::WreckId`]) is a `u32` whose
//! low 28 bits are `generation (16) | row (12)`, and an
//! [`crate::knowledge::AssetId`] puts its 4-bit kind tag above them:
//!
//! | Bits | What |
//! |---|---|
//! | 31..28 | the kind tag ([`crate::knowledge::AssetId`] only; zero in a per-kind id) |
//! | 27..12 | the [`Generation`]: how many times the row has been reused |
//! | 11..0 | the [`Row`]: where the asset lives in its table |
//!
//! Every id written before rows were reused has generation 0, so its bits are
//! its row and it **decodes as itself**: the event vocabulary's rule for a
//! value's bit layout (new bits every older value holds zero; AGENTS.md
//! section 5, item 135 (2) (j)) holds, and the wire, the gateway's ids and the
//! event subject's width are unchanged. The packed order is (tag, generation,
//! row), so "the lowest id" reads the oldest generation first and then the
//! lowest row: total, and deterministic. A row-major packing was weighed and
//! not taken, because it renumbers every id already written.
//!
//! # Bounded by type, not by mask
//!
//! A [`Row`] holds 0 to [`Row::MAX`] (4 094) and a [`Generation`] a `u16`, and
//! [`pack`] takes one of each, so it cannot truncate. Row 4 095 is never a
//! real row: a table whose room would reach it is refused at load
//! ([`ROOM_LIMIT`]), and the all-ones low bits are what a sentinel
//! (`u32::MAX`) carries, which [`unpack`] refuses with [`IdError::Sentinel`].
//!
//! # Rows are reused, references are checked
//!
//! A dead row is freed at the end of the Match phase once its death has
//! settled ([`crate::world`]), and the lowest free row is reused first under the
//! next generation. Nothing relies on clearing the holders of a freed row's id:
//! every read of a referenced row compares the reference's generation with the
//! row's own, and a stale reference answers [`RefError::Stale`], counted under
//! its [`StaleRef`] reason and read as "gone".

/// How far the generation is shifted.
const GENERATION_SHIFT: u32 = 12;

/// The low bits a row occupies.
const ROW_MASK: u32 = (1 << GENERATION_SHIFT) - 1;

/// The bits a per-kind id may use: row and generation, under the tag.
const ID_MASK: u32 = (1 << 28) - 1;

/// The most rows any table may be built with: rows 0 to [`Row::MAX`]. A table
/// whose room, derived at load, would exceed it is refused with a typed error
/// ([`crate::mapgen::MapError::RoomTooLarge`]).
pub const ROOM_LIMIT: u32 = 4_095;

/// Where an asset lives in its table: 0 to [`Row::MAX`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Row(u16);

impl Row {
    /// The highest real row. Row 4 095 is never one: it is the low bits of a
    /// sentinel.
    pub const MAX: Row = Row(4_094);

    /// The first row.
    pub const FIRST: Row = Row(0);

    /// The row at `index`, or `None` past [`Row::MAX`].
    #[must_use]
    pub const fn new(index: u32) -> Option<Row> {
        if index > Row::MAX.raw() {
            return None;
        }
        // At most 4 094, so the two high bytes are zero.
        let [low, high, _, _] = index.to_le_bytes();
        Some(Row(u16::from_le_bytes([low, high])))
    }

    /// The row at column index `index`, or `None` past [`Row::MAX`].
    #[must_use]
    pub fn of_index(index: usize) -> Option<Row> {
        u32::try_from(index).ok().and_then(Row::new)
    }

    /// Every row, in ascending order: what a walk over a table's columns zips
    /// with, so a column index never has to be converted back into a row.
    pub fn all() -> impl Iterator<Item = Row> + Clone {
        (0..=Row::MAX.0).map(Row)
    }

    /// The column index.
    #[must_use]
    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    /// The row as a `u32`.
    #[must_use]
    pub const fn raw(self) -> u32 {
        // A `u16` widens into a `u32` without loss.
        let [low, high] = self.0.to_le_bytes();
        u32::from_le_bytes([low, high, 0, 0])
    }
}

/// How many times a row has been reused: 0 for its first asset.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Generation(u16);

impl Generation {
    /// A row's first asset.
    pub const ZERO: Generation = Generation(0);

    /// Build from a raw count.
    #[must_use]
    pub const fn new(raw: u16) -> Generation {
        Generation(raw)
    }

    /// The raw count.
    #[must_use]
    pub const fn raw(self) -> u16 {
        self.0
    }

    /// The next generation, or `None` at `u16::MAX`: checked, never a wrap.
    /// A row whose next generation would pass the maximum is never reused
    /// again ([`IdDrop::Retired`]).
    #[must_use]
    pub const fn next(self) -> Option<Generation> {
        match self.0.checked_add(1) {
            Some(next) => Some(Generation(next)),
            None => None,
        }
    }
}

/// Why an id names no row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdError {
    /// The id is a "none" sentinel (`u32::MAX`): it names nothing, and its low
    /// bits would otherwise read as generation 65 535 and row 4 095.
    Sentinel,
    /// The id has bits above its row and generation (a tag where a per-kind id
    /// holds none), or a row past [`Row::MAX`]: it describes no id this sim
    /// writes.
    OutOfLayout,
}

impl core::fmt::Display for IdError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            IdError::Sentinel => write!(f, "the id is a sentinel and names no row"),
            IdError::OutOfLayout => write!(f, "the id is outside the row and generation layout"),
        }
    }
}

impl std::error::Error for IdError {}

/// A per-kind id's raw value: `generation << 12 | row`. Cannot truncate.
#[must_use]
pub const fn pack(row: Row, generation: Generation) -> u32 {
    let [low, high] = generation.0.to_le_bytes();
    (u32::from_le_bytes([low, high, 0, 0]) << GENERATION_SHIFT) | row.raw()
}

/// The row and generation a per-kind id's raw value names.
///
/// # Errors
///
/// [`IdError::Sentinel`] for `u32::MAX`, and [`IdError::OutOfLayout`] for any
/// other value with bits above the generation or a row past [`Row::MAX`].
pub const fn unpack(raw: u32) -> Result<(Row, Generation), IdError> {
    if raw == u32::MAX {
        return Err(IdError::Sentinel);
    }
    if raw & !ID_MASK != 0 {
        return Err(IdError::OutOfLayout);
    }
    let Some(row) = Row::new(raw & ROW_MASK) else {
        return Err(IdError::OutOfLayout);
    };
    let [low, high, _, _] = (raw >> GENERATION_SHIFT).to_le_bytes();
    Ok((row, Generation(u16::from_le_bytes([low, high]))))
}

/// Why a reference into a table answered nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefError {
    /// The reference is a sentinel or outside the layout ([`IdError`]).
    Id(IdError),
    /// The reference names a row the table does not hold.
    NoSuchRow,
    /// The reference names a row that has been freed, or reused under a later
    /// generation: the asset it named is gone (S2's plan, E1).
    Stale,
}

impl From<IdError> for RefError {
    fn from(error: IdError) -> RefError {
        RefError::Id(error)
    }
}

impl core::fmt::Display for RefError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RefError::Id(error) => write!(f, "{error}"),
            RefError::NoSuchRow => write!(f, "the reference names a row the table does not hold"),
            RefError::Stale => write!(f, "the reference names a row since freed or reused"),
        }
    }
}

impl std::error::Error for RefError {}

/// Where a stale reference was met (S2's plan, E1): each reader that holds an
/// id into another table names its reason, so a count says which holder still
/// pointed at a freed row. Counted beside [`crate::world::DamageDrop`], never
/// hashed.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum StaleRef {
    /// A Build target's `built` link to a structure since freed: the target is
    /// read as not going up.
    TargetBuilt,
    /// A structure's home beacon, since freed.
    StructureHome,
    /// A unit's home beacon, since freed.
    UnitHome,
    /// A kill-credit row against an asset since freed: the row is dropped and
    /// credits nobody.
    CreditSlot,
    /// A beacon the interpreter pinned, visited or resumes at, since freed.
    PinnedBeacon,
}

impl StaleRef {
    /// Every reason.
    pub const ALL: [StaleRef; 5] = [
        StaleRef::TargetBuilt,
        StaleRef::StructureHome,
        StaleRef::UnitHome,
        StaleRef::CreditSlot,
        StaleRef::PinnedBeacon,
    ];

    /// The reason's name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            StaleRef::TargetBuilt => "target_built",
            StaleRef::StructureHome => "structure_home",
            StaleRef::UnitHome => "unit_home",
            StaleRef::CreditSlot => "credit_slot",
            StaleRef::PinnedBeacon => "pinned_beacon",
        }
    }
}

/// Why a freed row was not handed out again (S2's plan, E1). Counted, never
/// hashed.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum IdDrop {
    /// The row's generation is at `u16::MAX`: reusing it would wrap, so the
    /// row is retired and stays free for the rest of the match.
    Retired,
}

/// How many stale references were met and rows retired, by reason, since the
/// world was built. Diagnostic: never hashed, never snapshotted, and equal to
/// any other count, so two worlds that differ only in what was last counted
/// are the same world (the [`crate::seams::UnitTally`] rule).
///
/// Interior-mutable, because the readers that meet a stale reference read the
/// world through a shared borrow.
#[derive(Debug, Default)]
pub struct RefCounts {
    target_built: Counter,
    structure_home: Counter,
    unit_home: Counter,
    credit_slot: Counter,
    pinned_beacon: Counter,
    retired: Counter,
}

/// One interior-mutable count.
#[derive(Debug, Default)]
struct Counter(core::sync::atomic::AtomicU64);

impl Counter {
    /// Add one. A `u64` of reads cannot be reached, so the add saturates by
    /// rule.
    fn bump(&self) {
        let now = self.get();
        self.0
            .store(now.saturating_add(1), core::sync::atomic::Ordering::Relaxed);
    }

    fn get(&self) -> u64 {
        self.0.load(core::sync::atomic::Ordering::Relaxed)
    }

    fn set(&self, value: u64) {
        self.0.store(value, core::sync::atomic::Ordering::Relaxed);
    }
}

impl RefCounts {
    const fn counter(&self, reason: StaleRef) -> &Counter {
        match reason {
            StaleRef::TargetBuilt => &self.target_built,
            StaleRef::StructureHome => &self.structure_home,
            StaleRef::UnitHome => &self.unit_home,
            StaleRef::CreditSlot => &self.credit_slot,
            StaleRef::PinnedBeacon => &self.pinned_beacon,
        }
    }

    /// Count one stale reference.
    pub fn stale(&self, reason: StaleRef) {
        self.counter(reason).bump();
    }

    /// Count one retired row.
    pub fn retired(&self) {
        self.retired.bump();
    }

    /// How many stale references were met for `reason`.
    #[must_use]
    pub fn of(&self, reason: StaleRef) -> u64 {
        self.counter(reason).get()
    }

    /// How many rows were retired for `reason`.
    #[must_use]
    pub fn dropped(&self, reason: IdDrop) -> u64 {
        match reason {
            IdDrop::Retired => self.retired.get(),
        }
    }

    /// Every stale reference, all reasons together.
    #[must_use]
    pub fn total(&self) -> u64 {
        StaleRef::ALL
            .iter()
            .fold(0_u64, |sum, reason| sum.saturating_add(self.of(*reason)))
    }
}

impl Clone for RefCounts {
    fn clone(&self) -> RefCounts {
        let counts = RefCounts::default();
        for reason in StaleRef::ALL {
            counts.counter(reason).set(self.of(reason));
        }
        counts.retired.set(self.dropped(IdDrop::Retired));
        counts
    }
}

/// Every count equals every other: a measurement is not state.
impl PartialEq for RefCounts {
    fn eq(&self, _other: &RefCounts) -> bool {
        true
    }
}

impl Eq for RefCounts {}

/// The per-kind id types share one shape: a raw `u32` in the layout above, a
/// sentinel that names nothing, and checked accessors.
macro_rules! generational_id {
    ($name:ident, $what:literal) => {
        impl $name {
            #[doc = concat!("The id of the ", $what, " at `row`, in `generation`.")]
            #[must_use]
            pub const fn of(row: $crate::ids::Row, generation: $crate::ids::Generation) -> $name {
                $name($crate::ids::pack(row, generation))
            }

            #[doc = concat!("The first ", $what, " a row ever holds: generation 0, so the id is the row.")]
            #[must_use]
            pub const fn first(row: $crate::ids::Row) -> $name {
                $name::of(row, $crate::ids::Generation::ZERO)
            }

            #[doc = concat!("The row the ", $what, " lives in.")]
            ///
            /// # Errors
            ///
            /// [`crate::ids::IdError`]: a sentinel names no row, and a value
            /// outside the layout describes no id this sim writes.
            pub const fn row(self) -> Result<$crate::ids::Row, $crate::ids::IdError> {
                match $crate::ids::unpack(self.0) {
                    Ok((row, _)) => Ok(row),
                    Err(error) => Err(error),
                }
            }

            /// How many times the row had been reused when this id was minted.
            ///
            /// # Errors
            ///
            /// As the row's.
            pub const fn generation(
                self,
            ) -> Result<$crate::ids::Generation, $crate::ids::IdError> {
                match $crate::ids::unpack(self.0) {
                    Ok((_, generation)) => Ok(generation),
                    Err(error) => Err(error),
                }
            }
        }
    };
}

pub(crate) use generational_id;

#[cfg(test)]
mod tests {
    use super::{Generation, IdError, Row, pack, unpack};

    #[test]
    fn a_generation_zero_id_is_its_row() {
        for index in [0_u32, 1, 7, 299, 4_094] {
            let row = Row::new(index).unwrap_or_else(|| panic!("{index} is a row"));
            assert_eq!(pack(row, Generation::ZERO), index);
            assert_eq!(unpack(index), Ok((row, Generation::ZERO)));
        }
    }

    #[test]
    fn the_layout_round_trips_and_orders_by_generation_first() {
        let row = Row::new(5).unwrap_or_else(|| panic!("a row"));
        let id = pack(row, Generation::new(3));
        assert_eq!(id, (3 << 12) | 5);
        assert_eq!(unpack(id), Ok((row, Generation::new(3))));
        let lower_row_later = pack(Row::FIRST, Generation::new(1));
        assert!(
            pack(Row::new(4_094).unwrap_or_default(), Generation::ZERO) < lower_row_later,
            "the oldest generation sorts first"
        );
    }

    #[test]
    fn a_sentinel_and_an_out_of_layout_value_name_no_row() {
        assert_eq!(unpack(u32::MAX), Err(IdError::Sentinel));
        assert_eq!(unpack(1 << 28), Err(IdError::OutOfLayout));
        assert_eq!(unpack(4_095), Err(IdError::OutOfLayout));
        assert_eq!(Row::new(4_095), None);
        assert_eq!(Generation::new(u16::MAX).next(), None);
    }
}

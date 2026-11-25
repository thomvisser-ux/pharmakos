// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`Reserved`]: a `Vec` whose clone keeps the room its owner reserved.
//!
//! The tick allocates nothing (`tests/allocations.rs`): every table, queue and
//! scratch buffer the world owns reserves its room at construction (and again
//! at a restore), so a `push` inside a tick never grows a vector. A derived
//! `Clone` of a `Vec` copies its elements and **not** that room: the clone's
//! capacity is its length. A cloned world (a `research` fork, a save's copy)
//! would then allocate on the first tick that filled one of those buffers past
//! its length, in the clone rather than in the original, which is exactly
//! where nobody looks.
//!
//! So every vector the world owns that is reserved past its length is a
//! `Reserved`, whether its contents are state (a table's column, hashed and
//! snapshotted) or scratch (dead between ticks): the type changes nothing but
//! the clone's capacity, and capacity is not state. Its clone copies every
//! element, as the derived clone did, so a clone still equals its original and
//! no hashed value can move; it then holds at least the original's room.
//! `World` keeps `#[derive(Clone)]`, and a new field of this type is held to
//! the rule by its type, not by a hand-written `Clone` that a new field could
//! be left out of.
//!
//! It dereferences to the `Vec` it wraps, so every reader and writer of the
//! vector is unchanged; only a construction names the type.

use core::ops::{Deref, DerefMut};

/// A `Vec` whose clone keeps the original's reserved room (see the module
/// docs). Compares, orders, hashes and prints as the `Vec` it wraps.
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Reserved<T>(Vec<T>);

impl<T> Reserved<T> {
    /// An empty vector with room for `capacity` elements, as
    /// `Vec::with_capacity`.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Reserved<T> {
        Reserved(Vec::with_capacity(capacity))
    }

    /// Room for at least `additional` more elements, as `Vec::reserve`.
    ///
    /// Inherent rather than reached through `DerefMut` so that a call whose
    /// argument reads the vector (`v.reserve(room - v.len())`, every table's
    /// reservation) borrows as it did on the bare `Vec`.
    pub fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional);
    }

    /// Room for at least `total` elements in all, whatever it holds now: a
    /// table's room stated as the rows it may hold, not as rows beyond its
    /// current length. Nothing is reserved when it has room for `total`
    /// already.
    pub fn reserve_total(&mut self, total: usize) {
        if let Some(more) = total.checked_sub(self.0.len()) {
            self.0.reserve(more);
        }
    }

    /// The `Vec` itself, room included.
    #[must_use]
    pub fn into_inner(self) -> Vec<T> {
        self.0
    }
}

/// Every element, as a derived clone copies them, into room for at least as
/// many elements as the original has room for.
impl<T: Clone> Clone for Reserved<T> {
    fn clone(&self) -> Reserved<T> {
        let mut copy = Vec::with_capacity(self.0.capacity());
        copy.extend_from_slice(&self.0);
        Reserved(copy)
    }
}

/// Empty, with no room: what `core::mem::take` leaves behind while a phase
/// holds the vector itself. Written out because a derived `Default` would ask
/// for `T: Default`, which an empty vector does not need.
impl<T> Default for Reserved<T> {
    fn default() -> Reserved<T> {
        Reserved(Vec::new())
    }
}

impl<T> From<Vec<T>> for Reserved<T> {
    /// Wraps `vector` with the room it already has.
    fn from(vector: Vec<T>) -> Reserved<T> {
        Reserved(vector)
    }
}

impl<T> FromIterator<T> for Reserved<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Reserved<T> {
        Reserved(Vec::from_iter(iter))
    }
}

impl<T> Deref for Reserved<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}

impl<T> DerefMut for Reserved<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.0
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for Reserved<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<'a, T> IntoIterator for &'a Reserved<T> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;

    fn into_iter(self) -> core::slice::Iter<'a, T> {
        self.0.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut Reserved<T> {
    type Item = &'a mut T;
    type IntoIter = core::slice::IterMut<'a, T>;

    fn into_iter(self) -> core::slice::IterMut<'a, T> {
        self.0.iter_mut()
    }
}

impl<T> IntoIterator for Reserved<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> std::vec::IntoIter<T> {
        self.0.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::Reserved;

    /// For every length `n` and room `c >= n` in the table, empty with room
    /// included, and full to its room: a clone has room for at least `c`
    /// elements and holds the original's elements, in order.
    #[test]
    fn a_clone_keeps_the_room_and_every_element() {
        for (n, c) in [
            (0_u32, 0_usize),
            (0, 1),
            (0, 64),
            (3, 3),
            (3, 64),
            (64, 64),
            (65, 4096),
        ] {
            let mut original: Reserved<u32> = Reserved::with_capacity(c);
            original.extend((0..n).map(|i| i.wrapping_mul(2_654_435_761)));
            let room = original.capacity();
            assert!(room >= c, "the original was reserved for {c}");
            let copy = original.clone();
            assert_eq!(
                copy.as_slice(),
                original.as_slice(),
                "a clone of {n} in room for {c} holds the same elements"
            );
            assert_eq!(copy, original, "a clone equals its original");
            assert!(
                copy.capacity() >= room,
                "a clone of {n} elements in room for {room} has room for {}",
                copy.capacity()
            );
        }
    }

    /// Records every byte a `Hash` impl writes, so two hashes compare as the
    /// bytes that make them.
    #[derive(Default)]
    struct Bytes(Vec<u8>);

    impl core::hash::Hasher for Bytes {
        fn finish(&self) -> u64 {
            0
        }

        fn write(&mut self, bytes: &[u8]) {
            self.0.extend_from_slice(bytes);
        }
    }

    /// Room is not state: two vectors with the same elements and different
    /// rooms compare equal, order equal and hash the same bytes, and a
    /// different element is still a difference.
    #[test]
    fn room_is_never_compared_ordered_or_hashed() {
        use core::hash::Hash;
        let mut small: Reserved<u32> = Reserved::with_capacity(3);
        small.extend([5, 6, 7]);
        let mut large: Reserved<u32> = Reserved::with_capacity(4096);
        large.extend([5, 6, 7]);
        assert!(large.capacity() > small.capacity());
        assert_eq!(small, large);
        assert_eq!(small.cmp(&large), core::cmp::Ordering::Equal);
        assert_eq!(small.partial_cmp(&large), Some(core::cmp::Ordering::Equal));
        let (mut a, mut b) = (Bytes::default(), Bytes::default());
        small.hash(&mut a);
        large.hash(&mut b);
        assert_eq!(a.0, b.0, "the same elements hash the same bytes");
        let mut other: Reserved<u32> = Reserved::with_capacity(4096);
        other.extend([5, 6, 8]);
        assert_ne!(small, other);
        assert_eq!(small.cmp(&other), core::cmp::Ordering::Less);
    }

    /// A clone of a clone keeps the room as well: a fork of a fork.
    #[test]
    fn a_clone_of_a_clone_keeps_the_room() {
        let mut original: Reserved<u8> = Reserved::with_capacity(128);
        original.push(7);
        let twice = original.clone().clone();
        assert!(twice.capacity() >= 128, "room for {}", twice.capacity());
        assert_eq!(twice.as_slice(), [7]);
    }
}

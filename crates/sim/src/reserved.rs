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

    #[test]
    fn a_clone_keeps_the_room_and_every_element() {
        let mut original: Reserved<u32> = Reserved::with_capacity(64);
        original.extend([3, 1, 2]);
        let copy = original.clone();
        assert_eq!(copy, original, "a clone equals its original");
        assert!(
            copy.capacity() >= original.capacity(),
            "a clone has room for {} elements, its original for {}",
            copy.capacity(),
            original.capacity()
        );
    }
}

// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The descriptor set, indexed once for the codec.
//!
//! [`crate::descriptor::Schema`] answers by full name, which is a string
//! comparison down a `BTreeMap` for every nested message and every enum the
//! codec meets. The codec meets one per node, so it asks once, here: every
//! message gets a dense index, and every message-typed or enum-typed field
//! carries what its type name resolves to. The answers are the schema's own;
//! nothing here is a second reading of the descriptor set.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::value::write_string;
use crate::descriptor::{Enum, Field, Message, ScalarKind, schema};

/// One message of the schema, with its fields resolved.
#[derive(Debug)]
pub(crate) struct Indexed {
    /// The schema's own message.
    pub(crate) message: &'static Message,
    /// Its fields, in the schema's order (ascending field number).
    pub(crate) fields: Vec<IndexedField>,
    /// Every name a member may use for a field — its `.proto` name and its
    /// JSON name — sorted, each with the position in `fields` of the first
    /// field, in field-number order, that answers to it.
    names: Vec<(&'static str, usize)>,
}

/// One field, with its type resolved.
#[derive(Debug)]
pub(crate) struct IndexedField {
    /// The schema's own field.
    pub(crate) field: &'static Field,
    /// For a message-typed field, the index of its message; `None` for any
    /// other kind, and for a type name the schema does not hold, which the
    /// codec reports where it would have looked the name up.
    pub(crate) nested: Option<usize>,
    /// For an enum-typed field, its enum; `None` otherwise, or when the schema
    /// does not hold the name.
    pub(crate) enumeration: Option<&'static Enum>,
    /// Whether the field has presence in proto3: a `oneof` member (an
    /// `optional` field included, as a synthetic one-field oneof), a
    /// repeated field or a message-typed field. A field without presence is
    /// dropped at its default by prost's encoder.
    pub(crate) presence: bool,
    /// The field's member name as canonical text writes it: quoted, then
    /// `": "`.
    pub(crate) key: String,
}

/// The whole schema, indexed.
#[derive(Debug)]
pub(crate) struct Index {
    messages: Vec<Indexed>,
    by_name: BTreeMap<&'static str, usize>,
}

impl Index {
    /// The index of a message, by full name.
    pub(crate) fn find(&self, full_name: &str) -> Option<usize> {
        self.by_name.get(full_name).copied()
    }

    /// A message by index.
    pub(crate) fn get(&self, index: usize) -> Option<&Indexed> {
        self.messages.get(index)
    }
}

impl Indexed {
    /// The field a JSON member names, by its `.proto` name or its JSON name:
    /// the first field, in field-number order, that answers to either, as
    /// [`Message::field_by_name`] answers.
    pub(crate) fn field_by_name(&self, name: &str) -> Option<&IndexedField> {
        // A binary search of the sorted names: a playbook names a field of a
        // message with dozens of them at every node.
        let at = self
            .names
            .binary_search_by(|(candidate, _)| (*candidate).cmp(name))
            .ok()?;
        let (_, field) = self.names.get(at)?;
        self.fields.get(*field)
    }

    /// The field with this number, as [`Message::field_by_number`] answers.
    pub(crate) fn field_by_number(&self, number: i32) -> Option<&IndexedField> {
        // The schema's fields are in ascending number order, so a binary
        // search finds the same first field a linear one would.
        self.fields
            .binary_search_by_key(&number, |indexed| indexed.field.number)
            .ok()
            .and_then(|at| self.fields.get(at))
    }
}

/// The schema, indexed once per process.
pub(crate) fn index() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(build)
}

fn build() -> Index {
    let schema = schema();
    let mut by_name: BTreeMap<&'static str, usize> = BTreeMap::new();
    let all: Vec<&'static Message> = schema.messages().collect();
    for (at, message) in all.iter().enumerate() {
        by_name.insert(message.full_name.as_str(), at);
    }
    let messages = all
        .iter()
        .map(|message| Indexed {
            message,
            names: names_of(message),
            fields: message
                .fields
                .iter()
                .map(|field| IndexedField {
                    field,
                    nested: if field.kind == ScalarKind::Message {
                        by_name.get(field.type_name.as_str()).copied()
                    } else {
                        None
                    },
                    enumeration: if field.kind == ScalarKind::Enum {
                        schema.enumeration(&field.type_name)
                    } else {
                        None
                    },
                    presence: field.repeated
                        || field.oneof_index.is_some()
                        || field.kind == ScalarKind::Message,
                    key: {
                        let mut key = String::with_capacity(field.name.len().saturating_add(4));
                        write_string(&field.name, &mut key);
                        key.push_str(": ");
                        key
                    },
                })
                .collect(),
        })
        .collect();
    Index { messages, by_name }
}

/// A message's member names, sorted, each with the first field (in the
/// schema's field-number order) that answers to it, which is the field
/// [`Message::field_by_name`]'s linear search finds.
fn names_of(message: &'static Message) -> Vec<(&'static str, usize)> {
    let mut names: Vec<(&'static str, usize)> = Vec::new();
    for (at, field) in message.fields.iter().enumerate() {
        names.push((field.name.as_str(), at));
        names.push((field.json_name.as_str(), at));
    }
    // Sorted by name and then by position, so that the first entry of a name
    // spelled by two fields is the earlier field's, and only it is kept.
    names.sort_unstable();
    names.dedup_by(|later, earlier| later.0 == earlier.0);
    names
}

#[cfg(test)]
mod tests {
    use super::index;
    use crate::descriptor::schema;

    #[test]
    fn the_index_answers_as_the_schema_does() {
        let schema = schema();
        let index = index();
        for message in schema.messages() {
            let at = index
                .find(&message.full_name)
                .expect("every message is indexed");
            let indexed = index.get(at).expect("its index is in range");
            assert_eq!(indexed.message.full_name, message.full_name);
            for field in &message.fields {
                for name in [&field.name, &field.json_name] {
                    assert_eq!(
                        indexed.field_by_name(name).map(|it| it.field.number),
                        message.field_by_name(name).map(|it| it.number),
                    );
                }
                assert_eq!(
                    indexed
                        .field_by_number(field.number)
                        .map(|it| it.field.name.as_str()),
                    message
                        .field_by_number(field.number)
                        .map(|it| it.name.as_str()),
                );
                let found = indexed.field_by_number(field.number).expect("declared");
                assert_eq!(
                    found
                        .nested
                        .and_then(|nested| index.get(nested))
                        .map(|nested| nested.message.full_name.as_str()),
                    schema
                        .message(&field.type_name)
                        .map(|nested| nested.full_name.as_str())
                        .filter(|_| field.kind == crate::descriptor::ScalarKind::Message),
                );
            }
        }
    }
}

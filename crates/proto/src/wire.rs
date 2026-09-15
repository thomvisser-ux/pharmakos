// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Protobuf binary wire format, read and written by hand.
//!
//! Two callers need it and neither can use prost:
//!
//! * [`crate::descriptor`] parses the checked-in descriptor set. It cannot use
//!   `prost-types`, because prost drops unknown fields and the method/scope
//!   table travels as a custom option — an extension, which is an unknown
//!   field to every generated `EnumValueOptions`.
//! * [`crate::json`] converts between canonical proto JSON and wire bytes and
//!   hands the bytes to prost for typed access. Going through the wire format
//!   is what makes one codec cover every message in the schema, instead of one
//!   hand-written mapping per message.
//!
//! Nothing here casts with `as` and nothing divides: the determinism lint set
//! bans both workspace-wide (AGENTS.md sections 4.3 and 4.1), and a wire
//! decoder is exactly the place a silent truncation would hide.

/// A field's wire type, the low three bits of a tag.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum WireType {
    /// Base-128 varint: `int32`, `int64`, `uint32`, `uint64`, `sint*`, `bool`,
    /// `enum`.
    Varint,
    /// Fixed eight bytes, little-endian: `fixed64`, `sfixed64`, `double`.
    Fixed64,
    /// A length prefix then that many bytes: `string`, `bytes`, embedded
    /// messages, packed repeated scalars.
    Delimited,
    /// Fixed four bytes, little-endian: `fixed32`, `sfixed32`, `float`.
    Fixed32,
}

impl WireType {
    pub(crate) const fn from_tag(bits: u64) -> Option<Self> {
        match bits {
            0 => Some(Self::Varint),
            1 => Some(Self::Fixed64),
            2 => Some(Self::Delimited),
            5 => Some(Self::Fixed32),
            // 3 and 4 are the group markers, removed in proto3, and 6 and 7
            // were never assigned. A message carrying one is corrupt.
            _ => None,
        }
    }

    pub(crate) const fn bits(self) -> u64 {
        match self {
            Self::Varint => 0,
            Self::Fixed64 => 1,
            Self::Delimited => 2,
            Self::Fixed32 => 5,
        }
    }
}

/// One field as it sits in the buffer: a payload borrowed from the input.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Value<'a> {
    Varint(u64),
    Fixed64(u64),
    Delimited(&'a [u8]),
    Fixed32(u32),
}

/// What went wrong reading a buffer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum WireError {
    /// The buffer ended in the middle of a value.
    Truncated,
    /// A varint ran past ten bytes, so it cannot fit in 64 bits.
    VarintTooLong,
    /// A group marker, or an unassigned wire type.
    BadWireType,
    /// Field number zero, or a number past the Protobuf maximum.
    BadFieldNumber,
}

/// Reads fields out of a buffer, one at a time, in the order they appear.
#[derive(Debug)]
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub(crate) const fn is_done(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn byte(&mut self) -> Result<u8, WireError> {
        let byte = *self.buf.get(self.pos).ok_or(WireError::Truncated)?;
        self.pos = self.pos.saturating_add(1);
        Ok(byte)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], WireError> {
        let end = self.pos.checked_add(len).ok_or(WireError::Truncated)?;
        let slice = self.buf.get(self.pos..end).ok_or(WireError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    /// The next `len` bytes, borrowed. Used to walk a packed repeated
    /// payload, which carries no tags of its own.
    pub(crate) fn raw(&mut self, len: usize) -> Result<&'a [u8], WireError> {
        self.take(len)
    }

    pub(crate) fn varint(&mut self) -> Result<u64, WireError> {
        let mut value: u64 = 0;
        let mut shift: u32 = 0;
        loop {
            let byte = self.byte()?;
            let payload = u64::from(byte & 0x7f);
            value |= payload.checked_shl(shift).ok_or(WireError::VarintTooLong)?;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift = shift.checked_add(7).ok_or(WireError::VarintTooLong)?;
            if shift >= 64 {
                return Err(WireError::VarintTooLong);
            }
        }
    }

    /// The next field: its number and its payload.
    pub(crate) fn field(&mut self) -> Result<(i32, Value<'a>), WireError> {
        let tag = self.varint()?;
        let wire = WireType::from_tag(tag & 7).ok_or(WireError::BadWireType)?;
        let number_bits = tag >> 3;
        // Protobuf field numbers run 1..=536_870_911.
        if number_bits == 0 || number_bits > 536_870_911 {
            return Err(WireError::BadFieldNumber);
        }
        let number = i32::try_from(number_bits).map_err(|_| WireError::BadFieldNumber)?;
        let value = match wire {
            WireType::Varint => Value::Varint(self.varint()?),
            WireType::Fixed64 => {
                let array: [u8; 8] = self.take(8)?.try_into().map_err(|_| WireError::Truncated)?;
                Value::Fixed64(u64::from_le_bytes(array))
            }
            WireType::Fixed32 => {
                let array: [u8; 4] = self.take(4)?.try_into().map_err(|_| WireError::Truncated)?;
                Value::Fixed32(u32::from_le_bytes(array))
            }
            WireType::Delimited => {
                let len = usize::try_from(self.varint()?).map_err(|_| WireError::Truncated)?;
                Value::Delimited(self.take(len)?)
            }
        };
        Ok((number, value))
    }
}

/// Appends fields to a buffer.
#[derive(Default, Debug)]
pub(crate) struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub(crate) fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    pub(crate) fn varint(&mut self, mut value: u64) {
        loop {
            // `value & 0x7f` is seven bits, so the conversion cannot fail; the
            // fallback keeps the lint set happy without an unwrap.
            let low = u8::try_from(value & 0x7f).unwrap_or(0);
            value >>= 7;
            if value == 0 {
                self.buf.push(low);
                return;
            }
            self.buf.push(low | 0x80);
        }
    }

    fn tag(&mut self, number: i32, wire: WireType) {
        let number_bits = u64::try_from(number).unwrap_or(0);
        self.varint((number_bits << 3) | wire.bits());
    }

    pub(crate) fn varint_field(&mut self, number: i32, value: u64) {
        self.tag(number, WireType::Varint);
        self.varint(value);
    }

    pub(crate) fn fixed64_field(&mut self, number: i32, value: u64) {
        self.tag(number, WireType::Fixed64);
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn fixed32_field(&mut self, number: i32, value: u32) {
        self.tag(number, WireType::Fixed32);
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn delimited_field(&mut self, number: i32, payload: &[u8]) {
        self.tag(number, WireType::Delimited);
        self.varint(u64::try_from(payload.len()).unwrap_or(0));
        self.buf.extend_from_slice(payload);
    }
}

/// Reinterprets the bits of a `u64` as an `i64`. A bit-for-bit move, not a
/// numeric conversion: `as` is denied workspace-wide and `try_from` would
/// reject exactly the values two's complement is meant to carry.
pub(crate) const fn u64_bits_to_i64(value: u64) -> i64 {
    i64::from_ne_bytes(value.to_ne_bytes())
}

/// Reinterprets the bits of an `i64` as a `u64`.
pub(crate) const fn i64_bits_to_u64(value: i64) -> u64 {
    u64::from_ne_bytes(value.to_ne_bytes())
}

/// `ZigZag`, for `sint32` and `sint64`.
pub(crate) const fn zigzag_decode(value: u64) -> i64 {
    let magnitude = u64_bits_to_i64(value >> 1);
    if value & 1 == 0 {
        magnitude
    } else {
        !magnitude
    }
}

/// `ZigZag`, the other way.
pub(crate) const fn zigzag_encode(value: i64) -> u64 {
    // Shifts, not arithmetic: `<<` never trips an overflow check, and `>> 63`
    // on a signed value is the sign mask.
    (i64_bits_to_u64(value) << 1) ^ i64_bits_to_u64(value >> 63)
}

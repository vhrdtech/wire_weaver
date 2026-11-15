//! What a sample is: its element type and channels.

use shrink_wrap::prelude::*;

/// The logical type of one value of a sample.
///
/// Its number (the enum's discriminant, [Element::code]) is the element code every block uses: summary heads name
/// their element by it.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq, Hash),
    ww_repr = u8
)]
pub enum Element {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    /// An I/Q pair of `i16`, I first.
    ComplexI16,
    /// An I/Q pair of `f32`, I first.
    ComplexF32,
    /// A bit; stored 8 per byte, the first in bit 0.
    Bool,
}

impl Element {
    /// Every element, by code.
    pub const ALL: [Element; 13] = [
        Element::I8,
        Element::I16,
        Element::I32,
        Element::I64,
        Element::U8,
        Element::U16,
        Element::U32,
        Element::U64,
        Element::F32,
        Element::F64,
        Element::ComplexI16,
        Element::ComplexF32,
        Element::Bool,
    ];

    /// The element code: 0 `I8` to 12 `Bool`, in declaration order; a newer writer's element keeps its code.
    pub const fn code(self) -> u8 {
        match self {
            Element::I8 => 0,
            Element::I16 => 1,
            Element::I32 => 2,
            Element::I64 => 3,
            Element::U8 => 4,
            Element::U16 => 5,
            Element::U32 => 6,
            Element::U64 => 7,
            Element::F32 => 8,
            Element::F64 => 9,
            Element::ComplexI16 => 10,
            Element::ComplexF32 => 11,
            Element::Bool => 12,
            Element::Unknown(code) => code as u8,
        }
    }

    /// The element of a code this reader knows, None for any other.
    pub const fn from_code(code: u8) -> Option<Element> {
        if (code as usize) < Self::ALL.len() {
            Some(Self::ALL[code as usize])
        } else {
            None
        }
    }

    /// The type a value is stored as: the part of a complex element, else the element itself.
    pub const fn scalar(self) -> Element {
        match self {
            Element::ComplexI16 => Element::I16,
            Element::ComplexF32 => Element::F32,
            other => other,
        }
    }

    /// Scalars per element: 2 for a complex element, else 1.
    pub const fn parts(self) -> u32 {
        match self {
            Element::ComplexI16 | Element::ComplexF32 => 2,
            _ => 1,
        }
    }

    /// Bits of one scalar of the element, 0 for an element this reader doesn't know.
    pub const fn scalar_bits(self) -> u32 {
        match self.scalar() {
            Element::Bool => 1,
            Element::I8 | Element::U8 => 8,
            Element::I16 | Element::U16 => 16,
            Element::I32 | Element::U32 | Element::F32 => 32,
            Element::I64 | Element::U64 | Element::F64 => 64,
            _ => 0,
        }
    }

    /// Whether the scalar is a signed integer.
    pub const fn is_signed(self) -> bool {
        matches!(
            self.scalar(),
            Element::I8 | Element::I16 | Element::I32 | Element::I64
        )
    }

    /// Whether the scalar is a float.
    pub const fn is_float(self) -> bool {
        matches!(self.scalar(), Element::F32 | Element::F64)
    }
}

/// What one sample of a series column is: `channels` elements, interleaved.
///
/// A column holds one channel or several interleaved; the [Codec](crate::Codec) of the column says how the values
/// are coded, apart from this.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq, Hash)
)]
pub struct SampleShape {
    pub element: Element,
    /// Elements per sample, 1 and up.
    pub channels: UNib32,
}

impl SampleShape {
    /// One channel of `element`.
    pub const fn scalar(element: Element) -> Self {
        SampleShape {
            element,
            channels: UNib32(1),
        }
    }

    /// `channels` channels of `element`.
    pub const fn new(element: Element, channels: u32) -> Self {
        SampleShape {
            element,
            channels: UNib32(channels),
        }
    }

    /// Scalars per sample (channels times [Element::parts]), None when 0 or past u32.
    pub const fn lanes(&self) -> Option<u32> {
        match self.channels.0.checked_mul(self.element.parts()) {
            Some(0) | None => None,
            Some(n) => Some(n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_declaration_order() {
        for (i, e) in Element::ALL.iter().enumerate() {
            assert_eq!(e.code() as usize, i);
            assert_eq!(Element::from_code(i as u8), Some(*e));
        }
        assert_eq!(Element::from_code(13), None);
        assert_eq!(Element::Unknown(14).code(), 14);
    }

    #[test]
    fn element_bytes_are_its_code() {
        // the element is a u8 discriminant on the wire, so its byte is the code
        for e in Element::ALL {
            let mut buf = [0u8; 8];
            let bytes = e.to_ww_bytes(&mut buf).unwrap();
            assert_eq!(bytes, [e.code()], "{e:?}");
        }
        let shape = SampleShape::new(Element::ComplexI16, 3);
        assert_eq!(shape.lanes(), Some(6));
        assert_eq!(SampleShape::new(Element::F32, 0).lanes(), None);
    }
}

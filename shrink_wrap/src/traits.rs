use crate::{BufReader, BufWriter, Error};
use paste::paste;

pub trait SerializeShrinkWrap {
    const ELEMENT_SIZE: ElementSize;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error>;

    fn to_ww_bytes<'i>(&self, buf: &'i mut [u8]) -> Result<&'i [u8], Error> {
        let mut wr = BufWriter::new(buf);
        self.ser_shrink_wrap(&mut wr)?;
        wr.finish_and_take()
    }
}

pub trait DeserializeShrinkWrap<'i>: Sized {
    const ELEMENT_SIZE: ElementSize;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error>;

    fn from_ww_bytes(buf: &'i [u8]) -> Result<Self, Error> {
        let mut rd = BufReader::new(buf);
        let value = Self::des_shrink_wrap(&mut rd)?;
        Ok(value)
    }
}

// pub fn to_ww_bytes<'i, T: SerializeShrinkWrap>(
//     buf: &'i mut [u8],
//     value: &T,
// ) -> Result<&'i [u8], Error> {
//     let mut wr = BufWriter::new(buf);
//     value.ser_shrink_wrap(&mut wr)?;
//     wr.finish_and_take()
// }

// pub fn from_ww_bytes<'i, T: DeserializeShrinkWrap<'i>>(buf: &'i [u8]) -> Result<T, Error> {
//     let mut rd = BufReader::new(buf);
//     let value = T::des_shrink_wrap(&mut rd)?;
//     Ok(value)
// }

/// Core type governing how objects are serialized and composed together.
///
/// Structs and enums are Unsized by default to promote potential future changes with backwards and forwards compatibility.
/// When an Unsized object is serialized, [write](BufWriter::write) reserves a new size slot in the back of the buffer.
/// Then it serializes the object and encodes any potential size slots that any of child objects might have used.
/// Finally, it updates the size slot with an actual object size (but does not encode it).
///
/// Sized and SelfDescribing objects are laid out as is, without serializing size.
///
/// UnsizedFinalStructure is almost like Unsized, but postpones the size calculation and encoding to a parent object.
/// Another way to look at it is its like a "flattening" operation - child Unsized objects are using the same size space at the back of the buffer.
///
/// As an example, consider Vec<T> - it is marked UnsizedFinalStructure. String is Unsized.
/// `Vec<Vec<String>>` is serialized as: `[s00 s01 s10 s11 s11_len s10_len s1_len s01_len s00_len s0_len outer_len]`.
/// If Vec where Unsized instead, then [write](BufWriter::write) would calculate the size in bytes of each serialized sub-vector as well.
/// Serialized buffer would then be: `[s00 s01 s01_len s00_len s0_len s10 s11 s11_len s10_len s1_len s1_size_bytes s0_size_bytes outer_len]`.
/// More space is used to encode lengths in bytes, in addition, padding nibbles will be inserted so that objects are on byte-boundaries,
/// wasting space further.
/// See tests, there is one with Vec<Vec<String>>.
///
/// This works for both owned and borrowed objects on std and on no_std without allocator.
///
/// `#[sized]` or `#[self_describing]` can be used to "lower" the requirement to save space (const check will be added to ensure that).
///
/// Size requirement of a struct or enum is bumped from Sized to SelfDescribing to Unsized if
/// any field or variant is a higher "requirement" than others.
///
/// UnsizedFinalStructure can only be requested manually using `#[final_structure]` to save some space at the cost of no further changes to Unsized
/// fields without breaking compatibility.
/// Only Unsized or UFS objects can contain UFS objects.
///
/// Calculations are done during compile time thanks to const evaluation and static asserts are inserted to ensure correct behavior.
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ElementSize {
    /// Element size is unknown and stored at the back of the buffer.
    /// Unsized structs and enums can be evolved over time without breaking compatibility.
    ///
    /// If Unsized value is stored in a parent type (struct or enum),
    /// parent must encode all reverse UNib32's and overall object size when serializing.
    /// Symmetrically deserializer must read a size and use split before deserializing an Unsized object.
    Unsized,

    /// Element size is unknown, but its structure is not going to change, unless a major version is bumped.
    ///
    /// Only the first evolution of a type can be UnsizedFinalStructure. Unsized cannot be marked UnsizedFinalStructure,
    /// without breaking compatibility. So this should be used with care, only for small types that are very likely not
    /// going to change.
    ///
    /// If UnsizedFinalStructure is stored in a parent type (struct or enum), parent must NOT encode reverse UNib32 sizes
    /// of such an object, nor its overall size. Deserializing works without reading the object's size.
    /// UnsizedFinalStructure type is essentially flattened onto a parent type.
    /// Reverse UNib32 sizes in the back of a buffer are shared with a parent type.
    UnsizedFinalStructure,

    /// Element's size is unknown, but deserializer is able to infer it from data itself (UNib32, LEB).
    /// Element size is not stored.
    SelfDescribing,

    /// Element size is known and not stored in a buffer. Actual size in bits is not currently used for anything meaningful,
    /// so some types are incorrectly reporting zero.
    Sized { size_bits: usize },
}

impl ElementSize {
    pub const fn add(&self, other: ElementSize) -> ElementSize {
        // Order is very important here, size requirement is bumped from Sized to SelfDescribing to Unsized.
        // UFS is a bit tricky, it is "contagious", so that Vec<T> with T Unsized is UFS.
        // Note that structs and enums cannot accidentally become UFS, because by default they are Unsized, and no sum operations are
        // performed, otherwise it would have been a compatibility problem.
        match (self, other) {
            (ElementSize::UnsizedFinalStructure, _) => ElementSize::UnsizedFinalStructure,
            (_, ElementSize::UnsizedFinalStructure) => ElementSize::UnsizedFinalStructure,
            (ElementSize::Unsized, _) => ElementSize::Unsized,
            (_, ElementSize::Unsized) => ElementSize::Unsized,
            (ElementSize::SelfDescribing, _) => ElementSize::SelfDescribing,
            (_, ElementSize::SelfDescribing) => ElementSize::SelfDescribing,
            (
                ElementSize::Sized { size_bits: size_a },
                ElementSize::Sized { size_bits: size_b },
            ) => ElementSize::Sized {
                size_bits: *size_a + size_b,
            },
        }
    }

    pub const fn is_unsized(&self) -> bool {
        matches!(self, ElementSize::Unsized)
    }

    pub const fn bits(size_bits: usize) -> ElementSize {
        ElementSize::Sized { size_bits }
    }

    pub fn discriminant(&self) -> u8 {
        unsafe { *<*const _>::from(self).cast::<u8>() }
    }
}

impl SerializeShrinkWrap for ElementSize {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 2 };

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_un8(2, self.discriminant())
    }
}

impl<'i> DeserializeShrinkWrap<'i> for ElementSize {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 2 };

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        let discriminant = rd.read_un8(2)?;
        Ok(match discriminant {
            0 => ElementSize::Unsized,
            1 => ElementSize::UnsizedFinalStructure,
            2 => ElementSize::SelfDescribing,
            3 => ElementSize::Sized { size_bits: 0 },
            _ => {
                return Err(Error::EnumFutureVersionOrMalformedData);
            }
        })
    }
}

impl SerializeShrinkWrap for bool {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 1 };

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_bool(*self)
    }
}

macro_rules! impl_serialize {
    ($sign:ident, $bits:literal) => {
        paste! {
            impl SerializeShrinkWrap for [<$sign $bits>] {
                const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: $bits };

                fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
                    wr.[<write_ $sign $bits>](*self)
                }
            }
        }
    };
}
impl_serialize!(u, 8);
impl_serialize!(u, 16);
impl_serialize!(u, 32);
impl_serialize!(u, 64);
impl_serialize!(u, 128);
impl_serialize!(i, 8);
impl_serialize!(i, 16);
impl_serialize!(i, 32);
impl_serialize!(i, 64);
impl_serialize!(i, 128);
impl_serialize!(f, 32);
impl_serialize!(f, 64);

impl<'i> DeserializeShrinkWrap<'i> for bool {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 1 };

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        rd.read_bool()
    }
}

macro_rules! impl_deserialize {
    ($sign:ident, $bits:literal) => {
        paste! {
            impl<'i> DeserializeShrinkWrap<'i> for [<$sign $bits>] {
                const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: $bits };

                fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
                    rd.[<read_ $sign $bits>]()
                }
            }
        }
    };
}
impl_deserialize!(u, 8);
impl_deserialize!(u, 16);
impl_deserialize!(u, 32);
impl_deserialize!(u, 64);
impl_deserialize!(u, 128);
impl_deserialize!(i, 8);
impl_deserialize!(i, 16);
impl_deserialize!(i, 32);
impl_deserialize!(i, 64);
impl_deserialize!(i, 128);
impl_deserialize!(f, 32);
impl_deserialize!(f, 64);

impl SerializeShrinkWrap for &'_ str {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_raw_str(self)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for &'i str {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        rd.read_raw_str()
    }
}

impl<T: SerializeShrinkWrap> SerializeShrinkWrap for Option<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        match self {
            Some(val) => {
                wr.write_bool(true)?;
                wr.write(val)
            }
            None => wr.write_bool(false),
        }
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for Option<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        let is_some = rd.read_bool()?;
        if is_some {
            Ok(Some(rd.read()?))
        } else {
            Ok(None)
        }
    }
}

impl<T: SerializeShrinkWrap, E: SerializeShrinkWrap> SerializeShrinkWrap for Result<T, E> {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        match self {
            Ok(val) => {
                wr.write_bool(true)?;
                wr.write(val)
            }
            Err(err) => {
                wr.write_bool(false)?;
                wr.write(err)
            }
        }
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>, E: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i>
    for Result<T, E>
{
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        let is_ok = rd.read_bool()?;
        if is_ok {
            Ok(Ok(rd.read()?))
        } else {
            Ok(Err(rd.read()?))
        }
    }
}

macro_rules! impl_tuple {
    ($($types:ident),* ; $($indices: literal),*) => {
        paste! {
            impl<$($types: SerializeShrinkWrap),*> SerializeShrinkWrap for ($($types),*) {
                const ELEMENT_SIZE: ElementSize = add_recursive!($($types),*);

                fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
                    $(wr.write(&self.$indices)?;)*
                    Ok(())
                }
            }

            impl<'i, $($types: DeserializeShrinkWrap<'i>),*> DeserializeShrinkWrap<'i>
                for ($($types),*)
            {
                const ELEMENT_SIZE: ElementSize = add_recursive!($($types),*);

                fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
                    $(let [<_ $indices>] = rd.read()?;)*
                    Ok(( $([<_ $indices>]),* ))
                }
            }
        }
    };
}

macro_rules! add_recursive {
    () => {};
    ($t: ident) => { $t::ELEMENT_SIZE };
    ($t: ident, $($types: ident),*) => { $t::ELEMENT_SIZE.add(add_recursive!($($types),*)) };
}

impl_tuple!(A, B; 0, 1);
impl_tuple!(A, B, C; 0, 1, 2);
impl_tuple!(A, B, C, D; 0, 1, 2, 3);
impl_tuple!(A, B, C, D, E; 0, 1, 2, 3, 4);
impl_tuple!(A, B, C, D, E, F; 0, 1, 2, 3, 4, 5);
impl_tuple!(A, B, C, D, E, F, G; 0, 1, 2, 3, 4, 5, 6);
impl_tuple!(A, B, C, D, E, F, G, H; 0, 1, 2, 3, 4, 5, 6, 7);

impl<const N: usize, T: SerializeShrinkWrap> SerializeShrinkWrap for [T; N] {
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        for elem in self {
            wr.write(elem)?;
        }
        Ok(())
    }
}

impl<'i, const N: usize, T: DeserializeShrinkWrap<'i> + Default + Copy> DeserializeShrinkWrap<'i>
    for [T; N]
{
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        let mut array: [T; N] = [T::default(); N];
        for i in 0..N {
            array[i] = rd.read()?;
        }

        // let mut array: [MaybeUninit<T>; N] = unsafe { MaybeUninit::uninit().assume_init() };
        //
        // for i in 0..N {
        //     let elem = rd.read()?;
        //     array[i] = MaybeUninit::new(elem);
        // }
        //
        // let array = unsafe { core::mem::transmute::<_, [T; N]>(array) };
        Ok(array)
    }
}

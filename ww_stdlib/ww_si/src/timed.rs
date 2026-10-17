//! [Timed]: a value with the time it was taken.

use crate::Second;
use shrink_wrap::prelude::*;
use shrink_wrap::{ElementSize, write_pushes_len};

/// A value with the time it was taken: a sample of a property, an item of a stream (`Timed<Ampere>`,
/// `Timed<IvSample>`).
///
/// `t` is a device timestamp, as [Second]'s docs say: the device's time since its clock started, at a prefix and
/// storage that stay the same for a given property or stream (`Second { Micro, U32 }` wraps every 71.6 minutes, the
/// host gets the full time back with [Unwrapper](crate::Unwrapper)).
///
/// On the wire it is a `final_structure` struct of `t` and then `value`: no fields can be added later. Written by
/// hand, `#[derive_shrink_wrap]` has no type parameters; the bytes are what the derive gives a non-generic struct of
/// the same two fields.
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Timed<T> {
    /// When `value` was taken, device time.
    pub t: Second,
    pub value: T,
}

impl<T> Timed<T> {
    pub const fn new(t: Second, value: T) -> Self {
        Timed { t, value }
    }
}

impl<T: SerializeShrinkWrap> SerializeShrinkWrap for Timed<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure
        .add(<Second as SerializeShrinkWrap>::ELEMENT_SIZE)
        .add(T::ELEMENT_SIZE);
    const PUSHES_LEN: bool = write_pushes_len::<Second>() || write_pushes_len::<T>();
    const MAX_SIZE: Option<MaxSize> = MaxSize::seq(
        MaxSize::seq(Some(MaxSize::ZERO), MaxSize::of_write::<Second>()),
        MaxSize::of_write::<T>(),
    );

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), ShrinkWrapError> {
        wr.write(&self.t)?;
        wr.write(&self.value)?;
        wr.latched()
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for Timed<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure
        .add(<Second as DeserializeShrinkWrap>::ELEMENT_SIZE)
        .add(T::ELEMENT_SIZE);

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, ShrinkWrapError> {
        let t = rd.read_field()?;
        let value = rd.read_field()?;
        rd.latched()?;
        Ok(Timed { t, value })
    }
}

#[cfg(feature = "std")]
impl<T: SerializeShrinkWrapOwned> SerializeShrinkWrapOwned for Timed<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure
        .add(<Second as SerializeShrinkWrapOwned>::ELEMENT_SIZE)
        .add(T::ELEMENT_SIZE);

    fn ser_shrink_wrap_owned(&self, wr: &mut BufWriterOwned) -> Result<(), ShrinkWrapError> {
        wr.write(&self.t)?;
        wr.write(&self.value)?;
        wr.latched()
    }
}

#[cfg(feature = "std")]
impl<T: DeserializeShrinkWrapOwned> DeserializeShrinkWrapOwned for Timed<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure
        .add(<Second as DeserializeShrinkWrapOwned>::ELEMENT_SIZE)
        .add(T::ELEMENT_SIZE);

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, ShrinkWrapError> {
        let t = rd.read_field_owned()?;
        let value = rd.read_field_owned()?;
        rd.latched()?;
        Ok(Timed { t, value })
    }
}

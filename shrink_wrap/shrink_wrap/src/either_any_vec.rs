use crate::buf_writer::BufWriterState;
use crate::traits::ElementSize;
use crate::{BufReader, BufWriter, DeserializeShrinkWrap, Error, SerializeShrinkWrap};
use either::Either;

/// Zero-copy, no_std and no-alloc array of `Either::L(Any), R(Any)`.
///
/// LR flags are stored in-line with data in groups of 8:
/// `|1B: flags|[up to 8 objects: false: L bytes, true: R bytes]|1B: flags|...|dynamic object sizes (reverse UNib32's)|`
///
/// Each element can be of any type that implements [DeserializeShrinkWrap].
/// Including user defined structs with dynamic size, etc.
/// Elements don't have to be of the same type, hence there is no generic parameter.
///
/// Note that EitherVec is a low-level machinery that requires knowledge of types for each element, but it can allow for
/// pretty neat optimizations. Length is not stored, as it is implied from type knowledge.
/// It is used to implemenent multi-calls and multi-read/write operations with arbitratry types in wire_weaver (storing `Result<Any, E>`).
///
/// Use [EitherAnyVecWriter] to construct the array.
#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct EitherAnyVec<'i> {
    data: &'i [u8],
}

/// Zero-copy, no_std and no-alloc array writer of `Either::L(Any), R(Any)` elements.
///
/// Each element can be of any type that implements [SerializeShrinkWrap].
/// Including user defined structs with dynamic size, etc.
/// Elements don't have to be of the same type, hence there is no generic parameter.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct EitherAnyVecWriter<'i> {
    data: &'i mut [u8],
    flags: Option<BufWriterState>,
    items: BufWriterState,
}

impl<'i> EitherAnyVec<'i> {
    pub fn new(data: &'i [u8]) -> Self {
        Self { data }
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn iter(&self) -> EitherVecIter<'i> {
        EitherVecIter {
            flags: None,
            rd: BufReader::new(self.data),
        }
    }
}

impl<'i> EitherAnyVecWriter<'i> {
    pub fn new(data: &'i mut [u8]) -> Self {
        let wr = BufWriter::new(data);
        let items = wr.save_state();
        let data = wr.deinit();
        Self {
            data,
            flags: None,
            items,
        }
    }

    pub fn write<L: SerializeShrinkWrap, R: SerializeShrinkWrap>(
        &mut self,
        elem: Either<L, R>,
    ) -> Result<(), Error> {
        let is_r = matches!(elem, Either::Right(_));
        if let Some(flags) = self.flags {
            let replenish_flags = flags.bits_in_byte_left() == 1;
            let mut wr = BufWriter::new(self.data);
            wr.restore_state(flags);
            wr.write_bool(is_r)?;
            if replenish_flags {
                self.flags = None;
            } else {
                self.flags = Some(wr.save_state());
            }
        } else {
            // on each flag group
            let mut wr = BufWriter::new(self.data);
            wr.restore_state(self.items);
            wr.align_byte();
            wr.write_bool(is_r)?;
            self.flags = Some(wr.save_state());
            wr.align_byte();
            self.items = wr.save_state();
        }

        let mut wr = BufWriter::new(self.data);
        wr.restore_state(self.items);
        match &elem {
            Either::Left(l) => wr.write(l)?,
            Either::Right(r) => wr.write(r)?,
        }
        self.items = wr.save_state();
        Ok(())
    }

    pub fn finish_and_take(self) -> Result<&'i [u8], Error> {
        let mut wr = BufWriter::new(self.data);
        wr.restore_state(self.items);
        wr.finish_and_take()
    }
}

impl<'i> SerializeShrinkWrap for EitherAnyVec<'i> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_raw_slice(self.data)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for EitherAnyVec<'i> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        Ok(EitherAnyVec {
            data: rd.into_raw_slice()?,
        })
    }
}

pub struct EitherVecIter<'i> {
    flags: Option<BufReader<'i>>,
    rd: BufReader<'i>,
}

impl<'i> EitherVecIter<'i> {
    /// Read the next flag and element with any `L` and `R` types specific to this call only.
    pub fn next<L: DeserializeShrinkWrap<'i>, R: DeserializeShrinkWrap<'i>>(
        &mut self,
    ) -> Result<Either<L, R>, Error> {
        let replenish_flags = if let Some(flags) = self.flags {
            flags.bits_left() == 0
        } else {
            true
        };
        if replenish_flags {
            let byte = self.rd.read_raw_slice(1)?;
            self.flags = Some(BufReader::new(byte));
        }
        // NOTE(safety): flags is Some as if it is None, it is initialized above
        // read_bool() must return Ok(_) since we checked the number of bits left as well
        let is_r = unsafe {
            self.flags
                .as_mut()
                .unwrap_unchecked()
                .read_bool()
                .unwrap_unchecked()
        };
        if is_r {
            Ok(Either::Right(self.rd.read()?))
        } else {
            Ok(Either::Left(self.rd.read()?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;

    const NUMBERS: &[u8] = &[
        0b0101_1100, // LRLR_RRLL
        0b0111_1101,
        0xAB,
        0xCD,
        0xEF,
        0xAA,
        0xBF,
        0b1010_0011, // RLRL_LLRR
        0b1111_1000,
    ];

    #[test]
    fn numbers() {
        let mut rd = BufReader::new(NUMBERS);
        let ev = EitherAnyVec::des_shrink_wrap(&mut rd).unwrap();
        let mut iter = ev.iter();

        assert_eq!(iter.next::<bool, bool>(), Ok(Either::Left(false)));
        assert_eq!(iter.next::<bool, U3>(), Ok(Either::Right(U3::max())));
        assert_eq!(
            iter.next::<U4, ()>(),
            Ok(Either::Left(U4::new(0b1101).unwrap()))
        );
        assert_eq!(iter.next::<(), u8>(), Ok(Either::Right(0xAB)));
        assert_eq!(iter.next::<(), u16>(), Ok(Either::Right(0xEFCD)));
        assert_eq!(iter.next::<(), u8>(), Ok(Either::Right(0xAA)));
        assert_eq!(
            iter.next::<Nibble, ()>(),
            Ok(Either::Left(Nibble::new(0xB).unwrap()))
        );
        assert_eq!(
            iter.next::<Nibble, ()>(),
            Ok(Either::Left(Nibble::new(0xF).unwrap()))
        );

        assert_eq!(iter.next::<(), bool>(), Ok(Either::Right(true)));
        assert_eq!(iter.next::<bool, ()>(), Ok(Either::Left(true)));
        assert_eq!(iter.next::<(), bool>(), Ok(Either::Right(true)));
        assert_eq!(iter.next::<bool, ()>(), Ok(Either::Left(true)));
        assert_eq!(iter.next::<bool, ()>(), Ok(Either::Left(true)));
        assert_eq!(iter.next::<bool, ()>(), Ok(Either::Left(false)));
        assert_eq!(iter.next::<(), bool>(), Ok(Either::Right(false)));
        assert_eq!(iter.next::<(), bool>(), Ok(Either::Right(false)));

        assert_eq!(
            iter.next::<(), ()>(),
            Err(crate::Error::OutOfBoundsReadRawSlice)
        );
    }

    #[test]
    fn write_numbers() {
        let mut buf = [0u8; 16];
        let mut wr = EitherAnyVecWriter::new(&mut buf);
        wr.write::<_, ()>(Either::Left(false)).unwrap();
        wr.write::<(), _>(Either::Right(U3::max())).unwrap();
        wr.write::<_, ()>(Either::Left(U4::new(0b1101).unwrap()))
            .unwrap();
        wr.write::<(), u8>(Either::Right(0xAB)).unwrap();
        wr.write::<(), u16>(Either::Right(0xEFCD)).unwrap();
        wr.write::<(), u8>(Either::Right(0xAA)).unwrap();
        wr.write::<_, ()>(Either::Left(Nibble::new(0xB).unwrap()))
            .unwrap();
        wr.write::<_, ()>(Either::Left(Nibble::max())).unwrap();

        wr.write::<(), _>(Either::Right(true)).unwrap();
        wr.write::<_, ()>(Either::Left(true)).unwrap();
        wr.write::<(), _>(Either::Right(true)).unwrap();
        wr.write::<_, ()>(Either::Left(true)).unwrap();
        wr.write::<_, ()>(Either::Left(true)).unwrap();
        wr.write::<_, ()>(Either::Left(false)).unwrap();
        wr.write::<(), _>(Either::Right(false)).unwrap();
        wr.write::<(), _>(Either::Right(false)).unwrap();

        let data = wr.finish_and_take().unwrap();
        assert_eq!(data, NUMBERS);
    }

    const RESULT_VEC: &[u8] = &[
        0b1110_0000, // RRRL_eeee
        0xAA,
        0xBB,
        0xCC,
        0xDD,
        0xEE,
        0xFF,
        0x40, // MyError (1B because it is Unsized)
        0x11, // lengths from the back: 3, 2, 1, 1
        0x23,
    ];

    #[derive_shrink_wrap]
    #[ww_repr(nib)]
    #[derive(Debug, PartialEq, Eq)]
    enum MyError {
        A,
        B,
        C,
        D,
        E,
    }

    #[test]
    fn result_vec_write() {
        let mut buf = [0u8; 64];
        let mut wr = EitherAnyVecWriter::new(&mut buf);
        wr.write::<MyError, Vec<u8>>(Either::Right(vec![0xAA, 0xBB, 0xCC]))
            .unwrap();
        wr.write::<MyError, Vec<u8>>(Either::Right(vec![0xDD, 0xEE]))
            .unwrap();
        wr.write::<MyError, Vec<u8>>(Either::Right(vec![0xFF]))
            .unwrap();
        wr.write::<_, ()>(Either::Left(MyError::E)).unwrap();

        let data = wr.finish_and_take().unwrap();
        assert_eq!(data, RESULT_VEC);
    }

    #[test]
    fn result_vec() {
        let mut rd = BufReader::new(RESULT_VEC);
        let ev = EitherAnyVec::des_shrink_wrap(&mut rd).unwrap();
        let mut iter = ev.iter();

        assert_eq!(
            iter.next::<MyError, Vec<u8>>(),
            Ok(Either::Right(vec![0xAA, 0xBB, 0xCC]))
        );
        assert_eq!(
            iter.next::<MyError, Vec<u8>>(),
            Ok(Either::Right(vec![0xDD, 0xEE]))
        );
        assert_eq!(
            iter.next::<MyError, Vec<u8>>(),
            Ok(Either::Right(vec![0xFF]))
        );
        assert_eq!(iter.next::<MyError, ()>(), Ok(Either::Left(MyError::E)));
    }
}

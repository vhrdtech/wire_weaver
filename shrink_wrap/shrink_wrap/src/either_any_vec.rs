#[cfg(feature = "std")]
use crate::DeserializeShrinkWrapOwned;
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
/// Use [EitherAnyVecWriter] or [EitherAnyVecBuilder] to construct the array.
#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct EitherAnyVec<'i> {
    data: &'i [u8],
}

/// Iterator over EitherAnyVec
pub struct EitherAnyVecIter<'i> {
    flags: Option<BufReader<'i>>,
    rd: BufReader<'i>,
}

/// no_std and no-alloc array writer of `Either::L(Any), R(Any)` elements.
///
/// Each element is written in 1 stage with the call to [EitherAnyVecWriter::write].
///
/// Elements can be of any type that implements [SerializeShrinkWrap].
/// Including user defined structs with dynamic size, etc.
/// Elements don't have to be of the same type, hence there is no generic parameter.
pub struct EitherAnyVecWriter<'i> {
    data: &'i mut [u8],
    builder: EitherAnyVecBuilder,
}

/// no_std and no-alloc array builder of `Either::L(Any), R(Any)` elements.
///
/// Each element is written in 3 stages: `start`, `write` (with regular BufWriter), `finish`.
/// Information about whether written element was actually left or right is only required at the `finish` stage.
/// This allows getting mutable BufWriter without closures and keeps borrow checker happy.
pub struct EitherAnyVecBuilder {
    // unsized_builder: UnsizedBuilder,
    flags: Option<BufWriterState>,
    items: BufWriterState,
}

/// Special type returned by [EitherAnyVecBuilder::write_item_start] and consumed by [EitherAnyVecBuilder::write_item_finish]
pub struct EitherAnyVecMarker {
    flags: BufWriterState,
}

impl<'i> EitherAnyVec<'i> {
    pub fn new(data: &'i [u8]) -> Self {
        Self { data }
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn iter(&self) -> EitherAnyVecIter<'i> {
        EitherAnyVecIter {
            flags: None,
            rd: BufReader::new(self.data),
        }
    }
}

impl EitherAnyVecBuilder {
    pub fn new(wr: &BufWriter<'_>) -> Self {
        // let wr = BufWriter::new(buf);
        // let unsized_builder = UnsizedBuilder::new(&mut wr)?;
        let items = wr.save_state();
        Self {
            // unsized_builder,
            flags: None,
            items,
        }
    }

    fn write_flag(
        &mut self,
        is_right: bool,
        wr: &mut BufWriter<'_>,
    ) -> Result<EitherAnyVecMarker, Error> {
        let marker = if let Some(flags) = self.flags {
            let marker = flags;
            let replenish_flags = flags.bits_in_byte_left() == 1;
            wr.restore_state(flags);
            wr.write_bool(is_right)?;
            if replenish_flags {
                self.flags = None;
            } else {
                self.flags = Some(wr.save_state());
            }
            marker
        } else {
            // on each flag group
            wr.restore_state(self.items);
            wr.align_byte();
            let marker = wr.save_state();
            wr.write_bool(is_right)?;
            self.flags = Some(wr.save_state());
            wr.align_byte();
            self.items = wr.save_state();
            marker
        };
        Ok(EitherAnyVecMarker { flags: marker })
    }

    /// Begin writing a new item, after this call a new flag is allocated in the buffer.
    /// It is not yet known whether left or right item is going to be written next.
    pub fn write_item_start(
        &mut self,
        wr: &mut BufWriter<'_>,
    ) -> Result<EitherAnyVecMarker, Error> {
        let marker = self.write_flag(false, wr)?;
        wr.restore_state(self.items);
        Ok(marker)
    }

    /// Serialize the next item using wr BufWriter itself.
    /// Then call this method, providing marker from [Self::write_item_start] and whether left or right item was written.
    pub fn write_item_finish(
        &mut self,
        marker: EitherAnyVecMarker,
        is_right: bool,
        wr: &mut BufWriter<'_>,
    ) {
        self.items = wr.save_state();
        wr.restore_state(marker.flags);
        let _ = wr.write_bool(is_right);
    }

    /// Write left item in one step.
    pub fn write_left<L: SerializeShrinkWrap>(
        &mut self,
        item_left: &L,
        wr: &mut BufWriter<'_>,
    ) -> Result<(), Error> {
        self.write_flag(false, wr)?;
        wr.restore_state(self.items);
        wr.write(item_left)?;
        self.items = wr.save_state();
        Ok(())
    }

    /// Write right item in one step.
    pub fn write_right<R: SerializeShrinkWrap>(
        &mut self,
        item_right: &R,
        wr: &mut BufWriter<'_>,
    ) -> Result<(), Error> {
        self.write_flag(true, wr)?;
        wr.restore_state(self.items);
        wr.write(item_right)?;
        self.items = wr.save_state();
        Ok(())
    }

    pub fn finish(self, wr: &mut BufWriter<'_>) {
        wr.restore_state(self.items);
    }

    /// Finalize the BufWriter and get result bytes
    pub fn finish_and_take(self, mut wr: BufWriter<'_>) -> Result<&[u8], Error> {
        wr.restore_state(self.items);
        // self.unsized_builder.finish(&mut wr)?;
        wr.finish_and_take()
    }
}

impl<'i> EitherAnyVecWriter<'i> {
    pub fn new(data: &'i mut [u8]) -> Result<Self, Error> {
        let wr = BufWriter::new(data);
        let builder = EitherAnyVecBuilder::new(&wr);
        let data = wr.deinit();
        Ok(Self { data, builder })
    }

    /// Write either left or right item to the buffer.
    pub fn write<L: SerializeShrinkWrap, R: SerializeShrinkWrap>(
        &mut self,
        elem: Either<L, R>,
    ) -> Result<(), Error> {
        let is_right = matches!(elem, Either::Right(_));
        let mut wr = BufWriter::new(self.data);
        self.builder.write_flag(is_right, &mut wr)?;

        wr.restore_state(self.builder.items);
        match &elem {
            Either::Left(l) => wr.write(l)?,
            Either::Right(r) => wr.write(r)?,
        }
        self.builder.items = wr.save_state();
        Ok(())
    }

    /// Write an item inside a closure first and then update the flag.
    /// Closure must return an `is_right` boolean.
    pub fn write_with<F: FnMut(&mut BufWriter<'_>) -> bool>(
        &mut self,
        mut f: F,
    ) -> Result<(), Error> {
        let mut wr = BufWriter::new(self.data);
        let marker = self.builder.write_item_start(&mut wr)?;
        let is_right = f(&mut wr);
        self.builder.write_item_finish(marker, is_right, &mut wr);
        Ok(())
    }

    /// Finalize the BufWriter and get result bytes
    pub fn finish_and_take(self) -> Result<&'i [u8], Error> {
        let wr = BufWriter::new(self.data);
        self.builder.finish_and_take(wr)
    }
}

impl<'i> SerializeShrinkWrap for EitherAnyVec<'i> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_raw_slice(self.data)
    }
}

#[cfg(feature = "std")]
impl<'i> crate::SerializeShrinkWrapOwned for EitherAnyVec<'i> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap_owned(&self, wr: &mut crate::BufWriterOwned) -> Result<(), Error> {
        wr.write_raw_slice(self.data)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for EitherAnyVec<'i> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        Ok(EitherAnyVec {
            data: rd.read_raw_slice(rd.bytes_left())?,
        })
    }
}

impl<'i> EitherAnyVecIter<'i> {
    /// Read the next flag and deserialize either `L` or `R` type.
    pub fn next<L: DeserializeShrinkWrap<'i>, R: DeserializeShrinkWrap<'i>>(
        &mut self,
    ) -> Result<Either<L, R>, Error> {
        if self.read_flag()? {
            Ok(Either::Right(self.rd.read()?))
        } else {
            Ok(Either::Left(self.rd.read()?))
        }
    }

    /// Read the next flag and deserialize either `L` or `R` type.
    #[cfg(feature = "std")]
    pub fn next_owned<L: DeserializeShrinkWrapOwned, R: DeserializeShrinkWrapOwned>(
        &mut self,
    ) -> Result<Either<L, R>, Error> {
        if self.read_flag()? {
            Ok(Either::Right(self.rd.read_owned()?))
        } else {
            Ok(Either::Left(self.rd.read_owned()?))
        }
    }

    fn read_flag(&mut self) -> Result<bool, Error> {
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
        Ok(is_r)
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
        // 0x19,
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
        let mut wr = EitherAnyVecWriter::new(&mut buf).unwrap();
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
        // 0x29, // size in bytes
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
        let mut wr = EitherAnyVecWriter::new(&mut buf).unwrap();
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

    #[test]
    fn result_vec_write_with() {
        let mut buf = [0u8; 64];
        let mut wr = EitherAnyVecWriter::new(&mut buf).unwrap();

        wr.write_with(|wr| write_with_buf_writer(wr, vec![0xAA, 0xBB, 0xCC]))
            .unwrap();

        wr.write_with(|wr| write_with_buf_writer(wr, vec![0xDD, 0xEE]))
            .unwrap();

        wr.write_with(|wr| write_with_buf_writer(wr, vec![0xFF]))
            .unwrap();

        wr.write_with(|wr| {
            wr.write(&MyError::E).unwrap();
            false
        })
        .unwrap();

        let data = wr.finish_and_take().unwrap();
        assert_eq!(data, RESULT_VEC);
    }

    // look Ma, no closures!
    #[test]
    fn result_vec_write_builder() {
        let mut buf = [0u8; 64];
        let mut wr = BufWriter::new(&mut buf);
        let mut builder = EitherAnyVecBuilder::new(&wr);

        let marker = builder.write_item_start(&mut wr).unwrap();
        let is_right = write_with_buf_writer(&mut wr, vec![0xAA, 0xBB, 0xCC]);
        builder.write_item_finish(marker, is_right, &mut wr);

        let marker = builder.write_item_start(&mut wr).unwrap();
        let is_right = write_with_buf_writer(&mut wr, vec![0xDD, 0xEE]);
        builder.write_item_finish(marker, is_right, &mut wr);

        builder.write_right(&vec![0xFFu8], &mut wr).unwrap();
        builder.write_left(&MyError::E, &mut wr).unwrap();

        let data = builder.finish_and_take(wr).unwrap();
        assert_eq!(data, RESULT_VEC);
    }

    fn write_with_buf_writer(wr: &mut BufWriter, ty: Vec<u8>) -> bool {
        wr.write(&ty).unwrap();
        true
    }
}

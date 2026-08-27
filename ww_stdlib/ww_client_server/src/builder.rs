use wire_weaver::shrink_wrap::{
    BufWriter, Error,
    buf_writer::{BufWriterState, UnsizedBuilder},
};

use crate::{ErrorKindDiscriminants, EventKindDiscriminants};

pub struct EventBuilder {
    result_flag: BufWriterState,
}

pub struct EventKindBuilder {
    discriminant: BufWriterState,
}

pub struct ErrorBuilder {
    discriminant: BufWriterState,
    builder: UnsizedBuilder,
}

impl EventBuilder {
    pub fn new(seq: u16, wr: &mut BufWriter) -> Result<Self, Error> {
        wr.write_u16(seq)?;
        let result_flag = wr.save_state();
        wr.write_bool(false)?;
        Ok(Self { result_flag })
    }

    pub fn finish(self, is_ok: bool, wr: &mut BufWriter<'_>) {
        let items = wr.save_state();
        wr.restore_state(self.result_flag);
        _ = wr.write_bool(is_ok);
        wr.restore_state(items);
    }
}

impl EventKindBuilder {
    pub fn new(wr: &mut BufWriter<'_>) -> Result<Self, Error> {
        let discriminant = wr.save_state();
        wr.write_nib_masked(0)?;
        Ok(Self { discriminant })
    }

    pub fn finish_with_kind(self, kind: EventKindDiscriminants, wr: &mut BufWriter<'_>) {
        let state = wr.save_state();
        wr.restore_state(self.discriminant);
        _ = wr.write_nib_masked(kind.discriminant());
        wr.restore_state(state);
    }
}

impl ErrorBuilder {
    pub fn new(seq: u32, wr: &mut BufWriter<'_>) -> Result<Self, Error> {
        wr.write_unib32(seq)?;
        let builder = UnsizedBuilder::new(wr)?;
        let discriminant = wr.save_state();
        wr.write_u8(0)?;
        Ok(Self {
            discriminant,
            builder,
        })
    }

    pub fn finish_with_kind(
        self,
        kind: ErrorKindDiscriminants,
        wr: &mut BufWriter<'_>,
    ) -> Result<(), Error> {
        let state = wr.save_state();
        wr.restore_state(self.discriminant);
        _ = wr.write_u8(kind.discriminant());
        wr.restore_state(state);
        self.builder.finish(wr)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use wire_weaver::{
        derive_shrink_wrap,
        shrink_wrap::{prelude::*, tail_bytes::TailBytes},
    };

    use crate::{
        Error, ErrorKind, Event, EventKind,
        builder::{ErrorBuilder, EventBuilder, EventKindBuilder},
    };

    #[test]
    fn event_builder_ok() {
        let ev = Event {
            seq: 0xABCD,
            result: Ok(crate::EventKind::Value {
                data: TailBytes(&[0xAA, 0xBB, 0xCC]),
            }),
        };
        let mut buf = [0u8; 32];
        let ev_bytes = ev.to_ww_bytes(&mut buf).unwrap();

        let mut buf = [0u8; 32];
        let mut wr = BufWriter::new(&mut buf);
        let builder = EventBuilder::new(0xABCD, &mut wr).unwrap();
        EventKind::Value {
            data: TailBytes(&[0xAA, 0xBB, 0xCC]),
        }
        .ser_shrink_wrap(&mut wr)
        .unwrap();
        builder.finish(true, &mut wr);
        let builder_bytes = wr.finish_and_take().unwrap();

        assert_eq!(ev_bytes, builder_bytes);
    }

    #[test]
    fn event_builder_err() {
        let ev = Event {
            seq: 0xABCD,
            result: Err(Error::not_supported(123)),
        };
        let mut buf = [0u8; 32];
        let ev_bytes = ev.to_ww_bytes(&mut buf).unwrap();

        let mut buf = [0u8; 32];
        let mut wr = BufWriter::new(&mut buf);
        let builder = EventBuilder::new(0xABCD, &mut wr).unwrap();
        // wr.write(&Error::not_supported(123)).unwrap();
        Error::not_supported(123).ser_shrink_wrap(&mut wr).unwrap();
        builder.finish(false, &mut wr);
        let builder_bytes = wr.finish_and_take().unwrap();

        assert_eq!(ev_bytes, builder_bytes);
    }

    #[test]
    fn event_builder_low_level() {
        let ev = Event {
            seq: 0xABCD,
            result: Ok(crate::EventKind::Value {
                data: TailBytes(&[0xAA, 0xBB, 0xCC, 0x03]),
            }),
        };
        let mut buf = [0u8; 32];
        let ev_bytes = ev.to_ww_bytes(&mut buf).unwrap();

        let mut buf = [0u8; 32];
        let mut wr = BufWriter::new(&mut buf);
        let ev_builder = EventBuilder::new(0xABCD, &mut wr).unwrap();
        let ev_kind_builder = EventKindBuilder::new(&mut wr).unwrap();
        // wr.write(&&[0xAAu8, 0xBB, 0xCC][..]).unwrap();
        let bytes = &[0xAAu8, 0xBB, 0xCC][..];
        bytes.ser_shrink_wrap(&mut wr).unwrap();
        ev_kind_builder.finish_with_kind(crate::EventKindDiscriminants::Value, &mut wr);
        ev_builder.finish(true, &mut wr);
        let builder_bytes = wr.finish_and_take().unwrap();

        assert_eq!(ev_bytes, builder_bytes);
    }

    #[derive_shrink_wrap]
    struct Custom<'i> {
        a: u8,
        b: RefVec<'i, u8>,
    }

    #[test]
    fn event_builder_custom_type() {
        let custom = Custom {
            a: 37,
            b: RefVec::new_bytes(b"1234"),
        };
        let mut buf = [0u8; 32];
        let mut wr = BufWriter::new(&mut buf);
        custom.ser_shrink_wrap(&mut wr).unwrap();
        let custom_bytes = wr.finish_and_take().unwrap();

        let ev = Event {
            seq: 0xABCD,
            result: Ok(crate::EventKind::Value {
                data: TailBytes(custom_bytes),
            }),
        };
        let mut buf = [0u8; 32];
        let ev_bytes = ev.to_ww_bytes(&mut buf).unwrap();
        println!("{ev_bytes:02x?}");

        let mut buf = [0u8; 32];
        let mut wr = BufWriter::new(&mut buf);
        let ev_builder = EventBuilder::new(0xABCD, &mut wr).unwrap();
        let ev_kind_builder = EventKindBuilder::new(&mut wr).unwrap();
        custom.ser_shrink_wrap(&mut wr).unwrap();
        // wr.write(&custom).unwrap();
        ev_kind_builder.finish_with_kind(crate::EventKindDiscriminants::Value, &mut wr);
        ev_builder.finish(true, &mut wr);
        let builder_bytes = wr.finish_and_take().unwrap();

        println!("{builder_bytes:02x?}");
        assert_eq!(ev_bytes, builder_bytes);
    }

    #[test]
    fn error_builder() {
        let err = Error::new(
            0xFF,
            ErrorKind::UserBytes(RefVec::Slice {
                slice: &[0xAA, 0xBB, 0xCC],
            }),
        );
        let mut buf = [0u8; 32];
        let err_bytes = err.to_ww_bytes(&mut buf).unwrap();

        let mut buf = [0u8; 32];
        let mut wr = BufWriter::new(&mut buf);
        let err_builder = ErrorBuilder::new(0xFF, &mut wr).unwrap();
        wr.write(&&[0xAAu8, 0xBB, 0xCC][..]).unwrap();
        err_builder
            .finish_with_kind(crate::ErrorKindDiscriminants::UserBytes, &mut wr)
            .unwrap();
        let builder_bytes = wr.finish_and_take().unwrap();

        assert_eq!(err_bytes, builder_bytes);
    }

    #[derive_shrink_wrap]
    struct CustomUserError<'i> {
        a: u8,
        b: RefVec<'i, u8>,
    }

    #[test]
    fn error_builder_custom_type() {
        let mut buf = [0u8; 32];
        let custom_error = CustomUserError {
            a: 0xDD,
            b: RefVec::new_bytes(&[0xAA, 0xBB, 0xCC]),
        };
        let user_bytes = custom_error.to_ww_bytes(&mut buf).unwrap();

        let err = Error::new(3, ErrorKind::UserBytes(RefVec::new_bytes(user_bytes)));
        let mut buf = [0u8; 32];
        let err_bytes = err.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(
            err_bytes,
            &[
                0x30, // error.seq (UNib32)
                0x0C, // ErrorKind discriminant (u8)
                0xDD, // custom_error.a
                0xAA, 0xBB, 0xCC, // custom_error.b
                0x03, // .b len
                0x05, // custom_error len
                0x07  // ErrorKind len
            ]
        );

        let mut buf = [0u8; 32];
        let mut wr = BufWriter::new(&mut buf);
        let err_builder = ErrorBuilder::new(3, &mut wr).unwrap();

        wr.write(&custom_error).unwrap();

        err_builder
            .finish_with_kind(crate::ErrorKindDiscriminants::UserBytes, &mut wr)
            .unwrap();
        let builder_bytes = wr.finish_and_take().unwrap();

        assert_eq!(err_bytes, builder_bytes);
    }
}

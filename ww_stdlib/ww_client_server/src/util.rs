use super::{Event, EventKind};
use wire_weaver::shrink_wrap::{BufWriter, Error, SerializeShrinkWrap, tail_bytes::TailBytes};

pub fn ser_ok_event<'a>(
    scratch: &'a mut [u8],
    seq: u16,
    kind: EventKind<'_>,
) -> Result<&'a [u8], Error> {
    let mut wr = BufWriter::new(scratch);
    let event = Event {
        seq,
        result: Ok(kind),
    };
    event.ser_shrink_wrap(&mut wr)?;
    wr.finish_and_take()
}

pub fn ser_err_event<'i>(
    scratch: &'i mut [u8],
    seq: u16,
    error: super::Error<'_>,
) -> Result<&'i [u8], Error> {
    let mut wr = BufWriter::new(scratch);
    let event = Event {
        seq,
        result: Err(error),
    };
    event.ser_shrink_wrap(&mut wr)?;
    wr.finish_and_take()
}

pub fn ser_unit_return_event(scratch: &mut [u8], seq: u16) -> Result<&[u8], Error> {
    if seq == 0 {
        return Ok(&[]);
    }
    let mut wr = BufWriter::new(scratch);
    let event = Event {
        seq,
        result: Ok(EventKind::Value {
            data: TailBytes(&[]),
        }),
    };
    event.ser_shrink_wrap(&mut wr)?;
    wr.finish_and_take()
}

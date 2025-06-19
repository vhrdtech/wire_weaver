use super::{Event, EventKind};
use wire_weaver::prelude::RefVec;
use wire_weaver::shrink_wrap::{BufWriter, Error, SerializeShrinkWrap};

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

pub fn ser_err_event(scratch: &mut [u8], seq: u16, error: super::Error) -> Result<&[u8], Error> {
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
        result: Ok(EventKind::ReturnValue {
            // 0 is for future compatibility if unit is changed to something else
            data: RefVec::Slice { slice: &[0x00] },
        }),
    };
    event.ser_shrink_wrap(&mut wr)?;
    wr.finish_and_take()
}

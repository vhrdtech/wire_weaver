#[derive(Debug)]
pub struct Request {
    pub seq: u16,
    pub kind: RequestKind,
}
impl wire_weaver::shrink_wrap::SerializeShrinkWrap for Request {
    fn ser_shrink_wrap(&self, wr: &mut shrink_wrap::BufWriter) -> Result<(), shrink_wrap::Error> {
        wr.write_u16(self.seq)?;
        let u16_rev_from = wr.u16_rev_pos();
        let unsized_start = wr.pos().0;
        wr.write(&self.kind)?;
        wr.align_byte();
        let size = wr.pos().0 - unsized_start;
        wr.encode_vlu16n_rev(u16_rev_from, wr.u16_rev_pos())?;
        let Ok(size) = u16::try_from(size) else {
            return Err(shrink_wrap::Error::ItemTooLong);
        };
        wr.write_u16_rev(size)?;
        Ok(())
    }
}
impl<'i> wire_weaver::shrink_wrap::DeserializeShrinkWrap<'i> for Request {
    fn des_shrink_wrap<'di>(
        rd: &'di mut shrink_wrap::BufReader<'i>,
        _element_size: shrink_wrap::ElementSize,
    ) -> Result<Self, shrink_wrap::Error> {
        let seq = rd.read_u16()?;
        let size = rd.read_vlu16n_rev()? as usize;
        let mut rd_split = rd.split(size)?;
        let kind = rd_split.read(shrink_wrap::ElementSize::Implied)?;
        Ok(Request { seq, kind })
    }
}
#[derive(Debug)]
#[repr(u16)]
pub enum RequestKind {
    Call = 0,
    Heartbeat = 1,
}
impl RequestKind {
    pub fn discriminant(&self) -> u16 {
        unsafe { *<*const _>::from(self).cast::<u16>() }
    }
}
impl wire_weaver::shrink_wrap::SerializeShrinkWrap for RequestKind {
    fn ser_shrink_wrap(&self, wr: &mut shrink_wrap::BufWriter) -> Result<(), shrink_wrap::Error> {
        wr.write_vlu16n(self.discriminant())?;
        Ok(())
    }
}
impl<'i> wire_weaver::shrink_wrap::DeserializeShrinkWrap<'i> for RequestKind {
    fn des_shrink_wrap<'di>(
        rd: &'di mut shrink_wrap::BufReader<'i>,
        _element_size: shrink_wrap::ElementSize,
    ) -> Result<Self, shrink_wrap::Error> {
        let discriminant = rd.read_vlu16n()?;
        Ok(match discriminant {
            0 => RequestKind::Call,
            1 => RequestKind::Heartbeat,
            _ => {
                return Err(shrink_wrap::Error::EnumFutureVersionOrMalformedData);
            }
        })
    }
}
#[derive(Debug)]
pub struct Event {
    pub seq: u16,
    pub result: Result,
}
impl wire_weaver::shrink_wrap::SerializeShrinkWrap for Event {
    fn ser_shrink_wrap(&self, wr: &mut shrink_wrap::BufWriter) -> Result<(), shrink_wrap::Error> {
        wr.write_u16(self.seq)?;
        let u16_rev_from = wr.u16_rev_pos();
        let unsized_start = wr.pos().0;
        wr.write(&self.result)?;
        wr.align_byte();
        let size = wr.pos().0 - unsized_start;
        wr.encode_vlu16n_rev(u16_rev_from, wr.u16_rev_pos())?;
        let Ok(size) = u16::try_from(size) else {
            return Err(shrink_wrap::Error::ItemTooLong);
        };
        wr.write_u16_rev(size)?;
        Ok(())
    }
}
impl<'i> wire_weaver::shrink_wrap::DeserializeShrinkWrap<'i> for Event {
    fn des_shrink_wrap<'di>(
        rd: &'di mut shrink_wrap::BufReader<'i>,
        _element_size: shrink_wrap::ElementSize,
    ) -> Result<Self, shrink_wrap::Error> {
        let seq = rd.read_u16()?;
        let size = rd.read_vlu16n_rev()? as usize;
        let mut rd_split = rd.split(size)?;
        let result = rd_split.read(shrink_wrap::ElementSize::Implied)?;
        Ok(Event { seq, result })
    }
}
#[derive(Debug)]
#[repr(u16)]
pub enum EventKind {
    ReturnValue = 0,
    Heartbeat = 1,
}
impl EventKind {
    pub fn discriminant(&self) -> u16 {
        unsafe { *<*const _>::from(self).cast::<u16>() }
    }
}
impl wire_weaver::shrink_wrap::SerializeShrinkWrap for EventKind {
    fn ser_shrink_wrap(&self, wr: &mut shrink_wrap::BufWriter) -> Result<(), shrink_wrap::Error> {
        wr.write_vlu16n(self.discriminant())?;
        Ok(())
    }
}
impl<'i> wire_weaver::shrink_wrap::DeserializeShrinkWrap<'i> for EventKind {
    fn des_shrink_wrap<'di>(
        rd: &'di mut shrink_wrap::BufReader<'i>,
        _element_size: shrink_wrap::ElementSize,
    ) -> Result<Self, shrink_wrap::Error> {
        let discriminant = rd.read_vlu16n()?;
        Ok(match discriminant {
            0 => EventKind::ReturnValue,
            1 => EventKind::Heartbeat,
            _ => {
                return Err(shrink_wrap::Error::EnumFutureVersionOrMalformedData);
            }
        })
    }
}

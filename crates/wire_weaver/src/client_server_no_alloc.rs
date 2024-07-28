use crate as wire_weaver;
#[derive(Debug)]
pub struct Request<'i> {
    pub seq: u16,
    pub kind: RequestKind<'i>,
}
impl<'i> wire_weaver::shrink_wrap::SerializeShrinkWrap for Request<'i> {
    fn ser_shrink_wrap(
        &self,
        wr: &mut wire_weaver::shrink_wrap::BufWriter,
    ) -> Result<(), wire_weaver::shrink_wrap::Error> {
        wr.write_u16(self.seq)?;
        let size_slot_pos = wr.write_u16_rev(0)?;
        let unsized_start_bytes = wr.pos().0;
        wr.write(&self.kind)?;
        wr.encode_nib16_rev(wr.u16_rev_pos(), size_slot_pos)?;
        wr.align_byte();
        let size_bytes = wr.pos().0 - unsized_start_bytes;
        let Ok(size_bytes) = u16::try_from(size_bytes) else {
            return Err(wire_weaver::shrink_wrap::Error::ItemTooLong);
        };
        wr.update_u16_rev(size_slot_pos, size_bytes)?;
        Ok(())
    }
}
impl<'i> wire_weaver::shrink_wrap::DeserializeShrinkWrap<'i> for Request<'i> {
    fn des_shrink_wrap<'di>(
        rd: &'di mut wire_weaver::shrink_wrap::BufReader<'i>,
        _element_size: wire_weaver::shrink_wrap::ElementSize,
    ) -> Result<Self, wire_weaver::shrink_wrap::Error> {
        let seq = rd.read_u16()?;
        let size = rd.read_nib16_rev()? as usize;
        let mut rd_split = rd.split(size)?;
        let kind = rd_split.read(wire_weaver::shrink_wrap::ElementSize::Unsized)?;
        Ok(Request { seq, kind })
    }
}
#[derive(Debug)]
#[repr(u16)]
pub enum RequestKind<'i> {
    Version {
        protocol_id: u32,
        version: Version,
    } = 0,
    Call {
        args: wire_weaver::shrink_wrap::vec::RefVec<'i, u8>,
    } = 1,
    Read = 2,
    Heartbeat = 3,
}
impl<'i> RequestKind<'i> {
    pub fn discriminant(&self) -> u16 {
        unsafe { *<*const _>::from(self).cast::<u16>() }
    }
}
impl<'i> wire_weaver::shrink_wrap::SerializeShrinkWrap for RequestKind<'i> {
    fn ser_shrink_wrap(
        &self,
        wr: &mut wire_weaver::shrink_wrap::BufWriter,
    ) -> Result<(), wire_weaver::shrink_wrap::Error> {
        wr.write_nib16(self.discriminant())?;
        match &self {
            RequestKind::Version {
                protocol_id,
                version,
            } => {
                wr.write_u32(*protocol_id)?;
                let size_slot_pos = wr.write_u16_rev(0)?;
                let unsized_start_bytes = wr.pos().0;
                wr.write(version)?;
                wr.encode_nib16_rev(wr.u16_rev_pos(), size_slot_pos)?;
                wr.align_byte();
                let size_bytes = wr.pos().0 - unsized_start_bytes;
                let Ok(size_bytes) = u16::try_from(size_bytes) else {
                    return Err(wire_weaver::shrink_wrap::Error::ItemTooLong);
                };
                wr.update_u16_rev(size_slot_pos, size_bytes)?;
            }
            RequestKind::Call { args } => {
                args.ser_shrink_wrap_vec_u8(wr)?;
            }
            _ => {}
        }
        Ok(())
    }
}
impl<'i> wire_weaver::shrink_wrap::DeserializeShrinkWrap<'i> for RequestKind<'i> {
    fn des_shrink_wrap<'di>(
        rd: &'di mut wire_weaver::shrink_wrap::BufReader<'i>,
        _element_size: wire_weaver::shrink_wrap::ElementSize,
    ) -> Result<Self, wire_weaver::shrink_wrap::Error> {
        let discriminant = rd.read_nib16()?;
        Ok(match discriminant {
            0 => {
                let protocol_id = rd.read_u32()?;
                let size = rd.read_nib16_rev()? as usize;
                let mut rd_split = rd.split(size)?;
                let version = rd_split.read(wire_weaver::shrink_wrap::ElementSize::Unsized)?;
                RequestKind::Version {
                    protocol_id,
                    version,
                }
            }
            1 => {
                let args =
                    rd.read(wire_weaver::shrink_wrap::traits::ElementSize::Sized { size_bits: 8 })?;
                RequestKind::Call { args }
            }
            2 => RequestKind::Read,
            3 => RequestKind::Heartbeat,
            _ => {
                return Err(wire_weaver::shrink_wrap::Error::EnumFutureVersionOrMalformedData);
            }
        })
    }
}
#[derive(Debug)]
pub struct Event<'i> {
    pub seq: u16,
    pub result: Result<EventKind<'i>, Error>,
}
impl<'i> wire_weaver::shrink_wrap::SerializeShrinkWrap for Event<'i> {
    fn ser_shrink_wrap(
        &self,
        wr: &mut wire_weaver::shrink_wrap::BufWriter,
    ) -> Result<(), wire_weaver::shrink_wrap::Error> {
        wr.write_u16(self.seq)?;
        wr.write_bool(self.result.is_ok())?;
        match &self.result {
            Ok(v) => {
                wr.write(v)?;
            }
            Err(e) => {
                wr.write(e)?;
            }
        }
        Ok(())
    }
}
impl<'i> wire_weaver::shrink_wrap::DeserializeShrinkWrap<'i> for Event<'i> {
    fn des_shrink_wrap<'di>(
        rd: &'di mut wire_weaver::shrink_wrap::BufReader<'i>,
        _element_size: wire_weaver::shrink_wrap::ElementSize,
    ) -> Result<Self, wire_weaver::shrink_wrap::Error> {
        let seq = rd.read_u16()?;
        let _result_flag = rd.read_bool()?;
        let result = if _result_flag {
            Ok(rd.read(wire_weaver::shrink_wrap::traits::ElementSize::Unsized)?)
        } else {
            Err(rd.read(wire_weaver::shrink_wrap::traits::ElementSize::Unsized)?)
        };
        Ok(Event { seq, result })
    }
}
#[derive(Debug)]
#[repr(u16)]
pub enum EventKind<'i> {
    Version {
        protocol_id: u32,
        version: Version,
    } = 0,
    ReturnValue {
        data: wire_weaver::shrink_wrap::vec::RefVec<'i, u8>,
    } = 1,
    ReadValue {
        data: wire_weaver::shrink_wrap::vec::RefVec<'i, u8>,
    } = 2,
    Heartbeat = 3,
}
impl<'i> EventKind<'i> {
    pub fn discriminant(&self) -> u16 {
        unsafe { *<*const _>::from(self).cast::<u16>() }
    }
}
impl<'i> wire_weaver::shrink_wrap::SerializeShrinkWrap for EventKind<'i> {
    fn ser_shrink_wrap(
        &self,
        wr: &mut wire_weaver::shrink_wrap::BufWriter,
    ) -> Result<(), wire_weaver::shrink_wrap::Error> {
        wr.write_nib16(self.discriminant())?;
        match &self {
            EventKind::Version {
                protocol_id,
                version,
            } => {
                wr.write_u32(*protocol_id)?;
                let size_slot_pos = wr.write_u16_rev(0)?;
                let unsized_start_bytes = wr.pos().0;
                wr.write(version)?;
                wr.encode_nib16_rev(wr.u16_rev_pos(), size_slot_pos)?;
                wr.align_byte();
                let size_bytes = wr.pos().0 - unsized_start_bytes;
                let Ok(size_bytes) = u16::try_from(size_bytes) else {
                    return Err(wire_weaver::shrink_wrap::Error::ItemTooLong);
                };
                wr.update_u16_rev(size_slot_pos, size_bytes)?;
            }
            EventKind::ReturnValue { data } => {
                data.ser_shrink_wrap_vec_u8(wr)?;
            }
            EventKind::ReadValue { data } => {
                data.ser_shrink_wrap_vec_u8(wr)?;
            }
            _ => {}
        }
        Ok(())
    }
}
impl<'i> wire_weaver::shrink_wrap::DeserializeShrinkWrap<'i> for EventKind<'i> {
    fn des_shrink_wrap<'di>(
        rd: &'di mut wire_weaver::shrink_wrap::BufReader<'i>,
        _element_size: wire_weaver::shrink_wrap::ElementSize,
    ) -> Result<Self, wire_weaver::shrink_wrap::Error> {
        let discriminant = rd.read_nib16()?;
        Ok(match discriminant {
            0 => {
                let protocol_id = rd.read_u32()?;
                let size = rd.read_nib16_rev()? as usize;
                let mut rd_split = rd.split(size)?;
                let version = rd_split.read(wire_weaver::shrink_wrap::ElementSize::Unsized)?;
                EventKind::Version {
                    protocol_id,
                    version,
                }
            }
            1 => {
                let data =
                    rd.read(wire_weaver::shrink_wrap::traits::ElementSize::Sized { size_bits: 8 })?;
                EventKind::ReturnValue { data }
            }
            2 => {
                let data =
                    rd.read(wire_weaver::shrink_wrap::traits::ElementSize::Sized { size_bits: 8 })?;
                EventKind::ReadValue { data }
            }
            3 => EventKind::Heartbeat,
            _ => {
                return Err(wire_weaver::shrink_wrap::Error::EnumFutureVersionOrMalformedData);
            }
        })
    }
}
#[derive(Debug)]
#[repr(u16)]
pub enum Error {
    Test = 0,
}
impl Error {
    pub fn discriminant(&self) -> u16 {
        unsafe { *<*const _>::from(self).cast::<u16>() }
    }
}
impl wire_weaver::shrink_wrap::SerializeShrinkWrap for Error {
    fn ser_shrink_wrap(
        &self,
        wr: &mut wire_weaver::shrink_wrap::BufWriter,
    ) -> Result<(), wire_weaver::shrink_wrap::Error> {
        wr.write_nib16(self.discriminant())?;
        Ok(())
    }
}
impl<'i> wire_weaver::shrink_wrap::DeserializeShrinkWrap<'i> for Error {
    fn des_shrink_wrap<'di>(
        rd: &'di mut wire_weaver::shrink_wrap::BufReader<'i>,
        _element_size: wire_weaver::shrink_wrap::ElementSize,
    ) -> Result<Self, wire_weaver::shrink_wrap::Error> {
        let discriminant = rd.read_nib16()?;
        Ok(match discriminant {
            0 => Error::Test,
            _ => {
                return Err(wire_weaver::shrink_wrap::Error::EnumFutureVersionOrMalformedData);
            }
        })
    }
}
#[derive(Debug)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}
impl wire_weaver::shrink_wrap::SerializeShrinkWrap for Version {
    fn ser_shrink_wrap(
        &self,
        wr: &mut wire_weaver::shrink_wrap::BufWriter,
    ) -> Result<(), wire_weaver::shrink_wrap::Error> {
        wr.write_nib16(self.major)?;
        wr.write_nib16(self.minor)?;
        wr.write_nib16(self.patch)?;
        Ok(())
    }
}
impl<'i> wire_weaver::shrink_wrap::DeserializeShrinkWrap<'i> for Version {
    fn des_shrink_wrap<'di>(
        rd: &'di mut wire_weaver::shrink_wrap::BufReader<'i>,
        _element_size: wire_weaver::shrink_wrap::ElementSize,
    ) -> Result<Self, wire_weaver::shrink_wrap::Error> {
        let major = rd.read_nib16()?;
        let minor = rd.read_nib16()?;
        let patch = rd.read_nib16()?;
        Ok(Version {
            major,
            minor,
            patch,
        })
    }
}

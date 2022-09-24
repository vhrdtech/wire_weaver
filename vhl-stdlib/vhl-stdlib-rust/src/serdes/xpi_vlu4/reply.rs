use crate::serdes::bit_buf::BitBufMut;
use crate::serdes::traits::{DeserializeCoupledBitsVlu4, SerializeBits};
use crate::serdes::vlu4::vec::Vlu4Vec;
use crate::serdes::xpi_vlu4::addressing::{NodeSet, RequestId, XpiResourceSet};
use crate::serdes::xpi_vlu4::error::{FailReason, XpiVlu4Error};
use crate::serdes::xpi_vlu4::priority::Priority;
use crate::serdes::xpi_vlu4::resource_info::ResourceInfo;
use crate::serdes::xpi_vlu4::NodeId;
use crate::serdes::{BitBuf, DeserializeVlu4, NibbleBuf, NibbleBufMut};
use crate::xpi::reply::{XpiGenericReply, XpiGenericReplyKind};
// use enum_kinds::EnumKind;
// use enum_primitive_derive::Primitive;

/// Highly space efficient xPI reply data structure supporting zero copy and no_std without alloc
/// even for variable length arrays or strings.
/// See [XpiGenericReply](crate::xpi::reply::XpiGenericReply) for detailed information.
pub type XpiReply<'rep> = XpiGenericReply<
    NodeId,
    NodeSet<'rep>,
    XpiResourceSet<'rep>,
    Vlu4Vec<'rep, &'rep [u8]>,
    Vlu4Vec<'rep, Result<&'rep [u8], FailReason>>,
    Vlu4Vec<'rep, Result<(), FailReason>>,
    Vlu4Vec<'rep, Result<ResourceInfo<'rep>, FailReason>>,
    RequestId,
    Priority
>;

/// See [XpiGenericReplyKind](crate::xpi::reply::XpiGenericReplyKind) for detailed information.
pub type XpiReplyKind<'rep> = XpiGenericReplyKind<
    Vlu4Vec<'rep, &'rep [u8]>,
    Vlu4Vec<'rep, Result<&'rep [u8], FailReason>>,
    Vlu4Vec<'rep, Result<(), FailReason>>,
    Vlu4Vec<'rep, Result<ResourceInfo<'rep>, FailReason>>,
>;

pub enum XpiReplyDiscriminant {
    CallComplete,
    ReadComplete,
    WriteComplete,
    OpenStream,
    StreamUpdate,
    CloseStream,
    Subscribe,
    RateChange,
    Unsubscribe,
    Borrow,
    Release,
    Introspect,
}

impl<'i> SerializeBits for XpiReplyDiscriminant {
    type Error = crate::serdes::bit_buf::Error;

    fn ser_bits(&self, wgr: &mut BitBufMut) -> Result<(), Self::Error> {
        use XpiReplyDiscriminant::*;
        let kind = match self {
            CallComplete => 0,
            ReadComplete => 1,
            WriteComplete => 2,
            OpenStream => 3,
            StreamUpdate => 4,
            CloseStream => 5,
            Subscribe => 6,
            RateChange => 7,
            Unsubscribe => 8,
            Borrow => 9,
            Release => 10,
            Introspect => 11,
        };
        wgr.put_up_to_8(4, kind)?;
        Ok(())
    }
}

// impl<'i> SerializeVlu4 for XpiReplyKind<'i> {
//     type Error = XpiVlu4Error;
//
//     fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
//         match *self {
//             XpiReplyKind::CallComplete(call_results) => {
//                 // match call_result {
//                 //     Ok(return_value) => {
//                 //         wgr.put_nibble(0)?;
//                 //         wgr.put(return_value)
//                 //     },
//                 //     Err(e) => wgr.put(e)
//                 // }
//                 wgr.put(call_results)
//             }
//             XpiReplyKind::ReadComplete(_) => {
//                 todo!()
//             }
//             XpiReplyKind::WriteComplete(_) => {
//
//                 todo!()
//             }
//             XpiReplyKind::OpenStream(_) => {
//
//                 todo!()
//             }
//             XpiReplyKind::StreamUpdate(_) => {
//
//                 todo!()
//             }
//             XpiReplyKind::CloseStream(_) => {
//
//                 todo!()
//             }
//             XpiReplyKind::Subscribe(_) => {
//
//                 todo!()
//             }
//             XpiReplyKind::RateChange(_) => {
//
//                 todo!()
//             }
//             XpiReplyKind::Unsubscribe(_) => {
//
//                 todo!()
//             }
//             XpiReplyKind::Borrow(_) => {
//
//                 todo!()
//             }
//             XpiReplyKind::Release(_) => {
//
//                 todo!()
//             }
//             XpiReplyKind::Introspect(_) => {
//
//                 todo!()
//             }
//         }?;
//         Ok(())
//     }
// }

impl<'i> DeserializeCoupledBitsVlu4<'i> for XpiReplyKind<'i> {
    type Error = XpiVlu4Error;

    fn des_coupled_bits_vlu4<'di>(
        bits_rdr: &'di mut BitBuf<'i>,
        vlu4_rdr: &'di mut NibbleBuf<'i>,
    ) -> Result<Self, Self::Error> {
        let kind = bits_rdr.get_up_to_8(4)?;
        use XpiGenericReplyKind::*;
        match kind {
            0 => Ok(CallComplete(vlu4_rdr.des_vlu4()?)),
            _ => Err(XpiVlu4Error::Unimplemented),
        }
    }
}

// impl<'i> SerializeVlu4 for XpiReply<'i> {
//     type Error = XpiVlu4Error;
//
//     fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
//         wgr.as_bit_buf::<XpiVlu4Error, _>(|wgr| {
//             wgr.put_up_to_8(3, 0b000)?; // unused 31:29
//             wgr.put(self.priority)?; // bits 28:26
//             wgr.put_bit(true)?; // bit 25, is_unicast
//             wgr.put_bit(false)?; // bit 24, is_request
//             wgr.put_bit(true)?; // bit 23, reserved
//             wgr.put(self.source)?; // bits 22:16
//             wgr.put(self.destination)?; // bits 15:7 - discriminant of NodeSet (2b) + 7b for NodeId or other
//             wgr.put(self.resource_set)?; // bits 6:4 - discriminant of ResourceSet+Uri
//             wgr.put(self.kind)?; // bits 3:0 - discriminant of XpiReplyKind
//             Ok(())
//         })?;
//         wgr.put(self.destination)?;
//         wgr.put(self.resource_set)?;
//         wgr.put(self.kind)?;
//         wgr.put(self.request_id)?;
//         Ok(())
//     }
// }

pub struct XpiReplyBuilder<'i> {
    nwr: NibbleBufMut<'i>,
    source: NodeId,
    destination: NodeSet<'i>,
    resource_set: XpiResourceSet<'i>,
    request_id: RequestId,
    priority: Priority,
}

impl<'i> XpiReplyBuilder<'i> {
    pub fn new(
        mut nwr: NibbleBufMut<'i>,
        source: NodeId,
        destination: NodeSet<'i>,
        resource_set: XpiResourceSet<'i>,
        request_id: RequestId,
        priority: Priority,
    ) -> Result<Self, XpiVlu4Error> {
        nwr.skip(8)?;
        nwr.put(&destination)?;
        nwr.put(&resource_set)?;
        Ok(XpiReplyBuilder {
            nwr,
            source,
            destination,
            resource_set,
            request_id,
            priority,
        })
    }

    pub fn build_kind_with<F>(self, f: F) -> Result<NibbleBufMut<'i>, FailReason>
        where
            F: Fn(NibbleBufMut<'i>) -> Result<(XpiReplyDiscriminant, NibbleBufMut<'i>), FailReason>,
    {
        let (kind, mut nwr) = f(self.nwr)?;
        nwr.put(&self.request_id).unwrap();
        nwr.rewind::<_, FailReason>(0, |nwr| {
            nwr.as_bit_buf::<FailReason, _>(|bwr| {
                bwr.put_up_to_8(3, 0b000)?; // unused 31:29
                bwr.put(&self.priority)?; // bits 28:26
                bwr.put_bit(true)?; // bit 25, is_unicast
                bwr.put_bit(false)?; // bit 24, is_request
                bwr.put_bit(true)?; // bit 23, reserved
                bwr.put(&self.source)?; // bits 22:16
                bwr.put(&self.destination)?; // bits 15:7 - destination node or node set
                bwr.put(&self.resource_set)?; // bits 6:4 - discriminant of ResourceSet+Uri
                bwr.put(&kind)?; // bits 3:0 - discriminant of XpiReplyKind
                Ok(())
            })?;
            Ok(())
        })?;
        Ok(nwr)
    }
}

impl<'i> DeserializeVlu4<'i> for XpiReply<'i> {
    type Error = XpiVlu4Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        // get first 32 bits as BitBuf
        let mut bits_rdr = rdr.get_bit_buf(8)?;
        let _absent_31_29 = bits_rdr.get_up_to_8(3);

        // bits 28:26
        let priority: Priority = bits_rdr.des_bits()?;

        // bit 25
        let is_unicast = bits_rdr.get_bit()?;
        if !is_unicast {
            return Err(XpiVlu4Error::NotAResponse);
        }

        // bit 24
        let is_response = !bits_rdr.get_bit()?;
        if !is_response {
            return Err(XpiVlu4Error::NotAResponse);
        }

        // UAVCAN reserved bit 23, discard if 0 (UAVCAN discards if 1).
        let reserved_23 = bits_rdr.get_bit()?;
        if !reserved_23 {
            return Err(XpiVlu4Error::ReservedDiscard);
        }

        // bits: 22:16
        let source: NodeId = bits_rdr.des_bits()?;

        // bits: 15:7 + variable nibbles if not NodeSet::Unicast
        let destination = NodeSet::des_coupled_bits_vlu4(&mut bits_rdr, rdr)?;

        // bits 6:4 + 1/2/3/4 nibbles for Uri::OnePart4/TwoPart44/ThreePart* or variable otherwise
        let resource_set = XpiResourceSet::des_coupled_bits_vlu4(&mut bits_rdr, rdr)?;

        // bits 3:0
        let kind = XpiReplyKind::des_coupled_bits_vlu4(&mut bits_rdr, rdr)?;

        // tail byte should be at byte boundary, if not 4b padding is added
        if !rdr.is_at_byte_boundary() {
            let _ = rdr.get_nibble()?;
        }
        let request_id: RequestId = rdr.des_vlu4()?;

        Ok(XpiReply {
            source,
            destination,
            resource_set,
            kind,
            request_id,
            priority,
        })
    }
}

#[cfg(test)]
mod test {
    extern crate std;

    use crate::discrete::{U2Sp1, U4};
    use crate::serdes::xpi_vlu4::addressing::{NodeSet, RequestId, XpiResourceSet};
    use crate::serdes::xpi_vlu4::error::FailReason;
    use crate::serdes::xpi_vlu4::priority::Priority;
    use crate::serdes::xpi_vlu4::reply::{
        XpiReply, XpiReplyBuilder, XpiReplyKind, XpiReplyDiscriminant,
    };
    use crate::serdes::xpi_vlu4::{NodeId, Uri};
    use crate::serdes::{NibbleBuf, NibbleBufMut};
    use hex_literal::hex;

    #[test]
    fn call_reply_ser() {
        let mut buf = [0u8; 32];
        let reply_builder = XpiReplyBuilder::new(
            NibbleBufMut::new_all(&mut buf),
            NodeId::new(85).unwrap(),
            NodeSet::Unicast(NodeId::new(33).unwrap()),
            XpiResourceSet::Uri(Uri::TwoPart44(U4::new(4).unwrap(), U4::new(5).unwrap())),
            RequestId::new(27).unwrap(),
            Priority::Lossy(U2Sp1::new(1).unwrap()),
        )
        .unwrap();
        let nwr = reply_builder
            .build_kind_with(|nwr| {
                let mut vb = nwr.put_vec::<Result<&[u8], FailReason>>();
                vb.put_result_with_slice(Ok(&[0xaa, 0xbb][..]))?;
                vb.put_result_with_slice(Ok(&[0xcc, 0xdd][..]))?;
                let nwr = vb.finish()?;
                Ok((XpiReplyDiscriminant::CallComplete, nwr))
            })
            .unwrap();

        let (buf, len, _) = nwr.finish();
        assert_eq!(len, len);
        assert_eq!(&buf[..len], hex!("02 d5 10 90 45 20 20 aa bb 02 cc dd 1b"));
    }

    #[test]
    fn call_reply_des() {
        let buf = hex!("02 d5 10 90 45 20 20 aa bb 02 cc dd 1b");
        let mut rgr = NibbleBuf::new_all(&buf);

        let reply: XpiReply = rgr.des_vlu4().unwrap();

        assert_eq!(reply.source, NodeId::new(85).unwrap());
        if let NodeSet::Unicast(id) = reply.destination {
            assert_eq!(id, NodeId::new(33).unwrap());
        } else {
            panic!("Expected NodeSet::Unicast(_)");
        }
        if let XpiResourceSet::Uri(uri) = reply.resource_set {
            let mut iter = uri.iter();
            assert_eq!(iter.next(), Some(4));
            assert_eq!(iter.next(), Some(5));
            assert_eq!(iter.next(), None);
        } else {
            panic!("Expected XpiResourceSet::Uri(_)");
        }
        if let XpiReplyKind::CallComplete(result) = reply.kind {
            let mut result_iter = result.iter();
            assert_eq!(result_iter.next(), Some(Ok(&[0xaa, 0xbb][..])));
            assert_eq!(result_iter.next(), Some(Ok(&[0xcc, 0xdd][..])));
            assert_eq!(result_iter.next(), None);
        } else {
            panic!("Expected XpiReplyKind::CallComplete(_)");
        }
        assert_eq!(reply.request_id, RequestId::new(27).unwrap());
        assert_eq!(reply.priority, Priority::Lossy(U2Sp1::new(1).unwrap()));
    }
}

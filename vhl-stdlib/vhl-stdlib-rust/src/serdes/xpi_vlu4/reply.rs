use crate::serdes::bit_buf::BitBufMut;
use crate::serdes::traits::{DeserializeCoupledBitsVlu4, SerializeBits};
use crate::serdes::vlu4::vec::Vlu4Vec;
use crate::serdes::xpi_vlu4::addressing::{NodeSet, RequestId, XpiResourceSet};
use crate::serdes::xpi_vlu4::error::{FailReason, XpiVlu4Error};
use crate::serdes::xpi_vlu4::priority::Priority;
use crate::serdes::xpi_vlu4::resource_info::ResourceInfo;
use crate::serdes::xpi_vlu4::{MultiUri, NodeId, Uri};
use crate::serdes::{BitBuf, NibbleBuf, NibbleBufMut};
use crate::xpi::reply::{XpiGenericReply, XpiGenericReplyKind, XpiReplyDiscriminant};

/// Highly space efficient xPI reply data structure supporting zero copy and no_std without alloc
/// even for variable length arrays or strings.
/// See [XpiGenericReply](crate::xpi::reply::XpiGenericReply) for detailed information.
pub type XpiReply<'rep> = XpiGenericReply<
    Uri<'rep>,
    MultiUri<'rep>,
    Vlu4Vec<'rep, &'rep [u8]>,
    Vlu4Vec<'rep, Result<&'rep [u8], FailReason>>,
    Vlu4Vec<'rep, Result<(), FailReason>>,
    Vlu4Vec<'rep, Result<ResourceInfo<'rep>, FailReason>>,
    RequestId,
>;

/// See [XpiGenericReplyKind](crate::xpi::reply::XpiGenericReplyKind) for detailed information.
pub type XpiReplyKind<'rep> = XpiGenericReplyKind<
    Vlu4Vec<'rep, &'rep [u8]>,
    Vlu4Vec<'rep, Result<&'rep [u8], FailReason>>,
    Vlu4Vec<'rep, Result<(), FailReason>>,
    Vlu4Vec<'rep, Result<ResourceInfo<'rep>, FailReason>>,
>;

impl<'i> SerializeBits for XpiReplyDiscriminant {
    type Error = crate::serdes::bit_buf::Error;

    fn ser_bits(&self, wgr: &mut BitBufMut) -> Result<(), Self::Error> {
        wgr.put_up_to_8(4, *self as u8)?;
        Ok(())
    }
}

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

#[cfg(test)]
mod test {
    extern crate std;

    use crate::discrete::{U2Sp1, U4};
    use crate::serdes::xpi_vlu4::addressing::{NodeSet, RequestId, XpiResourceSet};
    use crate::serdes::xpi_vlu4::error::FailReason;
    use crate::serdes::xpi_vlu4::priority::Priority;
    use crate::serdes::xpi_vlu4::reply::{
        XpiReplyBuilder, XpiReplyKind, XpiReplyDiscriminant,
    };
    use crate::serdes::xpi_vlu4::{NodeId, Uri};
    use crate::serdes::{NibbleBuf, NibbleBufMut};
    use hex_literal::hex;
    use crate::serdes::xpi_vlu4::event::{XpiEvent, XpiEventKind};

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
        let mut nrd = NibbleBuf::new_all(&buf);

        let event: XpiEvent = nrd.des_vlu4().unwrap();

        assert_eq!(event.source, NodeId::new(85).unwrap());
        if let NodeSet::Unicast(id) = event.destination {
            assert_eq!(id, NodeId::new(33).unwrap());
        } else {
            panic!("Expected NodeSet::Unicast(_)");
        }
        if let XpiEventKind::Reply(reply) = event.kind {
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
        } else {
            panic!("Expected XpiEventKind::Reply(_)");
        }
        assert_eq!(event.priority, Priority::Lossy(U2Sp1::new(1).unwrap()));
        assert!(nrd.is_at_end());
    }
}

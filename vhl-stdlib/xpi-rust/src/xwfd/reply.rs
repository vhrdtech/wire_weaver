use vhl_stdlib_nostd::serdes::bit_buf::BitBufMut;
use vhl_stdlib_nostd::serdes::traits::{DeserializeCoupledBitsVlu4, SerializeBits};
use vhl_stdlib_nostd::serdes::vlu4::vec::Vlu4Vec;
use crate::error::XpiError;
use vhl_stdlib_nostd::serdes::{bit_buf, BitBuf, NibbleBuf, NibbleBufMut};
use crate::reply::{XpiGenericReply, XpiGenericReplyKind, XpiReplyDiscriminant};
use crate::xwfd::node_set::NodeSet;
use super::{
    NodeId, Priority,
    RequestId, ResourceInfo, ResourceSet,
    SerialMultiUri, SerialUri,
    XwfdError,
};

/// Highly space efficient xPI reply data structure supporting zero copy and no_std without alloc
/// even for variable length arrays or strings.
/// See [XpiGenericReply](crate::xpi::reply::XpiGenericReply) for detailed information.
pub type Reply<'rep> = XpiGenericReply<
    SerialUri<'rep>,
    SerialMultiUri<'rep>,
    Vlu4Vec<'rep, &'rep [u8]>,
    Vlu4Vec<'rep, Result<&'rep [u8], XpiError>>,
    Vlu4Vec<'rep, Result<(), XpiError>>,
    Vlu4Vec<'rep, Result<ResourceInfo<'rep>, XpiError>>,
    RequestId,
>;

/// See [XpiGenericReplyKind](crate::xpi::reply::XpiGenericReplyKind) for detailed information.
pub type ReplyKind<'rep> = XpiGenericReplyKind<
    Vlu4Vec<'rep, &'rep [u8]>,
    Vlu4Vec<'rep, Result<&'rep [u8], XpiError>>,
    Vlu4Vec<'rep, Result<(), XpiError>>,
    Vlu4Vec<'rep, Result<ResourceInfo<'rep>, XpiError>>,
>;

impl<'i> SerializeBits for XpiReplyDiscriminant {
    type Error = bit_buf::Error;

    fn ser_bits(&self, wgr: &mut BitBufMut) -> Result<(), Self::Error> {
        wgr.put_up_to_8(4, *self as u8)?;
        Ok(())
    }
}

impl<'i> DeserializeCoupledBitsVlu4<'i> for ReplyKind<'i> {
    type Error = XwfdError;

    fn des_coupled_bits_vlu4<'di>(
        bits_rdr: &'di mut BitBuf<'i>,
        vlu4_rdr: &'di mut NibbleBuf<'i>,
    ) -> Result<Self, Self::Error> {
        let kind = bits_rdr.get_up_to_8(4)?;
        use XpiGenericReplyKind::*;
        match kind {
            0 => Ok(CallComplete(vlu4_rdr.des_vlu4()?)),
            _ => Err(XwfdError::Unimplemented),
        }
    }
}

pub struct XpiReplyVlu4Builder<'i> {
    nwr: NibbleBufMut<'i>,
    source: NodeId,
    destination: NodeSet<'i>,
    resource_set: ResourceSet<'i>,
    request_id: RequestId,
    priority: Priority,
}

impl<'i> XpiReplyVlu4Builder<'i> {
    pub fn new(
        mut nwr: NibbleBufMut<'i>,
        source: NodeId,
        destination: NodeSet<'i>,
        resource_set: ResourceSet<'i>,
        request_id: RequestId,
        priority: Priority,
    ) -> Result<Self, XwfdError> {
        nwr.skip(8)?;
        nwr.put(&destination)?;
        nwr.put(&resource_set)?;
        Ok(XpiReplyVlu4Builder {
            nwr,
            source,
            destination,
            resource_set,
            request_id,
            priority,
        })
    }

    pub fn build_kind_with<F>(self, f: F) -> Result<NibbleBufMut<'i>, XpiError>
        where
            F: Fn(NibbleBufMut<'i>) -> Result<(XpiReplyDiscriminant, NibbleBufMut<'i>), XpiError>,
    {
        let (kind, mut nwr) = f(self.nwr)?;
        nwr.put(&self.request_id).unwrap();
        nwr.rewind::<_, XpiError>(0, |nwr| {
            nwr.as_bit_buf::<_, XpiError>(|bwr| {
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

    use vhl_stdlib_nostd::discrete::{U2Sp1, U4};
    use crate::xwfd::resource_set::{RequestId, ResourceSet};
    use crate::xwfd::error::FailReason;
    use crate::xwfd::priority::Priority;
    use crate::xwfd::reply::{
        ReplyKind, XpiReplyDiscriminant, XpiReplyVlu4Builder,
    };
    use crate::xwfd::{NodeId, SerialUri};
    use vhl_stdlib_nostd::serdes::{NibbleBuf, NibbleBufMut};
    use hex_literal::hex;
    use crate::xwfd::event::{Event, EventKind};
    use crate::xwfd::node_set::NodeSet;

    #[test]
    fn call_reply_ser() {
        let mut buf = [0u8; 32];
        let reply_builder = XpiReplyVlu4Builder::new(
            NibbleBufMut::new_all(&mut buf),
            NodeId::new(85).unwrap(),
            NodeSet::Unicast(NodeId::new(33).unwrap()),
            ResourceSet::Uri(SerialUri::TwoPart44(U4::new(4).unwrap(), U4::new(5).unwrap())),
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

        let event: Event = nrd.des_vlu4().unwrap();

        assert_eq!(event.source, NodeId::new(85).unwrap());
        if let NodeSet::Unicast(id) = event.destination {
            assert_eq!(id, NodeId::new(33).unwrap());
        } else {
            panic!("Expected NodeSet::Unicast(_)");
        }
        if let EventKind::Reply(reply) = event.kind {
            if let ResourceSet::Uri(uri) = reply.resource_set {
                let mut iter = uri.iter();
                assert_eq!(iter.next(), Some(4));
                assert_eq!(iter.next(), Some(5));
                assert_eq!(iter.next(), None);
            } else {
                panic!("Expected XpiResourceSet::Uri(_)");
            }
            if let ReplyKind::CallComplete(result) = reply.kind {
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

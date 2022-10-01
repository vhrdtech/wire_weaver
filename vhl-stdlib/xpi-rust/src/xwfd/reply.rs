use core::fmt::{Display, Formatter};
use crate::error::XpiError;
use crate::reply::{XpiGenericReply, XpiGenericReplyKind, XpiReplyDiscriminant};
use crate::xwfd::xwfd_info::XwfdInfo;
use crate::xwfd::node_set::NodeSet;
use vhl_stdlib::{
    serdes::{
        bit_buf, BitBuf, BitBufMut,
        NibbleBuf, NibbleBufMut,
        traits::{DeserializeCoupledBitsVlu4, SerializeBits},
        vlu4::{Vlu32, Vlu4Vec, Vlu4VecIter},
    }
};
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
    SerialUri<Vlu4VecIter<'rep, Vlu32>>,
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

pub struct ReplyBuilder<'i> {
    nwr: NibbleBufMut<'i>,
    source: NodeId,
    destination: NodeSet<'i>,
    resource_set: ResourceSet<'i>,
    request_id: RequestId,
    priority: Priority,
}

impl<'i> ReplyBuilder<'i> {
    pub fn new(
        mut nwr: NibbleBufMut<'i>,
        source: NodeId,
        destination: NodeSet<'i>,
        resource_set: ResourceSet<'i>,
        request_id: RequestId,
        priority: Priority,
    ) -> Result<Self, XwfdError> {
        nwr.skip(8)?;
        nwr.put(&XwfdInfo::FormatIsXwfd)?;
        nwr.put(&destination)?;
        nwr.put(&resource_set)?;
        Ok(ReplyBuilder {
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
                bwr.put_bit(true)?; // bit 23, is_xwfd_or_bigger
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

impl<'i> Display for Reply<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Reply<@{}> {{ {:#} {:?} }}",
            self.request_id,
            self.resource_set,
            self.kind,
        )
    }
}

#[cfg(test)]
mod test {
    extern crate std;

    use vhl_stdlib::discrete::{U2, U4};
    use crate::xwfd::*;
    use vhl_stdlib::serdes::{NibbleBuf, NibbleBufMut};
    use hex_literal::hex;
    use crate::error::XpiError;
    use crate::reply::XpiReplyDiscriminant;

    #[test]
    fn call_reply_ser() {
        let mut buf = [0u8; 32];
        let reply_builder = ReplyBuilder::new(
            NibbleBufMut::new_all(&mut buf),
            NodeId::new(85).unwrap(),
            NodeSet::Unicast(NodeId::new(33).unwrap()),
            ResourceSet::Uri(SerialUri::TwoPart44(U4::new(4).unwrap(), U4::new(5).unwrap())),
            RequestId::new(27).unwrap(),
            Priority::Lossy(U2::new(0).unwrap()),
        )
            .unwrap();
        let nwr = reply_builder
            .build_kind_with(|nwr| {
                let mut vb = nwr.put_vec::<Result<&[u8], XpiError>>();
                vb.put(&Ok(&[0xaa, 0xbb][..]))?;
                vb.put(&Ok(&[0xcc, 0xdd][..]))?;
                let nwr = vb.finish()?;
                Ok((XpiReplyDiscriminant::CallComplete, nwr))
            })
            .unwrap();

        let (buf, len, _) = nwr.finish();
        assert_eq!(len, len);
        assert_eq!(&buf[..len], hex!("02 d5 10 90 04 52 02 aa bb 02 cc dd 1b"));
    }

    #[test]
    fn call_reply_des() {
        let buf = hex!("02 d5 10 90 04 52 02 aa bb 02 cc dd 1b");
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
        assert_eq!(event.priority, Priority::Lossy(U2::new(0).unwrap()));
        assert!(nrd.is_at_end());
    }
}

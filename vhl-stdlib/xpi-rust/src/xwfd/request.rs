use super::NodeId;
use vhl_stdlib_nostd::serdes::bit_buf::BitBufMut;
use vhl_stdlib_nostd::serdes::traits::{DeserializeCoupledBitsVlu4, SerializeBits};
use vhl_stdlib_nostd::serdes::vlu4::{Vlu32, Vlu4Vec, Vlu4VecIter};
use crate::error::XpiError;
use vhl_stdlib_nostd::serdes::{bit_buf, BitBuf, NibbleBuf, NibbleBufMut};
use core::fmt::{Display, Formatter};
use crate::request::{XpiGenericRequest, XpiGenericRequestKind, XpiRequestDiscriminant};
use crate::xwfd::xwfd_info::XwfdInfo;
use crate::xwfd::node_set::NodeSet;
use super::{
    Priority, Rate, RequestId, ResourceSet,
    SerialMultiUri, SerialUri,
    XwfdError,
};

/// Highly space efficient xPI request data structure supporting zero copy and no_std without alloc
/// even for variable length arrays or strings.
/// See [XpiGenericRequest](crate::xpi::request::XpiGenericRequest) for detailed information.
pub type XpiRequestVlu4<'req> = XpiGenericRequest<
    SerialUri<Vlu4VecIter<'req, Vlu32>>,
    SerialMultiUri<'req>,
    &'req [u8],
    Vlu4Vec<'req, &'req [u8]>,
    Vlu4Vec<'req, Rate>,
    RequestId,
>;

/// See [XpiGenericRequestKind](crate::xpi::request::XpiGenericRequestKind) for detailed information.
pub type XpiRequestKindVlu4<'req> = XpiGenericRequestKind<
    &'req [u8],
    Vlu4Vec<'req, &'req [u8]>,
    Vlu4Vec<'req, Rate>
>;

impl<'i> Display for XpiRequestVlu4<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "XpiRequest<@{}> {{ {:#} {:?} }}",
            self.request_id,
            self.resource_set,
            self.kind,
        )
    }
}

impl<'i> SerializeBits for XpiRequestDiscriminant {
    type Error = bit_buf::Error;

    fn ser_bits(&self, wgr: &mut BitBufMut) -> Result<(), Self::Error> {
        wgr.put_up_to_8(4, *self as u8)?;
        Ok(())
    }
}

pub struct RequestBuilder<'i> {
    nwr: NibbleBufMut<'i>,
    source: NodeId,
    destination: NodeSet<'i>,
    resource_set: ResourceSet<'i>,
    request_id: RequestId,
    priority: Priority,
}

impl<'i> RequestBuilder<'i> {
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
        Ok(RequestBuilder {
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
            F: Fn(NibbleBufMut<'i>) -> Result<(XpiRequestDiscriminant, NibbleBufMut<'i>), XpiError>,
    {
        let (kind, mut nwr) = f(self.nwr)?;
        nwr.put(&self.request_id).unwrap();
        nwr.rewind::<_, XpiError>(0, |nwr| {
            nwr.as_bit_buf::<_, XpiError>(|bwr| {
                bwr.put_up_to_8(3, 0b000)?; // unused 31:29
                bwr.put(&self.priority)?; // bits 28:26
                bwr.put_bit(true)?; // bit 25, is_unicast
                bwr.put_bit(true)?; // bit 24, is_request
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

impl<'i> DeserializeCoupledBitsVlu4<'i> for XpiRequestKindVlu4<'i> {
    type Error = XwfdError;

    fn des_coupled_bits_vlu4<'di>(
        bits_rdr: &'di mut BitBuf<'i>,
        vlu4_rdr: &'di mut NibbleBuf<'i>,
    ) -> Result<Self, Self::Error> {
        let kind = bits_rdr.get_up_to_8(4)?;
        use XpiGenericRequestKind::*;
        match kind {
            0 => Ok(Call {
                args_set: vlu4_rdr.des_vlu4()?,
            }),
            1 => Ok(Read),
            2 => Ok(Write {
                values: vlu4_rdr.des_vlu4()?,
            }),
            3 => Ok(OpenStreams),
            4 => Ok(CloseStreams),
            5 => Ok(Subscribe {
                rates: vlu4_rdr.des_vlu4()?,
            }),
            6 => Ok(Unsubscribe),
            7 => Ok(Borrow),
            8 => Ok(Release),
            9 => Ok(Introspect),
            10 => Ok(ChainCall {
                args: vlu4_rdr.des_vlu4()?,
            }),
            11..=15 => Err(XwfdError::ReservedDiscard),
            _ => Err(XwfdError::InternalError),
        }
    }
}

#[cfg(test)]
mod test {
    extern crate std;

    use vhl_stdlib_nostd::discrete::{U2, U4};
    use vhl_stdlib_nostd::serdes::{NibbleBuf, NibbleBufMut};
    use crate::request::XpiRequestDiscriminant;
    pub use crate::xwfd::{
        Event, EventKind,
        NodeId, Priority, RequestId, ResourceSet, SerialUri,
        XpiRequestKindVlu4, RequestBuilder,
    };
    pub use crate::xwfd::node_set::NodeSet;

    #[test]
    fn call_request_des() {
        let buf = [
            0b000_100_11,
            0b1_0101010,
            0b00_101010,
            0b1_001_0000,
            0b0011_1100,
            0b0001_0010,
            0xaa,
            0xbb,
            0b000_11011,
        ];
        let mut nrd = NibbleBuf::new_all(&buf);
        let event: Event = nrd.des_vlu4().unwrap();
        // println!("{}", event);

        assert_eq!(event.priority, Priority::Lossless(U2::new(0).unwrap()));
        assert_eq!(event.source, NodeId::new(42).unwrap());
        assert!(matches!(event.destination, NodeSet::Unicast(_)));
        if let NodeSet::Unicast(id) = event.destination {
            assert_eq!(id, NodeId::new(85).unwrap());
        }
        if let EventKind::Request(request) = event.kind {
            assert_eq!(request.request_id, RequestId::new(27).unwrap());
            assert!(matches!(request.resource_set, ResourceSet::Uri(_)));
            if let ResourceSet::Uri(uri) = request.resource_set {
                assert!(matches!(uri, SerialUri::TwoPart44(_, _)));
                if let SerialUri::TwoPart44(a, b) = uri {
                    assert_eq!(a.inner(), 3);
                    assert_eq!(b.inner(), 12);
                }
            }
            assert!(matches!(request.kind, XpiRequestKindVlu4::Call { .. }));
            if let XpiRequestKindVlu4::Call { args_set: args } = request.kind {
                assert_eq!(args.len(), 1);
                let slice = args.iter().next().unwrap();
                assert_eq!(slice.len(), 2);
                assert_eq!(slice[0], 0xaa);
                assert_eq!(slice[1], 0xbb);
            }
        } else {
            panic!("Expected XpiEventKind::Request(_)");
        }
        assert!(nrd.is_at_end());
    }

    #[test]
    fn call_request_ser() {
        let mut buf = [0u8; 32];
        let request_builder = RequestBuilder::new(
            NibbleBufMut::new_all(&mut buf),
            NodeId::new(42).unwrap(),
            NodeSet::Unicast(NodeId::new(85).unwrap()),
            ResourceSet::Uri(SerialUri::TwoPart44(U4::new(3).unwrap(), U4::new(12).unwrap())),
            RequestId::new(27).unwrap(),
            Priority::Lossless(U2::new(0).unwrap()),
        )
            .unwrap();
        let nwr = request_builder
            .build_kind_with(|nwr| {
                let mut vb = nwr.put_vec::<&[u8]>();

                vb.put(&&[0xaa, 0xbb][..])?;

                let nwr = vb.finish()?;
                Ok((XpiRequestDiscriminant::Call, nwr))
            })
            .unwrap();

        let (buf, len, _) = nwr.finish();
        assert_eq!(len, 9);
        let buf_expected = [
            0b000_100_11,
            0b1_0101010,
            0b00_101010,
            0b1_001_0000,
            0b0011_1100,
            0b0001_0010,
            0xaa,
            0xbb,
            0b000_11011,
        ];
        assert_eq!(&buf[0..len], &buf_expected);
    }
}

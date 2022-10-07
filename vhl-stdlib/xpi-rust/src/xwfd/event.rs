use core::fmt::{Display, Formatter};
use vhl_stdlib::{
    discrete::U4,
    serdes::{
        DeserializeCoupledBitsVlu4, DeserializeVlu4, NibbleBuf, NibbleBufMut,
        vlu4::{
            TraitSet, Vlu32, Vlu4VecIter,
        },
    },
};
use vhl_stdlib::discrete::{U3, U9};
use crate::error::XpiError;
use crate::event::XpiGenericEvent;
use crate::event_kind::XpiEventDiscriminant;
use super::{
    error::XwfdError, NodeId,
    Priority, RequestId, ResourceSet,
    SerialUri, SerialMultiUri, NodeSet,
    EventKind, XwfdInfo,
};

/// Highly space efficient xPI Event data structure supporting zero copy and no_std without alloc
/// even for variable length arrays or strings.
/// See [XpiGenericEvent](crate:::event::XpiGenericEvent) for detailed information.
pub type Event<'ev> = XpiGenericEvent<
    NodeId,
    TraitSet<'ev>,
    SerialUri<Vlu4VecIter<'ev, Vlu32>>,
    SerialMultiUri<'ev>,
    EventKind<'ev>,
    Priority,
    RequestId,
    U4,
>;

impl<'i> DeserializeVlu4<'i> for Event<'i> {
    type Error = XwfdError;

    fn des_vlu4<'di>(nrd: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        // get first 32 bits as BitBuf
        let mut bits_rdr = nrd.get_bit_buf(8)?;
        let _absent_31_29 = bits_rdr.get_up_to_8(3);

        // bits 28:26
        let priority: Priority = bits_rdr.des_bits()?;

        // bit 25:24
        let kind54 = bits_rdr.get_up_to_8(2)?;

        // bit 23: is_xwfd_or_bigger
        let is_xwfd_or_bigger = bits_rdr.get_bit()?;
        if !is_xwfd_or_bigger {
            return Err(XwfdError::ReservedDiscard);
        }
        let format_info: XwfdInfo = nrd.des_vlu4()?;
        if format_info != XwfdInfo::FormatIsXwfd {
            return Err(XwfdError::WrongFormat);
        }
        let ttl = unsafe { U4::new_unchecked(nrd.get_nibble()?) };

        // bits: 22:16
        let source: NodeId = bits_rdr.des_bits()?;

        // bits: 15:7 + variable nibbles if not NodeSet::Unicast
        let destination = NodeSet::des_coupled_bits_vlu4(&mut bits_rdr, nrd)?;

        // bits 6:4 + 1/2/3/4 nibbles for Uri::OnePart4/TwoPart44/ThreePart* or variable otherwise
        let resource_set = ResourceSet::des_coupled_bits_vlu4(&mut bits_rdr, nrd)?;

        let kind30 = bits_rdr.get_up_to_8(4)?;
        let kind = (kind54 << 4) | kind30;
        let kind = EventKind::des_vlu4_with_discriminant(kind, nrd)?;
        nrd.align_to_byte()?;
        let request_id: RequestId = nrd.des_vlu4()?;

        Ok(Event {
            source,
            destination,
            resource_set,
            kind,
            priority,
            ttl,
            request_id,
        })
    }
}

pub struct EventBuilder<'i> {
    nwr: NibbleBufMut<'i>,
    source: NodeId,
    request_id: RequestId,
    priority: Priority,
}

pub struct EventBuilderResourceSetState<'i> {
    nwr: NibbleBufMut<'i>,
    source: NodeId,
    request_id: RequestId,
    priority: Priority,
    node_set_header: U9,
}

pub struct EventBuilderKindState<'i> {
    nwr: NibbleBufMut<'i>,
    source: NodeId,
    request_id: RequestId,
    priority: Priority,
    node_set_header: U9,
    resource_set_header: U3,
}

impl<'i> EventBuilder<'i> {
    pub fn new(
        mut nwr: NibbleBufMut<'i>,
        source: NodeId,
        request_id: RequestId,
        priority: Priority,
        ttl: U4,
    ) -> Result<Self, XwfdError> {
        nwr.skip(8)?;
        nwr.put(&XwfdInfo::FormatIsXwfd)?;
        nwr.put_nibble(ttl.inner())?;
        Ok(EventBuilder {
            nwr,
            source,
            request_id,
            priority,
        })
    }

    pub fn build_node_set_with<F>(self, f: F) -> Result<EventBuilderResourceSetState<'i>, XpiError>
        where
            F: Fn(NibbleBufMut<'i>) -> Result<(U9, NibbleBufMut<'i>), XpiError>,
    {
        let (node_set_header, nwr) = f(self.nwr)?;
        Ok(EventBuilderResourceSetState {
            nwr,
            source: self.source,
            request_id: self.request_id,
            priority: self.priority,
            node_set_header,
        })
    }
}

impl<'i> EventBuilderResourceSetState<'i> {
    pub fn build_resource_set_with<F>(self, f: F) -> Result<EventBuilderKindState<'i>, XpiError>
        where
            F: Fn(NibbleBufMut<'i>) -> Result<(U3, NibbleBufMut<'i>), XpiError>,
    {
        let (resource_set_header, nwr) = f(self.nwr)?;
        Ok(EventBuilderKindState {
            nwr,
            source: self.source,
            request_id: self.request_id,
            priority: self.priority,
            node_set_header: self.node_set_header,
            resource_set_header,
        })
    }
}

impl<'i> EventBuilderKindState<'i> {
    pub fn build_kind_with<F>(self, f: F) -> Result<NibbleBufMut<'i>, XpiError>
        where
            F: Fn(NibbleBufMut<'i>) -> Result<(XpiEventDiscriminant, NibbleBufMut<'i>), XpiError>,
    {
        let (kind, mut nwr) = f(self.nwr)?;
        let kind = kind as u8;
        let kind54 = kind >> 4;
        let kind30 = kind & 0xF;
        nwr.align_to_byte()?;
        nwr.put(&self.request_id).unwrap();
        nwr.rewind::<_, XpiError>(0, |nwr| {
            nwr.as_bit_buf::<_, XpiError>(|bwr| {
                bwr.put_up_to_8(3, 0b000)?; // unused 31:29
                bwr.put(&self.priority)?; // bits 28:26
                bwr.put_up_to_8(2, kind54)?; // bits 25:24 - discriminant of event kind
                bwr.put_bit(true)?; // bit 23, is_xwfd_or_bigger
                bwr.put(&self.source)?; // bits 22:16
                bwr.put_up_to_16(9, self.node_set_header.inner())?; // bits 15:7 - dst node set discriminant + node id if unicast
                bwr.put_up_to_8(3, self.resource_set_header.inner())?; // bits 6:4 - discriminant of ResourceSet+Uri
                bwr.put_up_to_8(4, kind30)?; // bits 3:0 - discriminant of event kind
                Ok(())
            })?;
            Ok(())
        })?;
        Ok(nwr)
    }
}

impl<'i> Display for Event<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "XwfdEvent{{ {} -> {}::{}: {} #{} {} }}",
            self.source,
            self.destination,
            self.resource_set,
            self.kind,
            self.request_id,
            self.priority
        )
    }
}

#[cfg(test)]
mod test {
    extern crate std;

    use hex_literal::hex;
    use vhl_stdlib::discrete::{U2, U4};
    use vhl_stdlib::serdes::{NibbleBuf, NibbleBufMut};
    use crate::error::XpiError;
    use crate::event_kind::XpiEventDiscriminant;
    pub use crate::xwfd::{
        Event, EventKind,
        NodeId, Priority, RequestId, ResourceSet, SerialUri,
        EventBuilder, XwfdError, NodeSet,
    };

    #[test]
    fn des_is_xwdf_or_bigger_false() {
        let buf = hex!("02 55 10 90 04 50");
        let mut nrd = NibbleBuf::new_all(&buf);
        let r: Result<Event, XwfdError> = nrd.des_vlu4();

        matches!(r, Err(XwfdError::WrongFormat));
    }

    #[test]
    fn des_is_xwdf_info_other_format() {
        let buf = hex!("02 d5 10 90 84 50");
        let mut nrd = NibbleBuf::new_all(&buf);
        let r: Result<Event, XwfdError> = nrd.des_vlu4();

        matches!(r, Err(XwfdError::WrongFormat));
    }

    #[test]
    fn call_request_des() {
        let buf = [
            0b000_100_00, // n/a, priority, event kind group = requests
            0b1_0101010, // is_xwfd_or_bigger, source
            0b00_101010, // node set kind, destination 7:1
            0b1_001_0000, // destination 0, resources set kind, request kind = Call
            0b0000_1010, // xwfd_info, ttl
            0b0011_1100, // resource set: U4 / U4
            0b0001_0010, // args set len = 1, slice len = 2 + no padding
            0xaa,
            0xbb,
            0b000_11011,
        ];
        let mut nrd = NibbleBuf::new_all(&buf);
        let event: Event = nrd.des_vlu4().unwrap();
        // println!("{}", event);

        assert_eq!(event.priority, Priority::Lossless(U2::new(0).unwrap()));
        assert_eq!(event.source, NodeId::new(42).unwrap());
        assert_eq!(event.ttl, U4::new(0xa).unwrap());
        assert!(matches!(event.destination, NodeSet::Unicast(_)));
        if let NodeSet::Unicast(id) = event.destination {
            assert_eq!(id, NodeId::new(85).unwrap());
        }
        assert_eq!(event.request_id, RequestId::new(27).unwrap());
        assert!(matches!(event.resource_set, ResourceSet::Uri(_)));
        if let ResourceSet::Uri(uri) = event.resource_set {
            assert!(matches!(uri, SerialUri::TwoPart44(_, _)));
            if let SerialUri::TwoPart44(a, b) = uri {
                assert_eq!(a.inner(), 3);
                assert_eq!(b.inner(), 12);
            }
        }
        assert!(matches!(event.kind, EventKind::Call { .. }));
        if let EventKind::Call { args_set: args } = event.kind {
            assert_eq!(args.len(), 1);
            let slice = args.iter().next().unwrap();
            assert_eq!(slice.len(), 2);
            assert_eq!(slice[0], 0xaa);
            assert_eq!(slice[1], 0xbb);
        }
        assert!(nrd.is_at_end());
    }

    #[test]
    fn call_request_ser() {
        let mut buf = [0u8; 32];
        let request_builder = EventBuilder::new(
            NibbleBufMut::new_all(&mut buf),
            NodeId::new(42).unwrap(),
            NodeSet::Unicast(NodeId::new(85).unwrap()),
            ResourceSet::Uri(SerialUri::TwoPart44(U4::new(3).unwrap(), U4::new(12).unwrap())),
            RequestId::new(27).unwrap(),
            Priority::Lossless(U2::new(0).unwrap()),
            U4::new(0xa).unwrap(),
        )
            .unwrap();
        let nwr = request_builder
            .build_kind_with(|nwr| {
                let mut vb = nwr.put_vec::<&[u8]>();

                vb.put(&&[0xaa, 0xbb][..])?;

                let nwr = vb.finish()?;
                Ok((XpiEventDiscriminant::Call, nwr))
            })
            .unwrap();

        let (buf, len, _) = nwr.finish();
        assert_eq!(len, 10);
        let buf_expected = [
            0b000_100_00, // n/a, priority, event kind group = requests
            0b1_0101010, // is_xwfd_or_bigger, source
            0b00_101010, // node set kind, destination 7:1
            0b1_001_0000, // destination 0, resources set kind, request kind = Call
            0b0000_1010, // xwfd_info, ttl
            0b0011_1100, // resource set: U4 / U4
            0b0001_0010, // args set len = 1, slice len = 2 + no padding
            0xaa,
            0xbb,
            0b000_11011,
        ];
        assert_eq!(&buf[0..len], &buf_expected);
    }

    #[test]
    fn call_reply_ser() {
        let mut buf = [0u8; 32];
        let reply_builder = EventBuilder::new(
            NibbleBufMut::new_all(&mut buf),
            NodeId::new(85).unwrap(),
            NodeSet::Unicast(NodeId::new(33).unwrap()),
            ResourceSet::Uri(SerialUri::TwoPart44(U4::new(4).unwrap(), U4::new(5).unwrap())),
            RequestId::new(27).unwrap(),
            Priority::Lossy(U2::new(0).unwrap()),
            U4::new(0xa).unwrap(),
        )
            .unwrap();
        let nwr = reply_builder
            .build_kind_with(|nwr| {
                let mut vb = nwr.put_vec::<Result<&[u8], XpiError>>();
                vb.put(&Ok(&[0xaa, 0xbb][..]))?;
                vb.put(&Ok(&[0xcc, 0xdd][..]))?;
                let nwr = vb.finish()?;
                Ok((XpiEventDiscriminant::CallResults, nwr))
            })
            .unwrap();

        let (buf, len, _) = nwr.finish();
        assert_eq!(len, len);
        assert_eq!(&buf[..len], hex!("01 d5 10 90 0a 45 20 20 aa bb 02 cc dd 1b"));
    }

    #[test]
    fn call_reply_des() {
        let buf = hex!("01 d5 10 90 0a 45 20 20 aa bb 02 cc dd 1b");
        let mut nrd = NibbleBuf::new_all(&buf);

        let event: Event = nrd.des_vlu4().unwrap();

        assert_eq!(event.source, NodeId::new(85).unwrap());
        if let NodeSet::Unicast(id) = event.destination {
            assert_eq!(id, NodeId::new(33).unwrap());
        } else {
            panic!("Expected NodeSet::Unicast(_)");
        }
        if let ResourceSet::Uri(uri) = event.resource_set {
            let mut iter = uri.iter();
            assert_eq!(iter.next(), Some(4));
            assert_eq!(iter.next(), Some(5));
            assert_eq!(iter.next(), None);
        } else {
            panic!("Expected ResourceSet::Uri(_)");
        }
        if let EventKind::CallResults(result) = event.kind {
            let mut result_iter = result.iter();
            assert_eq!(result_iter.next(), Some(Ok(&[0xaa, 0xbb][..])));
            assert_eq!(result_iter.next(), Some(Ok(&[0xcc, 0xdd][..])));
            assert_eq!(result_iter.next(), None);
        } else {
            panic!("Expected EventKind::CallResults(_)");
        }
        assert_eq!(event.request_id, RequestId::new(27).unwrap());
        assert_eq!(event.priority, Priority::Lossy(U2::new(0).unwrap()));
        assert_eq!(event.ttl, U4::new(0xa).unwrap());
        assert!(nrd.is_at_end());
    }
}

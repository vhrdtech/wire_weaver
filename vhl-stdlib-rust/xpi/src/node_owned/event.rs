// use crate::event::XpiGenericEvent;
// use crate::owned::convert_error::ConvertError;
// use crate::owned::Priority;
// use crate::xwfd;
// use crate::xwfd::xwfd_info::XwfdInfo;
// use std::fmt::{Display, Formatter};
// use vhl_stdlib::discrete::U4;
// use vhl_stdlib::serdes::NibbleBufMut;

// use super::{
//     resource_set::ResourceSetConvertXwfd, EventKind, MultiUriOwned, NodeId, NodeSet, RequestId,
//     ResourceSet, TraitSet, UriOwned,
// };

pub struct Event {
    pub source: NodeId,
    pub destination: NodeSet,
    pub base_nrl: (),
    pub kind: (),
    pub priority: Priority,
    pub seq: Option<u32>,
    pub ttl: Ttl,
}

pub struct NodeId(u32);

pub enum NodeSet {
    Unicast {
        node_id: NodeId,
    },

    /// both only for 'impl once <trait>';
    /// otherwise: introspect, figure out a full nrl and use as normal
    UnicastTraits {
        node_id: NodeId,
        // all nrl all relative to this traits in order, /0 - first trait
        traits: TraitSet,
    },
    Multicast {
        // all nrls relative to traits
        all_that_impl: TraitSet,
        original_source: NodeId,
    },

    Broadcast,
}

pub struct Ttl(u8);
pub type Priority = ();

pub type TraitSet = Vec<TraitDescriptor>;
pub type TraitDescriptor = u64;
// pub type Event = XpiGenericEvent<
//     NodeId,
//     TraitSet,
//     UriOwned,
//     MultiUriOwned,
//     EventKind,
//     Priority,
//     RequestId,
//     U4, // ttl
// >;

// impl Event {
//     pub fn new_with_default_ttl(
//         source: NodeId,
//         destination: NodeSet,
//         resource_set: ResourceSet,
//         kind: EventKind,
//         request_id: RequestId,
//         priority: Priority,
//     ) -> Self {
//         Event {
//             source,
//             destination,
//             resource_set,
//             kind,
//             priority,
//             request_id,
//             ttl: U4::max(),
//         }
//     }
// }

// impl Event {
//     pub fn new_heartbeat(
//         source: NodeId,
//         request_id: RequestId,
//         priority: Priority,
//         heartbeat_info: u32,
//     ) -> Self {
//         Event {
//             source,
//             destination: NodeSet::Broadcast {
//                 original_source: source,
//             },
//             resource_set: ResourceSet::Uri(UriOwned::empty()),
//             kind: EventKind::Heartbeat(heartbeat_info),
//             priority,
//             request_id,
//             ttl: U4::max(),
//         }
//     }

//     pub fn ser_xwfd(&self, nwr: &mut NibbleBufMut) -> Result<(), ConvertError> {
//         // Some(_) if resource set is Uri only & it's a request or response
//         let mut resource_set: Option<ResourceSetConvertXwfd> = None;
//         let kind = self.kind.discriminant() as u8;
//         let kind54 = kind >> 4;
//         let kind30 = kind & 0xF;

//         nwr.as_bit_buf::<_, ConvertError>(|bwr| {
//             bwr.put_up_to_8(3, 0b000)?; // unused 31:29
//             let priority: xwfd::Priority = self.priority.try_into()?;
//             bwr.put(&priority)?; // bits 28:26
//             bwr.put_up_to_8(2, kind54)?; // bits 25:24 - event kind
//             bwr.put_bit(true)?; // bit 23 - is_xwfd_or_bigger
//             let node_id: xwfd::NodeId = self.source.try_into()?;
//             bwr.put(&node_id)?; // bits 22:16
//             self.destination.ser_header_xwfd(bwr)?; // bits 15:7 - destination node or node set

//             let rsc = self.resource_set.ser_header_xwfd()?;
//             bwr.put(&rsc.discriminant())?;
//             resource_set = Some(rsc);

//             bwr.put_up_to_8(4, kind30)?;
//             Ok(())
//         })?;

//         nwr.put(&XwfdInfo::FormatIsXwfd)?;
//         nwr.put_nibble(self.ttl.inner())?;
//         self.destination.ser_body_xwfd(nwr)?;
//         resource_set.expect("").ser_body_xwfd(nwr)?;
//         self.kind.ser_body_xwfd(nwr)?;
//         nwr.align_to_byte()?;
//         let request_id: xwfd::RequestId = self.request_id.try_into()?;
//         nwr.put(&request_id)?;
//         Ok(())
//     }
// }

// impl Display for Event {
//     fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
//         write!(
//             f,
//             "OwnedEvent{{ {} -> {}::{}: {} #{} {} }}",
//             self.source,
//             self.destination,
//             self.resource_set,
//             self.kind,
//             self.request_id,
//             self.priority
//         )
//     }
// }

// // impl Debug for Event {
// //     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
// //         write!(f, "{}", self)
// //     }
// // }

// impl<'i> From<xwfd::Event<'i>> for Event {
//     fn from(ev: xwfd::Event) -> Self {
//         Event {
//             source: ev.source.into(),
//             destination: ev.destination.into(),
//             resource_set: ev.resource_set.into(),
//             kind: ev.kind.into(),
//             priority: ev.priority.into(),
//             request_id: ev.request_id.into(),
//             ttl: ev.ttl,
//         }
//     }
// }

// #[cfg(test)]
// mod test {
//     use crate::owned::{
//         Event, EventKind, NodeId, NodeSet, Priority, RequestId, ResourceSet, UriOwned,
//     };
//     use vhl_stdlib::discrete::U4;
//     use vhl_stdlib::serdes::{NibbleBuf, NibbleBufMut};

//     #[test]
//     fn ser_xwfd_request() {
//         let ev = Event {
//             source: NodeId(42),
//             destination: NodeSet::Unicast(NodeId(85)),
//             resource_set: ResourceSet::Uri(UriOwned::new(&[3, 12])),
//             kind: EventKind::Call {
//                 args_set: vec![NibbleBuf::new_all(&[0xaa, 0xbb]).to_nibble_buf_owned()],
//             },
//             priority: Priority::Lossless(0),
//             request_id: RequestId(27),
//             ttl: U4::new(0xa).unwrap(),
//         };
//         let mut buf = [0u8; 256];
//         let mut nwr = NibbleBufMut::new_all(&mut buf);
//         ev.ser_xwfd(&mut nwr).unwrap();
//         println!("{}", nwr);
//         let (_, len, _) = nwr.finish();
//         // assert_eq!(len, 10);
//         let expected = [
//             0b000_100_00, // n/a, priority, event kind group = requests
//             0b1_0101010,  // is_xwfd_or_bigger, source
//             0b00_101010,  // node set kind, destination 7:1
//             0b1_001_0000, // destination 0, resources set kind, request kind = Call
//             0b0000_1010,  // xwfd_info, ttl
//             0b0011_1100,  // resource set: U4 / U4
//             0b0001_0010,  // args set len = 1, slice len = 2 + no padding
//             0xaa,
//             0xbb,
//             0b000_11011,
//         ];
//         assert_eq!(buf[..len], expected);
//     }
// }

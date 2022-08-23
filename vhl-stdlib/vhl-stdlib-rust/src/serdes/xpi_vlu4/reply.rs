use crate::serdes::bit_buf::BitBufMut;
use crate::serdes::{BitBuf, DeserializeVlu4, NibbleBuf, NibbleBufMut};
use crate::serdes::traits::{DeserializeCoupledBitsVlu4, SerializeBits, SerializeVlu4};
use crate::serdes::vlu4::slice::Vlu4Slice;
use crate::serdes::vlu4::Vlu4SliceArray;
use crate::serdes::xpi_vlu4::addressing::{NodeSet, RequestId, XpiResourceSet};
use crate::serdes::xpi_vlu4::error::{FailReason, XpiVlu4Error};
use crate::serdes::xpi_vlu4::NodeId;
use crate::serdes::xpi_vlu4::priority::Priority;
use crate::serdes::xpi_vlu4::resource_info::ResourceInfo;

/// Replies are sent to the Link in response to requests.
/// One request can result in one or more replies.
/// For subscriptions and streams many replies will be sent asynchronously.
#[derive(Copy, Clone, Debug)]
pub struct XpiReply<'rep> {
    /// Source node id that yielded reply
    pub source: NodeId,
    /// Destination node or nodes
    pub destination: NodeSet<'rep>,
    /// Set of resources that are considered in this reply
    pub resource_set: XpiResourceSet<'rep>,
    /// Kind of reply
    pub kind: XpiReplyKind<'rep>,
    /// Original request id used to map responses to requests.
    /// For StreamsUpdates use previous id + 1 and do not map to requests.
    pub request_id: RequestId,
    /// Most same priority as initial XpiRequest
    pub priority: Priority,
}

/// Reply to a previously made request
/// Each reply must also be linked with:
/// request id that was sent initially
/// Source node id
#[derive(Copy, Clone, Debug)]
pub enum XpiReplyKind<'rep> {
    /// Result of an each call
    /// TODO: should be Array<Result<Vlu4Slice, FailReason>> = Vlu4ResultSliceArray or just treat as?
    /// the only difference is saving one nibble when slice len + result gives aligned slice next
    CallComplete(Vlu4SliceArray<'rep>),

    /// Result of an each read.
    ReadComplete(Result<Vlu4SliceArray<'rep>, FailReason>),

    /// Result of an each read
    WriteComplete(Result<(), FailReason>),

    /// Result of an attempt to open a stream.
    /// If stream was closed before (and inherently not borrowed), Borrow(Ok(())) is received,
    /// followed by OpenStream(Ok(()))
    OpenStream(Result<(), FailReason>),

    /// Changed property or new element of a stream.
    /// request_id for this case is None, as counter may wrap many times while subscriptions are active.
    /// Mapping is straight forward without a request_id, since uri for each resource is known.
    /// Distinguishing between different updates is not needed as in case of 2 function calls vs 1 for example.
    ///
    /// Updates may be silently lost if lossy mode is selected, more likely so with lower priority.
    ///
    /// Updates are very unlikely to be lost in lossless mode, unless underlying channel is destroyed
    /// or memory is exceeded, in which case only an error can be reported to flag the issue.
    /// If lossless channel is affected, CloseStream is yielded with a failure reason indicated in it.
    StreamUpdate(Vlu4Slice<'rep>),

    /// Result of an attempt to close a stream or unrecoverable loss in lossless mode (priority > 0).
    /// If stream was open before (and inherently borrowed by self node), Close(Ok(())) is received,
    /// followed by Release(Ok(())).
    CloseStream(Result<(), FailReason>),

    /// Result of an attempt to subscribe to a stream or observable property
    /// On success Some(current value) is returned for a property, first available item is returned
    /// for streams, if available during subscription time.
    Subscribe(Result<Option<Vlu4Slice<'rep>>, FailReason>),

    /// Result of a request to change observing / publishing rate.
    RateChange(Result<(), FailReason>),

    /// Result of an attempt to unsubscribe from a stream of from an observable property.
    /// Unsubscribing twice will result in an error.
    Unsubscribe(Result<(), FailReason>),

    /// Result of a resource borrow
    Borrow(Result<(), FailReason>),
    /// Result of a resource release
    Release(Result<(), FailReason>),

    /// Result of an Introspect request
    Introspect(Result<ResourceInfo<'rep>, FailReason>),
}

impl<'i> SerializeBits for XpiReplyKind<'i> {
    type Error = XpiVlu4Error;

    fn ser_bits(&self, wgr: &mut BitBufMut) -> Result<(), Self::Error> {
        let kind = match self {
            XpiReplyKind::CallComplete(_) => 0,
            XpiReplyKind::ReadComplete(_) => 1,
            XpiReplyKind::WriteComplete(_) => 2,
            XpiReplyKind::OpenStream(_) => 3,
            XpiReplyKind::StreamUpdate(_) => 4,
            XpiReplyKind::CloseStream(_) => 5,
            XpiReplyKind::Subscribe(_) => 6,
            XpiReplyKind::RateChange(_) => 7,
            XpiReplyKind::Unsubscribe(_) => 8,
            XpiReplyKind::Borrow(_) => 9,
            XpiReplyKind::Release(_) => 10,
            XpiReplyKind::Introspect(_) => 11,
        };
        wgr.put_up_to_8(4, kind)?;
        Ok(())
    }
}

impl<'i> SerializeVlu4 for XpiReplyKind<'i> {
    type Error = XpiVlu4Error;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        match *self {
            XpiReplyKind::CallComplete(call_results) => {
                // match call_result {
                //     Ok(return_value) => {
                //         wgr.put_nibble(0)?;
                //         wgr.put(return_value)
                //     },
                //     Err(e) => wgr.put(e)
                // }
                wgr.put(call_results)
            }
            XpiReplyKind::ReadComplete(_) => {
                todo!()
            }
            XpiReplyKind::WriteComplete(_) => {

                todo!()
            }
            XpiReplyKind::OpenStream(_) => {

                todo!()
            }
            XpiReplyKind::StreamUpdate(_) => {

                todo!()
            }
            XpiReplyKind::CloseStream(_) => {

                todo!()
            }
            XpiReplyKind::Subscribe(_) => {

                todo!()
            }
            XpiReplyKind::RateChange(_) => {

                todo!()
            }
            XpiReplyKind::Unsubscribe(_) => {

                todo!()
            }
            XpiReplyKind::Borrow(_) => {

                todo!()
            }
            XpiReplyKind::Release(_) => {

                todo!()
            }
            XpiReplyKind::Introspect(_) => {

                todo!()
            }
        }?;
        Ok(())
    }
}

impl<'i> DeserializeCoupledBitsVlu4<'i> for XpiReplyKind<'i> {
    type Error = XpiVlu4Error;

    fn des_coupled_bits_vlu4<'di>(bits_rdr: &'di mut BitBuf<'i>, vlu4_rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let kind = bits_rdr.get_up_to_8(4)?;
        use XpiReplyKind::*;
        match kind {
            0 => Ok(CallComplete(vlu4_rdr.des_vlu4()?)),
            _ => Err(XpiVlu4Error::Unimplemented)
        }
    }
}

impl<'i> SerializeVlu4 for XpiReply<'i> {
    type Error = XpiVlu4Error;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        wgr.as_bit_buf::<XpiVlu4Error, _>(|wgr| {
            wgr.put_up_to_8(3, 0b000)?; // unused 31:29
            wgr.put(self.priority)?; // bits 28:26
            wgr.put_bit(true)?; // bit 25, is_unicast
            wgr.put_bit(false)?; // bit 24, is_request
            wgr.put_bit(true)?; // bit 23, reserved
            wgr.put(self.source)?; // bits 22:16
            wgr.put(self.destination)?; // bits 15:7 - discriminant of NodeSet (2b) + 7b for NodeId or other
            wgr.put(self.resource_set)?; // bits 6:4 - discriminant of ResourceSet+Uri
            wgr.put(self.kind)?; // bits 3:0 - discriminant of XpiReplyKind
            Ok(())
        })?;
        wgr.put(self.destination)?;
        wgr.put(self.resource_set)?;
        wgr.put(self.kind)?;
        wgr.put(self.request_id)?;
        Ok(())
    }
}

// pub struct XpiReplyBuilder<'i> {
//     wrr: &'i mut NibbleBufMut<'i>,
// }
//
// impl<'i> XpiReplyBuilder<'i> {
//     pub fn new(
//         wrr: &mut NibbleBufMut<'i>,
//     ) -> Result<Self, XpiVlu4Error> {
//         wrr.skip(8)?;
//         Ok(Self {
//             wrr
//         })
//     }
//
//
// }

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
            priority
        })
    }
}

#[cfg(test)]
mod test {
    extern crate std;
    use crate::discrete::{U2Sp1, U4};
    use crate::serdes::{NibbleBuf, NibbleBufMut};
    use crate::serdes::vlu4::slice::Vlu4Slice;
    use crate::serdes::xpi_vlu4::addressing::{NodeSet, RequestId, XpiResourceSet};
    use crate::serdes::xpi_vlu4::{NodeId, Uri};
    use crate::serdes::xpi_vlu4::priority::Priority;
    use crate::serdes::xpi_vlu4::reply::{XpiReply, XpiReplyKind};

    #[test]
    fn call_reply_ser() {
        let mut buf = [0u8; 32];
        let mut wgr = NibbleBufMut::new_all(&mut buf);

        let mut call_results = [0u8; 128];
        let call_results = {
            let wrr = NibbleBufMut::new_all(&mut call_results);
            let mut wrr = wrr.put_slice_array();
            wrr.put_slice(&[0, 1, 2, 3]);
            wrr.finish_as_slice_array().unwrap()
        };
        let reply_kind = XpiReplyKind::CallComplete(call_results);
        let reply = XpiReply {
            source: NodeId::new(33).unwrap(),
            destination: NodeSet::Unicast(NodeId::new(77).unwrap()),
            resource_set: XpiResourceSet::Uri(Uri::TwoPart44(
                U4::new(4).unwrap(), U4::new(8).unwrap())),
            kind: reply_kind,
            request_id: RequestId::new(5).unwrap(),
            priority: Priority::Lossy(U2Sp1::new(1).unwrap())
        };
        wgr.put(reply).unwrap();
        let (buf, byte_pos, _) = wgr.finish();
        assert_eq!(byte_pos, 10);
        assert_eq!(buf[0..10], [
            0b000_000_10, 0b1_0100001, 0b00100110, 0b1_001_0000,
            0x48, // uri
            0x03, // 0 - Ok, 3 - len(reply_data)
            0, //
            1, 2, 3, // reply_data
            5 // tail
        ]);
    }

    #[test]
    fn call_reply_des() {
        let buf = [0b000_000_10, 0b1_0100001, 0b00100110, 0b1_001_0000, 0x48, 0x03, 1, 2, 3, 5];
        let mut rgr = NibbleBuf::new_all(&buf);

        let reply: XpiReply = rgr.des_vlu4().unwrap();

        assert_eq!(reply.source, NodeId::new(33).unwrap());
        if let NodeSet::Unicast(id) = reply.destination {
            assert_eq!(id, NodeId::new(77).unwrap());
        } else {
            panic!("Expected NodeSet::Unicast(_)");
        }
        if let XpiResourceSet::Uri(uri) = reply.resource_set {
            let mut iter = uri.iter();
            assert_eq!(iter.next(), Some(4));
            assert_eq!(iter.next(), Some(8));
            assert_eq!(iter.next(), None);
        } else {
            panic!("Expected XpiResourceSet::Uri(_)");
        }
        if let XpiReplyKind::CallComplete(result) = reply.kind {
            todo!();
            // assert!(result.is_ok());
            // assert_eq!(result.unwrap().slice, [1, 2, 3]);
        } else {
            panic!("Expected XpiReplyKind::CallComplete(_)");
        }
        assert_eq!(reply.request_id, RequestId::new(5).unwrap());
        assert_eq!(reply.priority, Priority::Lossy(U2Sp1::new(1).unwrap()));
    }
}
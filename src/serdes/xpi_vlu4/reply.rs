use crate::serdes::bit_buf::BitBufMut;
use crate::serdes::NibbleBufMut;
use crate::serdes::traits::{SerializeBits, SerializeVlu4};
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
    CallComplete(Result<Vlu4Slice<'rep>, FailReason>),

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
            XpiReplyKind::CallComplete(call_result) => {
                match call_result {
                    Ok(return_value) => {
                        wgr.put_nibble(0)?;
                        wgr.put(return_value)
                    },
                    Err(e) => wgr.put(e)
                }
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

#[cfg(test)]
mod test {
    extern crate std;
    use std::println;
    use crate::discrete::{U2Sp1, U4};
    use crate::serdes::NibbleBufMut;
    use crate::serdes::vlu4::slice::Vlu4Slice;
    use crate::serdes::xpi_vlu4::addressing::{NodeSet, RequestId, XpiResourceSet};
    use crate::serdes::xpi_vlu4::{NodeId, Uri};
    use crate::serdes::xpi_vlu4::priority::Priority;
    use crate::serdes::xpi_vlu4::reply::{XpiReply, XpiReplyKind};

    #[test]
    fn call_reply() {
        let mut buf = [0u8; 32];
        let mut wgr = NibbleBufMut::new_all(&mut buf);

        let reply_data = [1, 2, 3];
        let reply_kind = XpiReplyKind::CallComplete(Ok(Vlu4Slice { slice: &reply_data }));
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
            1, 2, 3, // reply_data
            5 // tail
        ]);
    }
}
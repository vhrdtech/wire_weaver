use core::fmt::{Display, Formatter};
use crate::serdes::{BitBuf, NibbleBuf};
use crate::serdes::{DeserializeVlu4};
use crate::serdes::vlu4::slice::Vlu4Slice;
use crate::serdes::vlu4::Vlu4SliceArray;
use crate::serdes::xpi_vlu4::addressing::{NodeSet, RequestId, XpiResourceSet};
use crate::serdes::xpi_vlu4::error::XpiVlu4Error;
use crate::serdes::xpi_vlu4::priority::Priority;
use crate::serdes::xpi_vlu4::rate::Vlu4RateArray;
use crate::serdes::xpi_vlu4::Uri;
use super::NodeId;

/// Requests are sent to the Link by the initiator of an exchange, which can be any node on the Link.
/// One or several Responses are sent back for each kind of request.
///
/// In case of subscribing to property updates or streams, responses will continue to arrive
/// until unsubscribed, stream exhausted or closed or one of the nodes rebooting.
///
/// After subscribers node reboot, one or more responses may arrive, until publishing nodes notices
/// subscribers reboot, unless subscribed again.
#[derive(Copy, Clone, Debug)]
pub struct XpiRequest<'req> {
    /// Origin node of the request
    pub source: NodeId,
    /// Destination node or nodes
    pub destination: NodeSet<'req>,
    /// Set of resources that are considered in this request
    pub resource_set: XpiResourceSet<'req>,
    /// What kind of operation is request on a set of resources
    pub kind: XpiRequestKind<'req>,
    /// Modulo number to map responses with requests.
    /// When wrapping to 0, if there are any outgoing unanswered requests that are not subscriptions.
    pub request_id: RequestId,
    /// Priority selection: lossy or lossless (to an extent).
    pub priority: Priority,
}

impl<'i> Display for XpiRequest<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "XpiRequest<@{:#} {}> {{ {} -> {}{:#} {:?} }}",
            self.request_id,
            self.priority,
            self.source,
            self.destination,
            self.resource_set,
            self.kind,
        )
    }
}

/// Select what to do with one ore more selected resources.
#[derive(Copy, Clone, Debug)]
pub enum XpiRequestKind<'req> {
    /// Request binary descriptor block from a node.
    /// Descriptor block is a compiled binary version of a vhL source.
    /// It carries all the important information that is needed to interact with the node.
    /// Including:
    /// * All the data types, also those coming from dependencies
    /// * Unique IDs of all the dependencies and of itself (everything must be published to the
    ///     repository before binary block can be compiled or dirty flag can be set for dev)
    /// * All the xPI blocks with strings (names, descriptions), examples and valid values.
    ///
    /// [Format description (notion)](https://www.notion.so/vhrdtech/Descriptor-block-d0fb717035574255a9baebdb18b8a4f2)
    //GetDescriptorBlock, -> move to a stream_out<chunk> resource, can also add separate const property with a link to the vhL source

    /// Call one or more methods.
    /// Results in [XpiReply::FnCallFailed] or [XpiReply::FnReturn] for each method.
    Call {
        /// Arguments must be serialized with the chosen [Wire Format](https://github.com/vhrdtech/vhl/blob/master/book/src/wire_formats/wire_formats.md)
        /// Need to get buffer for serializing from user code, which decides how to handle memory
        args: Vlu4SliceArray<'req>,
    },

    /// Perform f(g(h(... (args) ...))) call on the destination node, saving
    /// round trip request and replies.
    /// Arguments must be compatible across all the members of a chain.
    /// One response is sent back for the outer most function.
    /// May not be supported by all nodes.
    /// Do not cover all the weird use cases, so maybe better be replaced with full-blown expression
    /// executor only were applicable and really needed?
    ChainCall {
        args: Vlu4Slice<'req>,
    },

    /// Read one or more resources.
    /// Reading several resources at once is more efficient as only one req-rep is needed in best case.
    /// Resources that support reads are: const, ro, ro + stream, rw, rw + stream
    Read,

    /// Write one or more resources.
    /// Resources that support writes are: wo, wo + stream, rw, rw + stream, stream_in<T> when open only.
    Write {
        /// Must be exactly the size of non-zero resources selected for writing in order of
        /// increasing serial numbers, depth first.
        values: Vlu4SliceArray<'req>,
    },

    /// Open one or more streams for read, writes, publishing or subscribing.
    /// stream_in<T> can be written into or published to.
    /// It is only a hint to codegen to create more useful abstractions, there is no functional
    /// difference between publishing or writing.
    ///
    /// stream_out<T> can be read or subscribed to.
    /// In contrast with writing vs publishing, reading is different from subscribing, as only
    /// one result is returned on read, but one or many after subscribing.
    ///
    /// Only opened streams can be written into, read from or subscribed to.
    /// Stream thus have a start and an end in contrast to properties with a +stream modifier.
    /// Stream are also inherently Borrowable (so writing stream_in<T> is equivalent to Cell<stream_in<T>>).
    /// When opening a closed stream, it is automatically borrowed. Opening an open stream returns an error.
    OpenStreams,

    /// Closes one or more streams.
    /// Can be used as an end mark for writing a file for example.
    CloseStreams,

    /// Subscribe to property changes or streams.
    /// Resources must be be rw + stream, ro + stream or stream_out<T>.
    ///
    /// To change rates, subscribe again to the same or different set of resources.
    ///
    /// Publishers must avoid emitting changes with higher than requested rates.
    Subscribe {
        /// For each uri there must be a specified [Rate] provided.
        rates: Vlu4RateArray<'req>,
    },

    // /// Request a change in properties observing or stream publishing rates.
    // ChangeRates {
    //     /// For each uri there must be a specified [Rate] provided.
    //     rates: &'req [Rate],
    // },

    /// Unsubscribe from one or many resources, unsubscribing from a stream do not close it,
    /// but releases a borrow, so that someone else can subscribe and continue receiving data.
    Unsubscribe,

    /// Borrow one or many resources for exclusive use. Only work ons streams and Cell<T> resources.
    /// Other nodes will receive an error if they attempt to access borrowed resources.
    ///
    /// Nodes may implement more logic to allow or block borrowing of a resource.
    /// For example expecting a correct configuration or a key first.
    /// /main {
    ///     /key<wo String> {}
    ///     /dangerous_things<Cell<_>> {
    ///         /wipe_data<fn()> {}
    ///     }
    /// }
    /// In this example one would first have to write a correct key and then try to borrow
    /// /dangerous_things. If the key is incorrect, borrow can be rejected. Stronger security
    /// algorithms can probably be also implemented to granularly restrict access.
    /// Link between the nodes can also be encrypted, with a common key or a set of keys between all nodes.
    /// Encryption is out of scope of this document though.
    ///
    /// Might be a good idea to introduce some limitation on how many borrows can be made from one node.
    /// Depends on the kind of resource. Do not protect against malicious attempts, as node ids can be
    /// faked, but can prevent bugs.
    Borrow,

    /// Release resources for others to use.
    Release,

    /// Get information about resources.
    /// Type information for all resources.
    /// In addition:
    /// * Cell<T>: whether resource is borrowed or not.
    /// * stream_in<T> or stream_out<T>: whether stream is opened or
    /// not (when implicit Cell is already borrowed) + subscribers info + rates.
    /// * +stream: subscribers info + rates
    /// * fn: nothing at the moment
    /// * const: nothing at the moment
    /// * array of resources: size of the array
    Introspect,
}

impl<'i> DeserializeVlu4<'i> for XpiRequest<'i> {
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
            return Err(XpiVlu4Error::NotARequest);
        }

        // bit 24
        let is_request = bits_rdr.get_bit()?;
        if !is_request {
            return Err(XpiVlu4Error::NotARequest);
        }

        // UAVCAN reserved bit 23, discard if 0
        let reserved_23 = bits_rdr.get_bit()?;
        if !reserved_23 {
            return Err(XpiVlu4Error::ReservedDiscard);
        }

        // bits 22:21
        let destination_kind = bits_rdr.get_up_to_8(2)?;

        // bits 20:17
        let request_kind = bits_rdr.get_up_to_8(4)?;

        // bits 16:14
        let uri_type = bits_rdr.get_up_to_8(3)?;

        // bits 13:7 and reads from rdr if traits are used
        let destination = des_destination(destination_kind, &mut bits_rdr, rdr)?;

        // bits 6:0
        let source: NodeId = bits_rdr.des_bits()?;

        let resource_set = des_resource_set(uri_type, rdr)?;
        let kind = des_request_kind(request_kind, rdr)?;

        // tail byte should be at byte boundary, if not 4b padding is added
        if !rdr.is_at_byte_boundary() {
            let _ = rdr.get_nibble()?;
        }
        let request_id: RequestId = rdr.des_vlu4()?;

        Ok(XpiRequest {
            source,
            destination,
            resource_set,
            kind,
            request_id,
            priority
        })
    }
}

fn des_destination<'di, 'i>(
    destination_kind: u8,
    bits_rdr: &'di mut BitBuf<'i>,
    _rdr: &'di mut NibbleBuf<'i>
) -> Result<NodeSet<'i>, XpiVlu4Error> {
    match destination_kind {
        0 => Ok(NodeSet::Unicast(bits_rdr.des_bits()?)),
        1 => Err(XpiVlu4Error::Unimplemented),
        2 => Err(XpiVlu4Error::Unimplemented),
        3 => Err(XpiVlu4Error::ReservedDiscard),
        _ => Err(XpiVlu4Error::InternalError)
    }
}

fn des_resource_set<'di, 'i>(
    uri_type: u8,
    rdr: &'di mut NibbleBuf<'i>
) -> Result<XpiResourceSet<'i>, XpiVlu4Error> {
    match uri_type {
        0 => Ok(XpiResourceSet::Uri( Uri::OnePart(rdr.get_nibble()?)) ),
        1 => Ok(XpiResourceSet::Uri( Uri::TwoPart(
            rdr.get_nibble()?, rdr.get_nibble()?))
        ),
        2 => Ok(XpiResourceSet::Uri( Uri::ThreePart(
            rdr.get_nibble()?,
            rdr.get_nibble()?,
            rdr.get_nibble()?
        ))),
        3 => {
            let mut bits = rdr.get_bit_buf(3)?;
            Ok(XpiResourceSet::Uri(Uri::ThreePart(
                bits.get_up_to_8(6)?,
                bits.get_up_to_8(3)?,
                bits.get_up_to_8(3)?,
            )))
        }
        4 => {
            let mut bits = rdr.get_bit_buf(4)?;
            Ok(XpiResourceSet::Uri(Uri::ThreePart(
                bits.get_up_to_8(6)?,
                bits.get_up_to_8(6)?,
                bits.get_up_to_8(4)?,
            )))
        }
        5 => Ok( XpiResourceSet::Uri(rdr.des_vlu4()?) ),
        6 => Ok( XpiResourceSet::MultiUri(rdr.des_vlu4()?) ),
        7 => {
            Err(XpiVlu4Error::ReservedDiscard)
        }
        _ => {
            Err(XpiVlu4Error::InternalError)
        }
    }
}

fn des_request_kind<'di, 'i>(
    request_kind: u8,
    rdr: &'di mut NibbleBuf<'i>
) -> Result<XpiRequestKind<'i>, XpiVlu4Error> {
    use XpiRequestKind::*;
    match request_kind {
        0 => Ok(Call {
                args: rdr.des_vlu4()?
        }),
        1 => Ok(Read),
        2 => Ok(Write {
            values: rdr.des_vlu4()?
        }),
        3 => Ok(OpenStreams),
        4 => Ok(CloseStreams),
        5 => Ok(Subscribe {
            rates: rdr.des_vlu4()?
        }),
        6 => Ok(Unsubscribe),
        7 => Ok(Borrow),
        8 => Ok(Release),
        9 => Ok(Introspect),
        10 => Ok(ChainCall {
            args: rdr.des_vlu4()?
        }),
        11..=15 => Err(XpiVlu4Error::ReservedDiscard),
        _ => Err(XpiVlu4Error::InternalError)
    }
}

#[cfg(test)]
mod test {
    extern crate std;
    use std::println;

    use crate::discrete::U2Sp1;
    use crate::serdes::NibbleBuf;
    use crate::serdes::xpi_vlu4::addressing::{NodeSet, RequestId, XpiResourceSet};
    use crate::serdes::xpi_vlu4::{NodeId, Uri};
    use crate::serdes::xpi_vlu4::priority::Priority;
    use crate::serdes::xpi_vlu4::request::{XpiRequest, XpiRequestKind};

    #[test]
    fn call_request() {
        let buf = [
            0b000_100_11, 0b1_00_00000, 0b001_101010, 0b1_0101010,
            0b0011_1100, 0b0001_0010, 0xaa, 0xbb, 0b000_11011
        ];
        let mut rdr = NibbleBuf::new_all(&buf);
        let req: XpiRequest = rdr.des_vlu4().unwrap();
        println!("{}", req);
        assert_eq!(req.priority, Priority::Lossless(U2Sp1::new(1).unwrap()));
        assert_eq!(req.request_id, RequestId::new(27).unwrap());
        assert_eq!(req.source, NodeId::new(42).unwrap());
        assert!(matches!(req.destination, NodeSet::Unicast(_)));
        if let NodeSet::Unicast(id) = req.destination {
            assert_eq!(id, NodeId::new(85).unwrap());
        }
        assert!(matches!(req.resource_set, XpiResourceSet::Uri(_)));
        if let XpiResourceSet::Uri(uri) = req.resource_set {
            assert!(matches!(uri, Uri::TwoPart(3, 12)));
        }
        assert!(matches!(req.kind, XpiRequestKind::Call { .. }));
        if let XpiRequestKind::Call { args } = req.kind {
            assert_eq!(args.len(), 1);
            let slice = args.iter().next().unwrap();
            assert_eq!(slice.len(), 2);
            assert_eq!(slice[0], 0xaa);
            assert_eq!(slice[1], 0xbb);
        }
        assert!(rdr.is_at_end());
    }
}
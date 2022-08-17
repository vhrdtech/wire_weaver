// use hash32_derive::Hash32;
use core::fmt::{Display, Formatter, Result as FmtResult};
use crate::serdes::{BitBuf, DeserializeVlu4, NibbleBuf, NibbleBufMut};
use crate::serdes::bit_buf::BitBufMut;
use crate::serdes::DeserializeBits;
use crate::serdes::traits::{SerializeBits, SerializeVlu4};
use crate::serdes::vlu4::TraitSet;
use crate::serdes::xpi_vlu4::{Uri, MultiUri};
use crate::discrete::max_bound_number;


max_bound_number!(NodeId, 7, u8, 127, "N:{}", put_up_to_8, get_up_to_8);

// Each outgoing request must be marked with an increasing number in order to distinguish
// requests of the same kind and map responses.
// Might be narrowed down to less bits. Detect an overflow when old request(s) was still unanswered.
// Should pause in that case or cancel all old requests. Overflow is ignored for subscriptions.
max_bound_number!(RequestId, u8, 31, "Req:{}");
impl<'i> DeserializeVlu4<'i> for RequestId {
    type Error = crate::serdes::nibble_buf::Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let tail_byte = rdr.get_u8()?;
        let request_id = tail_byte & 0b0001_1111;
        Ok(RequestId(request_id & 0b0001_1111))
    }
}

#[derive(Copy, Clone, Debug)]
pub enum NodeSet<'i> {
    /// Request is targeted at only one specific node.
    /// Any resources can be used from the node's vhL description.
    Unicast(NodeId),

    /// Request is targeted at only one node, but through traits interface.
    /// More expensive in terms of size and processing, but gives other benefits.
    UnicastTraits {
        destination: NodeId,
        traits: TraitSet<'i>,
    },

    /// Request is targeted at many nodes at once. Only nodes implementing a set of common traits can
    /// be addressed that way.
    ///
    /// Trait in this context is an xPI block defined and published to the Registry with a particular version.
    /// Might be thought of as an abstract class as well.
    ///
    /// Examples of xpi traits:
    /// * log - to e.g. subscribe to all node's logs at once
    /// * bootloader - to e.g. request all firmware versions
    /// * power_mgmt - to e.g. put all nodes to sleep
    /// Other more specific traits that only some nodes would implement:
    /// * led_feedback - to e.g. enable or disable led on devices
    /// * canbus_counters - to monitor CANBus status across the whole network
    Multicast {
        /// List of traits a node have to implement.
        /// Uri structure is arranged differently for this kind of requests.
        /// For example if 3 traits were provided, then there are /0, /1, /2 resources,
        /// each corresponding to the trait specified, in order.
        /// So e.g. it is possible to call 3 different functions from 3 different traits in one request.
        traits: TraitSet<'i>,
    },
    // Broadcast,
}

impl<'i> DeserializeBits<'i> for NodeSet<'i> {
    type Error = crate::serdes::bit_buf::Error;

    fn des_bits<'di>(_rdr: &'di mut BitBuf<'i>) -> Result<Self, Self::Error> {
        todo!() // deserialize UnicastTraits or Multicast
    }
}

impl<'i> SerializeBits for NodeSet<'i> {
    type Error = crate::serdes::bit_buf::Error;

    fn ser_bits(&self, wgr: &mut BitBufMut) -> Result<(), Self::Error> {
        match self {
            NodeSet::Unicast(id) => {
                wgr.put_up_to_8(2, 0b00)?;
                wgr.put(*id)?;
            }
            NodeSet::UnicastTraits { .. } => {
                wgr.put_up_to_8(2, 0b01)?;
                todo!()
            }
            NodeSet::Multicast { .. } => {
                wgr.put_up_to_8(2, 0b10)?;
                todo!()
            }
        };
        Ok(())
    }
}

impl<'i> SerializeVlu4 for NodeSet<'i> {
    type Error = crate::serdes::nibble_buf::Error;

    fn ser_vlu4(&self, _wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        match self {
            NodeSet::Unicast(_) => {
                // Unicast was already serialized into header, no need to add anything
                return Ok(());
            }
            NodeSet::UnicastTraits { .. } => {

                todo!()
            }
            NodeSet::Multicast { .. } => {

                todo!()
            }
        }
    }
}

impl<'i> Display for NodeSet<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            NodeSet::Unicast(node_id) => write!(f, "{}", node_id),
            NodeSet::UnicastTraits { destination, traits } => write!(f, "{}{}", destination, traits),
            NodeSet::Multicast { .. } => write!(f, "M_impl")
        }
    }
}

/// It is possible to perform operations on a set of resources at once for reducing requests and
/// responses amount.
///
/// If operation is only targeted at one resource, there are more efficient ways to select it than
/// using [MultiUri].
/// It is possible to select one resource in several different ways for efficiency reasons.
/// If there are several choices on how to construct the same uri, select the smallest one in size.
/// If both choices are the same size, choose [Uri].
///
/// [MultiUri] is the only way to select several resources at once within one request.
#[derive(Copy, Clone, Debug)]
pub enum XpiResourceSet<'i> {
    /// Select any one resource at any depth.
    /// Or root resource by providing 0 length Uri (probably never needed).
    Uri(Uri<'i>),

    /// Selects any set of resources at any depths at once.
    MultiUri(MultiUri<'i>),
}

impl<'i> SerializeBits for XpiResourceSet<'i> {
    type Error = crate::serdes::bit_buf::Error;

    fn ser_bits(&self, wgr: &mut BitBufMut) -> Result<(), Self::Error> {
        let kind = match self {
            XpiResourceSet::Uri(uri) => {
                match uri {
                    Uri::OnePart4(_) => 0,
                    Uri::TwoPart44(_, _) => 1,
                    Uri::ThreePart444(_, _, _) => 2,
                    Uri::ThreePart633(_, _, _) => 3,
                    Uri::ThreePart664(_, _, _) => 4,
                    Uri::MultiPart(_) => 5,
                }
            }
            XpiResourceSet::MultiUri(_) => 6
        };
        wgr.put_up_to_8(4, kind)
    }
}

impl<'i> Display for XpiResourceSet<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            XpiResourceSet::Uri(uri) => write!(f, "{}", uri),
            XpiResourceSet::MultiUri(multi_uri) => write!(f, "{}", multi_uri),
        }
    }
}
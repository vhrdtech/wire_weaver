// use hash32_derive::Hash32;
use core::fmt::{Display, Formatter, Result as FmtResult};
use crate::serdes::{BitBuf, DeserializeVlu4, NibbleBuf, NibbleBufMut};
use crate::serdes::bit_buf::BitBufMut;
use crate::serdes::DeserializeBits;
use crate::serdes::traits::{DeserializeCoupledBitsVlu4, SerializeBits, SerializeVlu4};
use crate::serdes::vlu4::TraitSet;
use crate::serdes::xpi_vlu4::{Uri, MultiUri};
use crate::discrete::max_bound_number;
use crate::serdes::xpi_vlu4::error::XpiVlu4Error;


max_bound_number!(NodeId, 7, u8, 127, "N:{}", put_up_to_8, get_up_to_8);

// Each outgoing request must be marked with an increasing number in order to distinguish
// requests of the same kind and map responses.
// Might be narrowed down to less bits. Detect an overflow when old request(s) was still unanswered.
// Should pause in that case or cancel all old requests. Overflow is ignored for subscriptions.
max_bound_number!(RequestId, u8, 31, "Req:{}");
impl<'i> DeserializeVlu4<'i> for RequestId {
    type Error = XpiVlu4Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let tail_byte = rdr.get_u8()?;
        let request_id = tail_byte & 0b0001_1111;
        Ok(RequestId(request_id & 0b0001_1111))
    }
}
impl SerializeVlu4 for RequestId {
    type Error = XpiVlu4Error;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        if !wgr.is_at_byte_boundary() {
            // since request id is a part of a tail byte, put padding before it to align
            wgr.put_nibble(0)?;
        }
        wgr.put_u8(self.inner())?;
        Ok(())
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

impl<'i> DeserializeCoupledBitsVlu4<'i> for NodeSet<'i> {
    type Error = XpiVlu4Error;

    fn des_coupled_bits_vlu4<'di>(bits_rdr: &'di mut BitBuf<'i>, _vlu4_rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let kind = bits_rdr.get_up_to_8(2)?;
        match kind {
            0 => Ok(NodeSet::Unicast(bits_rdr.des_bits()?)),
            1 => Err(XpiVlu4Error::Unimplemented),
            2 => Err(XpiVlu4Error::Unimplemented),
            3 => Err(XpiVlu4Error::ReservedDiscard),
            _ => Err(XpiVlu4Error::InternalError)
        }
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
        wgr.put_up_to_8(3, kind)
    }
}
impl<'i> SerializeVlu4 for XpiResourceSet<'i> {
    type Error = XpiVlu4Error;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        match self {
            XpiResourceSet::Uri(uri) => wgr.put(*uri),
            XpiResourceSet::MultiUri(multi_uri) => wgr.put(*multi_uri)
        }
    }
}

impl<'i> DeserializeCoupledBitsVlu4<'i> for XpiResourceSet<'i> {
    type Error = XpiVlu4Error;

    fn des_coupled_bits_vlu4<'di>(
        bits_rdr: &'di mut BitBuf<'i>,
        vlu4_rdr: &'di mut NibbleBuf<'i>,
    ) -> Result<Self, Self::Error> {
        let uri_type = bits_rdr.get_up_to_8(3)?;
        match uri_type {
            0 => Ok(XpiResourceSet::Uri( Uri::OnePart4(vlu4_rdr.des_vlu4()?)) ),
            1 => Ok(XpiResourceSet::Uri( Uri::TwoPart44(
                vlu4_rdr.des_vlu4()?, vlu4_rdr.des_vlu4()?))
            ),
            2 => Ok(XpiResourceSet::Uri( Uri::ThreePart444(
                vlu4_rdr.des_vlu4()?,
                vlu4_rdr.des_vlu4()?,
                vlu4_rdr.des_vlu4()?
            ))),
            3 => {
                let mut bits = vlu4_rdr.get_bit_buf(3)?;
                Ok(XpiResourceSet::Uri(Uri::ThreePart633(
                    bits.des_bits()?,
                    bits.des_bits()?,
                    bits.des_bits()?,
                )))
            }
            4 => {
                let mut bits = vlu4_rdr.get_bit_buf(4)?;
                Ok(XpiResourceSet::Uri(Uri::ThreePart664(
                    bits.des_bits()?,
                    bits.des_bits()?,
                    bits.des_bits()?,
                )))
            }
            5 => Ok( XpiResourceSet::Uri(vlu4_rdr.des_vlu4()?) ),
            6 => Ok( XpiResourceSet::MultiUri(vlu4_rdr.des_vlu4()?) ),
            7 => {
                Err(XpiVlu4Error::ReservedDiscard)
            }
            _ => {
                Err(XpiVlu4Error::InternalError)
            }
        }
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
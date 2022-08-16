//! no_std, no alloc, zero copy, space efficient implementation of xPI based
//! on variable length encoding and buffer with 4 bits elements.
//!
//! Nibble (4 bits) level access to buffers are used to save substantial amount of space for
//! lower bandwidth channels (for example CAN Bus).
//!
//! First 4 bytes of serialized data structures are directly mappable to 29bit CAN ID and uses
//! layout very similar to UAVCAN (now Cyphal). This optimization saves additional space.
//! It is also possible to use different underlying interface, just treating serialized data as one
//! buffer.
//!
//! With the tricks employed, it is for example
//! possible to encode up to 4 function call requests into 6 bytes, leaving one byte free and one
//! byte for transport layer.
//!
//!
//! One request or reply takes 3+ nibbles depending on the Uri length and resource tree
//! organization.
//!
//!
//! Resource index / serial
//! LSB bit of each nibble == 1 means there is another nibble carrying 3 more bits.
//! Little endian.
//! Minimum size is 4b => 0..=7
//! 8b => 0..=63
//! 12b => 0..=511
//! 16b => 0..=4095
//! pub type UriPart = VarInt<vlu4>;
//! Variable length encoding is used consisting of nibbles. Uri = PartCount followed by Parts.
//! Smallest size = 4 bits => empty Uri.
//! 8 bits => up to 8 resources from root == / one of 8
//! 12 bits => Uri(/ one of 8 / one of 8) or Uri(/one of 64)
//! 16 bits => Uri(/ one of 8 / one of 64) or Uri(/one of 64 / one of 8) or Uri(/ one of 8 / one of 8 / one of 8)
//! And so one with 4 bits steps.
//! 32 bits => 28 bits used for Uri = 7 nibbles each carrying 3 bits => up to 2_097_152 resources addressable.
//! Most of the realistic use cases will fall into 12 or 16 bits, resulting in a very compact uri
//! pub type Uri<'i> = &'i [UriPart];
//!
//! It is possible to perform operations on a set of resources at once for reducing requests and
//! responses amount.
//!
//! If operation is only targeted at one resource, there are more efficient ways to select it than
//! using [MultiUri].
//! It is possible to select one resource in several different ways for efficiency reasons.
//! If there are several choices on how to construct the same uri, select the smallest one in size.
//! If both choices are the same size, choose [Uri].
//!
//! [MultiUri] is the only way to select several resources at once within one request.
//! pub enum XpiResourceSet<'i> {
//! One of the alternative addressing modes.
//! Selects / one of 16.
//! Size required is 4 bits. Same Uri would be 12 bits.
//! Alpha(U4),
//!
//! One of the alternative addressing modes.
//! Selects / one of 16 / one of 16.
//! Size required is 8 bits. Same Uri would be 20 bits.
//! Beta(U4, U4),
//!
//! One of the alternative addressing modes.
//! Selects / one of 16 / one of 16 / one of 16.
//! Size required is 12 bits. Same Uri would be 28 bits.
//! Gamma(U4, U4, U4),
//!
//! One of the alternative addressing modes.
//! Selects / one of 64 / one of 8 / one of 8.
//! Size required is 12 bits. Same Uri would be 20 bits.
//! Delta(U6, U3, U3),
//!
//! One of the alternative addressing modes.
//! Selects / one of 64 / one of 64 / one of 16.
//! Size required is 16 bits. Same Uri would be 28 bits.
//! Epsilon(U6, U6, U4),
//!
//! Select any one resource at any depth.
//! May use more space than alpha-epsilon modes.
//! Size required is variable, most use cases should be in the range of 16-20 bits.
//! Minimum size is 4 bits for 0 sized Uri (root / resource) - also the way to select
//! root resource (probably never needed).
//! Uri(Uri<'i>),
//!
//! Selects any set of resources at any depths at once.
//! Use more space than Uri and alpha-epsilon modes but selects a whole set at once.
//! Minimum size is 12 bits for one 0 sized Uri and [UriMask::All] - selecting all resources
//! at root level ( / * ).
//! MultiUri(MultiUri<'i>),
//! }
//!
// impl<'req> SerializeVlu4 for XpiRequest<'req> {
//     fn ser_vlu4(&self, wgr: &mut NibbleBufMut) {
//         todo!()
//     }
// }
//
// impl<'de> DeserializeVlu4 for XpiRequest<'de> {
//     fn des_vlu4(rdr: &mut NibbleBuf) -> Self {
//
//         XpiRequest {
//             source: 0,
//             destination: (),
//             resource_set: (),
//             kind: XpiRequestKind::Read,
//             request_id: 0,
//             priority: ()
//         }
//     }
// }
pub mod uri;
pub mod uri_mask;
pub mod multi_uri;
pub mod request;
pub mod addressing;
// pub mod reply;
// pub mod resource_info;
// pub mod node_info;
// pub mod broadcast;
pub mod rate;
pub mod priority;
pub mod error;

pub use uri::{Uri, UriIter};
pub use multi_uri::{MultiUri, MultiUriIter};
pub use uri_mask::{UriMask, UriMaskIter};
pub use addressing::NodeId;
// pub use multi_uri::
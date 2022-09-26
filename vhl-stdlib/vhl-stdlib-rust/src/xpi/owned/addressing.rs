use crate::xpi::addressing::{XpiGenericNodeSet, XpiGenericResourceSet};
use super::{SerialUri, SerialMultiUri};

#[derive(Copy, Clone)]
pub struct RequestId(pub u32);

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct NodeId(pub u32);

pub type XpiResourceSet = XpiGenericResourceSet<SerialUri, SerialMultiUri>;

#[derive(Clone, Debug)]
pub struct TraitSet {}

pub type NodeSet = XpiGenericNodeSet<NodeId, TraitSet>;
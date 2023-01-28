use crate::node_set::XpiGenericNodeSet;
use crate::resource_set::XpiGenericResourceSet;

/// Root data type exchanged by nodes
#[derive(Clone, Debug)]
pub struct XpiGenericEvent<
    NID, // NodeId
    TS,  // TraitSet
    U,   // SerialUri
    MU,  // SerialMultiUri
    K,
    P,   // Priority
    RID, // Request - Response matching ID
    TTL,
> {
    /// Origin node of the request
    pub source: NID,
    /// Destination node or nodes
    pub destination: XpiGenericNodeSet<NID, TS>,
    /// Set of resources that are considered in this request.
    /// If event is a request, then this resource set is in context of a destination node.
    /// If event is a response, multicast or broadcast, then this resource set is in context of a source node.
    pub resource_set: XpiGenericResourceSet<U, MU>,
    pub kind: K,
    /// Priority selection: lossy or lossless (to an extent).
    pub priority: P,
    /// Modulo number to map responses with requests.
    /// When wrapping to 0, if there are any outgoing unanswered requests that are not subscriptions.
    pub request_id: RID,
    pub ttl: TTL,
}

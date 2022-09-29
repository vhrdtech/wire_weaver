use crate::node_set::XpiGenericNodeSet;

/// Root data type exchanged by nodes
#[derive(Clone, Debug)]
pub struct XpiGenericEvent<
    NID,
    TS,
    RQ,
    RP,
    BR,
    FW,
    P,
> {
    /// Origin node of the request
    pub source: NID,
    /// Destination node or nodes
    pub destination: XpiGenericNodeSet<NID, TS>,
    pub kind: XpiGenericEventKind<RQ, RP, BR, FW>,
    /// Priority selection: lossy or lossless (to an extent).
    pub priority: P,
}

#[derive(Clone, Debug)]
pub enum XpiGenericEventKind<
    RQ, // XpiRequest
    RP, // XpiReply
    BR, // XpiBroadcastKind
    FW, //
> {
    Request(RQ),
    Reply(RP),
    Broadcast(BR),
    Forward(FW),
}
/// Root data type exchanged by nodes
pub struct XpiGenericEvent<
    SRC,
    DST,
    RQ,
    RP,
    BR,
    FW,
    P,
> {
    /// Origin node of the request
    pub source: SRC,
    /// Destination node or nodes
    pub destination: DST,
    pub kind: XpiGenericEventKind<RQ, RP, BR, FW>,
    /// Priority selection: lossy or lossless (to an extent).
    pub priority: P,
}

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
use xpi::event_kind::XpiEventDiscriminant;
use xpi::node_set::XpiGenericNodeSet;
use xpi::owned::{Event, NodeId, NodeSet, RequestId};

#[derive(Debug)]
pub enum SourceFilter {
    Any,
    NodeId(NodeId),
}

#[derive(Debug)]
pub enum NodeSetFilter {
    Any,
    NodeId(NodeId),
    UnicastTraits,
    Multicast,
    Broadcast,
}

#[derive(Debug)]
pub enum EventKindFilter {
    Any,
    One(XpiEventDiscriminant),
}

#[derive(Debug)]
pub struct EventFilter {
    pub src: SourceFilter,
    pub dst: NodeSetFilter,
    pub kind: EventKindFilter,
    pub request_id: Option<RequestId>,
}

impl EventFilter {
    pub fn matches(&self, ev: &Event) -> bool {
        match self.kind {
            EventKindFilter::Any => {}
            EventKindFilter::One(discriminant) => {
                if discriminant != ev.kind.discriminant() {
                    return false;
                }
            }
        }
        match self.src {
            SourceFilter::Any => {}
            SourceFilter::NodeId(id) => {
                if ev.source != id {
                    return false;
                }
            }
        }
        match self.dst {
            NodeSetFilter::Any => {}
            NodeSetFilter::NodeId(id) => {
                if let NodeSet::Unicast(ev_id) = ev.destination {
                    if id != ev_id {
                        return false;
                    }
                } else {
                    return false;
                };
            }
            NodeSetFilter::UnicastTraits => unimplemented!(),
            NodeSetFilter::Multicast => unimplemented!(),
            NodeSetFilter::Broadcast => match ev.destination {
                XpiGenericNodeSet::Broadcast { .. } => {}
                _ => {
                    return false;
                }
            },
        }
        match self.request_id {
            None => {}
            Some(req_id) => {
                if req_id != ev.request_id {
                    return false;
                }
            }
        }
        true
    }
}

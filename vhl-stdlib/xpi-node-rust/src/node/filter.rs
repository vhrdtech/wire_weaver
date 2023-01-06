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
    src: SourceFilter,
    dst: NodeSetFilter,
    kind: EventKindFilter,
    request_id: Option<RequestId>,
    single_shot: bool,
}

impl EventFilter {
    pub fn new() -> Self {
        EventFilter {
            src: SourceFilter::Any,
            dst: NodeSetFilter::Any,
            kind: EventKindFilter::Any,
            request_id: None,
            single_shot: true,
        }
    }

    pub fn src(mut self, source_filter: SourceFilter) -> Self {
        self.src = source_filter;
        self
    }

    pub fn dst(mut self, destination_filter: NodeSetFilter) -> Self {
        self.dst = destination_filter;
        self
    }

    pub fn kind(mut self, filter_kind: EventKindFilter) -> Self {
        self.kind = filter_kind;
        self
    }

    pub fn request_id(mut self, request_id: RequestId) -> Self {
        self.request_id = Some(request_id);
        self
    }

    pub(crate) fn single_shot(mut self, single_shot: bool) -> Self {
        self.single_shot = single_shot;
        self
    }

    pub(crate) fn is_single_shot(&self) -> bool {
        self.single_shot
    }

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

impl Default for EventFilter {
    fn default() -> Self {
        Self::new()
    }
}
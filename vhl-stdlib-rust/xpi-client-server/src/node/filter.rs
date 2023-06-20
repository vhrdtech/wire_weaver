use std::time::{Duration, Instant};
use xpi::client_server::{Event, NodeId, Nrl, ReplyKindDiscriminants, RequestId};

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
}

#[derive(Debug)]
pub enum EventKindFilter {
    Any,
    ReplyWithKind(ReplyKindDiscriminants),
    ReplyWithKindEither(ReplyKindDiscriminants, ReplyKindDiscriminants),
}

#[derive(Debug)]
pub enum ResourceSetFilter {
    Any,
    ContainsUri(Nrl),
}

#[derive(Debug)]
pub struct EventFilter {
    src: SourceFilter,
    dst: NodeSetFilter,
    resource_set: ResourceSetFilter,
    kind: EventKindFilter,
    request_id: Option<RequestId>,

    single_shot: bool,

    created_at: Instant,
    timeout: Option<Duration>,
    drop_on_remote_disconnecting: bool,
}

impl EventFilter {
    pub fn new() -> Self {
        EventFilter {
            src: SourceFilter::Any,
            dst: NodeSetFilter::Any,
            resource_set: ResourceSetFilter::Any,
            kind: EventKindFilter::Any,
            request_id: None,
            single_shot: true,
            created_at: Instant::now(),
            timeout: None,
            drop_on_remote_disconnecting: true,
        }
    }

    pub fn new_with_timeout(timeout: Duration) -> Self {
        EventFilter {
            src: SourceFilter::Any,
            dst: NodeSetFilter::Any,
            resource_set: ResourceSetFilter::Any,
            kind: EventKindFilter::Any,
            request_id: None,
            single_shot: true,
            created_at: Instant::now(),
            timeout: Some(timeout),
            drop_on_remote_disconnecting: true,
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

    pub fn resource_set(mut self, resource_set_filter: ResourceSetFilter) -> Self {
        self.resource_set = resource_set_filter;
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

    pub fn drop_on_remote_disconnect(mut self, drop_or_not: bool) -> Self {
        self.drop_on_remote_disconnecting = drop_or_not;
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
        // match self.kind {
        //     EventKindFilter::Any => {}
        //     EventKindFilter::ReplyWithKind(discriminant) => {
        //         if discriminant != ReplyKindDiscriminants::from(ev.kind) {
        //             return false;
        //         }
        //     }
        //     EventKindFilter::ReplyWithKindEither(discriminant1, discriminant2) => {
        //         if ev.kind.discriminant() != discriminant1
        //             && ev.kind.discriminant() != discriminant2
        //         {
        //             return false;
        //         }
        //     }
        // }
        match self.src {
            SourceFilter::Any => {}
            SourceFilter::NodeId(id) => {
                if ev.source.node_id != id {
                    return false;
                }
            }
        }
        match self.dst {
            NodeSetFilter::Any => {}
            NodeSetFilter::NodeId(id) => {
                if ev.destination.node_id != id {
                    return false;
                }
            }
            NodeSetFilter::UnicastTraits => unimplemented!(),
        }
        match &self.resource_set {
            ResourceSetFilter::Any => {}
            _ => unimplemented!(),
        }
        match self.request_id {
            None => {}
            Some(req_id) => {
                if Some(req_id) != ev.seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    pub fn is_timed_out(&self) -> bool {
        match self.timeout {
            Some(timeout) => Instant::now().duration_since(self.created_at) > timeout,
            None => false,
        }
    }

    pub fn is_drop_on_remote_disconnect(&self) -> bool {
        self.drop_on_remote_disconnecting
    }

    pub fn is_waiting_for_node(&self, remote_id: NodeId) -> bool {
        match self.src {
            SourceFilter::Any => false,
            SourceFilter::NodeId(id) => id == remote_id,
        }
    }
}

impl Default for EventFilter {
    fn default() -> Self {
        Self::new_with_timeout(Duration::from_millis(500))
    }
}

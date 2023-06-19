use super::{Address, Error, Reply, Request, RequestId};
use smallvec::SmallVec;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Event {
    pub source: Address,
    pub destination: Address,
    // pub base_nrl: Option<Nrl>,
    pub kind: EventKind,
    pub seq: Option<RequestId>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum EventKind {
    Request {
        // If multiple actions are targeted at one sub level
        // common_nrl: Option<Nrl>,
        actions: SmallVec<[Request; 1]>,
        bail_on_error: bool,
    },
    Reply {
        results: SmallVec<[Reply; 1]>,
    },
}

impl Event {
    pub fn flip_with_error(&self, err: Error) -> Option<Event> {
        match &self.kind {
            EventKind::Request { actions, .. } => {
                let kind = EventKind::Reply {
                    results: actions
                        .iter()
                        .map(|a| a.flip_with_error(err.clone()))
                        .collect(),
                };
                Some(Event {
                    source: self.destination.clone(),
                    destination: self.source.clone(),
                    kind,
                    seq: self.seq,
                })
            }
            EventKind::Reply { .. } => None,
        }
    }
}

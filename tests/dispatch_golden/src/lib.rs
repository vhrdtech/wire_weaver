//! TEST-31: golden reply bytes of the dispatcher. One test API (`dispatch_golden_api`) with a resource of every
//! kind and outcome the dispatcher serves, a sync and an async server built on it ([server], [async_server]), and
//! a corpus of requests whose replies are recorded as golden bytes (`corpus.json`, [corpus]), filled by
//! `just dispatch-golden-update` and frozen like the wire-format conformance corpus ([frozen], TEST-24). The reply
//! bytes are wire-defining; the dispatcher code that builds them (SRV-51, SRV-52, SRV-53) is not: a rework of it
//! must leave the corpus green. See `docs/serdes/conformance.md` for the pattern this follows.

pub mod async_server;
pub mod corpus;
pub mod frozen;
pub mod requests;
pub mod server;

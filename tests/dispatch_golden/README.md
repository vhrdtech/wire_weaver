# dispatch_golden

TEST-31: golden reply bytes of the dispatcher. A sync and an async server built on `dispatch_golden_api` ([server], [async_server]), and a corpus of requests whose replies are recorded as golden bytes, frozen like the wire-format conformance corpus (TEST-24).

Filled by `just dispatch-golden-update`; see `docs/serdes/conformance.md` for the pattern. The reply bytes are wire-defining, the dispatcher code that builds them (SRV-51, SRV-52, SRV-53) is not.

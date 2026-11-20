# shared_sub_level

One sub-level client for a trait two lines share (CLT-68). `shared_sub_level_api` has line 1 (w1.1) and line 2
(w2.1) in one crate; both reach `common::Channel` and `common::Indication`. `ChannelClient` and
`IndicationClient` are generated once, and each line's client returns them, so code over a channel or the
indication is written once for both lines.

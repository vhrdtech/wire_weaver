# serve_trait_gauge_api

> Test API of a level defined in a crate of its own, with its local trait (CLT-58), used by `tests/serve_trait`

The level `GaugeApi` names its local trait on its definition, `#[ww_trait(local = "Gauge")]`, and makes it with
`ww_codegen!(crate::GaugeApi, local_trait = "Gauge")`; `serve_trait_levels_api` reaches it through
`ww_impl!(gauge: serve_trait_gauge_api::GaugeApi)`.

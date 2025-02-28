use wire_weaver::wire_weaver_api;

#[wire_weaver_api(
    api_model = "client_server_v0_1",
    client = true,
    no_alloc = true,
    derive = "Debug",
    debug_to_file = "./target/ww_no_alloc.rs"
)]
pub(crate) mod no_alloc_client {}

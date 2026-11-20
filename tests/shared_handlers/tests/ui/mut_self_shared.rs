// A shared item's getter takes `&mut self`: the `&self` dispatch can't call it.
use wire_weaver::prelude::*;
use ww_client_server::ResourceIndex;

pub struct Server;

impl Server {
    fn valid_indices_root(&self) -> ValidIndices<'_> {
        ValidIndices::range_u32(0..1)
    }

    fn get_level(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<u32, CommonError<'_>> {
        Value(1)
    }

    fn scaled(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>, by: u32) -> RpcResult<u32> {
        Ready(by)
    }
}

mod api_impl {
    wire_weaver::ww_codegen!(
        shared_handlers_api :: Channel for super::Server,
        server = true, no_alloc = true, use_async = false,
        receiver_model = "level=shared",
    );
}

fn main() {
    let _ = ResourceIndex::new(UNib32(0));
}

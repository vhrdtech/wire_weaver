use wire_weaver_client::DynClient;

pub(crate) async fn introspect(device: &DynClient) -> Result<(), anyhow::Error> {
    let Some(introspect_bundle) = device.device_introspect() else {
        return Ok(());
    };
    // TODO: print AST more nicely, like api tree
    // TODO: print size in bytes and what crates where omitted and how much that saved
    println!("{:#?}", introspect_bundle);
    Ok(())
}

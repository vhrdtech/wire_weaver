use wire_weaver_client::{Commander, DynClient};

pub(crate) async fn introspect(device: &DynClient) -> Result<(), anyhow::Error> {
    let api_bundle = device.introspect().get().await?;
    // TODO: print AST more nicely, like api tree
    // TODO: print size in bytes and what crates where omitted and how much that saved
    println!("{:#?}", api_bundle);
    Ok(())
}

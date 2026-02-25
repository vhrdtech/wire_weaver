use wire_weaver_usb_host::wire_weaver_client_common::CommandSender;

pub(crate) async fn introspect(device: &mut CommandSender) -> Result<(), anyhow::Error> {
    let api_bundle = device.introspect().download().await?;
    // TODO: print AST more nicely, like api tree
    // TODO: print size in bytes and what crates where omitted and how much that saved
    println!("{:#?}", api_bundle);
    Ok(())
}

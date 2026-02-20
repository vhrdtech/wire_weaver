use driver::{OnError, MyDeviceDriver, DeviceFilter};
use anyhow::Result;
use wire_weaver::shrink_wrap::DeserializeShrinkWrap;
use ww_self::ApiBundle;

#[tokio::main]
async fn main()-> Result<()> {
    tracing_subscriber::fmt::init();

    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let mut driver = MyDeviceDriver::connect(filter, OnError::ExitImmediately).await?;

    let mut ww_self_stream = driver.introspect().ww_self()?;
    ww_self_stream.open()?;
    let ww_self_bytes = ww_self_stream.recv_all_bytes().await?;
    println!("got {} bytes", ww_self_bytes.len());

    let api_bundle = ApiBundle::from_ww_bytes(&ww_self_bytes).unwrap();
    println!("{:#?}", api_bundle);

    driver.disconnect_and_exit().await?;

    Ok(())
}

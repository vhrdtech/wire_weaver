//! A small host binary over the whole facade, for measuring what each wire line costs (EVO-42): it connects to the
//! in-process device named by its first argument and calls every operation once, so the code of every line stays in
//! the binary. `cargo build --release -p thermo --example probe`, then strip it.

use thermo::Thermo;
use thermo::model::{Command, Gain, Mode, Offset, Rate};
use ww_si::Seq;

#[tokio::main]
async fn main() {
    let path = std::env::args().nth(1).unwrap_or_default();
    let Ok(t) = Thermo::config(|c| c.in_process_path(path.clone()))
        .connect()
        .await
    else {
        return;
    };
    let rate = Rate::try_new(10).unwrap();
    let gain = Gain::try_new(200).unwrap();
    let offset = Offset::new(5);
    let r = [
        t.configure(rate, Mode::Continuous).await.is_ok(),
        t.read_once().await.is_ok(),
        t.mode().await.is_ok(),
        t.set_mode(Mode::Idle).await.is_ok(),
        t.gain().await.is_ok(),
        t.set_gain(gain).await.is_ok(),
        t.channel_enabled(1).await.is_ok(),
        t.set_channel_enabled(1, true).await.is_ok(),
        t.channel_last(1).await.is_ok(),
        t.calibrate(offset).await.is_ok(),
        t.uptime().await.is_ok(),
        t.self_test().await.is_ok(),
    ];
    if let Ok(mut gains) = t.observe_gain().await {
        println!("{:?}", gains.recv().await.is_ok());
    }
    if let Ok(mut samples) = t.samples().await {
        println!("{:?}", samples.recv().await.is_ok());
    }
    if let Ok(mut commands) = t.commands().await {
        let _ = commands.send(Command::Mark { seq: Seq::new(1) }).await;
    }
    let indication = t.indication();
    println!("{r:?} {:?}", indication.read_mode().read().await.is_ok());
}

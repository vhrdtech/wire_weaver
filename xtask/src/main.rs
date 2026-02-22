use std::env;

use xshell::{cmd, Shell};

fn main() -> Result<(), anyhow::Error> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let args = args.iter().map(|s| &**s).collect::<Vec<_>>();

    match &args[..] {
        ["check", "all"] => check_all(),
        // ["test", "host"] => test_host(),
        // ["test", "host-target"] => test_host_target(),
        // ["test", "target"] => test_target(),
        _ => {
            println!("USAGE cargo xtask test [all|host|host-target|target]");
            Ok(())
        }
    }
}

fn check_all() -> Result<(), anyhow::Error> {
    check_host()?;
    check_mcu()?;
    check_examples_mcu()?;
    Ok(())
}

fn check_host() -> Result<(), anyhow::Error> {
    let sh = Shell::new()?;
    cmd!(sh, "cargo check").run()?;
    Ok(())
}

fn check_mcu() -> Result<(), anyhow::Error> {
    let sh = Shell::new()?;
    sh.change_dir("mcu");
    cmd!(sh, "cargo check").run()?;
    Ok(())
}

fn check_examples_mcu() -> Result<(), anyhow::Error> {
    let sh = Shell::new()?;
    sh.change_dir("examples_mcu/usb_nucleo_h743zi2");
    cmd!(sh, "cargo +nightly check").run()?; // for some reason this does not respect rust-toolchain.toml, cargo check in normal shell works
    sh.change_dir("../../examples_mcu/mcu_qemu");
    cmd!(sh, "cargo +nightly check").run()?;
    Ok(())
}

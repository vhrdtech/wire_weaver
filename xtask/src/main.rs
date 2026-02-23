use std::env;

use xshell::{cmd, Shell};

fn main() -> Result<(), anyhow::Error> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let args = args.iter().map(|s| &**s).collect::<Vec<_>>();

    match &args[..] {
        ["check", "all"] => check_all(),
        ["check", "host"] => check_host(),
        ["check", "mcu"] => check_mcu(),
        ["check", "examples_mcu"] => check_examples_mcu(),
        _ => {
            println!("USAGE cargo xtask check [all|host|mcu|examples_mcu]");
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
    check_mcu_qemu()?;
    check_nucleo_h732zi2()?;
    check_stm32g0b1cetxn()?;
    check_stm32h725ig()?;
    Ok(())
}

fn check_mcu_qemu() -> Result<(), anyhow::Error> {
    let sh = Shell::new()?;
    sh.change_dir("examples_mcu/mcu_qemu");
    cmd!(sh, "cargo +nightly check").run()?;
    Ok(())
}

fn check_nucleo_h732zi2() -> Result<(), anyhow::Error> {
    let sh = Shell::new()?;
    sh.change_dir("examples_mcu/usb_nucleo_h743zi2");
    // for some reason this does not respect rust-toolchain.toml, cargo check in normal shell works
    cmd!(sh, "cargo +nightly check").run()?;
    Ok(())
}

fn check_stm32g0b1cetxn() -> Result<(), anyhow::Error> {
    let sh = Shell::new()?;
    sh.change_dir("examples_mcu/usb_stm32g0b1cetxn");
    cmd!(sh, "cargo +nightly check").run()?;
    Ok(())
}

fn check_stm32h725ig() -> Result<(), anyhow::Error> {
    let sh = Shell::new()?;
    sh.change_dir("examples_mcu/usb_stm32h725ig");
    cmd!(sh, "cargo +nightly check").run()?;
    Ok(())
}

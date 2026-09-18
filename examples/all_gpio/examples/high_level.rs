use all_gpio::AllGpio;
use ww_gpio_hl::blocking::BankBlocking;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let device = AllGpio::new().connect_blocking()?;

    // get a global trait "attachment" point
    let attachment = device.port(0).attachment();
    // init a high-level, hand-written driver with better ergonomics than generated code
    let mut bank = BankBlocking::new(attachment)?;

    let bank_name = bank.name()?;
    let mut pins = bank.all_pins()?;
    for pin in &mut pins {
        println!("{}{}: {:?}", bank_name, pin.index(), pin.mode()?);
    }

    //

    device.disconnect().blocking()?;
    Ok(())
}

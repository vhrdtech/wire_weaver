use vhl::loader::load;

fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    println!("vhl 0.3.0");

    let r = load();
    println!("load:{:#?}", r);

    Ok(())
}

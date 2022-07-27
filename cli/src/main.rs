use parser::ast::file::File;

fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    let input = std::fs::read_to_string("/Users/roman/git/vhl_hw/led_ctrl/led_ctrl.vhl")?;
    let s = parser::util::pest_file_parse_tree(&input);
    println!("{}", s);

    let file = File::parse(&input)?;
    println!("Warnings: {:?}", file.1);
    println!("File: {:#?}", file.0);

    // codegen::fun2();

    Ok(())
}

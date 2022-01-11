use parser::ast::file::File;
use parser::ast::item::Item;

fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    // let input = r#"
    //     /// Line 1
    //     /// Line 2
    //     enum FrameId {
    //         Standard(u11),
    //         Extended(u29)
    //     }"#;
    // let file = File::parse(input)?;
    // println!("Warnings: {:?}", file.1);
    // println!("{:?}", file.0.items[0]);
    //
    // match &file.0.items[0] {
    //     Item::Const(_) => {}
    //     Item::Enum(ie) => {
    //         println!("{:?}", codegen::fun(ie));
    //     }
    // }

    codegen::fun2();

    Ok(())
}

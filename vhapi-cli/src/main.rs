use std::fs::{File, read_to_string};
use std::io::Read;

use clap::{App, Arg, SubCommand};
use yaml_rust::yaml;

use vhapi::ir::*;
use vhapi::loader::*;
use vhapi::parser;

// fn callback(x: u8) {
//     println!("Callback called");
// }

fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    // let mut loader = Loader {
    //     fs_loader: Box::new(callback),
    // };
    //
    // loader.load("123".to_string());



    let matches =
        App::new("vhapi-cli")
            .version("0.1.0")
            .author("Roman I. <roman@vhrd.tech>")
            .about("CLI for vhapi library")
            .arg(Arg::with_name("INPUT")
                .help("vhapi file to parse")
                .required(true)
                .index(1))
           .get_matches();

    let input = matches.value_of("INPUT").unwrap();

    let s = read_to_string(input)?;
    println!("{}", s.len());

    let doc = yaml::YamlLoader::load_from_str(&input);
    if doc.is_err() {
        println!("Error while parsing yaml: {:?}", doc.err());
        return Ok(());
    }
    let doc = doc.unwrap();
    if doc.len() != 1 {
        println!("Empty yaml");
        return Ok(())
    }
    let doc = doc.first().unwrap();

    let r = parser::parse_resource(doc);

    dbg!(r);


//    println!("{:?}", doc);

    Ok(())
}

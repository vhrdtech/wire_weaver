use std::fs::{File, read_to_string};
use std::io::{Read, Write};

use clap::{App, Arg, SubCommand};
use yaml_rust::yaml;
use yaml_rust::Yaml;

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

    let doc = yaml::YamlLoader::load_from_str(&s)?;
//    if doc.is_err() {
//        println!("Error while parsing yaml: {:?}", doc.err());
//        return Ok(());
//    }
    //let doc = doc.unwrap();
    if doc.len() != 1 {
        println!("Empty yaml");
        return Ok(())
    }
    let doc = doc.first().unwrap();
//    let mut dbg_file = File::create("/Users/roman/dbg.txt")?;
//    write!(dbg_file, "{:#?}", doc);
    if let Yaml::Hash(h) = doc {
        let rname = "/TEC(register)[]";
        let val = h.get(&Yaml::String(rname.to_string()));
        if val.is_some() {
            let r = parser::parse_resource(rname, &val.unwrap());
            dbg!(r);
        } else {
            println!("none");
        }
    }



//    println!("{:?}", doc);

    Ok(())
}

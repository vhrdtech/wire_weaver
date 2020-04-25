use std::fs::File;
use std::io::Read;

use clap::{App, Arg, SubCommand};
use yaml_rust::yaml;

use vhapi::ir::*;

fn main() -> Result<(), std::io::Error> {
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
    let mut input = {
        let mut f = File::open(input)?;
        let mut s = String::new();
        f.read_to_string(&mut s)?;
        s
    };

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
    println!("{:?}", doc);

    Ok(())
}

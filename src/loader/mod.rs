use proc_macro2::TokenStream;
use std::str::FromStr;
use std::fs::File;
use std::io::Read;

use crate::{VhlError, VhlResult};
use crate::error::LoaderError;

// enum Location {
//     Local,
//     Git,
//     Github,
//     // User?
// }

// pub struct Loader<'a> {
//     pub fs_loader: Box<dyn FnMut(u8) + 'a>,
// }
//
// impl<'a> Loader<'a> {
//     pub fn load(&mut self, _uri: String) -> String {
//         (self.fs_loader)(123);
//         return "abc".to_string();
//     }
//
//     pub fn set_fs_loader(&mut self, loader: impl FnMut(u8) + 'a) {
//         self.fs_loader = Box::new(loader);
//     }
// }

pub fn load() -> VhlResult<()> {
    let mut file = File::open("syntax-pieces/resource_name.vhl").map_err(|e| LoaderError::IoError(e))?;
    let mut content = String::new();
    file.read_to_string(&mut content).map_err(|e| LoaderError::IoError(e))?;
    let ts = TokenStream::from_str(&content);
    println!("{:#?}", ts);

    Ok(())
}
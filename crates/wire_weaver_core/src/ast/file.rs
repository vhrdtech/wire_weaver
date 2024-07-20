use crate::ast::item::Item;
use crate::ast::syn_convert::{SynConversionError, SynConversionWarning};
use crate::ast::version::Version;
use std::path::PathBuf;

#[derive(Debug)]
pub struct WWFile {
    pub source: FileSource,
    // shebang
    // attrs
    pub version: Version,
    pub items: Vec<Item>,
}

#[derive(Debug)]
pub enum FileSource {
    File(PathBuf),
    Registry,
    Git,
}

impl WWFile {
    pub fn from_str<S: AsRef<str>>(
        file_source: FileSource,
        source: S,
    ) -> Result<(Self, Vec<SynConversionWarning>), Vec<SynConversionError>> {
        let syn_file = syn::parse_file(source.as_ref()).unwrap();
        Self::from_syn(file_source, syn_file)
    }

    pub fn from_syn(
        source: FileSource,
        file: syn::File,
    ) -> Result<(Self, Vec<SynConversionWarning>), Vec<SynConversionError>> {
        let mut items = vec![];
        let mut errors = vec![];
        let mut warnings = vec![];
        for item in file.items {
            match Item::from_syn(item) {
                Ok((Some(item), w)) => {
                    items.push(item);
                    warnings.extend(w);
                }
                Ok((None, w)) => {
                    warnings.extend(w);
                }
                Err(e) => {
                    errors.extend(e);
                }
            }
        }
        if errors.is_empty() {
            let version = source.file_version();
            Ok((
                WWFile {
                    source,
                    version,
                    items,
                },
                warnings,
            ))
        } else {
            Err(errors)
        }
    }
}

impl FileSource {
    pub fn file_version(&self) -> Version {
        Version { major: 0, minor: 1 }
    }
}

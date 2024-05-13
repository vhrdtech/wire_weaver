use crate::ast::item::Item;
use crate::ast::version::Version;
use std::path::PathBuf;

#[derive(Debug)]
pub struct File {
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

#[derive(Debug)]
pub enum SynConversionWarning {
    UnknownAttribute,
    UnknownFileItem,
}

#[derive(Debug)]
pub enum SynConversionError {
    UnknownType,
}

impl File {
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
                File {
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

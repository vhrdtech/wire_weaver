use std::collections::HashMap;
use std::ops::Range;
use codespan_reporting::files::Files;
use ast::{Definition, File, Path, SpanOrigin, XpiDef};
use crate::Error;
use crate::user_error::UserError;
use crate::warning::Warning;
use codespan_reporting::files::Error as CRError;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};

#[derive(Clone)]
pub struct Project {
    pub root: File,
    local: HashMap<Path, File>,
    id_to_path: HashMap<usize, Path>,
    _deps: HashMap<String, Project>,
    // config: Toml
    pub errors: Vec<UserError>,
    pub warnings: Vec<Warning>,
}

impl Project {
    pub fn new(root: File) -> Self {
        Project {
            root,
            local: Default::default(),
            id_to_path: Default::default(),
            _deps: Default::default(),
            errors: vec![],
            warnings: vec![],
        }
    }

    pub fn find_def(&self, mut path: Path) -> Result<Definition, Error> {
        if path.segments.is_empty() {
            return Err(Error::FindDef("path cannot be empty".to_owned()));
        }
        if path.is_from_crate() {
            let _ = path.pop_front();
            match path.pop_front() {
                Some(id) => {
                    let def = self.root.defs
                        .get(&id)
                        .ok_or(Error::FindDef(format!("crate::{} not found", id)))?;
                    if path.is_empty() {
                        Ok(def.clone())
                    } else {
                        match def {
                            Definition::Xpi(xpi_def) => {
                                Ok(Definition::Xpi(Self::find_in_xpi_def(xpi_def, path)?))
                            }
                            _ => Err(Error::FindDef("only xpi definition can have child items".to_owned()))
                        }
                    }
                }
                None => {
                    return Err(Error::FindDef("crate root is not a definition".to_owned()));
                }
            }
        } else {
            todo!()
        }
    }

    fn find_in_xpi_def(xpi_def: &XpiDef, mut path: Path) -> Result<XpiDef, Error> {
        match path.pop_front() {
            Some(search_key) => {
                for c in &xpi_def.children {
                    if c.uri_segment.expect_resolved().unwrap() == search_key {
                        return if path.is_empty() {
                            Ok(c.clone())
                        } else {
                            Self::find_in_xpi_def(c, path)
                        };
                    }
                }
                Err(Error::FindDef(format!("find_in_xpi_def: not found: {}", search_key)))
            }
            None => Ok(xpi_def.clone())
        }
    }

    pub fn find_xpi_def(&self, path: Path) -> Result<XpiDef, Error> {
        let def = self.find_def(path.clone())?;
        match def {
            Definition::Xpi(xpi_def) => Ok(xpi_def),
            _ => Err(Error::FindXpiDef(format!("{} is not and xpi definition", path)))
        }
    }

    pub fn find_file_by_id(&self, file_id: usize) -> Result<&File, Error> {
        let path = self.id_to_path.get(&file_id).ok_or(Error::FileNotFound(file_id))?;
        Ok(self.local.get(path).expect("find_file_by_id: internal error"))
    }

    pub fn print_report(&self) {
        let writer = StandardStream::stderr(ColorChoice::Always);
        let config = codespan_reporting::term::Config::default();
        for diagnostic in self.warnings.iter().map(|warn| warn.report()) {
            codespan_reporting::term::emit(&mut writer.lock(), &config, self, &diagnostic).unwrap();
        }
        for diagnostic in self.errors.iter().map(|err| err.report()) {
            codespan_reporting::term::emit(&mut writer.lock(), &config, self, &diagnostic).unwrap();
        }
    }
}

impl<'a> Files<'a> for Project {
    type FileId = usize;
    type Name = SpanOrigin;
    type Source = &'a str;

    fn name(&'a self, id: Self::FileId) -> Result<Self::Name, CRError> {
        if id == 0 {
            Ok(self.root.origin.clone())
        } else {
            Ok(self.find_file_by_id(id).map_err(|_| CRError::FileMissing)?.origin.clone())
        }
    }

    fn source(&'a self, id: Self::FileId) -> Result<Self::Source, CRError> {
        if id == 0 {
            Ok(self.root.input.as_str())
        } else {
            Ok(self.find_file_by_id(id).map_err(|_| CRError::FileMissing)?.input.as_str())
        }
    }

    fn line_index(&'a self, id: Self::FileId, byte_index: usize) -> Result<usize, CRError> {
        if id == 0 {
            Ok(self.root.line_index(byte_index).map_err(|e| map_to_cr_err(e))?)
        } else {
            Ok(self
                .find_file_by_id(id).map_err(|_| CRError::FileMissing)?
                .line_index(byte_index).map_err(|e| map_to_cr_err(e))?
            )
        }
    }

    fn line_range(&'a self, id: Self::FileId, line_index: usize) -> Result<Range<usize>, CRError> {
        if id == 0 {
            Ok(self.root.line_range(line_index).map_err(|e| map_to_cr_err(e))?)
        } else {
            Ok(self
                .find_file_by_id(id).map_err(|_| CRError::FileMissing)?
                .line_range(line_index).map_err(|e| map_to_cr_err(e))?
            )
        }
    }
}

fn map_to_cr_err(e: ast::error::Error) -> CRError {
    if let ast::error::Error::LineTooLarge { given, max } = e {
        CRError::LineTooLarge { given, max }
    } else {
        CRError::LineTooLarge { given: 0, max: 0 }
    }
}
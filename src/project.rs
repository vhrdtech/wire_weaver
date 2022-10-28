use std::collections::HashMap;
use std::ops::Range;
use codespan_reporting::files::Files;
use ast::{Definition, File, Path, Span, SpanOrigin, XpiDef};
use crate::error::{Error, ErrorKind};
use crate::warning::Warning;

#[derive(Clone)]
pub struct Project {
    pub root: File,
    local: HashMap<Path, File>,
    deps: HashMap<String, Project>,
    // config: Toml
    pub errors: Vec<Error>,
    pub warnings: Vec<Warning>,
}

impl Project {
    pub fn new(root: File) -> Self {
        Project {
            root,
            local: Default::default(),
            deps: Default::default(),
            errors: vec![],
            warnings: vec![],
        }
    }

    pub fn find_def(&self, mut path: Path) -> Result<Definition, Error> {
        if path.segments.is_empty() {
            return Err(Error {
                kind: ErrorKind::FindDef("path cannot be empty".to_owned()),
                span: Span::call_site(),
            });
        }
        if path.is_from_crate() {
            let _ = path.pop_front();
            match path.pop_front() {
                Some(id) => {
                    let def = self.root.defs.get(&id).ok_or(Error {
                        kind: ErrorKind::FindDef(format!("crate::{} not found", id)),
                        span: Span::call_site(),
                    })?;
                    if path.is_empty() {
                        Ok(def.clone())
                    } else {
                        match def {
                            Definition::Xpi(xpi_def) => {
                                Ok(Definition::Xpi(Self::find_in_xpi_def(xpi_def, path)?))
                            }
                            _ => Err(Error {
                                kind: ErrorKind::FindDef("only xpi definition can have child items".to_owned()),
                                span: Span::call_site(),
                            })
                        }
                    }
                }
                None => {
                    return Err(Error {
                        kind: ErrorKind::FindDef("crate root is not a definition".to_owned()),
                        span: Span::call_site(),
                    });
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
                Err(Error {
                    kind: ErrorKind::FindDef(format!("find_in_xpi_def: not found: {}", search_key)),
                    span: Span::call_site(),
                })
            }
            None => Ok(xpi_def.clone())
        }
    }

    pub fn find_xpi_def(&self, path: Path) -> Result<XpiDef, Error> {
        let def = self.find_def(path.clone())?;
        match def {
            Definition::Xpi(xpi_def) => Ok(xpi_def),
            _ => Err(Error {
                kind: ErrorKind::FindXpiDef(format!("{} is not and xpi definition", path)),
                span: Span::call_site(),
            })
        }
    }

    pub fn print_report(&self) {}
}

impl<'a> Files<'a> for Project {
    type FileId = usize;
    type Name = SpanOrigin;
    type Source = &'a str;

    fn name(&'a self, id: Self::FileId) -> Result<Self::Name, codespan_reporting::files::Error> {
        todo!()
    }

    fn source(&'a self, id: Self::FileId) -> Result<Self::Source, codespan_reporting::files::Error> {
        todo!()
    }

    fn line_index(&'a self, id: Self::FileId, byte_index: usize) -> Result<usize, codespan_reporting::files::Error> {
        todo!()
    }

    fn line_range(&'a self, id: Self::FileId, line_index: usize) -> Result<Range<usize>, codespan_reporting::files::Error> {
        todo!()
    }
}
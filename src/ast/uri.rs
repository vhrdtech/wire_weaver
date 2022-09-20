use crate::ast::identifier::Identifier;
use crate::ast::lit::VecLit;

/// Universal resource identifier, consisting of UriSegment's.
/// Syntax: (/ ~ UriSegment ~ UriIndex?)+
/// Examples:
/// `/abc/def` - simple form
/// `/channel[3]/def` - with indexing
/// `/0/1/2/3` - mapped into serials
/// `/method()` ?
/// `/method(1, "str")` ?
/// `/property = 10` ?
/// 'let uri = Uri::parse("/abc/def").unwrap();'
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Uri {
    pub segments: Vec<UriSegment>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UriSegment {
    Ident { ident: Identifier },
    Index { ident: Identifier, by: VecLit },
    Serial { serial: u32 },
    SerialIndex { serial: u32, by: VecLit },
}
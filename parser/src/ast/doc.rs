use super::prelude::*;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct Doc(pub ast::Doc);

impl<'i> Parse<'i> for Doc {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Doc, ParseErrorSource> {
        let mut lines = Vec::new();
        while let Some(p) = input.pairs.peek() {
            if p.as_rule() == Rule::doc_comment {
                let p = input.pairs.next().unwrap();
                let line = &p.as_str()[3..];
                let line = line.strip_prefix(" ").unwrap_or(line);
                let line = line
                    .strip_suffix("\r\n")
                    .or(line.strip_suffix("\n"))
                    .unwrap_or(line);
                lines.push((Rc::new(line.to_owned()), ast_span_from_pest(p.as_span())));
            } else {
                break;
            }
        }
        Ok(Doc(ast::Doc { lines }))
    }
}

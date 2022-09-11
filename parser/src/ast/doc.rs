use super::prelude::*;
use pest::Span;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
pub struct Doc<'i> {
    pub lines: Vec<(&'i str, Span<'i>)>,
}

impl<'i> Parse<'i> for Doc<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Doc<'i>, ParseErrorSource> {
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
                lines.push((line, p.as_span()));
            } else {
                break;
            }
        }
        Ok(Doc { lines })
    }
}

impl<'i> Display for Doc<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\x1b[32m")?;
        for l in &self.lines {
            writeln!(f, "/// {}", l.0)?;
        }
        write!(f, "\x1b[0m")
    }
}

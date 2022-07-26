use super::prelude::*;

#[derive(Debug)]
pub struct Typename<'i> {
    pub typename: &'i str,
}

impl<'i> Parse<'i> for Typename<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Typename<'i>, ()> {
        if let Some(p) = input.pairs.peek() {
            return if p.as_rule() == Rule::identifier {
                let p = input.pairs.next().unwrap();
                Ok(Typename {
                    typename: p.as_str()
                })
            } else {
                Err(())
            };
        }
        Err(())
    }
}

#[derive(Debug)]
pub struct PathSegment<'i> {
    pub segment: &'i str,
}

impl<'i> Parse<'i> for PathSegment<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ()> {
        match input.pairs.next() {
            Some(identifier) => {
                if identifier.as_rule() != Rule::identifier {
                    return Err(())
                }
                Ok(PathSegment {
                    segment: identifier.as_str()
                })
            },
            None => Err(())
        }
    }
}
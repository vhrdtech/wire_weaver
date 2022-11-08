use crate::token_tree::TokenTree;
use crate::{Group, Spacing};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::iter::FromIterator;

#[derive(Clone, Eq, PartialEq)]
pub struct TokenStream {
    pub(crate) inner: VecDeque<TokenTree>,
}

pub trait ToTokens {
    /// Convert self into a set of tokens and append them to the provided TokenStream.
    fn to_tokens(&self, tokens: &mut TokenStream);
}

impl TokenStream {
    pub fn new() -> Self {
        TokenStream {
            inner: VecDeque::new(),
        }
    }

    /// Modify spacing of the latest token tree inserted.
    /// Used to remove spaces with ∅ sign it mquote! when this is important.
    pub fn modify_last_spacing(&mut self, spacing: Spacing) {
        match self.inner.back_mut() {
            Some(tt) => tt.modify_spacing(spacing),
            None => {}
        }
    }

    /// Replace all groups that contain [TokenTree::Repetition] inside with many groups, each containing
    /// it's own tokens.
    pub fn interpolate_repetitions(&mut self, streams_at: HashMap<usize, VecDeque<TokenStream>>) {
        // for (idx, streams) in streams_at {
        //     println!("RI{} = {:?}", idx, streams);
        // }
        self.inner = Self::interpolate_repetitions_inner(self, &streams_at, None).inner;
    }

    fn interpolate_repetitions_inner(
        ts: &mut TokenStream,
        streams_all: &HashMap<usize, VecDeque<TokenStream>>,
        mut streams_current_rg: Option<&mut HashMap<usize, VecDeque<TokenStream>>>,
    ) -> TokenStream {
        let mut tts_reassemble = TokenStream::new();
        while let Some(t) = ts.inner.pop_front() {
            match t {
                TokenTree::RepetitionGroup(group_ts, separator) => {
                    // collect all repetitions in this repetition group, recursively inside normal groups as well
                    let mut streams_in_this_group: HashMap<usize, VecDeque<TokenStream>> =
                        HashMap::new();
                    // all provided streams must be of the same length, or collect_repetitions() will panic
                    let streams_len = Self::collect_repetitions(
                        &group_ts,
                        streams_all,
                        &mut streams_in_this_group,
                        None,
                    )
                        .expect(
                            "Empty repetition group, consider removing ⸨ ⸩* or adding ∀iter inside",
                        );

                    // iterate over them all in parallel appending to tts_reassemble
                    for i in 0..streams_len {
                        for t in &group_ts.inner {
                            match t.clone() {
                                TokenTree::Repetition(idx) => {
                                    match streams_in_this_group.get_mut(&idx) {
                                        Some(streams) => match streams.pop_front() {
                                            Some(repeat_over_ts) => {
                                                for t in repeat_over_ts.inner {
                                                    tts_reassemble.inner.push_back(t);
                                                }
                                            }
                                            None => {}
                                        },
                                        None => {
                                            panic!("Internal error in interpolate_repetitions_inner: {}", idx)
                                        }
                                    }
                                }
                                TokenTree::RepetitionGroup(mut group_ts, separator) => {
                                    for tt in Self::interpolate_repetitions_inner(
                                        &mut group_ts,
                                        streams_all,
                                        None,
                                    ) {
                                        tts_reassemble.inner.push_back(tt);
                                        match separator.clone() {
                                            Some(p) => {
                                                if p.is_sequence_delimiter() {
                                                    tts_reassemble.inner.back_mut().map(|tt| {
                                                        tt.modify_spacing(Spacing::Joint)
                                                    });
                                                }
                                                tts_reassemble.inner.push_back(p.into())
                                            }
                                            None => {}
                                        }
                                    }
                                }
                                TokenTree::Group(mut group) => {
                                    tts_reassemble.inner.push_back(TokenTree::Group(Group::new(
                                        group.delimiter,
                                        Self::interpolate_repetitions_inner(
                                            &mut group.stream,
                                            streams_all,
                                            Some(&mut streams_in_this_group),
                                        ),
                                    )));
                                }
                                any_else => {
                                    tts_reassemble.inner.push_back(any_else.clone());
                                }
                            }
                        }
                        if i < streams_len - 1 {
                            match separator.clone() {
                                Some(p) => {
                                    if p.is_sequence_delimiter() {
                                        tts_reassemble
                                            .inner
                                            .back_mut()
                                            .map(|tt| tt.modify_spacing(Spacing::Joint));
                                    }
                                    tts_reassemble.inner.push_back(p.into())
                                }
                                None => {}
                            }
                        }
                    }
                }
                TokenTree::Repetition(idx) => {
                    match streams_current_rg
                        .as_mut()
                        .expect("Repetition can only be inside a repetition group delimited by ⸨ ⸩")
                        .get_mut(&idx)
                    {
                        Some(streams) => match streams.pop_front() {
                            Some(repeat_over_ts) => {
                                for t in repeat_over_ts.inner {
                                    tts_reassemble.inner.push_back(t);
                                }
                            }
                            None => {}
                        },
                        None => {
                            panic!("Internal error in interpolate_repetitions_inner: {}", idx)
                        }
                    }
                }
                TokenTree::Group(mut group) => {
                    tts_reassemble.inner.push_back(TokenTree::Group(Group::new(
                        group.delimiter,
                        Self::interpolate_repetitions_inner(&mut group.stream, streams_all, None),
                    )));
                }
                any_else => {
                    tts_reassemble.inner.push_back(any_else);
                }
            }
        }
        tts_reassemble
    }

    fn collect_repetitions(
        ts: &TokenStream,
        streams_all: &HashMap<usize, VecDeque<TokenStream>>,
        to: &mut HashMap<usize, VecDeque<TokenStream>>,
        mut len: Option<usize>,
    ) -> Option<usize> {
        for t in &ts.inner {
            match t {
                TokenTree::Repetition(idx) => {
                    let streams = streams_all.get(&idx).unwrap().clone();
                    let next_streams_len = streams.len();
                    to.insert(*idx, streams);
                    len = match len {
                        Some(len) => {
                            if len != next_streams_len {
                                panic!("Repetition iterables must be of the same length inside each group, at repetition #{}", idx)
                            }
                            Some(len)
                        }
                        None => Some(next_streams_len),
                    };
                }
                TokenTree::Group(group) => {
                    len = Self::collect_repetitions(&group.stream, streams_all, to, len);
                }
                _ => {}
            }
        }
        len
    }

    // /// Recreate proper tree structure after using interpolation with escaped delimiters.
    // ///
    // /// For example if `#( #methods \\( #names \\) ?; )*` construction was used in mquote macro,
    // /// token stream will contain DelimiterRaw tokens flat with other tokens (no nested groups):
    // /// `Id(fun1) DR( Id(self) P. Id(x) DR) P; Id(fun2) DR( Id(self) P. Id(y) DR) P;`
    // /// will be turned into
    // /// `Id(fun1) G( Id(self) P. Id(x)  ) P; Id(fun2) G( Id(self) P. Id(y)  ) P;`
    // /// Note that first token stream is flat while the second has two nested groups.
    // ///
    // /// # Panics
    // ///
    // /// Panics if:
    // /// * Unterminated opening or closing raw delimiter is encountered.
    // /// * Non matching closing delimiter is encountered.
    // pub fn recreate_trees(&mut self) {
    //     self.inner = Self::collect_inner(self, None);
    // }
    //
    // fn collect_inner(ts: &mut TokenStream, raw: Option<DelimiterRaw>) -> VecDeque<TokenTree> {
    //     let mut tts_reassemble = VecDeque::new();
    //     while let Some(t) = ts.inner.pop_front() {
    //         match t {
    //             TokenTree::DelimiterRaw(delim) => {
    //                 if delim.is_open() {
    //                     tts_reassemble.push_back(TokenTree::Group(Group {
    //                         delimiter: delim.clone().into(),
    //                         stream: TokenStream {
    //                             inner: Self::collect_inner(ts, Some(delim)),
    //                         },
    //                     }));
    //                 } else {
    //                     match raw {
    //                         Some(open_raw_delim) => {
    //                             if !open_raw_delim.is_same_kind(delim) {
    //                                 panic!(
    //                                     "Open delimiter was: {:?} got non matching closing: {:?}",
    //                                     open_raw_delim, delim
    //                                 )
    //                             }
    //                         }
    //                         None => panic!("Unexpected closing raw delimiter: {:?}", delim),
    //                     }
    //                     return tts_reassemble;
    //                 }
    //             }
    //             TokenTree::Group(mut group) => {
    //                 tts_reassemble.push_back(TokenTree::Group(Group {
    //                     delimiter: group.delimiter,
    //                     stream: TokenStream {
    //                         inner: Self::collect_inner(&mut group.stream, None),
    //                     },
    //                 }));
    //             }
    //             any_else => {
    //                 tts_reassemble.push_back(any_else);
    //             }
    //         }
    //     }
    //     match raw {
    //         Some(open_raw_delim) => panic!("Unterminated raw delimiter: {:?}", open_raw_delim),
    //         None => {}
    //     }
    //     tts_reassemble
    // }
}

impl Display for TokenStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut joint = false;
        let mut spacing_is_enabled = true;
        for (i, tt) in self.inner.iter().enumerate() {
            if i != 0 && !joint && spacing_is_enabled {
                write!(f, " ")?;
            }
            joint = false;
            match tt {
                TokenTree::Group(tt) => {
                    joint = tt.spacing_after == Spacing::Joint;
                    write!(f, "{}", tt)
                }
                TokenTree::Ident(tt) => {
                    joint = tt.spacing() == Spacing::Joint;
                    write!(f, "{}", tt)
                }
                TokenTree::Punct(tt) => {
                    joint = tt.spacing() == Spacing::Joint;
                    write!(f, "{}", tt)
                }
                TokenTree::Literal(tt) => {
                    joint = tt.spacing() == Spacing::Joint;
                    write!(f, "{}", tt)
                }
                TokenTree::Spacing(is_enabled) => {
                    spacing_is_enabled = *is_enabled;
                    Ok(())
                }
                TokenTree::Comment(tt) => write!(f, "{}", tt),
                TokenTree::Repetition(idx) => write!(f, "RI{}", idx),
                TokenTree::RepetitionGroup(g, p) => write!(f, "RG⸨{} {:?}⸩", g, p),
            }?;
        }

        Ok(())
    }
}

impl From<TokenTree> for TokenStream {
    fn from(tree: TokenTree) -> TokenStream {
        let mut stream = TokenStream::new();
        // stream.push_token(tree);
        stream.inner.push_back(tree);
        stream
    }
}

impl FromIterator<TokenTree> for TokenStream {
    fn from_iter<I: IntoIterator<Item = TokenTree>>(tokens: I) -> Self {
        let mut stream = TokenStream::new();
        stream.extend(tokens);
        stream
    }
}

impl FromIterator<TokenStream> for TokenStream {
    fn from_iter<I: IntoIterator<Item = TokenStream>>(streams: I) -> Self {
        let mut v = VecDeque::new();

        for stream in streams {
            v.extend(stream.inner);
        }

        TokenStream { inner: v }
    }
}

impl Extend<TokenTree> for TokenStream {
    fn extend<T: IntoIterator<Item = TokenTree>>(&mut self, tokens: T) {
        tokens.into_iter().for_each(|tt| self.inner.push_back(tt));
    }
}

impl Extend<TokenStream> for TokenStream {
    fn extend<T: IntoIterator<Item = TokenStream>>(&mut self, stream: T) {
        self.inner.extend(stream.into_iter().flatten());
    }
}

impl ToTokens for TokenStream {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(self.clone().into_iter())
    }
}

pub(crate) type TokenTreeIter = std::collections::vec_deque::IntoIter<TokenTree>;

impl IntoIterator for TokenStream {
    type Item = TokenTree;
    type IntoIter = TokenTreeIter;

    fn into_iter(self) -> TokenTreeIter {
        self.inner.into_iter()
    }
}

impl Debug for TokenStream {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.inner.is_empty() {
            write!(f, "TS{{∅}}")?;
        } else {
            write!(f, "TS{{ ")?;
            for t in &self.inner {
                if f.alternate() {
                    write!(f, "{:#?} ", t)?;
                } else {
                    write!(f, "{:?} ", t)?;
                }
            }
            write!(f, "}}")?;
        }
        Ok(())
    }
}

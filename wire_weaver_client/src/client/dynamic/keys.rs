//! Keyed arrays in the dynamic client (CLT-72): the `keyed!` companion as a level of its own (`flow_keys.index_of`,
//! `flow_keys.key[3]`, `flow_keys.keys`), an element found by its key (`flow[FlowKey { .. }]`), and the paths that
//! write one.

use super::{DynResource, Node};
use crate::client::keyed::KeyNotes;
use crate::{Commander, Error};
use std::sync::Arc;
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use ww_client_server::{PathKind, ResourceIndex};
use ww_self::keys::{INDEX_OF, KEY, KEYS};
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, FieldsValueOwned, TypeOwned,
    ValueOwned,
};

/// What a resource is to a keyed array.
#[derive(Clone, Debug)]
pub(crate) enum KeyedRole {
    /// An array with a `keyed!` companion: its elements are also found by key.
    Array(Arc<Companion>),
    /// The companion, or an item of its level.
    Companion(Arc<Companion>),
}

/// A `keyed!` companion, as both its array and its own items reach it.
#[derive(Debug)]
pub(crate) struct Companion {
    /// Path of the companion.
    chain: Vec<ResourceIndex>,
    /// Id of the keyed array in their level.
    of: u32,
    key_ty: TypeOwned,
    /// `index_of`, `key[]` and `keys`, see [ww_self::keys::keys_level].
    pub(super) level: ApiLevelOwned,
}

impl Companion {
    fn keyed<'c>(&self, cmd: &'c Commander) -> Result<crate::Keyed<'c>, Error> {
        cmd.keyed(PathKind::absolute(&self.chain), self.of)
    }

    fn index_of(&self) -> &ApiItemOwned {
        self.level
            .items
            .iter()
            .find(|item| item.id.0 == INDEX_OF)
            .expect("keys_level makes index_of")
    }
}

/// One step of a resource path: `flow`, `[3]` or `[FlowKey { .. }]`.
#[derive(Clone, Debug, PartialEq)]
pub enum PathStep {
    /// A resource of a level, by name.
    Name(String),
    /// An element of an array, by index.
    Index(u32),
    /// An element of a keyed array, by its key written as a Rust expression of the key type.
    Key(String),
}

/// Most steps a resource path may have.
pub const PATH_STEPS_MAX: usize = 64;

/// Steps of `path` in the syntax of [DynResource::path], with keys in brackets: `flow[3].bytes`,
/// `flow[FlowKey { proto: Proto::Tcp, .. }].packets`. Dots, brackets and quotes inside a key belong to the key.
pub fn parse_path(path: &str) -> Result<Vec<PathStep>, Error> {
    let bad = |what: &str| Error::Other(format!("path '{path}': {what}"));
    let mut steps = vec![];
    let mut chars = path.char_indices().peekable();
    let mut name = String::new();
    // a `.` ends a name, or follows a `]`
    let mut after_bracket = false;
    while let Some((at, c)) = chars.next() {
        match c {
            '.' => {
                if name.is_empty() && !after_bracket {
                    return Err(bad("empty name"));
                }
                if !name.is_empty() {
                    steps.push(PathStep::Name(std::mem::take(&mut name)));
                }
                after_bracket = false;
            }
            '[' => {
                if !name.is_empty() {
                    steps.push(PathStep::Name(std::mem::take(&mut name)));
                } else if steps.is_empty() {
                    return Err(bad("an index needs an array before it"));
                }
                let start = at + 1;
                let end = bracket_end(path, start).ok_or_else(|| bad("missing ']'"))?;
                let inside = path[start..end].trim();
                if inside.is_empty() {
                    return Err(bad("empty index"));
                }
                steps.push(match inside.parse::<u32>() {
                    Ok(index) => PathStep::Index(index),
                    Err(_) if inside.bytes().all(|b| b.is_ascii_digit()) => {
                        return Err(bad("index out of range"));
                    }
                    Err(_) => PathStep::Key(inside.to_owned()),
                });
                while chars.next_if(|&(i, _)| i <= end).is_some() {}
                after_bracket = true;
            }
            c if after_bracket => {
                return Err(bad(&format!("expected '.' or '[' after ']', found '{c}'")));
            }
            c => name.push(c),
        }
        if steps.len() > PATH_STEPS_MAX {
            return Err(bad(&format!("more than {PATH_STEPS_MAX} steps")));
        }
    }
    if !name.is_empty() {
        steps.push(PathStep::Name(name));
    } else if !path.is_empty() && !after_bracket {
        return Err(bad("ends with '.'"));
    }
    Ok(steps)
}

/// Byte offset of the `]` that closes a bracket opened just before `start`, skipping nested brackets, braces,
/// parentheses, strings and char literals.
fn bracket_end(path: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (at, c) in path[start..].char_indices() {
        if let Some(q) = quote {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                c if c == q => quote = None,
                _ => {}
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '[' | '{' | '(' => depth += 1,
            ']' if depth == 0 => return Some(start + at),
            ']' | '}' | ')' => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    None
}

impl DynResource {
    /// The role `item` of `level` (this resource's level) plays in a keyed array.
    pub(super) fn keyed_role(
        &self,
        level: &ApiLevelOwned,
        item: &ApiItemOwned,
    ) -> Result<Option<KeyedRole>, Error> {
        if let Node::Keys = self.node {
            return Ok(self.keyed.clone());
        }
        let companion = match &item.kind {
            ApiItemKindOwned::Keys { .. } => Some(item),
            _ if item.is_array() => level.items.iter().find(
                |keys| matches!(keys.kind, ApiItemKindOwned::Keys { of, .. } if of == item.id),
            ),
            _ => None,
        };
        let Some(keys) = companion else {
            return Ok(None);
        };
        let ApiItemKindOwned::Keys { of, key, .. } = &keys.kind else {
            unreachable!("found as a Keys item");
        };
        let mut chain = self.index_chain.clone();
        chain.push(ResourceIndex::new(keys.id));
        let companion = Arc::new(Companion {
            chain,
            of: of.0,
            key_ty: key.clone(),
            level: ww_self::keys::keys_level(level, keys)
                .map_err(|e| Error::Other(format!("'{}': {e}", keys.ident)))?,
        });
        Ok(Some(if keys.id == item.id {
            KeyedRole::Companion(companion)
        } else {
            KeyedRole::Array(companion)
        }))
    }

    /// The companion of a keyed array, for an array that has one.
    fn companion(&self) -> Result<&Companion, Error> {
        match &self.keyed {
            Some(KeyedRole::Array(companion)) if matches!(self.node, Node::Array) => Ok(companion),
            _ => Err(self.not_a("keyed array")),
        }
    }

    /// Key type of a keyed array, `None` for anything else.
    pub fn key_ty(&self) -> Option<&TypeOwned> {
        self.companion().ok().map(|c| &c.key_ty)
    }

    /// The `index_of` request of `key`: the companion, the key's bytes and the arguments.
    fn index_of_request(&self, key: &ValueOwned) -> Result<(&Companion, Vec<u8>, Vec<u8>), Error> {
        let companion = self.companion()?;
        let key_bytes = key
            .ser_shrink_wrap_dyn(&companion.key_ty, &self.bundle)
            .map_err(|e| Error::Other(format!("'{}' key: {e:#}", self.path)))?;
        let ApiItemKindOwned::Method { args, .. } = &companion.index_of().kind else {
            unreachable!("index_of is a method");
        };
        let args = ValueOwned::ser_args_dyn(std::slice::from_ref(key), args, &self.bundle)
            .map_err(|e| Error::Other(format!("'{}' key: {e:#}", self.path)))?;
        Ok((companion, key_bytes, args))
    }

    /// `index_of`'s answer, an `Option` of the array's index type, as an index.
    fn decode_index(&self, bytes: &[u8]) -> Result<Option<u32>, Error> {
        let companion = self.companion()?;
        let ApiItemKindOwned::Method {
            return_ty: Some(return_ty),
            ..
        } = &companion.index_of().kind
        else {
            unreachable!("index_of is a method returning an index");
        };
        let value = ValueOwned::des_shrink_wrap_dyn(bytes, return_ty, &self.bundle)
            .map_err(|e| Error::Other(format!("'{}' index_of: {e:#}", self.path)))?;
        match value {
            ValueOwned::Option(None) => Ok(None),
            ValueOwned::Option(Some(index)) => index_number(&index).map(Some).ok_or_else(|| {
                Error::Other(format!(
                    "'{}' index_of: {index:?} is not an index",
                    self.path
                ))
            }),
            other => Err(Error::Other(format!(
                "'{}' index_of: {other:?} is not an Option",
                self.path
            ))),
        }
    }

    /// The element of a keyed array with `key`, `None` when the device has none: the device is asked once
    /// (`index_of`), later lookups take the index from the connection's key cache.
    pub async fn by_key(
        &self,
        cmd: &Commander,
        key: &ValueOwned,
    ) -> Result<Option<DynResource>, Error> {
        let (companion, key_bytes, args) = self.index_of_request(key)?;
        let index = companion
            .keyed(cmd)?
            .index_of_with(key_bytes, Ok(args), |b| self.decode_index(b))
            .await?;
        index.map(|index| self.index(index)).transpose()
    }

    /// Blocking variant of [Self::by_key].
    pub fn by_key_blocking(
        &self,
        cmd: &Commander,
        key: &ValueOwned,
    ) -> Result<Option<DynResource>, Error> {
        let (companion, key_bytes, args) = self.index_of_request(key)?;
        let index = companion
            .keyed(cmd)?
            .index_of_with_blocking(key_bytes, Ok(args), |b| self.decode_index(b))?;
        index.map(|index| self.index(index)).transpose()
    }

    /// The element of a keyed array with the key `text`, a Rust expression of the key type.
    fn by_key_text(&self, text: &str) -> Result<ValueOwned, Error> {
        let key_ty = self
            .key_ty()
            .ok_or_else(|| self.not_a("keyed array"))?
            .clone();
        crate::expr::parse_value(text, &key_ty, &self.bundle)
            .map_err(|e| Error::Other(format!("'{}[{text}]': {e:#}", self.path)))
    }

    /// The resource at `path` from this one, in the syntax of [Self::path]: `flow[3].bytes`. A key step
    /// (`flow[FlowKey { .. }]`) needs the device, see [Self::walk_on].
    pub fn walk(&self, path: &str) -> Result<DynResource, Error> {
        let mut res = self.clone();
        for step in parse_path(path)? {
            res = match step {
                PathStep::Name(name) => res.child(&name)?,
                PathStep::Index(index) => res.index(index)?,
                PathStep::Key(key) => {
                    return Err(Error::Other(format!(
                        "'{}[{key}]': an element by key is found on the device, connect to it",
                        res.path
                    )));
                }
            };
        }
        Ok(res)
    }

    /// [Self::walk], keys included: each key step asks the device for the index once per key.
    pub async fn walk_on(&self, cmd: &Commander, path: &str) -> Result<DynResource, Error> {
        let mut res = self.clone();
        for step in parse_path(path)? {
            res = match step {
                PathStep::Name(name) => res.child(&name)?,
                PathStep::Index(index) => res.index(index)?,
                PathStep::Key(text) => {
                    let key = res.by_key_text(&text)?;
                    res.by_key(cmd, &key)
                        .await?
                        .ok_or_else(|| no_element(&res, &text))?
                }
            };
        }
        Ok(res)
    }

    /// Blocking variant of [Self::walk_on].
    pub fn walk_on_blocking(&self, cmd: &Commander, path: &str) -> Result<DynResource, Error> {
        let mut res = self.clone();
        for step in parse_path(path)? {
            res = match step {
                PathStep::Name(name) => res.child(&name)?,
                PathStep::Index(index) => res.index(index)?,
                PathStep::Key(text) => {
                    let key = res.by_key_text(&text)?;
                    res.by_key_blocking(cmd, &key)?
                        .ok_or_else(|| no_element(&res, &text))?
                }
            };
        }
        Ok(res)
    }

    /// The cache of the keyed array this resource is the `key[]` element or `keys` stream of.
    fn notes(&self, cmd: &Commander, id: u32) -> Option<KeyNotes> {
        let Some(KeyedRole::Companion(companion)) = &self.keyed else {
            return None;
        };
        if self.item.as_ref()?.id.0 != id {
            return None;
        }
        companion.keyed(cmd).ok()?.notes().cloned()
    }

    /// The cache follows a read of `key[i]`.
    pub(super) fn key_read(&self, cmd: &Commander, result: &Result<TailBytesOwned, Error>) {
        if let Some(notes) = self.notes(cmd, KEY)
            && let Some(index) = self.index_chain.last()
        {
            notes.key_read(index.get().0, result);
        }
    }

    /// The cache and the key type a `keys` stream's announcements are followed with.
    pub(super) fn key_events(&self, cmd: &Commander) -> Option<(KeyNotes, TypeOwned)> {
        let Some(KeyedRole::Companion(companion)) = &self.keyed else {
            return None;
        };
        Some((self.notes(cmd, KEYS)?, companion.key_ty.clone()))
    }
}

fn no_element(array: &DynResource, key: &str) -> Error {
    Error::Other(format!(
        "'{}' has no element with the key {key}",
        array.path
    ))
}

/// The number of an index value: a plain number, or a newtype around one (`FlowIdx(u16)`).
fn index_number(value: &ValueOwned) -> Option<u32> {
    match value {
        ValueOwned::Numeric(n) => u32::try_from(n.to_i128()?).ok(),
        ValueOwned::Struct {
            fields: FieldsValueOwned::Unnamed(fields),
        } if fields.len() == 1 => index_number(&fields[0]),
        _ => None,
    }
}

/// The cache follows one announcement of a `keys` stream, `KeyEvent::Added { index, key }` or `Removed { index }`.
pub(super) fn note_event(
    notes: &KeyNotes,
    key_ty: &TypeOwned,
    bundle: &ApiBundleOwned,
    event: &ValueOwned,
) {
    let ValueOwned::Enum {
        variant,
        fields: FieldsValueOwned::Named(fields),
    } = event
    else {
        return;
    };
    let field = |name: &str| fields.iter().find(|(n, _)| n == name).map(|(_, v)| v);
    let Some(index) = field("index").and_then(index_number) else {
        return;
    };
    match variant.as_str() {
        "Added" => {
            if let Some(key) = field("key")
                && let Ok(bytes) = key.ser_shrink_wrap_dyn(key_ty, bundle)
            {
                notes.learn(index, bytes);
            }
        }
        "Removed" => notes.forget(index),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> PathStep {
        PathStep::Key(s.to_owned())
    }

    fn name(s: &str) -> PathStep {
        PathStep::Name(s.to_owned())
    }

    #[test]
    fn paths_with_indices_and_keys() {
        assert_eq!(parse_path("").unwrap(), []);
        assert_eq!(
            parse_path("bank[0].level").unwrap(),
            [name("bank"), PathStep::Index(0), name("level")]
        );
        assert_eq!(
            parse_path("a[1][2]").unwrap(),
            [name("a"), PathStep::Index(1), PathStep::Index(2)]
        );
        assert_eq!(
            parse_path(
                "flow[FlowKey { port: Port(5000), addr: [10, 0, 0, 1], s: \"a.]\" }].packets"
            )
            .unwrap(),
            [
                name("flow"),
                key("FlowKey { port: Port(5000), addr: [10, 0, 0, 1], s: \"a.]\" }"),
                name("packets")
            ]
        );
        assert_eq!(
            parse_path("flow[b']'].x").unwrap(),
            [name("flow"), key("b']'"), name("x")]
        );
    }

    #[test]
    fn bad_paths() {
        for path in [
            "a..b",
            "a.",
            ".a",
            "[0]",
            "a[",
            "a[]",
            "a[0]b",
            "a[{]",
            "a[99999999999]",
        ] {
            assert!(parse_path(path).is_err(), "{path}");
        }
    }

    #[test]
    fn index_numbers() {
        use ww_numeric::NumericValue;
        assert_eq!(
            index_number(&ValueOwned::Numeric(NumericValue::U16(7))),
            Some(7)
        );
        let newtype = ValueOwned::Struct {
            fields: FieldsValueOwned::Unnamed(vec![ValueOwned::Numeric(NumericValue::U8(3))]),
        };
        assert_eq!(index_number(&newtype), Some(3));
        assert_eq!(
            index_number(&ValueOwned::Numeric(NumericValue::I8(-1))),
            None
        );
    }
}

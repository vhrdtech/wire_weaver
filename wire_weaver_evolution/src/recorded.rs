//! What a snapshot file recorded: the fields of the API model its writer knew.
//!
//! A snapshot is the RON of a `ww_self` bundle, written with every field of every struct. A field added to the model
//! later (`ItemEnum::future_version`, EVO-10) is missing from every snapshot saved before, and reads back as its
//! default, which current codegen may not write: the unchanged source would then differ from its snapshot only because
//! wire_weaver was upgraded. So a field that a file has nowhere, in no instance of its struct, counts as not recorded
//! there, and both sides are compared without it ([Recorded::leave_out]). This holds for any field added to the model
//! later, without a list of fields or of `ww_self` versions (a field can be added without a `ww_self` bump in between
//! releases).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Deserialize;
use serde::de::value::StringDeserializer;
use serde::de::{
    self, DeserializeSeed, Deserializer, EnumAccess, IgnoredAny, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};
use ww_self::ApiBundleOwned;

/// Which fields of the model a snapshot file has, see the [module docs](self).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recorded {
    /// Every struct (or struct variant) met in the file: the fields its type declares, and those the file has in at
    /// least one instance.
    structs: BTreeMap<String, StructFields>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct StructFields {
    declared: Vec<&'static str>,
    seen: BTreeSet<String>,
}

/// A field of the model a file did not record: `ItemEnumOwned` and `future_version`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unrecorded {
    /// The struct's serde name, or `Enum::Variant` for a struct variant.
    pub owner: String,
    pub field: String,
}

impl fmt::Display for Unrecorded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `ItemEnumOwned` is `ItemEnum` in the docs, `ApiItemKindOwned::Method` is `ApiItemKind::Method`
        let owner = self.owner.replace("Owned::", "::");
        let owner = owner.strip_suffix("Owned").unwrap_or(&owner);
        write!(f, "{owner}::{}", self.field)
    }
}

impl Recorded {
    /// Reads a bundle from a snapshot's RON text and what it recorded.
    pub fn parse(text: &str) -> Result<(ApiBundleOwned, Recorded), String> {
        let state = RefCell::new(State::default());
        let parsed = (|| {
            let mut de = ron::Deserializer::from_str(text).map_err(|e| e.to_string())?;
            let bundle = ApiBundleOwned::deserialize(Wrap::new(&mut de, &state))
                .map_err(|e| e.to_string())?;
            de.end().map_err(|e| e.to_string())?;
            Ok::<_, String>(bundle)
        })();
        match parsed {
            Ok(bundle) => Ok((bundle, state.into_inner().recorded)),
            // ron's own entry point says where in the file
            Err(e) => Err(ron::from_str::<ApiBundleOwned>(text)
                .err()
                .map_or(e, |e| e.to_string())),
        }
    }

    /// Fields that the file has in no instance of their struct, of the structs it has at all. A struct the file
    /// doesn't have tells nothing: what uses it is new on the other side anyway.
    pub fn unrecorded(&self) -> BTreeSet<Unrecorded> {
        self.structs
            .iter()
            .flat_map(|(owner, fields)| {
                fields
                    .declared
                    .iter()
                    .filter(|f| !fields.seen.contains(**f))
                    .map(|field| Unrecorded {
                        owner: owner.clone(),
                        field: field.to_string(),
                    })
            })
            .collect()
    }

    /// `bundle` with the `fields` as their default, as if read from a file without them.
    pub fn leave_out(
        bundle: &ApiBundleOwned,
        fields: &BTreeSet<Unrecorded>,
    ) -> Result<ApiBundleOwned, String> {
        if fields.is_empty() {
            return Ok(bundle.clone());
        }
        let text = ron::to_string(bundle).map_err(|e| e.to_string())?;
        let state = RefCell::new(State {
            leave_out: fields
                .iter()
                .map(|u| (u.owner.clone(), u.field.clone()))
                .collect(),
            ..State::default()
        });
        let mut de = ron::Deserializer::from_str(&text).map_err(|e| e.to_string())?;
        ApiBundleOwned::deserialize(Wrap::new(&mut de, &state)).map_err(|e| e.to_string())
    }
}

#[derive(Default)]
struct State {
    recorded: Recorded,
    /// Fields skipped while reading, by owner and name.
    leave_out: BTreeSet<(String, String)>,
}

impl State {
    fn start(&mut self, owner: &str, fields: &'static [&'static str]) {
        let entry = self.recorded.structs.entry(owner.to_string()).or_default();
        entry.declared = fields.to_vec();
    }
}

/// A serde deserializer (or seed, visitor, access) forwarding to the wrapped one and wrapping what it hands out, so
/// that every struct below is seen. `owner` is set for the visitor of a struct: its map's keys are its fields.
struct Wrap<'s, T> {
    inner: T,
    state: &'s RefCell<State>,
    owner: Option<String>,
}

impl<'s, T> Wrap<'s, T> {
    fn new(inner: T, state: &'s RefCell<State>) -> Self {
        Wrap {
            inner,
            state,
            owner: None,
        }
    }

    fn with<U>(&self, inner: U) -> Wrap<'s, U> {
        Wrap::new(inner, self.state)
    }

    fn owned_by<U>(&self, inner: U, owner: String) -> Wrap<'s, U> {
        Wrap {
            inner,
            state: self.state,
            owner: Some(owner),
        }
    }
}

macro_rules! forward_deserialize {
    ($($method:ident)*) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            let visitor = self.with(visitor);
            self.inner.$method(visitor)
        }
    )*};
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for Wrap<'_, D> {
    type Error = D::Error;

    forward_deserialize!(
        deserialize_any deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64
        deserialize_i128 deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64 deserialize_u128
        deserialize_f32 deserialize_f64 deserialize_char deserialize_str deserialize_string deserialize_bytes
        deserialize_byte_buf deserialize_option deserialize_unit deserialize_seq deserialize_map
        deserialize_identifier deserialize_ignored_any
    );

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let visitor = self.with(visitor);
        self.inner.deserialize_unit_struct(name, visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let visitor = self.with(visitor);
        self.inner.deserialize_newtype_struct(name, visitor)
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let visitor = self.with(visitor);
        self.inner.deserialize_tuple(len, visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let visitor = self.with(visitor);
        self.inner.deserialize_tuple_struct(name, len, visitor)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.state.borrow_mut().start(name, fields);
        let visitor = self.owned_by(visitor, name.to_string());
        self.inner.deserialize_struct(name, fields, visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let visitor = self.owned_by(visitor, name.to_string());
        self.inner.deserialize_enum(name, variants, visitor)
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Wrap<'_, S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        let deserializer = self.with(deserializer);
        self.inner.deserialize(deserializer)
    }
}

macro_rules! forward_visit {
    ($($method:ident($ty:ty))*) => {$(
        fn $method<E: de::Error>(self, v: $ty) -> Result<Self::Value, E> {
            self.inner.$method(v)
        }
    )*};
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Wrap<'_, V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.inner.expecting(f)
    }

    forward_visit!(
        visit_bool(bool) visit_i8(i8) visit_i16(i16) visit_i32(i32) visit_i64(i64) visit_i128(i128)
        visit_u8(u8) visit_u16(u16) visit_u32(u32) visit_u64(u64) visit_u128(u128) visit_f32(f32) visit_f64(f64)
        visit_char(char) visit_str(&str) visit_borrowed_str(&'de str) visit_string(String)
        visit_bytes(&[u8]) visit_borrowed_bytes(&'de [u8]) visit_byte_buf(Vec<u8>)
    );

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_none()
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        let deserializer = self.with(deserializer);
        self.inner.visit_some(deserializer)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        let deserializer = self.with(deserializer);
        self.inner.visit_newtype_struct(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        let seq = self.with(seq);
        self.inner.visit_seq(seq)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        let map = Wrap {
            inner: map,
            state: self.state,
            owner: self.owner,
        };
        self.inner.visit_map(map)
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
        let data = Wrap {
            inner: data,
            state: self.state,
            owner: self.owner,
        };
        self.inner.visit_enum(data)
    }
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Wrap<'_, A> {
    type Error = A::Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        let seed = self.with(seed);
        self.inner.next_element_seed(seed)
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Wrap<'_, A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        let Some(owner) = &self.owner else {
            let seed = self.with(seed);
            return self.inner.next_key_seed(seed);
        };
        loop {
            let Some(field) = self.inner.next_key_seed(Identifier)? else {
                return Ok(None);
            };
            let left_out = {
                let mut state = self.state.borrow_mut();
                if let Some(fields) = state.recorded.structs.get_mut(owner) {
                    fields.seen.insert(field.clone());
                }
                state.leave_out.contains(&(owner.clone(), field.clone()))
            };
            if !left_out {
                return seed.deserialize(StringDeserializer::new(field)).map(Some);
            }
            self.inner.next_value::<IgnoredAny>()?;
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, Self::Error> {
        let seed = self.with(seed);
        self.inner.next_value_seed(seed)
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

impl<'de, 's, A: EnumAccess<'de>> EnumAccess<'de> for Wrap<'s, A> {
    type Error = A::Error;
    type Variant = Wrap<'s, A::Variant>;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Self::Error> {
        let (variant, access) = self.inner.variant_seed(Identifier)?;
        let enum_name = self.owner.unwrap_or_default();
        let owner = format!("{enum_name}::{variant}");
        let value = seed.deserialize(StringDeserializer::new(variant))?;
        Ok((
            value,
            Wrap {
                inner: access,
                state: self.state,
                owner: Some(owner),
            },
        ))
    }
}

impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for Wrap<'_, A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        self.inner.unit_variant()
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, Self::Error> {
        let seed = self.with(seed);
        self.inner.newtype_variant_seed(seed)
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let visitor = self.with(visitor);
        self.inner.tuple_variant(len, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let owner = self.owner.clone().unwrap_or_default();
        self.state.borrow_mut().start(&owner, fields);
        let visitor = self.owned_by(visitor, owner);
        self.inner.struct_variant(fields, visitor)
    }
}

/// A field or variant name, read as the derived code would and handed on to it afterwards.
struct Identifier;

impl<'de> DeserializeSeed<'de> for Identifier {
    type Value = String;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        deserializer.deserialize_identifier(self)
    }
}

impl<'de> Visitor<'de> for Identifier {
    type Value = String;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a field or variant name")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
        Ok(v.to_string())
    }
}

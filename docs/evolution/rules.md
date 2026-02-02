# Evolution rules

There are two sets of evolution rules - one for data types and one for API.

## Data types

WireWeaver considers two root types for evolution and evolution rule checks - `struct` and `enum` (plain and with data
variants).
Size of a type is marked as one of: `Unsized`, `FinalStructure`, `SelfDescribing` or `Sized`.

### Unsized types

By default, user defined struct or enum is `Unsized`. Both can contain variable-size types - vectors,
strings or other structs and enums. Unsized types support all the evolution options. It is recommended to stick with
unsized types, unless extreme space-saving is required.

When serializing, size of such objects is calculated and written to the resulting byte array. Which is the only
overhead, giving all the nice backwards and forwards compatibility benefits.

* New fields with default capability can be added to the end of structs, enum struct and tuple variants.
    * `Option<T>` - None is read from old data,
    * `Vec<T>` - Empty vector is read from old data,
    * `String` - Empty string is read from old data,
    * `T` can be anything.
* New `Sized` fields can be added into previously unused padding bits.
* TODO: clarify: `T` -> struct containing `T`
* Struct fields and enum variants can be renamed (but their position must NOT change).

### FinalStructure, SelfDescribing and Sized types

* New `Sized` fields can be added into previously unused padding bits.
* Struct fields and enum variants can be renamed (but their position must NOT change).

## API

Data types used in API are a part of SemVer guarantee and are subject to the rules above. Meaning that it's not allowed
to break compatibility on any of the data types used directly or indirectly without also bumping a major version of an
API as well.

* Adding argument
* `T` to struct of `T` in return position
* `T` to `Vec<T>` in return position?

### API model

API model (like `ww_client_server`) is part of the compatibility equation, it is not allowed to update the model
version without breaking compatibility.

E.g., if user_device_api v0.1.0 depends on ww_client_server v0.4.0 and a new major version of ww_client_server comes
out (v0.5.0),
then user_device_api must be bumped to v0.2.0 to use the newer API model. This should only happen to add new features
though,
and if previous version is doing all that it is supposed to, there might not be a need to upgrade.
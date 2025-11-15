# Introduction

<div style="text-align:center">
<img src="./assets/logo.png" alt="logo" width="200"/>
</div>

WireWeaver is an API code generator for microcontrollers, supporting user-defined types, methods, properties, streams,
and traits.
It handles unsized types like Vec<T> and String even in no_std environments without an allocator,
and ensures full backward and forward compatibility between devices across format versions.

# Recommended learning order

1. Get familiar with the SerDes functionality: [wire format](./serdes/shrink_wrap.md)
   and [derive macro](./serdes/derive.md).
2. See the full list of [supported types](./types.md).
3. Understand [API capabilities](./api/overview.md).
4. See it in action on real hardware or on virtual device [template](https://github.com/vhrdtech/wire_weaver_template).
5. Read the rest of the docs, in particular: [evolution rules](./evolution/rules.md), [versioning](./api/versioning.md)
   and [addressing](./api/addressing.md).
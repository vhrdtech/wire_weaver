---
level: basic
---

# Structs and enums

Your own types go on the wire the same way as the built-in ones: put `#[derive_shrink_wrap]` on a struct or an enum,
and it can be a field of another type. This chapter shows what a struct and an enum cost on the wire, and how an enum
picks the size of the number that tells its variants apart.

The sensor node of the example sends more than readings. After power-up it says hello with its number and its
firmware version, and before its battery runs out it says goodbye. Each of these is a message, and an enum holds all
of them.

## Structs

A struct is its fields in the order you declare them, with the rules of
[Your types on the wire](types_on_the_wire.md#where-each-bit-goes): flags take a bit, whole-byte values start on a
byte, and lengths go to the back. A struct with fields named, a tuple struct and a unit struct are all written this
way.

The number of a sensor node is a tuple struct of one `u8`. A type of its own, rather than a bare `u8`, keeps a node's
number from being mixed up with any other byte:

```rust
--8<-- "examples/sensor_packet/src/lib.rs:node_id"
```

On its own, `NodeId(7)` is one byte, `07`. A struct inside another type also records its length, usually in a
nibble. That length lets a newer version of the struct add fields later, which
[Types that change](types_that_change.md) explains. Such a struct is [[evolvable]], and every derived struct is
evolvable unless you say otherwise.

## Enums

An enum is a [[discriminant]], the number of its variant, followed by the fields of that variant. Variants are
numbered in the order you declare them, from 0. Here are the messages of the sensor node:

```rust
--8<-- "examples/sensor_packet/src/lib.rs:message"
```

A variant can have named fields like `Hello`, a single value like `Reading`, or nothing like `Goodbye`. `ww_repr = u4`
says how the discriminant goes on the wire: in 4 bits, which leaves room for 16 variants. Every enum needs a
`ww_repr`. These are the choices:

| `ww_repr`              | Discriminant on the wire                 | Variants    |
|------------------------|------------------------------------------|-------------|
| `u1` to `u31`          | that many bits                           | 2 to 2³¹    |
| `nib`                  | a nibble, on a 4-bit boundary            | 16          |
| `u8`, `u16`, `u32`     | whole bytes                              | up to 2³²   |
| `unib32`               | a variable-length number in nibbles      | up to 2³²   |

Choose the smallest that fits every variant you expect to need, including the ones a later firmware adds. `unib32`
takes one nibble for the first 8 variants and grows from there, so it never runs out.

## What the messages cost

The test `book_output_messages` writes each message of the example on its own:

--8<-- "docs/generated/sensor_packet/messages.md"

Each message starts with its discriminant in the first nibble: 0 for `Hello`, 1 for `Reading`, 2 for `Goodbye`.
`Goodbye` has no fields, so it is that nibble and four bits of padding. `Hello` has the node's number, `07`, and the
firmware version, `31 2e 30` for `"1.0"`. Its last byte holds two lengths, a nibble each: 1 byte for the node's
number and 3 for the version.

`Reading` carries the same nine bytes the node sent in [the first chapter](types_on_the_wire.md), unchanged. The
reading's own length, 9, follows at the back. It takes two nibbles, `19`, because one nibble holds a length up to 7.

## A variant the reader doesn't know

A gateway with older software can get a message from a newer node with a variant it has never heard of. The derive
prepares every enum for this: it adds a variant `Unknown(u32)` after the ones you declare, which holds the
discriminant of a variant the reader doesn't know. The older gateway then gets `Message::Unknown(3)` instead of an
error, and can log it and go on.

`Unknown` takes no number of its own, and it doesn't change the bytes of any other variant.
[Types that change](types_that_change.md#a-new-variant) shows it at work.

## Where to go next

- [Types that change](types_that_change.md) adds a field and a variant in the next firmware, and old and new ends
  keep reading each other.
- [Size assumptions](../serdes/size_assumptions.md) shows how a struct that will never change can drop its length.

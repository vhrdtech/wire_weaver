# WireWeaver

WireWeaver is a wire format and API code generator for resource constrained systems (for example microcontrollers).
It allows you to define data types, methods, properties and streams and generate code that uses no standard library or
memory allocation. Unsized types - `Vec<T>`, String and others are supported (even on no_std without allocator!).
Backwards and forwards compatibility is supported: devices with older format version can communicate with newer ones and
vice versa.

Currently only Rust language is supported, with the idea to handle device communications in Rust and provide higher
level bindings for C++, Python or other languages if needed.

Current state is - highly experimental.


Multi-language Quasi-Quoting
===

This crate provides the mquote! macro for turning any language syntax tree data
structures into tokens of source code.

Heavily inspired by Rust quote, while adding more functionality and targeting different use case:

* Main use case is code generation for various other languages
* More punctuation is added to cover language specifics
* Single line, multi line and doc comments that are automatically mapped into correct ones for a given language
* Interpolation not only for local variables, but for anything in scope with `#{path.to.smth}` syntax
* Force new line symbol: ⏎
* Force joint output (no spacing) modifier: ◡
* Automatic raw identifier conversion depending on the language (r# is added for Rust, r_ for Dart, etc)
* Force identifier (cancel raw) with: ȸ symbol before ident (`ȸtype` will produce `type`; `type` will produce `r#type`
  in Rust)

Syntax
---

```rust
let tokens = mquote!(rust r#"
    struct #name {
        x: u32
    }
"#);
```
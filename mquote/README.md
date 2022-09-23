Multi-language Quasi-Quoting
===

This crate provides the mquote! macro for turning any language syntax tree data
structures into tokens of source code.

Heavily inspired by Rust quote, while adding more functionality and targeting different use case:

* Main use case is code generation for various other languages
* More punctuation is added to cover language specifics
* Single line, multi line and doc comments that are automatically mapped into correct ones for a given language
* Interpolation not only for local variables, but for anything in scope with `Λ{path.to.smth}` syntax
* Method calling in interpolation paths: `Λ{node.name()}`
* Repetition over any number of iterables: `⸨ ∀objects: ∀methods() ⸩,*`
* Repetitions in nested groups: `⸨ ∀methods(∀args); ⸩*`
* Nested repetitions: `⸨ ∀iter1 ⸨ ∀iter2: ∀iter3 ⸩* ∀iter4 ⸩*`
* Repetition and interpolation: `⸨ Λarg: [ ⸨ ∀values ⸩* ] ⸩,*`
* Force new line symbol: ⏎ (U+23CE)
* Force joint output (no spacing) modifier: ◡ (U+25E1), disable spacing: ◌ (U+25CC) and enable spacing: ○ (U+25CB)
* Automatic raw identifier conversion depending on the language (r# is added for Rust, r_ for Dart, etc)
* Force identifier (cancel raw) with: ȸ (U+0238) symbol before ident (`ȸtype` will produce `type`; `type` will
  produce `r#type`
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
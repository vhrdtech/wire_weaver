## Unreleased

### 🐛 Fixes

- `Bank` trait is `pub`: it is used from other crates (`ww_impl!(port[]: ww_gpio::Bank)`), which needs the trait
  itself public now that `#[ww_trait]` emits one for rustdoc. No wire change.

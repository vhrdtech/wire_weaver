## Unreleased

### 🚀 Features

- New crate: API snapshots of `ww_global` and `ww_stdlib` crates, embedded at build time. `get()` and `all()` look them
  up by crate name and version, `inline_skipped()` puts the definitions of skipped traits and types known from them
  back into a bundle, `files()` returns the embedded files as saved.

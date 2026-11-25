# book_outputs changelog

## Unreleased

### 🐛 Fixes

- `check` writes into the running test's repository, so a repository taking this crate by git keeps its own outputs (DOC-16).

### 🚀 Features

- `check` and `check_in`: a test's output under `docs/generated/`, compared or written with `WW_BOOK_OUTPUTS=write` (`just book-outputs`) (DOC-16).

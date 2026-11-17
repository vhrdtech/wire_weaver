# ww_book changelog

## Unreleased

### 🚀 Features

- `book_sources.toml` takes a git source (`git`, `branch`): pages from a checkout of the branch, fetched by `ww-book assemble`, which prints the commit.
- `features_page` names a source's tracker page under `into` (`features.md` by default), for two sources sharing one directory.
- `ww-book assemble` copies the files a page includes (`--8<-- "path"`, relative to its repository root) to `target/ww_book/snippets/<name>/` and points the include there.

# ww_book changelog

## Unreleased

### 🚀 Features

- `ww-book check` (`check::check`, `book_checks.toml`): fails a nav that steps down a level, a page without a level or listed twice, unwaived bare `rust` fences, and unincluded generated files (DOC-16).
- `ww-book assemble` writes `target/vz/sources.toml` (`write_sources_file`, `Repository::url`/`commit`): the repositories and pages it copied, so code links on copied pages resolve against their repositories (DOC-13).
- A tracker's links to files of its repository that are not pages point to them on the web at the locked commit (`Located::repository`).
- A source's `book` names its own book: its pages are not copied, only its tracker, whose links point into that book (DOC-12).
- `book_sources.toml` takes a git source (`git`, `branch`): pages from a checkout of the branch, fetched by `ww-book assemble`, which prints the commit.
- `features_page` names a source's tracker page under `into` (`features.md` by default), for two sources sharing one directory.
- `ww-book assemble` copies the files a page includes (`--8<-- "path"`, relative to its repository root) to `target/ww_book/snippets/<name>/` and points the include there.

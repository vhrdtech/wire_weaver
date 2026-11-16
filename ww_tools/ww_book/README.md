# ww_book

Assembles the WireWeaver book. Pages live with their code: a repository the framework takes by git (shrink_wrap now;
ww_base, ww_tape and ww_stdlib as they are cut) keeps its pages in its own `docs/`. `book_sources.toml` at the
repository root lists those repositories; `ww-book assemble` (`just book-assemble`, run by `just build-docs` and
`just serve-docs`) copies each one's pages from the checkout `Cargo.lock` pins into `docs/` and its feature tracker
next to them, so the book reads as one. The copies are not in git: each target directory gets a `.gitignore` naming
them. The Features page test reads the trackers through the same library.

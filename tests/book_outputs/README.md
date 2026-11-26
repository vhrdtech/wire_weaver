# book_outputs

Golden outputs of the book's examples. A test that shows a reader what an example prints (a transcript, bytes, a
chart's data) calls `book_outputs::check("example/name.txt", &text)`: the call fails when
`docs/generated/example/name.txt` differs from `text`, and a page includes the file with
`--8<-- "docs/generated/example/name.txt"`. `just book-outputs` writes every file again: it runs the tests whose name
holds `book_output` with `WW_BOOK_OUTPUTS=write`. `ww-book check` fails a generated file that no page includes.

The files go into the repository of the running test: the nearest directory above its crate with a `Cargo.lock` and
a `docs` directory. Another repository that takes this crate by git (ww_stdlib) keeps its outputs in its own
`docs/generated/`.

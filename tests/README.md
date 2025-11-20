# integration tests

Useful tips:

* Run tests with `--show-output`, to see debug output that might be useful in gaining understanding on how things work.
* Uncomment `debug_to_file` in `ww_api` invocations to generate a file with final AST and generated code
  (IDE macro expansion won't show AST, since it's printed in a comment).
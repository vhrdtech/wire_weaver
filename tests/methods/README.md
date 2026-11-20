# methods

Tests of `methods_api`'s client-server dispatch: one test per method shape (plain, user-defined types, deferred,
bit-sized arguments, fallible, context-visible seq/medium).

Runs the generated server and client against each other over a simulated link.

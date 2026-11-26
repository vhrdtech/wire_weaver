# blinky_api 0.1.1

The API of the blinky example device after its first change: `led_toggle` added at wire w1.1. It is
[`blinky_api`](../blinky_api) as its author would change it in place; the repository keeps it beside 0.1.0, under the
same package name and outside the workspace, so that old and new firmware and hosts can meet in one test
(`blinky_evolved`).

- `api_snapshots/` holds both versions' snapshots, as the crate's history would.
- `ww api check examples/blinky_api_evolved --against examples/blinky_api/api_snapshots/blinky_api_0_1_0.ron` says
  which version bumps the change needs.
- The book's chapter "Changing your API" walks through it.

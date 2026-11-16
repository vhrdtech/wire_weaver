"""`python -m wire_weaver.hub`: serves the ww hub (TOOL-18, CLT-39).

Not meant to be run by hand: `wire_weaver_client::hub::ensure_running` starts this module (`sys.executable -m
wire_weaver.hub`, registered by `wire_weaver._wire_weaver` at import) when it needs a hub and finds none newer, the
same way `ww_hub::main_if_hub()` lets a Rust program host it. Run `ww hub serve` to watch a hub's log by hand.
"""

import os
import sys

from . import _wire_weaver


def main() -> None:
    if len(sys.argv) > 1 and sys.argv[1] == _wire_weaver.VERSION_ARG:
        print(_wire_weaver.HUB_VERSION)
        return
    if _wire_weaver.SERVE_ENV not in os.environ:
        print(
            f"python -m wire_weaver.hub serves the ww hub, started by wire_weaver_client when it needs one "
            f"({_wire_weaver.SERVE_ENV} unset); run `ww hub serve` by hand instead",
            file=sys.stderr,
        )
        sys.exit(1)
    sys.exit(_wire_weaver.run_hub())


if __name__ == "__main__":
    main()

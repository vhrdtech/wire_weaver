"""Watches the thermometer's gain from Python: one update for each change, the script's own write and the
device lowering the gain after its amplifier clipped.

Run the simulated device first (`cargo run -p thermometer --example thermometer_sim`), then
`python observe.py`, or `python observe.py ws://ADDRESS:PORT/ww` for another address.
"""

import sys

import wire_weaver as ww


def main(url):
    dev = ww.connect(websocket_url=url)

    # --8<-- [start:observe]
    # dev.api is the API's root level, the level gain is on
    print("observable:", dev.api.observable_properties())
    # None: every change of gain, as soon as it happens
    with dev.api.observe({"gain": None}) as obs:
        print("update:", obs.recv(timeout=1.0)["values"])
        dev.gain.write(16)
        print("update:", obs.recv(timeout=1.0)["values"])
        # above gain 8 the simulated amplifier clips, and the device halves the gain
        dev.read_once()
        print("update:", obs.recv(timeout=1.0)["values"])
    # --8<-- [end:observe]

    dev.disconnect()


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "ws://127.0.0.1:8080/ww")

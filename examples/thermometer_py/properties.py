"""Reads and writes the thermometer's properties: the sampler's mode and the amplifier's gain.

Run the simulated device first (`cargo run -p thermometer --example thermometer_sim`), then
`python properties.py`, or `python properties.py ws://ADDRESS:PORT/ww` for another address.
"""

import sys

import wire_weaver as ww


def main(url):
    # --8<-- [start:connect]
    dev = ww.connect(websocket_url=url)
    # --8<-- [end:connect]

    # --8<-- [start:mode]
    print("mode:", dev.mode.read())
    dev.mode.write("Continuous")
    print("mode:", dev.mode.read())
    dev.mode.write("Idle")
    # --8<-- [end:mode]

    # --8<-- [start:gain]
    print("gain:", dev.gain.read())
    dev.gain.write(8)
    print("gain:", dev.gain.read())
    try:
        dev.gain.write(20)
    except ValueError as e:
        print("refused:", e)
    print("gain:", dev.gain.read())
    # --8<-- [end:gain]

    dev.disconnect()


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "ws://127.0.0.1:8080/ww")

"""What goes wrong from Python, and where: values Python refuses itself, and requests the device refuses.

Run the simulated device first (`cargo run -p thermometer --example thermometer_sim -- --still`), then
`python errors.py`, or `python errors.py ws://ADDRESS:PORT/ww` for another address.
"""

import sys

import wire_weaver as ww


def main(url):
    dev = ww.connect(websocket_url=url)

    # --8<-- [start:local]
    for call in [lambda: dev.set_rate("fast"), lambda: dev.gain.write(-1), lambda: dev.mode.write("Off")]:
        try:
            call()
        except (TypeError, ValueError) as e:
            print(type(e).__name__, e)
    # --8<-- [end:local]

    # --8<-- [start:device]
    try:
        dev.channel[7].enabled.read()
    except ww.WireWeaverError as e:
        print(type(e).__name__, e)
    # --8<-- [end:device]

    dev.disconnect()


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "ws://127.0.0.1:8080/ww")

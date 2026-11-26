"""Streams the thermometer's samples into Python and sends it commands.

Run the simulated device first (`cargo run -p thermometer --example thermometer_sim`), then
`python streams.py`, or `python streams.py ws://ADDRESS:PORT/ww` for another address.
"""

import sys

import wire_weaver as ww


def main(url):
    dev = ww.connect(websocket_url=url)

    # --8<-- [start:stream]
    with dev.samples as samples:
        dev.set_rate(ww.Quantity("5 Hz"))
        dev.mode.write("Continuous")
        for _, s in zip(range(4), samples):
            # a Seq and an IndexFrom1 come as a dict of their number
            print("sample", s["seq"]["seq"], "input", s["input"]["number"], "in", s["temp"].symbol)
        dev.mode.write("Idle")
        # --8<-- [end:stream]

        # --8<-- [start:sink]
        with dev.commands as commands:
            dev.mode.write("Triggered")
            commands.send({"Mark": {"seq": 1000}})
            commands.send("Trigger")
            s = next(samples)
            print("triggered: sample", s["seq"]["seq"], "input", s["input"]["number"])
        # --8<-- [end:sink]
        dev.mode.write("Idle")

    dev.disconnect()


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "ws://127.0.0.1:8080/ww")

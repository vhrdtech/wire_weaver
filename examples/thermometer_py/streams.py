"""Streams readings from the thermometer and sends it commands, from Python.

Run the simulated device first (`cargo run -p thermometer --example thermometer_sim -- --still`), then
`python streams.py`, or `python streams.py ws://ADDRESS:PORT/ww` for another address.
"""

import sys

import wire_weaver as ww


def main(url):
    dev = ww.connect(websocket_url=url)
    for n in [1, 2, 3]:
        dev.channel[n].enabled.write(False)

    # --8<-- [start:stream]
    dev.set_rate(5)
    with dev.samples as samples:
        dev.mode.write("Continuous")
        for _ in range(4):
            s = samples.recv(timeout=2)
            print(s["seq"]["seq"], s["temp"])
        dev.mode.write("Idle")
    # --8<-- [end:stream]

    # --8<-- [start:sink]
    dev.mode.write("Triggered")
    with dev.samples as samples, dev.commands as commands:
        commands.send({"Mark": {"seq": 100}})
        commands.send("Trigger")
        s = samples.recv(timeout=2)
        print(s["seq"]["seq"], s["temp"])
    dev.mode.write("Idle")
    # --8<-- [end:sink]

    dev.disconnect()


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "ws://127.0.0.1:8080/ww")

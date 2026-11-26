"""Reads and writes the thermometer's values from Python: a struct, numbers with units, an enum.

Run the simulated device first (`cargo run -p thermometer --example thermometer_sim -- --still`), then
`python values.py`, or `python values.py ws://ADDRESS:PORT/ww` for another address.
"""

import sys

import wire_weaver as ww


def main(url):
    dev = ww.connect(websocket_url=url)

    # --8<-- [start:struct]
    sample = dev.read_once()
    print(sample)
    print(sample["seq"]["seq"], sample["input"]["number"])
    # --8<-- [end:struct]

    # --8<-- [start:quantity]
    temp = sample["temp"]
    print(temp)
    print(float(temp), temp.value, temp.prefix, temp.origin)
    print(f"{temp.to_si():.2f} K")
    # --8<-- [end:quantity]

    # --8<-- [start:write]
    dev.set_rate(ww.Quantity("5 Hz"))
    dev.set_rate(ww.Quantity("0.02 kHz"))
    dev.set_rate(10)
    for rate in ["5 V", "5.5 Hz", "2 kHz"]:
        try:
            dev.set_rate(ww.Quantity(rate))
        except ValueError as e:
            print("refused:", e)
    # --8<-- [end:write]

    # --8<-- [start:enum]
    print(dev.indication.mode.read())
    dev.indication.mode.write("OnlyEssential")
    print(dev.indication.mode.read())
    # --8<-- [end:enum]

    dev.disconnect()


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "ws://127.0.0.1:8080/ww")

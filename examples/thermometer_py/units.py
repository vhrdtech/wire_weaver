"""Numbers with units from Python: the sampling rate as a quantity, the gain's bounds, and a temperature.

Run the simulated device first (`cargo run -p thermometer --example thermometer_sim`), then
`python units.py`, or `python units.py ws://ADDRESS:PORT/ww` for another address.
"""

import sys

import wire_weaver as ww


def main(url):
    dev = ww.connect(websocket_url=url)

    # --8<-- [start:rate]
    # a quantity in any prefix of the unit: 0.5 kHz is stored as 500 steps of 1 Hz
    dev.set_rate(ww.Quantity("0.5 kHz"))
    try:
        dev.set_rate(ww.Quantity("2 kHz"))
    except ValueError as e:
        print("refused:", e)
    # --8<-- [end:rate]

    # --8<-- [start:gain]
    gain = dev.gain.read()
    print("gain:", gain, "stored:", gain.value, "bounds:", gain.bounds)
    # --8<-- [end:gain]

    # --8<-- [start:temperature]
    sample = dev.read_once()
    temp = sample["temp"]
    print("symbol:", temp.symbol, "origin:", temp.origin, "unit:", temp.unit)
    # float() reads it in °C, to_si() in kelvin
    print("K minus °C:", round(temp.to_si() - float(temp), 2))
    # --8<-- [end:temperature]

    dev.disconnect()


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "ws://127.0.0.1:8080/ww")

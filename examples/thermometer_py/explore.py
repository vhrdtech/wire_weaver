"""Looks around the thermometer's API from Python: the tree the device describes, one signature, the inputs.

Run the simulated device first (`cargo run -p thermometer --example thermometer_sim -- --still`), then
`python explore.py`, or `python explore.py ws://ADDRESS:PORT/ww` for another address.
"""

import sys

import wire_weaver as ww


def main(url):
    # --8<-- [start:tree]
    dev = ww.connect(websocket_url=url)
    print(dev)
    # --8<-- [end:tree]

    # --8<-- [start:items]
    print(dev.set_rate.signature)
    print(dev.set_rate.doc)
    print(dev.channel.valid_indices())
    print(dev.channel[3].path)
    # --8<-- [end:items]

    dev.disconnect()


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "ws://127.0.0.1:8080/ww")

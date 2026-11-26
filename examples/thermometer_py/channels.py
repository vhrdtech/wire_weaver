"""The thermometer's inputs as an array of channels from Python, and its status LED.

Run the simulated device first (`cargo run -p thermometer --example thermometer_sim`), then
`python channels.py`, or `python channels.py ws://ADDRESS:PORT/ww` for another address.
"""

import sys

import wire_weaver as ww


def main(url):
    dev = ww.connect(websocket_url=url)

    # --8<-- [start:channels]
    print("valid indices:", dev.channel.valid_indices())
    for i in dev.channel.valid_indices():
        last = dev.channel[i].last()
        print(f"channel[{i}]: input", last["input"]["number"], "enabled", dev.channel[i].enabled.read())
    dev.channel[1].enabled.write(False)
    print("channel[1]: enabled", dev.channel[1].enabled.read())
    # --8<-- [end:channels]

    # --8<-- [start:indication]
    dev.indication.mode.write("OnlyEssential")
    print("indication:", dev.indication.mode.read())
    # --8<-- [end:indication]

    dev.disconnect()


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "ws://127.0.0.1:8080/ww")

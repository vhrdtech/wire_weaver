"""The bench supply's refusals from Python: the API's own error and a common error, both as `RemoteError`.

Run the simulated device first (`cargo run -p bench_supply --example bench_supply_sim`), then
`python errors.py`, or `python errors.py ws://ADDRESS:PORT/ww` for another address.
"""

import sys

import wire_weaver as ww


def main(url):
    dev = ww.connect(websocket_url=url)
    channel = dev.channel[0]

    # --8<-- [start:own]
    try:
        channel.set_output(True)
    except ww.RemoteError as e:
        print("value:", e.value)
        print("message:", e)
    # --8<-- [end:own]

    # --8<-- [start:fields]
    channel.voltage_limit.write(30_000)
    channel.current_limit.write(1_000)
    try:
        channel.set_output(True)
    except ww.RemoteError as e:
        print("would draw:", e.value["OverCurrent"]["would_draw"])
    # --8<-- [end:fields]

    # --8<-- [start:common]
    channel.voltage_limit.write(5_000)
    channel.set_output(True)
    try:
        channel.voltage_limit.write(6_000)
    except ww.RemoteError as e:
        print("value:", e.value)
        print("common:", e.common)
    # --8<-- [end:common]

    channel.set_output(False)
    dev.disconnect()


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "ws://127.0.0.1:8081/ww")

"""Calls the blinky device from Python: turns the LED on and off, then calls the methods with an argument and a
return value.

Run the simulated device first (`cargo run -p blinky --example blinky_sim`), then `python blinky.py`, or
`python blinky.py ws://ADDRESS:PORT/ww` for another address, or `python blinky.py usb` for a board over USB.
"""

import sys

import wire_weaver as ww


def main(where):
    # --8<-- [start:connect]
    if where == "usb":
        dev = ww.connect(vid_pid=(0xC0DE, 0xCAFE))
    else:
        dev = ww.connect(websocket_url=where)
    # --8<-- [end:connect]

    # --8<-- [start:tree]
    print(dev)
    # --8<-- [end:tree]

    # --8<-- [start:calls]
    dev.led_on()
    dev.led_off()
    print("was on:", dev.set_led(True))
    print("was on:", dev.set_led(on=False))
    print("blinks:", dev.blinks())
    # --8<-- [end:calls]

    # --8<-- [start:refused]
    try:
        dev.set_led("yes")
    except TypeError as e:
        print("refused:", e)
    # --8<-- [end:refused]

    dev.disconnect()


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "ws://127.0.0.1:8080/ww")

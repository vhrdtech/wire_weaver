from time import sleep

import blinky


def main():
    device = blinky.Device()
    device.connect()

    device.led_on()
    sleep(1.0)
    device.led_off()
    sleep(1.0)


if __name__ == "__main__":
    main()

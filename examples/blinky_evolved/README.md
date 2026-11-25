# blinky_evolved

The blinky example after its first change, `led_toggle` added at wire w1.1:

- `Blinky`, the host side of `blinky_api` 0.1.1 (`examples/blinky_api_evolved`);
- `device`, the firmware of 0.1.0 and of 0.1.1, simulated on a PC and served in process;
- `tests/it/book.rs`, which runs old and new hosts against old and new firmware and the checker on the change, for
  the book's chapter "Changing your API" (`docs/generated/blinky_evolved/`);
- the example `old_firmware`, which calls `led_toggle` on a board over USB that still runs 0.1.0.

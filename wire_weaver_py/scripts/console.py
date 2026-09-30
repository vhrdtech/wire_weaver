"""Interactive console with `wire_weaver` imported as `ww` and, when possible, a device connected as `dev`.

Run from the workspace root: `just py [options]` (builds the module first), or from wire_weaver_py:
`uv run python -i scripts/console.py [options]`. See `--help`.
"""

import argparse
import os
import sys

import wire_weaver as ww

os.chdir(os.environ.pop("WW_CWD", "."))  # `just py` runs in wire_weaver_py, paths are relative to where it was invoked

p = argparse.ArgumentParser(prog="just py", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
p.add_argument("--serial", help="USB serial number")
p.add_argument("--label", help="user label")
p.add_argument("--vid-pid", help="USB VID:PID in hex, e.g. c0de:cafe")
p.add_argument("--rtt", metavar="CHIP", help="connect over RTT, probe-rs chip name, e.g. STM32G0B1RETx")
p.add_argument("--elf", help="firmware ELF with the RTT control block address (with --rtt)")
p.add_argument("--api", metavar="PATH", help="API crate source, for devices without introspection (also works offline)")
p.add_argument("--trait", help="API trait name, if the crate has several (with --api)")
p.add_argument("--timeout", type=float, default=1.0, help="request timeout, seconds (default 1)")
p.add_argument("--no-connect", action="store_true", help="only import the module")
try:
    args = p.parse_args()
except SystemExit as e:  # --help or a bad argument: exit instead of dropping into the `python -i` prompt
    sys.stdout.flush()
    sys.stderr.flush()
    os._exit(e.code if isinstance(e.code, int) else 1)

api = ww.load_api(args.api, args.trait) if args.api else None
dev = None
if api is not None:
    print(f"api = ww.load_api({args.api!r}) - browse it, or encode/decode values offline")
if not args.no_connect:
    kw = {"timeout": args.timeout, "api": api}
    if args.rtt:
        kw.update(rtt=args.rtt, rtt_elf=args.elf)
    else:
        devices = ww.list_devices()
        print("devices:", *devices or ["(none)"], sep="\n  ")
        if args.vid_pid:
            vid, pid = args.vid_pid.split(":")
            kw["vid_pid"] = (int(vid, 16), int(pid, 16))
    if args.serial:
        kw["serial"] = args.serial
    if args.label:
        kw["user_label"] = args.label
    filtered = args.rtt or args.serial or args.label or args.vid_pid
    if filtered or len(devices) == 1:
        try:
            dev = ww.connect(**kw)
            print(f"dev = {dev.info}")
            print(dev)
        except Exception as e:  # stay in the console, the user can connect by hand
            print(f"connect failed: {e}")
    elif devices:
        print("several devices: pick one with --serial / --label, or dev = ww.connect(serial=...)")

names = ["`ww` is imported"] + (["`api` is loaded"] if api is not None else []) + (["`dev` is connected"] if dev else [])
print("\n" + ", ".join(names) + "; help(ww) lists what is there" + (", dev.disconnect() when done" if dev else ""))

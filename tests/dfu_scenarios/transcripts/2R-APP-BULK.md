```text
   ── device runs the app
   ── link up: dfu_app_api (link setup not shown, same on every link)
→ node.node()
         4 B  01 00 00 02
← { role: App, profile: AB(Remap), product: "sim-dfu", geometry_id: 4242, app_version: Some(1),
      version_floor: 1, manifest_digest: Some(1dd95effd1e00214), boot_state: Confirmed,
      last_result: { outcome: Nothing, version: 0 }, offers: { boot_control: true, inventory: true,
      stage: true, stage_bulk: true } }
        42 B  01 80 00 10 73 69 6d 2d 64 66 …
→ stage.limits()
         4 B  02 03 00 02
← { max_piece: 64, in_flight: 1, live: true, stored: false, plan_area: 0, writes_in_background: true }
        13 B  02 80 40 00 00 00 01 80 00 00 …
→ bulk.begin_bulk(target: Live, header: [35 B: 57 57 44 50 01 92 …], total_len: 359, ack_every: 0)
        49 B  03 04 00 00 57 57 44 50 01 92 …
← Ok({ steps: 11, resumed: false, resume_from: 0 })
        13 B  03 80 80 0b 00 00 00 00 00 00 …
→ bulk.plan_ack: StreamOpen { resume: None, args: tail(0:[]) }
         5 B  04 04 28 00 02
← ()
         6 B  04 80 00 00 00 00
→ bulk.plan: open
         4 B  05 04 16 02
← bulk.plan: credit: have 0 bytes, may send 2062 more
         7 B  00 83 41 50 e9 84 02
← written
         2 B  05 81
→ bulk.plan ⇐ { offset: 0, data: [64 B: 01 00 00 01 01 00 …] } (sink position 0)
        75 B  00 04 14 00 00 00 00 00 01 00 …
→ bulk.plan ⇐ { offset: 64, data: [64 B: e6 bd 84 3c 31 2f …] } (sink position 70)
        75 B  00 04 14 46 40 00 00 00 e6 bd …
→ bulk.plan ⇐ { offset: 128, data: [64 B: 28 29 f3 40 2e 64 …] } (sink position 140)
        76 B  00 04 14 81 0c 80 00 00 00 28 …
→ bulk.plan ⇐ { offset: 192, data: [64 B: 02 04 00 04 00 a8 …] } (sink position 210)
        76 B  00 04 14 81 52 c0 00 00 00 02 …
→ bulk.plan ⇐ { offset: 256, data: [64 B: e7 cd dc bd 9c c5 …] } (sink position 280)
        76 B  00 04 14 82 18 00 01 00 00 e7 …
→ bulk.plan ⇐ { offset: 320, data: [39 B: d6 1a dc e2 ce f7 …] } (sink position 350)
        50 B  00 04 14 82 5e 40 01 00 00 d6 …
← bulk.plan_ack ⇒ { have: 359, status: Complete }
         9 B  00 82 42 67 01 00 00 20 02
→ bulk.plan: credit query: sent 394 bytes
         6 B  06 04 16 6a 96 02
← bulk.plan: credit: have 394 bytes, may send 2062 more
         8 B  06 83 41 5a 96 e9 84 02
→ stage.finish()
         4 B  07 03 60 02
← Ok(Staged)
         3 B  07 80 82
→ boot.reboot(into: App)
         5 B  08 01 30 00 02
← Ok(())
         3 B  08 80 80
← Disconnect(RequestByUser)
         1 B  02
   ── device resets (into the app)
   ── bootloader switches to the staged image: maps bank 1 to the boot address, nothing is copied
   ── device runs the app
   ── link up: dfu_app_api (link setup not shown, same on every link)
→ node.node()
         4 B  01 00 00 02
← { role: App, profile: AB(Remap), product: "sim-dfu", geometry_id: 4242, app_version: Some(2),
      version_floor: 1, manifest_digest: Some(5673030c91c1ef0d), boot_state: Trial,
      last_result: { outcome: Nothing, version: 0 }, offers: { boot_control: true, inventory: true,
      stage: true, stage_bulk: true } }
        42 B  01 80 00 10 73 69 6d 2d 64 66 …
   ── the new image fails its own checks: gives up the trial instead of confirming
→ boot.revert()
         4 B  02 01 20 02
← Ok(())
         3 B  02 80 80
→ boot.reboot(into: App)
         5 B  03 01 30 00 02
← Ok(())
         3 B  03 80 80
← Disconnect(RequestByUser)
         1 B  02
   ── device resets (into the app)
   ── bootloader the trial image did not confirm: switches back: maps bank 0 to the boot address, nothing is copied
   ── device runs the app
   ── link up: dfu_app_api (link setup not shown, same on every link)
→ node.node()
         4 B  01 00 00 02
← { role: App, profile: AB(Remap), product: "sim-dfu", geometry_id: 4242, app_version: Some(1),
      version_floor: 1, manifest_digest: Some(1dd95effd1e00214), boot_state: Reverted,
      last_result: { outcome: Reverted, version: 2 }, offers: { boot_control: true, inventory: true,
      stage: true, stage_bulk: true } }
        42 B  01 80 00 10 73 69 6d 2d 64 66 …
```

<details markdown="1">
<summary>Wire bytes: 24 USB packets (high speed, 512 B max) (link setup and pings left out)</summary>

```text
   ── device runs the app
→    5 B  04 01 00 00 02
←   44 B  08 2a 01 80 00 10 73 69 6d 2d 64 66 75 92 10 00
          00 80 01 00 00 00 01 00 00 00 80 1d d9 5e ff d1
          e0 02 14 00 00 00 00 00 00 f0 01 57
→    5 B  04 02 03 00 02
←   15 B  08 0d 02 80 40 00 00 00 01 80 00 00 00 00 80
→   51 B  08 31 03 04 00 00 57 57 44 50 01 92 10 00 00 02
          00 00 00 0b 00 67 01 00 00 86 ba 52 71 00 2c c3
          5d c1 92 3c 26 b1 01 e5 86 67 01 00 00 00 00 00
          00 3c 02
←   15 B  08 0d 03 80 80 0b 00 00 00 00 00 00 00 00 19
→    6 B  05 04 04 28 00 02
←    7 B  06 04 80 00 00 00 00
→    5 B  04 05 04 16 02
←   11 B  07 00 83 41 50 e9 84 02 02 05 81
→  447 B  08 4b 00 04 14 00 00 00 00 00 01 00 00 01 01 00
          01 02 00 01 03 00 03 00 00 27 80 79 0d 97 ef cf
          f7 40 00 46 57 02 00 00 00 2f 86 5d 24 dc d1 cf
          67 47 9e 75 3c f4 e9 e7 7f 5f b6 8d 54 0c 01 ff
          97 77 ce a5 6c 24 19 17 af 8f 00 89 02 08 4b 00
          04 14 46 40 00 00 00 e6 bd 84 3c 31 2f c7 a7 fe
          d5 9c 54 49 47 df bf 16 ed b4 6c 61 5f f7 d7 2e
          02 01 00 01 00 3c e2 93 64 32 07 b8 7c 03 02 00
          ed cc 83 d5 b3 97 4a 90 40 00 61 70 70 20 76 32
          11 db 28 16 4c de 9f 00 89 02 08 4c 00 04 14 81
          0c 80 00 00 00 28 29 f3 40 2e 64 f6 b7 40 41 0b
          58 46 7c 0e cf 58 59 23 70 5e 94 26 e7 70 71 3b
          88 76 ac 3e ff 88 89 53 a0 8e c4 56 17 a0 a1 6b
          b8 a6 dc 6e 2f b8 b9 83 02 03 00 05 00 1e c7 0f
          b2 57 d1 be fc 00 89 02 08 4c 00 04 14 81 52 c0
          00 00 00 02 04 00 04 00 a8 09 c6 1b 42 94 29 60
          03 05 00 3b f9 a7 bd 49 c0 f8 87 40 00 6e 65 74
          20 76 32 87 6d 7c 5d 3c 65 75 24 9f 85 94 75 54
          7d 8d 3c b7 9d ac 8d 6c 95 a5 54 cf b5 c4 a5 84
          ad bd 6c 00 89 02 08 4c 00 04 14 82 18 00 01 00
          00 e7 cd dc bd 9c c5 d5 84 ff e5 f4 d5 b4 dd ed
          9c 17 fd 0c ed cc f5 05 b4 2f 15 03 06 00 d1 8f
          7c e6 80 78 12 57 40 00 6f 74 61 94 9a 86 af 33
          c8 a6 ea ac b2 9e c7 4b e0 be 02 c4 ca b6 df 63
          f8 00 89 02 08 32 00 04 14 82 5e 40 01 00 00 d6
          1a dc e2 ce f7 7b 10 ee 32 f4 fa e6 0f 93 28 06
          4a 0c 12 fe 27 ab 40 1e 62 24 2a 16 3f c3 58 36
          7a 3c 42 2e 57 db 7c 02 06 06 04 16 6a 96 02
←   21 B  08 09 00 82 42 67 01 00 00 20 02 08 08 06 83 41
          5a 96 e9 84 02
→    5 B  04 07 03 60 02
←    4 B  03 07 80 82
→    6 B  05 08 01 30 00 02
←    7 B  03 08 80 80 30 c1 02
   ── device resets (into the app)
   ── bootloader switches to the staged image: maps bank 1 to the boot address, nothing is copied
   ── device runs the app
→    5 B  04 01 00 00 02
←   44 B  08 2a 01 80 00 10 73 69 6d 2d 64 66 75 92 10 00
          00 80 02 00 00 00 01 00 00 00 80 56 73 03 0c 91
          c1 ef 0d 30 00 00 00 00 00 f0 01 57
   ── the new image fails its own checks: gives up the trial instead of confirming
→    5 B  04 02 01 20 02
←    4 B  03 02 80 80
→    6 B  05 03 01 30 00 02
←    7 B  03 03 80 80 30 c1 02
   ── device resets (into the app)
   ── bootloader the trial image did not confirm: switches back: maps bank 0 to the boot address, nothing is copied
   ── device runs the app
→    5 B  04 01 00 00 02
←   44 B  08 2a 01 80 00 10 73 69 6d 2d 64 66 75 92 10 00
          00 80 01 00 00 00 01 00 00 00 80 1d d9 5e ff d1
          e0 02 14 10 20 02 00 00 00 f0 01 57
```

</details>
